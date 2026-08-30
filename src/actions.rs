//! Shared actions: scan, copy, palette, external tools, and pure key-outcome builders.
use crate::app::{self, App};
use crate::commands::Outcome;
use crate::diff_tool::{self, ExternalDiffTool};
use crate::event::AppEvent;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::path::PathBuf;

/// Pure IO intent from a key press — built without performing IO.
/// [`dispatch_key_outcome`] performs the process spawn / terminal handoff.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome {
    /// No IO needed — the key was fully handled by pure state mutation (or ignored).
    None,
    LaunchDiff {
        tool: ExternalDiffTool,
        left: PathBuf,
        right: PathBuf,
    },
    LaunchEditor {
        path: PathBuf,
    },
}

/// Build the diff-launch intent for the currently selected row (the `D` key).
///
/// Validates tool availability immediately before handoff. The `Err` message is
/// the canonical failure text; Commands turns it into an outcome rather than
/// writing a toast from below the seam (Issue #282).
pub fn diff_launch_outcome(app: &App) -> Result<KeyOutcome, String> {
    let Some(row) = app.selected_row() else {
        return Ok(KeyOutcome::None);
    };
    if row.is_dir() || row.left.is_none() || row.right.is_none() {
        return Ok(KeyOutcome::None);
    }
    let tool = match &app.settings().external_diff_tool {
        crate::settings::DiffToolSetting::Disabled => {
            return Err("External diff is disabled".to_string());
        }
        crate::settings::DiffToolSetting::Auto => {
            let auto_tool = crate::diff_tool::SUPPORTED_TOOLS
                .iter()
                .find(|t| t.is_available())
                .copied();
            let Some(tool) = auto_tool else {
                return Err("No external diff tool is available".to_string());
            };
            tool
        }
        crate::settings::DiffToolSetting::Pinned(tool) => {
            if !tool.is_available() {
                return Err(format!("External diff tool '{}' not found", tool.as_str()));
            }
            *tool
        }
        crate::settings::DiffToolSetting::Unknown(name) => {
            return Err(format!("External diff tool '{name}' not found"));
        }
    };
    Ok(KeyOutcome::LaunchDiff {
        tool,
        left: app.left_path().join(row.left_relative_path()),
        right: app.right_path().join(row.right_relative_path()),
    })
}

/// Build the editor-launch intent for the active side's selected file (the `E` key).
pub fn editor_launch_outcome(app: &App) -> KeyOutcome {
    let Some(row) = app.selected_row() else {
        return KeyOutcome::None;
    };
    let side = if app.active_side_left() {
        &row.left
    } else {
        &row.right
    };
    if side.as_ref().is_none_or(|f| f.is_dir) {
        return KeyOutcome::None;
    }
    let (root, rel_path) = if app.active_side_left() {
        (app.left_path(), row.left_relative_path())
    } else {
        (app.right_path(), row.right_relative_path())
    };
    KeyOutcome::LaunchEditor {
        path: root.join(rel_path),
    }
}

/// Leaves raw mode + the alternate screen on construction (unless stdout isn't a real
/// terminal — see the "TTY recovery" invariant in AGENTS.md), and restores both on `Drop`.
/// Callers hold this across the external process and drop it **before** re-clearing the TUI.
pub(crate) struct RealTerminalHandoff {
    mouse_enabled: bool,
    is_terminal: bool,
}

impl RealTerminalHandoff {
    pub(crate) fn new(mouse_enabled: bool) -> std::io::Result<Self> {
        use std::io::IsTerminal;
        let is_terminal = std::io::stdout().is_terminal();
        if is_terminal {
            Self::suspend(mouse_enabled)?;
        }
        Ok(Self {
            mouse_enabled,
            is_terminal,
        })
    }

    fn suspend(mouse_enabled: bool) -> std::io::Result<()> {
        disable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            )
        } else {
            execute!(std::io::stdout(), LeaveAlternateScreen)
        }
    }

    fn resume(mouse_enabled: bool) -> std::io::Result<()> {
        enable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                crossterm::event::EnableMouseCapture
            )
        } else {
            execute!(std::io::stdout(), EnterAlternateScreen)
        }
    }
}

