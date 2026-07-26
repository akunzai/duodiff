use app::App;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use event::{AppEvent, EventHandler};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::path::PathBuf;
use std::time::Duration;

pub mod actions;
pub mod app;
pub mod diff;
pub mod diff_tool;
pub mod diff_view;
pub mod event;
pub mod ignore;
pub mod input;
pub mod key_outcome;
pub mod settings;
#[cfg(test)]
pub mod test_support;
pub mod text_input;
pub mod theme;
pub mod ui;
pub mod update_check;
pub mod upgrade;

#[derive(Parser, Debug)]
#[command(
    name = "duodiff",
    version,
    about = "A cross-platform TUI directory comparison tool"
)]
struct Args {
    /// Left directory to compare
    #[arg(value_name = "LEFT_DIR")]
    left_dir: Option<PathBuf>,
    /// Right directory to compare
    #[arg(value_name = "RIGHT_DIR")]
    right_dir: Option<PathBuf>,
    /// Glob pattern to exclude from comparison. Can be specified multiple times.
    #[arg(short = 'e', long = "exclude", value_name = "PATTERN")]
    exclude: Vec<String>,
    /// Print startup checks without launching the TUI
    #[arg(long, help = "Print startup checks without launching the TUI")]
    check: bool,
    /// Upgrade the running pre-built binary from GitHub Releases (combine with --check or --upgrade-version)
    #[arg(
        long,
        help = "Upgrade the running pre-built binary from GitHub Releases (combine with --check or --upgrade-version)"
    )]
    upgrade: bool,
    /// With --upgrade: install a specific release (v0.1.0 or 0.1.0)
    #[arg(
        long = "upgrade-version",
        value_name = "TAG",
        help = "With --upgrade: install a specific release (v0.1.0 or 0.1.0)"
    )]
    upgrade_version: Option<String>,
    /// Skip the startup check for a newer release for this session
    #[arg(
        long = "no-update-check",
        help = "Skip the startup check for a newer release for this session"
    )]
    no_update_check: bool,
    /// Disable mouse support for this session (overrides `mouse = true` in config.toml)
    #[arg(
        long = "no-mouse",
        help = "Disable mouse support for this session (overrides `mouse = true` in config.toml)"
    )]
    no_mouse: bool,
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    events: &mut EventHandler,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    loop {
        if app.should_quit {
            break;
        }
        // Refresh viewport geometry *before* drawing and before the key/mouse
        // handlers below, so rendering and scroll clamping always agree — and
        // neither reads geometry from the previous terminal size.
        app.sync_viewport(terminal.size()?.into());
        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press
                        && input::handle_key(key, app, terminal, tx.clone()).await?
                    {
                        break;
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse)) if app.mouse_enabled => {
                    input::handle_mouse(mouse, app, terminal, tx.clone()).await?;
                }
                AppEvent::ScanFinished { generation, node } => {
                    app.apply_scan_result(generation, node);
                }
                AppEvent::Error {
                    generation,
                    message,
                } => {
                    if app.fail_scan(generation) {
                        app.set_status(format!("Scan failed: {message}"), true);
                    }
                }
                AppEvent::Tick => {
                    app.clear_expired_status(std::time::Duration::from_secs(4));
                }
                AppEvent::UpdateCheckOutcome(outcome) => {
                    let now = crate::update_check::now_secs();
                    match outcome {
                        crate::update_check::UpdateCheckOutcome::Newer(version) => {
                            if let Ok(path) = crate::update_check::state_path() {
                                crate::update_check::save_state(
                                    &path,
                                    &crate::update_check::UpdateCheckState {
                                        last_check: now,
                                        latest_seen: version.clone(),
                                    },
                                );
                            }
                            app.update_available = Some(version);
                        }
                        crate::update_check::UpdateCheckOutcome::UpToDate => {
                            if let Ok(path) = crate::update_check::state_path() {
                                crate::update_check::save_state(
                                    &path,
                                    &crate::update_check::UpdateCheckState {
                                        last_check: now,
                                        latest_seen: String::new(),
                                    },
                                );
                            }
                            app.update_available = None;
                        }
                        crate::update_check::UpdateCheckOutcome::Failed => {}
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Lexically normalize a path (resolve `.` / `..` without touching the FS).
/// True when `path` is the same as `root` or a descendant (lexical check).
/// Recursive directory copy that never follows directory symlinks and refuses
/// destinations outside `dst_root`.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.upgrade {
        crate::upgrade::run(crate::upgrade::Options {
            check_only: args.check,
            version: args.upgrade_version,
        })?;
        return Ok(());
    }

    if args.check && args.left_dir.is_none() && args.right_dir.is_none() {
        println!("duodiff version {} is ready", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let left_dir = match args.left_dir.clone() {
        Some(d) => d,
        None => {
            eprintln!("Error: Missing LEFT_DIR directory argument.");
            std::process::exit(1);
        }
    };
    let right_dir = match args.right_dir.clone() {
        Some(d) => d,
        None => {
            eprintln!("Error: Missing RIGHT_DIR directory argument.");
            std::process::exit(1);
        }
    };

    if !left_dir.is_dir() || !right_dir.is_dir() {
        eprintln!("Both arguments must be valid directories.");
        std::process::exit(1);
    }

    let mut ignore_matcher = crate::ignore::IgnoreMatcher::new();
    ignore_matcher.add_patterns(&args.exclude);
    ignore_matcher.load_from_dir(&left_dir);
    ignore_matcher.load_from_dir(&right_dir);

    // Mouse capture is negotiated once at terminal setup, so the effective flag must be
    // known before `setup_terminal` runs (App, which owns `settings`, isn't built yet).
    let mouse_enabled = crate::settings::resolve_mouse_enabled(
        crate::settings::AppSettings::load().mouse,
        args.no_mouse,
    );

    // Initialize terminal safely
    let mut terminal = setup_terminal(mouse_enabled)?;

    let mut app = App::new_with_ignore(left_dir.clone(), right_dir.clone(), ignore_matcher.clone());
    app.mouse_enabled = mouse_enabled;
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    // Initialize update checker
    app.update_check_enabled = !args.no_update_check && app.settings.check_updates;
    if app.update_check_enabled {
        if let Ok(path) = crate::update_check::state_path() {
            let seen = crate::update_check::load_state(&path).latest_seen;
            if !seen.is_empty() {
                app.update_available =
                    crate::update_check::is_newer(&seen, env!("CARGO_PKG_VERSION"));
            }
        }

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let path_opt = crate::update_check::state_path().ok();
            let due = path_opt.as_ref().is_none_or(|path| {
                crate::update_check::should_check(
                    crate::update_check::load_state(path).last_check,
                    crate::update_check::now_secs(),
                )
            });
            if due {
                let outcome = tokio::task::spawn_blocking(move || {
                    crate::update_check::check(
                        &crate::upgrade::UreqClient,
                        env!("CARGO_PKG_VERSION"),
                    )
                })
                .await
                .unwrap_or(crate::update_check::UpdateCheckOutcome::Failed);
                let _ = tx_clone.send(AppEvent::UpdateCheckOutcome(outcome)).await;
            }
        });
    }

    actions::kick_scan(&mut app, tx.clone());

    let res = run_app(&mut terminal, &mut app, &mut events, tx.clone()).await;

    // Restore terminal unconditionally
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    );

    res
}

