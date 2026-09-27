//! The background scan that produces the Directory Tree: its lifecycle —
//! in flight, progress, generation, spinner — and the task that walks both
//! roots. A scan's events carry its generation, and [`ScanState`] drops any
//! from a superseded scan. The tree itself belongs to the Directory Tree
//! (ADR-0005).

use crate::app::{self, App};
use crate::event::AppEvent;
use std::path::PathBuf;

/// The background scan: whether one is in flight, its progress and
/// generation, and the spinner that shows it. Owned by [`crate::app::App::scan`] /
/// [`crate::app::App::scan_mut`]. The tree a scan produces belongs to
/// [`crate::app::DirectoryTreeState`] (ADR-0005).
#[derive(Clone, Debug, Default)]
pub struct ScanState {
    in_progress: bool,
    progress_count: usize,
    spinner_frame: usize,
    /// Monotonic counter bumped for every scan start. Stale `ScanFinished` /
    /// scan `Error` events with an older generation are ignored.
    generation: u64,
}

impl ScanState {
    /// True while a background scan is still running.
    pub(crate) fn in_progress(&self) -> bool {
        self.in_progress
    }

    /// Items scanned so far in the active scan.
    pub(crate) fn progress_count(&self) -> usize {
        self.progress_count
    }

    /// Current spinner animation frame index.
    pub(crate) fn spinner_frame(&self) -> usize {
        self.spinner_frame
    }

    /// Current background scan generation.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Advance the TUI animation frame.
    pub(crate) fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Take a progress report from scan `generation`; one from a superseded
    /// scan changes nothing.
    pub(crate) fn progress(&mut self, generation: u64, count: usize) {
        if generation == self.generation {
            self.progress_count = count;
        }
    }

    /// Mark a new background scan as in-flight and return its generation id.
    pub(crate) fn begin(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.in_progress = true;
        self.progress_count = 0;
        self.generation
    }

    /// Mark the scan `generation` as finished, whether it produced a tree or
    /// failed. Returns `false`, changing nothing, for a superseded generation,
    /// so the caller can drop its result or its error toast too.
    pub(crate) fn finish(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.in_progress = false;
        self.progress_count = 0;
        true
    }
}

/// Start a background scan of both roots, superseding any in flight. Only
/// the event loop starts one, for an [`App::request_rescan`].
pub(crate) fn start(app: &mut App, tx: tokio::sync::mpsc::Sender<AppEvent>) {
    start_at(app, PathBuf::new(), tx);
}

/// Start a background scan of the directory at `path` alone, for an
/// [`App::request_subtree_rescan`]. It supersedes any scan in flight, so
/// `App` asks for one only when none is.
pub(crate) fn start_subtree(app: &mut App, path: PathBuf, tx: tokio::sync::mpsc::Sender<AppEvent>) {
    start_at(app, path, tx);
}

fn start_at(app: &mut App, path: PathBuf, tx: tokio::sync::mpsc::Sender<AppEvent>) {
    let generation = app.scan_mut().begin();
    start_scan_task(
        app.left_path().to_path_buf(),
        app.right_path().to_path_buf(),
        path,
        app.settings().scan_mode().is_precise(),
        app.ignore_matchers().0.clone(),
        app.ignore_matchers().1.clone(),
        generation,
        tx,
    );
}

