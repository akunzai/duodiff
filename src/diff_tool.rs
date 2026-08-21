use std::path::Path;
use std::process::Command;

pub static TEST_MUTEX: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// The fixed, documented, platform-aware priority list of supported external diff tools.
pub const SUPPORTED_TOOLS: [ExternalDiffTool; 8] = [
    ExternalDiffTool::Vim,
    ExternalDiffTool::Nvim,
    ExternalDiffTool::Code,
    ExternalDiffTool::Meld,
    ExternalDiffTool::BeyondCompare,
    ExternalDiffTool::SublimeMerge,
    ExternalDiffTool::Kaleidoscope,
    ExternalDiffTool::Difftastic,
];

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

    pub fn is_available(&self) -> bool {
        is_tool_available(*self)
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

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = path.metadata() {
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    } else {
        false
    }
}

#[cfg(windows)]
pub fn is_executable(path: &Path) -> bool {
    if let Ok(meta) = path.metadata() {
        meta.is_file()
    } else {
        false
    }
}

#[cfg(not(any(unix, windows)))]
pub fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub fn find_executable_in_dir(dir: &Path, cmd: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let direct = dir.join(cmd);
        if direct.extension().is_some() && is_executable(&direct) {
            return Some(direct);
        }
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        for ext in pathext.split(';') {
            let ext = ext.trim();
            if ext.is_empty() {
                continue;
            }
            let ext_normalized = if ext.starts_with('.') { &ext[1..] } else { ext };
            let candidate = dir.join(format!("{cmd}.{ext_normalized}"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        let candidate = dir.join(cmd);
        if is_executable(&candidate) {
            Some(candidate)
        } else {
            None
        }
    }
}

pub fn resolve_executable(cmd: &str) -> Option<std::path::PathBuf> {
    let cmd_path = Path::new(cmd);
    if cmd_path.components().count() > 1 {
        if is_executable(cmd_path) {
            return Some(cmd_path.to_path_buf());
        }
        #[cfg(windows)]
        if cmd_path.extension().is_none() {
            if let Some(parent) = cmd_path.parent() {
                let file_name = cmd_path.file_name()?.to_str()?;
                return find_executable_in_dir(parent, file_name);
            }
        }
        return None;
    }

    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Some(found) = find_executable_in_dir(&dir, cmd) {
                return Some(found);
            }
        }
    }
    None
}

pub fn find_in_path(cmd: &str) -> bool {
    resolve_executable(cmd).is_some()
}

pub fn is_tool_available(tool: ExternalDiffTool) -> bool {
    find_in_path(tool.as_str())
}

pub fn detect_diff_tools() -> Vec<(ExternalDiffTool, bool)> {
    SUPPORTED_TOOLS
        .iter()
        .map(|tool| (*tool, is_tool_available(*tool)))
        .collect()
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

    #[test]
    fn test_supported_tools_order_is_stable() {
        assert_eq!(
            SUPPORTED_TOOLS,
            [
                ExternalDiffTool::Vim,
                ExternalDiffTool::Nvim,
                ExternalDiffTool::Code,
                ExternalDiffTool::Meld,
                ExternalDiffTool::BeyondCompare,
                ExternalDiffTool::SublimeMerge,
                ExternalDiffTool::Kaleidoscope,
                ExternalDiffTool::Difftastic,
            ]
        );
    }

    #[test]
    fn test_resolve_executable_in_custom_path() {
        let temp = tempfile::tempdir().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        // Non-executable file on Unix / directory
        let sub_dir = bin_dir.join("dirtool");
        std::fs::create_dir_all(&sub_dir).unwrap();

        #[cfg(unix)]
        {
            let non_exec = bin_dir.join("nonexec");
            std::fs::write(&non_exec, "echo hello").unwrap();

            let exec = bin_dir.join("myexec");
            std::fs::write(&exec, "#!/bin/sh\necho hi").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exec).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exec, perms).unwrap();

            let _guard = crate::test_support::PathEnvGuard::set(&bin_dir);
            assert!(!find_in_path("nonexec"));
            assert!(!find_in_path("dirtool"));
            assert!(find_in_path("myexec"));
            assert_eq!(resolve_executable("myexec"), Some(exec));
        }

        #[cfg(windows)]
        {
            let exec_exe = bin_dir.join("myexec.exe");
            std::fs::write(&exec_exe, "binary").unwrap();
            let exec_bat = bin_dir.join("mybat.bat");
            std::fs::write(&exec_bat, "@echo off").unwrap();

            let _guard = crate::test_support::PathEnvGuard::set(&bin_dir);
            assert!(!find_in_path("dirtool"));
            assert!(find_in_path("myexec"));
            let resolved_exec = resolve_executable("myexec").expect("myexec should resolve");
            assert_eq!(
                resolved_exec.to_string_lossy().to_lowercase(),
                exec_exe.to_string_lossy().to_lowercase()
            );
            assert!(find_in_path("mybat"));
            let resolved_bat = resolve_executable("mybat").expect("mybat should resolve");
            assert_eq!(
                resolved_bat.to_string_lossy().to_lowercase(),
                exec_bat.to_string_lossy().to_lowercase()
            );
        }
    }
}
