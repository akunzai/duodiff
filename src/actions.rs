//! Shared actions: scan, copy, palette, and external tools.
use crate::app::{self, App};
use crate::diff_tool;
use crate::event::AppEvent;
use crate::key_outcome::KeyOutcome;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::Terminal;
use std::path::PathBuf;

pub fn run_external_diff<B: ratatui::backend::Backend>(
    tool: &diff_tool::ExternalDiffTool,
    left: &std::path::Path,
    right: &std::path::Path,
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    use std::io::IsTerminal;
    let is_terminal = std::io::stdout().is_terminal();
    if is_terminal {
        disable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            )?;
        } else {
            execute!(std::io::stdout(), LeaveAlternateScreen)?;
        }
    }

    let res = diff_tool::open_diff(tool, left, right);
    if let Err(e) = res {
        eprintln!(
            "Error launching external diff: {}. Press Enter to continue...",
            e
        );
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    } else if matches!(tool, diff_tool::ExternalDiffTool::Difftastic) {
        println!("\nPress Enter to return to duodiff...");
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }

    if is_terminal {
        enable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                crossterm::event::EnableMouseCapture
            )?;
        } else {
            execute!(std::io::stdout(), EnterAlternateScreen)?;
        }
    }
    terminal.clear()?;
    Ok(())
}

pub fn run_external_editor<B: ratatui::backend::Backend>(
    file_path: &std::path::Path,
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    use std::io::IsTerminal;
    let is_terminal = std::io::stdout().is_terminal();
    if is_terminal {
        disable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            )?;
        } else {
            execute!(std::io::stdout(), LeaveAlternateScreen)?;
        }
    }

    let res = diff_tool::open_editor(file_path);
    if let Err(e) = res {
        eprintln!(
            "Error launching external editor: {}. Press Enter to continue...",
            e
        );
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }

    if is_terminal {
        enable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                crossterm::event::EnableMouseCapture
            )?;
        } else {
            execute!(std::io::stdout(), EnterAlternateScreen)?;
        }
    }
    terminal.clear()?;
    Ok(())
}

/// Perform the IO a [`KeyOutcome`] describes. Pure key-handling code (`input::handle_key`,
/// `key_outcome::*`) only ever builds a `KeyOutcome`; the process spawn and terminal mode
/// toggling live here so navigation/mode-switch key routing stays free of embedded IO.
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
            run_external_diff(&tool, &left, &right, terminal, mouse_enabled)
        }
        KeyOutcome::LaunchEditor { path } => run_external_editor(&path, terminal, mouse_enabled),
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
                app::ConfirmAction::CopyLeftToRight => app.left_path.join(&relative_path),
                app::ConfirmAction::CopyRightToLeft => app.right_path.join(&relative_path),
            };
            let dst = match action {
                app::ConfirmAction::CopyLeftToRight => app.right_path.join(&relative_path),
                app::ConfirmAction::CopyRightToLeft => app.left_path.join(&relative_path),
            };
            let dst_root = match action {
                app::ConfirmAction::CopyLeftToRight => app.right_path.clone(),
                app::ConfirmAction::CopyRightToLeft => app.left_path.clone(),
            };

            // Perform copy — all errors are captured uniformly in `res`
            let res = copy_entry_checked(&src, &dst, &dst_root);

            match res {
                Ok(()) => {
                    app.set_status(format!("Copied '{}'", name), false);
                    app.view_mode = app::ViewMode::DirectoryTree;
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
        app.left_path.clone(),
        app.right_path.clone(),
        app.precise_mode(),
        app.ignore_matcher.clone(),
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
    action: &crate::app::PaletteAction,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match action.action_id {
        "ext_diff" => {
            dispatch_key_outcome(
                crate::key_outcome::diff_launch_outcome(app),
                terminal,
                app.mouse_enabled,
            )?;
        }
        "ext_edit" => {
            dispatch_key_outcome(
                crate::key_outcome::editor_launch_outcome(app),
                terminal,
                app.mouse_enabled,
            )?;
        }
        "copy_l2r" => {
            if let Some(row) = app.selected_row() {
                if row.left.is_some() {
                    app.request_confirm(
                        format!("Copy '{}' to right side?", row.name),
                        app::ConfirmAction::CopyLeftToRight,
                    );
                }
            }
        }
        "copy_r2l" => {
            if let Some(row) = app.selected_row() {
                if row.right.is_some() {
                    app.request_confirm(
                        format!("Copy '{}' to left side?", row.name),
                        app::ConfirmAction::CopyRightToLeft,
                    );
                }
            }
        }
        "builtin_diff" => {
            app.enter_file_diff();
        }
        "swap_paths" => {
            app.swap_paths();
            kick_scan(app, tx);
        }
        "toggle_scan" => {
            app.toggle_precise_mode();
            kick_scan(app, tx);
        }
        "refresh" => {
            kick_scan(app, tx);
        }
        "config" => {
            app.open_config();
        }
        "help" => {
            app.open_help();
        }
        "filter" => {
            app.open_filter();
        }
        "quit" => {
            app.request_quit();
        }
        "toggle_wrap" => {
            app.toggle_diff_wrap();
        }
        "toggle_full" => {
            if let Err(e) = app.toggle_diff_show_full() {
                app.set_status(format!("Cannot refresh diff: {e}"), true);
            }
        }
        "next_change" => {
            app.jump_to_next_change();
        }
        "prev_change" => {
            app.jump_to_prev_change();
        }
        "copy_hunk_l2r" => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight) {
                Ok(()) => app.set_status("Copied change block to right".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        "copy_hunk_r2l" => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::RightToLeft) {
                Ok(()) => app.set_status("Copied change block to left".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        "back" => {
            if app.view_mode == app::ViewMode::FileDiff {
                app.view_mode = app::ViewMode::DirectoryTree;
            } else {
                app.close_help();
            }
        }
        _ => {}
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