impl Drop for RealTerminalHandoff {
    fn drop(&mut self) {
        if !self.is_terminal {
            return;
        }
        // Drop can't propagate a Result, so a restore failure is reported rather than
        // silently swallowed — but there's nothing more this guard can do about it.
        if let Err(e) = Self::resume(self.mouse_enabled) {
            eprintln!("Failed to restore terminal after external process: {e}");
        }
    }
}

fn wait_for_enter() {
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}

/// Spawn an external diff tool. Caller must keep the TUI suspended (e.g. hold
/// [`RealTerminalHandoff`]) across this call and only clear after that guard drops.
pub(crate) fn run_external_diff(
    tool: &diff_tool::ExternalDiffTool,
    left: &std::path::Path,
    right: &std::path::Path,
) {
    match diff_tool::open_diff(tool, left, right) {
        Err(e) => {
            eprintln!(
                "Error launching external diff: {}. Press Enter to continue...",
                e
            );
            wait_for_enter();
        }
        Ok(()) if matches!(tool, diff_tool::ExternalDiffTool::Difftastic) => {
            println!("\nPress Enter to return to duodiff...");
            wait_for_enter();
        }
        Ok(()) => {}
    }
}

/// Spawn `$EDITOR`/`$VISUAL`. Same handoff contract as [`run_external_diff`].
pub(crate) fn run_external_editor(file_path: &std::path::Path) {
    if let Err(e) = diff_tool::open_editor(file_path) {
        eprintln!(
            "Error launching external editor: {}. Press Enter to continue...",
            e
        );
        wait_for_enter();
    }
}

/// Suspend the TUI, run `body`, restore the TUI, then clear the alt-screen buffer.
fn with_terminal_handoff<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
    body: impl FnOnce(),
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    {
        let _handoff = RealTerminalHandoff::new(mouse_enabled)?;
        body();
    }
    terminal.clear()?;
    Ok(())
}

/// Perform the IO a [`KeyOutcome`] describes. Pure key-handling code only builds a
/// `KeyOutcome`; process spawn and terminal mode toggling live here.
pub fn dispatch_key_outcome<B: ratatui::backend::Backend>(
    outcome: KeyOutcome,
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match outcome {
        KeyOutcome::None => Ok(()),
        KeyOutcome::LaunchDiff { tool, left, right } => {
            with_terminal_handoff(terminal, mouse_enabled, || {
                run_external_diff(&tool, &left, &right);
            })
        }
        KeyOutcome::LaunchEditor { path } => with_terminal_handoff(terminal, mouse_enabled, || {
            run_external_editor(&path);
        }),
    }
}

/// Run the work a confirmed dialog approved, returning its canonical outcome.
pub fn execute_confirm_action(
    app: &mut App,
    action: app::ConfirmAction,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    Ok(match action {
        app::ConfirmAction::Cancel => Outcome::Completed,
        app::ConfirmAction::SaveStaged => save_staged(app, false, tx),
        app::ConfirmAction::SaveStagedThenLeave => save_staged(app, true, tx),
        app::ConfirmAction::DiscardStagedThenLeave => {
            app.discard_staged();
            app.leave_file_diff();
            Outcome::Completed
        }
        app::ConfirmAction::ReloadDiscardStaged => match app.reload_discarding_staged() {
            Ok(()) => Outcome::Message {
                text: "Reloaded from disk; staged changes discarded".to_string(),
                is_error: false,
            },
            Err(e) => Outcome::Failed {
                message: format!("Reload failed: {e}"),
            },
        },
        direction @ (app::ConfirmAction::CopyLeftToRight | app::ConfirmAction::CopyRightToLeft) => {
            copy_confirmed_entry(app, direction, tx)
        }
    })
}

