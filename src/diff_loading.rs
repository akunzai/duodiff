//! Background loading for a Directory Tree's File Diff. Cancellation retires
//! the request; a filesystem read already in progress may still finish.
use crate::app::{App, FileDiffState};
use crate::event::AppEvent;
use crate::side::Pair;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

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
    pub(crate) fn request(
        &mut self,
        files: Pair<PathBuf>,
        hint: String,
        context: usize,
        wrap: bool,
    ) {
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
    pub(crate) fn cancel(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.generation = self.generation.wrapping_add(1);
        self.started = None;
        self.queued = None;
    }
    pub(crate) fn take_job(&mut self) -> Option<LoadJob> {
        self.queued.take()
    }
    pub(crate) fn finish(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.in_progress() {
            return false;
        }
        self.started = None;
        self.cancelled = None;
        true
    }
}

pub(crate) struct LoadJob {
    pub(crate) generation: u64,
    files: Pair<PathBuf>,
    hint: String,
    context: usize,
    wrap: bool,
    cancelled: Arc<AtomicBool>,
}

impl LoadJob {
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

pub(crate) fn run_requests(app: &mut App, tx: &tokio::sync::mpsc::Sender<AppEvent>) {
    run_requests_with(app, tx, LoadJob::load);
}

pub(crate) fn run_requests_with(
    app: &mut App,
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
