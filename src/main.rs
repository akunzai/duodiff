use clap::Parser;
use std::path::PathBuf;

pub mod diff;

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
    if !args.left_dir.is_dir() {
        eprintln!("Error: Left path is not a directory: {:?}", args.left_dir);
        std::process::exit(1);
    }
    if !args.right_dir.is_dir() {
        eprintln!("Error: Right path is not a directory: {:?}", args.right_dir);
        std::process::exit(1);
    }
    println!("Comparing {:?} and {:?}", args.left_dir, args.right_dir);
    Ok(())
}
