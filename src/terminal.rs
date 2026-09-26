//! Handing the terminal to an external diff tool or editor and taking it
//! back: the "TTY recovery" and "Editor handoff" invariants
//! (`docs/agents/tui.md`), behind a guard a test can replace (Issue #304).

use crate::diff_tool::{self, ExternalDiffTool};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::path::PathBuf;

/// Pure IO intent from a key press — built without performing IO.
/// [`dispatch_key_outcome`] performs the process spawn / terminal handoff.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyOutcome {
    LaunchDiff {
        tool: ExternalDiffTool,
        left: PathBuf,
        right: PathBuf,
    },
    LaunchEditor {
        path: PathBuf,
    },
}

/// Suspends the TUI while an external process owns the terminal, and restores it
/// when released.
///
/// The seam the "TTY recovery" invariant lives on: [`with_terminal_handoff`] is
/// generic over this, so a test can substitute a recorder and assert the guard
/// really wraps the spawn (Issue #304).
pub(crate) trait TerminalGuard: Sized {
    /// Suspend the TUI. Restoring happens on `Drop`.
    fn acquire(mouse_enabled: bool) -> std::io::Result<Self>;

    /// Turn mouse capture on or off while the TUI owns the terminal.
    fn set_mouse_capture(on: bool) -> std::io::Result<()>;
}

/// Leaves raw mode + the alternate screen on construction (unless stdout isn't a real
/// terminal — see the "TTY recovery" invariant in docs/agents/tui.md), and restores both
/// on `Drop`.
/// Callers hold this across the external process and drop it **before** re-clearing the TUI.
pub(crate) struct RealTerminalGuard {
    mouse_enabled: bool,
    is_terminal: bool,
}

impl TerminalGuard for RealTerminalGuard {
    fn acquire(mouse_enabled: bool) -> std::io::Result<Self> {
        use std::io::IsTerminal;
        let is_terminal = std::io::stdout().is_terminal();
        if is_terminal {
            Self::suspend(mouse_enabled)?;
        }
        Ok(Self {
            mouse_enabled,
            is_terminal,
        })
    }

    fn set_mouse_capture(on: bool) -> std::io::Result<()> {
        use std::io::IsTerminal;
        if !std::io::stdout().is_terminal() {
            return Ok(());
        }
        if on {
            execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
        } else {
            execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)
        }
    }
}

impl RealTerminalGuard {
    fn suspend(mouse_enabled: bool) -> std::io::Result<()> {
        disable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            )
        } else {
            execute!(std::io::stdout(), LeaveAlternateScreen)
        }
    }

    fn resume(mouse_enabled: bool) -> std::io::Result<()> {
        enable_raw_mode()?;
        if mouse_enabled {
            execute!(
                std::io::stdout(),
                EnterAlternateScreen,
                crossterm::event::EnableMouseCapture
            )
        } else {
            execute!(std::io::stdout(), EnterAlternateScreen)
        }
    }
}

impl Drop for RealTerminalGuard {
    fn drop(&mut self) {
        if !self.is_terminal {
            return;
        }
        // Drop can't propagate a Result, so a restore failure is reported rather than
        // silently swallowed — but there's nothing more this guard can do about it.
        if let Err(e) = Self::resume(self.mouse_enabled) {
            eprintln!("Failed to restore terminal after external process: {e}");
        }
    }
}

fn wait_for_enter() {
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}

/// Spawn an external diff tool. Caller must keep the TUI suspended (e.g. hold
/// [`RealTerminalGuard`]) across this call and only clear after that guard drops.
pub(crate) fn run_external_diff(
    tool: &diff_tool::ExternalDiffTool,
    left: &std::path::Path,
    right: &std::path::Path,
) {
    match diff_tool::open_diff(tool, left, right) {
        Err(e) => {
            eprintln!(
                "Error launching external diff: {}. Press Enter to continue...",
                e
            );
            wait_for_enter();
        }
        Ok(()) if matches!(tool, diff_tool::ExternalDiffTool::Difftastic) => {
            println!("\nPress Enter to return to duodiff...");
            wait_for_enter();
        }
        Ok(()) => {}
    }
}

/// Spawn `$EDITOR`/`$VISUAL`. Same handoff contract as [`run_external_diff`].
pub(crate) fn run_external_editor(file_path: &std::path::Path) {
    if let Err(e) = diff_tool::open_editor(file_path) {
        eprintln!(
            "Error launching external editor: {}. Press Enter to continue...",
            e
        );
        wait_for_enter();
    }
}

