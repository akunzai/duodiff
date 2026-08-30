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
pub mod commands;
pub mod diff;
pub mod diff_tool;
pub mod diff_view;
pub mod event;
pub mod ignore;
pub mod input;
pub mod layout;
pub mod settings;
#[cfg(test)]
pub mod test_support;
pub mod text_input;
pub mod theme;
pub mod ui;
pub mod upgrade;
pub mod view;

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
    /// Process `.gitignore` files for this session (overrides config only).
    #[arg(long, conflicts_with = "no_gitignore")]
    gitignore: bool,
    /// Do not process `.gitignore` files for this session (overrides config only).
    #[arg(long, conflicts_with = "gitignore")]
    no_gitignore: bool,
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
    /// Scan mode for this session (overrides `scan_mode` in config.toml without writing it)
    #[arg(
        long = "scan-mode",
        value_name = "MODE",
        value_enum,
        help = "Scan mode for this session: fast (size + mtime) or precise (SHA-256). Overrides `scan_mode` in config.toml without writing it"
    )]
    scan_mode: Option<crate::settings::ScanMode>,
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
    let mut commands = crate::commands::Commands::new(tx.clone());
    loop {
        if app.should_quit() {
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
                        && input::handle_key_with_commands(
                            key,
                            app,
                            terminal,
                            tx.clone(),
                            &mut commands,
                        )
                        .await?
                    {
                        break;
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse))
                    if app.mouse_enabled() =>
                {
                    input::handle_mouse_with_commands(
                        mouse,
                        app,
                        terminal,
                        tx.clone(),
                        &mut commands,
                    )
                    .await?;
                }
                AppEvent::ScanProgress { generation, count } => {
                    if generation == app.scan_generation() {
                        app.set_scan_progress(count);
                    }
                }
                AppEvent::ScanFinished { generation, node } => {
                    app.apply_scan_result(generation, *node);
                }
                AppEvent::Error {
                    generation,
                    message,
                } => {
                    if app.fail_scan(generation) {
                        app.set_status(format!("Scan failed: {message}"), true);
                    }
                }
                AppEvent::CommandFailed { message } => {
                    app.set_status(message, true);
                }
                AppEvent::Tick => {
                    app.tick();
                    app.clear_expired_status(std::time::Duration::from_secs(4));
                }
                AppEvent::UpdateCheckOutcome(outcome) => {
                    app.apply_update_check_outcome(outcome);
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

    let settings = crate::settings::AppSettings::load();
    let cli_gitignore = args
        .gitignore
        .then_some(true)
        .or(args.no_gitignore.then_some(false));
    let respect_gitignore =
        crate::settings::resolve_respect_gitignore(settings.respect_gitignore, cli_gitignore);
    let left_ignore = crate::ignore::IgnoreMatcher::for_root(
        left_dir.clone(),
        &settings.global_exclusions,
        respect_gitignore,
        &args.exclude,
    );
    let right_ignore = crate::ignore::IgnoreMatcher::for_root(
        right_dir.clone(),
        &settings.global_exclusions,
        respect_gitignore,
        &args.exclude,
    );
    let (left_ignore, right_ignore) = match (left_ignore, right_ignore) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("Invalid exclusion pattern: {error}");
            std::process::exit(1);
        }
    };

    // Mouse capture is negotiated once at terminal setup, so the effective flag must be
    // known before `setup_terminal` runs (App, which owns `settings`, isn't built yet).
    let mouse_enabled = crate::settings::resolve_mouse_enabled(settings.mouse, args.no_mouse);

    // Initialize terminal safely
    let mut terminal = setup_terminal(mouse_enabled)?;

    let mut app = App::new_with_ignore(
        left_dir.clone(),
        right_dir.clone(),
        left_ignore,
        right_ignore,
    );
    app.set_ignore_cli_overrides(args.exclude.clone(), cli_gitignore);
    app.set_mouse_enabled(mouse_enabled);
    // Session-only: `--scan-mode` seeds the effective mode without writing the
    // config file, and any later in-app change supersedes it (Issue #238).
    app.set_scan_mode(crate::settings::resolve_scan_mode(
        app.settings().scan_mode,
        args.scan_mode,
    ));
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    // Initialize update checker
    app.set_update_check_enabled(!args.no_update_check && app.settings().check_updates);
    if app.update_check_enabled() {
        if let Ok(path) = crate::upgrade::state_path() {
            let seen = crate::upgrade::load_state(&path).latest_seen;
            if !seen.is_empty() {
                app.set_update_available(crate::upgrade::is_newer(
                    &seen,
                    env!("CARGO_PKG_VERSION"),
                ));
            }
        }

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let path_opt = crate::upgrade::state_path().ok();
            let due = path_opt.as_ref().is_none_or(|path| {
                crate::upgrade::should_check(
                    crate::upgrade::load_state(path).last_check,
                    crate::upgrade::now_secs(),
                )
            });
            if due {
                let outcome = tokio::task::spawn_blocking(move || {
                    crate::upgrade::check_for_update(
                        &crate::upgrade::UreqClient,
                        env!("CARGO_PKG_VERSION"),
                    )
                })
                .await
                .unwrap_or(crate::upgrade::UpdateCheckOutcome::Failed);
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
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "current".to_string(),
                relative_path: PathBuf::from("current"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
                ..Default::default()
            }],
            is_expanded: true,
            ..Default::default()
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
                    node: Box::new(AlignedNode {
                        name: String::new(),
                        relative_path: PathBuf::from(""),
                        left: None,
                        right: None,
                        state: DiffState::Identical,
                        children: vec![AlignedNode {
                            name: "stale".to_string(),
                            relative_path: PathBuf::from("stale"),
                            left: None,
                            right: None,
                            state: DiffState::Identical,
                            children: vec![],
                            is_expanded: false,
                            ..Default::default()
                        }],
                        is_expanded: true,
                        ..Default::default()
                    }),
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
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "keep-me".to_string(),
                relative_path: PathBuf::from("keep-me"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
                ..Default::default()
            }],
            is_expanded: true,
            ..Default::default()
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
        let (msg, is_error) = app.status_toast().expect("status toast");
        assert!(is_error);
        assert!(msg.contains("permission denied"));
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
    async fn test_palette_filter_action_preserves_committed_pattern() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.filter_mut().set_pattern("readme");
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        // Opening the filter bar from the command palette must behave like the `/`
        // keyboard shortcut (FilterState::open) and preserve the previously
        // committed pattern, not clear it.
        let action_filter = crate::commands::CommandEntry {
            key: "/".to_string(),
            label: "Filter".to_string(),
            command: crate::commands::Command::Filter,
            disabled_reason: None,
        };
        crate::commands::Commands::new(tx)
            .execute(
                &mut app,
                crate::commands::Invocation::Command(action_filter.command),
                &mut terminal,
            )
            .unwrap();

        assert!(app.filter().active());
        assert_eq!(app.filter().input(), "readme");
    }

    #[tokio::test]
    async fn test_run_app_pane_focus_number_keys() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert!(app.active_side_left());

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
        assert!(app.active_side_left());
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
                ..Default::default()
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
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

        assert_eq!(app.selected_idx(), 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Assert that the 'j' key was processed and app moved down
        assert_eq!(app.selected_idx(), 1);
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
                    ..Default::default()
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

        assert_eq!(app.selected_idx(), 0);
        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After one Ctrl+f, selection should have advanced by roughly a page.
        assert!(
            app.selected_idx() > 0,
            "Ctrl+f should page the selection down, got idx {}",
            app.selected_idx()
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
                ..Default::default()
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
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

        assert_eq!(app.selected_idx(), 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx(), 1);
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
                ..Default::default()
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
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

        assert_eq!(app.selected_idx(), 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx(), 1);
    }

    #[tokio::test]
    async fn test_help_index_mouse_click_selects_topic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::Help);
        app.help_mut().set_index_open(true);

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
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Mouse);
        assert!(!app.help().index_open());
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
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "folder".to_string(),
                relative_path: PathBuf::from("folder"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "child".to_string(),
                    relative_path: PathBuf::from("folder/child"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    is_expanded: false,
                    ..Default::default()
                }],
                is_expanded: true,
                ..Default::default()
            }],
            is_expanded: true,
            ..Default::default()
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
            ..Default::default()
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
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
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
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
        // Verify that it did enter FileDiff mode and populated diff().rows()
        assert!(!app.diff().rows().is_empty());
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

    /// The filesystem seam: a scanned subtree copy lands, and a destination
    /// outside the target root is refused.
    #[test]
    fn copy_dir_recursive_copies_a_subtree_and_refuses_to_escape() {
        use std::fs::{read_to_string, write};
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

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

        let outside = left_dir.path().join("outside");
        let err = actions::copy_dir_recursive(&src_sub, &outside, right_dir.path()).unwrap_err();
        assert!(err.to_string().contains("escapes"));
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
            ..Default::default()
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
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));

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
            ..Default::default()
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

        assert_eq!(app.left_path(), PathBuf::from("left"));
        assert_eq!(app.right_path(), PathBuf::from("right"));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Paths should be swapped
        assert_eq!(app.left_path(), PathBuf::from("right"));
        assert_eq!(app.right_path(), PathBuf::from("left"));
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        // Pane content width (38 at 80 columns) comes from `App::sync_viewport`,
        // which `run_app` runs each frame.
        app.diff_mut().set_rows(vec![
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
        ]);

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
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
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
            ..Default::default()
        }]);
        app.apply_filter();

        // Pre-populate diff rows with a long line so horizontal scrolling is meaningful.
        app.diff_mut().set_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
        ))]);

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

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
        assert!(!app.diff().wrap());
        assert_eq!(app.diff().h_scroll(), 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
        assert!(!app.diff().wrap());
        assert_eq!(app.diff().h_scroll(), 0);
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
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_with_contextual_topic_and_return_view() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

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
        assert_eq!(app.help().topic(), crate::app::HelpTopic::FileDiff);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
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
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Config);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::ConfigMenu);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_file_diff_and_returns_on_esc() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

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
        // config().return_view() proves `C` from FileDiff actually opened Config (rather than
        // being ignored as a no-op key), and the final DirectoryTree confirms Esc returned to
        // FileDiff (not stranding on DirectoryTree) before the subsequent q's unwound further.
        assert_eq!(app.config().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_help_and_returns_to_help() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

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
        assert_eq!(app.config().return_view(), crate::app::ViewMode::Help);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
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
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Mouse);
        assert!(!app.help().index_open());
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
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
        assert_eq!(app.help().index_sel(), 3);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
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
        assert_eq!(app.help().index_sel(), 1);
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
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Config);
        assert!(!app.help().index_open());
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
        app.set_view_mode(crate::app::ViewMode::Help);
        app.help_mut()
            .set_return_view(crate::app::ViewMode::DirectoryTree);
        app.help_mut().set_index_open(true);

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
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
        // Verify that index mode was properly closed when exiting Help from index-open state.
        // This assertion independently verifies help_index_open reset without relying on Tab working.
        assert!(!app.help().index_open());
    }
}
