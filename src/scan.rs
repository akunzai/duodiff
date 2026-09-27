//! The background scan that produces the Directory Tree: its lifecycle —
//! which scan runs next, in flight, progress, generation, spinner — and the
//! task that walks both roots. A scan's events carry its generation, and
//! [`ScanState`] drops any from a superseded scan. The tree itself belongs to
//! the Directory Tree (ADR-0005).

use crate::app::{self, App};
use crate::event::AppEvent;
use std::path::PathBuf;

/// A scan for the event loop to start: the whole tree, or one directory in
/// it, grafted into the tree when it finishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScanJob {
    Full,
    /// A directory's path relative to both roots; never the root itself.
    Directory(PathBuf),
}

/// The background scan: which one the event loop starts next, whether one
/// is in flight, its progress and generation, and the spinner that shows it. Owned by [`crate::app::App::scan`] /
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
    /// The scan asked for since the event loop last looked. One is enough:
    /// a later request either repeats it or covers it.
    queued: Option<ScanJob>,
    /// A session on a file pair has no directories to scan (ADR-0004).
    never: bool,
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

    /// Ask for a scan of both roots. It replaces a queued scan of one
    /// directory, which it covers.
    pub(crate) fn request_full(&mut self) {
        if !self.never {
            self.queued = Some(ScanJob::Full);
        }
    }

    /// Ask for a scan of the directory at `path` alone, after the tree
    /// changed there. Any other scan in flight or asked for would supersede
    /// that one, losing either it or the change, so then — and for the root
    /// itself — both roots are scanned instead.
    pub(crate) fn request_directory(&mut self, path: PathBuf) {
        if path.as_os_str().is_empty() || self.in_progress || self.queued.is_some() {
            self.request_full();
        } else if !self.never {
            self.queued = Some(ScanJob::Directory(path));
        }
    }

    /// This session compares two files: drop any scan asked for, and every
    /// one asked for from now on (Issue #327, ADR-0004).
    pub(crate) fn never_scan(&mut self) {
        self.never = true;
        self.queued = None;
    }

    /// The scan asked for since the last call, for the event loop to start.
    pub(crate) fn take_next(&mut self) -> Option<ScanJob> {
        self.queued.take()
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

/// Start `job` as a new scan, superseding any in flight. Only the event
/// loop starts one, for what [`ScanState::take_next`] hands it.
fn start(app: &mut App, job: ScanJob, tx: tokio::sync::mpsc::Sender<AppEvent>) {
    let path = match job {
        ScanJob::Full => PathBuf::new(),
        ScanJob::Directory(path) => path,
    };
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
            app::Request::MouseCapture(on) => {
                if let Err(error) = G::set_mouse_capture(on) {
                    app.mouse_capture_failed(on, error);
                }
            }
        }
    }
    if let Some(job) = app.scan_mut().take_next() {
        start(app, job, tx.clone());
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

    /// Asking again for a scan already asked for starts it once.
    #[test]
    fn a_scan_asked_for_twice_runs_once() {
        let mut scan = ScanState::default();
        scan.request_full();
        scan.request_full();

        assert_eq!(scan.take_next(), Some(ScanJob::Full));
        assert_eq!(scan.take_next(), None);
    }

    /// A scan of both roots covers a directory's, so it takes its place.
    #[test]
    fn a_full_scan_replaces_a_queued_directory_scan() {
        let mut scan = ScanState::default();
        scan.request_directory(PathBuf::from("dir"));
        scan.request_full();

        assert_eq!(scan.take_next(), Some(ScanJob::Full));
        assert_eq!(scan.take_next(), None);
    }

    /// Issue #362: a change scans its directory in the background, unless
    /// that scan would supersede another — then, as for a change at the
    /// root, the whole tree is scanned instead.
    #[test]
    fn a_directory_scan_widens_unless_it_runs_alone() {
        let mut scan = ScanState::default();
        scan.request_directory(PathBuf::from("dir"));
        assert_eq!(
            scan.take_next(),
            Some(ScanJob::Directory(PathBuf::from("dir")))
        );

        scan.request_directory(PathBuf::new());
        assert_eq!(scan.take_next(), Some(ScanJob::Full), "the root itself");

        scan.request_directory(PathBuf::from("dir"));
        scan.request_directory(PathBuf::from("other"));
        assert_eq!(
            scan.take_next(),
            Some(ScanJob::Full),
            "a second change would lose the first one's scan"
        );

        scan.begin();
        scan.request_directory(PathBuf::from("dir"));
        assert_eq!(
            scan.take_next(),
            Some(ScanJob::Full),
            "a scan in flight would be lost"
        );
    }

    /// A session on a file pair has no directories: whatever was asked for
    /// before it knew, and whatever is asked for after, never starts.
    #[test]
    fn a_file_pair_session_never_scans() {
        let mut scan = ScanState::default();
        scan.request_directory(PathBuf::from("dir"));
        scan.never_scan();
        assert_eq!(scan.take_next(), None);

        scan.request_full();
        scan.request_directory(PathBuf::from("dir"));
        scan.request_directory(PathBuf::new());
        assert_eq!(scan.take_next(), None);
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
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
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
        assert_eq!(app.take_pending(), (None, None));
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
        app.config_gesture(crate::app::ConfigGesture::Activate);
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
        app.config_gesture(crate::app::ConfigGesture::Activate);
        run_requests::<Refusing>(&mut app, &tx);

        assert!(app.settings().mouse(), "capture is still on");
        assert!(!app.saved_settings().mouse, "the choice is still saved");
        let (toast, is_error) = app.status_toast().unwrap();
        assert!(is_error);
        assert_eq!(toast, "Cannot switch mouse support: no terminal");
    }
}
