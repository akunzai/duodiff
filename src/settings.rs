use crate::theme::ThemeChoice;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// How directory scans decide whether two files match.
///
/// `Fast` is the built-in default: it compares size and modification time only,
/// so a same-size pair with differing timestamps is reported as `≈` (content
/// unverified) rather than a difference. `Precise` streams a SHA-256 of each
/// side and can therefore claim equality outright. See Issue #232.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    #[default]
    Fast,
    Precise,
}

impl ScanMode {
    /// Title-cased name used in the top bar, Config screen, and toasts.
    pub fn label(self) -> &'static str {
        match self {
            ScanMode::Fast => "Fast",
            ScanMode::Precise => "Precise",
        }
    }

    /// Whether scans compare content hashes rather than only size and mtime.
    pub fn is_precise(self) -> bool {
        matches!(self, ScanMode::Precise)
    }

    pub fn toggled(self) -> Self {
        match self {
            ScanMode::Fast => ScanMode::Precise,
            ScanMode::Precise => ScanMode::Fast,
        }
    }
}

use crate::diff_tool::ExternalDiffTool;

/// User preference for the external diff tool.
///
/// Choices:
/// - `Auto`: default; resolves first launchable tool from the priority list.
/// - `Disabled`: external diff is explicitly disabled.
/// - `Pinned`: use only the specified supported tool without fallback.
/// - `Unknown`: preserved legacy/unknown string, displayed as a disabled warning row.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum DiffToolSetting {
    #[default]
    Auto,
    Disabled,
    Pinned(ExternalDiffTool),
    Unknown(String),
}

impl DiffToolSetting {
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn pinned(&self) -> Option<ExternalDiffTool> {
        match self {
            Self::Pinned(tool) => Some(*tool),
            _ => None,
        }
    }

    pub fn unknown_name(&self) -> Option<&str> {
        match self {
            Self::Unknown(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl Serialize for DiffToolSetting {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DiffToolSetting::Auto => serializer.serialize_str("auto"),
            DiffToolSetting::Disabled => serializer.serialize_str("disabled"),
            DiffToolSetting::Pinned(tool) => serializer.serialize_str(tool.as_str()),
            DiffToolSetting::Unknown(name) => serializer.serialize_str(name),
        }
    }
}

impl<'de> Deserialize<'de> for DiffToolSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DiffToolSettingVisitor;

        impl<'de> serde::de::Visitor<'de> for DiffToolSettingVisitor {
            type Value = DiffToolSetting;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a diff tool string ('auto', 'disabled', or tool name)")
            }

            fn visit_str<E>(self, value: &str) -> Result<DiffToolSetting, E>
            where
                E: serde::de::Error,
            {
                let trimmed = value.trim();
                match trimmed.to_lowercase().as_str() {
                    "auto" => Ok(DiffToolSetting::Auto),
                    "disabled" | "none" | "off" => Ok(DiffToolSetting::Disabled),
                    _ => {
                        if let Ok(tool) = trimmed.parse::<ExternalDiffTool>() {
                            Ok(DiffToolSetting::Pinned(tool))
                        } else {
                            Ok(DiffToolSetting::Unknown(trimmed.to_string()))
                        }
                    }
                }
            }

            fn visit_none<E>(self) -> Result<DiffToolSetting, E>
            where
                E: serde::de::Error,
            {
                Ok(DiffToolSetting::Auto)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<DiffToolSetting, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                deserializer.deserialize_str(self)
            }
        }

        deserializer.deserialize_any(DiffToolSettingVisitor)
    }
}

/// Why an existing config file could not be used (Issue #342).
#[derive(Clone, Debug, PartialEq)]
pub struct LoadError {
    pub path: PathBuf,
    /// One line: the TOML message, prefixed with its line number when known.
    pub cause: String,
}

