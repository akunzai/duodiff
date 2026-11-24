//! The inline File Diff panel's own state: its enabled toggle and the
//! background-open error it tracks against the selected row.
//!
//! Owned by [`App::inline_panel`](super::App::inline_panel)/
//! [`App::inline_panel_mut`](super::App::inline_panel_mut); production code
//! reaches it only through the `App` methods that wrap these.

use std::path::{Path, PathBuf};

/// What the inline File Diff panel shows for the selected Directory Tree row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InlineFileDiffState<'a> {
    /// No panel: disabled, a directory row, nothing selected, or a different
    /// screen.
    Hidden,
    Loading,
    /// The selected file could not be opened in the built-in diff.
    Unsupported(&'a str),
    /// The shared `FileDiffSession` is open on the selected row.
    Ready,
}

#[derive(Default)]
pub(crate) struct InlineFileDiffPanel {
    /// Whether the panel feature is enabled. Starts disabled; toggled
    /// through the `ToggleInlinePanel` Command (`i` by default). Enabling it
    /// does not by itself paint the panel — that still follows the selected
    /// row and the terminal's size.
    enabled: bool,
    /// A background open that failed, for the panel to report against its
    /// row: `FileDiffSession` forgets the row once an open fails (ADR-0007),
    /// so this is captured before that happens.
    error: Option<(PathBuf, String)>,
}

impl InlineFileDiffPanel {
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Flip the panel on or off, and report the new state.
    pub(crate) fn toggle_enabled(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }

    /// The background-open error held against `path`, if any.
    pub(crate) fn error_for(&self, path: &Path) -> Option<&str> {
        self.error
            .as_ref()
            .and_then(|(p, message)| (p == path).then_some(message.as_str()))
    }

    pub(crate) fn set_error(&mut self, path: PathBuf, message: String) {
        self.error = Some((path, message));
    }

    pub(crate) fn clear_error(&mut self) {
        self.error = None;
    }
}