fn setup_terminal(
    mouse_enabled: bool,
) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    let setup_result = if mouse_enabled {
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )
    } else {
        execute!(stdout, EnterAlternateScreen)
    };
    if let Err(err) = setup_result {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(t) => Ok(t),
        Err(err) => {
            let _ = execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
            let _ = disable_raw_mode();
            Err(err.into())
        }
    }
}

/// Bump the scan generation and spawn a background directory scan.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AppEvent;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_start_scan_task() {
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        actions::start_scan_task(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
            false,
            crate::ignore::IgnoreMatcher::default(),
            7,
            tx,
        );

        let res = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        let opt = res.expect("Timeout waiting for scan result");
        let event = opt.expect("Expected Some(AppEvent::ScanFinished), got None");
        match event {
            AppEvent::ScanFinished { generation, .. } => assert_eq!(generation, 7),
            other => panic!("Expected AppEvent::ScanFinished, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_stale_scan_finished_is_ignored() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // Two scan starts → generation 2, still in flight.
        app.begin_scan();
        app.begin_scan();
        app.set_root_node(AlignedNode {
            name: "current".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            is_expanded: true,
        });
        app.flatten_tree();
        assert_eq!(app.flat_rows()[0].name, "current");

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Stale generation 1 must not replace the tree.
            let _ = tx_clone
                .send(AppEvent::ScanFinished {
                    generation: 1,
                    node: AlignedNode {
                        name: "stale".to_string(),
                        relative_path: PathBuf::from(""),
                        left: None,
                        right: None,
                        state: DiffState::Identical,
                        children: vec![],
                        is_expanded: true,
                    },
                })
                .await;
            let _ = tx_clone
                .send(AppEvent::Terminal(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char('q'),
                        crossterm::event::KeyModifiers::empty(),
                    ),
                )))
                .await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.flat_rows()[0].name, "current");
        assert!(app.scan_in_progress()); // still waiting for generation 2
    }

    #[tokio::test]
    async fn test_scan_error_toasts_and_keeps_running() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.begin_scan();
        app.set_root_node(AlignedNode {
            name: "keep-me".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            is_expanded: true,
        });
        app.flatten_tree();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let _ = tx_clone
                .send(AppEvent::Error {
                    generation: 1,
                    message: "permission denied".to_string(),
                })
                .await;
            let _ = tx_clone
                .send(AppEvent::Terminal(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char('q'),
                        crossterm::event::KeyModifiers::empty(),
                    ),
                )))
                .await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok(), "scan error must not exit the app");
        assert!(!app.scan_in_progress());
        assert_eq!(app.flat_rows()[0].name, "keep-me");
        let (msg, is_error, _) = app.status_message.as_ref().expect("status toast");
        assert!(is_error);
        assert!(msg.contains("permission denied"));
    }

    #[tokio::test]
    async fn test_filter_bar_edits_cjk_text_by_char_not_byte() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_filter();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        for c in "你好".chars() {
            input::handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(c),
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }
        assert_eq!(app.filter_input, "你好");

        // Backspace must remove the whole trailing CJK char, not one UTF-8 byte.
        input::handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Backspace,
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert_eq!(app.filter_input, "你");
    }

    #[tokio::test]
    async fn test_theme_toggle_key_from_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let quit = input::handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(!quit);
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Dark);

        input::handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
    }

    #[tokio::test]
    async fn test_theme_toggle_key_ignored_while_filtering() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.settings.theme = crate::theme::ThemeChoice::Dark;
        app.open_filter();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        input::handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();

        // 'T' should be typed into the filter input, not toggle the theme (and, since
        // no toggle happened, nothing was persisted to the shared config file either).
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Dark);
        assert_eq!(app.filter_input, "T");
    }

    #[tokio::test]
    async fn test_config_menu_mouse_scroll_navigates_and_adjusts_diff_context() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = app::ViewMode::ConfigMenu;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Row layout depends on which external diff tools are detected on the test
        // machine's $PATH, so look up positions rather than hardcoding indices.
        let rows = app.config_rows();
        let mouse_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::Mouse))
            .unwrap();
        let theme_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::Theme))
            .unwrap();
        let diff_context_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::DiffContext))
            .unwrap();

        app.config_selected_idx = mouse_idx;
        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.config_selected_idx, theme_idx,
            "scroll down navigates to next selectable row"
        );

        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.config_selected_idx, mouse_idx,
            "scroll up navigates to previous selectable row"
        );

        // On the Diff context row, scroll adjusts the value instead of navigating.
        app.config_selected_idx = diff_context_idx;
        assert_eq!(app.settings.diff_context, 7);
        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.settings.diff_context, 8,
            "scroll up increases diff context"
        );
        assert_eq!(
            app.config_selected_idx, diff_context_idx,
            "diff context row stays selected"
        );

        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.settings.diff_context, 6,
            "scroll down decreases diff context"
        );
    }

    #[tokio::test]
    async fn test_help_mouse_scroll_moves_topic_body_and_index_selection() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = app::ViewMode::Help;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        app.help_index_open = false;
        app.help_scroll = 0;
        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help_scroll, 1, "scroll down advances the topic body");
        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help_scroll, 0, "scroll up rewinds the topic body");
        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help_scroll, 0, "scroll up saturates at 0, no underflow");

        app.help_index_open = true;
        app.help_index_sel = 0;
        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel, 1,
            "scroll down moves the index selection"
        );
        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel, 0,
            "scroll up moves the index selection back"
        );
        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel,
            app::HelpTopic::all().len() - 1,
            "scroll up wraps to the last topic"
        );
    }

    #[tokio::test]
    async fn test_palette_mouse_scroll_navigates_items_without_leaking_to_background() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: crate::diff::DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: crate::diff::DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
        ]);
        app.apply_filter();
        app.selected_idx = 0;
        app.palette.visible = true;
        app.palette.items = vec![
            app::PaletteAction {
                key: "a".to_string(),
                label: "Action A".to_string(),
                action_id: "a",
                enabled: true,
            },
            app::PaletteAction {
                key: "b".to_string(),
                label: "Action B".to_string(),
                action_id: "b",
                enabled: true,
            },
        ];
        app.palette.selected_idx = 0;
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        input::handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.palette.selected_idx, 1,
            "scroll down navigates palette items"
        );
        assert_eq!(
            app.selected_idx, 0,
            "scroll must not leak through to the background directory tree"
        );

        input::handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.palette.selected_idx, 0,
            "scroll up navigates palette items back"
        );
        assert_eq!(
            app.selected_idx, 0,
            "scroll must not leak through to the background directory tree"
        );
    }

    #[tokio::test]
    async fn test_esc_quits_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let _ = tx_clone
                .send(AppEvent::Terminal(crossterm::event::Event::Key(
                    crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Esc,
                        crossterm::event::KeyModifiers::empty(),
                    ),
                )))
                .await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn execute_palette_action() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        // Test config action
        let action_config = crate::app::PaletteAction {
            key: "C".to_string(),
            label: "Edit Configuration".to_string(),
            action_id: "config",
            enabled: true,
        };
        actions::execute_palette_action(&action_config, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.view_mode, crate::app::ViewMode::ConfigMenu);

        // Test quit action
        let action_quit = crate::app::PaletteAction {
            key: "q".to_string(),
            label: "Quit".to_string(),
            action_id: "quit",
            enabled: true,
        };
        actions::execute_palette_action(&action_quit, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn test_run_app_pane_focus_number_keys() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert!(app.active_side_left);

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('2'),
                crossterm::event::KeyCode::Char('1'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert!(app.active_side_left);
    }

    #[tokio::test]
    async fn test_run_app_keyboard_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        // Let's send a key event to move down
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let key_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(key_event)).await;

            // And then send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Assert that the 'j' key was processed and app moved down
        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_run_app_ctrl_page_scroll() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(
            (0..40)
                .map(|i| crate::app::FlatRow {
                    depth: 0,
                    relative_path: PathBuf::from(format!("f{i}.txt")),
                    name: format!("f{i}.txt"),
                    state: crate::diff::DiffState::Identical,
                    left: None,
                    right: None,
                })
                .collect(),
        );
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let page_down = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('f'),
                crossterm::event::KeyModifiers::CONTROL,
            ));
            let _ = tx_clone.send(AppEvent::Terminal(page_down)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);
        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After one Ctrl+f, selection should have advanced by roughly a page.
        assert!(
            app.selected_idx > 0,
            "Ctrl+f should page the selection down, got idx {}",
            app.selected_idx
        );
    }

    #[tokio::test]
    async fn test_run_app_mouse_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Scroll down
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_run_app_mouse_click_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the second row (click_y = 3, which maps to index 3 - 2 = 1)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 3,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_help_index_mouse_click_selects_topic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::Help;
        app.help_index_open = true;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the 4th item (click_y = 5, maps to index 5 - 2 = 3 which is HelpTopic::Mouse)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Exit help topic view, then quit from directory tree.
            for code in [
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Mouse);
        assert!(!app.help_index_open);
    }

    #[tokio::test]
    async fn test_run_app_keyboard_expand_collapse() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "child".to_string(),
                relative_path: PathBuf::from("child"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.set_root_node(node);

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Select root (idx = 0) and collapse it using 'h'
            let collapse_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('h'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(collapse_event)).await;

            // Expand it using 'Right' key
            let expand_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Right,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(expand_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.flat_rows().len(), 2);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Since it was collapsed and expanded, flat_rows should be 2 again
        assert_eq!(app.flat_rows().len(), 2);
    }

    #[tokio::test]
    async fn test_run_app_file_diff_navigation() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press Enter to go to FileDiff mode
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Scroll down
            let down_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(down_event)).await;

            // Scroll up
            let up_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('k'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(up_event)).await;

            // Press Esc to exit FileDiff mode
            let esc_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(esc_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        // Initially in DirectoryTree mode
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_run_app_keyboard_diff_tool_launch() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let _guard = crate::test_support::lock_env_tests();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.settings.external_diff_tool = None;
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 'D' to launch diff tool
            let d_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('D'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(d_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_run_app_keyboard_editor_launch() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let _guard = crate::test_support::lock_env_tests();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 'E' to launch editor
            let e_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('E'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(e_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_app_mouse_double_click_enters_diff() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::fs;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        fs::write(left_dir.path().join("file.txt"), "hello").unwrap();
        fs::write(right_dir.path().join("file.txt"), "world").unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(crate::diff::FileInfo {
                is_dir: false,
                size: 10,
                modified: std::time::SystemTime::UNIX_EPOCH,
            }),
            right: Some(crate::diff::FileInfo {
                is_dir: false,
                size: 15,
                modified: std::time::SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // First click
            let click1 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(click1)).await;

            // Second click immediately
            let click2 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(click2)).await;

            // Wait, then quit
            tokio::time::sleep(Duration::from_millis(50)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event.clone())).await;

            // Send a second 'q' to quit the app from DirectoryTree mode
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        // Verify that it did enter FileDiff mode and populated diff_rows
        assert!(!app.diff_rows.is_empty());
    }

    #[test]
    fn test_path_is_under_lexical() {
        let root = std::path::Path::new("/tmp/root");
        assert!(actions::path_is_under(
            std::path::Path::new("/tmp/root"),
            root
        ));
        assert!(actions::path_is_under(
            std::path::Path::new("/tmp/root/a/b"),
            root
        ));
        assert!(!actions::path_is_under(
            std::path::Path::new("/tmp/root/../escape"),
            root
        ));
        assert!(!actions::path_is_under(
            std::path::Path::new("/tmp/other"),
            root
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_copy_recreates_symlink_not_target_tree() {
        use std::os::unix::fs::symlink;
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        // Symlink inside left pointing at outside dir
        symlink(outside.path(), left.path().join("link_out")).unwrap();

        // Copying the symlink should recreate the link, not walk outside.
        actions::copy_entry_checked(
            &left.path().join("link_out"),
            &right.path().join("link_out"),
            right.path(),
        )
        .unwrap();
        assert!(right
            .path()
            .join("link_out")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        // Destination must not materialize secret.txt as a regular copied tree.
        assert!(
            !right.path().join("link_out").join("secret.txt").is_file()
                || std::fs::symlink_metadata(right.path().join("link_out"))
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn test_copy_file_and_directory() {
        use crate::diff::FileInfo;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        // 1. Test copy_dir_recursive helper
        let src_sub = left_dir.path().join("sub");
        std::fs::create_dir_all(&src_sub).unwrap();
        write(src_sub.join("file.txt"), "hello sub").unwrap();

        let dst_sub = right_dir.path().join("sub");
        actions::copy_dir_recursive(&src_sub, &dst_sub, right_dir.path()).unwrap();

        assert!(dst_sub.join("file.txt").exists());
        assert_eq!(
            read_to_string(dst_sub.join("file.txt")).unwrap(),
            "hello sub"
        );

        // Escape attempt: destination outside dst_root must fail.
        let outside = left_dir.path().join("outside");
        let err = actions::copy_dir_recursive(&src_sub, &outside, right_dir.path()).unwrap_err();
        assert!(err.to_string().contains("escapes"));

        // 2. Test execute_confirm_action (CopyLeftToRight)
        write(left_dir.path().join("test_copy.txt"), "copy content").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.selected_idx = 0;
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("test_copy.txt"),
            name: "test_copy.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }]);
        app.apply_filter();

        app.show_confirm_modal = true;
        app.confirm_modal_action = Some(app::ConfirmAction::CopyLeftToRight);
        app.confirm_modal_message = "Copy test_copy.txt to right side?".to_string();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let res = actions::execute_confirm_action(&mut app, tx).await;
        assert!(res.is_ok());

        // Verify the file was copied to the right directory
        let copied_path = right_dir.path().join("test_copy.txt");
        assert!(copied_path.exists());
        assert_eq!(read_to_string(copied_path).unwrap(), "copy content");

        // Verify show_confirm_modal was reset
        assert!(!app.show_confirm_modal);

        // Verify success status message was set
        assert!(app.status_message.is_some());
        let (msg, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(!is_error, "Expected success status, got error");
        assert!(
            msg.contains("test_copy.txt"),
            "Status should mention the file name"
        );

        // Verify re-scan was triggered (message sent to rx)
        let msg = rx.recv().await;
        assert!(msg.is_some());
    }

    #[tokio::test]
    async fn test_copy_error_source_not_found() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        // Don't create the source file — it doesn't exist on disk
        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.selected_idx = 0;
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("nonexistent.txt"),
            name: "nonexistent.txt".to_string(),
            state: crate::diff::DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }]);
        app.apply_filter();

        app.show_confirm_modal = true;
        app.confirm_modal_action = Some(app::ConfirmAction::CopyLeftToRight);

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let res = actions::execute_confirm_action(&mut app, tx).await;
        // The function itself should not return Err — errors are captured in status
        assert!(res.is_ok());

        // Verify error status message was set
        assert!(app.status_message.is_some());
        let (msg, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(is_error, "Expected error status");
        assert!(
            msg.contains("Copy failed"),
            "Status should indicate failure: {}",
            msg
        );

        // Verify NO re-scan was triggered (channel should be empty)
        assert!(
            rx.try_recv().is_err(),
            "Re-scan should not be triggered on copy failure"
        );
    }

    #[tokio::test]
    async fn test_copy_from_file_diff_view() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        write(left_dir.path().join("file.txt"), "left content").unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // First enter Diff View by pressing Enter
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Wait, then press 'R' to copy left to right
            tokio::time::sleep(Duration::from_millis(50)).await;
            let r_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('R'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(r_event)).await;

            // Wait, then press 'y' to confirm copy
            tokio::time::sleep(Duration::from_millis(50)).await;
            let y_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('y'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(y_event)).await;

            // Wait, then quit TUI
            tokio::time::sleep(Duration::from_millis(50)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        // Run the event loop
        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Verify it switched back to DirectoryTree
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        // Verify the file was copied to the right directory
        let copied_path = right_dir.path().join("file.txt");
        assert!(copied_path.exists());
        assert_eq!(read_to_string(copied_path).unwrap(), "left content");
    }

    #[tokio::test]
    async fn test_run_app_keyboard_swap_directories() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from(""),
            name: "root".to_string(),
            state: crate::diff::DiffState::Identical,
            left: None,
            right: None,
        }]);
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 's' to swap
            let s_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('s'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(s_event)).await;

            // Wait for scan to finish, then quit
            tokio::time::sleep(Duration::from_millis(100)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.left_path, PathBuf::from("left"));
        assert_eq!(app.right_path, PathBuf::from("right"));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Paths should be swapped
        assert_eq!(app.left_path, PathBuf::from("right"));
        assert_eq!(app.right_path, PathBuf::from("left"));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_change_navigation() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();
        app.view_mode = crate::app::ViewMode::FileDiff;
        // Pane content width (38 at 80 columns) comes from `App::sync_viewport`,
        // which `run_app` runs each frame.
        app.diff_rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "header".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "header".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "tail".to_string(),
                }),
                None,
            )),
        ];

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('N'),
                crossterm::event::KeyCode::Char('N'),
                crossterm::event::KeyCode::Char('P'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_wrap_and_horizontal_scroll() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }]);
        app.apply_filter();

        // Pre-populate diff_rows with a long line so horizontal scrolling is meaningful.
        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
        ))];

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Enter FileDiff mode
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Toggle wrap mode on
            let w_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(w_event)).await;

            // Toggle wrap mode off
            let w_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(w_event)).await;

            // Scroll right horizontally
            let right_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Right,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(right_event)).await;

            // Scroll left horizontally
            let left_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(left_event)).await;

            // Exit FileDiff and quit
            let esc_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(esc_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        assert!(!app.diff_wrap);
        assert_eq!(app.diff_h_scroll, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        assert!(!app.diff_wrap);
        assert_eq!(app.diff_h_scroll, 0);
    }

    #[tokio::test]
    async fn test_diff_right_arrow_clamps_to_synced_viewport_width_not_terminal_size() {
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        use ratatui::Terminal;
        use similar::ChangeTag;

        // The TestBackend is much wider than the viewport synced below, so the
        // old `terminal.size().width / 2` formula and the real, layout-derived
        // `diff_content_width` disagree sharply. A regression back to deriving
        // the clamp from terminal size would land far past the value asserted
        // here.
        let backend = TestBackend::new(200, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;
        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "a".repeat(100),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "a".repeat(100),
            }),
        ))];
        app.sync_viewport(Rect::new(0, 0, 40, 24));
        let expected_max_h_scroll = app.viewport().max_diff_h_scroll();
        assert_ne!(
            expected_max_h_scroll, 0,
            "test setup must produce a non-trivial clamp"
        );

        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        for _ in 0..(expected_max_h_scroll + 5) {
            input::handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Right,
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }

        assert_eq!(
            app.diff_h_scroll, expected_max_h_scroll,
            "Right-arrow must clamp to the synced viewport width, not the terminal's actual width"
        );
    }

    #[tokio::test]
    async fn test_confirm_modal_interception_identical_across_all_view_modes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for view_mode in [
            crate::app::ViewMode::DirectoryTree,
            crate::app::ViewMode::FileDiff,
            crate::app::ViewMode::ConfigMenu,
            crate::app::ViewMode::Help,
        ] {
            // 'n'/Esc must dismiss the modal and clear the pending action, rather
            // than falling through to that ViewMode's own Esc handling (e.g.
            // ConfigMenu's Esc normally navigates back to config_return_view).
            let backend = TestBackend::new(80, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.view_mode = view_mode;
            app.show_confirm_modal = true;
            app.confirm_modal_action = Some(crate::app::ConfirmAction::CopyLeftToRight);
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            input::handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Esc,
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                !app.show_confirm_modal,
                "{view_mode:?}: Esc must dismiss the confirm modal"
            );
            assert!(
                app.confirm_modal_action.is_none(),
                "{view_mode:?}: Esc must clear the pending confirm action"
            );
            assert_eq!(
                app.view_mode, view_mode,
                "{view_mode:?}: dismissing the modal must not itself change the view mode"
            );

            // 'y' must route through execute_confirm_action (which resets
            // show_confirm_modal unconditionally) rather than that ViewMode's own
            // 'y' handling. `confirm_modal_action: None` exercises the routing
            // without touching the filesystem.
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.view_mode = view_mode;
            app.show_confirm_modal = true;
            app.confirm_modal_action = None;
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            input::handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('y'),
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                !app.show_confirm_modal,
                "{view_mode:?}: 'y' must route through execute_confirm_action"
            );
        }
    }

    #[tokio::test]
    async fn test_help_opens_from_directory_tree_and_returns_on_esc() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_with_contextual_topic_and_return_view() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Open Help from FileDiff, then unwind back to DirectoryTree to quit:
            // Esc (Help -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // help_topic/help_return_view were set correctly when `?` was pressed from
        // FileDiff, and are still holding those values after the full unwind.
        assert_eq!(app.help_topic, crate::app::HelpTopic::FileDiff);
        assert_eq!(app.help_return_view, crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_from_config_and_returns_to_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_config();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? (-> Help) -> Esc (-> Config) -> q (-> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Config);
        assert_eq!(app.help_return_view, crate::app::ViewMode::ConfigMenu);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_file_diff_and_returns_on_esc() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // C (FileDiff -> Config) -> Esc (Config -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('C'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // config_return_view proves `C` from FileDiff actually opened Config (rather than
        // being ignored as a no-op key), and the final DirectoryTree confirms Esc returned to
        // FileDiff (not stranding on DirectoryTree) before the subsequent q's unwound further.
        assert_eq!(app.config_return_view, crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_close_button_mouse_click_returns_to_file_diff() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;
        app.open_config();
        assert_eq!(app.view_mode, crate::app::ViewMode::ConfigMenu);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Click the [x] close button (top-right, row 1, terminal width 80 -> columns
        // 75..77 per draw_close_button). Distinct from the `Esc`/`q` key path fixed above —
        // this exercises the separate mouse click-detection code in handle_mouse.
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 76,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        input::handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        // Must land back on FileDiff, not be stranded on DirectoryTree.
        assert_eq!(app.view_mode, crate::app::ViewMode::FileDiff);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_help_and_returns_to_help() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? (FileDiff -> Help) -> C (Help -> Config) -> Esc (Config -> Help) ->
            // Esc (Help -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Char('C'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.config_return_view, crate::app::ViewMode::Help);
        assert_eq!(app.help_return_view, crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_digit_key_jumps_topic_without_opening_index() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? (Help, topic=DirectoryTree) -> '4' (topic=Mouse) -> Esc -> q
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Char('4'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Mouse);
        assert!(!app.help_index_open);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_tab_opens_index_at_current_topic_position() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? -> '4' (jump to Mouse, pos 3) -> Tab (open index at sel=3) -> Esc -> q
            // Tests that Tab correctly maps current topic to its position in the index
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Char('4'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After jumping to '4' (Mouse at position 3) and pressing Tab, index should open at sel=3
        assert_eq!(app.help_index_sel, 3);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_index_navigation_wraps_both_directions() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Up-wrap: ? -> Tab (index open, sel=0) -> k (wraps to sel=4)
            // Down-wrap: j (wraps back from sel=4 to sel=0) -> j (sel=0 to sel=1) -> Esc -> q
            // This final j movement to sel=1 only happens if k/j navigation works;
            // it's a genuinely falsifiable assertion (would fail under old flat-match code).
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Char('k'),
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After 'k' from sel=0, wraps to sel=4 (up wraps to end)
        // After 'j' from sel=4, wraps back to sel=0 (down wraps to start)
        // After 'j' from sel=0, moves to sel=1 (normal forward move)
        // Only the current implementation produces sel=1; old flat-match code never navigates, stays at 0
        assert_eq!(app.help_index_sel, 1);
    }

    #[tokio::test]
    async fn test_help_index_digit_selects_topic_and_closes_index() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? -> Tab (open index) -> '3' (select Config, index at position 2) -> Esc -> q
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Char('3'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Config);
        assert!(!app.help_index_open);
    }

    #[tokio::test]
    async fn test_help_esc_from_open_index_exits_help_entirely() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // Directly seed app into (Help, index open) state, bypassing Tab key processing.
        // This isolates the test to verify Esc handler's help_index_open reset logic.
        // Under old flat-match code, Esc wouldn't reset help_index_open (only view_mode),
        // making assert!(!help_index_open) genuinely fail (RED).
        app.view_mode = crate::app::ViewMode::Help;
        app.help_return_view = crate::app::ViewMode::DirectoryTree;
        app.help_index_open = true;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Esc (from index-open Help, should reset help_index_open) -> q (break)
            for code in [
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
        // Verify that index mode was properly closed when exiting Help from index-open state.
        // This assertion independently verifies help_index_open reset without relying on Tab working.
        assert!(!app.help_index_open);
    }
}
