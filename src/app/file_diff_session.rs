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
pub(crate) use crate::target::SideSource;
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

/// Read both sides, naming the side that failed and, through `hint`, the way
/// out of content the built-in diff cannot show (Issue #339).
fn load_sides(
    sources: &Pair<SideSource>,
    hint: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Pair<crate::text::LoadedText>, String> {
    sources.as_ref().try_map(|source| {
        if cancelled() {
            return Err("cancelled".into());
        }
        source.load().map_err(|cause| {
            format!(
                "{cause}: {}{hint}",
                crate::fit::truncate_path_left(source.path(), 32)
            )
        })
    })
}

/// What a worker hands back: the content to show, and each side's size and
/// modification time as it was read.
#[derive(Debug)]
pub struct Loaded {
    content: FileDiffState,
    info: Pair<Option<crate::diff::FileInfo>>,
}

/// What a finished load did.
#[derive(Debug, PartialEq)]
pub(crate) enum LoadOutcome {
    /// File Diff shows the pair.
    Opened,
    /// File Diff could not open the pair; it shows nothing.
    OpenFailed(String),
    /// File Diff shows the pair as read again, staged edits gone. `message`
    /// is what the reload asked to say once it worked; `info` is each side's
    /// size and modification time, for a file named on the command line.
    Reloaded {
        message: Option<String>,
        info: Pair<Option<crate::diff::FileInfo>>,
    },
    /// The pair could not be read again; File Diff and its staged edits are
    /// as they were.
    ReloadFailed(String),
    /// A load the user cancelled or replaced; nothing changed.
    Stale,
}

/// Why a load runs, which decides what its result does.
enum LoadKind {
    Open,
    Reload { message: Option<String> },
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

    /// Open `sources` — `row` under each root — in the background, keeping
    /// the wrap setting and diffing with `context` lines. `hint` names the way
    /// out of a file the built-in diff cannot show.
    pub(crate) fn open(
        &mut self,
        row: FlatRow,
        sources: Pair<SideSource>,
        hint: String,
        context: usize,
    ) {
        let wrap = self.content.wrap();
        self.opened_row = Some(row);
        self.content = FileDiffState::with_context(context);
        if wrap {
            self.content.toggle_wrap();
        }
        self.loading
            .request(LoadKind::Open, sources, hint, self.content.clone());
    }

    /// Open on content already read, diff-only and scrolled to the top.
    pub(crate) fn open_loaded(&mut self, loaded: Pair<crate::text::LoadedText>) {
        self.content.set_show_full(false);
        self.content.load(loaded);
        self.content.reset_scroll();
    }

    /// Read `sources` again in the background, to replace the content and its
    /// staged edits once both sides are read. Until then File Diff shows what
    /// it showed; a failed or cancelled reload leaves it so. `message` is what
    /// to say once it worked.
    pub(crate) fn reload(
        &mut self,
        sources: Pair<SideSource>,
        hint: String,
        message: Option<String>,
    ) {
        self.loading.request(
            LoadKind::Reload { message },
            sources,
            hint,
            self.content.clone(),
        );
    }

    /// Whether the load in flight is a reload, which Back cancels without
    /// leaving File Diff.
    pub(crate) fn is_reloading(&self) -> bool {
        matches!(self.loading.kind, Some(LoadKind::Reload { .. }))
    }

    /// Stop a reload in flight, keeping what File Diff shows. Whether there
    /// was one.
    pub(crate) fn cancel_reload(&mut self) -> bool {
        let reloading = self.is_reloading();
        if reloading {
            self.loading.cancel();
        }
        reloading
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
        result: Result<Box<Loaded>, String>,
    ) -> LoadOutcome {
        let Some(kind) = self.loading.finish(generation) else {
            return LoadOutcome::Stale;
        };
        match (kind, result) {
            (LoadKind::Open, Ok(loaded)) => {
                self.content = loaded.content;
                LoadOutcome::Opened
            }
            (LoadKind::Open, Err(error)) => {
                self.opened_row = None;
                LoadOutcome::OpenFailed(error)
            }
            (LoadKind::Reload { message }, Ok(mut loaded)) => {
                // Scrolling is the one thing File Diff still takes while a
                // reload reads; keep where the user scrolled to.
                loaded.content.scroll_like(&self.content);
                self.content = loaded.content;
                LoadOutcome::Reloaded {
                    message,
                    info: loaded.info,
                }
            }
            (LoadKind::Reload { .. }, Err(error)) => LoadOutcome::ReloadFailed(error),
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
    pub(crate) fn save_targets(&self, sources: &Pair<SideSource>) -> Vec<PathBuf> {
        self.dirty_files(sources)
            .into_iter()
            .map(|(_, path)| crate::write::absolute_lexical(&path))
            .collect()
    }

    /// Write every dirty side to its source, all-or-nothing.
    ///
    /// Each side is staged into a temporary file in its own directory first, so
    /// the visible file is only replaced once both writes are known to have
    /// worked; a failure part-way restores the originals rather than leaving one
    /// side written and the other not (Issue #235).
    ///
    /// Returns [`StagedSave::Conflicted`] with the offending paths when the
    /// on-disk content no longer matches the session baseline; nothing is
    /// written and the caller decides what to ask the user. A side that is
    /// not a regular file (the null device, a pipe) was never written and
    /// keeps its hash.
    pub(crate) fn save(
        &mut self,
        sources: &Pair<SideSource>,
    ) -> Result<StagedSave, std::io::Error> {
        if !self.content.is_dirty() {
            return Ok(StagedSave::Written);
        }
        let conflicted = self.conflicts(sources);
        if !conflicted.is_empty() {
            return Ok(StagedSave::Conflicted(conflicted));
        }
        let writes: Vec<(PathBuf, String, String)> = self
            .dirty_files(sources)
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
            let source = sources.side(side);
            if source.is_written_to_disk() {
                crate::diff::compute_file_sha256(source.path()).ok()
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
    fn dirty_files(&self, sources: &Pair<SideSource>) -> Vec<(Side, PathBuf)> {
        Side::BOTH
            .into_iter()
            .filter(|&side| self.content.dirty(side))
            .map(|side| (side, sources.side(side).path().to_path_buf()))
            .collect()
    }

    /// Check each dirty side against its disk baseline; returns absolute paths
    /// of files that changed on disk underneath the session.
    fn conflicts(&self, sources: &Pair<SideSource>) -> Vec<PathBuf> {
        self.dirty_files(sources)
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
    kind: Option<LoadKind>,
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

    fn request(
        &mut self,
        kind: LoadKind,
        sources: Pair<SideSource>,
        hint: String,
        template: FileDiffState,
    ) {
        self.cancel();
        self.started = Some(Instant::now());
        self.kind = Some(kind);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.cancelled = Some(cancelled.clone());
        self.queued = Some(LoadJob {
            generation: self.generation,
            sources,
            hint,
            template,
            cancelled,
        });
    }

    fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.generation = self.generation.wrapping_add(1);
        self.started = None;
        self.kind = None;
        self.queued = None;
    }

    fn take_job(&mut self) -> Option<LoadJob> {
        self.queued.take()
    }

    /// The kind of the current load, when `generation` is it.
    fn finish(&mut self, generation: u64) -> Option<LoadKind> {
        if generation != self.generation || !self.in_progress() {
            return None;
        }
        self.started = None;
        self.cancelled = None;
        self.kind.take()
    }
}

/// One load for a worker thread to run.
pub(crate) struct LoadJob {
    pub(crate) generation: u64,
    sources: Pair<SideSource>,
    hint: String,
    /// What File Diff showed when the load was asked for — its settings,
    /// scroll, and wrap — for the read content to replace.
    template: FileDiffState,
    cancelled: Arc<AtomicBool>,
}

impl LoadJob {
    /// Read both sides and diff them. Blocks; run it off the UI thread.
    pub(crate) fn load(self) -> Result<Box<Loaded>, String> {
        let cancelled = || self.cancelled.load(Ordering::Relaxed);
        let loaded = load_sides(&self.sources, &self.hint, cancelled)?;
        if cancelled() {
            return Err("cancelled".into());
        }
        let info = self.sources.as_ref().map(SideSource::info);
        let mut content = self.template;
        content.load(loaded);
        content.clamp_scroll();
        Ok(Box::new(Loaded { content, info }))
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
    load: impl FnOnce(LoadJob) -> Result<Box<Loaded>, String> + Send + 'static,
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
    use crate::diff_view::staging::HunkCopyDirection;
    use crate::text::LoadedText;
    use std::fs;

    /// A session on `left` and `right` with the left side's one change staged
    /// onto the right.
    fn staged(left: &str, right: &str) -> (tempfile::TempDir, Pair<SideSource>, FileDiffSession) {
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
        (dir, paths.map(SideSource::entry), session)
    }

    const HINT: &str = " (press D for external diff)";

    fn load_one(source: SideSource) -> Result<LoadedText, String> {
        let other = SideSource::null_device(PathBuf::from("/dev/null"));
        load_sides(&Pair::new(source, other), HINT, || false).map(|pair| pair.left)
    }

    #[test]
    fn a_row_side_with_nothing_there_loads_empty() {
        let missing = PathBuf::from("/nonexistent/duodiff-missing.txt");
        assert_eq!(
            load_one(SideSource::entry(missing.clone())).unwrap(),
            LoadedText::default()
        );
        assert!(
            load_one(SideSource::file(missing)).is_err(),
            "a file named on the command line must still be there"
        );
    }

    #[test]
    fn content_the_built_in_diff_cannot_show_names_the_file_and_the_way_out() {
        let dir = tempfile::tempdir().unwrap();
        let cases: [(&str, &[u8], &str); 2] = [
            ("binary.bin", b"hello\0world", "binary"),
            ("latin1.txt", &[0xC3, 0x28], "non-UTF-8"),
        ];
        for (name, bytes, reason) in cases {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            let error = load_one(SideSource::entry(path)).unwrap_err();
            assert!(error.contains(reason), "{error}");
            assert!(error.contains(name), "{error}");
            assert!(error.contains("press D for external diff"), "{error}");
        }

        let big = dir.path().join("big.txt");
        fs::File::create(&big)
            .unwrap()
            .set_len(crate::text::MAX_DIFF_FILE_BYTES + 1)
            .unwrap();
        let error = load_one(SideSource::entry(big)).unwrap_err();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn a_side_reads_its_bytes_once_for_text_hash_and_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crlf.txt");
        fs::write(&path, "a\r\nb\r\n").unwrap();
        let loaded = load_one(SideSource::entry(path)).unwrap();
        assert_eq!(loaded.text, "a\nb\n");
        assert_eq!(loaded.line_ending.as_deref(), Some("CRLF"));
        assert_eq!(
            loaded.sha256.as_deref(),
            Some(crate::diff::sha256_hex(b"a\r\nb\r\n").as_str())
        );
    }

    #[test]
    fn a_save_writes_the_staged_side_and_clears_dirty() {
        let (_dir, paths, mut session) = staged("keep\nleft\n", "keep\nright\n");
        assert_eq!(session.save(&paths).unwrap(), StagedSave::Written);
        assert_eq!(
            fs::read_to_string(paths.right.path()).unwrap(),
            "keep\nleft\n"
        );
        assert!(!session.content().is_dirty());
    }

    #[test]
    fn a_save_over_a_file_changed_on_disk_writes_nothing() {
        let (_dir, paths, mut session) = staged("keep\nleft\n", "keep\nright\n");
        fs::write(paths.right.path(), "edited elsewhere\n").unwrap();
        let StagedSave::Conflicted(conflicted) = session.save(&paths).unwrap() else {
            panic!("an external edit must stop the save");
        };
        assert_eq!(conflicted.len(), 1);
        assert!(conflicted[0].ends_with("b.txt"));
        assert_eq!(
            fs::read_to_string(paths.right.path()).unwrap(),
            "edited elsewhere\n"
        );
        assert!(session.content().is_dirty());
    }

    /// Run the queued load on this thread and take its result.
    fn finish_now(session: &mut FileDiffSession) -> LoadOutcome {
        let job = session.take_job().expect("a queued load");
        let generation = job.generation;
        session.finish(generation, job.load())
    }

    #[test]
    fn a_reload_replaces_the_content_and_its_staged_edits_once_read() {
        let tail: String = (0..30).map(|i| format!("tail {i}\n")).collect();
        let (_dir, sources, mut session) = staged(
            &format!("keep\nleft\n{tail}"),
            &format!("keep\nright\n{tail}"),
        );
        fs::write(sources.right.path(), format!("keep\nnewer\n{tail}")).unwrap();

        session.reload(sources, String::new(), Some("done".into()));
        // The user scrolls while the reload reads.
        session.content_mut().set_scroll(5);
        assert!(session.is_reloading());
        assert!(session.content().is_dirty(), "nothing changes until read");

        let LoadOutcome::Reloaded { message, .. } = finish_now(&mut session) else {
            panic!("the reload should succeed");
        };
        assert_eq!(message.as_deref(), Some("done"));
        assert!(!session.content().is_dirty());
        assert_eq!(session.content().buffer(Side::Right).lines[1], "newer");
        assert_eq!(session.content().scroll(), 5, "the user's scroll stays");
    }

    #[test]
    fn a_reload_that_fails_or_is_cancelled_keeps_the_staged_edits() {
        let (_dir, sources, mut session) = staged("keep\nleft\n", "keep\nright\n");
        fs::write(sources.right.path(), b"now\0binary").unwrap();

        session.reload(sources.clone(), String::new(), None);
        let LoadOutcome::ReloadFailed(error) = finish_now(&mut session) else {
            panic!("binary content cannot be shown");
        };
        assert!(error.contains("binary"), "{error}");
        assert!(session.content().is_dirty());

        session.reload(sources, String::new(), None);
        let job = session.take_job().unwrap();
        assert!(session.cancel_reload());
        assert_eq!(
            session.finish(job.generation, job.load()),
            LoadOutcome::Stale
        );
        assert!(session.content().is_dirty());
        assert!(!session.cancel_reload(), "nothing left to cancel");
    }

    #[test]
    fn a_load_cancelled_by_closing_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Pair::new(dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&paths.left, "a\n").unwrap();
        fs::write(&paths.right, "b\n").unwrap();
        let mut session = FileDiffSession::with_context(3);
        session.open(
            FlatRow::default(),
            paths.map(SideSource::entry),
            String::new(),
            3,
        );
        let job = session.take_job().unwrap();
        let generation = job.generation;
        session.close();
        assert_eq!(session.finish(generation, job.load()), LoadOutcome::Stale);
        assert!(session.content().rows().is_empty());
        assert!(session.opened_row().is_none());
    }
}
