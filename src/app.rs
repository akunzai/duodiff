use crate::diff::{AlignedNode, DiffState, FileInfo};
use crate::ignore::IgnoreMatcher;
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct FlatRow {
    pub depth: usize,
    pub relative_path: PathBuf,
    pub name: String,
    pub state: DiffState,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
}

impl FlatRow {
    /// Whether either side of this row is a directory.
    pub(crate) fn is_dir(&self) -> bool {
        self.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || self.right.as_ref().map(|f| f.is_dir).unwrap_or(false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopic {
    DirectoryTree,
    FileDiff,
    Config,
    Mouse,
    General,
    About,
}

impl HelpTopic {
    /// All topics in index / quick-jump order (`1`-`6` map to these positions).
    pub fn all() -> [HelpTopic; 6] {
        use HelpTopic::*;
        [DirectoryTree, FileDiff, Config, Mouse, General, About]
    }

    /// Short title shown in the index list and the topic-view block title.
    pub fn title(self) -> &'static str {
        match self {
            HelpTopic::DirectoryTree => "Directory Tree",
            HelpTopic::FileDiff => "File Diff",
            HelpTopic::Config => "Config",
            HelpTopic::Mouse => "Mouse",
            HelpTopic::General => "General",
            HelpTopic::About => "About",
        }
    }

    /// The topic to open when `?` is pressed from a given `ViewMode`. `Mouse` and
    /// `General` have no view that maps to them directly; they're reached only via
    /// the index list or a direct number-key jump from within Help.
    pub fn for_view(view: ViewMode) -> HelpTopic {
        match view {
            ViewMode::DirectoryTree => HelpTopic::DirectoryTree,
            ViewMode::FileDiff => HelpTopic::FileDiff,
            ViewMode::ConfigMenu => HelpTopic::Config,
            ViewMode::Help => HelpTopic::General,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewMode {
    DirectoryTree,
    FileDiff,
    ConfigMenu,
    Help,
}

/// A row in the flat configuration screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigRowKind {
    Header(&'static str),
    DiffTool(usize),
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
}

impl ConfigRowKind {
    pub fn is_selectable(self) -> bool {
        !matches!(self, ConfigRowKind::Header(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    CopyLeftToRight,
    CopyRightToLeft,
}

/// A pending confirmation prompt: the message to show and the action to run if accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmModal {
    pub message: String,
    pub action: ConfirmAction,
}

/// The one Command Palette. `;`, `Ctrl+p`, and right-click all open this same
/// contextual surface — the former Menu / Command split is gone, along with its
/// single-character immediate execution and the `c`/`C` accelerator ambiguity
/// (Issue #239).
#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    pub visible: bool,
    pub query: String,
    pub items: Vec<crate::ui::PaletteAction>,
    pub selected_idx: usize,
    /// First item row painted in the list viewport, so a selection past the
    /// bottom of a long inventory stays visible.
    pub scroll_offset: usize,
}

/// The Help screen's own state: active topic, the topic index overlay, and the
/// view to restore on close. Owned by [`App::help`]/[`App::help_mut`]; production
/// code reaches it only through [`App::open_help`]/[`App::close_help`] (which also
/// touch `view_mode`, a nav concern that stays on `App`) plus the methods here.
#[derive(Clone, Copy, Debug)]
pub struct HelpState {
    topic: HelpTopic,
    return_view: ViewMode,
    index_open: bool,
    index_sel: usize,
    scroll: u16,
}

impl Default for HelpState {
    fn default() -> Self {
        Self {
            topic: HelpTopic::General,
            return_view: ViewMode::DirectoryTree,
            index_open: false,
            index_sel: 0,
            scroll: 0,
        }
    }
}

impl HelpState {
    /// Remember the view to restore on [`App::close_help`] (called from
    /// [`App::open_overlay`] before the topic/index setup in `enter`).
    pub(crate) fn set_return_view(&mut self, view: ViewMode) {
        self.return_view = view;
    }

    /// Enter Help on `topic`: sync the index cursor to it, close the index, and
    /// reset scroll. Called by [`App::open_help`] with the contextual topic for
    /// the just-recorded `return_view`.
    fn enter(&mut self, topic: HelpTopic) {
        self.topic = topic;
        self.index_sel = HelpTopic::all()
            .iter()
            .position(|&t| t == topic)
            .unwrap_or(0);
        self.index_open = false;
        self.scroll = 0;
    }

    /// Leave Help: force `index_open = false`. Unifies the body-Esc and
    /// index-Esc paths (body already has the index closed; setting it again is a
    /// no-op UX-wise). `view_mode` restore stays on [`App::close_help`].
    fn leave(&mut self) {
        self.index_open = false;
    }

    /// Set the active topic body, close the index if open, and reset scroll to 0.
    /// Shared by Enter (index), digit keys (index or body), and mouse topic click.
    pub(crate) fn select_topic(&mut self, topic: HelpTopic) {
        self.topic = topic;
        self.index_open = false;
        self.scroll = 0;
    }

    /// Select the topic at `idx` in `HelpTopic::all()`, if in range. Shared by the
    /// digit-key shortcut, Enter-on-index, and mouse click-on-index-row — the one
    /// deep entry point for resolving a raw index to a topic.
    pub(crate) fn select_topic_by_index(&mut self, idx: usize) -> bool {
        match HelpTopic::all().get(idx) {
            Some(&topic) => {
                self.select_topic(topic);
                true
            }
            None => false,
        }
    }

    /// Open the topic index, syncing `index_sel` to the current topic (Tab).
    pub(crate) fn open_index(&mut self) {
        self.index_sel = HelpTopic::all()
            .iter()
            .position(|&t| t == self.topic)
            .unwrap_or(0);
        self.index_open = true;
    }

    /// Close the topic index only (`index_open = false`), stay on Help.
    /// Symmetric with `open_index`; production Esc uses [`App::close_help`] instead.
    /// Currently exercised only by tests — no key/mouse path closes just the index today.
    #[allow(dead_code)]
    pub(crate) fn close_index(&mut self) {
        self.index_open = false;
    }

    /// Wrap-around next over `HelpTopic::all()` for the index cursor.
    /// Shared by keyboard j/k (index mode) and mouse scroll (index mode).
    pub(crate) fn index_select_next(&mut self) {
        self.index_sel = (self.index_sel + 1) % HelpTopic::all().len();
    }

    /// Wrap-around prev over `HelpTopic::all()` for the index cursor.
    /// Shared by keyboard j/k (index mode) and mouse scroll (index mode).
    pub(crate) fn index_select_prev(&mut self) {
        self.index_sel = self
            .index_sel
            .checked_sub(1)
            .unwrap_or(HelpTopic::all().len() - 1);
    }

    /// Scroll the topic body down by one row. Shared by keyboard j/k (body mode)
    /// and mouse scroll (body mode).
    pub(crate) fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    /// Scroll the topic body up by one row (saturating). Shared by keyboard j/k
    /// (body mode) and mouse scroll (body mode).
    pub(crate) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Move down: index-select-next if the topic index is open, else scroll the
    /// body down. Owns the mode branch so callers (keyboard j/k·Down, mouse
    /// scroll) don't have to re-derive it.
    pub(crate) fn move_down(&mut self) {
        if self.index_open {
            self.index_select_next();
        } else {
            self.scroll_down();
        }
    }

    /// Move up: index-select-prev if the topic index is open, else scroll the
    /// body up. Symmetric with [`HelpState::move_down`].
    pub(crate) fn move_up(&mut self) {
        if self.index_open {
            self.index_select_prev();
        } else {
            self.scroll_up();
        }
    }

    /// The active Help topic body. Read access for `draw_help` / tests.
    pub(crate) fn topic(&self) -> HelpTopic {
        self.topic
    }

    /// The view to restore on [`App::close_help`].
    pub(crate) fn return_view(&self) -> ViewMode {
        self.return_view
    }

    /// Whether the topic index (vs. the topic body) is showing. Read access for
    /// `draw_help` / tests.
    pub(crate) fn index_open(&self) -> bool {
        self.index_open
    }

    /// The index cursor's current position. Read access for `draw_help` / tests.
    pub(crate) fn index_sel(&self) -> usize {
        self.index_sel
    }

    /// The topic body's vertical scroll offset. Read access for `draw_help` / tests.
    pub(crate) fn scroll(&self) -> u16 {
        self.scroll
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_topic(&mut self, topic: HelpTopic) {
        self.topic = topic;
    }

    #[allow(dead_code)]
    pub(crate) fn set_index_open(&mut self, open: bool) {
        self.index_open = open;
    }

    #[allow(dead_code)]
    pub(crate) fn set_index_sel(&mut self, idx: usize) {
        self.index_sel = idx;
    }

    #[allow(dead_code)]
    pub(crate) fn set_scroll(&mut self, scroll: u16) {
        self.scroll = scroll;
    }
}

/// The filter bar's own state: input-bar text, the committed pattern/diffs-only
/// flag, and the filtered row list they produce. Owned by [`App::filter`]/
/// [`App::filter_mut`]. [`App::apply_filter`] (and the `commit_filter`/
/// `clear_filter` callers that end in it) stays on `App` — recomputing rows
/// also restores `App`'s `selected_idx`/`scroll_offset` (a nav concern) from
/// `App`'s `flat_rows` (a scan concern), neither of which `FilterState` owns.
#[derive(Clone, Debug, Default)]
pub struct FilterState {
    active: bool,
    input: crate::text_input::TextInput,
    pattern: String,
    /// Committed diffs-only flag — the one [`FilterState::recompute`] applies.
    diffs_only: bool,
    /// The editing session's diffs-only value. Mirrors the typed text: it only
    /// updates the badge until Enter commits both together, and Esc restores it
    /// alongside the query (Issue #236).
    draft_diffs_only: bool,
    rows: Vec<FlatRow>,
}

impl FilterState {
    /// True while the filter input bar is open and routing key events.
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// The committed filter pattern (set on Enter/Esc), lowercase-matched
    /// against row names/paths in [`FilterState::recompute`].
    pub(crate) fn pattern(&self) -> &str {
        &self.pattern
    }

    /// True when the row list should exclude [`DiffState::Identical`] rows.
    pub(crate) fn diffs_only(&self) -> bool {
        self.diffs_only
    }

    /// The diffs-only value to show in the filter bar's badge: the editing
    /// session's draft while the bar is open, the committed flag otherwise.
    pub(crate) fn editing_diffs_only(&self) -> bool {
        if self.active {
            self.draft_diffs_only
        } else {
            self.diffs_only
        }
    }

    /// Flip the editing session's diffs-only flag. Like typed pattern text, this
    /// only reaches `rows` once the filter bar is committed via
    /// [`App::commit_filter`].
    pub(crate) fn toggle_diffs_only(&mut self) {
        self.draft_diffs_only = !self.draft_diffs_only;
    }

    /// The filter input bar's text, for rendering.
    pub(crate) fn input(&self) -> &crate::text_input::TextInput {
        &self.input
    }

    /// Mutable access to the filter input bar's text, for key-by-key editing.
    pub(crate) fn input_mut(&mut self) -> &mut crate::text_input::TextInput {
        &mut self.input
    }

    /// The filtered tree rows currently shown in the directory tree. Read
    /// access for the tree render loop's full-list iteration.
    pub(crate) fn rows(&self) -> &[FlatRow] {
        &self.rows
    }

    /// Open the filter input bar, pre-filling with the committed pattern and
    /// diffs-only flag so Esc can restore both.
    pub(crate) fn open(&mut self) {
        self.active = true;
        self.input.set(self.pattern.clone());
        self.draft_diffs_only = self.diffs_only;
    }

    /// Close the filter input bar, committing the typed text and the drafted
    /// diffs-only flag together. Does not recompute `rows` itself —
    /// [`App::commit_filter`] follows up with [`App::apply_filter`], which also
    /// restores selection/scroll.
    pub(crate) fn commit(&mut self) {
        self.active = false;
        self.pattern = self.input.to_string();
        self.diffs_only = self.draft_diffs_only;
    }

    /// Close the filter input bar, discarding any uncommitted typing and any
    /// diffs-only toggle made during the editing session.
    pub(crate) fn cancel(&mut self) {
        self.active = false;
        self.input.set(self.pattern.clone());
        self.draft_diffs_only = self.diffs_only;
    }

    /// Clear the filter entirely (pattern + diffs-only). Does not recompute
    /// `rows` itself — [`App::clear_filter`] follows up with [`App::apply_filter`].
    pub(crate) fn clear(&mut self) {
        self.pattern.clear();
        self.input.clear();
        self.diffs_only = false;
        self.draft_diffs_only = false;
    }

    /// Rebuild `rows` from `source` (`App`'s `flat_rows`) using the current
    /// pattern and diffs-only flag. Pure recompute — leaves selection/scroll
    /// restoration to [`App::apply_filter`], the only caller.
    pub(crate) fn recompute(&mut self, source: &[FlatRow]) {
        let pattern = self.pattern.to_lowercase();
        let diffs_only = self.diffs_only;

        if pattern.is_empty() && !diffs_only {
            self.rows = source.to_vec();
        } else {
            self.rows = source
                .iter()
                .filter(|row| {
                    if diffs_only && row.state == DiffState::Identical {
                        return false;
                    }
                    if pattern.is_empty() {
                        return true;
                    }
                    row.name.to_lowercase().contains(&pattern)
                        || row
                            .relative_path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&pattern)
                })
                .cloned()
                .collect();
        }
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_rows(&mut self, rows: Vec<FlatRow>) {
        self.rows = rows;
    }

    #[allow(dead_code)]
    pub(crate) fn set_pattern(&mut self, pattern: impl Into<String>) {
        self.pattern = pattern.into();
    }
}

/// The Config screen's own state: the selected row and the view to restore on
/// close. Owned by [`App::config`]/[`App::config_mut`]. Unlike [`HelpState`]/
/// [`FilterState`], most Config methods stay on `App` as orchestration:
/// [`App::config_rows`] (the row list `ConfigState`'s selection indexes into)
/// reads `App::detected_diff_tools`, a concern `ConfigState` doesn't own, so
/// [`App::ensure_config_selection`]/`config_select_next`/`config_select_prev`/
/// `config_select_at` build the row list on `App` and hand it to a
/// [`ConfigState`] method that does the pure index math — mirroring how
/// `App::apply_filter` stayed on `App` for [`FilterState`] and
/// `App::open_help`/`close_help` stayed on `App` for [`HelpState`].
#[derive(Clone, Copy, Debug)]
pub struct ConfigState {
    selected_idx: usize,
    return_view: ViewMode,
}

impl Default for ConfigState {
    fn default() -> Self {
        Self {
            selected_idx: 0,
            return_view: ViewMode::DirectoryTree,
        }
    }
}

impl ConfigState {
    /// The currently selected config row index. Read access for rendering / tests.
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    /// The view to restore on [`App::close_config`].
    pub(crate) fn return_view(&self) -> ViewMode {
        self.return_view
    }

    /// Remember the view to restore on [`App::close_config`] (called from
    /// [`App::open_overlay`]).
    pub(crate) fn set_return_view(&mut self, view: ViewMode) {
        self.return_view = view;
    }

    /// Ensure `selected_idx` points at a selectable row in `rows`, falling
    /// back to the first selectable row (or 0 if none are). `rows` is
    /// [`App::config_rows`]'s output — pure index math over data `App` computed.
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

/// The file-diff content state: the built-in diff's rows, both scroll
/// offsets, the wrap/full-file toggles, and the cached hashes/line-endings
/// shown above the diff panes. Owned by [`App::diff`]/[`App::diff_mut`].
///
/// Named `FileDiffState` rather than `DiffState` — [`crate::diff::DiffState`]
/// (the per-row Identical/LeftOnly/... status) already owns that name.
///
/// Most of `FileDiffState`'s own methods take a `max`/`width` parameter
/// instead of reading it themselves: that geometry lives in `App::viewport`,
/// computed from both the tree pane and the diff pane, so it isn't a
/// `FileDiffState` concern (see [`Viewport`]). Methods that also need
/// `App`-only data — [`App::refresh_file_diff`] (selected row +
/// `settings.diff_context`), [`App::enter_file_diff`]/`copy_hunk_at_cursor`
/// (orchestrate view_mode / file I/O around it),
/// [`App::resync_diff_geometry`]/`clamp_diff_scroll` (write `App::viewport`
/// back) — stay on `App` as orchestration, mirroring the precedent set by
/// the three prior slices (#186, #187, #188).
#[derive(Clone, Debug, Default)]
pub struct FileDiffState {
    rows: Vec<crate::diff_view::DiffRow>,
    scroll: usize,
    h_scroll: usize,
    wrap: bool,
    show_full: bool,
    left_hash: Option<String>,
    right_hash: Option<String>,
    left_line_ending: Option<String>,
    right_line_ending: Option<String>,
}

impl FileDiffState {
    /// The current file diff's rows. Read access for rendering.
    pub(crate) fn rows(&self) -> &[crate::diff_view::DiffRow] {
        &self.rows
    }

    /// True when the current file diff has at least one added/removed line.
    pub(crate) fn has_changes(&self) -> bool {
        self.rows.iter().any(crate::diff_view::diff_row_is_change)
    }

    /// The file-diff view's vertical scroll offset.
    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }

    /// The file-diff view's horizontal scroll offset (used when wrap is off).
    pub(crate) fn h_scroll(&self) -> usize {
        self.h_scroll
    }

    /// Whether long lines wrap in the file-diff view.
    pub(crate) fn wrap(&self) -> bool {
        self.wrap
    }

    /// Whether the file-diff view shows the full file rather than collapsed hunks.
    pub(crate) fn show_full(&self) -> bool {
        self.show_full
    }

    /// SHA-256 hash of the left side's file, if it loaded successfully.
    pub(crate) fn left_hash(&self) -> Option<&str> {
        self.left_hash.as_deref()
    }

    /// SHA-256 hash of the right side's file, if it loaded successfully.
    pub(crate) fn right_hash(&self) -> Option<&str> {
        self.right_hash.as_deref()
    }

    /// Detected line-ending style of the left side's file, if any.
    pub(crate) fn left_line_ending(&self) -> Option<&str> {
        self.left_line_ending.as_deref()
    }

    /// Detected line-ending style of the right side's file, if any.
    pub(crate) fn right_line_ending(&self) -> Option<&str> {
        self.right_line_ending.as_deref()
    }

    /// Recompute `rows`/hashes/line-endings for `left_file`/`right_file`,
    /// using `show_full` and `diff_context` (an `App::settings` concern,
    /// passed in) for the compare call. Leaves `self` untouched on error, so
    /// [`App::toggle_diff_show_full`]'s rollback stays a plain field flip.
    pub(crate) fn load(
        &mut self,
        left_file: &Path,
        right_file: &Path,
        diff_context: usize,
    ) -> Result<(), String> {
        self.rows =
            crate::diff_view::compare_files(left_file, right_file, self.show_full, diff_context)
                .map_err(|e| e.to_string())?;
        self.left_hash = crate::diff::compute_file_sha256(left_file).ok();
        self.right_hash = crate::diff::compute_file_sha256(right_file).ok();
        self.left_line_ending = crate::diff_view::detect_file_line_ending(left_file);
        self.right_line_ending = crate::diff_view::detect_file_line_ending(right_file);
        Ok(())
    }

    /// Flip line wrapping and reset scroll, since the old scroll position no
    /// longer lines up once wrapping changes the layout.
    pub(crate) fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.reset_scroll();
    }

    /// Flip full-file vs. diff-only content. Pure flag flip — reloading the
    /// diff and resetting scroll are [`App::toggle_diff_show_full`]'s job.
    pub(crate) fn toggle_show_full(&mut self) {
        self.show_full = !self.show_full;
    }

    /// Set the full-file flag directly (vs. [`FileDiffState::toggle_show_full`]'s
    /// flip). Used by [`App::enter_file_diff`] to force diff-only mode before
    /// the first load, and by tests to seed a specific state.
    pub(crate) fn set_show_full(&mut self, on: bool) {
        self.show_full = on;
    }

    /// Line-step down, clamped to `max` (`viewport.max_diff_scroll()`).
    /// Shared by keyboard j/Down and mouse scroll down.
    pub(crate) fn scroll_down(&mut self, max: usize) {
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    /// Line-step up (no-op at the top). Shared by keyboard k/Up and mouse scroll up.
    pub(crate) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Page down by `step`, clamped to `max` (`viewport.max_diff_scroll()`).
    pub(crate) fn page_down(&mut self, step: usize, max: usize) {
        self.scroll = (self.scroll + step).min(max);
    }

    /// Page up by `step` (no-op past the top).
    pub(crate) fn page_up(&mut self, step: usize) {
        self.scroll = self.scroll.saturating_sub(step);
    }

    /// Horizontal step left, when wrap is off (no-op while wrapping or at the
    /// left edge).
    pub(crate) fn h_scroll_left(&mut self) {
        if !self.wrap && self.h_scroll > 0 {
            self.h_scroll -= 1;
        }
    }

    /// Horizontal step right, when wrap is off, clamped to `max`
    /// (`viewport.max_diff_h_scroll()`).
    pub(crate) fn h_scroll_right(&mut self, max: usize) {
        if !self.wrap && self.h_scroll < max {
            self.h_scroll += 1;
        }
    }

    /// Zero both scroll offsets. Used after wrap/full toggles and on entering
    /// a fresh file diff, where the old scroll position no longer applies.
    pub(crate) fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.h_scroll = 0;
    }

    /// Pull both scroll offsets back inside `max_scroll`/`max_h_scroll`
    /// (`viewport.max_diff_scroll()`/`max_diff_h_scroll()`). Growing the
    /// terminal (or opening a shorter file) can leave them past the end of
    /// the content; without this the next page or arrow key would appear to
    /// jump backwards.
    pub(crate) fn clamp_scroll(&mut self, max_scroll: usize, max_h_scroll: usize) {
        self.scroll = self.scroll.min(max_scroll);
        self.h_scroll = self.h_scroll.min(max_h_scroll);
    }

    /// Reset scroll and clear cached hashes after [`App::swap_paths`] — rows
    /// and line-endings are left for the next `refresh_file_diff` to replace.
    pub(crate) fn reset_for_swap(&mut self) {
        self.scroll = 0;
        self.left_hash = None;
        self.right_hash = None;
    }

    /// Jump to the next (`forward`) or previous differing block, given the
    /// diff pane's content `width` (`viewport.diff_content_width`).
    pub(crate) fn jump_to_change(&mut self, width: usize, forward: bool) {
        if let Some(scroll) = crate::diff_view::jump_to_change_scroll(
            &self.rows,
            self.scroll,
            width,
            self.wrap,
            forward,
        ) {
            self.scroll = scroll;
        }
    }

    /// Set the vertical scroll offset directly. Used by
    /// [`App::copy_hunk_at_cursor`] to restore a clamped scroll position
    /// after a hunk copy reloads the diff, and by tests to seed a position.
    pub(crate) fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_rows(&mut self, rows: Vec<crate::diff_view::DiffRow>) {
        self.rows = rows;
    }

    #[allow(dead_code)]
    pub(crate) fn set_h_scroll(&mut self, scroll: usize) {
        self.h_scroll = scroll;
    }

    #[allow(dead_code)]
    pub(crate) fn set_wrap(&mut self, on: bool) {
        self.wrap = on;
    }

    #[allow(dead_code)]
    pub(crate) fn set_hashes(&mut self, left: Option<String>, right: Option<String>) {
        self.left_hash = left;
        self.right_hash = right;
    }
}

/// Terminal-derived geometry for the frame currently being handled.
///
/// **Ordering contract:** these values are only meaningful after
/// [`App::sync_viewport`] has run for the current frame. The event loop calls it
/// once per iteration *before* drawing and before any key/mouse handling, so the
/// render pass and the input handlers always agree on the same geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Viewport {
    /// Content rows visible in the active list/diff pane (borders excluded).
    pub visible_height: usize,
    /// Content columns available inside one diff pane (borders excluded).
    pub diff_content_width: usize,
    /// Longest line (in characters) across the current [`FileDiffState::rows`].
    pub diff_max_line_width: usize,
    /// Physical (post-wrap) row count of the current [`FileDiffState::rows`].
    pub diff_physical_rows: usize,
}

impl Viewport {
    /// Largest vertical scroll offset that still fills the diff panes.
    pub fn max_diff_scroll(self) -> usize {
        self.diff_physical_rows.saturating_sub(self.visible_height)
    }

    /// Largest horizontal scroll offset that keeps the longest line reachable.
    pub fn max_diff_h_scroll(self) -> usize {
        self.diff_max_line_width
            .saturating_sub(self.diff_content_width)
    }
}

pub struct App {
    left_path: PathBuf,
    right_path: PathBuf,
    /// Effective scan mode for this session. Seeded once at bootstrap from the
    /// persisted setting or `--scan-mode` (via [`App::set_scan_mode`], which
    /// deliberately does not persist); changed thereafter only through
    /// [`App::apply_scan_mode`], which persists first (Issue #238).
    scan_mode: crate::settings::ScanMode,
    root_node: Option<AlignedNode>,
    scan_in_progress: bool,
    /// Monotonic counter bumped for every scan start. Stale `ScanFinished` /
    /// scan `Error` events with an older generation are ignored.
    scan_generation: u64,
    flat_rows: Vec<FlatRow>,
    selected_idx: usize,
    scroll_offset: usize,
    active_side_left: bool,
    view_mode: ViewMode,
    diff: FileDiffState,
    /// Terminal geometry for the current frame; see [`App::sync_viewport`].
    viewport: Viewport,
    last_click_idx: Option<usize>,
    last_click_time: Option<std::time::Instant>,
    settings: crate::settings::AppSettings,
    detected_diff_tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    config: ConfigState,
    palette: PaletteState,
    confirm_modal: Option<ConfirmModal>,
    /// Transient status toast: (message, is_error, created_at)
    status_message: Option<(String, bool, Instant)>,
    filter: FilterState,
    /// Glob-based ignore matcher used during directory scans.
    ignore_matcher: IgnoreMatcher,
    update_check_enabled: bool,
    /// Effective mouse-capture state for this session: `settings.mouse` unless overridden
    /// by the `--no-mouse` CLI flag. See [`crate::settings::resolve_mouse_enabled`].
    mouse_enabled: bool,
    update_available: Option<String>,
    install_method: crate::upgrade::InstallMethod,
    help: HelpState,
    should_quit: bool,
}

impl App {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        Self::new_with_ignore(left, right, IgnoreMatcher::default())
    }

    pub fn new_with_ignore(left: PathBuf, right: PathBuf, ignore_matcher: IgnoreMatcher) -> Self {
        let mut settings = crate::settings::AppSettings::load();
        let detected_diff_tools = crate::diff_tool::detect_diff_tools();
        // Session default only — do not write config.toml until the user
        // explicitly selects a tool (or other setting) in the Config screen.
        if settings.external_diff_tool.is_none() {
            if let Some((tool, _)) = detected_diff_tools.iter().find(|(_, avail)| *avail) {
                settings.external_diff_tool = Some(tool.as_str().to_string());
            }
        }

        let install_method = if let Ok(exe_path) = std::env::current_exe() {
            crate::upgrade::detect_install_method(&exe_path)
        } else {
            crate::upgrade::InstallMethod::Standalone
        };

        Self {
            left_path: left,
            right_path: right,
            scan_mode: settings.scan_mode,
            root_node: None,
            scan_in_progress: false,
            scan_generation: 0,
            flat_rows: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            active_side_left: true,
            view_mode: ViewMode::DirectoryTree,
            diff: FileDiffState::default(),
            viewport: Viewport::default(),
            last_click_idx: None,
            last_click_time: None,
            settings,
            detected_diff_tools,
            config: ConfigState::default(),
            palette: PaletteState::default(),
            confirm_modal: None,
            status_message: None,
            filter: FilterState::default(),
            ignore_matcher,
            update_check_enabled: true,
            mouse_enabled: true,
            update_available: None,
            install_method,
            help: HelpState::default(),
            should_quit: false,
        }
    }

    /// Mark a new background scan as in-flight and return its generation id.
    pub fn begin_scan(&mut self) -> u64 {
        self.scan_generation = self.scan_generation.wrapping_add(1);
        self.scan_in_progress = true;
        self.scan_generation
    }

    /// True while a background scan is still running.
    pub fn scan_in_progress(&self) -> bool {
        self.scan_in_progress
    }

    /// Apply a finished background scan.
    ///
    /// Owns the whole scan-result invariant — tree, restored expand state,
    /// in-flight flag and the flattened row cache all move together, so a caller
    /// cannot update one without the others. Results from a superseded
    /// [`App::begin_scan`] generation are dropped; returns `false` in that case
    /// and leaves the app untouched.
    pub fn apply_scan_result(&mut self, generation: u64, node: AlignedNode) -> bool {
        if generation != self.scan_generation {
            return false;
        }
        let expanded_paths = self.collect_expanded_paths();
        self.root_node = Some(node);
        self.restore_expanded_paths(&expanded_paths);
        self.scan_in_progress = false;
        self.flatten_tree();
        true
    }

    /// Mark a failed background scan as finished, keeping the previous tree.
    ///
    /// Returns `false` (and changes nothing) for a superseded generation, so the
    /// caller can skip its error toast too.
    pub fn fail_scan(&mut self, generation: u64) -> bool {
        if generation != self.scan_generation {
            return false;
        }
        self.scan_in_progress = false;
        true
    }

    /// Apply a finished background update check.
    ///
    /// Owns the match on [`crate::upgrade::UpdateCheckOutcome`], the
    /// throttle-state write, and `update_available`, so the event loop only
    /// dispatches. A failed check stays silent and does not touch throttle state
    /// (so the next launch can retry immediately).
    pub fn apply_update_check_outcome(&mut self, outcome: crate::upgrade::UpdateCheckOutcome) {
        let now = crate::upgrade::now_secs();
        match outcome {
            crate::upgrade::UpdateCheckOutcome::Newer(version) => {
                if let Ok(path) = crate::upgrade::state_path() {
                    crate::upgrade::save_state(
                        &path,
                        &crate::upgrade::UpdateCheckState {
                            last_check: now,
                            latest_seen: version.clone(),
                        },
                    );
                }
                self.update_available = Some(version);
            }
            crate::upgrade::UpdateCheckOutcome::UpToDate => {
                if let Ok(path) = crate::upgrade::state_path() {
                    crate::upgrade::save_state(
                        &path,
                        &crate::upgrade::UpdateCheckState {
                            last_check: now,
                            latest_seen: String::new(),
                        },
                    );
                }
                self.update_available = None;
            }
            crate::upgrade::UpdateCheckOutcome::Failed => {}
        }
    }

    /// Geometry for the frame currently being handled.
    ///
    /// Only valid after [`App::sync_viewport`] has run for this frame — see
    /// [`Viewport`] for the ordering contract.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Recompute every terminal-derived measurement for the drawable `area`.
    ///
    /// This is the single place viewport geometry is produced. Call it once per
    /// frame from the event loop **before** drawing and before handling keys or
    /// mouse events; rendering is then a pure read of [`App::viewport`], and
    /// scroll clamping can never act on geometry from a previous terminal size or
    /// a previously opened file.
    pub fn sync_viewport(&mut self, area: Rect) {
        match self.view_mode {
            ViewMode::DirectoryTree => {
                let inputs = self.tree_layout_inputs();
                let layout = crate::ui::tree_layout(&inputs, area);
                self.viewport.visible_height = layout.left.height.saturating_sub(2) as usize;
                self.adjust_scroll(self.viewport.visible_height);
            }
            ViewMode::FileDiff => {
                let inputs = self.diff_layout_inputs();
                let layout = crate::ui::diff_layout(&inputs, area);
                self.viewport.visible_height = layout.left.height.saturating_sub(2) as usize;
                self.viewport.diff_content_width = layout.left.width.saturating_sub(2) as usize;
                self.resync_diff_geometry();
                self.clamp_diff_scroll();
            }
            // Help and Config scroll by their own drawn line counts, not the
            // shared list/diff geometry, so nothing to sync here.
            ViewMode::ConfigMenu | ViewMode::Help => {}
        }
    }

    /// Set a transient status message displayed in the footer.
    /// `is_error` = true → red styling, false → green styling.
    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_message = Some((msg.into(), is_error, Instant::now()));
    }

    /// Active footer toast, if any: `(message, is_error)`.
    ///
    /// Layout uses [`.is_some()`](Option::is_some) for footer height; render and
    /// tests use the payload. Does not expose the private expiry timestamp.
    pub(crate) fn status_toast(&self) -> Option<(&str, bool)> {
        self.status_message
            .as_ref()
            .map(|(msg, is_error, _)| (msg.as_str(), *is_error))
    }

    /// Ask the event loop to exit after the current frame.
    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    /// Whether the event loop should break on the next iteration.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The effective scan mode for this session.
    pub fn scan_mode(&self) -> crate::settings::ScanMode {
        self.scan_mode
    }

    /// The persisted scan mode. Differs from [`App::scan_mode`] only while a
    /// `--scan-mode` CLI value is overriding it for this session.
    pub fn saved_scan_mode(&self) -> crate::settings::ScanMode {
        self.settings.scan_mode
    }

    /// Whether the Config screen should annotate the scan-mode row as a session
    /// override: true exactly while the effective mode and the saved default
    /// disagree, which only a `--scan-mode` CLI value can cause. Any in-app
    /// change persists and therefore clears it (Issue #238).
    pub fn scan_mode_is_session_override(&self) -> bool {
        self.scan_mode != self.settings.scan_mode
    }

    /// Seed the session's effective scan mode without persisting it. Used once
    /// at bootstrap (`main`) to apply the `--scan-mode` CLI value, which must not
    /// write the config file.
    pub(crate) fn set_scan_mode(&mut self, mode: crate::settings::ScanMode) {
        self.scan_mode = mode;
    }

    /// Persist `mode`, then adopt it as the effective scan mode.
    ///
    /// Persist-first is deliberate: on a save failure the previous runtime mode
    /// is kept and the caller must not rescan, so the screen never shows results
    /// from a mode the user's config does not agree with (Issue #238).
    pub fn apply_scan_mode(
        &mut self,
        mode: crate::settings::ScanMode,
    ) -> Result<(), std::io::Error> {
        let previous = self.settings.scan_mode;
        self.settings.scan_mode = mode;
        if let Err(e) = self.settings.save() {
            self.settings.scan_mode = previous;
            return Err(e);
        }
        self.scan_mode = mode;
        Ok(())
    }

    /// The one scan-mode switch behind the Directory Tree `c` key, the Palette,
    /// and the Config screen: persist, adopt, report the outcome as a toast, and
    /// tell the caller whether to start the single background rescan.
    ///
    /// The rescan itself stays with the caller, which owns the event sender
    /// (Issue #238).
    #[must_use]
    pub fn switch_scan_mode(&mut self, mode: crate::settings::ScanMode) -> bool {
        match self.apply_scan_mode(mode) {
            Ok(()) => {
                self.set_status(format!("Scan mode: {}", mode.label()), false);
                true
            }
            Err(e) => {
                self.set_status(format!("Could not save scan mode: {e}"), true);
                false
            }
        }
    }

    /// Whether directory scans compare file content hashes, not only mtime/size.
    pub fn precise_mode(&self) -> bool {
        self.scan_mode.is_precise()
    }

    /// Glob-based ignore matcher used during directory scans. Set once at construction
    /// (see [`App::new_with_ignore`]); read access for rescans (`kick_scan`).
    pub fn ignore_matcher(&self) -> &IgnoreMatcher {
        &self.ignore_matcher
    }

    /// Effective mouse-capture state for this session: `settings.mouse` unless overridden
    /// by the `--no-mouse` CLI flag. See [`crate::settings::resolve_mouse_enabled`].
    pub fn mouse_enabled(&self) -> bool {
        self.mouse_enabled
    }

    /// Set the effective mouse-capture state. Used once at bootstrap (`main`), before
    /// the event loop starts; `apply_config_selection` flips it in lockstep with the
    /// persisted `settings.mouse` toggle thereafter.
    pub(crate) fn set_mouse_enabled(&mut self, enabled: bool) {
        self.mouse_enabled = enabled;
    }

    /// Whether the background update check is enabled for this session
    /// (`settings.check_updates`, unless the `--no-update-check` CLI flag disabled it).
    pub fn update_check_enabled(&self) -> bool {
        self.update_check_enabled
    }

    /// Set whether the background update check is enabled. Used once at bootstrap
    /// (`main`), before the event loop starts.
    pub(crate) fn set_update_check_enabled(&mut self, enabled: bool) {
        self.update_check_enabled = enabled;
    }

    /// Newer version string, if a completed update check found one.
    pub fn update_available(&self) -> Option<&str> {
        self.update_available.as_deref()
    }

    /// Set the newer-version hint from a completed update check. Used once at
    /// bootstrap (`main`) for the cached last-seen version; live check outcomes go
    /// through [`App::apply_update_check_outcome`] instead.
    pub(crate) fn set_update_available(&mut self, version: Option<String>) {
        self.update_available = version;
    }

    /// How this binary was installed (Homebrew, Scoop, standalone, ...), used to
    /// tailor the update hint's suggested command.
    pub fn install_method(&self) -> &crate::upgrade::InstallMethod {
        &self.install_method
    }

    /// Build the flat configuration row list (headers + fields).
    pub fn config_rows(&self) -> Vec<ConfigRowKind> {
        let mut rows = vec![ConfigRowKind::Header("External Diff Tool")];
        rows.extend(
            self.detected_diff_tools
                .iter()
                .enumerate()
                .map(|(i, _)| ConfigRowKind::DiffTool(i)),
        );
        rows.push(ConfigRowKind::Header("Updates"));
        rows.push(ConfigRowKind::CheckUpdates);
        rows.push(ConfigRowKind::Header("Mouse"));
        rows.push(ConfigRowKind::Mouse);
        rows.push(ConfigRowKind::Header("Theme"));
        rows.push(ConfigRowKind::Theme);
        rows.push(ConfigRowKind::Header("Diff View"));
        rows.push(ConfigRowKind::DiffContext);
        rows.push(ConfigRowKind::Header("Scan"));
        rows.push(ConfigRowKind::ScanMode);
        rows
    }

    /// Read access to the persisted settings blob. Mutations go through App methods
    /// (`toggle_theme`, `apply_config_selection`, `adjust_config_selection`) that also
    /// persist via [`crate::settings::AppSettings::save`].
    pub fn settings(&self) -> &crate::settings::AppSettings {
        &self.settings
    }

    /// Resolved colour palette for the current [`crate::settings::AppSettings::theme`].
    pub fn theme(&self) -> crate::theme::Theme {
        crate::theme::Theme::for_choice(self.settings.theme)
    }

    /// Flip between the dark and light theme and persist the choice.
    pub fn toggle_theme(&mut self) {
        self.settings.theme = self.settings.theme.toggled();
        let _ = self.settings.save();
        self.set_status(format!("Theme: {}", self.settings.theme.label()), false);
    }

    /// The view currently shown. Production code navigates only through named
    /// transitions (`enter_file_diff`, `leave_file_diff`, `open_config`,
    /// `close_config`, `open_help`, `close_help`); this getter is read-only.
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// Open `target` (Config or Help), remembering the current view so `Esc`/`q` can
    /// return to it. No-op (returns `false`) while already on `target` — otherwise the
    /// top bar's mouse click for this overlay (reachable from any view, including the
    /// overlay itself) would overwrite the remembered return view, trapping Esc/`q`
    /// with no way out via the keyboard. Returns `true` when it actually transitioned,
    /// so callers can gate per-screen setup on that.
    fn open_overlay(&mut self, target: ViewMode) -> bool {
        if self.view_mode == target {
            return false;
        }
        match target {
            ViewMode::ConfigMenu => self.config.set_return_view(self.view_mode),
            ViewMode::Help => self.help.set_return_view(self.view_mode),
            _ => unreachable!("open_overlay is only used for the ConfigMenu/Help targets"),
        }
        self.view_mode = target;
        true
    }

    /// Open the Config screen, remembering the current view so `Esc`/`q` can return to it.
    pub fn open_config(&mut self) {
        if self.open_overlay(ViewMode::ConfigMenu) {
            self.ensure_config_selection();
        }
    }

    /// Leave Config and restore the view remembered by [`App::open_config`].
    ///
    /// Shared by Esc / `q` / mouse close-button on the Config screen. Pure restore:
    /// `view_mode = config's return view` only — no other side effects.
    pub(crate) fn close_config(&mut self) {
        self.view_mode = self.config.return_view();
    }

    /// Read access to the Config screen's own state (selected row, return
    /// view). Production code drives it through [`App::open_config`]/
    /// `close_config`/`ensure_config_selection`/`config_select_next`/
    /// `config_select_prev`/`config_select_at`/`config_scroll` — see
    /// `input.rs`. Exists as a read seam for tests; no production call site
    /// reads through it directly since `config_scroll` folded in the last one.
    #[allow(dead_code)]
    pub(crate) fn config(&self) -> &ConfigState {
        &self.config
    }

    /// Mutable access to the Config screen's own state. See [`App::config`].
    /// Unlike `App::help_mut`/`filter_mut`, every `ConfigState` mutator needs
    /// the row list from [`App::config_rows`], so production code always goes
    /// through an `App` orchestration method instead — this exists for tests
    /// to seed a selection directly.
    #[allow(dead_code)]
    pub(crate) fn config_mut(&mut self) -> &mut ConfigState {
        &mut self.config
    }

    /// Ensure the Config selection points at a selectable row, recomputing
    /// [`App::config_rows`] first. Orchestration: `config_rows` reads
    /// `detected_diff_tools`, a concern `ConfigState` doesn't own, so the row
    /// list is built here and handed to [`ConfigState::ensure_selection`] for
    /// the pure index math.
    pub fn ensure_config_selection(&mut self) {
        let rows = self.config_rows();
        self.config.ensure_selection(&rows);
    }

    pub fn config_select_next(&mut self) {
        let rows = self.config_rows();
        self.config.select_next(&rows);
    }

    pub fn config_select_prev(&mut self) {
        let rows = self.config_rows();
        self.config.select_prev(&rows);
    }

    /// Select config row `idx` if it exists and `is_selectable()`; otherwise no-op.
    /// Returns whether the selection was accepted. Used by mouse click on a config row.
    pub(crate) fn config_select_at(&mut self, idx: usize) -> bool {
        let rows = self.config_rows();
        self.config.select_at(idx, &rows)
    }

    /// Apply the selected Config row. Returns `true` when the change needs a
    /// background rescan, which the caller (which owns the event sender) kicks —
    /// exactly once. A failed save reports its own error toast and returns
    /// `false`, so the previous mode's results stay on screen (Issue #238).
    #[must_use]
    pub fn apply_config_selection(&mut self) -> bool {
        let rows = self.config_rows();
        match rows.get(self.config.selected_idx()) {
            Some(ConfigRowKind::ScanMode) => {
                return self.switch_scan_mode(self.scan_mode.toggled());
            }
            Some(ConfigRowKind::DiffTool(idx)) => {
                if let Some((tool, _)) = self.detected_diff_tools.get(*idx) {
                    self.settings.external_diff_tool = Some(tool.as_str().to_string());
                    let _ = self.settings.save();
                }
            }
            Some(ConfigRowKind::CheckUpdates) => {
                self.settings.check_updates = !self.settings.check_updates;
                self.update_check_enabled = self.settings.check_updates;
                let _ = self.settings.save();
            }
            Some(ConfigRowKind::Mouse) => {
                self.settings.mouse = !self.settings.mouse;
                self.mouse_enabled = self.settings.mouse;
                let _ = self.settings.save();
            }
            Some(ConfigRowKind::Theme) => {
                self.toggle_theme();
            }
            _ => {}
        }
        false
    }

    /// Nudge a numeric config field (currently only [`ConfigRowKind::DiffContext`]) up
    /// or down by one and persist. No-op for non-numeric rows.
    pub fn adjust_config_selection(&mut self, forward: bool) {
        let rows = self.config_rows();
        if let Some(ConfigRowKind::DiffContext) = rows.get(self.config.selected_idx()) {
            self.settings.diff_context = if forward {
                self.settings.diff_context.saturating_add(1).min(50)
            } else {
                self.settings.diff_context.saturating_sub(1)
            };
            let _ = self.settings.save();
        }
    }

    /// Scroll the config list by mouse wheel: adjusts the selected
    /// [`ConfigRowKind::DiffContext`] value if that row is selected, else moves
    /// the row selection. The DiffContext value moves the opposite way from
    /// row selection's "forward" sense — ScrollDown decreases it, ScrollUp
    /// increases it — matching the original per-direction call sites this
    /// replaces, so the parameter names the concrete gesture rather than an
    /// ambiguous shared "forward".
    ///
    /// The keyboard handler doesn't need this — `h`/`l` and `j`/`k` are already
    /// separate keys — but the scroll wheel's single up/down axis has to decide
    /// contextually.
    pub(crate) fn config_scroll(&mut self, scroll_down: bool) {
        let rows = self.config_rows();
        if matches!(
            rows.get(self.config.selected_idx()),
            Some(ConfigRowKind::DiffContext)
        ) {
            self.adjust_config_selection(!scroll_down);
        } else if scroll_down {
            self.config_select_next();
        } else {
            self.config_select_prev();
        }
    }

    /// Focus the left directory tree pane.
    pub fn focus_left_pane(&mut self) {
        self.active_side_left = true;
    }

    /// Focus the right directory tree pane.
    pub fn focus_right_pane(&mut self) {
        self.active_side_left = false;
    }

    /// Flip focused pane (left ↔ right). Used by Tab in the directory tree.
    ///
    /// Pure focus flip — no rescan or other side effects.
    pub(crate) fn toggle_active_side(&mut self) {
        self.active_side_left = !self.active_side_left;
    }

    /// `true` when the left pane has focus (green border / editor side).
    pub(crate) fn active_side_left(&self) -> bool {
        self.active_side_left
    }

    /// Left-hand directory being compared. Read access only; mutate via [`App::swap_paths`].
    pub fn left_path(&self) -> &Path {
        &self.left_path
    }

    /// Right-hand directory being compared. Read access only; mutate via [`App::swap_paths`].
    pub fn right_path(&self) -> &Path {
        &self.right_path
    }

    /// Swap the left and right directory paths and reset selection state.
    pub fn swap_paths(&mut self) {
        std::mem::swap(&mut self.left_path, &mut self.right_path);
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.diff.reset_for_swap();
    }

    /// Clear the status message if it has been visible longer than `duration`.
    pub fn clear_expired_status(&mut self, duration: std::time::Duration) {
        if let Some((_, _, created)) = &self.status_message {
            if created.elapsed() >= duration {
                self.status_message = None;
            }
        }
    }

    /// Read access to the file-diff content state (rows, scroll, wrap/full
    /// toggles, cached hashes/line-endings). Production code drives mutation
    /// through [`App::enter_file_diff`]/`refresh_file_diff`/
    /// `toggle_diff_show_full`/`diff_scroll_down`/etc.; rendering reads through
    /// [`App::diff_view`]/[`App::diff_layout_inputs`] instead of this directly.
    /// Test-only now (assertions in `app.rs`/`input.rs`/`main.rs`) — clippy's
    /// dead-code pass flags it as unreachable outside `#[cfg(test)]` call sites.
    #[allow(dead_code)]
    pub(crate) fn diff(&self) -> &FileDiffState {
        &self.diff
    }

    /// Mutable access to the file-diff content state. See [`App::diff`].
    pub(crate) fn diff_mut(&mut self) -> &mut FileDiffState {
        &mut self.diff
    }

    /// Borrowed snapshot of the file-diff **content** state for rendering.
    ///
    /// Used by [`crate::ui::draw_diff_content`]/[`crate::ui::draw_diff_footer`]; ui
    /// tests can build a [`crate::ui::DiffView`] by hand instead of constructing a
    /// full `App`.
    pub(crate) fn diff_view(&self) -> crate::ui::DiffView<'_> {
        let viewport = self.viewport();
        crate::ui::DiffView {
            rows: self.diff.rows(),
            wrap: self.diff.wrap(),
            scroll: self.diff.scroll(),
            h_scroll: self.diff.h_scroll(),
            visible_height: viewport.visible_height,
            content_width: viewport.diff_content_width,
            left_root: &self.left_path,
            right_root: &self.right_path,
            row: self.selected_row(),
            left_hash: self.diff.left_hash(),
            right_hash: self.diff.right_hash(),
            left_line_ending: self.diff.left_line_ending(),
            right_line_ending: self.diff.right_line_ending(),
            theme: self.theme(),
            status_toast: self.status_toast(),
            has_changes: self.diff.has_changes(),
            update_available: self.update_available(),
            install_method: self.install_method(),
        }
    }

    /// Pure geometry-decision inputs for [`crate::ui::diff_layout`]: whether the
    /// selected row shows the "identical" notice, and whether the status/update
    /// footer lines are present. Built once and reused by both [`App::sync_viewport`]
    /// (geometry) and [`crate::ui::draw_diff`] (render), so the two cannot compute
    /// different `show_identical`/footer-height decisions for the same frame.
    pub(crate) fn diff_layout_inputs(&self) -> crate::ui::DiffLayoutInputs {
        let row = self.selected_row();
        crate::ui::DiffLayoutInputs {
            has_changes: self.diff.has_changes(),
            row_has_content: row.is_some_and(|r| r.left.is_some() || r.right.is_some()),
            has_status: self.status_toast().is_some(),
            has_update: self.update_available().is_some(),
        }
    }

    /// Borrowed snapshot of the directory-tree **content** state for rendering.
    ///
    /// Used by [`crate::ui::draw_tree_content`]; ui tests can build a
    /// [`crate::ui::TreeView`] by hand instead of constructing a full `App`.
    pub(crate) fn tree_view(&self) -> crate::ui::TreeView<'_> {
        crate::ui::TreeView {
            rows: self.filter.rows(),
            scroll_offset: self.scroll_offset,
            selected_idx: self.selected_idx,
            visible_height: self.viewport().visible_height,
            left_root: &self.left_path,
            right_root: &self.right_path,
            active_side_left: self.active_side_left,
            theme: self.theme(),
        }
    }

    /// Borrowed snapshot of the directory-tree **footer** state for rendering.
    ///
    /// Used by [`crate::ui::draw_tree_footer`]; ui tests can build a
    /// [`crate::ui::TreeFooterView`] by hand instead of constructing a full `App`.
    /// Separate from [`App::tree_view`] because the footer needs several more
    /// fields than the content pane ever reads.
    pub(crate) fn tree_footer_view(&self) -> crate::ui::TreeFooterView<'_> {
        crate::ui::TreeFooterView {
            row: self.selected_row(),
            status_toast: self.status_toast(),
            filter_active: self.filter.active(),
            filter_input: self.filter.input(),
            filter_pattern: self.filter.pattern(),
            filter_diffs_only: self.filter.editing_diffs_only(),
            scan_in_progress: self.scan_in_progress(),
            update_available: self.update_available(),
            install_method: self.install_method(),
            theme: self.theme(),
        }
    }

