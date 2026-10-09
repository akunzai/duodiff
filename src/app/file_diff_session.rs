//! File Diff (`GLOSSARY.md`): what the screen shows, how it gets there, and
//! how staged edits reach disk — loading in the background, cancelling a load
//! the user walked away from, and saving every dirty side all-or-nothing.
//!
//! `App` keeps what the session compares (ADR-0004) and hands this module the
//! two paths; this module owns everything that reads or writes them for File
//! Diff.

use super::{FileDiffState, FlatRow, StagedSave};
use crate::event::AppEvent;
use crate::side::{Pair, Side};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

/// File Diff: its content, the load in flight, and the Directory Tree row it
/// was opened on.
#[derive(Default)]
pub(crate) struct FileDiffSession {
    content: FileDiffState,
    loading: LoadState,
    /// The row opened in File Diff; a background scan may move the tree cursor.
    opened_row: Option<FlatRow>,
}

/// What a finished load did.
#[derive(Debug, PartialEq)]
pub(crate) enum LoadOutcome {
    /// File Diff shows the pair.
    Opened,
    /// File Diff could not open the pair; it shows nothing.
    OpenFailed(String),
    /// A load the user cancelled or replaced; nothing changed.
    Stale,
}

impl FileDiffSession {
    /// An empty File Diff that diffs with `context` unchanged lines.
    pub(crate) fn with_context(context: usize) -> Self {
        Self {
            content: FileDiffState::with_context(context),
            ..Self::default()
        }
    }

    /// What File Diff shows.
    pub(crate) fn content(&self) -> &FileDiffState {
        &self.content
    }

    /// Drive what File Diff shows: scroll, stage, toggle.
    pub(crate) fn content_mut(&mut self) -> &mut FileDiffState {
        &mut self.content
    }

    /// The load in flight, if any.
    pub(crate) fn loading(&self) -> &LoadState {
        &self.loading
    }

    /// The Directory Tree row File Diff was opened on.
    pub(crate) fn opened_row(&self) -> Option<&FlatRow> {
        self.opened_row.as_ref()
    }

    /// Open `paths` — `row` under each root — in the background, keeping the
    /// wrap setting and diffing with `context` lines. `hint` names the way out
    /// of a file the built-in diff cannot show.
    pub(crate) fn open(
        &mut self,
        row: FlatRow,
        paths: Pair<PathBuf>,
        hint: String,
        context: usize,
    ) {
        let wrap = self.content.wrap();
        self.opened_row = Some(row);
        self.loading.request(paths, hint, context, wrap);
        self.content = FileDiffState::with_context(context);
        if wrap {
            self.content.toggle_wrap();
        }
    }

    /// Open on content already read, diff-only and scrolled to the top.
    pub(crate) fn open_loaded(&mut self, loaded: Pair<crate::diff_view::LoadedText>) {
        self.content.set_show_full(false);
        self.content.load(loaded);
        self.content.reset_scroll();
    }

    /// Hand the event loop the load to run, once.
    pub(crate) fn take_job(&mut self) -> Option<LoadJob> {
        self.loading.take_job()
    }

    /// Take a finished load. A result from a cancelled or replaced load is
    /// [`LoadOutcome::Stale`] and changes nothing.
    pub(crate) fn finish(
        &mut self,
        generation: u64,
        result: Result<Box<FileDiffState>, String>,
    ) -> LoadOutcome {
        if !self.loading.finish(generation) {
            return LoadOutcome::Stale;
        }
        match result {
            Ok(content) => {
                self.content = *content;
                LoadOutcome::Opened
            }
            Err(error) => {
                self.opened_row = None;
                LoadOutcome::OpenFailed(error)
            }
        }
    }

    /// Stop any load in flight and forget the row, as leaving File Diff does.
    pub(crate) fn close(&mut self) {
        self.loading.cancel();
        self.opened_row = None;
    }

    /// Forget the opened row and the hashes after the roots swapped sides.
    pub(crate) fn reset_for_swap(&mut self) {
        self.close();
        self.content.reset_for_swap();
    }

    /// Absolute destination paths a save would write, left side first.
    pub(crate) fn save_targets(&self, paths: &Pair<PathBuf>) -> Vec<PathBuf> {
        self.dirty_files(paths)
            .into_iter()
            .map(|(_, path)| crate::write::absolute_lexical(&path))
            .collect()
    }

