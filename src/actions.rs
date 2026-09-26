//! Command effects that outlive a key press: the work a confirmation
//! approved (saving staged edits, running a planned copy), the requests a
//! change leaves for the event loop, and the browser launch for Help's link.
//!
//! Nothing here composes a user-visible sentence: an effect reports what it
//! did as data and `commands` names it (ADR-0003), except for work that
//! outlives the synchronous call and reports through [`AppEvent`]. Writing to
//! disk goes through `write`; handing over the terminal through `terminal`.
use crate::app::{self, App};
use crate::event::AppEvent;

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
        Some(entries) => crate::write::copy_scanned_subtree(&src, &dst, &dst_root, &entries),
        None => crate::write::copy_entry_checked(&src, &dst, &dst_root),
    };

    match res {
        Ok(()) => {
            app.leave_file_diff();
            let copied_is_dir = std::fs::symlink_metadata(&dst)
                .map(|m| {
                    let ft = m.file_type();
                    ft.is_dir() && !ft.is_symlink()
                })
                .unwrap_or(false);
            app.request_subtree_rescan(&relative_path, copied_is_dir);
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

/// Carry out the work changes left for the event loop.
pub(crate) fn run_requests<G: crate::terminal::TerminalGuard>(
    app: &mut App,
    tx: &tokio::sync::mpsc::Sender<AppEvent>,
) {
    for request in app.take_requests() {
        match request {
            app::Request::Rescan => crate::scan::start(app, tx.clone()),
            app::Request::RescanSubtree(path) => crate::scan::start_subtree(app, path, tx.clone()),
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
    use crate::diff_tool::ExternalDiffTool;
    use crate::terminal::TerminalGuard;
    use std::path::PathBuf;
    use std::time::SystemTime;

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
