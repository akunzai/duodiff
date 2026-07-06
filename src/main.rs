use clap::Parser;
use std::path::PathBuf;
use std::time::Duration;
use event::{AppEvent, EventHandler};
use app::App;

pub mod diff;
pub mod app;
pub mod event;

#[derive(Parser, Debug)]
#[command(name = "duodiff", about = "A cross-platform TUI directory comparison tool")]
struct Args {
    /// Left directory path to compare
    left_dir: PathBuf,
    /// Right directory path to compare
    right_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if !args.left_dir.is_dir() || !args.right_dir.is_dir() {
        eprintln!("Both arguments must be valid directories.");
        std::process::exit(1);
    }

    let mut app = App::new(args.left_dir.clone(), args.right_dir.clone());
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    app.scan_in_progress = true;
    start_scan_task(args.left_dir, args.right_dir, app.precise_mode, tx.clone());

    println!("Scanning directories in background...");
    
    // Basic event processing verification
    let mut loops = 0;
    while let Some(event) = events.next().await {
        match event {
            AppEvent::ScanFinished(node) => {
                app.root_node = Some(node);
                app.scan_in_progress = false;
                app.flatten_tree();
                println!("Scan completed successfully! Loaded {} rows.", app.flat_rows.len());
                break;
            }
            AppEvent::Error(err) => {
                eprintln!("Scan error occurred: {}", err);
                std::process::exit(1);
            }
            AppEvent::Tick => {
                loops += 1;
                if loops > 20 { // Timeout check for verification
                    eprintln!("Scan timeout reached.");
                    break;
                }
            }
            _ => {}
        }
    }
    Ok(())
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

