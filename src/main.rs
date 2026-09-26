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
pub mod help;
pub mod ignore;
pub mod input;
pub mod keymap;
pub mod layout;
pub mod scan;
pub mod settings;
pub mod startup;
pub mod target;
#[cfg(test)]
pub mod test_support;
pub mod text_input;
pub mod theme;
pub mod ui;
pub mod upgrade;
pub mod view;
pub mod wrap;

#[derive(Parser, Debug)]
#[command(
    name = "duodiff",
    version,
    about = "A cross-platform TUI for comparing two directories or two files"
)]
struct Args {
    /// Left directory or file to compare
    #[arg(value_name = "LEFT")]
    left: Option<PathBuf>,
    /// Right directory or file to compare
    #[arg(value_name = "RIGHT")]
    right: Option<PathBuf>,
    /// Glob pattern to exclude from comparison. Can be specified multiple times.
    #[arg(short = 'e', long = "exclude", value_name = "PATTERN")]
    exclude: Vec<String>,
    /// Process `.gitignore` files in this session (overrides config until changed in Config).
    #[arg(long, conflicts_with = "no_gitignore")]
    gitignore: bool,
    /// Do not process `.gitignore` files in this session (overrides config until changed in Config).
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
    /// Start this session with mouse support off (overrides `mouse = true` in config.toml until changed in Config)
    #[arg(
        long = "no-mouse",
        help = "Start this session with mouse support off (overrides `mouse = true` in config.toml until changed in Config)"
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
        // Start what the last event asked for before drawing, so the frame
        // already shows a requested scan in flight.
        actions::run_requests::<actions::RealTerminalGuard>(app, &tx);
        // Refresh viewport geometry *before* drawing and before the key/mouse
        // handlers below, so rendering and scroll clamping always agree — and
        // neither reads geometry from the previous terminal size.
        let area = terminal.size()?.into();
        view::prepare_frame(app, area);
        let screen = view::assemble(app);
        terminal.draw(|f| ui::draw(f, &screen))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        input::handle_key_with_commands(key, app, terminal, &mut commands).await?;
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse))
                    if app.settings().mouse() =>
                {
                    input::handle_mouse_with_commands(mouse, app, terminal, &mut commands).await?;
                }
                AppEvent::ScanProgress { generation, count } => {
                    app.apply_scan_progress(generation, count);
                }
                AppEvent::ScanFinished { generation, node } => {
                    app.apply_scan_result(generation, *node);
                }
                AppEvent::Error {
                    generation,
                    message,
                } => {
                    app.apply_scan_error(generation, &message);
                }
                AppEvent::CommandFailed { message } => {
                    app.set_status(message, true);
                }
                AppEvent::Tick => {
                    app.scan_mut().tick();
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

    let startup = crate::startup::Startup::resolve(crate::startup::CliOverrides {
        no_mouse: args.no_mouse,
        scan_mode: args.scan_mode,
        no_update_check: args.no_update_check,
        exclude: args.exclude.clone(),
        gitignore: args
            .gitignore
            .then_some(true)
            .or(args.no_gitignore.then_some(false)),
    });

    if args.check && args.left.is_none() && args.right.is_none() {
        match startup.check_report() {
            Ok(ready) => println!("{ready}"),
            Err(problem) => {
                eprintln!("{problem}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let (Some(left), Some(right)) = (args.left.clone(), args.right.clone()) else {
        let missing = if args.left.is_none() { "LEFT" } else { "RIGHT" };
        eprintln!("Error: Missing {missing} argument.");
        std::process::exit(1);
    };

    let target = match crate::target::resolve(&left, &right) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let check_updates = startup.update_check_due;

    let mut app = match target {
        crate::target::ComparisonTarget::Directories {
            left: left_dir,
            right: right_dir,
        } => {
            let (left_ignore, right_ignore) =
                match startup.ignore_matchers(left_dir.clone(), right_dir.clone()) {
                    Ok(matchers) => matchers,
                    Err(error) => {
                        eprintln!("Invalid exclusion pattern: {error}");
                        std::process::exit(1);
                    }
                };
            App::from_startup(left_dir, right_dir, left_ignore, right_ignore, startup)
        }
        // Exclusion flags only shape a directory scan, so a file pair ignores
        // them rather than failing a shell alias that always passes them.
        crate::target::ComparisonTarget::Files(pair) => {
            let (left, right) = (
                pair.left.path().to_path_buf(),
                pair.right.path().to_path_buf(),
            );
            let no_exclusions = |root: &std::path::Path| {
                crate::ignore::IgnoreMatcher::for_root(root.to_path_buf(), &[], true, &[])
                    .expect("empty ignore matcher is valid")
            };
            let (left_ignore, right_ignore) = (no_exclusions(&left), no_exclusions(&right));
            let mut app = App::from_startup(
                left,
                right,
                left_ignore,
                right_ignore,
                startup.for_file_pair(),
            );
            if let Err(cause) = app.open_file_pair(pair) {
                eprintln!("Error: Cannot open the file diff\nCause: {cause}");
                std::process::exit(1);
            }
            app
        }
    };

    // Initialize terminal safely
    let mut terminal = setup_terminal(app.settings().mouse())?;

    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    // `Startup` read the last check once: its version for the hint, its time
    // for whether a check is due today. A due check refreshes both.
    if check_updates {
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                crate::upgrade::check_for_update(
                    &crate::upgrade::UreqClient,
                    env!("CARGO_PKG_VERSION"),
                )
            })
            .await
            .unwrap_or(crate::upgrade::UpdateCheckOutcome::Failed);
            let _ = tx_clone.send(AppEvent::UpdateCheckOutcome(outcome)).await;
        });
    }

    app.request_rescan();

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
    use crate::test_support::AppHarness;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_start_scan_task() {
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        scan::start_scan_task(
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
        use std::time::SystemTime;

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // Two scan starts → generation 2, still in flight.
        app.scan_mut().begin();
        app.scan_mut().begin();
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
                expanded_by_default: false,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        });
        app.flatten_tree();
        assert_eq!(app.directory_tree().flat_rows()[0].name, "current");

        // Stale generation 1 must not replace the tree.
        AppHarness::new(&mut app)
            .scan_finished(
                1,
                AlignedNode {
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
                        expanded_by_default: false,
                        ..Default::default()
                    }],
                    expanded_by_default: true,
                    ..Default::default()
                },
            )
            .key('q')
            .run()
            .await;
        assert_eq!(app.directory_tree().flat_rows()[0].name, "current");
        assert!(app.scan().in_progress()); // still waiting for generation 2
    }

    #[tokio::test]
    async fn test_scan_error_toasts_and_keeps_running() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use std::time::SystemTime;

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.scan_mut().begin();
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
                expanded_by_default: false,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        });
        app.flatten_tree();

        // A scan error must not exit the app.
        AppHarness::new(&mut app)
            .scan_error(1, "permission denied")
            .key('q')
            .run()
            .await;
        assert!(!app.scan().in_progress());
        assert_eq!(app.directory_tree().flat_rows()[0].name, "keep-me");
        let (msg, is_error) = app.status_toast().expect("status toast");
        assert!(is_error);
        assert!(msg.contains("permission denied"));
    }

    #[tokio::test]
    async fn test_esc_quits_directory_tree() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        AppHarness::new(&mut app)
            .key_code(crossterm::event::KeyCode::Esc)
            .run()
            .await;
    }

    #[tokio::test]
    async fn test_palette_filter_action_preserves_committed_pattern() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_pattern("readme");
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        // Opening the filter bar from the command palette must behave like the `/`
        // keyboard shortcut (DirectoryTreeState::open) and preserve the previously
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

        assert!(app.directory_tree().active());
        assert_eq!(app.directory_tree().input(), "readme");
    }

    #[tokio::test]
    async fn test_run_app_pane_focus_number_keys() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert!(app.active_side_left());

        AppHarness::new(&mut app)
            .key('2')
            .key('1')
            .key('q')
            .run()
            .await;

        assert!(app.active_side_left());
    }

    #[tokio::test]
    async fn test_run_app_keyboard_navigation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(vec![
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

        assert_eq!(app.directory_tree().selected_idx(), 0);

        AppHarness::new(&mut app)
            // 'j' moves down
            .key('j')
            .key('q')
            .run()
            .await;

        // Assert that the 'j' key was processed and app moved down
        assert_eq!(app.directory_tree().selected_idx(), 1);
    }

    #[tokio::test]
    async fn test_run_app_ctrl_page_scroll() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(
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

        assert_eq!(app.directory_tree().selected_idx(), 0);
        AppHarness::new(&mut app)
            .key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('f'),
                crossterm::event::KeyModifiers::CONTROL,
            ))
            .key('q')
            .run()
            .await;
        // After one Ctrl+f, selection should have advanced by roughly a page.
        assert!(
            app.directory_tree().selected_idx() > 0,
            "Ctrl+f should page the selection down, got idx {}",
            app.directory_tree().selected_idx()
        );
    }

    #[tokio::test]
    async fn test_run_app_mouse_navigation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(vec![
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

        assert_eq!(app.directory_tree().selected_idx(), 0);

        AppHarness::new(&mut app)
            // Scroll down
            .mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            })
            .key('q')
            .run()
            .await;

        assert_eq!(app.directory_tree().selected_idx(), 1);
    }

    #[tokio::test]
    async fn test_run_app_mouse_click_navigation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(vec![
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

        assert_eq!(app.directory_tree().selected_idx(), 0);

        AppHarness::new(&mut app)
            // Click on the second row (click_y = 3, which maps to index 3 - 2 = 1)
            .mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 3,
                modifiers: crossterm::event::KeyModifiers::empty(),
            })
            .key('q')
            .run()
            .await;

        assert_eq!(app.directory_tree().selected_idx(), 1);
    }

    #[tokio::test]
    async fn test_help_index_mouse_click_selects_topic() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::Help);
        app.help_mut().set_index_open(true);

        AppHarness::new(&mut app)
            // Click on the 4th item (click_y = 5, maps to index 5 - 2 = 3 which is HelpTopic::Mouse)
            .mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            })
            // Exit help topic view, then quit from directory tree.
            .key('q')
            .key('q')
            .run()
            .await;
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Mouse);
        assert!(!app.help().index_open());
    }

    #[tokio::test]
    async fn test_run_app_keyboard_expand_collapse() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use std::time::SystemTime;

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
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(node);

        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        AppHarness::new(&mut app)
            // Select root (idx = 0) and collapse it using 'h'
            .key('h')
            // Expand it using 'Right' key
            .key_code(crossterm::event::KeyCode::Right)
            .key('q')
            .run()
            .await;

        // Since it was collapsed and expanded, flat_rows should be 2 again
        assert_eq!(app.directory_tree().flat_rows().len(), 2);
    }

    #[tokio::test]
    async fn test_run_app_file_diff_navigation() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        // Initially in DirectoryTree mode
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));

        AppHarness::new(&mut app)
            // Press Enter to go to FileDiff mode
            .key_code(crossterm::event::KeyCode::Enter)
            // Scroll down
            .key('j')
            // Scroll up
            .key('k')
            // Press Esc to exit FileDiff mode
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;

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

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        AppHarness::new(&mut app)
            // Press 'D' to launch diff tool
            .key('D')
            .key('q')
            .run()
            .await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_run_app_keyboard_editor_launch() {
        use crate::diff::FileInfo;
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

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        AppHarness::new(&mut app)
            // Press 'E' to launch editor
            .key('E')
            .key('q')
            .run()
            .await;
    }

    #[tokio::test]
    async fn test_run_app_mouse_double_click_enters_diff() {
        use std::fs;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        fs::write(left_dir.path().join("file.txt"), "hello").unwrap();
        fs::write(right_dir.path().join("file.txt"), "world").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));

        AppHarness::new(&mut app)
            // First click
            .mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            })
            // Second click immediately
            .mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            })
            // Wait, then quit
            .wait_ms(50)
            .key('q')
            // Send a second 'q' to quit the app from DirectoryTree mode
            .wait_ms(50)
            .key('q')
            .run()
            .await;

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
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        write(left_dir.path().join("file.txt"), "left content").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        // Run the event loop
        AppHarness::new(&mut app)
            // First enter Diff View by pressing Enter
            .key_code(crossterm::event::KeyCode::Enter)
            // Wait, then press 'R' to copy left to right
            .wait_ms(50)
            .key('R')
            // Wait, then press 'y' to confirm copy
            .wait_ms(50)
            .key('y')
            // Wait, then quit TUI
            .wait_ms(50)
            .key('q')
            .run()
            .await;

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
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            }]);
        app.apply_filter();

        assert_eq!(app.left_path(), PathBuf::from("left"));
        assert_eq!(app.right_path(), PathBuf::from("right"));

        // Swap, wait for the rescan it starts, then quit.
        AppHarness::new(&mut app)
            .key('s')
            .wait_ms(100)
            .key('q')
            .run()
            .await;

        // Paths should be swapped
        assert_eq!(app.left_path(), PathBuf::from("right"));
        assert_eq!(app.right_path(), PathBuf::from("left"));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_change_navigation() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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
        // Pane content width (38 at 80 columns) comes from `view::prepare_frame`,
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

        AppHarness::new(&mut app)
            .key('N')
            .key('N')
            .key('P')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_wrap_and_horizontal_scroll() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![crate::app::FlatRow {
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

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
        assert!(!app.diff().wrap());
        assert_eq!(app.diff().h_scroll(), 0);

        AppHarness::new(&mut app)
            // Enter FileDiff mode
            .key_code(crossterm::event::KeyCode::Enter)
            // Toggle wrap mode on
            .key('w')
            // Toggle wrap mode off
            .key('w')
            // Scroll right horizontally
            .key_code(crossterm::event::KeyCode::Right)
            // Scroll left horizontally
            .key_code(crossterm::event::KeyCode::Left)
            // Exit FileDiff and quit
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;

        assert!(matches!(
            app.view_mode(),
            crate::app::ViewMode::DirectoryTree
        ));
        assert!(!app.diff().wrap());
        assert_eq!(app.diff().h_scroll(), 0);
    }

    #[tokio::test]
    async fn test_help_opens_from_directory_tree_and_returns_on_esc() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        AppHarness::new(&mut app)
            .key('?')
            .key_code(crossterm::event::KeyCode::Esc)
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_with_contextual_topic_and_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

        // Open Help from FileDiff, then unwind back to DirectoryTree to quit:
        // Esc (Help -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
        AppHarness::new(&mut app)
            .key('?')
            .key_code(crossterm::event::KeyCode::Esc)
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .key('q')
            .run()
            .await;
        // help_topic/help_return_view were set correctly when `?` was pressed from
        // FileDiff, and are still holding those values after the full unwind.
        assert_eq!(app.help().topic(), crate::app::HelpTopic::FileDiff);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_from_config_and_returns_to_directory_tree() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_config();

        // ? (-> Help) -> Esc (-> Config) -> q (-> DirectoryTree) -> q (break)
        AppHarness::new(&mut app)
            .key('?')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .key('q')
            .run()
            .await;
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Config);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::ConfigMenu);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_file_diff_and_returns_on_esc() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

        // C (FileDiff -> Config) -> Esc (Config -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
        AppHarness::new(&mut app)
            .key('C')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .key('q')
            .run()
            .await;
        // config().return_view() proves `C` from FileDiff actually opened Config (rather than
        // being ignored as a no-op key), and the final DirectoryTree confirms Esc returned to
        // FileDiff (not stranding on DirectoryTree) before the subsequent q's unwound further.
        assert_eq!(app.config().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_config_hotkey_opens_from_help_and_returns_to_help() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);

        // ? (FileDiff -> Help) -> C (Help -> Config) -> Esc (Config -> Help) ->
        // Esc (Help -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
        AppHarness::new(&mut app)
            .key('?')
            .key('C')
            .key_code(crossterm::event::KeyCode::Esc)
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .key('q')
            .run()
            .await;
        assert_eq!(app.config().return_view(), crate::app::ViewMode::Help);
        assert_eq!(app.help().return_view(), crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_digit_key_jumps_topic_without_opening_index() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // ? (Help, topic=DirectoryTree) -> '4' (topic=Mouse) -> Esc -> q
        AppHarness::new(&mut app)
            .key('?')
            .key('4')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Mouse);
        assert!(!app.help().index_open());
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_tab_opens_index_at_current_topic_position() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // ? -> '4' (jump to Mouse, pos 3) -> Tab (open index at sel=3) -> Esc -> q
        // Tests that Tab correctly maps current topic to its position in the index
        AppHarness::new(&mut app)
            .key('?')
            .key('4')
            .key_code(crossterm::event::KeyCode::Tab)
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        // After jumping to '4' (Mouse at position 3) and pressing Tab, index should open at sel=3
        assert_eq!(app.help().index_sel(), 3);
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_index_navigation_wraps_both_directions() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // Up-wrap: ? -> Tab (index open, sel=0) -> k (wraps to sel=4)
        // Down-wrap: j (wraps back from sel=4 to sel=0) -> j (sel=0 to sel=1) -> Esc -> q
        // This final j movement to sel=1 only happens if k/j navigation works;
        // it's a genuinely falsifiable assertion (would fail under old flat-match code).
        AppHarness::new(&mut app)
            .key('?')
            .key_code(crossterm::event::KeyCode::Tab)
            .key('k')
            .key('j')
            .key('j')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        // After 'k' from sel=0, wraps to sel=4 (up wraps to end)
        // After 'j' from sel=4, wraps back to sel=0 (down wraps to start)
        // After 'j' from sel=0, moves to sel=1 (normal forward move)
        // Only the current implementation produces sel=1; old flat-match code never navigates, stays at 0
        assert_eq!(app.help().index_sel(), 1);
    }

    #[tokio::test]
    async fn test_help_index_digit_selects_topic_and_closes_index() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // ? -> Tab (open index) -> '3' (select Config, index at position 2) -> Esc -> q
        AppHarness::new(&mut app)
            .key('?')
            .key_code(crossterm::event::KeyCode::Tab)
            .key('3')
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        assert_eq!(app.help().topic(), crate::app::HelpTopic::Config);
        assert!(!app.help().index_open());
    }

    #[tokio::test]
    async fn test_help_esc_from_open_index_exits_help_entirely() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // Directly seed app into (Help, index open) state, bypassing Tab key processing.
        // This isolates the test to verify Esc handler's help_index_open reset logic.
        // Under old flat-match code, Esc wouldn't reset help_index_open (only view_mode),
        // making assert!(!help_index_open) genuinely fail (RED).
        app.set_view_mode(crate::app::ViewMode::Help);
        app.help_mut()
            .set_return_view(crate::app::ViewMode::DirectoryTree);
        app.help_mut().set_index_open(true);

        // Esc (from index-open Help, should reset help_index_open) -> q (break)
        AppHarness::new(&mut app)
            .key_code(crossterm::event::KeyCode::Esc)
            .key('q')
            .run()
            .await;
        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
        // Verify that index mode was properly closed when exiting Help from index-open state.
        // This assertion independently verifies help_index_open reset without relying on Tab working.
        assert!(!app.help().index_open());
    }

    /// Direct file comparison (Issue #327): the session opens on one file pair
    /// with no Directory Tree behind it.
    /// Issue #339: `--check` lists every ignored `[keys]` entry, one per line.
    #[test]
    fn check_lists_every_ignored_key_binding() {
        let problems = [
            "keys.bogus: no command is named `bogus`".to_string(),
            "keys.help: `j` is handled by Directory Tree itself and cannot be bound".to_string(),
        ];
        assert_eq!(
            crate::startup::key_problem(&problems).unwrap().report(),
            "Error: Some key bindings in the config file were ignored\n\
             Cause: keys.bogus: no command is named `bogus`\n\
             Cause: keys.help: `j` is handled by Directory Tree itself and cannot be bound\n\
             Next: Fix those [keys] entries; ignored commands keep their default keys"
        );
    }

    /// Issue #342: `--check` fails on a config file the next run could not use.
    #[test]
    fn check_reports_a_broken_config_file() {
        let problem = crate::startup::config_problem(&crate::settings::LoadError {
            path: PathBuf::from("/cfg/config.toml"),
            cause: "line 2: unknown variant `blue`".to_string(),
        })
        .report();
        assert_eq!(
            problem.lines().collect::<Vec<_>>(),
            [
                "Error: Cannot load the config file",
                &format!(
                    "Cause: {}: line 2: unknown variant `blue`",
                    App::display_path_with_home_tilde(&PathBuf::from("/cfg/config.toml"))
                ),
                "Next: Fix the file; until then duodiff uses the defaults and does not save settings",
            ]
        );
    }

    /// A Settings change that needs a rescan gets exactly one, started by
    /// the event loop rather than by the key that made the change.
    #[tokio::test]
    async fn a_scan_mode_switch_starts_one_scan() {
        let dir = tempdir().unwrap();
        let mut app = App::new(dir.path().join("left"), dir.path().join("right"));

        AppHarness::new(&mut app).key('c').key('q').run().await;

        assert_eq!(app.scan().generation(), 1);
        assert!(app.requests().is_empty());
    }

    mod file_comparison {
        use super::*;
        use std::fs;

        /// An `App` opened on `left` and `right` written into a temp dir, the
        /// way `main` opens it for two file arguments.
        fn open_pair(left: &str, right: &str) -> (tempfile::TempDir, App) {
            let dir = tempdir().unwrap();
            let left_path = dir.path().join("left.txt");
            let right_path = dir.path().join("right.txt");
            fs::write(&left_path, left).unwrap();
            fs::write(&right_path, right).unwrap();
            let app = open_resolved(&left_path, &right_path);
            (dir, app)
        }

        fn open_resolved(left: &std::path::Path, right: &std::path::Path) -> App {
            let crate::target::ComparisonTarget::Files(pair) =
                crate::target::resolve(left, right).unwrap()
            else {
                panic!("expected a file pair");
            };
            let mut app = App::new(left.to_path_buf(), right.to_path_buf());
            app.open_file_pair(pair).unwrap();
            app
        }

        /// Run the script, failing instead of hanging when it never quits.
        async fn run(harness: AppHarness<'_>) {
            tokio::time::timeout(Duration::from_secs(5), harness.run())
                .await
                .expect("the session should have ended");
        }

        /// Run a script that leaves the session open, then stop the loop.
        async fn run_without_quitting(harness: AppHarness<'_>) {
            let result = tokio::time::timeout(Duration::from_millis(300), harness.run()).await;
            assert!(
                result.is_err(),
                "the session ended, but it should stay open"
            );
        }

        #[tokio::test]
        async fn back_quits_a_file_diff_opened_directly() {
            let (_dir, mut app) = open_pair("a\n", "b\n");
            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);

            run(AppHarness::new(&mut app).key_code(crossterm::event::KeyCode::Esc)).await;

            assert!(app.should_quit());
        }

        #[tokio::test]
        async fn back_with_staged_changes_asks_first_and_cancel_stays() {
            let (dir, mut app) = open_pair("a\n", "b\n");

            run(AppHarness::new(&mut app)
                .key(']')
                .key_code(crossterm::event::KeyCode::Esc)
                .key('c')
                .key_code(crossterm::event::KeyCode::Esc)
                .key('d'))
            .await;

            assert!(app.should_quit());
            assert_eq!(
                fs::read_to_string(dir.path().join("right.txt")).unwrap(),
                "b\n",
                "discarding must not write the staged change"
            );
        }

        #[tokio::test]
        async fn cancel_keeps_the_file_diff_open() {
            let (_dir, mut app) = open_pair("a\n", "b\n");

            run_without_quitting(
                AppHarness::new(&mut app)
                    .key(']')
                    .key_code(crossterm::event::KeyCode::Esc)
                    .key('c'),
            )
            .await;

            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
            assert!(app.diff().is_dirty());
        }

        #[tokio::test]
        async fn save_writes_the_staged_side_without_starting_a_scan() {
            let (dir, mut app) = open_pair("a\n", "b\n");

            run_without_quitting(AppHarness::new(&mut app).key(']').key('s').key('s')).await;

            assert_eq!(
                fs::read_to_string(dir.path().join("right.txt")).unwrap(),
                "a\n"
            );
            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
            assert!(!app.diff().is_dirty());
            assert_eq!(app.scan().generation(), 0, "no directory scan may start");
        }

        #[tokio::test]
        async fn a_config_change_does_not_start_a_scan() {
            let (_dir, mut app) = open_pair("a\n", "b\n");
            let before = app.settings().scan_mode();
            app.open_config();
            let scan_mode_row = app
                .config_rows()
                .iter()
                .position(|row| *row == crate::app::ConfigRowKind::ScanMode)
                .unwrap();
            assert!(app.config_select_at(scan_mode_row));

            run_without_quitting(
                AppHarness::new(&mut app).key_code(crossterm::event::KeyCode::Enter),
            )
            .await;

            assert_ne!(
                app.settings().scan_mode(),
                before,
                "the scan mode row should have switched"
            );
            assert_eq!(app.scan().generation(), 0, "no directory scan may start");
        }

        #[tokio::test]
        async fn copy_replaces_the_other_file_and_stays_on_the_file_diff() {
            let (dir, mut app) = open_pair("a\n", "b\n");

            run_without_quitting(AppHarness::new(&mut app).key('R').key('y')).await;

            assert_eq!(
                fs::read_to_string(dir.path().join("right.txt")).unwrap(),
                "a\n"
            );
            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
            assert!(
                !app.diff().has_changes(),
                "the reloaded pair should be identical"
            );
            assert_eq!(app.status_toast(), Some(("Copied 'left.txt'", false)));
            assert_eq!(app.scan().generation(), 0, "no directory scan may start");
        }

        #[tokio::test]
        async fn staging_into_a_read_only_side_is_refused_with_the_reason() {
            let dir = tempdir().unwrap();
            let added = dir.path().join("added.txt");
            fs::write(&added, "new\n").unwrap();
            let mut app = open_resolved(std::path::Path::new("/dev/null"), &added);

            run_without_quitting(AppHarness::new(&mut app).key('[')).await;

            assert!(!app.diff().is_dirty());
            assert_eq!(
                app.status_toast(),
                Some((
                    "Stage the change block to the left: the left side is read-only",
                    false
                ))
            );
        }

        #[tokio::test]
        async fn copying_onto_a_read_only_side_is_refused_with_the_reason() {
            let dir = tempdir().unwrap();
            let added = dir.path().join("added.txt");
            fs::write(&added, "new\n").unwrap();
            let mut app = open_resolved(std::path::Path::new("/dev/null"), &added);

            run_without_quitting(AppHarness::new(&mut app).key('L')).await;

            assert!(app.confirm_modal().is_none());
            assert_eq!(
                app.status_toast(),
                Some((
                    "Copy the whole right file to the left: the left side is read-only",
                    false
                ))
            );
            assert_eq!(fs::read_to_string(&added).unwrap(), "new\n");
        }

        /// Draw one frame the way `run_app` does and return it row by row.
        fn render(app: &mut App) -> String {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 20)).unwrap();
            let area = terminal.size().unwrap().into();
            view::prepare_frame(app, area);
            let screen = view::assemble(app);
            terminal.draw(|f| ui::draw(f, &screen)).unwrap();
            let buffer = terminal.backend().buffer();
            buffer
                .content
                .chunks(buffer.area.width as usize)
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        }

        #[test]
        fn identical_files_open_the_file_diff_and_say_so() {
            let dir = tempdir().unwrap();
            let left = dir.path().join("same.txt");
            let right = dir.path().join("copy.txt");
            fs::write(&left, "same\n").unwrap();
            fs::write(&right, "same\n").unwrap();
            let mut app = open_resolved(&left, &right);

            let frame = render(&mut app);

            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
            assert!(frame.contains("Both files are identical"), "{frame}");
            assert!(frame.contains("same.txt"), "{frame}");
            assert!(frame.contains("copy.txt"), "{frame}");
        }

        #[test]
        fn a_read_only_side_is_labelled_in_its_pane_title() {
            let dir = tempdir().unwrap();
            let added = dir.path().join("added.txt");
            fs::write(&added, "new\n").unwrap();
            let mut app = open_resolved(std::path::Path::new("/dev/null"), &added);

            let frame = render(&mut app);
            let title = frame
                .lines()
                .find(|line| line.contains("[1]"))
                .unwrap_or_else(|| panic!("no pane title in\n{frame}"));

            assert!(title.contains("/dev/null read-only"), "{title}");
            assert_eq!(frame.matches("read-only").count(), 1, "{frame}");
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn saving_a_symlinked_side_writes_through_the_link() {
            let dir = tempdir().unwrap();
            let real = dir.path().join("real.txt");
            let link = dir.path().join("link.txt");
            let other = dir.path().join("other.txt");
            fs::write(&real, "old\n").unwrap();
            fs::write(&other, "new\n").unwrap();
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let mut app = open_resolved(&link, &other);

            run_without_quitting(AppHarness::new(&mut app).key('[').key('s').key('s')).await;

            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the link must survive the save"
            );
            assert_eq!(fs::read_to_string(&real).unwrap(), "new\n");
        }

        #[tokio::test]
        async fn copying_from_the_null_device_is_refused() {
            let dir = tempdir().unwrap();
            let deleted = dir.path().join("deleted.txt");
            fs::write(&deleted, "old\n").unwrap();
            let mut app = open_resolved(&deleted, std::path::Path::new("/dev/null"));

            run_without_quitting(AppHarness::new(&mut app).key('L')).await;

            assert!(app.confirm_modal().is_none());
            assert_eq!(
                app.status_toast(),
                Some((
                    "Copy the whole right file to the left: nothing on the right side to copy",
                    false
                ))
            );
            assert_eq!(fs::read_to_string(&deleted).unwrap(), "old\n");
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn copying_from_a_pipe_writes_what_was_read() {
            let dir = tempdir().unwrap();
            let piped = crate::test_support::fifo_with(dir.path(), "piped", "from a pipe\n");
            let target = dir.path().join("target.txt");
            fs::write(&target, "old\n").unwrap();
            let mut app = open_resolved(&piped, &target);

            run_without_quitting(AppHarness::new(&mut app).key('R').key('y')).await;

            assert_eq!(fs::read_to_string(&target).unwrap(), "from a pipe\n");
            assert!(!app.diff().has_changes());
        }
    }
}
