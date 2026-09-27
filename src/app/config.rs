//! The Config screen's state: the selected row, how far the list is
//! scrolled, the view to return to, and the global exclusion editor.

use super::ViewMode;

/// A row in the flat configuration screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigRowKind {
    Header(&'static str),
    DiffToolAuto,
    DiffToolDisabled,
    DiffTool {
        idx: usize,
        available: bool,
    },
    DiffToolUnknown,
    /// Toggle for [`crate::settings::AppSettings::check_updates`].
    CheckUpdates,
    /// Toggle for [`crate::settings::AppSettings::mouse`].
    Mouse,
    /// Toggle for [`crate::settings::AppSettings::theme`].
    Theme,
    /// Numeric adjust for [`crate::settings::AppSettings::diff_context`] (`h`/`l` or
    /// `Left`/`Right`).
    DiffContext,
    /// Toggle for [`crate::settings::AppSettings::scan_mode`]. Applying it
    /// persists, updates the effective mode, and triggers one background rescan.
    ScanMode,
    /// Toggle for reading `.gitignore` files during scans.
    RespectGitignore,
    /// Opens the dedicated global exclusion list editor.
    GlobalExclusions,
    /// Read-only provenance for project and command-line rule sources.
    IgnoreSources,
    /// Read-only count of the `[keys]` entries in effect (Issue #339); keys
    /// are changed in the config file, not here.
    KeyBindings,
}

#[derive(Clone, Debug, Default)]
pub struct ExclusionEditorState {
    draft: Vec<String>,
    selected_idx: usize,
    scroll_offset: usize,
    input: crate::text_input::TextInput,
    editing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExclusionEditorAction {
    None,
    Apply,
    Cancel,
}

impl ExclusionEditorState {
    /// An editor whose draft starts from `draft`, the saved rules.
    pub(super) fn new(draft: Vec<String>) -> Self {
        Self {
            draft,
            ..Self::default()
        }
    }

    pub(crate) fn draft(&self) -> &[String] {
        &self.draft
    }

    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn input(&self) -> &crate::text_input::TextInput {
        &self.input
    }

    pub(crate) fn editing(&self) -> bool {
        self.editing
    }

    /// Highlight rule `index`, as when it fails to validate.
    pub(super) fn select(&mut self, index: usize) {
        self.selected_idx = index;
    }

    /// Keep the highlighted rule in a `visible_rows`-tall list viewport.
    pub(super) fn sync_viewport(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        let max_offset = self.draft.len().saturating_sub(visible_rows);
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_idx + 1 - visible_rows;
        }
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    pub(super) fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> ExclusionEditorAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.editing {
            match key.code {
                KeyCode::Enter => self.finish_edit(),
                KeyCode::Esc => self.editing = false,
                code => self.input.apply_edit(code),
            }
            return ExclusionEditorAction::None;
        }
        match key.code {
            KeyCode::Esc => return ExclusionEditorAction::Cancel,
            KeyCode::Char('a') => self.add(),
            KeyCode::Enter => self.begin_edit(),
            KeyCode::Char('d') => self.delete(),
            KeyCode::Char('r') => self.restore_defaults(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('J') => self.move_down(),
            KeyCode::Char('K') => self.move_up(),
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return ExclusionEditorAction::Apply;
            }
            _ => {}
        }
        ExclusionEditorAction::None
    }

    fn add(&mut self) {
        self.draft.push(String::new());
        self.selected_idx = self.draft.len() - 1;
        self.input.clear();
        self.editing = true;
    }

    fn begin_edit(&mut self) {
        if let Some(pattern) = self.draft.get(self.selected_idx) {
            self.input.set(pattern.clone());
            self.editing = true;
        }
    }

    fn finish_edit(&mut self) {
        if let Some(entry) = self.draft.get_mut(self.selected_idx) {
            *entry = self.input.to_string();
        }
        self.editing = false;
    }

    fn delete(&mut self) {
        if self.draft.is_empty() {
            return;
        }
        self.draft.remove(self.selected_idx);
        self.selected_idx = self.selected_idx.min(self.draft.len().saturating_sub(1));
    }

    fn restore_defaults(&mut self) {
        self.draft = crate::settings::AppSettings::default().global_exclusions;
        self.selected_idx = 0;
        self.editing = false;
        self.input.clear();
    }

    fn select_next(&mut self) {
        if !self.draft.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.draft.len();
        }
    }

    fn select_prev(&mut self) {
        if !self.draft.is_empty() {
            self.selected_idx = self
                .selected_idx
                .checked_sub(1)
                .unwrap_or(self.draft.len() - 1);
        }
    }

    fn move_down(&mut self) {
        if self.selected_idx + 1 < self.draft.len() {
            self.draft.swap(self.selected_idx, self.selected_idx + 1);
            self.selected_idx += 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected_idx > 0 {
            self.draft.swap(self.selected_idx, self.selected_idx - 1);
            self.selected_idx -= 1;
        }
    }
}