    /// Write every dirty side to `paths`, all-or-nothing.
    ///
    /// Each side is staged into a temporary file in its own directory first, so
    /// the visible file is only replaced once both writes are known to have
    /// worked; a failure part-way restores the originals rather than leaving one
    /// side written and the other not (Issue #235).
    ///
    /// Returns [`StagedSave::Conflicted`] with the offending paths when the
    /// on-disk content no longer matches the session baseline; nothing is
    /// written and the caller decides what to ask the user. A side `rehash`
    /// says is not a regular file (the null device, a pipe) was never written
    /// and keeps its hash.
    pub(crate) fn save(
        &mut self,
        paths: &Pair<PathBuf>,
        rehash: Pair<bool>,
    ) -> Result<StagedSave, std::io::Error> {
        if !self.content.is_dirty() {
            return Ok(StagedSave::Written);
        }
        let conflicted = self.conflicts(paths);
        if !conflicted.is_empty() {
            return Ok(StagedSave::Conflicted(conflicted));
        }
        let writes: Vec<(PathBuf, String, String)> = self
            .dirty_files(paths)
            .into_iter()
            .map(|(side, path)| {
                (
                    path,
                    self.content.buffer(side).to_text(),
                    self.content.baseline_text(side),
                )
            })
            .collect();
        crate::write::commit_all_or_nothing(&writes)?;

        let hashes = Pair::from_fn(|side| {
            if *rehash.side(side) {
                crate::diff::compute_file_sha256(paths.side(side)).ok()
            } else {
                self.content.hash(side).map(str::to_string)
            }
        });
        self.content.commit_baselines(hashes);
        self.content.recompute_rows();
        self.content.clamp_scroll();
        Ok(StagedSave::Written)
    }

    /// Each dirty side with the file a save writes it to, left side first.
    fn dirty_files(&self, paths: &Pair<PathBuf>) -> Vec<(Side, PathBuf)> {
        Side::BOTH
            .into_iter()
            .filter(|&side| self.content.dirty(side))
            .map(|side| (side, paths.side(side).clone()))
            .collect()
    }

    /// Check each dirty side against its disk baseline; returns absolute paths
    /// of files that changed on disk underneath the session.
    fn conflicts(&self, paths: &Pair<PathBuf>) -> Vec<PathBuf> {
        self.dirty_files(paths)
            .into_iter()
            .filter(|(side, path)| {
                crate::diff::compute_file_sha256(path).ok().as_deref() != self.content.hash(*side)
            })
            .map(|(_, path)| crate::write::absolute_lexical(&path))
            .collect()
    }
}

/// The background load: which one is current, since when, and the job the
/// event loop has not picked up yet. Cancellation retires the request; a
/// filesystem read already in progress may still finish.
#[derive(Default)]
pub(crate) struct LoadState {
    generation: u64,
    started: Option<Instant>,
    queued: Option<LoadJob>,
    cancelled: Option<Arc<AtomicBool>>,
}

impl LoadState {
    pub(crate) fn in_progress(&self) -> bool {
        self.started.is_some()
    }

    pub(crate) fn elapsed_seconds(&self) -> Option<u64> {
        self.started.map(|start| start.elapsed().as_secs())
    }

    fn request(&mut self, files: Pair<PathBuf>, hint: String, context: usize, wrap: bool) {
        self.cancel();
        self.started = Some(Instant::now());
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancelled = Some(cancelled.clone());
        self.queued = Some(LoadJob {
            generation: self.generation,
            files,
            hint,
            context,
            wrap,
            cancelled,
        });
    }

    fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.generation = self.generation.wrapping_add(1);
        self.started = None;
        self.queued = None;
    }

    fn take_job(&mut self) -> Option<LoadJob> {
        self.queued.take()
    }

    fn finish(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.in_progress() {
            return false;
        }
        self.started = None;
        self.cancelled = None;
        true
    }
}

/// One load for a worker thread to run.
pub(crate) struct LoadJob {
    pub(crate) generation: u64,
    files: Pair<PathBuf>,
    hint: String,
    context: usize,
    wrap: bool,
    cancelled: Arc<AtomicBool>,
}