/// The TOML parser's message, flattened to one line and prefixed with the
/// 1-based line it points at, so a toast can carry it.
fn describe_toml_error(content: &str, error: &toml::de::Error) -> String {
    let message = error.message().trim().replace('\n', " ");
    match error.span() {
        Some(span) => {
            let line = content[..span.start.min(content.len())]
                .matches('\n')
                .count()
                + 1;
            format!("line {line}: {message}")
        }
        None => message,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    pub external_diff_tool: DiffToolSetting,
    pub check_updates: bool,
    /// Enable mouse support (wheel scroll, click-to-focus/select). Default `true`;
    /// set `false` to opt out (the `--no-mouse` CLI flag also forces it off for one session).
    pub mouse: bool,
    pub theme: ThemeChoice,
    /// Unchanged context lines kept around each change in the diff view when not
    /// showing the full file (`FileDiffState::show_full`). Adjustable from the Config screen.
    pub diff_context: usize,
    /// Persisted scan mode. Missing in older config files, which `#[serde(default)]`
    /// resolves to `Fast` — the built-in default — with no explicit migration.
    pub scan_mode: ScanMode,
    /// Global ignore patterns applied before project-local rules. An explicit empty
    /// list disables these built-in defaults.
    pub global_exclusions: Vec<String>,
    /// Whether `.gitignore` files participate in a session's effective matcher.
    /// `.duodiffignore`, global exclusions, and CLI exclusions remain active.
    pub respect_gitignore: bool,
    /// `[keys]`: Command config name → key or list of keys (Issue #339). Kept
    /// as raw TOML so an entry of the wrong shape is reported by
    /// [`crate::keymap::Keymap::with_overrides`] instead of failing the whole
    /// file, and so a save writes it back as the user wrote it.
    #[serde(skip_serializing_if = "toml::Table::is_empty")]
    pub keys: toml::Table,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            external_diff_tool: DiffToolSetting::Auto,
            check_updates: true,
            mouse: true,
            theme: ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: ScanMode::Fast,
            global_exclusions: vec![
                ".git/".to_string(),
                ".hg/".to_string(),
                ".svn/".to_string(),
                "node_modules/".to_string(),
                ".DS_Store".to_string(),
                "Thumbs.db".to_string(),
                "desktop.ini".to_string(),
            ],
            respect_gitignore: true,
            keys: toml::Table::new(),
        }
    }
}

/// Effective scan mode for this session: the `--scan-mode` CLI value when given,
/// otherwise the persisted setting (itself defaulting to `Fast`).
///
/// The CLI value only seeds the session — it never writes the config file, and a
/// later in-app change supersedes it for the rest of the session (Issue #238).
pub fn resolve_scan_mode(config_scan_mode: ScanMode, cli_scan_mode: Option<ScanMode>) -> ScanMode {
    cli_scan_mode.unwrap_or(config_scan_mode)
}

/// Effective mouse-enabled state: the config value, with the `--no-mouse` CLI flag able to
/// force it off for one session. There is intentionally no `--mouse` flag to force it on.
pub fn resolve_mouse_enabled(config_mouse: bool, no_mouse: bool) -> bool {
    config_mouse && !no_mouse
}

/// Effective `.gitignore` processing for this session. A CLI value overrides
/// the persisted setting without changing it on disk.
pub fn resolve_respect_gitignore(config_value: bool, cli_value: Option<bool>) -> bool {
    cli_value.unwrap_or(config_value)
}

