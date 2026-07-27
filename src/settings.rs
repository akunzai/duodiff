use crate::theme::ThemeChoice;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub external_diff_tool: Option<String>,
    #[serde(default = "default_check_updates")]
    pub check_updates: bool,
    /// Enable mouse support (wheel scroll, click-to-focus/select). Default `true`;
    /// set `false` to opt out (the `--no-mouse` CLI flag also forces it off for one session).
    #[serde(default = "default_mouse")]
    pub mouse: bool,
    #[serde(default)]
    pub theme: ThemeChoice,
    /// Unchanged context lines kept around each change in the diff view when not
    /// showing the full file (`diff_show_full`). Adjustable from the Config screen.
    #[serde(default = "default_diff_context")]
    pub diff_context: usize,
}

fn default_check_updates() -> bool {
    true
}

fn default_mouse() -> bool {
    true
}

fn default_diff_context() -> usize {
    3
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            external_diff_tool: None,
            check_updates: true,
            mouse: true,
            theme: ThemeChoice::Dark,
            diff_context: default_diff_context(),
        }
    }
}

/// Effective mouse-enabled state: the config value, with the `--no-mouse` CLI flag able to
/// force it off for one session. There is intentionally no `--mouse` flag to force it on.
pub fn resolve_mouse_enabled(config_mouse: bool, no_mouse: bool) -> bool {
    config_mouse && !no_mouse
}

impl AppSettings {
    /// Home directory used for config layout (`$HOME` or `%USERPROFILE%`).
    fn home_dir() -> Option<PathBuf> {
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

    /// Load from the first readable path in [`Self::config_search_paths`].
    pub fn load() -> Self {
        Self::load_from_paths(Self::config_search_paths())
    }

    fn load_from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        for path in paths {
            if let Some(settings) = Self::try_load_file(&path) {
                return settings;
            }
        }
        AppSettings::default()
    }

    fn try_load_file(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let content = fs::read_to_string(path).ok()?;
        toml::from_str::<AppSettings>(&content).ok()
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

        let loaded = AppSettings::load_from_paths([primary.clone(), fallback.clone()]);
        assert_eq!(loaded.external_diff_tool.as_deref(), Some("nvim"));
        assert!(loaded.check_updates);

        fs::remove_file(&primary).unwrap();
        let loaded = AppSettings::load_from_paths([primary, fallback]);
        assert_eq!(loaded.external_diff_tool.as_deref(), Some("vim"));
        assert!(!loaded.check_updates);
    }

    #[test]
    fn load_from_paths_defaults_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("nope.toml");
        assert_eq!(
            AppSettings::load_from_paths([missing]),
            AppSettings::default()
        );
    }

    #[test]
    fn mouse_defaults_to_true_when_absent() {
        // A config file with no `mouse` key must load as enabled.
        let parsed: AppSettings = toml::from_str("check_updates = true\n").unwrap();
        assert!(parsed.mouse);
    }

    #[test]
    fn mouse_round_trips() {
        let settings = AppSettings {
            external_diff_tool: None,
            check_updates: true,
            mouse: false,
            theme: ThemeChoice::Dark,
            diff_context: 3,
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
            external_diff_tool: None,
            check_updates: true,
            mouse: true,
            theme: crate::theme::ThemeChoice::Light,
            diff_context: 3,
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
            external_diff_tool: None,
            check_updates: true,
            mouse: true,
            theme: ThemeChoice::Dark,
            diff_context: 10,
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
        // Regression guard: keeps `config.example.toml` in sync with `AppSettings`.
        // If a future PR adds/renames a field without updating the example, this
        // either fails to parse (unknown/missing field) or the round-tripped value
        // silently diverges from `AppSettings::default()` below.
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
}