impl ConfigRowKind {
    pub fn is_selectable(self) -> bool {
        !matches!(
            self,
            ConfigRowKind::Header(_)
                | ConfigRowKind::IgnoreSources
                | ConfigRowKind::KeyBindings
                | ConfigRowKind::DiffToolUnknown
                | ConfigRowKind::DiffTool {
                    available: false,
                    ..
                }
        )
    }
}

/// The Config screen's own state: the selected row and the view to restore on
/// close. Owned by [`App::config`](super::App::config)/[`App::config_mut`](super::App::config_mut). Unlike [`HelpState`](super::HelpState)/
/// [`DirectoryTreeState`](super::DirectoryTreeState), most Config methods stay on `App` as orchestration:
/// [`App::config_rows`](super::App::config_rows) (the row list `ConfigState`'s selection indexes into)
/// reads `App::detected_diff_tools`, a concern `ConfigState` doesn't own, so
/// [`App::ensure_config_selection`](super::App::ensure_config_selection)/`config_select_next`/`config_select_prev`/
/// `config_select_at` build the row list on `App` and hand it to a
/// [`ConfigState`] method that does the pure index math — mirroring how
/// `App::open_help`/`close_help` stayed on `App` for [`HelpState`](super::HelpState).
#[derive(Clone, Copy, Debug)]
pub struct ConfigState {
    selected_idx: usize,
    /// How many of the list's painted lines are scrolled off the top.
    scroll: usize,
    return_view: ViewMode,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            selected_idx: 0,
            scroll: 0,
            return_view: ViewMode::DirectoryTree,
        }
    }
}

impl ConfigState {
    /// The currently selected config row index. Read access for rendering / tests.
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }

    /// Scroll as little as keeps `reveal` — the selected row's lines and the
    /// header above it — within `height` of the list's `total` lines. When
    /// `reveal` is taller than `height`, its end wins: the selected row.
    pub(crate) fn set_frame(
        &mut self,
        reveal: std::ops::Range<usize>,
        height: usize,
        total: usize,
    ) {
        if reveal.start < self.scroll {
            self.scroll = reveal.start;
        }
        if reveal.end > self.scroll + height {
            self.scroll = reveal.end - height;
        }
        self.scroll = self.scroll.min(total.saturating_sub(height));
    }

    /// The view to restore on [`App::close_config`](super::App::close_config).
    pub(crate) fn return_view(&self) -> ViewMode {
        self.return_view
    }

    /// Remember the view to restore on [`App::close_config`](super::App::close_config) (called from
    /// [`App::open_overlay`](super::App::open_overlay)).
    pub(crate) fn set_return_view(&mut self, view: ViewMode) {
        self.return_view = view;
    }

    /// Ensure `selected_idx` points at a selectable row in `rows`, falling
    /// back to the first selectable row (or 0 if none are). `rows` is
    /// [`App::config_rows`](super::App::config_rows)'s output — pure index math over data `App` computed.
    pub(crate) fn ensure_selection(&mut self, rows: &[ConfigRowKind]) {
        if rows.is_empty() {
            self.selected_idx = 0;
            return;
        }
        if self.selected_idx >= rows.len() || !rows[self.selected_idx].is_selectable() {
            self.selected_idx = rows.iter().position(|r| r.is_selectable()).unwrap_or(0);
        }
    }

    /// Wrap-around next selectable row in `rows`. See [`ConfigState::ensure_selection`].
    pub(crate) fn select_next(&mut self, rows: &[ConfigRowKind]) {
        if rows.is_empty() {
            return;
        }
        let mut next = self.selected_idx;
        for _ in 0..rows.len() {
            next = (next + 1) % rows.len();
            if rows[next].is_selectable() {
                self.selected_idx = next;
                return;
            }
        }
    }

    /// Wrap-around previous selectable row in `rows`. See [`ConfigState::ensure_selection`].
    pub(crate) fn select_prev(&mut self, rows: &[ConfigRowKind]) {
        if rows.is_empty() {
            return;
        }
        let mut prev = self.selected_idx;
        for _ in 0..rows.len() {
            prev = prev.checked_sub(1).unwrap_or(rows.len() - 1);
            if rows[prev].is_selectable() {
                self.selected_idx = prev;
                return;
            }
        }
    }

    /// Select row `idx` in `rows` if it exists and `is_selectable()`; otherwise
    /// no-op. Returns whether the selection was accepted. Used by mouse click.
    pub(crate) fn select_at(&mut self, idx: usize, rows: &[ConfigRowKind]) -> bool {
        if idx < rows.len() && rows[idx].is_selectable() {
            self.selected_idx = idx;
            true
        } else {
            false
        }
    }

    // Test-only field setter, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags this as unreachable
    // outside `#[cfg(test)]` call sites, so it needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }
}
