//! What a session starts from, resolved once: the config file, the keymap it
//! remaps, the diff tools on `PATH`, how this binary was installed, and the
//! command-line overrides applied on top.
//!
//! `main` resolves it and hands it to [`crate::app::App`]; `--check` reports on
//! the same value. Every startup read of the environment and the filesystem
//! happens here, so nothing downstream reads the config file a second time.

use crate::diff_tool::ExternalDiffTool;
use crate::ignore::IgnoreMatcher;
use crate::keymap::Keymap;
use crate::settings::{AppSettings, LoadError, ScanMode, SettingsStore};
use crate::upgrade::InstallMethod;
use std::path::PathBuf;

/// Flags from the command line that shape the session without being saved.
#[derive(Clone, Debug, Default)]
pub struct CliOverrides {
    pub no_mouse: bool,
    pub scan_mode: Option<ScanMode>,
    pub no_update_check: bool,
    /// Repeated `--exclude` patterns, for this session only.
    pub exclude: Vec<String>,
    /// `--gitignore` (`Some(true)`) or `--no-gitignore` (`Some(false)`).
    pub gitignore: Option<bool>,
}

/// A startup problem, in the three parts `docs/agents/design.md` asks an
/// error for. `--check` prints it in that shape; the startup toast joins the
/// same parts into one line, so the two never word it differently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub what: String,
    pub causes: Vec<String>,
    pub next: String,
}

impl Problem {
    /// `Error:` / `Cause:` / `Next:` lines, for the terminal.
    pub fn report(&self) -> String {
        let mut lines = vec![format!("Error: {}", self.what)];
        lines.extend(self.causes.iter().map(|cause| format!("Cause: {cause}")));
        lines.push(format!("Next: {}", self.next));
        lines.join("\n")
    }

    /// One line for the startup toast: the first cause, and how many more
    /// `duodiff --check` lists.
    pub fn toast(&self) -> String {
        let first = self.causes.first().map(String::as_str).unwrap_or_default();
        let more = match self.causes.len().saturating_sub(1) {
            0 => String::new(),
            n => format!(" (+{n} more — run duodiff --check)"),
        };
        format!("{}: {first}{more} — {}", self.what, self.next)
    }
}

/// A config file that could not be used (Issue #342).
pub fn config_problem(error: &LoadError) -> Problem {
    Problem {
        what: "Cannot load the config file".to_string(),
        causes: vec![format!(
            "{}: {}",
            crate::app::App::display_path_with_home_tilde(&error.path),
            error.cause
        )],
        next: "Fix the file; until then duodiff uses the defaults and does not save settings"
            .to_string(),
    }
}

/// `[keys]` entries that were ignored (Issue #339).
pub fn key_problem(problems: &[String]) -> Option<Problem> {
    (!problems.is_empty()).then(|| Problem {
        what: "Some key bindings in the config file were ignored".to_string(),
        causes: problems.to_vec(),
        next: "Fix those [keys] entries; ignored commands keep their default keys".to_string(),
    })
}

/// Everything a session starts from. See the module doc.
#[derive(Debug)]
pub struct Startup {
    pub settings: AppSettings,
    /// Why the config file was not loaded, when it exists but is broken.
    pub config_load_error: Option<LoadError>,
    /// Where settings changes persist.
    pub store: SettingsStore,
    pub keymap: Keymap,
    pub key_problems: Vec<String>,
    pub detected_diff_tools: Vec<(ExternalDiffTool, bool)>,
    pub install_method: InstallMethod,
    pub overrides: CliOverrides,
    /// The cached newer version from the last update check, when the check
    /// is enabled.
    pub update_available: Option<String>,
}

impl Startup {
    /// Read the config file, the environment, and `PATH` once, and apply the
    /// command-line overrides.
    pub fn resolve(overrides: CliOverrides) -> Self {
        let mut startup = Self::load(overrides);
        if startup.update_check_enabled() {
            if let Ok(path) = crate::upgrade::state_path() {
                let seen = crate::upgrade::load_state(&path).latest_seen;
                if !seen.is_empty() {
                    startup.update_available =
                        crate::upgrade::is_newer(&seen, env!("CARGO_PKG_VERSION"));
                }
            }
        }
        startup
    }

    /// [`Startup::resolve`] without the cached update-check result.
    fn load(overrides: CliOverrides) -> Self {
        let (settings, config_load_error) = AppSettings::load_reporting();
        let (keymap, key_problems) = Keymap::with_overrides(&settings.keys);
        let install_method = match std::env::current_exe() {
            Ok(exe_path) => crate::upgrade::detect_install_method(&exe_path),
            Err(_) => InstallMethod::Standalone,
        };
        Self {
            settings,
            store: SettingsStore::File {
                broken: config_load_error.clone(),
            },
            config_load_error,
            keymap,
            key_problems,
            detected_diff_tools: crate::diff_tool::detect_diff_tools(),
            install_method,
            overrides,
            update_available: None,
        }
    }

