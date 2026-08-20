//! Shared actions: scan, copy, palette, external tools, and pure key-outcome builders.
use crate::app::{self, App};
use crate::diff_tool::{self, ExternalDiffTool};
use crate::event::AppEvent;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;
use std::path::PathBuf;
use std::str::FromStr;

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
pub fn diff_launch_outcome(app: &App) -> KeyOutcome {
    let Some(row) = app.selected_row() else {
        return KeyOutcome::None;
    };
    if row.is_dir() || row.left.is_none() || row.right.is_none() {
        return KeyOutcome::None;
    }
    let Some(tool_str) = app.settings().external_diff_tool.as_ref() else {
        return KeyOutcome::None;
    };
    let Ok(tool) = ExternalDiffTool::from_str(tool_str) else {
        return KeyOutcome::None;
    };
    KeyOutcome::LaunchDiff {
        tool,
        left: app.left_path().join(&row.relative_path),
        right: app.right_path().join(&row.relative_path),
    }
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
    let root = if app.active_side_left() {
        app.left_path()
    } else {
        app.right_path()
    };
    KeyOutcome::LaunchEditor {
        path: root.join(&row.relative_path),
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

pub async fn execute_confirm_action(
    app: &mut App,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(action) = app.take_confirmed_action() {
        if let Some(row) = app.selected_row() {
            let relative_path = row.relative_path.clone();
            let name = row.name.clone();

            let src = match action {
                app::ConfirmAction::CopyLeftToRight => app.left_path().join(&relative_path),
                app::ConfirmAction::CopyRightToLeft => app.right_path().join(&relative_path),
            };
            let dst = match action {
                app::ConfirmAction::CopyLeftToRight => app.right_path().join(&relative_path),
                app::ConfirmAction::CopyRightToLeft => app.left_path().join(&relative_path),
            };
            let dst_root = match action {
                app::ConfirmAction::CopyLeftToRight => app.right_path().to_path_buf(),
                app::ConfirmAction::CopyRightToLeft => app.left_path().to_path_buf(),
            };

            // Perform copy — all errors are captured uniformly in `res`
            let res = copy_entry_checked(&src, &dst, &dst_root);

            match res {
                Ok(()) => {
                    app.set_status(format!("Copied '{}'", name), false);
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
                }
                Err(e) => {
                    app.set_status(format!("Copy failed: {}", e), true);
                }
            }
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
        recreate_symlink(src, dst)
    } else if file_type.is_dir() {
        copy_dir_recursive(src, dst, dst_root)
    } else if file_type.is_file() {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
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
            recreate_symlink(&src_path, &dst_path)?;
        } else if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, dst_root)?;
        } else {
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
        app.ignore_matcher().clone(),
        generation,
        tx,
    );
}

pub fn start_scan_task(
    left: PathBuf,
    right: PathBuf,
    precise: bool,
    ignore: crate::ignore::IgnoreMatcher,
    generation: u64,
    tx: tokio::sync::mpsc::Sender<crate::event::AppEvent>,
) {
    tokio::spawn(async move {
        let root = tokio::task::spawn_blocking(move || {
            crate::diff::align_directories(
                &left,
                &right,
                std::path::Path::new(""),
                precise,
                &ignore,
            )
        })
        .await;

        match root {
            Ok(Ok(node)) => {
                let _ = tx
                    .send(crate::event::AppEvent::ScanFinished { generation, node })
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

pub async fn execute_palette_action<B: ratatui::backend::Backend>(
    action: &crate::ui::PaletteAction,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match action.action_id {
        crate::ui::PaletteActionId::ExternalDiff => {
            dispatch_key_outcome(diff_launch_outcome(app), terminal, app.mouse_enabled())?;
        }
        crate::ui::PaletteActionId::ExternalEdit => {
            dispatch_key_outcome(editor_launch_outcome(app), terminal, app.mouse_enabled())?;
        }
        crate::ui::PaletteActionId::CopyLeftToRight => {
            app.request_copy(app::ConfirmAction::CopyLeftToRight);
        }
        crate::ui::PaletteActionId::CopyRightToLeft => {
            app.request_copy(app::ConfirmAction::CopyRightToLeft);
        }
        crate::ui::PaletteActionId::BuiltinDiff => {
            app.enter_file_diff();
        }
        crate::ui::PaletteActionId::SwapPaths => {
            app.swap_paths();
            kick_scan(app, tx);
        }
        crate::ui::PaletteActionId::ToggleScan => {
            if app.switch_scan_mode(app.scan_mode().toggled()) {
                kick_scan(app, tx);
            }
        }
        crate::ui::PaletteActionId::Refresh => {
            kick_scan(app, tx);
        }
        crate::ui::PaletteActionId::Config => {
            app.open_config();
        }
        crate::ui::PaletteActionId::Help => {
            app.open_help();
        }
        crate::ui::PaletteActionId::Filter => {
            app.filter_mut().open();
        }
        crate::ui::PaletteActionId::Quit => {
            app.request_quit();
        }
        crate::ui::PaletteActionId::ToggleWrap => {
            app.diff_mut().toggle_wrap();
        }
        crate::ui::PaletteActionId::ToggleFullDiff => {
            if let Err(e) = app.toggle_diff_show_full() {
                app.set_status(format!("Cannot refresh diff: {e}"), true);
            }
        }
        crate::ui::PaletteActionId::NextChange => {
            app.jump_to_next_change();
        }
        crate::ui::PaletteActionId::PrevChange => {
            app.jump_to_prev_change();
        }
        crate::ui::PaletteActionId::CopyHunkLeftToRight => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight) {
                Ok(()) => app.set_status("Copied change block to right".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        crate::ui::PaletteActionId::CopyHunkRightToLeft => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::RightToLeft) {
                Ok(()) => app.set_status("Copied change block to left".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        crate::ui::PaletteActionId::ToggleTheme => {
            app.toggle_theme();
        }
        crate::ui::PaletteActionId::ToggleFocus => {
            app.toggle_active_side();
        }
        crate::ui::PaletteActionId::FocusLeft => {
            app.focus_left_pane();
        }
        crate::ui::PaletteActionId::FocusRight => {
            app.focus_right_pane();
        }
        crate::ui::PaletteActionId::ExpandSelected => {
            app.expand_selected();
        }
        crate::ui::PaletteActionId::CollapseSelected => {
            app.collapse_selected();
        }
        crate::ui::PaletteActionId::Back => match app.view_mode() {
            app::ViewMode::FileDiff => app.leave_file_diff(),
            app::ViewMode::ConfigMenu => app.close_config(),
            _ => app.close_help(),
        },
    }
    Ok(())
}

pub fn open_repo_url(app: &mut App) {
    app.set_status("Opening GitHub repository in the browser...", false);
    let url = env!("CARGO_PKG_REPOSITORY");
    std::thread::spawn(move || {
        let _ = match std::env::consts::OS {
            "macos" => std::process::Command::new("open").arg(url).status(),
            "windows" => std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .status(),
            _ => std::process::Command::new("xdg-open").arg(url).status(),
        };
    });
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
        }
    }

    #[test]
    fn diff_launch_outcome_none_without_configured_tool() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(None);
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_none_for_directory() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.filter_mut()
            .set_rows(vec![file_row("dir", true, true, true)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_none_for_single_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, false, false)]);
        app.set_selected_idx(0);
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
    }

    #[test]
    fn diff_launch_outcome_builds_paths_for_both_sided_file() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(Some("vim".to_string()));
        app.filter_mut()
            .set_rows(vec![file_row("a.txt", true, true, false)]);
        app.set_selected_idx(0);
        assert_eq!(
            diff_launch_outcome(&app),
            KeyOutcome::LaunchDiff {
                tool: ExternalDiffTool::Vim,
                left: PathBuf::from("/left/a.txt"),
                right: PathBuf::from("/right/a.txt"),
            }
        );
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
        assert_eq!(diff_launch_outcome(&app), KeyOutcome::None);
        assert_eq!(editor_launch_outcome(&app), KeyOutcome::None);
    }

    /// Issue #238: the Palette runs the same atomic flow as the `c` key —
    /// persist, adopt, and start exactly one background rescan.
    #[tokio::test]
    async fn test_palette_toggle_scan_persists_and_starts_exactly_one_rescan() {
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

        let action = crate::ui::PaletteAction {
            key: "c".to_string(),
            label: "Toggle Scan Mode".to_string(),
            action_id: crate::ui::PaletteActionId::ToggleScan,
            disabled_reason: None,
        };
        execute_palette_action(&action, &mut app, &mut terminal, tx)
            .await
            .unwrap();

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
