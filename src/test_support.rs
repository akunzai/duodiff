//! Shared test-only helpers for isolating `AppSettings::load()`/`save()` from
//! the developer's real `~/.config/duodiff/config.toml`. Used by tests in
//! both `app.rs` and `main.rs` that exercise config persistence.

/// Serializes tests that mutate process-wide env vars, shared with
/// `crate::diff_tool`'s $EDITOR/$VISUAL tests (see AGENTS.md "Environment
/// Mutating Tests").
///
/// Recovers from a poisoned lock rather than panicking: the guarded data is
/// `()`, so there's no invariant a prior panicking test could have left
/// broken — and letting poison propagate would fail every other test that
/// shares this mutex, turning one assertion failure into a cascade of
/// unrelated ones.
pub fn lock_env_tests() -> std::sync::MutexGuard<'static, ()> {
    crate::diff_tool::TEST_MUTEX
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Redirects `XDG_CONFIG_HOME`/`HOME`/`USERPROFILE` to a throwaway tempdir
/// seeded with a config file where every field holds a non-default value,
/// isolating `AppSettings::load()`/`save()` from the developer's real
/// `~/.config/duodiff/config.toml`. Callers must hold `lock_env_tests()` for
/// the lifetime of this guard.
pub struct RedirectedConfigDir {
    _dir: tempfile::TempDir,
    old_xdg: Option<String>,
    old_home: Option<String>,
    old_userprofile: Option<String>,
}

impl RedirectedConfigDir {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();

        // SAFETY: caller holds `lock_env_tests()` for our lifetime; restored in Drop.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
            std::env::set_var("HOME", dir.path());
            std::env::set_var("USERPROFILE", dir.path());
        }

        let seed = crate::settings::AppSettings {
            external_diff_tool: Some("vim".to_string()),
            check_updates: false,
            mouse: false,
            theme: crate::theme::ThemeChoice::Light,
            diff_context: 7,
        };
        let config_dir = dir.path().join("duodiff");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            toml::to_string(&seed).unwrap(),
        )
        .unwrap();

        Self {
            _dir: dir,
            old_xdg,
            old_home,
            old_userprofile,
        }
    }
}

impl Default for RedirectedConfigDir {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RedirectedConfigDir {
    fn drop(&mut self) {
        // SAFETY: caller still holds `lock_env_tests()` while we restore.
        unsafe {
            match &self.old_xdg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match &self.old_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match &self.old_userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }
}

/// Convenience bundle for the common case: acquire the lock and redirect the
/// config dir together, both released on drop.
pub struct ConfigEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _redirect: RedirectedConfigDir,
}

impl ConfigEnvGuard {
    pub fn new() -> Self {
        let lock = lock_env_tests();
        let redirect = RedirectedConfigDir::new();
        Self {
            _lock: lock,
            _redirect: redirect,
        }
    }
}

impl Default for ConfigEnvGuard {
    fn default() -> Self {
        Self::new()
    }
}
