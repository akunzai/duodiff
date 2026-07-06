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
    _tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        if key.code == crossterm::event::KeyCode::Char('q') {
                            break;
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
    if let Err(err) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(t) => Ok(t),
        Err(err) => {
            let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
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
}

