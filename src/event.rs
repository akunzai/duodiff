use crate::diff::AlignedNode;
use crossterm::event::{self, Event as CrosstermEvent};
use std::io::IsTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    Terminal(CrosstermEvent),
    /// Background scan finished. `generation` must match `App::scan_generation`
    /// or the result is stale and should be ignored.
    ScanFinished {
        generation: u64,
        node: Box<AlignedNode>,
    },
    /// Periodic progress report from an active background scan.
    ScanProgress {
        generation: u64,
        count: usize,
    },
    /// Background scan (or other async work) failed. `generation` is checked
    /// the same way as [`AppEvent::ScanFinished`].
    Error {
        generation: u64,
        message: String,
    },
    /// A Command effect that outlived its synchronous execution failed. Carries
    /// the canonical failure text, so work that cannot report through
    /// `Outcome` still reaches the user (Issue #282).
    CommandFailed {
        message: String,
    },
    Tick,
    UpdateCheckOutcome(crate::upgrade::UpdateCheckOutcome),
}

pub struct EventHandler {
    pub rx: mpsc::Receiver<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> (Self, mpsc::Sender<AppEvent>) {
        let (tx, rx) = mpsc::channel(100);
        let terminal_tx = tx.clone();

        // Only poll real terminal events when running in an interactive TTY.
        // In test / CI environments (non-TTY) crossterm::event::poll() can hang
        // indefinitely, so we skip the listener entirely and rely solely on the
        // channel filled by the test harness.
        if std::io::stdout().is_terminal() && !cfg!(test) {
            tokio::spawn(async move {
                loop {
                    if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                        if let Ok(evt) = event::read() {
                            let _ = terminal_tx.send(AppEvent::Terminal(evt)).await;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            });
        }

        let tick_tx = tx.clone();
        // Spawn ticker
        tokio::spawn(async move {
            loop {
                let _ = tick_tx.send(AppEvent::Tick).await;
                tokio::time::sleep(tick_rate).await;
            }
        });

        (Self { rx }, tx)
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tick_event() {
        let (mut handler, _tx) = EventHandler::new(Duration::from_millis(10));
        // We expect to receive a Tick event within a short time.
        // But since we didn't spawn the tick task, this will fail or return None (or hang, so we use timeout).
        let event = tokio::time::timeout(Duration::from_millis(50), handler.next()).await;

        let opt = event.expect("Timeout waiting for event");
        let evt = opt.expect("Expected Some(AppEvent::Tick), got None");
        assert!(matches!(evt, AppEvent::Tick), "Expected AppEvent::Tick");
    }
}