impl AppSettings {
    /// Home directory used for config layout (`$HOME` or `%USERPROFILE%`).
    ///
    /// Reads the environment rather than `dirs::home_dir()`, which on Windows
    /// uses the Known Folder API and ignores test redirects of `USERPROFILE`.
    pub(crate) fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
    }

    /// Config directory: always under a `…/.config/duodiff`-style layout.
    ///
    /// - If `XDG_CONFIG_HOME` is set → `$XDG_CONFIG_HOME/duodiff`
    /// - Else → `$HOME/.config/duodiff` (or `%USERPROFILE%\.config\duodiff`)
    ///
    /// This intentionally does **not** use `dirs::config_dir()` (macOS
    /// Application Support / Windows `%APPDATA%`), so the path stays uniform.
    pub fn config_dir() -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let xdg = xdg.trim();
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("duodiff"));
            }
        }
        Self::home_dir().map(|h| h.join(".config").join("duodiff"))
    }

    /// `$HOME/.config/duodiff` (ignores `XDG_CONFIG_HOME`). Used as a load
    /// fallback when the primary path was redirected via XDG.
    pub fn home_config_dir() -> Option<PathBuf> {
        Self::home_dir().map(|h| h.join(".config").join("duodiff"))
    }

    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|d| d.join("config.toml"))
    }

    pub fn home_config_path() -> Option<PathBuf> {
        Self::home_config_dir().map(|d| d.join("config.toml"))
    }

    /// Candidate config files in search order (primary, then home fallback).
    pub fn config_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(p) = Self::config_path() {
            paths.push(p);
        }
        if let Some(p) = Self::home_config_path() {
            if paths.first().is_none_or(|primary| primary != &p) {
                paths.push(p);
            }
        }
        paths
    }

    /// Load from the first existing path in [`Self::config_search_paths`].
    pub fn load() -> Self {
        Self::load_reporting().0
    }

    /// Like [`Self::load`], but also returns why the config file could not be
    /// used, when it exists and cannot be read or parsed (Issue #342).
    pub fn load_reporting() -> (Self, Option<LoadError>) {
        Self::load_from_paths(Self::config_search_paths())
    }

    /// The first file that exists decides: a broken one yields the defaults and
    /// its error rather than falling through to the next path, so the user's
    /// real config is never silently swapped for another one.
    fn load_from_paths(paths: impl IntoIterator<Item = PathBuf>) -> (Self, Option<LoadError>) {
        for path in paths {
            match Self::try_load_file(&path) {
                Ok(Some(settings)) => return (settings, None),
                Ok(None) => {}
                Err(error) => return (AppSettings::default(), Some(error)),
            }
        }
        (AppSettings::default(), None)
    }

    fn try_load_file(path: &Path) -> Result<Option<Self>, LoadError> {
        if !path.exists() {
            return Ok(None);
        }
        let error = |cause: String| LoadError {
            path: path.to_path_buf(),
            cause,
        };
        let content = fs::read_to_string(path).map_err(|e| error(e.to_string()))?;
        toml::from_str::<AppSettings>(&content)
            .map(Some)
            .map_err(|e| error(describe_toml_error(&content, &e)))
    }

    /// Save under [`Self::config_dir`] (creating it if needed).
    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(dir) = Self::config_dir() {
            fs::create_dir_all(&dir)?;
            if let Some(path) = Self::config_path() {
                let content = toml::to_string(self).map_err(std::io::Error::other)?;
                fs::write(path, content)?;
            }
        }
        Ok(())
    }
}

/// Where a session's settings persist. The file is the one startup found;
/// memory keeps them in the process, so a test's session never touches the
/// process-wide config location (and needs no environment lock).
#[derive(Debug)]
pub enum SettingsStore {
    /// The config file. `broken` is why it failed to load, if it did: that
    /// file is never overwritten, since writing would replace the user's file
    /// with the defaults plus one change (Issue #342).
    File { broken: Option<LoadError> },
    /// In memory: the last settings saved, for a test to read back.
    Memory(std::cell::RefCell<Option<AppSettings>>),
}

impl SettingsStore {
    /// An empty in-memory store.
    pub fn memory() -> Self {
        Self::Memory(std::cell::RefCell::new(None))
    }

    /// Persist `settings`, or say why they cannot be.
    pub fn save(&self, settings: &AppSettings) -> Result<(), std::io::Error> {
        match self {
            Self::File { broken } => {
                if let Some(error) = broken {
                    if AppSettings::config_path().as_deref() == Some(error.path.as_path()) {
                        return Err(std::io::Error::other(format!(
                            "{} has an error — fix it and restart duodiff",
                            crate::app::App::display_path_with_home_tilde(&error.path)
                        )));
                    }
                }
                settings.save()
            }
            Self::Memory(saved) => {
                *saved.borrow_mut() = Some(settings.clone());
                Ok(())
            }
        }
    }

    /// What the last save put in a memory store; `None` for the file.
    #[cfg(test)]
    pub fn saved(&self) -> Option<AppSettings> {
        match self {
            Self::File { .. } => None,
            Self::Memory(saved) => saved.borrow().clone(),
        }
    }
}

/// The most unchanged context lines File Diff keeps around a change.
pub const MAX_DIFF_CONTEXT: usize = 50;

/// One change to the Settings (`CONTEXT.md`), whichever screen or Command
/// asked for it.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingChange {
    ExternalDiffTool(DiffToolSetting),
    CheckUpdates(bool),
    Mouse(bool),
    Theme(ThemeChoice),
    /// Clamped to [`MAX_DIFF_CONTEXT`].
    DiffContext(usize),
    ScanMode(ScanMode),
    RespectGitignore(bool),
    GlobalExclusions(Vec<String>),
}

