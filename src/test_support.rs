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

thread_local! {
    /// Set while this thread holds a [`RedirectedConfigDir`]. Read by
    /// [`assert_config_env_redirected`].
    static CONFIG_ENV_REDIRECTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Fail loudly when a test persists settings without redirecting the config
/// directory first.
///
/// Such a test writes the developer's real `~/.config/duodiff/config.toml`, and
/// — because `HOME` is process-global — a concurrent guarded test has that path
/// pointed at *its* tempdir, so the stray write lands in the guarded test's
/// config and silently reverts what it just saved. That was a real flake: a
/// theme or scan-mode toggle in one test rewriting another test's config
/// underneath it.
pub fn assert_config_env_redirected() {
    assert!(
        CONFIG_ENV_REDIRECTED.with(|c| c.get()) > 0,
        "this test persists settings, so it must hold a \
         crate::test_support::ConfigEnvGuard for the write's lifetime"
    );
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
            external_diff_tool: crate::settings::DiffToolSetting::Pinned(
                crate::diff_tool::ExternalDiffTool::Vim,
            ),
            check_updates: false,
            mouse: false,
            theme: crate::theme::ThemeChoice::Light,
            diff_context: 7,
            scan_mode: crate::settings::ScanMode::Precise,
            global_exclusions: crate::settings::AppSettings::default().global_exclusions,
            respect_gitignore: true,
        };
        let config_dir = dir.path().join("duodiff");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            toml::to_string(&seed).unwrap(),
        )
        .unwrap();

        CONFIG_ENV_REDIRECTED.with(|c| c.set(c.get() + 1));

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
        CONFIG_ENV_REDIRECTED.with(|c| c.set(c.get().saturating_sub(1)));
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
///
/// Field order matters: struct fields drop in declaration order, so
/// `_redirect` must precede `_lock` here — otherwise the mutex would release
/// before `HOME`/`XDG_CONFIG_HOME` are restored, leaving a window where a
/// waiting thread can acquire the lock and read the still-redirected env vars.
pub struct ConfigEnvGuard {
    _redirect: RedirectedConfigDir,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl ConfigEnvGuard {
    pub fn new() -> Self {
        let lock = lock_env_tests();
        let redirect = RedirectedConfigDir::new();
        Self {
            _redirect: redirect,
            _lock: lock,
        }
    }
}

impl Default for ConfigEnvGuard {
    fn default() -> Self {
        Self::new()
    }
}

/// Test double for [`crate::actions::RealTerminalGuard`]: records the handoff
/// into thread-local storage instead of touching a real terminal.
///
/// Thread-local rather than shared, so tests running in parallel each read their
/// own record without taking a lock (Issue #304).
pub struct RecordingTerminalGuard {
    mouse_enabled: bool,
}

thread_local! {
    static HANDOFF_LOG: std::cell::RefCell<Vec<String>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

impl RecordingTerminalGuard {
    /// Forget anything earlier tests on this thread recorded.
    pub fn reset_log() {
        HANDOFF_LOG.with(|log| log.borrow_mut().clear());
    }

    /// What this thread recorded, oldest first.
    pub fn log() -> Vec<String> {
        HANDOFF_LOG.with(|log| log.borrow().clone())
    }

    /// Append to this thread's record — tests use it to place the spawn
    /// between the guard's own entries.
    pub fn record(entry: String) {
        HANDOFF_LOG.with(|log| log.borrow_mut().push(entry));
    }
}

impl crate::actions::TerminalGuard for RecordingTerminalGuard {
    fn acquire(mouse_enabled: bool) -> std::io::Result<Self> {
        Self::record(format!("suspend(mouse_enabled={mouse_enabled})"));
        Ok(Self { mouse_enabled })
    }
}

impl Drop for RecordingTerminalGuard {
    fn drop(&mut self) {
        Self::record(format!("resume(mouse_enabled={})", self.mouse_enabled));
    }
}

/// Temporarily overrides `PATH` for the lifetime of this guard and restores it on Drop.
/// Acquires `lock_env_tests()`.
pub struct PathEnvGuard {
    old_path: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl PathEnvGuard {
    pub fn set(new_path: impl AsRef<std::path::Path>) -> Self {
        let lock = lock_env_tests();
        let old_path = std::env::var("PATH").ok();
        unsafe {
            std::env::set_var("PATH", new_path.as_ref());
        }
        Self {
            old_path,
            _lock: lock,
        }
    }
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}

/// Drives the event loop for a test.
///
/// Assembles the terminal, the event handler, and the sender task that every
/// event-loop test used to build by hand, so a test says only what a user does
/// (Issue #302). Fixture setup happens before the harness, assertions after it.
///
/// ```ignore
/// AppHarness::new(&mut app).key('s').wait_ms(100).key('q').run().await;
/// ```
///
/// Scripts end by saying how they end: there is no implicit quit, because what
/// a quit key does is not constant — with staged changes it opens a
/// confirmation rather than leaving.
pub struct AppHarness<'a> {
    app: &'a mut crate::app::App,
    steps: Vec<HarnessStep>,
}

enum HarnessStep {
    Event(crate::event::AppEvent),
    Wait(std::time::Duration),
}

impl<'a> AppHarness<'a> {
    pub fn new(app: &'a mut crate::app::App) -> Self {
        Self {
            app,
            steps: Vec::new(),
        }
    }

    /// Press an unmodified character key.
    pub fn key(self, c: char) -> Self {
        self.key_code(crossterm::event::KeyCode::Char(c))
    }

    /// Press an unmodified key such as `Esc`, `Enter`, or an arrow.
    pub fn key_code(self, code: crossterm::event::KeyCode) -> Self {
        self.key_event(crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::empty(),
        ))
    }

    /// Press a key carrying modifiers, or one whose kind matters.
    pub fn key_event(self, key: crossterm::event::KeyEvent) -> Self {
        self.event(crate::event::AppEvent::Terminal(
            crossterm::event::Event::Key(key),
        ))
    }

    /// Deliver a mouse event.
    pub fn mouse(self, mouse: crossterm::event::MouseEvent) -> Self {
        self.event(crate::event::AppEvent::Terminal(
            crossterm::event::Event::Mouse(mouse),
        ))
    }

    /// Deliver a finished background scan.
    pub fn scan_finished(self, generation: u64, node: crate::diff::AlignedNode) -> Self {
        self.event(crate::event::AppEvent::ScanFinished {
            generation,
            node: Box::new(node),
        })
    }

    /// Deliver a failed background scan.
    pub fn scan_error(self, generation: u64, message: impl Into<String>) -> Self {
        self.event(crate::event::AppEvent::Error {
            generation,
            message: message.into(),
        })
    }

    /// Deliver any other event the loop handles.
    pub fn event(mut self, event: crate::event::AppEvent) -> Self {
        self.steps.push(HarnessStep::Event(event));
        self
    }

    /// Pause before the next step, the way a test waits for real background work.
    pub fn wait_ms(mut self, millis: u64) -> Self {
        self.steps
            .push(HarnessStep::Wait(std::time::Duration::from_millis(millis)));
        self
    }

    /// Run the loop until the script stops it, asserting it returned cleanly.
    ///
    /// A test that needs to inspect the loop's error should grow a `try_run`
    /// alongside this; no test needs one today.
    pub async fn run(self) {
        let terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24));
        let mut terminal = terminal.unwrap();
        let (mut events, tx) =
            crate::event::EventHandler::new(std::time::Duration::from_millis(10));

        let sender = tx.clone();
        let steps = self.steps;
        tokio::spawn(async move {
            for step in steps {
                match step {
                    HarnessStep::Event(event) => {
                        let _ = sender.send(event).await;
                    }
                    HarnessStep::Wait(duration) => tokio::time::sleep(duration).await,
                }
            }
        });

        let result = crate::run_app(&mut terminal, self.app, &mut events, tx).await;
        assert!(
            result.is_ok(),
            "the event loop returned an error: {:?}",
            result.err()
        );
    }
}