impl LoadJob {
    /// Read both sides and diff them. Blocks; run it off the UI thread.
    pub(crate) fn load(self) -> Result<Box<FileDiffState>, String> {
        let loaded = self.files.try_map(|path| {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            crate::diff_view::LoadedText::from_path(&path, &self.hint).map_err(|e| e.to_string())
        })?;
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let mut diff = FileDiffState::with_context(self.context);
        diff.load(loaded);
        if self.wrap {
            diff.toggle_wrap();
        }
        Ok(Box::new(diff))
    }
}

/// Run the load File Diff asked for, if any, on a blocking worker, and send
/// its result back as [`AppEvent::FileDiffLoaded`].
pub(crate) fn run_requests(app: &mut super::App, tx: &tokio::sync::mpsc::Sender<AppEvent>) {
    run_requests_with(app, tx, LoadJob::load);
}

/// [`run_requests`] with the load itself substituted, for a test to hold it.
pub(crate) fn run_requests_with(
    app: &mut super::App,
    tx: &tokio::sync::mpsc::Sender<AppEvent>,
    load: impl FnOnce(LoadJob) -> Result<Box<FileDiffState>, String> + Send + 'static,
) {
    let Some(job) = app.take_file_diff_job() else {
        return;
    };
    let generation = job.generation;
    let tx = tx.clone();
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || load(job))
            .await
            .unwrap_or_else(|error| Err(format!("file diff worker failed: {error}")));
        let _ = tx
            .send(AppEvent::FileDiffLoaded { generation, result })
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_view::{HunkCopyDirection, LoadedText};
    use std::fs;

    /// A session on `left` and `right` with the left side's one change staged
    /// onto the right.
    fn staged(left: &str, right: &str) -> (tempfile::TempDir, Pair<PathBuf>, FileDiffSession) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Pair::new(dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&paths.left, left).unwrap();
        fs::write(&paths.right, right).unwrap();
        let mut session = FileDiffSession::with_context(3);
        session.open_loaded(
            paths
                .as_ref()
                .map(|path| LoadedText::from_bytes(fs::read(path).unwrap()).unwrap()),
        );
        session.content_mut().set_show_full(true);
        session.content_mut().recompute_rows();
        session.content_mut().set_frame(10, 40);
        session.content_mut().set_scroll(1);
        assert!(session
            .content_mut()
            .stage_active_hunk(HunkCopyDirection::LeftToRight)
            .unwrap());
        (dir, paths, session)
    }

    #[test]
    fn a_save_writes_the_staged_side_and_clears_dirty() {
        let (_dir, paths, mut session) = staged("keep\nleft\n", "keep\nright\n");
        assert_eq!(
            session.save(&paths, Pair::new(true, true)).unwrap(),
            StagedSave::Written
        );
        assert_eq!(fs::read_to_string(&paths.right).unwrap(), "keep\nleft\n");
        assert!(!session.content().is_dirty());
    }

    #[test]
    fn a_save_over_a_file_changed_on_disk_writes_nothing() {
        let (_dir, paths, mut session) = staged("keep\nleft\n", "keep\nright\n");
        fs::write(&paths.right, "edited elsewhere\n").unwrap();
        let StagedSave::Conflicted(conflicted) =
            session.save(&paths, Pair::new(true, true)).unwrap()
        else {
            panic!("an external edit must stop the save");
        };
        assert_eq!(conflicted.len(), 1);
        assert!(conflicted[0].ends_with("b.txt"));
        assert_eq!(
            fs::read_to_string(&paths.right).unwrap(),
            "edited elsewhere\n"
        );
        assert!(session.content().is_dirty());
    }

    #[test]
    fn a_load_cancelled_by_closing_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Pair::new(dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&paths.left, "a\n").unwrap();
        fs::write(&paths.right, "b\n").unwrap();
        let mut session = FileDiffSession::with_context(3);
        session.open(FlatRow::default(), paths, String::new(), 3);
        let job = session.take_job().unwrap();
        let generation = job.generation;
        session.close();
        assert_eq!(session.finish(generation, job.load()), LoadOutcome::Stale);
        assert!(session.content().rows().is_empty());
        assert!(session.opened_row().is_none());
    }
}
