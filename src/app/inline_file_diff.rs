//! The inline File Diff panel's own state (`GLOSSARY.md`): the Directory
//! Tree's three-way keyboard focus, the panel's visibility/follow bookkeeping,
//! its enabled toggle, and the pending row-change / background-open error it
//! tracks against the selected row.
//!
//! Owned by [`App::inline_panel`](super::App::inline_panel)/
//! [`App::inline_panel_mut`](super::App::inline_panel_mut); production code
//! reaches it only through the `App` methods that wrap these (which also
//! coordinate `active_side` and the shared `FileDiffSession`, both owned by
//! `App` since they serve the Directory Tree and full-screen File Diff
//! alike).

use crate::side::Side;
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

/// Which of the Directory Tree's three panes has keyboard focus: a tree
/// pane (`App::active_side` says which) or the inline File Diff panel.
/// Switching to the panel never touches the shared `FileDiffSession`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum TreeFocus {
    #[default]
    Pane,
    InlinePanel,
}

/// `Tab`'s cycle direction, shared by [`InlineFileDiffPanel::cycle_focus`]
/// (forward) and its `Shift+Tab` mirror (backward).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Forward,
    Backward,
}

#[derive(Default)]
pub(crate) struct InlineFileDiffPanel {
    tree_focus: TreeFocus,
    /// Whether the panel is actually painted this frame — the layout may
    /// hide it even when `enabled` wants it, on a terminal too short for
    /// both it and the tree. Kept so input routing and Command availability
    /// never target a panel that is not on screen.
    visible: bool,
    /// Set by `J`/`K` cross-navigation when it moved the Directory Tree's
    /// selection because the panel had nothing to scroll; consumed the next
    /// time the panel has something to show, so arriving on a file after
    /// walking through directories or unsupported rows resumes focus on the
    /// panel without an extra keypress. An explicit focus change cancels it.
    follow: bool,
    /// A Directory Tree row the user tried to select while the shared
    /// `FileDiffSession` is dirty on a different row — the move
    /// `Commands::sync_inline_file_diff` is asking to confirm, held here so
    /// `ConfirmAction::Cancel` leaves it unapplied for free and a confirmed
    /// Save or Discard can apply it afterward (ADR-0008).
    pending_row_change: Option<PathBuf>,
    /// Whether the panel feature is enabled. Starts disabled; toggled
    /// through the `ToggleInlinePanel` Command (`i` by default). Enabling it
    /// does not by itself paint the panel — that still follows the selected
    /// row and the terminal's size (`visible`).
    enabled: bool,
    /// A background open that failed, for the panel to report against its
    /// row: `FileDiffSession` forgets the row once an open fails (ADR-0007),
    /// so this is captured before that happens.
    error: Option<(PathBuf, String)>,
}

impl InlineFileDiffPanel {
    pub(crate) fn tree_focus_is_inline_panel(&self) -> bool {
        self.tree_focus == TreeFocus::InlinePanel
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// Record whether this frame's layout painted the panel, and drop focus
    /// back to a tree pane if it had it and no longer can — a target that
    /// disappeared cannot keep the keyboard. The reverse: a pending follow
    /// claims focus if the panel has something to show on this very check —
    /// the one right after the `J`/`K` press that set it, since every frame
    /// clears the request whether or not it fires. Left open-ended instead,
    /// plain `j`/`k` wandering through the tree afterward would eventually
    /// trip it on an unrelated row.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if !visible && self.tree_focus == TreeFocus::InlinePanel {
            self.tree_focus = TreeFocus::Pane;
        }
        if self.follow {
            self.follow = false;
            if visible {
                self.tree_focus = TreeFocus::InlinePanel;
            }
        }
    }

    /// Move focus to the panel. A no-op on availability: the Command gate
    /// (`Commands::listed`) already requires it to be visible before this
    /// runs.
    pub(crate) fn focus(&mut self) {
        self.tree_focus = TreeFocus::InlinePanel;
        self.follow = false;
    }

    /// Drop focus to a tree pane and any pending follow — a side pane chosen
    /// on purpose (`App::focus_left_pane`/`focus_right_pane`) leaves the panel.
    pub(crate) fn reset_focus(&mut self) {
        self.tree_focus = TreeFocus::Pane;
        self.follow = false;
    }

    /// Ask to focus the panel the next time it has something to show
    /// (`set_visible`), for `J`/`K` cross-navigation that moved the tree's
    /// selection while the panel had nothing to scroll.
    pub(crate) fn request_follow(&mut self) {
        self.follow = true;
    }

    /// `Tab`'s cycle: the two tree panes, plus the panel when it is visible,
    /// wrapping back to the left pane; `Shift+Tab` runs it backward. Moving
    /// focus never touches the shared `FileDiffSession`.
    pub(crate) fn cycle_focus(&mut self, active_side: &mut Side, direction: Direction) {
        self.follow = false;
        match direction {
            Direction::Forward => match (self.tree_focus, *active_side) {
                (TreeFocus::Pane, Side::Left) => *active_side = Side::Right,
                (TreeFocus::Pane, Side::Right) if self.visible => {
                    self.tree_focus = TreeFocus::InlinePanel;
                }
                (TreeFocus::Pane, Side::Right) => *active_side = Side::Left,
                (TreeFocus::InlinePanel, _) => {
                    self.tree_focus = TreeFocus::Pane;
                    *active_side = Side::Left;
                }
            },
            Direction::Backward => match (self.tree_focus, *active_side) {
                (TreeFocus::Pane, Side::Left) if self.visible => {
                    self.tree_focus = TreeFocus::InlinePanel;
                }
                (TreeFocus::Pane, Side::Left) => *active_side = Side::Right,
                (TreeFocus::Pane, Side::Right) => *active_side = Side::Left,
                (TreeFocus::InlinePanel, _) => {
                    self.tree_focus = TreeFocus::Pane;
                    *active_side = Side::Right;
                }
            },
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Flip the panel on or off, and report the new state.
    pub(crate) fn toggle_enabled(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }

    /// Turn the panel off without touching focus or visibility bookkeeping —
    /// callers close the shared `FileDiffSession` themselves first.
    pub(crate) fn disable(&mut self) {
        self.enabled = false;
    }

    /// Hold `target` as a row change the dirty `FileDiffSession` is blocking.
    pub(crate) fn hold_row_change(&mut self, target: PathBuf) {
        self.pending_row_change = Some(target);
    }

    /// Forget a held row change without applying it — `ConfirmAction::Cancel`.
    pub(crate) fn clear_pending_row_change(&mut self) {
        self.pending_row_change = None;
    }

    /// Take a held row change a confirmed Save or Discard approved.
    pub(crate) fn take_pending_row_change(&mut self) -> Option<PathBuf> {
        self.pending_row_change.take()
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
