//! Command effects and the seams they run through: copy, external tools, and
//! the pure key-outcome builders.
//!
//! This module holds the effect implementations grouped by concept; `commands`
//! is their only caller and the one external interface (ADR-0003). Nothing here
//! composes a user-visible sentence: an effect reports what it did as data and
//! `commands` names it, except for work that outlives the synchronous call and
//! reports through [`AppEvent`].
use crate::app::{self, App};
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
    LaunchDiff {
        tool: ExternalDiffTool,
        left: PathBuf,
        right: PathBuf,
    },
    LaunchEditor {
        path: PathBuf,
    },
}

/// Suspends the TUI while an external process owns the terminal, and restores it
/// when released.
///
/// The seam the "TTY recovery" invariant lives on: [`with_terminal_handoff`] is
/// generic over this, so a test can substitute a recorder and assert the guard
/// really wraps the spawn (Issue #304).
pub(crate) trait TerminalGuard: Sized {
    /// Suspend the TUI. Restoring happens on `Drop`.
    fn acquire(mouse_enabled: bool) -> std::io::Result<Self>;

    /// Turn mouse capture on or off while the TUI owns the terminal.
    fn set_mouse_capture(on: bool) -> std::io::Result<()>;
}

/// Leaves raw mode + the alternate screen on construction (unless stdout isn't a real
/// terminal — see the "TTY recovery" invariant in docs/agents/tui.md), and restores both
/// on `Drop`.
/// Callers hold this across the external process and drop it **before** re-clearing the TUI.
pub(crate) struct RealTerminalGuard {
    mouse_enabled: bool,
    is_terminal: bool,
}

impl TerminalGuard for RealTerminalGuard {
    fn acquire(mouse_enabled: bool) -> std::io::Result<Self> {
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

    fn set_mouse_capture(on: bool) -> std::io::Result<()> {
        use std::io::IsTerminal;
        if !std::io::stdout().is_terminal() {
            return Ok(());
        }
        if on {
            execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
        } else {
            execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)
        }
    }
}