impl SettingChange {
    /// Whether the change reshapes what a scan leaves out, so the ignore
    /// matchers must be rebuilt, from [`SettingsState::ignore_rules_after`],
    /// before it applies.
    pub fn reshapes_ignore_rules(&self) -> bool {
        matches!(self, Self::RespectGitignore(_) | Self::GlobalExclusions(_))
    }
}

/// What else a change affects, beyond the Settings themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingEffect {
    None,
    /// The tree was scanned under other rules; scan it again.
    Rescan,
    /// Turn the terminal's mouse capture on or off.
    MouseCapture(bool),
    /// Re-diff an open File Diff with this many context lines.
    DiffContext(usize),
}

/// What [`SettingsState::apply`] did: the change is in effect either way,
/// and `saved` says whether it will outlast this session.
#[derive(Debug)]
pub struct Applied {
    pub effect: SettingEffect,
    pub saved: Result<(), std::io::Error>,
}

/// What a scan of one root leaves out.
#[derive(Clone, Copy, Debug)]
pub struct IgnoreRules<'a> {
    global_exclusions: &'a [String],
    respect_gitignore: bool,
    cli_exclusions: &'a [String],
}

impl IgnoreRules<'_> {
    /// The matcher for a scan of `root` under these rules.
    pub fn matcher(&self, root: PathBuf) -> Result<crate::ignore::IgnoreMatcher, String> {
        crate::ignore::IgnoreMatcher::for_root(
            root,
            self.global_exclusions,
            self.respect_gitignore,
            self.cli_exclusions,
        )
    }
}

/// The Settings a session runs with (`CONTEXT.md`): what the config file
/// holds, the command-line flags the session started from, and the value of
/// each setting in effect now.
///
/// Every change goes through [`SettingsState::apply`], which puts it in
/// effect, saves it, and says what else it affects, so no caller keeps a
/// saved value and its effect in step by hand.
#[derive(Debug)]
pub struct SettingsState {
    saved: AppSettings,
    store: SettingsStore,
    scan_mode: ScanMode,
    mouse: bool,
    gitignore_override: Option<bool>,
    /// Session-only patterns from repeated `--exclude` flags.
    cli_exclusions: Vec<String>,
}

impl SettingsState {
    /// The Settings a session starts from: `saved` as loaded, with the
    /// command-line flags applied on top.
    pub fn new(
        saved: AppSettings,
        store: SettingsStore,
        overrides: &crate::startup::CliOverrides,
    ) -> Self {
        Self {
            scan_mode: resolve_scan_mode(saved.scan_mode, overrides.scan_mode),
            mouse: resolve_mouse_enabled(saved.mouse, overrides.no_mouse),
            gitignore_override: overrides.gitignore,
            cli_exclusions: overrides.exclude.clone(),
            saved,
            store,
        }
    }

    /// What the config file holds, or would hold had every save succeeded.
    pub fn saved(&self) -> &AppSettings {
        &self.saved
    }

    /// The scan mode in effect.
    pub fn scan_mode(&self) -> ScanMode {
        self.scan_mode
    }

    /// Whether the scan mode in effect is not the saved one, which only
    /// `--scan-mode` can cause; any change in the app saves it (Issue #238).
    pub fn scan_mode_is_session_override(&self) -> bool {
        self.scan_mode != self.saved.scan_mode
    }

    /// Whether mouse capture is on.
    pub fn mouse(&self) -> bool {
        self.mouse
    }

    /// Put mouse capture `on` in effect without saving it, when the
    /// terminal could not switch to what a change asked for.
    pub fn set_mouse_in_effect(&mut self, on: bool) {
        self.mouse = on;
    }

    /// Whether scans read `.gitignore` files.
    pub fn respect_gitignore(&self) -> bool {
        resolve_respect_gitignore(self.saved.respect_gitignore, self.gitignore_override)
    }

    /// How many `--exclude` patterns this session scans with.
    pub fn cli_exclusion_count(&self) -> usize {
        self.cli_exclusions.len()
    }

    /// The colour palette for the theme in effect.
    pub fn theme(&self) -> crate::theme::Theme {
        crate::theme::Theme::for_choice(self.saved.theme)
    }

