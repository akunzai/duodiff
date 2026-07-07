use std::path::Path;
use std::process::Command;

pub static TEST_MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[derive(Clone, Debug, PartialEq)]
pub enum ExternalDiffTool {
    Vim,
    Nvim,
    Code,
    Meld,
    BeyondCompare,
    SublimeMerge,
    Kaleidoscope,
    Difftastic,
}

impl ExternalDiffTool {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vim => "vim",
            Self::Nvim => "nvim",
            Self::Code => "code",
            Self::Meld => "meld",
            Self::BeyondCompare => "bcomp",
            Self::SublimeMerge => "smerge",
            Self::Kaleidoscope => "ksdiff",
            Self::Difftastic => "difft",
        }
    }
}

impl std::str::FromStr for ExternalDiffTool {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "vim" => Ok(Self::Vim),
            "nvim" => Ok(Self::Nvim),
            "code" => Ok(Self::Code),
            "meld" => Ok(Self::Meld),
            "bcomp" | "beyondcompare" => Ok(Self::BeyondCompare),
            "smerge" | "sublimemerge" => Ok(Self::SublimeMerge),
            "ksdiff" | "kaleidoscope" => Ok(Self::Kaleidoscope),
            "difft" | "difftastic" => Ok(Self::Difftastic),
            _ => Err(()),
        }
    }
}

impl ExternalDiffTool {
    pub fn diff_args(&self) -> &'static [&'static str] {
        match self {
            Self::Vim => &["-d"],
            Self::Nvim => &["-d"],
            Self::Code => &["--diff"],
            Self::Meld => &[],
            Self::BeyondCompare => &[],
            Self::SublimeMerge => &["diff"],
            Self::Kaleidoscope => &[],
            Self::Difftastic => &[],
        }
    }
}

pub fn find_in_path(cmd: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for p in std::env::split_paths(&path) {
            let exe_path = p.join(cmd);
            #[cfg(windows)]
            let exe_path = if exe_path.extension().is_none() {
                p.join(format!("{}.exe", cmd))
            } else {
                exe_path
            };
            if exe_path.exists() && exe_path.is_file() {
                return true;
            }
        }
    }
    false
}

pub fn detect_diff_tools() -> Vec<(ExternalDiffTool, bool)> {
    vec![
        (ExternalDiffTool::Vim, find_in_path("vim")),
        (ExternalDiffTool::Nvim, find_in_path("nvim")),
        (ExternalDiffTool::Code, find_in_path("code")),
        (ExternalDiffTool::Meld, find_in_path("meld")),
        (ExternalDiffTool::BeyondCompare, find_in_path("bcomp")),
        (ExternalDiffTool::SublimeMerge, find_in_path("smerge")),
        (ExternalDiffTool::Kaleidoscope, find_in_path("ksdiff")),
        (ExternalDiffTool::Difftastic, find_in_path("difft")),
    ]
}

pub fn open_diff(
    tool: &ExternalDiffTool,
    left_path: &Path,
    right_path: &Path,
) -> Result<(), std::io::Error> {
    let mut command = Command::new(tool.as_str());
    for arg in tool.diff_args() {
        command.arg(arg);
    }
    command.arg(left_path);
    command.arg(right_path);

    let mut child = command
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    child.wait()?;
    Ok(())
}

// Keep the compatibility for open_editor as it might be used elsewhere (like editing single files)
pub fn open_editor(file_path: &Path) -> Result<(), std::io::Error> {
    let editor_var = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string());
    let parts: Vec<&str> = editor_var.split_whitespace().collect();
    if parts.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "VISUAL or EDITOR is empty",
        ));
    }
    let mut command = Command::new(parts[0]);
    for arg in &parts[1..] {
        command.arg(arg);
    }
    command.arg(file_path);
    let mut child = command
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_editor_success() {
        let _guard = TEST_MUTEX
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");
        let result = open_editor(Path::new("dummy"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_open_editor_visual_preference() {
        let _guard = TEST_MUTEX
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();
        #[cfg(not(target_os = "windows"))]
        {
            std::env::set_var("VISUAL", "true");
            std::env::set_var("EDITOR", "non_existent_command_xyz");
        }
        #[cfg(target_os = "windows")]
        {
            std::env::set_var("VISUAL", "cargo --version");
            std::env::set_var("EDITOR", "non_existent_command_xyz");
        }
        let result = open_editor(Path::new("dummy"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_diff_tool_conversions() {
        use std::str::FromStr;
        assert_eq!(ExternalDiffTool::from_str("vim"), Ok(ExternalDiffTool::Vim));
        assert_eq!(
            ExternalDiffTool::from_str("Nvim"),
            Ok(ExternalDiffTool::Nvim)
        );
        assert_eq!(
            ExternalDiffTool::from_str("code"),
            Ok(ExternalDiffTool::Code)
        );
        assert_eq!(
            ExternalDiffTool::from_str("meld"),
            Ok(ExternalDiffTool::Meld)
        );
        assert_eq!(
            ExternalDiffTool::from_str("bcomp"),
            Ok(ExternalDiffTool::BeyondCompare)
        );
        assert_eq!(
            ExternalDiffTool::from_str("smerge"),
            Ok(ExternalDiffTool::SublimeMerge)
        );
        assert_eq!(
            ExternalDiffTool::from_str("ksdiff"),
            Ok(ExternalDiffTool::Kaleidoscope)
        );
        assert_eq!(
            ExternalDiffTool::from_str("difft"),
            Ok(ExternalDiffTool::Difftastic)
        );
        assert_eq!(ExternalDiffTool::from_str("unknown"), Err(()));
    }
}