    /// Pure geometry-decision inputs for [`crate::ui::tree_layout`]: whether the
    /// footer shows a detail line / status toast / filter bar / update hint. Built
    /// once and reused by both [`App::sync_viewport`] (geometry) and
    /// [`crate::ui::draw_tree`] (render), so the two cannot compute different
    /// footer-height decisions for the same frame. Same shape as
    /// [`App::diff_layout_inputs`].
    pub(crate) fn tree_layout_inputs(&self) -> crate::ui::TreeLayoutInputs {
        crate::ui::TreeLayoutInputs {
            has_detail: crate::ui::selected_row_detail(self.selected_row()).is_some(),
            has_status: self.status_toast().is_some(),
            has_filter: self.filter.active(),
            has_update: self.update_available().is_some(),
        }
    }

    /// Borrowed snapshot of the Help **body** state for rendering.
    ///
    /// Used by [`crate::ui::draw_help_content`]; ui tests can build a
    /// [`crate::ui::HelpView`] by hand instead of constructing a full `App`.
    pub(crate) fn help_view(&self) -> crate::ui::HelpView<'_> {
        crate::ui::HelpView {
            topic: self.help.topic(),
            index_open: self.help.index_open(),
            index_sel: self.help.index_sel(),
            scroll: self.help.scroll(),
            theme: self.theme(),
            update_available: self.update_available.as_deref(),
            install_method: &self.install_method,
        }
    }

    /// Snapshot of the Config **list** state for rendering.
    ///
    /// Call after [`App::ensure_config_selection`] so `selected_idx` is valid.
    /// Used by [`crate::ui::draw_config_content`].
    pub(crate) fn config_view(&self) -> crate::ui::ConfigView<'_> {
        crate::ui::ConfigView {
            rows: self.config_rows(),
            selected_idx: self.config.selected_idx(),
            detected_diff_tools: &self.detected_diff_tools,
            external_diff_tool: self.settings.external_diff_tool.as_deref(),
            check_updates: self.settings.check_updates,
            mouse: self.settings.mouse,
            theme_choice: self.settings.theme,
            diff_context: self.settings.diff_context,
            scan_mode: self.scan_mode,
            saved_scan_mode: self.settings.scan_mode,
            theme: self.theme(),
        }
    }

    /// Pure title-bar chrome for the current view.
    pub(crate) fn top_bar_view(&self) -> crate::ui::TopBarView {
        crate::ui::TopBarView {
            view_mode: self.view_mode,
            precise_mode: self.precise_mode(),
            diff_show_full: self.diff.show_full(),
            diff_wrap: self.diff.wrap(),
            theme: self.theme(),
        }
    }

    /// Confirm dialog message + theme (empty message if no modal).
    pub(crate) fn confirm_view(&self) -> crate::ui::ConfirmView<'_> {
        crate::ui::ConfirmView {
            message: self
                .confirm_modal
                .as_ref()
                .map(|m| m.message.as_str())
                .unwrap_or(""),
            theme: self.theme(),
        }
    }

    /// The currently selected filtered row, if any.
    pub(crate) fn selected_row(&self) -> Option<&FlatRow> {
        self.filter.rows().get(self.selected_idx)
    }

    /// Jump to the next differing block in the diff view (wraps around).
    pub fn jump_to_next_change(&mut self) {
        let width = self.viewport.diff_content_width.max(1);
        self.diff.jump_to_change(width, true);
    }

    /// Jump to the previous differing block in the diff view (wraps around).
    pub fn jump_to_prev_change(&mut self) {
        let width = self.viewport.diff_content_width.max(1);
        self.diff.jump_to_change(width, false);
    }

    /// Recompute the built-in diff for the currently selected file pair.
    ///
    /// Returns `Err` when a side is binary, non-UTF-8, or over the size limit so
    /// callers can surface a toast instead of opening an empty/false view.
    pub fn refresh_file_diff(&mut self) -> Result<(), String> {
        let Some(row) = self.selected_row() else {
            return Err("no file selected".to_string());
        };
        let left_file = self.left_path.join(&row.relative_path);
        let right_file = self.right_path.join(&row.relative_path);
        self.diff
            .load(&left_file, &right_file, self.settings.diff_context)?;
        self.resync_diff_geometry();
        Ok(())
    }

    /// Flip full-file vs. diff-only content in the diff view and reload it.
    ///
    /// On failure the flag is rolled back and the current diff view is left
    /// untouched; callers should surface the error via a status toast.
    pub fn toggle_diff_show_full(&mut self) -> Result<(), String> {
        self.diff.toggle_show_full();
        if let Err(e) = self.refresh_file_diff() {
            self.diff.toggle_show_full();
            return Err(e);
        }
        self.diff.reset_scroll();
        Ok(())
    }

    /// Recompute the diff-rows-derived half of [`Viewport`] at the last known
    /// content width.
    ///
    /// [`App::sync_viewport`] redoes this every frame; this exists so callers that
    /// replace the diff rows mid-frame (loading another file, applying a hunk copy)
    /// can clamp scrolling against the new content instead of the old row count.
    fn resync_diff_geometry(&mut self) {
        self.viewport.diff_max_line_width = crate::diff_view::diff_max_line_width(self.diff.rows());
        self.viewport.diff_physical_rows = crate::diff_view::diff_total_physical_rows(
            self.diff.rows(),
            self.viewport.diff_content_width,
            self.diff.wrap(),
        );
    }

    /// Pull the diff view's scroll offsets back inside the current geometry.
    ///
    /// Growing the terminal (or opening a shorter file) can leave the scroll
    /// offsets past the end of the content; without this the next page or
    /// arrow key would appear to jump backwards.
    fn clamp_diff_scroll(&mut self) {
        let max_scroll = self.viewport.max_diff_scroll();
        let max_h_scroll = self.viewport.max_diff_h_scroll();
        self.diff.clamp_scroll(max_scroll, max_h_scroll);
    }

    /// Open the built-in File Diff view for the current selection.
    /// On load failure, keeps the current view and sets an error status toast.
    pub fn enter_file_diff(&mut self) -> bool {
        let Some(row) = self.selected_row() else {
            return false;
        };
        let is_dir = row.is_dir();
        if is_dir {
            return false;
        }
        self.diff.set_show_full(false);
        match self.refresh_file_diff() {
            Ok(()) => {
                self.view_mode = ViewMode::FileDiff;
                self.diff.reset_scroll();
                true
            }
            Err(e) => {
                self.set_status(format!("Cannot open diff: {e}"), true);
                false
            }
        }
    }

    /// Leave the File Diff view and return to the Directory Tree.
    ///
    /// Shared by Esc/`q`, the mouse close glyph, the post-copy return-to-tree, and
    /// the command palette's "back" action.
    pub fn leave_file_diff(&mut self) {
        self.view_mode = ViewMode::DirectoryTree;
    }

    /// Copy the change hunk at the current scroll position in the given direction.
    pub fn copy_hunk_at_cursor(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<(), std::io::Error> {
        let Some(row) = self.selected_row() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no file selected",
            ));
        };
        let width = self.viewport.diff_content_width.max(1);
        let hunk_index = crate::diff_view::hunk_index_at_scroll(
            self.diff.rows(),
            self.diff.scroll(),
            width,
            self.diff.wrap(),
        )
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no change block at cursor",
            )
        })?;
        let left_file = self.left_path.join(&row.relative_path);
        let right_file = self.right_path.join(&row.relative_path);
        let prev_scroll = self.diff.scroll();
        crate::diff_view::apply_hunk_copy(
            &left_file,
            &right_file,
            self.diff.rows(),
            hunk_index,
            direction,
        )?;
        self.refresh_file_diff().map_err(std::io::Error::other)?;
        let max_scroll = self.viewport.max_diff_scroll();
        self.diff.set_scroll(prev_scroll.min(max_scroll));
        Ok(())
    }

    pub fn flatten_tree(&mut self) {
        self.flat_rows.clear();
        if let Some(root) = self.root_node.take() {
            self.flatten_node(&root, 0);
            self.root_node = Some(root);
        }
        self.apply_filter();
    }

    fn flatten_node(&mut self, node: &AlignedNode, depth: usize) {
        self.flat_rows.push(FlatRow {
            depth,
            relative_path: node.relative_path.clone(),
            name: node.name.clone(),
            state: node.state,
            left: node.left.clone(),
            right: node.right.clone(),
        });
        if node.is_expanded {
            for child in &node.children {
                self.flatten_node(child, depth + 1);
            }
        }
    }

    /// Relative path of the currently selected filtered row, if any.
    pub fn selected_relative_path(&self) -> Option<PathBuf> {
        self.selected_row().map(|r| r.relative_path.clone())
    }

    /// Collect relative paths of expanded directories in the current tree.
    /// Used to restore expand state after a full rescan.
    pub fn collect_expanded_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(root) = &self.root_node {
            Self::collect_expanded_paths_node(root, &mut paths);
        }
        paths
    }

    fn collect_expanded_paths_node(node: &AlignedNode, paths: &mut Vec<PathBuf>) {
        if node.is_expanded {
            paths.push(node.relative_path.clone());
        }
        for child in &node.children {
            Self::collect_expanded_paths_node(child, paths);
        }
    }

    /// Re-expand directories whose relative paths appear in `paths`.
    /// Paths that no longer exist after a rescan are ignored.
    pub fn restore_expanded_paths(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        if let Some(ref mut root) = self.root_node {
            for path in paths {
                Self::set_expand_node(root, path, true);
            }
        }
    }

    /// Re-align only the affected directory after a copy and graft it into the
    /// existing tree (preserving expand/selection via flatten).
    ///
    /// - Directory copy: re-scan that directory path.
    /// - File copy: re-scan its parent directory.
    /// - Root-level / empty tree: returns `Err` so the caller can fall back to a
    ///   full background scan.
    pub fn apply_incremental_rescan(
        &mut self,
        copied_rel: &std::path::Path,
        copied_is_dir: bool,
    ) -> Result<(), std::io::Error> {
        let scan_rel: PathBuf = if copied_is_dir {
            copied_rel.to_path_buf()
        } else {
            copied_rel
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        };

        // Full-tree realign should stay on the async scanner path.
        if scan_rel.as_os_str().is_empty() {
            return Err(std::io::Error::other(
                "incremental rescan not used for root",
            ));
        }

        let expanded = self.collect_expanded_paths();
        let new_node = crate::diff::align_directories(
            &self.left_path,
            &self.right_path,
            &scan_rel,
            self.precise_mode(),
            &self.ignore_matcher,
        )?;

        let Some(root) = self.root_node.as_mut() else {
            self.root_node = Some(new_node);
            self.restore_expanded_paths(&expanded);
            self.flatten_tree();
            return Ok(());
        };

        if !crate::diff::replace_subtree(root, &scan_rel, new_node) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "subtree path not found in tree",
            ));
        }

        self.restore_expanded_paths(&expanded);
        self.flatten_tree();
        Ok(())
    }

    /// Read access to the filter bar's own state (input text, committed
    /// pattern, diffs-only flag, filtered rows). Production code drives it
    /// through [`App::apply_filter`]/`commit_filter`/`clear_filter` plus
    /// [`FilterState`]'s own methods (see `input.rs`).
    pub(crate) fn filter(&self) -> &FilterState {
        &self.filter
    }

    /// Mutable access to the filter bar's own state. See [`App::filter`].
    pub(crate) fn filter_mut(&mut self) -> &mut FilterState {
        &mut self.filter
    }

    /// Rebuild the filtered row list from `flat_rows` using the filter bar's
    /// current pattern and diffs-only flag. Preserves selection and scroll
    /// position by matching the previously selected relative path when still
    /// present. Orchestration: combines `filter`'s pure recompute with the
    /// nav-concern (`selected_idx`/`scroll_offset`) and scan-concern
    /// (`flat_rows`) fields that stay flat on `App`.
    pub fn apply_filter(&mut self) {
        let prev_path = self.selected_relative_path();
        let prev_scroll = self.scroll_offset;

        self.filter.recompute(&self.flat_rows);

        if self.filter.rows().is_empty() {
            self.selected_idx = 0;
            self.scroll_offset = 0;
            return;
        }

        if let Some(path) = prev_path {
            if let Some(idx) = self
                .filter
                .rows()
                .iter()
                .position(|r| r.relative_path == path)
            {
                self.selected_idx = idx;
                let max_scroll = self.filter.rows().len().saturating_sub(1);
                self.scroll_offset = prev_scroll.min(max_scroll);
                self.adjust_scroll(self.viewport.visible_height);
                return;
            }
        }

        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    /// Open the confirm modal with a prompt and the action to run if accepted.
    pub fn request_confirm(&mut self, message: impl Into<String>, action: ConfirmAction) {
        self.confirm_modal = Some(ConfirmModal {
            message: message.into(),
            action,
        });
    }

    /// Ask for confirmation before copying the selected row across sides. No-op if
    /// nothing is selected or the source side is empty — the guard every one of the
    /// three trigger sites (DirectoryTree `L`/`R`, FileDiff `l`/`L`/`r`/`R`, palette
    /// `copy_l2r`/`copy_r2l`) used to duplicate by hand.
    pub fn request_copy(&mut self, direction: ConfirmAction) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let source_present = match direction {
            ConfirmAction::CopyLeftToRight => row.left.is_some(),
            ConfirmAction::CopyRightToLeft => row.right.is_some(),
        };
        if !source_present {
            return;
        }
        let name = row.name.clone();
        let dest_label = match direction {
            ConfirmAction::CopyLeftToRight => "right",
            ConfirmAction::CopyRightToLeft => "left",
        };
        self.request_confirm(
            format!("Copy '{}' to {} side?", name, dest_label),
            direction,
        );
    }

    /// Close the confirm modal, returning the pending action to run (the "confirm" path).
    pub fn take_confirmed_action(&mut self) -> Option<ConfirmAction> {
        self.confirm_modal.take().map(|modal| modal.action)
    }

    /// Close the confirm modal, discarding the pending action (the "cancel" path).
    pub fn dismiss_confirm(&mut self) {
        self.confirm_modal = None;
    }

    /// The pending confirm modal, if one is open. Read access for rendering.
    pub fn confirm_modal(&self) -> Option<&ConfirmModal> {
        self.confirm_modal.as_ref()
    }

    /// Open the Help screen, remembering the current view so `Esc`/`q`/`?` can
    /// return to it, and jumping straight to that view's contextual topic body (the topic
    /// index is only shown once the user explicitly presses Tab).
    pub fn open_help(&mut self) {
        if !self.open_overlay(ViewMode::Help) {
            return;
        }
        let topic = HelpTopic::for_view(self.help.return_view());
        self.help.enter(topic);
    }

    /// Leave Help: restore `view_mode` from the Help state's remembered return
    /// view and close the topic index. Unifies the body-Esc and index-Esc paths
    /// (body already has the index closed; closing it again is a no-op UX-wise).
    pub(crate) fn close_help(&mut self) {
        self.view_mode = self.help.return_view();
        self.help.leave();
    }

    /// Read access to the Help screen's own state (active topic, topic index,
    /// scroll, return view). Production code drives it through [`App::open_help`]/
    /// [`App::close_help`] plus [`HelpState`]'s own methods (see `input.rs`).
    pub(crate) fn help(&self) -> &HelpState {
        &self.help
    }

    /// Mutable access to the Help screen's own state. See [`App::help`].
    pub(crate) fn help_mut(&mut self) -> &mut HelpState {
        &mut self.help
    }

    /// Close the filter input bar, committing the typed text as the pattern,
    /// and recompute the row list via [`App::apply_filter`].
    pub fn commit_filter(&mut self) {
        self.filter.commit();
        self.apply_filter();
    }

    /// Clear the filter entirely (pattern + diffs-only) and recompute the row
    /// list via [`App::apply_filter`].
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.apply_filter();
    }

    pub fn toggle_expand(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let is_dir = row.is_dir();
        if !is_dir {
            return;
        }
        let rel_path = row.relative_path.clone();
        if let Some(ref mut root) = self.root_node {
            Self::toggle_expand_node(root, &rel_path);
        }
        self.flatten_tree();
    }

    fn toggle_expand_node(node: &mut AlignedNode, target_path: &std::path::Path) {
        if node.relative_path == target_path {
            node.is_expanded = !node.is_expanded;
            return;
        }
        for child in &mut node.children {
            Self::toggle_expand_node(child, target_path);
        }
    }

    pub fn select_next(&mut self) {
        if !self.filter.rows().is_empty() && self.selected_idx < self.filter.rows().len() - 1 {
            self.selected_idx += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    /// Page size for list/diff paging (`Ctrl+f` / `Ctrl+b`).
    ///
    /// Uses the last drawn content height, with a one-row overlap when possible
    /// so context isn't completely lost between pages.
    fn page_step(&self) -> usize {
        self.viewport.visible_height.saturating_sub(1).max(1)
    }

    /// Move the directory-tree selection down by one page (`Ctrl+f`).
    pub fn page_down(&mut self) {
        if self.filter.rows().is_empty() {
            return;
        }
        let max_idx = self.filter.rows().len() - 1;
        self.selected_idx = (self.selected_idx + self.page_step()).min(max_idx);
        self.adjust_scroll(self.viewport.visible_height);
    }

    /// Move the directory-tree selection up by one page (`Ctrl+b`).
    pub fn page_up(&mut self) {
        if self.filter.rows().is_empty() {
            return;
        }
        self.selected_idx = self.selected_idx.saturating_sub(self.page_step());
        self.adjust_scroll(self.viewport.visible_height);
    }

    /// Scroll the file-diff view down by one page (`Ctrl+f`).
    pub fn diff_page_down(&mut self) {
        let step = self.page_step();
        let max = self.viewport.max_diff_scroll();
        self.diff.page_down(step, max);
    }

    /// Scroll the file-diff view up by one page (`Ctrl+b`).
    pub fn diff_page_up(&mut self) {
        let step = self.page_step();
        self.diff.page_up(step);
    }

    /// Line-step the file-diff view down, clamped to `viewport.max_diff_scroll()`
    /// (no-op at the end). Shared by keyboard j/Down and mouse scroll down.
    pub(crate) fn diff_scroll_down(&mut self) {
        let max = self.viewport.max_diff_scroll();
        self.diff.scroll_down(max);
    }

    /// Horizontal step right, when wrap is off, clamped to
    /// `viewport.max_diff_h_scroll()`.
    pub(crate) fn diff_h_scroll_right(&mut self) {
        let max = self.viewport.max_diff_h_scroll();
        self.diff.h_scroll_right(max);
    }

    pub fn expand_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let is_dir = row.is_dir();
        if !is_dir {
            return;
        }
        let rel_path = row.relative_path.clone();
        if let Some(ref mut root) = self.root_node {
            Self::set_expand_node(root, &rel_path, true);
        }
        self.flatten_tree();
    }

    pub fn collapse_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let is_dir = row.is_dir();
        if !is_dir {
            return;
        }
        let rel_path = row.relative_path.clone();
        if let Some(ref mut root) = self.root_node {
            Self::set_expand_node(root, &rel_path, false);
        }
        self.flatten_tree();
    }

    fn set_expand_node(node: &mut AlignedNode, target_path: &std::path::Path, expand: bool) {
        if node.relative_path == target_path {
            node.is_expanded = expand;
            return;
        }
        for child in &mut node.children {
            Self::set_expand_node(child, target_path, expand);
        }
    }

    pub fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected_idx - visible_height + 1;
        }
    }

    /// Select filtered row `idx` if in range. Used by mouse left/right click.
    /// Does not change scroll by itself (matches current mouse path; frame
    /// `sync_viewport` / keyboard page paths still call `adjust_scroll`).
    pub(crate) fn select_row_at(&mut self, idx: usize) -> bool {
        if idx >= self.filter.rows().len() {
            return false;
        }
        self.selected_idx = idx;
        true
    }

    /// Record a tree click at `idx` for double-click detection (400ms window).
    /// Returns `true` if this click is a double-click on the same index.
    /// On double-click, clears `last_click_*`; otherwise stores idx + now.
    /// Caller is responsible for `select_row_at` first (or this may select too —
    /// prefer: call `select_row_at` then `note_tree_click`, matching current order).
    pub(crate) fn note_tree_click(&mut self, idx: usize) -> bool {
        let now = std::time::Instant::now();
        let is_double_click = Some(idx) == self.last_click_idx
            && self
                .last_click_time
                .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(400));
        if is_double_click {
            self.last_click_idx = None;
            self.last_click_time = None;
        } else {
            self.last_click_idx = Some(idx);
            self.last_click_time = Some(now);
        }
        is_double_click
    }

    /// The directory-tree selection cursor.
    /// Production render reads this via [`App::tree_view`]; getter is for tests.
    #[allow(dead_code)]
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    /// The directory-tree list's vertical scroll offset. Read access for
    /// mouse hit-testing / tests.
    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Open the Command Palette. Shared by `;`, `Ctrl+p`, and right-click, so all
    /// three land on the same contextual inventory: the query is cleared and the
    /// first enabled action is selected (Issue #239).
    pub(crate) fn open_palette(&mut self) {
        self.palette.visible = true;
        self.palette.query.clear();
        self.refresh_palette_items();
        self.palette_select_first_enabled();
    }

    /// Dismiss the palette: hidden, query cleared.
    pub(crate) fn close_palette(&mut self) {
        self.palette.visible = false;
        self.palette.query.clear();
    }

    /// Move the selection to the first enabled action, or to `0` when the
    /// inventory is empty or entirely disabled.
    fn palette_select_first_enabled(&mut self) {
        self.palette.selected_idx = self
            .palette
            .items
            .iter()
            .position(|a| a.enabled())
            .unwrap_or(0);
        self.palette.scroll_offset = 0;
    }

    /// Wrap-around next over `palette.items` (no-op when empty).
    pub(crate) fn palette_select_next(&mut self) {
        if self.palette.items.is_empty() {
            return;
        }
        self.palette.selected_idx = (self.palette.selected_idx + 1) % self.palette.items.len();
    }

    /// Wrap-around previous over `palette.items` (no-op when empty).
    pub(crate) fn palette_select_prev(&mut self) {
        if self.palette.items.is_empty() {
            return;
        }
        self.palette.selected_idx = self
            .palette
            .selected_idx
            .checked_sub(1)
            .unwrap_or(self.palette.items.len() - 1);
    }

    /// Keep `selected_idx` inside a `visible_rows`-tall list viewport. Called
    /// once per frame from the render shell, which is the only place that knows
    /// the popup's clamped height.
    pub(crate) fn sync_palette_viewport(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            self.palette.scroll_offset = 0;
            return;
        }
        let max_offset = self.palette.items.len().saturating_sub(visible_rows);
        if self.palette.selected_idx < self.palette.scroll_offset {
            self.palette.scroll_offset = self.palette.selected_idx;
        } else if self.palette.selected_idx >= self.palette.scroll_offset + visible_rows {
            self.palette.scroll_offset = self.palette.selected_idx + 1 - visible_rows;
        }
        self.palette.scroll_offset = self.palette.scroll_offset.min(max_offset);
    }

    /// Query edit: append one character, re-filter, and reselect.
    pub(crate) fn palette_type_char(&mut self, c: char) {
        self.palette.query.push(c);
        self.refresh_palette_items();
        self.palette_select_first_enabled();
    }

    /// Query edit: drop the trailing character, re-filter, and reselect.
    pub(crate) fn palette_backspace(&mut self) {
        self.palette.query.pop();
        self.refresh_palette_items();
        self.palette_select_first_enabled();
    }

    /// Rebuild `palette.items` from [`Self::build_palette_actions`], keeping only
    /// the actions whose key or label contains the query — a case-insensitive
    /// substring search, not fuzzy matching. Called on every query edit and once
    /// per frame from `draw_palette`.
    pub(crate) fn refresh_palette_items(&mut self) {
        let query = self.palette.query.to_lowercase();
        self.palette.items = self
            .build_palette_actions()
            .into_iter()
            .filter(|a| {
                a.label.to_lowercase().contains(&query) || a.key.to_lowercase().contains(&query)
            })
            .collect();
        if self.palette.selected_idx >= self.palette.items.len() {
            self.palette.selected_idx = self.palette.items.len().saturating_sub(1);
        }
    }

    /// Read access for render / hit-test (mode, query, items, selected_idx).
    pub(crate) fn palette(&self) -> &PaletteState {
        &self.palette
    }

    /// Borrowed snapshot of the palette/menu popup for rendering.
    ///
    /// Call after [`App::refresh_palette_items`] so `items` match the query.
    /// Used by [`crate::ui::draw_palette_content`].
    pub(crate) fn palette_view(&self) -> crate::ui::PaletteView<'_> {
        crate::ui::PaletteView {
            items: &self.palette.items,
            selected_idx: self.palette.selected_idx,
            scroll_offset: self.palette.scroll_offset,
            query: &self.palette.query,
            theme: self.theme(),
        }
    }

    /// Convenience for the many `if app.palette.visible` guards.
    pub(crate) fn palette_visible(&self) -> bool {
        self.palette.visible
    }

    /// The Command Palette's contextual inventory for the active view and
    /// selection: every discrete state-changing or feature-entry command, with
    /// continuous cursor/page/horizontal scrolling deliberately left out.
    /// Unavailable actions stay listed, carrying the reason they cannot run, so
    /// the inventory does not change shape with the selection (Issue #239).
    pub fn build_palette_actions(&self) -> Vec<crate::ui::PaletteAction> {
        use crate::ui::{PaletteAction as A, PaletteActionId as Id};

        let mut actions = Vec::new();
        match self.view_mode {
            ViewMode::DirectoryTree => {
                let row = self.selected_row();
                let has_row = row.is_some();
                let is_dir = row.is_some_and(|r| r.is_dir());
                let is_file_pair =
                    row.is_some_and(|r| !r.is_dir() && r.left.is_some() && r.right.is_some());
                let is_file_active = row.is_some_and(|r| {
                    if self.active_side_left {
                        r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    } else {
                        r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    }
                });
                // Every gated Directory Tree action falls back to the same
                // reason when nothing is selected at all.
                let reason = |specific: &'static str| {
                    if has_row {
                        specific
                    } else {
                        "no row is selected"
                    }
                };

                actions.push(A::gated(
                    "Enter",
                    "Open built-in Diff view",
                    Id::BuiltinDiff,
                    row.is_some_and(|r| !r.is_dir()),
                    reason("the selected row is a directory"),
                ));
                actions.push(A::gated(
                    "D",
                    "Compare via External Diff Tool",
                    Id::ExternalDiff,
                    is_file_pair && self.settings.external_diff_tool.is_some(),
                    if self.settings.external_diff_tool.is_none() {
                        "no external diff tool is configured"
                    } else {
                        "needs a file present on both sides"
                    },
                ));
                actions.push(A::gated(
                    "E",
                    "Edit via External Editor",
                    Id::ExternalEdit,
                    is_file_active,
                    "the focused pane has no file at this row",
                ));
                actions.push(A::gated(
                    "R",
                    "Copy Left to Right",
                    Id::CopyLeftToRight,
                    row.is_some_and(|r| r.left.is_some()),
                    reason("nothing on the left side to copy"),
                ));
                actions.push(A::gated(
                    "L",
                    "Copy Right to Left",
                    Id::CopyRightToLeft,
                    row.is_some_and(|r| r.right.is_some()),
                    reason("nothing on the right side to copy"),
                ));
                actions.push(A::gated(
                    "l / Right",
                    "Expand selected directory",
                    Id::ExpandSelected,
                    is_dir,
                    reason("the selected row is not a directory"),
                ));
                actions.push(A::gated(
                    "h / Left",
                    "Collapse selected directory",
                    Id::CollapseSelected,
                    is_dir,
                    reason("the selected row is not a directory"),
                ));
                actions.push(A::new("Tab", "Switch focused pane", Id::ToggleFocus));
                actions.push(A::new("1", "Focus Left pane", Id::FocusLeft));
                actions.push(A::new("2", "Focus Right pane", Id::FocusRight));
                actions.push(A::new("/", "Open Filter Input", Id::Filter));
                actions.push(A::new("s", "Swap Left/Right Paths", Id::SwapPaths));
                actions.push(A::new(
                    "c",
                    "Toggle Scan Mode (Fast/Precise)",
                    Id::ToggleScan,
                ));
                actions.push(A::new("r", "Manual Re-scan / Refresh", Id::Refresh));
                actions.push(A::new("T", "Toggle Light/Dark Theme", Id::ToggleTheme));
                actions.push(A::new("C", "Edit Configuration", Id::Config));
                actions.push(A::new("?", "Open Help Screen", Id::Help));
                actions.push(A::new("q", "Quit duodiff", Id::Quit));
            }
            ViewMode::FileDiff => {
                let has_changes = self.diff.has_changes();
                let row = self.selected_row();
                let is_file_pair =
                    row.is_some_and(|r| !r.is_dir() && r.left.is_some() && r.right.is_some());
                let no_changes = "the two sides have no differing lines";

                actions.push(A::gated(
                    "N",
                    "Next Change",
                    Id::NextChange,
                    has_changes,
                    no_changes,
                ));
                actions.push(A::gated(
                    "P",
                    "Previous Change",
                    Id::PrevChange,
                    has_changes,
                    no_changes,
                ));
                actions.push(A::gated(
                    "]",
                    "Copy Change Block to Right",
                    Id::CopyHunkLeftToRight,
                    has_changes,
                    no_changes,
                ));
                actions.push(A::gated(
                    "[",
                    "Copy Change Block to Left",
                    Id::CopyHunkRightToLeft,
                    has_changes,
                    no_changes,
                ));
                actions.push(A::gated(
                    "R",
                    "Copy Whole File Left to Right",
                    Id::CopyLeftToRight,
                    row.is_some_and(|r| r.left.is_some()),
                    "nothing on the left side to copy",
                ));
                actions.push(A::gated(
                    "L",
                    "Copy Whole File Right to Left",
                    Id::CopyRightToLeft,
                    row.is_some_and(|r| r.right.is_some()),
                    "nothing on the right side to copy",
                ));
                actions.push(A::gated(
                    "D",
                    "Compare via External Diff Tool",
                    Id::ExternalDiff,
                    is_file_pair && self.settings.external_diff_tool.is_some(),
                    if self.settings.external_diff_tool.is_none() {
                        "no external diff tool is configured"
                    } else {
                        "needs a file present on both sides"
                    },
                ));
                actions.push(A::gated(
                    "E",
                    "Edit via External Editor",
                    Id::ExternalEdit,
                    row.is_some_and(|r| {
                        if self.active_side_left {
                            r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                        } else {
                            r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                        }
                    }),
                    "the focused pane has no file at this row",
                ));
                actions.push(A::new("w", "Toggle Wrap Mode", Id::ToggleWrap));
                actions.push(A::new("f", "Toggle Full Content", Id::ToggleFullDiff));
                actions.push(A::new("T", "Toggle Light/Dark Theme", Id::ToggleTheme));
                actions.push(A::new("C", "Edit Configuration", Id::Config));
                actions.push(A::new("?", "Open Help Screen", Id::Help));
                actions.push(A::new("Esc", "Return to Tree View", Id::Back));
            }
            ViewMode::ConfigMenu | ViewMode::Help => {
                actions.push(A::new("T", "Toggle Light/Dark Theme", Id::ToggleTheme));
                if self.view_mode == ViewMode::Help {
                    actions.push(A::new("C", "Edit Configuration", Id::Config));
                } else {
                    actions.push(A::new("?", "Open Help Screen", Id::Help));
                }
                actions.push(A::new("Esc", "Go Back", Id::Back));
            }
        }
        actions
    }
}

