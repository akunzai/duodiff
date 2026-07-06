use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use event::{AppEvent, EventHandler};
use app::App;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

pub mod diff;
pub mod app;
pub mod event;
pub mod ui;
pub mod diff_view;
pub mod editor;

#[derive(Parser, Debug)]
#[command(name = "duodiff", about = "A cross-platform TUI directory comparison tool")]
struct Args {
    left_dir: PathBuf,
    right_dir: PathBuf,
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    events: &mut EventHandler,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        use crossterm::event::KeyCode;
                        match app.view_mode {
                            app::ViewMode::DirectoryTree => {
                                match key.code {
                                    KeyCode::Char('q') => break,
                                    KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                                    KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                                    KeyCode::Char(' ') => app.toggle_expand(),
                                    KeyCode::Char('h') | KeyCode::Left => app.collapse_selected(),
                                    KeyCode::Char('l') | KeyCode::Right => app.expand_selected(),
                                    KeyCode::Tab => app.active_side_left = !app.active_side_left,
                                    KeyCode::Char('c') => {
                                        app.precise_mode = !app.precise_mode;
                                        app.scan_in_progress = true;
                                        start_scan_task(app.left_path.clone(), app.right_path.clone(), app.precise_mode, tx.clone());
                                    }
                                    KeyCode::Char('r') => {
                                        app.scan_in_progress = true;
                                        start_scan_task(app.left_path.clone(), app.right_path.clone(), app.precise_mode, tx.clone());
                                    }
                                    KeyCode::Char('e') if app.selected_idx < app.flat_rows.len() => {
                                        let row = &app.flat_rows[app.selected_idx];
                                        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                                            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                                        if !is_dir {
                                            let target_file = if app.active_side_left {
                                                if row.left.is_some() {
                                                    app.left_path.join(&row.relative_path)
                                                } else {
                                                    app.right_path.join(&row.relative_path)
                                                }
                                            } else {
                                                if row.right.is_some() {
                                                    app.right_path.join(&row.relative_path)
                                                } else {
                                                    app.left_path.join(&row.relative_path)
                                                }
                                            };

                                            use std::io::IsTerminal;
                                            let is_terminal = std::io::stdout().is_terminal();
                                            if is_terminal {
                                                disable_raw_mode()?;
                                                execute!(
                                                    std::io::stdout(),
                                                    LeaveAlternateScreen,
                                                    crossterm::event::DisableMouseCapture
                                                )?;
                                            }

                                            let res = editor::open_editor(&target_file);
                                            if let Err(e) = res {
                                                eprintln!("Error launching editor: {}. Press Enter to continue...", e);
                                                let mut buf = String::new();
                                                let _ = std::io::stdin().read_line(&mut buf);
                                            }

                                            if is_terminal {
                                                enable_raw_mode()?;
                                                execute!(
                                                    std::io::stdout(),
                                                    EnterAlternateScreen,
                                                    crossterm::event::EnableMouseCapture
                                                )?;
                                            }
                                            terminal.clear()?;

                                            app.scan_in_progress = true;
                                            start_scan_task(app.left_path.clone(), app.right_path.clone(), app.precise_mode, tx.clone());
                                        }
                                    }
                                    KeyCode::Enter if app.selected_idx < app.flat_rows.len() => {
                                        let row = &app.flat_rows[app.selected_idx];
                                        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                                            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                                        if !is_dir {
                                            let left_file = app.left_path.join(&row.relative_path);
                                            let right_file = app.right_path.join(&row.relative_path);
                                            app.diff_rows = crate::diff_view::compare_files(&left_file, &right_file).unwrap_or_default();
                                            app.view_mode = app::ViewMode::FileDiff;
                                            app.diff_scroll = 0;
                                        } else {
                                            app.toggle_expand();
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            app::ViewMode::FileDiff => {
                                match key.code {
                                    KeyCode::Esc | KeyCode::Char('q') => {
                                        app.view_mode = app::ViewMode::DirectoryTree;
                                    }
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        let max_scroll = app.diff_rows.len().saturating_sub(app.visible_height);
                                        if app.diff_scroll < max_scroll {
                                            app.diff_scroll += 1;
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Up if app.diff_scroll > 0 => {
                                        app.diff_scroll -= 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse)) => {
                    use crossterm::event::MouseEventKind;
                    match app.view_mode {
                        app::ViewMode::DirectoryTree => {
                            match mouse.kind {
                                MouseEventKind::ScrollDown => app.select_next(),
                                MouseEventKind::ScrollUp => app.select_prev(),
                                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                    let click_y = mouse.row as usize;
                                    if click_y >= 4 {
                                        let offset_y = click_y - 4;
                                        if offset_y < app.visible_height {
                                            let idx = app.scroll_offset + offset_y;
                                            if idx < app.flat_rows.len() {
                                                app.selected_idx = idx;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        app::ViewMode::FileDiff => {
                            match mouse.kind {
                                MouseEventKind::ScrollDown => {
                                    let max_scroll = app.diff_rows.len().saturating_sub(app.visible_height);
                                    if app.diff_scroll < max_scroll {
                                        app.diff_scroll += 1;
                                    }
                                }
                                MouseEventKind::ScrollUp => {
                                    if app.diff_scroll > 0 {
                                        app.diff_scroll -= 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                AppEvent::ScanFinished(node) => {
                    app.root_node = Some(node);
                    app.scan_in_progress = false;
                    app.flatten_tree();
                }
                AppEvent::Error(err) => {
                    return Err(err.into());
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
    if !args.left_dir.is_dir() || !args.right_dir.is_dir() {
        eprintln!("Both arguments must be valid directories.");
        std::process::exit(1);
    }

    // Initialize terminal safely
    let mut terminal = setup_terminal()?;

    let mut app = App::new(args.left_dir.clone(), args.right_dir.clone());
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    app.scan_in_progress = true;
    start_scan_task(args.left_dir.clone(), args.right_dir.clone(), app.precise_mode, tx.clone());

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

fn setup_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    if let Err(err) = execute!(stdout, EnterAlternateScreen, crossterm::event::EnableMouseCapture) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(t) => Ok(t),
        Err(err) => {
            let _ = execute!(std::io::stdout(), LeaveAlternateScreen, crossterm::event::DisableMouseCapture);
            let _ = disable_raw_mode();
            Err(err.into())
        }
    }
}

fn start_scan_task(left: PathBuf, right: PathBuf, precise: bool, tx: tokio::sync::mpsc::Sender<crate::event::AppEvent>) {
    tokio::spawn(async move {
        let root = tokio::task::spawn_blocking(move || {
            crate::diff::align_directories(&left, &right, std::path::Path::new(""), precise)
        }).await;

        match root {
            Ok(Ok(node)) => {
                let _ = tx.send(crate::event::AppEvent::ScanFinished(node)).await;
            }
            Ok(Err(err)) => {
                let _ = tx.send(crate::event::AppEvent::Error(err.to_string())).await;
            }
            Err(err) => {
                let _ = tx.send(crate::event::AppEvent::Error(err.to_string())).await;
            }
        }
    });
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::time::Duration;
    use crate::event::AppEvent;

    #[tokio::test]
    async fn test_start_scan_task() {
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        start_scan_task(left_dir.path().to_path_buf(), right_dir.path().to_path_buf(), false, tx);
        
        let res = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        let opt = res.expect("Timeout waiting for scan result");
        let event = opt.expect("Expected Some(AppEvent::ScanFinished), got None");
        assert!(matches!(event, AppEvent::ScanFinished(_)), "Expected AppEvent::ScanFinished");
    }

    #[tokio::test]
    async fn test_run_app_keyboard_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
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
        ];
        
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
    async fn test_run_app_mouse_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
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
        ];
        
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
        app.flat_rows = vec![
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
        ];
        
        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the second row (click_y = 5, which maps to index 5 - 4 = 1)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
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
    async fn test_run_app_keyboard_expand_collapse() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::diff::{AlignedNode, FileInfo, DiffState};
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
            children: vec![
                AlignedNode {
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
                }
            ],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();

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

        assert_eq!(app.flat_rows.len(), 2);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Since it was collapsed and expanded, flat_rows should be 2 again
        assert_eq!(app.flat_rows.len(), 2);
    }

    #[tokio::test]
    async fn test_run_app_file_diff_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
            crate::app::FlatRow {
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
            },
        ];

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
    async fn test_run_app_keyboard_editor_launch() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::diff::FileInfo;
        use std::time::SystemTime;
        use tempfile::tempdir;

        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(left_dir.path().to_path_buf(), right_dir.path().to_path_buf());
        app.flat_rows = vec![
            crate::app::FlatRow {
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
            },
        ];

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 'e' to launch editor
            let e_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('e'),
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
}