    /// What a scan leaves out now.
    pub fn ignore_rules(&self) -> IgnoreRules<'_> {
        IgnoreRules {
            global_exclusions: &self.saved.global_exclusions,
            respect_gitignore: self.respect_gitignore(),
            cli_exclusions: &self.cli_exclusions,
        }
    }

    /// What a scan would leave out once `change` applies, so a caller can
    /// build the matchers first and refuse a change they cannot be built for.
    pub fn ignore_rules_after<'a>(&'a self, change: &'a SettingChange) -> IgnoreRules<'a> {
        let mut rules = self.ignore_rules();
        match change {
            SettingChange::RespectGitignore(on) => {
                rules.respect_gitignore = resolve_respect_gitignore(*on, self.gitignore_override);
            }
            SettingChange::GlobalExclusions(patterns) => rules.global_exclusions = patterns,
            _ => {}
        }
        rules
    }

    /// Put `change` in effect and save it. A failed save leaves the change
    /// in effect until duodiff exits.
    pub fn apply(&mut self, change: SettingChange) -> Applied {
        let effect = match change {
            SettingChange::ExternalDiffTool(tool) => {
                self.saved.external_diff_tool = tool;
                SettingEffect::None
            }
            SettingChange::CheckUpdates(on) => {
                self.saved.check_updates = on;
                SettingEffect::None
            }
            SettingChange::Mouse(on) => {
                self.saved.mouse = on;
                self.mouse = on;
                SettingEffect::MouseCapture(on)
            }
            SettingChange::Theme(theme) => {
                self.saved.theme = theme;
                SettingEffect::None
            }
            SettingChange::DiffContext(lines) => {
                self.saved.diff_context = lines.min(MAX_DIFF_CONTEXT);
                SettingEffect::DiffContext(self.saved.diff_context)
            }
            SettingChange::ScanMode(mode) => {
                self.saved.scan_mode = mode;
                self.scan_mode = mode;
                SettingEffect::Rescan
            }
            SettingChange::RespectGitignore(on) => {
                self.saved.respect_gitignore = on;
                SettingEffect::Rescan
            }
            SettingChange::GlobalExclusions(patterns) => {
                self.saved.global_exclusions = patterns;
                SettingEffect::Rescan
            }
        };
        Applied {
            effect,
            saved: self.store.save(&self.saved),
        }
    }

    /// Put `mode` in effect without saving it, as `--scan-mode` does.
    #[cfg(test)]
    pub(crate) fn set_scan_mode(&mut self, mode: ScanMode) {
        self.scan_mode = mode;
    }

    /// The saved settings, for a test to seed without saving.
    #[cfg(test)]
    pub(crate) fn saved_mut(&mut self) -> &mut AppSettings {
        &mut self.saved
    }

    /// What the last save put in a memory store.
    #[cfg(test)]
    pub(crate) fn stored(&self) -> Option<AppSettings> {
        self.store.saved()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::lock_env_tests;

    #[test]
    fn config_dir_defaults_to_home_dot_config() {
        let _guard = lock_env_tests();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        // SAFETY: serialized by lock_env_tests(); restored below.
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }

        let home = AppSettings::home_dir();
        let dir = AppSettings::config_dir();
        assert_eq!(
            dir,
            home.map(|h| h.join(".config").join("duodiff")),
            "without XDG_CONFIG_HOME, config must be $HOME/.config/duodiff"
        );

        unsafe {
            match old_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn config_dir_honors_xdg_config_home() {
        let _guard = lock_env_tests();
        let temp = tempfile::tempdir().unwrap();
        let xdg = temp.path().join("xdg-config");
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &xdg);
        }

        assert_eq!(
            AppSettings::config_dir(),
            Some(xdg.join("duodiff")),
            "XDG_CONFIG_HOME should redirect config_dir"
        );

        unsafe {
            match old_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    fn load_from_paths_prefers_first_existing() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary.toml");
        let fallback = temp.path().join("fallback.toml");
        fs::write(
            &primary,
            "external_diff_tool = \"nvim\"\ncheck_updates = true\n",
        )
        .unwrap();
        fs::write(
            &fallback,
            "external_diff_tool = \"vim\"\ncheck_updates = false\n",
        )
        .unwrap();

        let (loaded, _) = AppSettings::load_from_paths([primary.clone(), fallback.clone()]);
        assert_eq!(
            loaded.external_diff_tool,
            DiffToolSetting::Pinned(ExternalDiffTool::Nvim)
        );
        assert!(loaded.check_updates);

        fs::remove_file(&primary).unwrap();
        let (loaded, _) = AppSettings::load_from_paths([primary, fallback]);
        assert_eq!(
            loaded.external_diff_tool,
            DiffToolSetting::Pinned(ExternalDiffTool::Vim)
        );
        assert!(!loaded.check_updates);
    }

    #[test]
    fn load_from_paths_defaults_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("nope.toml");
        assert_eq!(
            AppSettings::load_from_paths([missing]),
            (AppSettings::default(), None)
        );
    }

    /// Issue #342: a broken file is reported, not swapped for defaults in
    /// silence, and it does not fall through to the next path either.
    #[test]
    fn load_from_paths_reports_a_broken_file_instead_of_skipping_it() {
        let temp = tempfile::tempdir().unwrap();
        let primary = temp.path().join("primary.toml");
        let fallback = temp.path().join("fallback.toml");
        fs::write(&primary, "check_updates = false\ntheme = \"blue\"\n").unwrap();
        fs::write(&fallback, "check_updates = false\n").unwrap();

        let (loaded, error) = AppSettings::load_from_paths([primary.clone(), fallback]);

        assert_eq!(loaded, AppSettings::default());
        let error = error.expect("the broken file is reported");
        assert_eq!(error.path, primary);
        assert!(
            error.cause.starts_with("line 2: "),
            "the cause names the line: {}",
            error.cause
        );
        assert!(!error.cause.contains('\n'), "one line for a toast");
    }

    #[test]
    fn diff_tool_setting_migration_and_deserialization() {
        // Absent migrates to Auto
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert_eq!(parsed.external_diff_tool, DiffToolSetting::Auto);

        // Explicit "auto"
        let parsed: AppSettings = toml::from_str("external_diff_tool = \"auto\"\n").unwrap();
        assert_eq!(parsed.external_diff_tool, DiffToolSetting::Auto);

        // "disabled", "none", "off"
        for val in ["disabled", "none", "off", "Disabled", "NONE"] {
            let parsed: AppSettings =
                toml::from_str(&format!("external_diff_tool = \"{val}\"\n")).unwrap();
            assert_eq!(parsed.external_diff_tool, DiffToolSetting::Disabled);
        }

        // Known tool
        let parsed: AppSettings = toml::from_str("external_diff_tool = \"vim\"\n").unwrap();
        assert_eq!(
            parsed.external_diff_tool,
            DiffToolSetting::Pinned(ExternalDiffTool::Vim)
        );

        // Unknown tool preserved
        let parsed: AppSettings = toml::from_str("external_diff_tool = \"custom-diff\"\n").unwrap();
        assert_eq!(
            parsed.external_diff_tool,
            DiffToolSetting::Unknown("custom-diff".to_string())
        );
    }

    #[test]
    fn diff_tool_setting_round_trip() {
        for setting in [
            DiffToolSetting::Auto,
            DiffToolSetting::Disabled,
            DiffToolSetting::Pinned(ExternalDiffTool::Vim),
            DiffToolSetting::Pinned(ExternalDiffTool::Code),
            DiffToolSetting::Unknown("custom-tool".to_string()),
        ] {
            let settings = AppSettings {
                external_diff_tool: setting.clone(),
                ..AppSettings::default()
            };
            let serialized = toml::to_string(&settings).unwrap();
            let parsed: AppSettings = toml::from_str(&serialized).unwrap();
            assert_eq!(parsed.external_diff_tool, setting);
        }
    }

    #[test]
    fn mouse_defaults_to_true_when_absent() {
        // A config file with no `mouse` key must load as enabled.
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert!(parsed.mouse);
    }

    #[test]
    fn exclusions_default_and_round_trip_without_breaking_older_configs() {
        let settings = AppSettings::default();
        assert_eq!(
            settings.global_exclusions,
            vec![
                ".git/",
                ".hg/",
                ".svn/",
                "node_modules/",
                ".DS_Store",
                "Thumbs.db",
                "desktop.ini",
            ]
        );
        assert!(settings.respect_gitignore);

        let older: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert_eq!(older, settings);

        let parsed: AppSettings =
            toml::from_str("global_exclusions = []\nrespect_gitignore = false\n").unwrap();
        assert!(parsed.global_exclusions.is_empty());
        assert!(!parsed.respect_gitignore);
    }

    #[test]
    fn gitignore_cli_override_is_session_only() {
        assert!(resolve_respect_gitignore(true, None));
        assert!(!resolve_respect_gitignore(false, None));
        assert!(resolve_respect_gitignore(false, Some(true)));
        assert!(!resolve_respect_gitignore(true, Some(false)));
    }

    #[test]
    fn mouse_round_trips() {
        let settings = AppSettings {
            external_diff_tool: DiffToolSetting::Auto,
            check_updates: true,
            mouse: false,
            theme: ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: ScanMode::Fast,
            global_exclusions: AppSettings::default().global_exclusions,
            respect_gitignore: true,
            keys: toml::Table::new(),
        };
        let serialized = toml::to_string(&settings).unwrap();
        let parsed: AppSettings = toml::from_str(&serialized).unwrap();
        assert!(!parsed.mouse);
    }

    #[test]
    fn resolve_mouse_enabled_truth_table() {
        assert!(resolve_mouse_enabled(true, false)); // default on, no flag
        assert!(!resolve_mouse_enabled(true, true)); // flag forces off
        assert!(!resolve_mouse_enabled(false, false)); // config off
        assert!(!resolve_mouse_enabled(false, true)); // both off
    }

    #[test]
    fn theme_defaults_to_dark_when_absent() {
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert_eq!(parsed.theme, crate::theme::ThemeChoice::Dark);
    }

    #[test]
    fn theme_round_trips() {
        let settings = AppSettings {
            external_diff_tool: DiffToolSetting::Auto,
            check_updates: true,
            mouse: true,
            theme: crate::theme::ThemeChoice::Light,
            diff_context: 3,
            scan_mode: ScanMode::Fast,
            global_exclusions: AppSettings::default().global_exclusions,
            respect_gitignore: true,
            keys: toml::Table::new(),
        };
        let serialized = toml::to_string(&settings).unwrap();
        let parsed: AppSettings = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.theme, crate::theme::ThemeChoice::Light);
    }

    #[test]
    fn diff_context_defaults_to_three_when_absent() {
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert_eq!(parsed.diff_context, 3);
    }

    #[test]
    fn diff_context_round_trips() {
        let settings = AppSettings {
            external_diff_tool: DiffToolSetting::Auto,
            check_updates: true,
            mouse: true,
            theme: ThemeChoice::Dark,
            diff_context: 10,
            scan_mode: ScanMode::Fast,
            global_exclusions: AppSettings::default().global_exclusions,
            respect_gitignore: true,
            keys: toml::Table::new(),
        };
        let serialized = toml::to_string(&settings).unwrap();
        let parsed: AppSettings = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.diff_context, 10);
    }

    #[test]
    fn config_search_paths_end_with_config_toml() {
        let paths = AppSettings::config_search_paths();
        assert!(
            !paths.is_empty() && paths.iter().all(|p| p.ends_with("config.toml")),
            "unexpected paths: {paths:?}"
        );
    }

    #[test]
    fn config_example_toml_parses_and_matches_defaults() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let example_path = std::path::Path::new(manifest_dir).join("config.example.toml");
        let content = fs::read_to_string(&example_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", example_path.display()));
        let parsed: AppSettings = toml::from_str(&content)
            .unwrap_or_else(|e| panic!("config.example.toml failed to parse: {e}"));
        assert_eq!(
            parsed,
            AppSettings::default(),
            "config.example.toml's uncommented values should match AppSettings::default()"
        );
    }

    #[test]
    fn scan_mode_defaults_to_fast_when_absent() {
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert_eq!(parsed.scan_mode, ScanMode::Fast);
    }

    #[test]
    fn scan_mode_round_trips() {
        let settings = AppSettings {
            external_diff_tool: DiffToolSetting::Auto,
            check_updates: true,
            mouse: true,
            theme: ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: ScanMode::Precise,
            global_exclusions: AppSettings::default().global_exclusions,
            respect_gitignore: true,
            keys: toml::Table::new(),
        };
        let serialized = toml::to_string(&settings).unwrap();
        assert!(
            serialized.contains("scan_mode = \"precise\""),
            "{serialized}"
        );
        let parsed: AppSettings = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.scan_mode, ScanMode::Precise);
    }

    #[test]
    fn resolve_scan_mode_prefers_the_cli_value() {
        // CLI > persisted config > the built-in Fast default.
        assert_eq!(resolve_scan_mode(ScanMode::Fast, None), ScanMode::Fast);
        assert_eq!(
            resolve_scan_mode(ScanMode::Precise, None),
            ScanMode::Precise
        );
        assert_eq!(
            resolve_scan_mode(ScanMode::Precise, Some(ScanMode::Fast)),
            ScanMode::Fast
        );
        assert_eq!(
            resolve_scan_mode(ScanMode::Fast, Some(ScanMode::Precise)),
            ScanMode::Precise
        );
    }

    #[test]
    fn scan_mode_toggles_and_labels() {
        assert_eq!(ScanMode::Fast.toggled(), ScanMode::Precise);
        assert_eq!(ScanMode::Precise.toggled(), ScanMode::Fast);
        assert_eq!(ScanMode::Fast.label(), "Fast");
        assert_eq!(ScanMode::Precise.label(), "Precise");
        assert!(!ScanMode::Fast.is_precise());
        assert!(ScanMode::Precise.is_precise());
    }

    fn state(overrides: crate::startup::CliOverrides) -> SettingsState {
        SettingsState::new(AppSettings::default(), SettingsStore::memory(), &overrides)
    }

    /// A change is in effect, saved, and names what else it affects.
    #[test]
    fn a_change_takes_effect_is_saved_and_names_its_effect() {
        let mut settings = state(crate::startup::CliOverrides::default());

        let applied = settings.apply(SettingChange::ScanMode(ScanMode::Precise));
        assert!(applied.saved.is_ok());
        assert_eq!(applied.effect, SettingEffect::Rescan);
        assert_eq!(settings.scan_mode(), ScanMode::Precise);
        assert_eq!(settings.stored().unwrap().scan_mode, ScanMode::Precise);

        let applied = settings.apply(SettingChange::Theme(ThemeChoice::Light));
        assert_eq!(applied.effect, SettingEffect::None);
        assert_eq!(settings.saved().theme, ThemeChoice::Light);
    }

    #[test]
    fn the_diff_context_stops_at_its_maximum() {
        let mut settings = state(crate::startup::CliOverrides::default());
        settings.apply(SettingChange::DiffContext(MAX_DIFF_CONTEXT + 1));
        assert_eq!(settings.saved().diff_context, MAX_DIFF_CONTEXT);
    }

    /// `--scan-mode` puts a mode in effect without saving it, and the Config
    /// screen says so until a change in the app saves one (Issue #238).
    #[test]
    fn a_command_line_scan_mode_is_a_session_override() {
        let mut settings = state(crate::startup::CliOverrides {
            scan_mode: Some(ScanMode::Precise),
            ..Default::default()
        });
        assert_eq!(settings.scan_mode(), ScanMode::Precise);
        assert_eq!(settings.saved().scan_mode, ScanMode::Fast);
        assert!(settings.scan_mode_is_session_override());

        settings.apply(SettingChange::ScanMode(ScanMode::Fast));
        assert!(!settings.scan_mode_is_session_override());
    }

    /// The rules a change would scan with are there to build before it
    /// applies; asking changes nothing.
    #[test]
    fn the_ignore_rules_after_a_change_leave_the_settings_alone() {
        let dir = tempfile::tempdir().unwrap();
        let settings = state(crate::startup::CliOverrides {
            exclude: vec!["*.log".to_string()],
            ..Default::default()
        });
        let change = SettingChange::GlobalExclusions(vec!["*.tmp".to_string()]);
        assert!(change.reshapes_ignore_rules());
        assert!(!SettingChange::Mouse(false).reshapes_ignore_rules());
        let ignores = |rules: IgnoreRules<'_>, name: &str| {
            rules
                .matcher(dir.path().to_path_buf())
                .unwrap()
                .is_ignored(Path::new(name), false)
                .unwrap()
        };

        assert!(ignores(settings.ignore_rules_after(&change), "a.tmp"));
        assert!(!ignores(settings.ignore_rules(), "a.tmp"));
        assert!(
            ignores(settings.ignore_rules(), "a.log"),
            "--exclude still applies"
        );
    }
}
