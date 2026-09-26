//! The background scan that produces the Directory Tree: its lifecycle —
//! in flight, progress, generation, spinner — and the task that walks both
//! roots. A scan's events carry its generation, and [`ScanState`] drops any
//! from a superseded scan. The tree itself belongs to the Directory Tree
//! (ADR-0005).

use crate::app::App;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