/// Walk `left` and `right` from `path` — the empty path for the whole tree —
/// on a blocking thread, reporting progress and then the aligned tree, tagged
/// with `generation`.
#[allow(clippy::too_many_arguments)]
pub fn start_scan_task(
    left: PathBuf,
    right: PathBuf,
    path: PathBuf,
    precise: bool,
    mut left_ignore: crate::ignore::IgnoreMatcher,
    mut right_ignore: crate::ignore::IgnoreMatcher,
    generation: u64,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) {
    let (prog_tx, mut prog_rx) = tokio::sync::mpsc::channel::<usize>(100);
    let app_tx = tx.clone();
    tokio::spawn(async move {
        while let Some(count) = prog_rx.recv().await {
            if app_tx
                .send(AppEvent::ScanProgress { generation, count })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let scanned = path.clone();
        let root = tokio::task::spawn_blocking(move || {
            let mut on_progress = |count: usize| {
                let _ = prog_tx.try_send(count);
            };
            crate::diff::align_directories(
                &left,
                &right,
                &scanned,
                precise,
                &mut left_ignore,
                &mut right_ignore,
                &mut on_progress,
            )
        })
        .await;

        match root {
            Ok(Ok(node)) => {
                let node = Box::new(node);
                let event = if path.as_os_str().is_empty() {
                    AppEvent::ScanFinished { generation, node }
                } else {
                    AppEvent::SubtreeScanFinished {
                        generation,
                        path,
                        node,
                    }
                };
                let _ = tx.send(event).await;
            }
            Ok(Err(err)) => {
                let _ = tx
                    .send(AppEvent::Error {
                        generation,
                        message: err.to_string(),
                    })
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(AppEvent::Error {
                        generation,
                        message: err.to_string(),
                    })
                    .await;
            }
        }
    });
}

/// Carry out the work changes left for the event loop.
pub(crate) fn run_requests<G: crate::terminal::TerminalGuard>(
    app: &mut App,
    tx: &tokio::sync::mpsc::Sender<AppEvent>,
) {
    for request in app.take_requests() {
        match request {
            app::Request::Rescan => start(app, tx.clone()),
            app::Request::RescanSubtree(path) => start_subtree(app, path, tx.clone()),
            app::Request::MouseCapture(on) => {
                if let Err(error) = G::set_mouse_capture(on) {
                    app.mouse_capture_failed(on, error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::TerminalGuard;

    /// Progress from a superseded scan must not overwrite the count of the
    /// one in flight.
    #[test]
    fn progress_from_a_superseded_scan_is_dropped() {
        let mut scan = ScanState::default();
        let stale = scan.begin();
        let current = scan.begin();

        scan.progress(current, 10);
        scan.progress(stale, 99);

        assert_eq!(scan.progress_count(), 10);
        assert!(!scan.finish(stale));
        assert!(scan.in_progress());
    }

    /// Issue #238: the Palette runs the same flow as the `c` key — persist,
    /// adopt, and leave exactly one background rescan for the event loop.
    #[test]
    fn test_palette_toggle_scan_persists_and_starts_exactly_one_rescan() {
        use crate::settings::ScanMode;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::seeded(PathBuf::from("left"), PathBuf::from("right"));
        // The seeded config persists Precise, so the toggle lands on Fast.
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);
        let before = app.scan().generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let action = crate::commands::CommandEntry {
            key: "c".to_string(),
            label: "Toggle Scan Mode".to_string(),
            command: crate::commands::Command::ToggleScan,
            disabled_reason: None,
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                crate::commands::Commands::new(tx)
                    .execute(
                        &mut app,
                        crate::commands::Invocation::Command(action.command),
                        &mut terminal,
                    )
                    .unwrap();
            });

        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        assert_eq!(
            app.saved_settings().scan_mode,
            ScanMode::Fast,
            "the palette persists the new mode"
        );
        assert_eq!(
            app.requests(),
            [app::Request::Rescan],
            "exactly one background rescan"
        );
        assert_eq!(app.scan().generation(), before, "the event loop starts it");
    }

    /// The event loop starts the rescan a change asked for, once, and the
    /// request is gone afterwards.
    #[tokio::test]
    async fn a_requested_rescan_starts_one_scan() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("left"), dir.path().join("right"));
        app.switch_scan_mode(crate::settings::ScanMode::Precise);
        app.switch_scan_mode(crate::settings::ScanMode::Fast);
        let before = app.scan().generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        run_requests::<crate::test_support::RecordingTerminalGuard>(&mut app, &tx);

        assert_eq!(app.scan().generation(), before + 1);
        assert!(app.requests().is_empty());
    }

    fn select_config_row(app: &mut App, row: app::ConfigRowKind) {
        let idx = app.config_rows().iter().position(|r| *r == row).unwrap();
        app.config_mut().set_selected_idx(idx);
    }

    /// Turning mouse support off in Config releases the terminal's mouse
    /// capture right away, not at the next editor handoff.
    #[tokio::test]
    async fn the_mouse_row_switches_the_terminal_capture() {
        use crate::test_support::RecordingTerminalGuard;
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        RecordingTerminalGuard::reset_log();

        select_config_row(&mut app, app::ConfigRowKind::Mouse);
        app.apply_config_selection();
        run_requests::<RecordingTerminalGuard>(&mut app, &tx);

        assert_eq!(RecordingTerminalGuard::log(), ["mouse_capture(false)"]);
        assert!(!app.settings().mouse());
    }

    /// A terminal that cannot switch capture keeps what it does in effect,
    /// so mouse events are handled exactly when they arrive.
    #[tokio::test]
    async fn a_refused_mouse_switch_keeps_the_capture_in_effect() {
        struct Refusing;
        impl TerminalGuard for Refusing {
            fn acquire(_: bool) -> std::io::Result<Self> {
                Ok(Self)
            }
            fn set_mouse_capture(_: bool) -> std::io::Result<()> {
                Err(std::io::Error::other("no terminal"))
            }
        }
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        select_config_row(&mut app, app::ConfigRowKind::Mouse);
        app.apply_config_selection();
        run_requests::<Refusing>(&mut app, &tx);

        assert!(app.settings().mouse(), "capture is still on");
        assert!(!app.saved_settings().mouse, "the choice is still saved");
        let (toast, is_error) = app.status_toast().unwrap();
        assert!(is_error);
        assert_eq!(toast, "Cannot switch mouse support: no terminal");
    }
}