impl RealTerminalGuard {
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

impl Drop for RealTerminalGuard {
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
/// [`RealTerminalGuard`]) across this call and only clear after that guard drops.
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
fn with_terminal_handoff<B: ratatui::backend::Backend, G: TerminalGuard>(
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
    body: impl FnOnce(),
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    {
        let _guard = G::acquire(mouse_enabled)?;
        body();
    }
    // Only after the guard is released: clearing while the external process still
    // owns the terminal would paint into its screen.
    terminal.clear()?;
    Ok(())
}

/// Perform the IO a [`KeyOutcome`] describes. Pure key-handling code only builds a
/// `KeyOutcome`; process spawn and terminal mode toggling live here.
pub(crate) fn dispatch_key_outcome<B: ratatui::backend::Backend, G: TerminalGuard>(
    outcome: KeyOutcome,
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match outcome {
        KeyOutcome::LaunchDiff { tool, left, right } => {
            with_terminal_handoff::<B, G>(terminal, mouse_enabled, || {
                run_external_diff(&tool, &left, &right);
            })
        }
        KeyOutcome::LaunchEditor { path } => {
            with_terminal_handoff::<B, G>(terminal, mouse_enabled, || {
                run_external_editor(&path);
            })
        }
    }
}

/// What running a confirmed action actually did.
///
/// Facts, not sentences: `commands` names each of these for the user, and a
/// conflict comes back with its paths so `commands` can raise the follow-up
/// dialog rather than this module writing one (Issue #284).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmEffect {
    Nothing,
    Saved,
    /// Nothing written: these absolute paths changed on disk underneath us.
    SaveConflicted(Vec<std::path::PathBuf>),
    SaveFailed(String),
    Reloaded,
    ReloadFailed(String),
    /// The named entry landed on the other side.
    Copied(String),
    CopyFailed(String),
}

/// Run the work a confirmed dialog approved.
pub(crate) fn execute_confirm_action(
    app: &mut App,
    action: app::ConfirmAction,
) -> Result<ConfirmEffect, Box<dyn std::error::Error>> {
    Ok(match action {
        app::ConfirmAction::Cancel => ConfirmEffect::Nothing,
        app::ConfirmAction::SaveStaged => save_staged(app, false),
        app::ConfirmAction::SaveStagedThenLeave => save_staged(app, true),
        app::ConfirmAction::DiscardStagedThenLeave => {
            app.discard_staged();
            app.leave_file_diff();
            ConfirmEffect::Nothing
        }
        app::ConfirmAction::ReloadDiscardStaged => match app.reload_discarding_staged() {
            Ok(()) => ConfirmEffect::Reloaded,
            Err(e) => ConfirmEffect::ReloadFailed(e),
        },
        // A confirmed copy runs its plan through `copy_planned`, which
        // `commands` calls with the plan the answer matched.
        app::ConfirmAction::CopyLeftToRight | app::ConfirmAction::CopyRightToLeft => {
            ConfirmEffect::Nothing
        }
    })
}

/// Write the staged buffers, then rescan the tree so the row states follow.
///
/// A conflict writes nothing and comes back with the paths that moved, and
/// `then_leave` only returns to the tree once the write actually succeeded
/// (Issue #235).
fn save_staged(app: &mut App, then_leave: bool) -> ConfirmEffect {
    match app.save_staged() {
        Ok(app::StagedSave::Written) => {
            if then_leave {
                app.leave_file_diff();
            }
            app.request_rescan();
            ConfirmEffect::Saved
        }
        Ok(app::StagedSave::Conflicted(paths)) => ConfirmEffect::SaveConflicted(paths),
        Err(e) => ConfirmEffect::SaveFailed(e.to_string()),
    }
}

/// Run a copy the user confirmed, exactly as planned: the plan already holds
/// every precondition, so nothing here checks one again (ADR-0003).
pub(crate) fn copy_planned(app: &mut App, plan: &app::CopyPlan) -> ConfirmEffect {
    let left_to_right = plan.direction == app::CopyDirection::LeftToRight;
    let app::CopyTarget::Entry {
        relative_path,
        source_name: name,
        source: src,
        destination: dst,
        destination_root: dst_root,
        ..
    } = &plan.target
    else {
        return copy_within_file_pair(app, left_to_right);
    };
    let (relative_path, name, src, dst, dst_root) = (
        relative_path.clone(),
        name.clone(),
        src.clone(),
        dst.clone(),
        dst_root.clone(),
    );

    // Directory copies walk the scan model, not the filesystem, so excluded
    // entries (`.git`, …) and files that appeared after the scan are never
    // copied implicitly (Issue #235).
    let res = match app.scanned_subtree_entries(&relative_path, left_to_right) {
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
                app.request_rescan();
            }
            ConfirmEffect::Copied(name)
        }
        Err(e) => ConfirmEffect::CopyFailed(e.to_string()),
    }
}

