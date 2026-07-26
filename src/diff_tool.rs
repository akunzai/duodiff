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

/// GUI editors fork and return immediately unless given a "wait" flag, so `Command::wait()`
/// returns before the user saves and duodiff resumes on stale content. Keyed by basename so a
/// full path or a `.exe` suffix still matches.
fn editor_is_gui(program: &str) -> bool {
    let basename = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let base = Path::new(basename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(basename)
        .to_ascii_lowercase();
    matches!(
        base.as_str(),
        "code"
            | "code-insiders"
            | "codium"
            | "vscodium"
            | "cursor"
            | "windsurf"
            | "zed"
            | "subl"
            | "sublime_text"
    )
}

/// Splits a `$VISUAL`/`$EDITOR` string into `(program, args)`, injecting a wait flag for known
/// GUI editors that don't already have one. Terminal editors are left untouched. Returns `None`
/// only when the string is blank (no program).
fn editor_command(editor: &str) -> Option<(String, Vec<String>)> {
    let mut parts = editor.split_whitespace();
    let program = parts.next()?.to_string();
    let mut args: Vec<String> = parts.map(str::to_string).collect();

    if editor_is_gui(&program) && !args.iter().any(|a| a == "--wait" || a == "-w") {
        args.push("--wait".to_string());
    }

    Some((program, args))
}

// Keep the compatibility for open_editor as it might be used elsewhere (like editing single files)
pub fn open_editor(file_path: &Path) -> Result<(), std::io::Error> {
    let editor_var = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string());
    let Some((program, args)) = editor_command(&editor_var) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "VISUAL or EDITOR is empty",
        ));
    };
    let mut command = Command::new(program);
    command.args(&args);
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
        let _guard = crate::test_support::lock_env_tests();
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
        let _guard = crate::test_support::lock_env_tests();
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
    fn editor_command_injects_wait_for_gui_editors() {
        for ed in ["zed", "code", "code-insiders", "cursor", "windsurf", "subl"] {
            let (program, args) = editor_command(ed).unwrap();
            assert_eq!(program, ed);
            assert!(
                args.iter().any(|a| a == "--wait" || a == "-w"),
                "expected a wait flag for GUI editor {ed:?}, got {args:?}"
            );
        }
    }

    #[test]
    fn editor_command_matches_gui_editor_by_basename() {
        let (program, args) = editor_command("/usr/local/bin/zed -n").unwrap();
        assert_eq!(program, "/usr/local/bin/zed");
        assert_eq!(args, vec!["-n", "--wait"]);
    }

    #[test]
    fn editor_command_leaves_terminal_editors_untouched() {
        for ed in ["vi", "vim", "nvim", "nano", "emacs", "hx"] {
            let (program, args) = editor_command(ed).unwrap();
            assert_eq!(program, ed);
            assert!(
                args.is_empty(),
                "terminal editor {ed:?} should get no injected flag, got {args:?}"
            );
        }
    }

    #[test]
    fn editor_command_keeps_an_existing_wait_flag() {
        let (_, args) = editor_command("code --wait").unwrap();
        assert_eq!(args, vec!["--wait"]);
        let (_, args) = editor_command("subl -w").unwrap();
        assert_eq!(args, vec!["-w"]);
    }

    #[test]
    fn editor_command_blank_is_none() {
        assert!(editor_command("").is_none());
        assert!(editor_command("   ").is_none());
    }

    #[test]
    fn editor_is_gui_matches_known_gui_editors() {
        for ed in [
            "zed",
            "code",
            "code-insiders",
            "codium",
            "vscodium",
            "cursor",
            "windsurf",
            "subl",
            "sublime_text",
        ] {
            assert!(
                editor_is_gui(ed),
                "{ed} should be recognised as a GUI editor"
            );
        }
    }

    #[test]
    fn editor_is_gui_rejects_terminal_editors() {
        for ed in ["vi", "vim", "nvim", "nano", "emacs", "hx"] {
            assert!(
                !editor_is_gui(ed),
                "{ed} should not be recognised as a GUI editor"
            );
        }
    }

    #[test]
    fn editor_is_gui_matches_by_basename_from_full_path() {
        assert!(editor_is_gui("/usr/local/bin/zed"));
        assert!(editor_is_gui("C:\\Tools\\code.exe"));
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