    /// What a test's `App` starts from, reading nothing: the default
    /// settings in memory, no diff tool found, a standalone install.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self::with_settings(AppSettings::default())
    }

    /// [`Startup::for_test`] with `settings` as if loaded from a config file,
    /// `[keys]` applied.
    #[cfg(test)]
    pub(crate) fn with_settings(settings: AppSettings) -> Self {
        let (keymap, key_problems) = Keymap::with_overrides(&settings.keys);
        Self {
            settings,
            config_load_error: None,
            store: SettingsStore::memory(),
            keymap,
            key_problems,
            detected_diff_tools: crate::diff_tool::SUPPORTED_TOOLS
                .iter()
                .map(|tool| (*tool, false))
                .collect(),
            install_method: InstallMethod::Standalone,
            overrides: CliOverrides::default(),
            update_available: None,
        }
    }

    /// A test's session on the config location, for the tests about the file
    /// itself. The guard proves the location is a throwaway directory: `HOME`
    /// is process-wide, so an unredirected write would land in the
    /// developer's config, or in a concurrent test's.
    #[cfg(test)]
    pub(crate) fn from_disk(_redirected: &crate::test_support::ConfigEnvGuard) -> Self {
        Self::load(CliOverrides::default())
    }

    /// The problem to show when the session opens: a broken config file
    /// first, then ignored key bindings.
    pub fn problem(&self) -> Option<Problem> {
        self.config_load_error
            .as_ref()
            .map(config_problem)
            .or_else(|| key_problem(&self.key_problems))
    }

    /// What `--check` reports: the ready line, or the problem that would
    /// make the next run fall back to the defaults (Issue #342) or ignore some
    /// `[keys]` entries (Issue #339).
    pub fn check_report(&self) -> Result<String, String> {
        match self.problem() {
            None => Ok(format!(
                "duodiff version {} is ready",
                env!("CARGO_PKG_VERSION")
            )),
            Some(problem) => Err(problem.report()),
        }
    }

    /// This startup for a session on a file pair. Exclusion flags only shape a
    /// directory scan, so a file pair ignores them rather than failing a shell
    /// alias that always passes them.
    pub fn for_file_pair(mut self) -> Self {
        self.overrides.exclude.clear();
        self.overrides.gitignore = None;
        self
    }

    /// Whether the background update check runs this session.
    pub fn update_check_enabled(&self) -> bool {
        !self.overrides.no_update_check && self.settings.check_updates
    }

    /// The exclusion matchers for scanning `left` and `right`: the config's
    /// global exclusions, the gitignore choice, and the session's patterns.
    pub fn ignore_matchers(
        &self,
        left: PathBuf,
        right: PathBuf,
    ) -> Result<(IgnoreMatcher, IgnoreMatcher), String> {
        let respect_gitignore = crate::settings::resolve_respect_gitignore(
            self.settings.respect_gitignore,
            self.overrides.gitignore,
        );
        let matcher = |root: PathBuf| {
            IgnoreMatcher::for_root(
                root,
                &self.settings.global_exclusions,
                respect_gitignore,
                &self.overrides.exclude,
            )
            .map_err(|error| error.to_string())
        };
        Ok((matcher(left)?, matcher(right)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_problem_reads_the_same_on_the_terminal_and_in_the_toast() {
        let problem = key_problem(&["a".to_string(), "b".to_string(), "c".to_string()]).unwrap();
        assert_eq!(
            problem.report(),
            "Error: Some key bindings in the config file were ignored\n\
             Cause: a\nCause: b\nCause: c\n\
             Next: Fix those [keys] entries; ignored commands keep their default keys"
        );
        assert_eq!(
            problem.toast(),
            "Some key bindings in the config file were ignored: a (+2 more — run duodiff \
             --check) — Fix those [keys] entries; ignored commands keep their default keys"
        );
        assert_eq!(key_problem(&[]), None);
    }

    /// A broken config file is the problem to report, ahead of key bindings
    /// it could not have loaded anyway; with neither, `--check` is ready.
    #[test]
    fn a_broken_config_file_comes_before_ignored_keys() {
        let mut startup = Startup::for_test();
        assert!(startup.check_report().unwrap().ends_with("is ready"));

        startup.key_problems = vec!["keys.bogus: no command is named `bogus`".to_string()];
        assert_eq!(
            startup.problem().unwrap().what,
            "Some key bindings in the config file were ignored"
        );
        startup.config_load_error = Some(LoadError {
            path: PathBuf::from("/cfg/config.toml"),
            cause: "line 1: bad".to_string(),
        });
        assert_eq!(
            startup.problem().unwrap().what,
            "Cannot load the config file"
        );
        assert!(startup.check_report().is_err());
    }

    /// A file pair drops the exclusion flags a directory scan would use, so
    /// an alias that always passes them cannot break its Config screen.
    #[test]
    fn a_file_pair_ignores_the_exclusion_flags() {
        let startup = Startup {
            overrides: CliOverrides {
                exclude: vec!["[".to_string()],
                gitignore: Some(false),
                no_mouse: true,
                ..CliOverrides::default()
            },
            ..Startup::for_test()
        }
        .for_file_pair();
        assert!(startup.overrides.exclude.is_empty());
        assert_eq!(startup.overrides.gitignore, None);
        assert!(startup.overrides.no_mouse, "other flags still apply");
    }
}