/// Replace one side of a file pair with the other and reload the pair, staying
/// on File Diff: there is no tree to return to (Issue #327).
///
/// A side read from a pipe cannot be read again, so its captured bytes are
/// written instead of copying the path.
fn copy_within_file_pair(app: &mut App, left_to_right: bool) -> ConfirmEffect {
    let Some(pair) = app.file_pair() else {
        // A file-pair plan is only ever built in a file-pair session.
        return ConfirmEffect::Nothing;
    };
    let (source, destination) = (pair.side(left_to_right), pair.side(!left_to_right));
    let name = source.name();
    let written = match source.captured_bytes() {
        Some(bytes) => std::fs::write(destination.target_path(), bytes),
        None => std::fs::copy(source.target_path(), destination.target_path()).map(|_| ()),
    };
    if let Err(e) = written {
        return ConfirmEffect::CopyFailed(e.to_string());
    }
    match app.refresh_file_diff() {
        Ok(()) => ConfirmEffect::Copied(name),
        Err(e) => ConfirmEffect::ReloadFailed(e),
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

/// Carry out the work changes left for the event loop.
pub(crate) fn run_requests<G: TerminalGuard>(
    app: &mut App,
    tx: &tokio::sync::mpsc::Sender<AppEvent>,
) {
    for request in app.take_requests() {
        match request {
            app::Request::Rescan => crate::scan::start(app, tx.clone()),
            app::Request::MouseCapture(on) => {
                if let Err(error) = G::set_mouse_capture(on) {
                    app.mouse_capture_failed(on, error);
                }
            }
        }
    }
}

/// Hand the project repository URL to the platform browser launcher.
///
/// The launcher runs on its own thread because `xdg-open` can block for as long
/// as the browser lives, so a failure cannot be part of the synchronous
/// [`Outcome`]. It is reported through [`AppEvent::CommandFailed`] instead of
/// being dropped (Issue #282).
pub(crate) fn open_repo_url(tx: tokio::sync::mpsc::Sender<AppEvent>) {
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
    use crate::test_support::{lock_env_tests, RecordingBackend, RecordingTerminalGuard};
    use std::path::PathBuf;
    use std::time::SystemTime;

    /// A terminal showing `text`, backed by the recorder so the handoff's
    /// `clear` lands in the same log the guard writes to.
    fn terminal_showing(text: &str) -> ratatui::Terminal<RecordingBackend> {
        let mut terminal = ratatui::Terminal::new(RecordingBackend::new(20, 2)).unwrap();
        terminal
            .draw(|f| f.render_widget(ratatui::widgets::Paragraph::new(text), f.area()))
            .unwrap();
        terminal
    }

    /// The guard must wrap the spawn, and the screen must be cleared only after it
    /// is released — the "TTY recovery" and "Editor handoff" invariants (#304).
    ///
    /// Only the editor launch is driven end to end: `run_external_diff` waits on
    /// stdin when the tool fails, and `ExternalDiffTool::as_str()` names a real
    /// binary with no env-var override (unlike `$EDITOR`), so there is no fast,
    /// portable way to make it succeed in CI. Both launches go through
    /// `with_terminal_handoff`, so what stays uncovered is one match arm.
    #[test]
    fn test_dispatch_wraps_the_editor_spawn_in_the_terminal_guard() {
        let _lock = lock_env_tests();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");
        assert!(terminal.backend().rendered().contains("leftovers"));

        dispatch_key_outcome::<_, RecordingTerminalGuard>(
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("dummy.txt"),
            },
            &mut terminal,
            true,
        )
        .unwrap();

        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=true)".to_string(),
                "resume(mouse_enabled=true)".to_string(),
                "clear".to_string(),
            ],
            "the spawn must happen inside the guard, with mouse capture released,
             and the screen cleared only once the guard has let go"
        );
        assert!(
            !terminal.backend().rendered().contains("leftovers"),
            "the screen must be cleared once the external program is done"
        );
    }

    /// The spawn must happen *between* suspend and resume, not beside them.
    ///
    /// Records the body into the same log the guard writes, so a handoff that
    /// released the guard before running the body fails here rather than passing
    /// on a log that looks the same from outside (#304).
    #[test]
    fn test_the_guard_wraps_the_spawn_rather_than_bracketing_it() {
        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");

        with_terminal_handoff::<_, RecordingTerminalGuard>(&mut terminal, true, || {
            RecordingTerminalGuard::record("spawn".to_string());
        })
        .unwrap();

        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=true)".to_string(),
                "spawn".to_string(),
                "resume(mouse_enabled=true)".to_string(),
                "clear".to_string(),
            ]
        );
    }

    /// A crash inside the external program must not cost the user their terminal.
    ///
    /// Driven through `with_terminal_handoff` rather than the dispatcher: the
    /// dispatcher builds its body from the outcome, so a panic cannot be injected
    /// there, and this is production code too — not a stand-in.
    #[test]
    fn test_terminal_guard_resumes_even_if_the_spawn_panics() {
        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_terminal_handoff::<_, RecordingTerminalGuard>(&mut terminal, false, || {
                panic!("simulated failure mid external-process handoff");
            })
        }));

        assert!(result.is_err());
        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=false)".to_string(),
                "resume(mouse_enabled=false)".to_string(),
            ],
            "the guard must restore the terminal while unwinding"
        );
        assert!(
            terminal.backend().rendered().contains("leftovers"),
            "the clear sits after the guard, so unwinding skips it"
        );
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
    fn plan_external_diff_refuses_when_disabled() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::Disabled));
    }

    #[test]
    fn plan_external_diff_refuses_a_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("dir", true, true, true)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
    }

    #[test]
    fn plan_external_diff_refuses_a_single_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
    }

    #[test]
    /// The tool list is the one detected at startup, which the gate and the
    /// launch both read, so a pinned tool missing then is refused up front.
    fn plan_external_diff_refuses_a_pinned_tool_missing_at_startup() {
        let _guard = crate::test_support::PathEnvGuard::set("/nonexistent_dir_123");
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Meld,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);

        assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::ToolMissing));
    }

    #[test]
    fn plan_external_diff_builds_paths_for_both_sided_file_when_available() {
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

        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                detected_diff_tools: crate::diff_tool::detect_diff_tools(),
                ..crate::startup::Startup::for_test()
            },
        );
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
            ExternalDiffTool::Vim,
        ));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(
            app.plan_external_diff(),
            Ok(app::DiffPlan {
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
    fn plan_editor_refuses_a_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_left_pane();
        app.directory_tree_mut()
            .set_rows(vec![file_row("dir", true, false, true)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_editor(), None);
    }

    #[test]
    fn plan_editor_follows_active_side() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.directory_tree_mut().set_selected_idx(0);

        app.focus_left_pane();
        assert_eq!(app.plan_editor(), Some(PathBuf::from("/left/a.txt")));

        app.focus_right_pane();
        assert_eq!(app.plan_editor(), Some(PathBuf::from("/right/a.txt")));
    }

    #[test]
    fn plan_editor_refuses_a_side_with_no_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.focus_right_pane();
        app.directory_tree_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.plan_editor(), None);
    }

    #[test]
    fn plans_refuse_when_nothing_is_selected() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(
            app.plan_external_diff(),
            Err(app::DiffRefusal::NotBothFiles)
        );
        assert_eq!(app.plan_editor(), None);
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

    /// Issue #238: the Palette runs the same flow as the `c` key — persist,
    /// adopt, and leave exactly one background rescan for the event loop.
    #[test]
    fn test_palette_toggle_scan_persists_and_starts_exactly_one_rescan() {
        use crate::settings::ScanMode;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::seeded(PathBuf::from("left"), PathBuf::from("right"));
        // The seeded config persists Precise, so the toggle lands on Fast.
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);
        let before = app.scan().generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let action = crate::commands::CommandEntry {
            key: "c".to_string(),
            label: "Toggle Scan Mode".to_string(),
            command: crate::commands::Command::ToggleScan,
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
                        crate::commands::Invocation::Command(action.command),
                        &mut terminal,
                    )
                    .unwrap();
            });

        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        assert_eq!(
            app.saved_settings().scan_mode,
            ScanMode::Fast,
            "the palette persists the new mode"
        );
        assert_eq!(
            app.requests(),
            [app::Request::Rescan],
            "exactly one background rescan"
        );
        assert_eq!(app.scan().generation(), before, "the event loop starts it");
    }

    /// The event loop starts the rescan a change asked for, once, and the
    /// request is gone afterwards.
    #[tokio::test]
    async fn a_requested_rescan_starts_one_scan() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("left"), dir.path().join("right"));
        app.switch_scan_mode(crate::settings::ScanMode::Precise);
        app.switch_scan_mode(crate::settings::ScanMode::Fast);
        let before = app.scan().generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        run_requests::<crate::test_support::RecordingTerminalGuard>(&mut app, &tx);

        assert_eq!(app.scan().generation(), before + 1);
        assert!(app.requests().is_empty());
    }

    fn select_config_row(app: &mut App, row: app::ConfigRowKind) {
        let idx = app.config_rows().iter().position(|r| *r == row).unwrap();
        app.config_mut().set_selected_idx(idx);
    }

    /// Turning mouse support off in Config releases the terminal's mouse
    /// capture right away, not at the next editor handoff.
    #[tokio::test]
    async fn the_mouse_row_switches_the_terminal_capture() {
        use crate::test_support::RecordingTerminalGuard;
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        RecordingTerminalGuard::reset_log();

        select_config_row(&mut app, app::ConfigRowKind::Mouse);
        app.apply_config_selection();
        run_requests::<RecordingTerminalGuard>(&mut app, &tx);

        assert_eq!(RecordingTerminalGuard::log(), ["mouse_capture(false)"]);
        assert!(!app.settings().mouse());
    }

    /// A terminal that cannot switch capture keeps what it does in effect,
    /// so mouse events are handled exactly when they arrive.
    #[tokio::test]
    async fn a_refused_mouse_switch_keeps_the_capture_in_effect() {
        struct Refusing;
        impl TerminalGuard for Refusing {
            fn acquire(_: bool) -> std::io::Result<Self> {
                Ok(Self)
            }
            fn set_mouse_capture(_: bool) -> std::io::Result<()> {
                Err(std::io::Error::other("no terminal"))
            }
        }
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        select_config_row(&mut app, app::ConfigRowKind::Mouse);
        app.apply_config_selection();
        run_requests::<Refusing>(&mut app, &tx);

        assert!(app.settings().mouse(), "capture is still on");
        assert!(!app.saved_settings().mouse, "the choice is still saved");
        let (toast, is_error) = app.status_toast().unwrap();
        assert!(is_error);
        assert_eq!(toast, "Cannot switch mouse support: no terminal");
    }
}