/// Write the staged buffers, then rescan the tree so the row states follow.
///
/// A conflict opens its own dialog instead of writing (`save_staged` returns
/// `Ok(false)`), and `then_leave` only returns to the tree once the write
/// actually succeeded (Issue #235).
fn save_staged(
    app: &mut App,
    then_leave: bool,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Outcome {
    match app.save_staged() {
        Ok(true) => {
            if then_leave {
                app.leave_file_diff();
            }
            kick_scan(app, tx);
            Outcome::Message {
                text: "Saved staged changes".to_string(),
                is_error: false,
            }
        }
        // A conflict dialog is already open; nothing was written.
        Ok(false) => Outcome::NeedsConfirmation,
        Err(e) => Outcome::Failed {
            message: format!("Save failed: {e}"),
        },
    }
}

fn copy_confirmed_entry(
    app: &mut App,
    direction: app::ConfirmAction,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Outcome {
    let Some(row) = app.selected_row() else {
        return Outcome::Completed;
    };
    if row.relative_path.as_os_str().is_empty() {
        return Outcome::Completed;
    }
    let relative_path = row.relative_path.clone();
    let left_to_right = direction == app::ConfirmAction::CopyLeftToRight;
    let name = if left_to_right {
        row.left_name().to_string()
    } else {
        row.right_name().to_string()
    };
    let src_rel = if left_to_right {
        row.left_relative_path()
    } else {
        row.right_relative_path()
    };
    let dst_rel = if left_to_right {
        row.right_relative_path()
    } else {
        row.left_relative_path()
    };
    let src = if left_to_right {
        app.left_path().join(src_rel)
    } else {
        app.right_path().join(src_rel)
    };
    let (dst, dst_root) = if left_to_right {
        (
            app.right_path().join(dst_rel),
            app.right_path().to_path_buf(),
        )
    } else {
        (app.left_path().join(dst_rel), app.left_path().to_path_buf())
    };

    // Directory copies walk the scan model, not the filesystem, so excluded
    // entries (`.git`, …) and files that appeared after the scan are never
    // copied implicitly (Issue #235).
    let res = match app.scanned_subtree_entries(&row.relative_path, left_to_right) {
        Some(entries) => copy_scanned_subtree(&src, &dst, &dst_root, &entries),
        None => copy_entry_checked(&src, &dst, &dst_root),
    };

    match res {
        Ok(()) => {
            app.leave_file_diff();
            // Prefer a targeted subtree re-align; fall back to full scan
            // for root-level copies or missing tree paths.
            let copied_is_dir = std::fs::symlink_metadata(&dst)
                .map(|m| {
                    let ft = m.file_type();
                    ft.is_dir() && !ft.is_symlink()
                })
                .unwrap_or(false);
            if app
                .apply_incremental_rescan(&relative_path, copied_is_dir)
                .is_err()
            {
                kick_scan(app, tx);
            }
            Outcome::Message {
                text: format!("Copied '{name}'"),
                is_error: false,
            }
        }
        Err(e) => Outcome::Failed {
            message: format!("Copy failed: {e}"),
        },
    }
}

/// Copy exactly the entries the scan listed under a directory, relative to it.
///
/// `entries` comes from the scan snapshot, so nothing hidden by an exclusion and
/// nothing created since the scan is copied. Existing destination entries that
/// the scan did not list are left in place — that is what makes it a merge.
fn copy_scanned_subtree(
    src_root: &std::path::Path,
    dst_root_dir: &std::path::Path,
    dst_root: &std::path::Path,
    entries: &[(std::path::PathBuf, bool)],
) -> std::io::Result<()> {
    if !path_is_under(dst_root_dir, dst_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "copy destination escapes the target root",
        ));
    }
    remove_destination_symlink(dst_root_dir)?;
    std::fs::create_dir_all(dst_root_dir)?;
    for (relative, is_dir) in entries {
        let src = src_root.join(relative);
        let dst = dst_root_dir.join(relative);
        if !path_is_under(&dst, dst_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "copy destination escapes the target root",
            ));
        }
        if *is_dir {
            remove_destination_symlink(&dst)?;
            std::fs::create_dir_all(&dst)?;
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let meta = std::fs::symlink_metadata(&src)?;
        if meta.file_type().is_symlink() {
            // Replace only this validated leaf; never follow or delete through it.
            remove_destination_symlink(&dst)?;
            recreate_symlink(&src, &dst)?;
        } else {
            remove_destination_symlink(&dst)?;
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub(crate) fn normalize_lexically(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out
}

pub(crate) fn path_is_under(path: &std::path::Path, root: &std::path::Path) -> bool {
    let path = normalize_lexically(path);
    let root = normalize_lexically(root);
    path.starts_with(&root)
}

/// Write every `(path, new_contents, original_contents)` triple, all-or-nothing.
///
/// Each file is staged into a sibling temporary file first, so nothing visible
/// changes until every write is known to have succeeded. If a replacement fails
/// part-way, the already-replaced files are restored from their originals rather
/// than leaving one side written and the other not (Issue #235).
pub(crate) fn commit_all_or_nothing(
    writes: &[(std::path::PathBuf, String, String)],
) -> std::io::Result<()> {
    use std::path::PathBuf;

    let mut temps: Vec<(PathBuf, PathBuf)> = Vec::new();
    let cleanup = |temps: &[(PathBuf, PathBuf)]| {
        for (temp, _) in temps {
            let _ = std::fs::remove_file(temp);
        }
    };

    for (path, contents, _) in writes {
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".to_string());
        let temp = parent.join(format!(".{file_name}.duodiff-tmp"));
        if let Err(e) =
            std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&temp, contents))
        {
            cleanup(&temps);
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
        temps.push((temp, path.clone()));
    }

    let mut replaced: Vec<usize> = Vec::new();
    for (i, (temp, path)) in temps.iter().enumerate() {
        if let Err(e) = std::fs::rename(temp, path) {
            // Put back whatever already moved, then drop the remaining temps.
            for &done in &replaced {
                let _ = std::fs::write(&temps[done].1, &writes[done].2);
            }
            cleanup(&temps[i..]);
            return Err(e);
        }
        replaced.push(i);
    }
    Ok(())
}

pub(crate) fn copy_entry_checked(
    src: &std::path::Path,
    dst: &std::path::Path,
    dst_root: &std::path::Path,
) -> std::io::Result<()> {
    if !path_is_under(dst, dst_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "copy destination escapes the target root",
        ));
    }

    let meta = std::fs::symlink_metadata(src)?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        remove_destination_symlink(dst)?;
        recreate_symlink(src, dst)
    } else if file_type.is_dir() {
        copy_dir_recursive(src, dst, dst_root)
    } else if file_type.is_file() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        remove_destination_symlink(dst)?;
        std::fs::copy(src, dst).map(|_| ())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Source path not found on disk",
        ))
    }
}