/// Seams for tests in sibling modules, which cannot reach `App`'s private state
/// but still need to stand up a tree, a row list, or a viewport without running a
/// real scan or a real terminal.
#[cfg(test)]
impl App {
    pub(crate) fn flat_rows(&self) -> &[FlatRow] {
        &self.flat_rows
    }

    pub(crate) fn push_flat_row(&mut self, row: FlatRow) {
        self.flat_rows.push(row);
    }

    pub(crate) fn scan_generation(&self) -> u64 {
        self.scan_generation
    }

    pub(crate) fn set_flat_rows(&mut self, rows: Vec<FlatRow>) {
        self.flat_rows = rows;
    }

    pub(crate) fn set_palette_items(&mut self, items: Vec<crate::ui::PaletteAction>) {
        self.palette.items = items;
    }

    pub(crate) fn set_palette_selected_idx(&mut self, idx: usize) {
        self.palette.selected_idx = idx;
    }

    /// Install a tree and flatten it, as [`App::apply_scan_result`] would.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.flatten_tree();
    }

    pub(crate) fn set_view_mode(&mut self, view_mode: ViewMode) {
        self.view_mode = view_mode;
    }

    pub(crate) fn set_theme(&mut self, theme: crate::theme::ThemeChoice) {
        self.settings.theme = theme;
    }

    pub(crate) fn set_external_diff_tool(&mut self, tool: Option<String>) {
        self.settings.external_diff_tool = tool;
    }

    pub(crate) fn set_detected_diff_tools(
        &mut self,
        tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    ) {
        self.detected_diff_tools = tools;
    }

    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }

    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    pub(crate) fn set_active_side_left(&mut self, left: bool) {
        self.active_side_left = left;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{lock_env_tests, ConfigEnvGuard, RedirectedConfigDir};
    use std::time::SystemTime;

    fn file_info(is_dir: bool) -> FileInfo {
        FileInfo {
            is_dir,
            size: 0,
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn flat_row_with_sides(left: Option<FileInfo>, right: Option<FileInfo>) -> FlatRow {
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from("entry"),
            name: "entry".to_string(),
            state: DiffState::Identical,
            left,
            right,
        }
    }

    #[test]
    fn test_flat_row_is_dir_true_when_either_side_is_a_directory() {
        assert!(flat_row_with_sides(Some(file_info(true)), Some(file_info(true))).is_dir());
        assert!(flat_row_with_sides(Some(file_info(true)), Some(file_info(false))).is_dir());
        assert!(flat_row_with_sides(None, Some(file_info(true))).is_dir());
        assert!(flat_row_with_sides(Some(file_info(true)), None).is_dir());
    }

    #[test]
    fn test_flat_row_is_dir_false_when_both_sides_are_files_or_missing() {
        assert!(!flat_row_with_sides(Some(file_info(false)), Some(file_info(false))).is_dir());
        assert!(!flat_row_with_sides(None, Some(file_info(false))).is_dir());
        assert!(!flat_row_with_sides(Some(file_info(false)), None).is_dir());
        assert!(!flat_row_with_sides(None, None).is_dir());
    }

    #[test]
    fn test_flatten_tree() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "child".to_string(),
                relative_path: PathBuf::from("child"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();

        // We expect root and child to be flattened since root is expanded
        assert_eq!(app.flat_rows.len(), 2, "Expected 2 flattened rows");
        assert_eq!(app.flat_rows[0].name, "root");
        assert_eq!(app.flat_rows[1].name, "child");
        assert_eq!(app.flat_rows[1].depth, 1, "Child depth should be 1");
    }

    #[test]
    fn test_select_next_prev() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();

        assert_eq!(app.selected_idx(), 0);
        app.select_next();
        assert_eq!(app.selected_idx(), 1);
        app.select_next();
        assert_eq!(app.selected_idx(), 1); // bounds check
        app.select_prev();
        assert_eq!(app.selected_idx(), 0);
        app.select_prev();
        assert_eq!(app.selected_idx(), 0); // bounds check
    }

    #[test]
    fn test_page_down_up_moves_by_visible_height() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = (0..20)
            .map(|i| FlatRow {
                depth: 0,
                relative_path: PathBuf::from(format!("f{i}.txt")),
                name: format!("f{i}.txt"),
                state: DiffState::Identical,
                left: None,
                right: None,
            })
            .collect();
        app.apply_filter();
        app.viewport.visible_height = 5; // page_step = 4

        app.page_down();
        assert_eq!(app.selected_idx(), 4);
        assert_eq!(app.scroll_offset(), 0); // still visible within first page

        app.page_down();
        assert_eq!(app.selected_idx(), 8);
        assert_eq!(app.scroll_offset(), 4); // selection pushed view down

        app.page_up();
        assert_eq!(app.selected_idx(), 4);

        // Overshoot clamps to last row
        app.set_selected_idx(18);
        app.page_down();
        assert_eq!(app.selected_idx(), 19);

        app.page_up();
        assert_eq!(app.selected_idx(), 15);

        // Empty list is a no-op
        app.filter_mut().set_rows(Vec::new());
        app.set_selected_idx(0);
        app.page_down();
        app.page_up();
        assert_eq!(app.selected_idx(), 0);
    }

    #[test]
    fn test_diff_page_down_up() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.viewport.diff_physical_rows = 30;
        app.viewport.visible_height = 10; // page_step = 9
        app.diff_mut().set_scroll(0);

        app.diff_page_down();
        assert_eq!(app.diff().scroll(), 9);

        app.diff_page_down();
        assert_eq!(app.diff().scroll(), 18);

        // Clamp to max scroll (30 - 10 = 20)
        app.diff_page_down();
        assert_eq!(app.diff().scroll(), 20);

        app.diff_page_up();
        assert_eq!(app.diff().scroll(), 11);

        app.diff_mut().set_scroll(3);
        app.diff_page_up();
        assert_eq!(app.diff().scroll(), 0);
    }

    #[test]
    fn test_begin_scan_bumps_generation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan_generation, 0);
        assert!(!app.scan_in_progress);

        let g1 = app.begin_scan();
        assert_eq!(g1, 1);
        assert_eq!(app.scan_generation, 1);
        assert!(app.scan_in_progress);

        let g2 = app.begin_scan();
        assert_eq!(g2, 2);
        assert_eq!(app.scan_generation, 2);
    }

    #[test]
    fn test_toggle_expand() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "child".to_string(),
                relative_path: PathBuf::from("child"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();

        assert_eq!(app.flat_rows.len(), 2);

        // select root and collapse it
        app.set_selected_idx(0);
        app.toggle_expand();

        // root should now be collapsed, so only root in flat_rows
        assert_eq!(app.flat_rows.len(), 1);
        assert_eq!(app.flat_rows[0].name, "root");

        // toggle expand again
        app.toggle_expand();
        assert_eq!(app.flat_rows.len(), 2);
    }

    #[test]
    fn test_expand_collapse_selected() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "child".to_string(),
                relative_path: PathBuf::from("child"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();

        assert_eq!(app.flat_rows.len(), 2);

        // collapse root
        app.set_selected_idx(0);
        app.collapse_selected();
        assert_eq!(app.flat_rows.len(), 1);

        // expand root again
        app.expand_selected();
        assert_eq!(app.flat_rows.len(), 2);
    }

    #[test]
    fn test_adjust_scroll() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_scroll_offset(2);

        // 1. visible_height == 0 does nothing
        app.set_selected_idx(5);
        app.adjust_scroll(0);
        assert_eq!(app.scroll_offset(), 2);

        // 2. selected_idx < scroll_offset -> scroll_offset becomes selected_idx
        app.set_selected_idx(1);
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset(), 1);

        // 3. selected_idx >= scroll_offset + visible_height -> scroll_offset adjusts
        app.set_selected_idx(7);
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset(), 3);

        // 4. selected_idx within view (e.g. 5) -> scroll_offset stays same
        app.set_selected_idx(5);
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset(), 3);
    }

    #[test]
    fn test_status_message_lifecycle() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // Initially no status
        assert!(app.status_toast().is_none());

        // Set an error status
        app.set_status("Copy failed: permission denied", true);
        assert!(app.status_toast().is_some());
        let (msg, is_error) = app.status_toast().unwrap();
        assert!(is_error);
        assert!(msg.contains("permission denied"));

        // Should NOT expire with a short duration just after setting
        app.clear_expired_status(std::time::Duration::from_secs(10));
        assert!(app.status_toast().is_some());

        // Should expire with zero duration
        app.clear_expired_status(std::time::Duration::ZERO);
        assert!(app.status_toast().is_none());

        // Set a success status
        app.set_status("Copied 'file.txt'", false);
        let (_, is_error) = app.status_toast().unwrap();
        assert!(!is_error);
    }

    #[test]
    fn test_swap_paths() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        assert_eq!(app.left_path(), PathBuf::from("/left"));
        assert_eq!(app.right_path(), PathBuf::from("/right"));

        app.swap_paths();

        assert_eq!(app.left_path(), PathBuf::from("/right"));
        assert_eq!(app.right_path(), PathBuf::from("/left"));
    }

    #[test]
    fn test_swap_paths_resets_state() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_selected_idx(5);
        app.set_scroll_offset(3);
        app.diff_mut().set_scroll(2);
        app.diff_mut()
            .set_hashes(Some("abc".to_string()), Some("def".to_string()));

        app.swap_paths();

        assert_eq!(app.selected_idx(), 0);
        assert_eq!(app.scroll_offset(), 0);
        assert_eq!(app.diff().scroll(), 0);
        assert!(app.diff().left_hash().is_none());
        assert!(app.diff().right_hash().is_none());
    }

    #[test]
    fn test_swap_paths_twice_restores() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.swap_paths();
        app.swap_paths();
        assert_eq!(app.left_path(), PathBuf::from("/left"));
        assert_eq!(app.right_path(), PathBuf::from("/right"));
    }

    #[test]
    fn test_filter_by_pattern() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("alpha.txt"),
                name: "alpha.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("beta.txt"),
                name: "beta.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("gamma.txt"),
                name: "gamma.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();
        assert_eq!(app.filter().rows().len(), 3);

        // Filter by "alpha"
        app.filter_mut().set_pattern("alpha");
        app.apply_filter();
        assert_eq!(app.filter().rows().len(), 1);
        assert_eq!(app.filter().rows()[0].name, "alpha.txt");

        // Clear filter
        app.filter_mut().set_pattern("");
        app.apply_filter();
        assert_eq!(app.filter().rows().len(), 3);
    }

    #[test]
    fn test_filter_diffs_only() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("only.txt"),
                name: "only.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
            },
        ];

        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        assert_eq!(app.filter().rows().len(), 2);
        assert!(app
            .filter()
            .rows()
            .iter()
            .all(|r| r.state != DiffState::Identical));
    }

    /// Issue #232: `≈` rows are unresolved, not identical — diffs-only must keep
    /// them so switching to Precise mode from the filtered view is possible.
    #[test]
    fn test_filter_diffs_only_retains_unverified_rows() {
        use crate::diff::UnverifiedReason;

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("image.png"),
                name: "image.png".to_string(),
                state: DiffState::Unverified(UnverifiedReason::NotCompared),
                left: None,
                right: None,
            },
        ];

        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        assert_eq!(app.filter().rows().len(), 1);
        assert_eq!(app.filter().rows()[0].name, "image.png");
    }

    #[test]
    fn test_filter_pattern_and_diffs_only_combined() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_a.txt"),
                name: "diff_a.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_b.txt"),
                name: "diff_b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
            },
        ];

        // Filter by "a" + diffs only → should match "diff_a.txt" only
        app.filter_mut().set_pattern("a");
        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        assert_eq!(app.filter().rows().len(), 1);
        assert_eq!(app.filter().rows()[0].name, "diff_a.txt");
    }

    #[test]
    fn test_filter_case_insensitive() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("README.md"),
            name: "README.md".to_string(),
            state: DiffState::Identical,
            left: None,
            right: None,
        }];
        app.filter_mut().set_pattern("readme");
        app.apply_filter();
        assert_eq!(app.filter().rows().len(), 1);
    }

    #[test]
    fn test_apply_filter_preserves_selection_and_scroll() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("c.txt"),
                name: "c.txt".to_string(),
                state: DiffState::RightOnly,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();
        app.set_selected_idx(2);
        app.set_scroll_offset(1);
        app.viewport.visible_height = 10;

        // Rebuild without changing filter criteria — keep the same row selected.
        app.apply_filter();
        assert_eq!(app.selected_idx(), 2);
        assert_eq!(
            app.filter().rows()[app.selected_idx()].relative_path,
            PathBuf::from("c.txt")
        );
        assert_eq!(app.scroll_offset(), 1);
    }

    #[test]
    fn test_apply_filter_resets_when_selection_filtered_out() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();
        app.set_selected_idx(0); // same.txt
        app.set_scroll_offset(0);

        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        // same.txt is filtered out → fall back to top of remaining list
        assert_eq!(app.selected_idx(), 0);
        assert_eq!(app.filter().rows()[0].name, "diff.txt");
        assert_eq!(app.scroll_offset(), 0);
    }

    #[test]
    fn test_flatten_tree_preserves_selection() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![
                AlignedNode {
                    name: "child_a".to_string(),
                    relative_path: PathBuf::from("child_a"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    is_expanded: false,
                },
                AlignedNode {
                    name: "child_b".to_string(),
                    relative_path: PathBuf::from("child_b"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    is_expanded: false,
                },
            ],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();
        app.set_selected_idx(2); // child_b
        app.set_scroll_offset(1);
        app.viewport.visible_height = 10;

        app.flatten_tree();
        assert_eq!(app.selected_idx(), 2);
        assert_eq!(app.flat_rows[app.selected_idx()].name, "child_b");
        assert_eq!(app.scroll_offset(), 1);
    }

    #[test]
    fn test_restore_expanded_paths_after_rescan() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let old_tree = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "subdir".to_string(),
                relative_path: PathBuf::from("subdir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "file.txt".to_string(),
                    relative_path: PathBuf::from("subdir/file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    is_expanded: false,
                }],
                is_expanded: true,
            }],
            is_expanded: true,
        };
        app.root_node = Some(old_tree);
        app.flatten_tree();
        app.set_selected_idx(
            app.filter()
                .rows()
                .iter()
                .position(|r| r.relative_path == *"subdir/file.txt")
                .unwrap(),
        );

        let expanded = app.collect_expanded_paths();
        assert!(expanded.contains(&PathBuf::from("")));
        assert!(expanded.contains(&PathBuf::from("subdir")));

        // Simulate a fresh scan result (dirs start collapsed except root).
        let new_tree = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "subdir".to_string(),
                relative_path: PathBuf::from("subdir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "file.txt".to_string(),
                    relative_path: PathBuf::from("subdir/file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    is_expanded: false,
                }],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.root_node = Some(new_tree);
        app.restore_expanded_paths(&expanded);
        app.flatten_tree();

        assert!(app
            .filter()
            .rows()
            .iter()
            .any(|r| r.relative_path == *"subdir/file.txt"));
        assert_eq!(
            app.filter().rows()[app.selected_idx()].relative_path,
            PathBuf::from("subdir/file.txt")
        );
    }

    #[test]
    fn test_open_commit_cancel_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.filter_mut().set_pattern("abc");

        // FilterState::open pre-fills input with committed pattern
        app.filter_mut().open();
        assert!(app.filter().active());
        assert_eq!(app.filter().input(), "abc");

        // Type more
        for c in "def".chars() {
            app.filter_mut().input_mut().insert(c);
        }
        assert_eq!(app.filter().input(), "abcdef");

        // Cancel restores to original pattern
        app.filter_mut().cancel();
        assert!(!app.filter().active());
        assert_eq!(app.filter().input(), "abc");
        assert_eq!(app.filter().pattern(), "abc");

        // Open again and commit
        app.filter_mut().open();
        app.filter_mut().input_mut().set("xyz");
        app.commit_filter();
        assert!(!app.filter().active());
        assert_eq!(app.filter().pattern(), "xyz");
    }

    #[test]
    fn test_clear_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.filter_mut().set_pattern("a");
        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        assert_eq!(app.filter().rows().len(), 0);

        app.clear_filter();
        assert!(app.filter().pattern().is_empty());
        assert!(!app.filter().diffs_only());
        assert_eq!(app.filter().rows().len(), 2);
    }

    #[test]
    fn test_help_topic_all_returns_six_topics_in_order() {
        use HelpTopic::*;
        assert_eq!(
            HelpTopic::all(),
            [DirectoryTree, FileDiff, Config, Mouse, General, About]
        );
    }

    #[test]
    fn test_help_topic_titles_are_distinct_non_empty() {
        let titles: Vec<&str> = HelpTopic::all().iter().map(|t| t.title()).collect();
        for title in &titles {
            assert!(!title.is_empty());
        }
        let mut unique = titles.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), titles.len(), "topic titles must be distinct");
    }

    #[test]
    fn test_help_topic_for_view_maps_each_view_correctly() {
        assert_eq!(
            HelpTopic::for_view(ViewMode::DirectoryTree),
            HelpTopic::DirectoryTree
        );
        assert_eq!(HelpTopic::for_view(ViewMode::FileDiff), HelpTopic::FileDiff);
        assert_eq!(HelpTopic::for_view(ViewMode::ConfigMenu), HelpTopic::Config);
    }

    #[test]
    fn test_app_help_fields_have_expected_defaults() {
        let app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.help().topic(), HelpTopic::General);
        assert_eq!(app.help().return_view(), ViewMode::DirectoryTree);
        assert!(!app.help().index_open());
        assert_eq!(app.help().index_sel(), 0);
        assert_eq!(app.help().scroll(), 0);
    }

    #[test]
    fn test_open_help_sets_contextual_topic_and_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.help_mut().set_index_open(true); // prove open_help sets this to false
        app.help_mut().set_scroll(7); // prove open_help resets this

        app.open_help();

        assert_eq!(app.help().return_view(), ViewMode::FileDiff);
        assert_eq!(app.help().topic(), HelpTopic::FileDiff);
        assert!(!app.help().index_open());
        assert_eq!(app.help().scroll(), 0);
        assert_eq!(app.view_mode(), ViewMode::Help);
    }

    #[test]
    fn test_open_help_while_already_on_help_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_help();
        assert_eq!(app.help().return_view(), ViewMode::FileDiff);

        // Calling open_help() again while already on Help (e.g. clicking the top bar's
        // (?)Help hotspot from within Help itself) must be a no-op — otherwise
        // help_return_view would be overwritten with ViewMode::Help, trapping Esc/`?`/q in
        // Help with no keyboard way out.
        app.open_help();
        assert_eq!(app.help().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::Help);
    }

    #[test]
    fn test_open_help_index_syncs_selection_to_current_topic() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_help();
        app.help_mut().set_topic(HelpTopic::Mouse);

        app.help_mut().open_index();

        assert!(app.help().index_open());
        assert_eq!(app.help().index_sel(), 3);
    }

    #[test]
    fn test_close_help_index_stays_on_help_and_closes_index_only() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_help();
        app.help_mut().open_index();
        assert!(app.help().index_open());

        app.help_mut().close_index();

        assert!(!app.help().index_open());
        assert_eq!(
            app.view_mode(),
            ViewMode::Help,
            "unlike close_help, closing just the index must not leave Help"
        );
    }

    #[test]
    fn test_open_config_remembers_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();

        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);
    }

    #[test]
    fn test_open_config_while_already_on_config_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);

        // Calling open_config() again while already on Config (e.g. clicking the top bar's
        // (C)onfig hotspot from within Config itself) must be a no-op — otherwise
        // config().return_view() would be overwritten with ViewMode::ConfigMenu, trapping Esc/q
        // in Config with no keyboard way out.
        app.open_config();
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);
    }

    #[test]
    fn test_close_config_restores_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);

        app.close_config();
        assert_eq!(app.view_mode(), ViewMode::FileDiff);
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
    }

    #[test]
    fn test_config_rows_and_navigation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ]);

        let rows = app.config_rows();
        // Header + 2 tools + Updates header + CheckUpdates + Mouse header + Mouse
        // + Theme header + Theme + Diff View header + DiffContext
        // + Scan header + ScanMode
        assert_eq!(rows.len(), 13);
        assert!(matches!(
            rows[0],
            ConfigRowKind::Header("External Diff Tool")
        ));
        assert!(matches!(rows[1], ConfigRowKind::DiffTool(0)));
        assert!(matches!(rows[2], ConfigRowKind::DiffTool(1)));
        assert!(matches!(rows[3], ConfigRowKind::Header("Updates")));
        assert!(matches!(rows[4], ConfigRowKind::CheckUpdates));
        assert!(matches!(rows[5], ConfigRowKind::Header("Mouse")));
        assert!(matches!(rows[6], ConfigRowKind::Mouse));
        assert!(matches!(rows[7], ConfigRowKind::Header("Theme")));
        assert!(matches!(rows[8], ConfigRowKind::Theme));
        assert!(matches!(rows[9], ConfigRowKind::Header("Diff View")));
        assert!(matches!(rows[10], ConfigRowKind::DiffContext));
        assert!(matches!(rows[11], ConfigRowKind::Header("Scan")));
        assert!(matches!(rows[12], ConfigRowKind::ScanMode));

        app.config_mut().set_selected_idx(0);
        app.ensure_config_selection();
        assert_eq!(app.config().selected_idx(), 1);

        // Selectable indices: 1, 2, 4, 6, 8, 10, 12
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 2);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 4);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 6);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 8);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 10);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 12);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 1);

        app.config_select_prev();
        assert_eq!(app.config().selected_idx(), 12);
    }

    /// Issue #238: applying the Config scan-mode row persists, updates the
    /// effective mode, and asks the caller for exactly one background rescan.
    #[test]
    fn test_config_scan_mode_row_persists_and_requests_a_rescan() {
        use crate::settings::ScanMode;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.scan_mode(), ScanMode::Precise);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        assert!(
            app.apply_config_selection(),
            "a successful scan-mode change needs a rescan"
        );
        assert_eq!(app.scan_mode(), ScanMode::Fast);
        assert_eq!(
            crate::settings::AppSettings::load().scan_mode,
            ScanMode::Fast
        );
        assert!(!app.scan_mode_is_session_override());

        // Rows that do not affect scanning never ask for a rescan.
        let theme_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(theme_idx);
        assert!(!app.apply_config_selection());
    }

    /// Issue #238: if persisting fails, keep the previous runtime mode and tell
    /// the caller not to rescan, so the screen never shows results from a mode
    /// the config does not agree with.
    #[cfg(unix)]
    #[test]
    fn test_scan_mode_save_failure_keeps_the_previous_mode_and_skips_the_rescan() {
        use crate::settings::ScanMode;
        use std::os::unix::fs::PermissionsExt;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.scan_mode(), ScanMode::Precise);

        // Make the seeded config file read-only so `save()`'s truncating write fails.
        let path = crate::settings::AppSettings::config_path().unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut locked = original.clone();
        locked.set_mode(0o444);
        std::fs::set_permissions(&path, locked).unwrap();

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        let needs_rescan = app.apply_config_selection();

        // Restore before asserting so a failure cannot leave the tempdir locked.
        std::fs::set_permissions(&path, original).unwrap();

        assert!(!needs_rescan, "a failed save must not trigger a rescan");
        assert_eq!(
            app.scan_mode(),
            ScanMode::Precise,
            "the runtime mode must survive a failed save"
        );
        assert_eq!(app.saved_scan_mode(), ScanMode::Precise);
        let (msg, is_error, _) = app.status_message.clone().unwrap();
        assert!(is_error, "{msg}");
        assert!(msg.contains("Could not save scan mode"), "{msg}");
    }

    /// Issue #238: changing scan mode from Config while a File Diff session is
    /// open must not discard that session.
    #[test]
    fn test_scan_mode_change_from_file_diff_keeps_the_diff_session() {
        use crate::settings::ScanMode;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows(vec![crate::diff_view::DiffRow::from((
                Some(crate::diff_view::DiffLine {
                    tag: similar::ChangeTag::Equal,
                    text: "kept".to_string(),
                }),
                None,
            ))]);
        app.open_config();

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        assert!(app.apply_config_selection());
        assert_eq!(app.scan_mode(), ScanMode::Fast);

        app.close_config();
        assert_eq!(app.view_mode(), ViewMode::FileDiff);
        assert_eq!(app.diff().rows().len(), 1, "the diff session is preserved");
    }

    #[test]
    fn test_mouse_toggle_persists_in_settings() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // App::new() doesn't sync the session-only `mouse_enabled` flag from
        // `settings.mouse` — main.rs does that after construction.
        app.set_mouse_enabled(app.settings().mouse);
        assert!(!app.settings().mouse);
        assert!(!app.mouse_enabled());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        let _ = app.apply_config_selection();
        assert!(app.settings().mouse);
        assert!(app.mouse_enabled());

        let _ = app.apply_config_selection();
        assert!(!app.settings().mouse);
        assert!(!app.mouse_enabled());
    }

    #[test]
    fn test_theme_toggle_persists_in_settings() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Light);
        assert_eq!(app.theme(), crate::theme::Theme::LIGHT);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        let _ = app.apply_config_selection();
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Dark);
        assert_eq!(app.theme(), crate::theme::Theme::DARK);

        let _ = app.apply_config_selection();
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Light);
    }

    #[test]
    fn test_diff_context_adjust_persists_and_clamps() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings().diff_context, 7);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        app.adjust_config_selection(true);
        assert_eq!(app.settings().diff_context, 8);
        app.adjust_config_selection(false);
        app.adjust_config_selection(false);
        assert_eq!(app.settings().diff_context, 6);

        // Clamped at 0 (saturating_sub), not underflowing.
        for _ in 0..10 {
            app.adjust_config_selection(false);
        }
        assert_eq!(app.settings().diff_context, 0);

        // Clamped at 50.
        for _ in 0..60 {
            app.adjust_config_selection(true);
        }
        assert_eq!(app.settings().diff_context, 50);
    }

    #[test]
    fn test_adjust_config_selection_is_noop_for_non_numeric_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::CheckUpdates))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        let before = app.settings().diff_context;
        app.adjust_config_selection(true);
        assert_eq!(app.settings().diff_context, before);
    }

    #[test]
    fn test_apply_incremental_rescan_nested_file() {
        use std::fs::{create_dir_all, write};
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        create_dir_all(left.path().join("nested")).unwrap();
        create_dir_all(right.path().join("nested")).unwrap();
        write(left.path().join("nested/a.txt"), "left").unwrap();
        write(right.path().join("nested/a.txt"), "right-old").unwrap();
        write(left.path().join("nested/b.txt"), "only-left").unwrap();

        let root = crate::diff::align_directories(
            left.path(),
            right.path(),
            std::path::Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.root_node = Some(root);
        // Expand nested so file rows are visible after flatten.
        app.restore_expanded_paths(&[PathBuf::from(""), PathBuf::from("nested")]);
        app.flatten_tree();
        let before_len = app.flat_rows.len();

        // Simulate copy left → right of b.txt (now both sides have it).
        write(right.path().join("nested/b.txt"), "only-left").unwrap();
        app.apply_incremental_rescan(std::path::Path::new("nested/b.txt"), false)
            .expect("nested incremental rescan");

        assert!(
            app.flat_rows
                .iter()
                .any(|r| r.relative_path == *"nested/b.txt"
                    && r.left.is_some()
                    && r.right.is_some()),
            "copied file should appear on both sides after incremental rescan"
        );
        // Unrelated root structure should still be present (not empty rebuild only).
        assert!(app.flat_rows.len() >= before_len);
        assert!(app
            .root_node
            .as_ref()
            .unwrap()
            .children
            .iter()
            .any(|c| c.name == "nested"));
    }

    #[test]
    fn test_check_updates_toggle_persists_in_settings() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // App::new() doesn't sync the session-only `update_check_enabled` flag
        // from `settings.check_updates` — main.rs does that after construction.
        app.set_update_check_enabled(app.settings().check_updates);
        assert!(!app.settings().check_updates);
        assert!(!app.update_check_enabled());

        // Land on CheckUpdates row and toggle.
        app.open_config();
        while !matches!(
            app.config_rows().get(app.config().selected_idx()),
            Some(ConfigRowKind::CheckUpdates)
        ) {
            app.config_select_next();
        }
        let _ = app.apply_config_selection();
        assert!(app.settings().check_updates);
        assert!(app.update_check_enabled());

        let _ = app.apply_config_selection();
        assert!(!app.settings().check_updates);
        assert!(!app.update_check_enabled());
    }

    #[test]
    fn test_config_tests_never_touch_real_config_file() {
        // Hold the env lock while reading the *real* (unredirected) config
        // path, so a concurrently running guarded test can't be mid-redirect.
        let _lock = lock_env_tests();
        let real_path = crate::settings::AppSettings::config_search_paths()
            .into_iter()
            .next();
        let snapshot = |p: &Option<PathBuf>| {
            p.as_ref().map(|p| {
                (
                    p.exists(),
                    std::fs::metadata(p).ok().and_then(|m| m.modified().ok()),
                )
            })
        };
        let before = snapshot(&real_path);

        {
            // Exercise the real write path (settings.save() via
            // apply_config_selection) exactly like the toggle tests above,
            // but redirected to a tempdir.
            let _redirect = RedirectedConfigDir::new();
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.set_mouse_enabled(app.settings().mouse);
            let idx = app
                .config_rows()
                .iter()
                .position(|r| matches!(r, ConfigRowKind::Mouse))
                .unwrap();
            app.config_mut().set_selected_idx(idx);
            let _ = app.apply_config_selection();
            let _ = app.apply_config_selection();
        }

        let after = snapshot(&real_path);
        assert_eq!(
            before, after,
            "exercising the config save path must not modify the real config file"
        );
    }

    #[test]
    fn test_first_run_detect_does_not_require_saved_config() {
        // Auto-pick in memory is fine; the important contract is we no longer
        // force-save on construction (save failures / missing home still OK).
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // If a tool was auto-detected it lives only in the in-memory settings
        // until the user confirms via Config — load() may still return default
        // when no file exists, which is acceptable.
        let _ = app.settings().external_diff_tool;
    }

    #[test]
    fn test_focus_pane_shortcuts() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.active_side_left());

        app.focus_right_pane();
        assert!(!app.active_side_left());

        app.focus_left_pane();
        assert!(app.active_side_left());
    }

    #[test]
    fn test_toggle_active_side_flips_focus() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.active_side_left(), "starts on left");

        app.toggle_active_side();
        assert!(!app.active_side_left());

        app.toggle_active_side();
        assert!(app.active_side_left());

        // Test-only setter for fixtures that should not go through focus_* intent.
        app.set_active_side_left(false);
        assert!(!app.active_side_left());
        app.toggle_active_side();
        assert!(app.active_side_left());
    }

    #[test]
    fn test_request_quit_sets_should_quit() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.should_quit());

        app.request_quit();
        assert!(app.should_quit());
    }

    /// Issue #238: `--scan-mode` seeds the session only. It never writes the
    /// config, Config annotates the mismatch as a session override, and the
    /// first in-app change persists and clears the annotation.
    #[test]
    fn test_cli_scan_mode_overrides_the_session_without_persisting() {
        use crate::settings::ScanMode;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // The seeded config persists Precise, so that is the effective mode.
        assert_eq!(app.scan_mode(), ScanMode::Precise);
        assert!(app.precise_mode());
        assert!(!app.scan_mode_is_session_override());

        // What `--scan-mode fast` does at bootstrap.
        app.set_scan_mode(ScanMode::Fast);
        assert!(!app.precise_mode());
        assert_eq!(
            app.saved_scan_mode(),
            ScanMode::Precise,
            "the CLI value must not write the config file"
        );
        assert!(app.scan_mode_is_session_override());

        // An in-app change persists, so effective and saved agree again.
        app.apply_scan_mode(ScanMode::Fast).unwrap();
        assert_eq!(app.saved_scan_mode(), ScanMode::Fast);
        assert!(!app.scan_mode_is_session_override());
        assert_eq!(
            crate::settings::AppSettings::load().scan_mode,
            ScanMode::Fast,
            "apply_scan_mode persists before adopting the mode"
        );
    }

    #[test]
    fn test_build_palette_actions_directory_tree() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::DirectoryTree);
        let actions = app.build_palette_actions();
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::Quit));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::Help));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::Refresh));
    }

    #[test]
    fn test_build_palette_actions_file_diff() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        let actions = app.build_palette_actions();
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::ToggleWrap));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::ToggleFullDiff));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::NextChange));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::PrevChange));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::CopyHunkLeftToRight));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::CopyHunkRightToLeft));
        // Issue #239 added these to the File Diff inventory; `D` and `E` also
        // gained matching direct bindings in `input.rs`.
        for expected in [
            crate::ui::PaletteActionId::ExternalDiff,
            crate::ui::PaletteActionId::ExternalEdit,
            crate::ui::PaletteActionId::Config,
            crate::ui::PaletteActionId::ToggleTheme,
            crate::ui::PaletteActionId::Back,
        ] {
            assert!(
                actions.iter().any(|a| a.action_id == expected),
                "File Diff must list {expected:?}"
            );
        }
    }

    /// Issue #239: Config and Help list their applicable Theme / Config / Help /
    /// Back actions rather than the old two-entry fallback.
    #[test]
    fn test_build_palette_actions_config_and_help_views() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.set_view_mode(ViewMode::ConfigMenu);
        let config_ids: Vec<_> = app
            .build_palette_actions()
            .iter()
            .map(|a| a.action_id)
            .collect();
        assert_eq!(
            config_ids,
            vec![
                crate::ui::PaletteActionId::ToggleTheme,
                crate::ui::PaletteActionId::Help,
                crate::ui::PaletteActionId::Back,
            ]
        );

        app.set_view_mode(ViewMode::Help);
        let help_ids: Vec<_> = app
            .build_palette_actions()
            .iter()
            .map(|a| a.action_id)
            .collect();
        assert_eq!(
            help_ids,
            vec![
                crate::ui::PaletteActionId::ToggleTheme,
                crate::ui::PaletteActionId::Config,
                crate::ui::PaletteActionId::Back,
            ]
        );

        // Every action listed in these views is runnable.
        assert!(app.build_palette_actions().iter().all(|a| a.enabled()));
    }

    #[test]
    fn test_copy_hunk_at_cursor_updates_target_file() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        write(right_dir.path().join("merge.txt"), "keep\nright-line\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.flat_rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_scroll(1);

        app.copy_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should succeed");

        let right_text = read_to_string(right_dir.path().join("merge.txt")).unwrap();
        assert!(right_text.contains("left-line"));
        assert!(!right_text.contains("right-line"));
    }

    #[test]
    fn test_jump_to_next_and_prev_change() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.viewport.diff_content_width = 40;
        app.diff_mut().set_rows(vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "ctx".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "ctx".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "bye".to_string(),
                }),
                None,
            )),
        ]);

        app.jump_to_next_change();
        assert_eq!(app.diff().scroll(), 1);
        app.jump_to_next_change();
        assert_eq!(app.diff().scroll(), 2);
        app.jump_to_prev_change();
        assert_eq!(app.diff().scroll(), 1);
    }

    fn flat_row(name: &str) -> FlatRow {
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from(name),
            name: name.to_string(),
            state: DiffState::Identical,
            left: None,
            right: None,
        }
    }

    fn dir_node(name: &str) -> AlignedNode {
        AlignedNode {
            name: name.to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            is_expanded: true,
        }
    }

    fn equal_row(text: &str) -> crate::diff_view::DiffRow {
        crate::diff_view::DiffRow::from((
            Some(crate::diff_view::DiffLine {
                tag: similar::ChangeTag::Equal,
                text: text.to_string(),
            }),
            Some(crate::diff_view::DiffLine {
                tag: similar::ChangeTag::Equal,
                text: text.to_string(),
            }),
        ))
    }

    fn deleted_row(text: &str) -> crate::diff_view::DiffRow {
        crate::diff_view::DiffRow::from((
            Some(crate::diff_view::DiffLine {
                tag: similar::ChangeTag::Delete,
                text: text.to_string(),
            }),
            None,
        ))
    }

    #[test]
    fn test_diff_rows_accessor_reflects_set_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.diff().rows().is_empty());

        let rows = vec![equal_row("a"), equal_row("b")];
        app.diff_mut().set_rows(rows.clone());

        assert_eq!(app.diff().rows(), rows.as_slice());
    }

    #[test]
    fn test_filter_rows_accessor_reflects_set_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.filter().rows().is_empty());

        let rows = vec![flat_row("a.txt"), flat_row("b.txt")];
        app.filter_mut().set_rows(rows.clone());

        assert_eq!(app.filter().rows().len(), 2);
        assert_eq!(app.filter().rows()[0].name, "a.txt");
        assert_eq!(app.filter().rows()[1].name, "b.txt");
    }

    #[test]
    fn test_selected_row_none_when_empty_or_out_of_range() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.selected_row().is_none());

        app.filter_mut().set_rows(vec![flat_row("a.txt")]);
        app.set_selected_idx(0);
        assert_eq!(app.selected_row().map(|r| r.name.as_str()), Some("a.txt"));

        app.set_selected_idx(1);
        assert!(app.selected_row().is_none());
    }

    /// Issue #239: every launcher opens the same surface, and every open clears
    /// the query and lands on the first enabled action.
    #[test]
    fn test_open_palette_clears_the_query_and_selects_the_first_enabled_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.palette_visible());

        app.open_palette();
        assert!(app.palette_visible());
        assert!(app.palette().query.is_empty());
        // With no row selected the first few Directory Tree actions are gated,
        // so the selection must skip past them.
        let selected = app.palette().items[app.palette().selected_idx].clone();
        assert!(selected.enabled(), "{selected:?}");
        assert!(
            app.palette().items[..app.palette().selected_idx]
                .iter()
                .all(|a| !a.enabled()),
            "the first enabled action wins"
        );

        app.palette_type_char('x');
        assert_eq!(app.palette().query, "x");
        app.close_palette();
        assert!(!app.palette_visible());
        assert!(app.palette().query.is_empty(), "close clears query");

        app.open_palette();
        assert!(app.palette().query.is_empty(), "open clears query");
    }

    #[test]
    fn test_palette_select_next_prev_wraps() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        app.set_palette_items(vec![
            crate::ui::PaletteAction::new("a", "A", crate::ui::PaletteActionId::Help),
            crate::ui::PaletteAction::new("b", "B", crate::ui::PaletteActionId::Quit),
        ]);
        app.set_palette_selected_idx(0);

        app.palette_select_next();
        assert_eq!(app.palette().selected_idx, 1);
        app.palette_select_next();
        assert_eq!(app.palette().selected_idx, 0, "wraps around");
        app.palette_select_prev();
        assert_eq!(app.palette().selected_idx, 1, "wraps backward");
    }

    /// Issue #239: case-insensitive substring search, not fuzzy matching.
    #[test]
    fn test_palette_search_is_case_insensitive_substring() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::DirectoryTree);
        app.open_palette();
        for c in "QUIT".chars() {
            app.palette_type_char(c);
        }

        assert!(
            app.palette()
                .items
                .iter()
                .any(|a| a.action_id == crate::ui::PaletteActionId::Quit),
            "an upper-case query must still match the lower-case label"
        );
        assert!(
            app.palette()
                .items
                .iter()
                .all(|a| a.label.to_lowercase().contains("quit")
                    || a.key.to_lowercase().contains("quit")),
            "every remaining item must match the query"
        );

        // A query that matches nothing empties the list; the popup renders its
        // own non-selectable notice rather than a stale selection.
        for c in "zzz".chars() {
            app.palette_type_char(c);
        }
        assert!(app.palette().items.is_empty());

        // Fuzzy subsequence matching is explicitly not what this does.
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        assert_eq!(app.palette().query, "");
        for c in "qit".chars() {
            app.palette_type_char(c);
        }
        assert!(
            app.palette()
                .items
                .iter()
                .all(|a| a.action_id != crate::ui::PaletteActionId::Quit),
            "\"qit\" is a subsequence of \"quit\" but not a substring"
        );
    }

    /// Issue #239: unavailable actions stay listed with the reason they cannot run.
    #[test]
    fn test_palette_keeps_unavailable_actions_visible_with_a_reason() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::DirectoryTree);
        let actions = app.build_palette_actions();

        let diff = actions
            .iter()
            .find(|a| a.action_id == crate::ui::PaletteActionId::BuiltinDiff)
            .expect("the built-in diff action stays listed with no row selected");
        assert!(!diff.enabled());
        assert_eq!(diff.disabled_reason, Some("no row is selected"));

        // Every view lists Back or Quit, Help or Config, and Theme.
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::ToggleTheme));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::ToggleFocus));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::FocusLeft));
        assert!(actions
            .iter()
            .any(|a| a.action_id == crate::ui::PaletteActionId::ExpandSelected));
    }

    /// Issue #239: a selection past the bottom of a long inventory scrolls into view.
    #[test]
    fn test_sync_palette_viewport_keeps_the_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        app.set_palette_items(
            (0..20)
                .map(|i| {
                    crate::ui::PaletteAction::new(
                        &i.to_string(),
                        &format!("Action {i}"),
                        crate::ui::PaletteActionId::Help,
                    )
                })
                .collect(),
        );

        app.set_palette_selected_idx(0);
        app.sync_palette_viewport(8);
        assert_eq!(app.palette().scroll_offset, 0);

        // The ninth item is the first that does not fit an 8-row viewport.
        app.set_palette_selected_idx(8);
        app.sync_palette_viewport(8);
        assert_eq!(app.palette().scroll_offset, 1, "scrolls just far enough");

        app.set_palette_selected_idx(19);
        app.sync_palette_viewport(8);
        assert_eq!(app.palette().scroll_offset, 12);

        app.set_palette_selected_idx(0);
        app.sync_palette_viewport(8);
        assert_eq!(app.palette().scroll_offset, 0, "scrolls back up");
    }

    #[test]
    fn test_diff_has_changes_false_when_all_rows_equal() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.diff_mut()
            .set_rows(vec![equal_row("a"), equal_row("b")]);

        assert!(!app.diff().has_changes());
    }

    #[test]
    fn test_diff_has_changes_true_when_a_row_differs() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.diff_mut()
            .set_rows(vec![equal_row("a"), deleted_row("b")]);

        assert!(app.diff().has_changes());
    }

    #[test]
    fn test_sync_viewport_tree_derives_visible_height() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // 24 rows = 1 top bar + 22 body + 1 footer; the pane's two borders are
        // not content.
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.viewport().visible_height, 20);

        // A status toast grows the footer by one row, shrinking the body.
        app.set_status("copied", false);
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.viewport().visible_height, 19);
    }

    #[test]
    fn test_sync_viewport_tree_keeps_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.flat_rows = (0..40).map(|i| flat_row(&format!("f{i}.txt"))).collect();
        app.apply_filter();
        app.set_selected_idx(30);

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.viewport().visible_height, 20);
        assert_eq!(app.scroll_offset(), 11, "selection scrolled into view");
    }

    #[test]
    fn test_sync_viewport_diff_derives_geometry_from_area() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        // 24 rows = 1 header + 1 info bar + 21 body + 1 footer; 80 columns split
        // in half leaves 38 content columns per pane.
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        let viewport = app.viewport();
        assert_eq!(viewport.visible_height, 19);
        assert_eq!(viewport.diff_content_width, 38);
        assert_eq!(viewport.diff_max_line_width, 100);
        assert_eq!(
            viewport.diff_physical_rows, 1,
            "no wrapping: one logical row is one physical row"
        );
    }

    #[test]
    fn test_sync_viewport_diff_counts_wrapped_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows(vec![equal_row(&"a".repeat(100)), equal_row("short")]);
        app.diff_mut().set_wrap(true);

        // 100 chars over 38-column panes wraps to 3 rows, plus 1 for "short".
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.viewport().diff_physical_rows, 4);

        // Halving the width re-wraps: 100 chars over 18 columns is 6 rows.
        app.sync_viewport(Rect::new(0, 0, 40, 24));
        let viewport = app.viewport();
        assert_eq!(viewport.diff_content_width, 18);
        assert_eq!(viewport.diff_physical_rows, 7);
    }

    #[test]
    fn test_sync_viewport_after_resize_clamps_diff_paging() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows((0..40).map(|i| equal_row(&format!("line {i}"))).collect());

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        app.diff_page_down();
        assert_eq!(app.diff().scroll(), 18, "page step is visible_height - 1");

        // Growing the terminal shows more rows at once, so the bottom of the
        // document now sits at a smaller scroll offset. The sync itself must pull
        // the current position back inside the new geometry — otherwise the next
        // page-down would appear to scroll backwards.
        app.sync_viewport(Rect::new(0, 0, 80, 40));
        assert_eq!(app.viewport().visible_height, 35);
        assert_eq!(app.diff().scroll(), 5, "clamped to 40 rows - 35 visible");
        app.diff_page_down();
        assert_eq!(app.diff().scroll(), 5, "already at the bottom, stays put");
    }

    #[test]
    fn test_sync_viewport_clamps_horizontal_scroll_to_longest_line() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        let max_h_scroll = app.viewport().max_diff_h_scroll();
        app.diff_mut().set_h_scroll(max_h_scroll);
        assert_eq!(app.diff().h_scroll(), 62, "100 chars less the 38 on screen");

        // Opening a shorter file must not leave the pane scrolled past its end.
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(50))]);
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff().h_scroll(), 12);
    }

    #[test]
    fn test_sync_viewport_ignores_help_and_config_views() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        let tree_viewport = app.viewport();

        app.open_help();
        app.sync_viewport(Rect::new(0, 0, 120, 60));
        assert_eq!(
            app.viewport(),
            tree_viewport,
            "Help scrolls by its own drawn lines and must not disturb list geometry"
        );
    }

    #[test]
    fn test_apply_scan_result_updates_tree_flag_and_rows_together() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let generation = app.begin_scan();
        assert!(app.scan_in_progress());

        assert!(app.apply_scan_result(generation, dir_node("root")));

        assert!(!app.scan_in_progress(), "scan is no longer in flight");
        assert_eq!(app.flat_rows().len(), 1);
        assert_eq!(app.flat_rows()[0].name, "root");
        assert_eq!(app.filter().rows().len(), 1, "filter view rebuilt too");
    }

    #[test]
    fn test_apply_scan_result_ignores_stale_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.begin_scan();
        app.apply_scan_result(stale, dir_node("first"));
        app.begin_scan();

        assert!(!app.apply_scan_result(stale, dir_node("stale")));

        assert_eq!(app.flat_rows()[0].name, "first", "tree left untouched");
        assert!(app.scan_in_progress(), "still waiting for the newer scan");
    }

    #[test]
    fn test_apply_scan_result_restores_expanded_directories() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let mut node = dir_node("root");
        node.children.push(AlignedNode {
            name: "sub".to_string(),
            relative_path: PathBuf::from("sub"),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "leaf.txt".to_string(),
                relative_path: PathBuf::from("sub/leaf.txt"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 1,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        });

        let generation = app.begin_scan();
        app.apply_scan_result(generation, node.clone());
        assert_eq!(app.flat_rows().len(), 3);

        // A rescan returns the subdirectory collapsed; the expand state the user
        // had must survive.
        let mut collapsed = node;
        collapsed.children[0].is_expanded = false;
        let generation = app.begin_scan();
        app.apply_scan_result(generation, collapsed);
        assert_eq!(app.flat_rows().len(), 3, "sub stayed expanded");
    }

    #[test]
    fn test_fail_scan_clears_flag_only_for_current_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.begin_scan();
        app.begin_scan();

        assert!(!app.fail_scan(stale));
        assert!(app.scan_in_progress(), "stale failure changes nothing");

        let current = app.scan_generation;
        assert!(app.fail_scan(current));
        assert!(!app.scan_in_progress());
    }

    #[test]
    fn test_apply_update_check_outcome_updates_hint_state_per_outcome() {
        // Newer/UpToDate persist throttle state under the real cache path; restore it
        // so the suite does not rewrite the developer's update-check throttle.
        let prior = crate::upgrade::state_path()
            .ok()
            .map(|path| (path.clone(), crate::upgrade::load_state(&path)));

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Newer(
            "0.9.0".to_string(),
        ));
        assert_eq!(app.update_available(), Some("0.9.0"));

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::UpToDate);
        assert_eq!(app.update_available(), None);

        app.set_update_available(Some("0.7.0".to_string()));
        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Failed);
        assert_eq!(
            app.update_available(),
            Some("0.7.0"),
            "Failed must stay silent and leave the previous hint alone"
        );

        if let Some((path, state)) = prior {
            crate::upgrade::save_state(&path, &state);
        }
    }

    #[test]
    fn test_request_confirm_opens_modal_with_message_and_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.confirm_modal().is_none());

        app.request_confirm(
            "Copy foo.txt to right side?",
            ConfirmAction::CopyLeftToRight,
        );

        let modal = app.confirm_modal().expect("modal should be open");
        assert_eq!(modal.message, "Copy foo.txt to right side?");
        assert_eq!(modal.action, ConfirmAction::CopyLeftToRight);
    }

    #[test]
    fn test_request_copy_left_to_right_opens_modal_when_left_present() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_flat_rows(vec![{
            let mut row = flat_row_with_sides(Some(file_info(false)), None);
            row.name = "foo.txt".to_string();
            row
        }]);
        app.apply_filter();
        app.set_selected_idx(0);

        app.request_copy(ConfirmAction::CopyLeftToRight);

        let modal = app.confirm_modal().expect("modal should be open");
        assert_eq!(modal.message, "Copy 'foo.txt' to right side?");
        assert_eq!(modal.action, ConfirmAction::CopyLeftToRight);
    }

    #[test]
    fn test_request_copy_right_to_left_opens_modal_when_right_present() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_flat_rows(vec![{
            let mut row = flat_row_with_sides(None, Some(file_info(false)));
            row.name = "bar.txt".to_string();
            row
        }]);
        app.apply_filter();
        app.set_selected_idx(0);

        app.request_copy(ConfirmAction::CopyRightToLeft);

        let modal = app.confirm_modal().expect("modal should be open");
        assert_eq!(modal.message, "Copy 'bar.txt' to left side?");
        assert_eq!(modal.action, ConfirmAction::CopyRightToLeft);
    }

    #[test]
    fn test_request_copy_is_a_noop_when_the_source_side_is_missing() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_flat_rows(vec![flat_row_with_sides(None, Some(file_info(false)))]);
        app.apply_filter();
        app.set_selected_idx(0);

        // Right-only row: copying left-to-right has nothing to copy from.
        app.request_copy(ConfirmAction::CopyLeftToRight);

        assert!(app.confirm_modal().is_none());
    }

    #[test]
    fn test_request_copy_is_a_noop_when_nothing_is_selected() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.request_copy(ConfirmAction::CopyLeftToRight);

        assert!(app.confirm_modal().is_none());
    }

    #[test]
    fn test_take_confirmed_action_closes_modal_and_returns_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_confirm("Copy foo.txt to left side?", ConfirmAction::CopyRightToLeft);

        let action = app.take_confirmed_action();

        assert_eq!(action, Some(ConfirmAction::CopyRightToLeft));
        assert!(app.confirm_modal().is_none(), "modal closes after taking");
    }

    #[test]
    fn test_take_confirmed_action_returns_none_when_no_modal_open() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.take_confirmed_action(), None);
    }

    #[test]
    fn test_dismiss_confirm_closes_modal_and_discards_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_confirm(
            "Copy foo.txt to right side?",
            ConfirmAction::CopyLeftToRight,
        );

        app.dismiss_confirm();

        assert!(app.confirm_modal().is_none());
    }

    /// Issue #236: the diffs-only toggle is drafted like the typed query — the
    /// badge updates immediately, but only Enter commits it, and Esc restores
    /// the value from before the editing session.
    #[test]
    fn test_diffs_only_is_drafted_until_commit_and_restored_on_cancel() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.filter().diffs_only());

        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        assert!(
            app.filter().editing_diffs_only(),
            "the badge follows the draft straight away"
        );
        assert!(
            !app.filter().diffs_only(),
            "but the committed flag is untouched until Enter"
        );

        app.commit_filter();
        assert!(app.filter().diffs_only());
        assert!(app.filter().editing_diffs_only());

        // Toggling it back off and cancelling restores the committed value.
        app.filter_mut().open();
        app.filter_mut().toggle_diffs_only();
        assert!(!app.filter().editing_diffs_only());
        app.filter_mut().cancel();
        assert!(app.filter().diffs_only());
        assert!(app.filter().editing_diffs_only());
    }

    #[test]
    fn test_filter_input_mut_allows_key_by_key_editing() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.filter_mut().input_mut().insert('a');
        app.filter_mut().input_mut().insert('b');

        assert_eq!(app.filter().input(), "ab");
    }
}
