use std::process::Command;
use std::path::Path;

pub fn open_editor(file_path: &Path) -> Result<(), std::io::Error> {
    let editor_var = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    let parts: Vec<&str> = editor_var.split_whitespace().collect();
    if parts.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "EDITOR is empty"));
    }
    let mut command = Command::new(parts[0]);
    for arg in &parts[1..] {
        command.arg(arg);
    }
    command.arg(file_path);
    let mut child = command.spawn()?;
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_editor_success() {
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");
        let result = open_editor(Path::new("dummy"));
        assert!(result.is_ok());
    }
}