fn recreate_symlink(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    let target = std::fs::read_link(src)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, dst)
    }
    #[cfg(windows)]
    {
        // Prefer recreating the link; Windows may require elevated privileges.
        let meta = std::fs::symlink_metadata(src)?;
        // `is_dir` on symlink metadata reports the *target* type on Windows.
        if meta.file_type().is_dir() {
            std::os::windows::fs::symlink_dir(target, dst)
        } else {
            std::os::windows::fs::symlink_file(target, dst)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (src, dst, target);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "symlink copy is not supported on this platform",
        ))
    }
}

/// Replace a destination link itself, never the file or directory it points to.
fn remove_destination_symlink(dst: &std::path::Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(dst) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(dst),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub(crate) fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
    dst_root: &std::path::Path,
) -> std::io::Result<()> {
    if !path_is_under(dst, dst_root) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "copy destination escapes the target root",
        ));
    }
    remove_destination_symlink(dst)?;
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if !path_is_under(&dst_path, dst_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "copy destination escapes the target root",
            ));
        }
        if file_type.is_symlink() {
            remove_destination_symlink(&dst_path)?;
            recreate_symlink(&src_path, &dst_path)?;
        } else if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, dst_root)?;
        } else {
            remove_destination_symlink(&dst_path)?;
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn kick_scan(app: &mut App, tx: tokio::sync::mpsc::Sender<AppEvent>) {
    let generation = app.begin_scan();
    start_scan_task(
        app.left_path().to_path_buf(),
        app.right_path().to_path_buf(),
        app.precise_mode(),
        app.ignore_matchers().0.clone(),
        app.ignore_matchers().1.clone(),
        generation,
        tx,
    );
}

pub fn start_scan_task(
    left: PathBuf,
    right: PathBuf,
    precise: bool,
    mut left_ignore: crate::ignore::IgnoreMatcher,
    mut right_ignore: crate::ignore::IgnoreMatcher,
    generation: u64,
    tx: tokio::sync::mpsc::Sender<crate::event::AppEvent>,
) {
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<usize>(100);
    let app_tx = tx.clone();
    tokio::spawn(async move {
        while let Some(count) = prog_rx.recv().await {
            if app_tx
                .send(crate::event::AppEvent::ScanProgress { generation, count })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let root = tokio::task::spawn_blocking(move || {
            let mut on_progress = |count: usize| {
                let _ = prog_tx.try_send(count);
            };
            crate::diff::align_directories_with_matchers_and_progress(
                &left,
                &right,
                std::path::Path::new(""),
                precise,
                &mut left_ignore,
                &mut right_ignore,
                &mut on_progress,
            )
        })
        .await;

        match root {
            Ok(Ok(node)) => {
                let _ = tx
                    .send(crate::event::AppEvent::ScanFinished {
                        generation,
                        node: Box::new(node),
                    })
                    .await;
            }
            Ok(Err(err)) => {
                let _ = tx
                    .send(crate::event::AppEvent::Error {
                        generation,
                        message: err.to_string(),
                    })
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(crate::event::AppEvent::Error {
                        generation,
                        message: err.to_string(),
                    })
                    .await;
            }
        }
    });
}

/// Hand the project repository URL to the platform browser launcher.
///
/// The launcher runs on its own thread because `xdg-open` can block for as long
/// as the browser lives, so a failure cannot be part of the synchronous
/// [`Outcome`]. It is reported through [`AppEvent::CommandFailed`] instead of
/// being dropped (Issue #282).
pub fn open_repo_url(tx: tokio::sync::mpsc::Sender<AppEvent>) {
    let url = env!("CARGO_PKG_REPOSITORY");
    std::thread::spawn(move || {
        let status = match std::env::consts::OS {
            "macos" => std::process::Command::new("open").arg(url).status(),
            "windows" => std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .status(),
            _ => std::process::Command::new("xdg-open").arg(url).status(),
        };
        if let Some(message) = repo_launch_failure(status) {
            let _ = tx.blocking_send(AppEvent::CommandFailed { message });
        }
    });
}

/// The canonical failure text for a browser launch, or `None` when it worked.
fn repo_launch_failure(status: std::io::Result<std::process::ExitStatus>) -> Option<String> {
    match status {
        Ok(status) if status.success() => None,
        Ok(_) => Some("Cannot open the repository page: the browser launcher failed".to_string()),
        Err(error) => Some(format!("Cannot open the repository page: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::FlatRow;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{lock_env_tests, RecordingTerminalHandoff};
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::SystemTime;

    // `run_external_diff` isn't exercised directly here: `ExternalDiffTool::as_str()`
    // names a real GUI/CLI tool binary with no env-var override (unlike `$EDITOR`), so
    // there's no fast, portable way to make it succeed in CI. The editor path covers the
    // same handoff scope pattern used by `dispatch_key_outcome` (guard around the spawn).
    #[test]
    fn test_run_external_editor_suspends_then_resumes_around_the_spawn() {
        let _guard = lock_env_tests();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let log = Rc::new(RefCell::new(Vec::new()));
        {
            let _handoff = RecordingTerminalHandoff::new(log.clone());
            run_external_editor(std::path::Path::new("dummy.txt"));
        }
        assert_eq!(*log.borrow(), vec!["suspend", "resume"]);
    }

    #[test]
    fn test_terminal_handoff_resumes_even_if_the_scope_panics() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let log_for_closure = log.clone();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _handoff = RecordingTerminalHandoff::new(log_for_closure);
            panic!("simulated failure mid external-process handoff");
        }));

        assert!(result.is_err());
        assert_eq!(*log.borrow(), vec!["suspend", "resume"]);
    }

    fn file_row(name: &str, left: bool, right: bool, is_dir: bool) -> FlatRow {
        let info = FileInfo {
            is_dir,
            size: 10,
            modified: SystemTime::UNIX_EPOCH,
        };
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from(name),
            name: name.to_string(),
            state: DiffState::DifferentNewerLeft,
            left: left.then_some(info.clone()),
            right: right.then_some(info),
            ..Default::default()
        }
    }

    #[test]
    fn diff_launch_outcome_none_when_disabled() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(
            diff_launch_outcome(&app),
            Err("External diff is disabled".to_string())
        );
    }

    #[test]
    fn diff_launch_outcome_none_for_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.filter_mut()
            .set_rows(vec![file_row("dir", true, true, true)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), Ok(KeyOutcome::None));
    }

    #[test]
    fn diff_launch_outcome_none_for_single_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), Ok(KeyOutcome::None));
    }

    #[test]
    fn diff_launch_outcome_pinned_disappeared_stays_in_tui_with_a_failure_message() {
        let _guard = crate::test_support::PathEnvGuard::set("/nonexistent_dir_123");
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Meld,
        ));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);

        assert_eq!(
            diff_launch_outcome(&app),
            Err("External diff tool 'meld' not found".to_string())
        );
    }

    #[test]
    fn diff_launch_outcome_builds_paths_for_both_sided_file_when_available() {
        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        #[cfg(windows)]
        let vim_exe = bin_dir.join("vim.exe");
        #[cfg(not(windows))]
        let vim_exe = bin_dir.join("vim");
        std::fs::write(&vim_exe, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&vim_exe).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&vim_exe, perms).unwrap();
        }

        let _guard = crate::test_support::PathEnvGuard::set(&bin_dir);

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(
            diff_launch_outcome(&app),
            Ok(KeyOutcome::LaunchDiff {
                tool: ExternalDiffTool::Vim,
                left: PathBuf::from("/left/a.txt"),
                right: PathBuf::from("/right/a.txt"),
            })
        );
    }

    /// The browser launch outlives `execute`, so its failure has to reach the
    /// user through an event rather than being dropped (Issue #282).
    #[test]
    fn repo_launch_failure_reports_a_failed_spawn_and_a_failed_exit() {
        assert_eq!(
            repo_launch_failure(Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "xdg-open not found"
            ))),
            Some("Cannot open the repository page: xdg-open not found".to_string())
        );

        let failed = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--definitely-not-a-flag")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        assert_eq!(
            repo_launch_failure(failed),
            Some("Cannot open the repository page: the browser launcher failed".to_string())
        );
    }

    #[test]
    fn repo_launch_failure_is_silent_when_the_launcher_succeeds() {
        // `--list` makes libtest print the test names and exit 0, which gives a
        // successful child without depending on anything on `PATH`.
        let ok = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        assert_eq!(repo_launch_failure(ok), None);
    }

    #[test]
    fn editor_launch_outcome_none_for_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_left_pane();
        app.filter_mut()
            .set_rows(vec![file_row("dir", true, false, true)]);
        app.set_selected_idx(0);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn editor_launch_outcome_follows_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);

        app.focus_left_pane();
        assert_eq!(
            editor_launch_outcome(&app),
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("/left/a.txt"),
            }
        );

        app.focus_right_pane();
        assert_eq!(
            editor_launch_outcome(&app),
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("/right/a.txt"),
            }
        );
    }

    #[test]
    fn editor_launch_outcome_none_when_missing_on_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_right_pane();
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.set_selected_idx(0);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn outcomes_are_none_when_selection_out_of_range() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(diff_launch_outcome(&app), Ok(KeyOutcome::None));
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn test_scanned_directory_copy_skips_entries_absent_from_the_snapshot() {
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        let source = left.path().join("project");
        std::fs::create_dir_all(source.join(".git")).unwrap();
        std::fs::write(source.join("visible.txt"), "copy me").unwrap();
        std::fs::write(source.join(".git/config"), "do not copy").unwrap();

        copy_scanned_subtree(
            &source,
            &right.path().join("project"),
            right.path(),
            &[(PathBuf::from("visible.txt"), false)],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(right.path().join("project/visible.txt")).unwrap(),
            "copy me"
        );
        assert!(
            !right.path().join("project/.git").exists(),
            "excluded .git must not be copied merely because it exists on disk"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_replaces_destination_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("safe.txt");
        let destination = destination_dir.path().join("safe.txt");
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&source, "replacement").unwrap();
        std::fs::write(&outside_file, "must survive").unwrap();
        symlink(&outside_file, &destination).unwrap();

        copy_entry_checked(&source, &destination, destination_dir.path()).unwrap();

        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "replacement"
        );
        assert!(destination
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_file());
        assert_eq!(
            std::fs::read_to_string(outside_file).unwrap(),
            "must survive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_scanned_copy_replaces_destination_root_symlink_without_walking_it() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("project");
        let destination = destination_dir.path().join("project");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("visible.txt"), "safe copy").unwrap();
        symlink(outside.path(), &destination).unwrap();

        copy_scanned_subtree(
            &source,
            &destination,
            destination_dir.path(),
            &[(PathBuf::from("visible.txt"), false)],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("visible.txt")).unwrap(),
            "safe copy"
        );
        assert!(
            !outside.path().join("visible.txt").exists(),
            "a destination root symlink must be replaced, not followed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_scanned_copy_replaces_destination_directory_symlink_without_walking_it() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let source_dir = tempdir().unwrap();
        let destination_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let source = source_dir.path().join("project");
        let destination = destination_dir.path().join("project");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("nested/visible.txt"), "safe copy").unwrap();
        symlink(outside.path(), destination.join("nested")).unwrap();

        copy_scanned_subtree(
            &source,
            &destination,
            destination_dir.path(),
            &[
                (PathBuf::from("nested"), true),
                (PathBuf::from("nested/visible.txt"), false),
            ],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(destination.join("nested/visible.txt")).unwrap(),
            "safe copy"
        );
        assert!(
            !outside.path().join("visible.txt").exists(),
            "a destination directory symlink must be replaced, not followed"
        );
    }

    /// Issue #238: the Palette runs the same atomic flow as the `c` key —
    /// persist, adopt, and start exactly one background rescan.
    ///
    /// Synchronous so `ConfigEnvGuard` stays live for the whole test; tokio
    /// drop-tracking can drop an unused `_guard` before `.await`.
    #[test]
    fn test_palette_toggle_scan_persists_and_starts_exactly_one_rescan() {
        use crate::settings::ScanMode;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // The seeded config persists Precise, so the toggle lands on Fast.
        assert_eq!(app.scan_mode(), ScanMode::Precise);
        let before = app.scan_generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let action = crate::commands::CommandEntry {
            key: "c".to_string(),
            label: "Toggle Scan Mode".to_string(),
            action_id: crate::commands::Command::ToggleScan,
            disabled_reason: None,
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                crate::commands::Commands::new(tx)
                    .execute(
                        &mut app,
                        crate::commands::Invocation::Command(action.action_id),
                        &mut terminal,
                    )
                    .unwrap();
            });

        assert_eq!(app.scan_mode(), ScanMode::Fast);
        assert_eq!(
            crate::settings::AppSettings::load().scan_mode,
            ScanMode::Fast,
            "the palette persists the new mode"
        );
        assert_eq!(
            app.scan_generation(),
            before + 1,
            "exactly one background rescan"
        );
    }
}