/// Suspend the TUI, run `body`, restore the TUI, then clear the alt-screen buffer.
fn with_terminal_handoff<B: ratatui::backend::Backend, G: TerminalGuard>(
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
    body: impl FnOnce(),
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    {
        let _guard = G::acquire(mouse_enabled)?;
        body();
    }
    // Only after the guard is released: clearing while the external process still
    // owns the terminal would paint into its screen.
    terminal.clear()?;
    Ok(())
}

/// Perform the IO a [`KeyOutcome`] describes. Pure key-handling code only builds a
/// `KeyOutcome`; process spawn and terminal mode toggling live here.
pub(crate) fn dispatch_key_outcome<B: ratatui::backend::Backend, G: TerminalGuard>(
    outcome: KeyOutcome,
    terminal: &mut ratatui::Terminal<B>,
    mouse_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match outcome {
        KeyOutcome::LaunchDiff { tool, left, right } => {
            with_terminal_handoff::<B, G>(terminal, mouse_enabled, || {
                run_external_diff(&tool, &left, &right);
            })
        }
        KeyOutcome::LaunchEditor { path } => {
            with_terminal_handoff::<B, G>(terminal, mouse_enabled, || {
                run_external_editor(&path);
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env_tests, RecordingBackend, RecordingTerminalGuard};

    /// A terminal showing `text`, backed by the recorder so the handoff's
    /// `clear` lands in the same log the guard writes to.
    fn terminal_showing(text: &str) -> ratatui::Terminal<RecordingBackend> {
        let mut terminal = ratatui::Terminal::new(RecordingBackend::new(20, 2)).unwrap();
        terminal
            .draw(|f| f.render_widget(ratatui::widgets::Paragraph::new(text), f.area()))
            .unwrap();
        terminal
    }

    /// The guard must wrap the spawn, and the screen must be cleared only after it
    /// is released — the "TTY recovery" and "Editor handoff" invariants (#304).
    ///
    /// Only the editor launch is driven end to end: `run_external_diff` waits on
    /// stdin when the tool fails, and `ExternalDiffTool::as_str()` names a real
    /// binary with no env-var override (unlike `$EDITOR`), so there is no fast,
    /// portable way to make it succeed in CI. Both launches go through
    /// `with_terminal_handoff`, so what stays uncovered is one match arm.
    #[test]
    fn test_dispatch_wraps_the_editor_spawn_in_the_terminal_guard() {
        let _lock = lock_env_tests();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");
        assert!(terminal.backend().rendered().contains("leftovers"));

        dispatch_key_outcome::<_, RecordingTerminalGuard>(
            KeyOutcome::LaunchEditor {
                path: PathBuf::from("dummy.txt"),
            },
            &mut terminal,
            true,
        )
        .unwrap();

        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=true)".to_string(),
                "resume(mouse_enabled=true)".to_string(),
                "clear".to_string(),
            ],
            "the spawn must happen inside the guard, with mouse capture released,
             and the screen cleared only once the guard has let go"
        );
        assert!(
            !terminal.backend().rendered().contains("leftovers"),
            "the screen must be cleared once the external program is done"
        );
    }

    /// The spawn must happen *between* suspend and resume, not beside them.
    ///
    /// Records the body into the same log the guard writes, so a handoff that
    /// released the guard before running the body fails here rather than passing
    /// on a log that looks the same from outside (#304).
    #[test]
    fn test_the_guard_wraps_the_spawn_rather_than_bracketing_it() {
        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");

        with_terminal_handoff::<_, RecordingTerminalGuard>(&mut terminal, true, || {
            RecordingTerminalGuard::record("spawn".to_string());
        })
        .unwrap();

        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=true)".to_string(),
                "spawn".to_string(),
                "resume(mouse_enabled=true)".to_string(),
                "clear".to_string(),
            ]
        );
    }

    /// A crash inside the external program must not cost the user their terminal.
    ///
    /// Driven through `with_terminal_handoff` rather than the dispatcher: the
    /// dispatcher builds its body from the outcome, so a panic cannot be injected
    /// there, and this is production code too — not a stand-in.
    #[test]
    fn test_terminal_guard_resumes_even_if_the_spawn_panics() {
        RecordingTerminalGuard::reset_log();
        let mut terminal = terminal_showing("leftovers");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_terminal_handoff::<_, RecordingTerminalGuard>(&mut terminal, false, || {
                panic!("simulated failure mid external-process handoff");
            })
        }));

        assert!(result.is_err());
        assert_eq!(
            RecordingTerminalGuard::log(),
            vec![
                "suspend(mouse_enabled=false)".to_string(),
                "resume(mouse_enabled=false)".to_string(),
            ],
            "the guard must restore the terminal while unwinding"
        );
        assert!(
            terminal.backend().rendered().contains("leftovers"),
            "the clear sits after the guard, so unwinding skips it"
        );
    }
}
