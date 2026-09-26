use crate::diff::{AlignedNode, DiffState, FileInfo};
use crate::ignore::IgnoreMatcher;
#[cfg(test)]
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub struct FlatRow {
    pub depth: usize,
    pub relative_path: PathBuf,
    pub name: String,
    pub left_name_raw: Option<String>,
    pub right_name_raw: Option<String>,
    pub left_relative_path_raw: Option<PathBuf>,
    pub right_relative_path_raw: Option<PathBuf>,
    pub state: DiffState,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
    /// Expand state of the underlying node, mirrored so the renderer can draw
    /// the directory disclosure marker without walking the tree.
    pub is_expanded: bool,
    pub has_case_conflict: bool,
    pub contains_case_conflict: bool,
    pub is_ambiguous_case_collision: bool,
}

impl Default for FlatRow {
    fn default() -> Self {
        Self {
            depth: 0,
            relative_path: PathBuf::new(),
            name: String::new(),
            left_name_raw: None,
            right_name_raw: None,
            left_relative_path_raw: None,
            right_relative_path_raw: None,
            state: DiffState::Identical,
            left: None,
            right: None,
            is_expanded: false,
            has_case_conflict: false,
            contains_case_conflict: false,
            is_ambiguous_case_collision: false,
        }
    }
}

impl FlatRow {
    /// The `left` (or right) side's entry, when that side has one.
    pub(crate) fn side(&self, left: bool) -> &Option<FileInfo> {
        if left {
            &self.left
        } else {
            &self.right
        }
    }

    /// Whether either side of this row is a directory.
    pub(crate) fn is_dir(&self) -> bool {
        self.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || self.right.as_ref().map(|f| f.is_dir).unwrap_or(false)
    }

    /// Return the left-specific relative path, falling back to the canonical path.
    pub fn left_relative_path(&self) -> &Path {
        self.left_relative_path_raw
            .as_deref()
            .unwrap_or(&self.relative_path)
    }

    /// Return the right-specific relative path, falling back to the canonical path.
    pub fn right_relative_path(&self) -> &Path {
        self.right_relative_path_raw
            .as_deref()
            .unwrap_or(&self.relative_path)
    }

    /// Return the left-specific basename, falling back to the canonical name.
    pub fn left_name(&self) -> &str {
        self.left_name_raw.as_deref().unwrap_or(&self.name)
    }

    /// Return the right-specific basename, falling back to the canonical name.
    pub fn right_name(&self) -> &str {
        self.right_name_raw.as_deref().unwrap_or(&self.name)
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
enum ExclusionEditorAction {
    None,
    Apply,
    Cancel,
}

impl ExclusionEditorState {
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

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> ExclusionEditorAction {
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

#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    CopyLeftToRight,
    CopyRightToLeft,
    /// Write every dirty working buffer, keeping the File Diff session open.
    SaveStaged,
    /// Write every dirty working buffer, then return to the Directory Tree —
    /// only if the write succeeds.
    SaveStagedThenLeave,
    /// Throw the staged edits away and return to the Directory Tree.
    DiscardStagedThenLeave,
    /// Re-read both sides from disk, discarding the staged edits. The only way
    /// forward when the files changed underneath the session.
    ReloadDiscardStaged,
    /// Close the dialog and do nothing.
    Cancel,
}

/// The Compared pair (`CONTEXT.md`): the two sides File Diff shows and the
/// copy, external-diff, and editor Commands act on. A file-pair session's pair,
/// or the selected Directory Tree row under each root. The one place that tells
/// the two apart, so every gate and effect asks it the same question
/// (ADR-0004).
#[derive(Clone, Copy, Debug)]
pub(crate) enum ComparedPair<'a> {
    Files(&'a crate::target::FilePair),
    Row {
        row: &'a FlatRow,
        left_root: &'a Path,
        right_root: &'a Path,
    },
}

impl ComparedPair<'_> {
    /// The two files to read, write, and hand to external tools: a file-pair
    /// side's target (a symlink resolved, the null device under the platform's
    /// name), or the row's entry under each root.
    pub(crate) fn paths(&self) -> (PathBuf, PathBuf) {
        match self {
            Self::Files(pair) => (
                pair.left.target_path().to_path_buf(),
                pair.right.target_path().to_path_buf(),
            ),
            Self::Row {
                row,
                left_root,
                right_root,
            } => (
                left_root.join(row.left_relative_path()),
                right_root.join(row.right_relative_path()),
            ),
        }
    }

    /// Whether the `left` (or right) side is a file — not a directory, not
    /// nothing, not a pipe or the null device. What an editor can open.
    pub(crate) fn has_file(&self, left: bool) -> bool {
        match self {
            Self::Files(pair) => pair.side(left).is_regular_file(),
            Self::Row { row, .. } => row.side(left).as_ref().is_some_and(|file| !file.is_dir),
        }
    }

    /// Whether the `left` (or right) side has anything to copy: the null
    /// device, like a row's absent side, has nothing.
    pub(crate) fn has_content(&self, left: bool) -> bool {
        match self {
            Self::Files(pair) => !pair.side(left).is_null_device(),
            Self::Row { row, .. } => row.side(left).is_some(),
        }
    }

    /// Whether the `left` (or right) side can be written. A row's side always
    /// can; a file-pair side only when its file opened for writing.
    pub(crate) fn is_writable(&self, left: bool) -> bool {
        match self {
            Self::Files(pair) => pair.side(left).is_writable(),
            Self::Row { .. } => true,
        }
    }

    /// Whether both sides can be read again from disk, as an external tool
    /// needs. A side captured from a pipe cannot.
    pub(crate) fn can_reopen(&self) -> bool {
        match self {
            Self::Files(pair) => pair.left.can_reopen() && pair.right.can_reopen(),
            Self::Row { .. } => true,
        }
    }

    /// Whether both sides are present files, as an external diff needs.
    pub(crate) fn are_files(&self) -> bool {
        match self {
            Self::Files(_) => true,
            Self::Row { row, .. } => !row.is_dir() && row.left.is_some() && row.right.is_some(),
        }
    }

    /// Whether the pair is an ambiguous case collision, which nothing may copy.
    pub(crate) fn is_ambiguous(&self) -> bool {
        match self {
            Self::Files(_) => false,
            Self::Row { row, .. } => row.is_ambiguous_case_collision,
        }
    }

    /// What a pending confirmation applies to, so an answer is refused when the
    /// selection moved underneath it. A file pair never moves.
    pub(crate) fn subject(&self) -> PathBuf {
        match self {
            Self::Files(pair) => pair.left.path().to_path_buf(),
            Self::Row { row, .. } => row.relative_path.clone(),
        }
    }
}

/// Which way a copy runs. Two-valued, unlike [`ConfirmAction`], so a copy
/// preview has no impossible direction to reject.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyDirection {
    LeftToRight,
    RightToLeft,
}

impl CopyDirection {
    /// The action a confirmed copy in this direction runs.
    pub(crate) fn confirmed(self) -> ConfirmAction {
        match self {
            Self::LeftToRight => ConfirmAction::CopyLeftToRight,
            Self::RightToLeft => ConfirmAction::CopyRightToLeft,
        }
    }
}

/// What a copy would do to its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyKind {
    /// Nothing is there yet.
    Create,
    /// Something is there and will be replaced.
    Overwrite,
    /// Both sides are directories; listed entries land in the existing one.
    Merge,
}

/// The facts a copy confirmation is built from, with no wording attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPreview {
    pub kind: CopyKind,
    pub source_name: String,
    pub destination_name: String,
    /// Absolute, lexically normalized; not canonicalized (Issue #235).
    pub source: PathBuf,
    pub destination: PathBuf,
    /// The two sides spell the name differently; the destination keeps its own.
    pub case_mismatch: bool,
}

/// Why [`App::plan_copy`] refuses. Facts only; `commands` words them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRefusal {
    /// No Compared pair: nothing is selected.
    NoSelection,
    AmbiguousCaseCollision,
    /// The source side has nothing: absent, or the null device.
    NothingToCopy,
    /// The destination side cannot be written.
    ReadOnly,
    /// The File Diff holds staged edits that a whole-file copy would discard.
    StagedChangesUnsaved,
    AlreadyIdentical,
}

/// A copy whose preconditions hold: what it copies and where. Built without
/// touching the filesystem, so the Palette's gate can ask for it every frame;
/// the confirmation shows [`App::copy_preview`] of it, and the effect runs
/// exactly it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPlan {
    pub direction: CopyDirection,
    pub target: CopyTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyTarget {
    /// Replace one side of the file pair with the other.
    FilePair,
    /// Copy the selected row's entry from one root to the other.
    Entry {
        relative_path: PathBuf,
        source_name: String,
        destination_name: String,
        source: PathBuf,
        destination: PathBuf,
        /// The destination side's root, which a copy may not escape.
        destination_root: PathBuf,
        /// The two sides spell the name differently; the destination keeps its own.
        case_mismatch: bool,
        source_is_dir: bool,
    },
}

/// An external diff whose preconditions hold: the tool and the two files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffPlan {
    pub tool: crate::diff_tool::ExternalDiffTool,
    pub left: PathBuf,
    pub right: PathBuf,
}

/// Why [`App::plan_external_diff`] refuses. Facts only; `commands` words them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffRefusal {
    ReadFromPipe,
    NotBothFiles,
    Disabled,
    /// Auto, and none of the supported tools was found at startup.
    NoTool,
    /// A pinned or unknown tool that was not found at startup.
    ToolMissing,
}

/// The result of writing the staged sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagedSave {
    Written,
    /// Nothing was written: these absolute paths changed on disk since the diff
    /// was opened.
    Conflicted(Vec<PathBuf>),
}

/// A pending confirmation prompt: the message to show and the action to run if accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmModal {
    pub title: String,
    /// The one sentence that states what `Enter` will do. Rendered emphasized,
    /// above the body, so the consequence is legible before the detail is read.
    pub headline: String,
    /// Supporting body lines, already assembled. Rendering wraps them to the
    /// popup width so the dialog stays usable on a narrow terminal (Issue #235).
    pub lines: Vec<String>,
    /// Offered choices, most affirmative first. `Enter` picks the first; `Esc`
    /// picks whichever choice carries [`ConfirmAction::Cancel`].
    pub choices: Vec<ConfirmChoice>,
}

/// One button in a [`ConfirmModal`].
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmChoice {
    /// The letter that selects it, matched case-insensitively.
    pub key: char,
    pub label: String,
    pub action: ConfirmAction,
}

impl ConfirmModal {
    /// The action a typed character selects, if any.
    pub fn action_for_key(&self, typed: char) -> Option<ConfirmAction> {
        let typed = typed.to_ascii_lowercase();
        self.choices
            .iter()
            .find(|c| c.key.to_ascii_lowercase() == typed)
            .map(|c| c.action.clone())
    }

    /// The action `Enter` selects: the first, most affirmative choice.
    pub fn default_action(&self) -> Option<ConfirmAction> {
        self.choices.first().map(|c| c.action.clone())
    }

    /// The action `Esc` selects: the cancel choice, when the dialog offers one.
    pub fn cancel_action(&self) -> Option<ConfirmAction> {
        self.choices
            .iter()
            .find(|c| c.action == ConfirmAction::Cancel)
            .map(|c| c.action.clone())
    }
}

/// The one Command Palette. `;`, `Ctrl+p`, and right-click all open this same
/// contextual surface — the former Menu / Command split is gone, along with its
/// single-character immediate execution and the `c`/`C` accelerator ambiguity
/// (Issue #239).
#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    visible: bool,
    query: String,
    items: Vec<crate::commands::CommandEntry>,
    selected_idx: usize,
    /// First item row painted in the list viewport, so a selection past the
    /// bottom of a long inventory stays visible.
    scroll_offset: usize,
}

impl PaletteState {
    pub(crate) fn visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn items(&self) -> &[crate::commands::CommandEntry] {
        &self.items
    }

    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Show the palette with an empty query. The caller fills the inventory,
    /// which only [`App`] can compute — see [`App::open_palette`].
    pub(crate) fn open(&mut self) {
        self.visible = true;
        self.query.clear();
    }

    /// Dismiss the palette: hidden, query cleared.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Replace the inventory, keeping the selection inside it.
    pub(crate) fn set_items(&mut self, items: Vec<crate::commands::CommandEntry>) {
        self.items = items;
        if self.selected_idx >= self.items.len() {
            self.selected_idx = self.items.len().saturating_sub(1);
        }
    }

    /// Wrap-around next over the inventory (no-op when empty).
    pub(crate) fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % self.items.len();
    }

    /// Wrap-around previous over the inventory (no-op when empty).
    pub(crate) fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected_idx = self
            .selected_idx
            .checked_sub(1)
            .unwrap_or(self.items.len() - 1);
    }

    /// Move the selection to the first enabled Command, or to `0` when the
    /// inventory is empty or entirely unavailable.
    pub(crate) fn select_first_enabled(&mut self) {
        self.selected_idx = self.items.iter().position(|a| a.enabled()).unwrap_or(0);
        self.scroll_offset = 0;
    }

    /// Keep the selection inside a `visible_rows`-tall list viewport. Called
    /// once per frame from the render shell, which is the only place that knows
    /// the popup's clamped height.
    pub(crate) fn sync_viewport(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        let max_offset = self.items.len().saturating_sub(visible_rows);
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_idx + 1 - visible_rows;
        }
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Append one character to the query. Re-filtering is the caller's job:
    /// only [`App`] can rebuild the inventory — see [`App::palette_type_char`].
    pub(crate) fn push_query_char(&mut self, c: char) {
        self.query.push(c);
    }

    /// Drop the query's trailing character. Re-filtering is the caller's job.
    pub(crate) fn pop_query_char(&mut self) {
        self.query.pop();
    }

    // Test-only field setter, same role as `DirectoryTreeState`'s. Clippy's dead-code
    // pass flags it as unreachable outside `#[cfg(test)]` call sites, so it
    // needs an explicit `#[allow]` (ADR-0002).
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }
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

/// The Directory Tree: the two roots' aligned tree, the expand state the user
/// chose, the rows it flattens to, the filter over those rows, and the cursor
/// into what is listed. Owned by [`App::directory_tree`] /
/// [`App::directory_tree_mut`].
///
/// Every mutating method leaves the tree, the rows, and the cursor consistent
/// before it returns, so no caller has to reflatten or refilter (ADR-0005).
/// The scan that produces the tree is [`ScanState`]'s; this type only adopts
/// its result.
#[derive(Clone, Debug, Default)]
pub struct DirectoryTreeState {
    root_node: Option<AlignedNode>,
    /// Cached leaf-pair inventory for the tree footer (Issue #252).
    /// Recomputed whenever the tree changes, not while drawing.
    tree_summary: Option<crate::diff::TreeSummary>,
    /// Every directory's expand state, keyed by relative path — the user's
    /// choice, kept apart from the scan's output so a rescan cannot lose it.
    /// Holds exactly the directories below the root of the current tree.
    expanded: HashMap<PathBuf, bool>,
    /// Rows a test lists without building a tree (ADR-0002); production
    /// always lists from the tree.
    #[cfg(test)]
    seed_rows: Vec<FlatRow>,
    active: bool,
    input: crate::text_input::TextInput,
    pattern: String,
    /// Committed diffs-only flag — the one the listed rows apply.
    diffs_only: bool,
    /// The editing session's diffs-only value. Mirrors the typed text: it only
    /// updates the badge until Enter commits both together, and Esc restores it
    /// alongside the query (Issue #236).
    draft_diffs_only: bool,
    /// The rows the user sees: the tree, flattened through the expand state or
    /// walked whole through the filter. The only copy of them.
    rows: Vec<FlatRow>,
    /// Cursor into `rows` (Issue #309).
    selected_idx: usize,
    /// First row painted in the list viewport.
    scroll_offset: usize,
    /// Content rows the last frame showed, set by [`crate::view::prepare_frame`].
    visible_height: usize,
    last_click_idx: Option<usize>,
    last_click_time: Option<std::time::Instant>,
}

impl DirectoryTreeState {
    /// The directory-tree selection cursor.
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    /// The list's vertical scroll offset.
    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// The row under the cursor, if any.
    pub(crate) fn selected_row(&self) -> Option<&FlatRow> {
        self.rows.get(self.selected_idx)
    }

    /// Put the cursor back at the top, as a swap or an emptied list does.
    pub(crate) fn reset_cursor(&mut self) {
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn select_next(&mut self) {
        if !self.rows.is_empty() && self.selected_idx < self.rows.len() - 1 {
            self.selected_idx += 1;
        }
    }

    pub(crate) fn select_prev(&mut self) {
        if !self.rows.is_empty() && self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    /// Select row `idx` if in range. Used by mouse left/right click. Does not
    /// change scroll by itself (matches the mouse path; the frame and keyboard
    /// page paths still call [`DirectoryTreeState::adjust_scroll`]).
    pub(crate) fn select_row_at(&mut self, idx: usize) -> bool {
        if idx >= self.rows.len() {
            return false;
        }
        self.selected_idx = idx;
        true
    }

    /// Page size for `Ctrl+f` / `Ctrl+b`: the last drawn height, with a
    /// one-row overlap when possible so context isn't completely lost.
    fn page_step(&self) -> usize {
        self.visible_height.saturating_sub(1).max(1)
    }

    /// Move the cursor down by one page, then keep it on screen.
    pub(crate) fn page_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let max_idx = self.rows.len() - 1;
        self.selected_idx = (self.selected_idx + self.page_step()).min(max_idx);
        self.adjust_scroll(self.visible_height);
    }

    /// Move the cursor up by one page, then keep it on screen.
    pub(crate) fn page_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected_idx = self.selected_idx.saturating_sub(self.page_step());
        self.adjust_scroll(self.visible_height);
    }

    /// Content rows the last frame showed.
    pub(crate) fn visible_height(&self) -> usize {
        self.visible_height
    }

    /// Record the frame's list height and keep the cursor inside it.
    pub(crate) fn set_visible_height(&mut self, visible_height: usize) {
        self.visible_height = visible_height;
        self.adjust_scroll(visible_height);
    }

    /// Scroll the minimum needed to keep the cursor inside a `visible_height`
    /// -tall viewport.
    pub(crate) fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected_idx - visible_height + 1;
        }
    }

    /// Restore the cursor onto `path` after a recompute, keeping the previous
    /// scroll where the row is still on screen. A path a collapse hid falls
    /// back to its nearest listed ancestor. Returns false when neither is
    /// listed, leaving the cursor at the top.
    pub(crate) fn restore_cursor(
        &mut self,
        path: Option<&std::path::Path>,
        prev_scroll: usize,
        visible_height: usize,
    ) -> bool {
        let found = path.and_then(|path| {
            path.ancestors()
                .take_while(|candidate| !candidate.as_os_str().is_empty())
                .find_map(|candidate| {
                    self.rows.iter().position(|r| {
                        r.relative_path == candidate
                            || r.left_relative_path_raw.as_deref() == Some(candidate)
                            || r.right_relative_path_raw.as_deref() == Some(candidate)
                    })
                })
        });
        match found {
            Some(idx) => {
                self.selected_idx = idx;
                let max_scroll = self.rows.len().saturating_sub(1);
                self.scroll_offset = prev_scroll.min(max_scroll);
                self.adjust_scroll(visible_height);
                true
            }
            None => {
                self.reset_cursor();
                false
            }
        }
    }

    /// True while the filter input bar is open and routing key events.
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// The committed filter pattern (set on Enter/Esc), lowercase-matched
    /// against row names and paths.
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
    /// [`DirectoryTreeState::commit`].
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
    /// diffs-only flag together, and list the rows they keep.
    pub(crate) fn commit(&mut self) {
        self.active = false;
        self.pattern = self.input.to_string();
        self.diffs_only = self.draft_diffs_only;
        self.apply_filter();
    }

    /// Close the filter input bar, discarding any uncommitted typing and any
    /// diffs-only toggle made during the editing session.
    pub(crate) fn cancel(&mut self) {
        self.active = false;
        self.input.set(self.pattern.clone());
        self.draft_diffs_only = self.diffs_only;
    }

    /// Clear the filter entirely (pattern + diffs-only) and list every row.
    pub(crate) fn clear(&mut self) {
        self.pattern.clear();
        self.input.clear();
        self.diffs_only = false;
        self.draft_diffs_only = false;
        self.apply_filter();
    }

    /// List the rows the filter keeps, keeping the cursor on the same row
    /// where it survived the recompute.
    fn apply_filter(&mut self) {
        let prev_path = self.selected_row().map(|r| r.relative_path.clone());
        let prev_scroll = self.scroll_offset;
        self.recompute();
        self.restore_cursor(prev_path.as_deref(), prev_scroll, self.visible_height);
    }

    /// Relist the tree after it changed, and recount the footer summary.
    fn refresh(&mut self) {
        self.tree_summary = self
            .root_node
            .as_ref()
            .map(crate::diff::TreeSummary::from_root);
        // As before a tree existed: rows seeded without one do not survive a
        // tree change.
        #[cfg(test)]
        if self.root_node.is_none() {
            self.seed_rows.clear();
        }
        self.apply_filter();
    }

    pub(crate) fn root_node(&self) -> Option<&AlignedNode> {
        self.root_node.as_ref()
    }

    pub(crate) fn tree_summary(&self) -> Option<crate::diff::TreeSummary> {
        self.tree_summary
    }

    /// Every row the tree flattens to, before the filter applies.
    #[cfg(test)]
    pub(crate) fn flat_rows(&self) -> Vec<FlatRow> {
        match &self.root_node {
            Some(_) => self.flattened(),
            None => self.seed_rows.clone(),
        }
    }

    /// Adopt a finished scan's tree, keeping the expand state the previous
    /// tree carried.
    pub(crate) fn adopt(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.reconcile_expanded();
        self.refresh();
    }

    /// Graft a freshly rescanned subtree into the tree, or adopt it wholesale
    /// when there is no tree yet, keeping every directory's expand state.
    /// Returns false, changing nothing, when `path` is not in the tree.
    pub(crate) fn graft_subtree(&mut self, path: &Path, node: AlignedNode) -> bool {
        let grafted = match self.root_node.as_mut() {
            Some(root) => crate::diff::replace_subtree(root, path, node),
            None => {
                self.root_node = Some(node);
                true
            }
        };
        if grafted {
            self.reconcile_expanded();
            self.refresh();
        }
        grafted
    }

    /// Expand the selected directory.
    pub(crate) fn expand_selected(&mut self) {
        self.set_selected_expanded(true);
    }

    /// Collapse the selected directory. A selection it hides moves to the
    /// directory itself.
    pub(crate) fn collapse_selected(&mut self) {
        self.set_selected_expanded(false);
    }

    fn set_selected_expanded(&mut self, expanded: bool) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if !row.is_dir() {
            return;
        }
        let rel_path = row.relative_path.clone();
        self.set_expanded(&rel_path, expanded);
        self.refresh();
    }

    /// Expand (`true`) or collapse (`false`) every directory below the root,
    /// which stays open. A selection a collapse hides moves to its nearest
    /// listed ancestor.
    pub(crate) fn set_all_expanded(&mut self, expanded: bool) {
        for state in self.expanded.values_mut() {
            *state = expanded;
        }
        self.refresh();
    }

    /// Move the selection to the next difference (or the previous one when
    /// `forward` is false), wrapping around. Without a filter the jump expands
    /// the directories above the stop; with one it moves within the listed
    /// rows and leaves the expand state alone. Returns false when there is no
    /// stop.
    pub(crate) fn jump_to_difference(&mut self, forward: bool) -> bool {
        let current = self.selected_row().map(|r| r.relative_path.clone());
        let Some(target) = self.next_difference(current.as_deref(), forward) else {
            return false;
        };
        if !self.is_filtering() {
            for ancestor in target
                .ancestors()
                .skip(1)
                .take_while(|p| !p.as_os_str().is_empty())
            {
                self.set_expanded(ancestor, true);
            }
            self.refresh();
        }
        if let Some(idx) = self.rows.iter().position(|row| row.relative_path == target) {
            self.select_row_at(idx);
            self.adjust_scroll(self.visible_height);
        }
        true
    }

    /// Record a tree click at `idx` for double-click detection (400ms window).
    /// Returns `true` if this click is a double-click on the same index.
    pub(crate) fn note_click(&mut self, idx: usize) -> bool {
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

    /// Rebuild `rows` from the tree through the expand state, or through the
    /// current pattern and diffs-only flag while a filter applies. Leaves the
    /// cursor to [`DirectoryTreeState::apply_filter`].
    fn recompute(&mut self) {
        let root = self.root_node.as_ref();
        let pattern = &self.pattern;
        let diffs_only = self.diffs_only;

        if pattern.is_empty() && !diffs_only {
            self.rows = match root {
                Some(_) => self.flattened(),
                None => self.unfiltered_without_tree(),
            };
            return;
        }

        if let Some(root_node) = root {
            let mut matches = Vec::new();
            collect_matching_rows(root_node, pattern, diffs_only, &self.expanded, &mut matches);
            self.rows = matches;
        } else {
            let norm_pattern = crate::diff::normalize_for_matching(pattern);
            let source = self.unfiltered_without_tree();
            self.rows = source
                .iter()
                .filter(|row| {
                    if diffs_only
                        && row.state == DiffState::Identical
                        && !row.has_case_conflict
                        && !row.contains_case_conflict
                        && !row.is_ambiguous_case_collision
                    {
                        return false;
                    }
                    if pattern.is_empty() {
                        return true;
                    }
                    let norm_name = crate::diff::normalize_for_matching(&row.name);
                    let norm_path =
                        crate::diff::normalize_for_matching(&row.relative_path.to_string_lossy());
                    let norm_l_name = row
                        .left_name_raw
                        .as_ref()
                        .map(|n| crate::diff::normalize_for_matching(n));
                    let norm_r_name = row
                        .right_name_raw
                        .as_ref()
                        .map(|n| crate::diff::normalize_for_matching(n));
                    let norm_l_path = row
                        .left_relative_path_raw
                        .as_ref()
                        .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));
                    let norm_r_path = row
                        .right_relative_path_raw
                        .as_ref()
                        .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));

                    norm_name.contains(&norm_pattern)
                        || norm_path.contains(&norm_pattern)
                        || norm_l_name
                            .as_ref()
                            .is_some_and(|n| n.contains(&norm_pattern))
                        || norm_r_name
                            .as_ref()
                            .is_some_and(|n| n.contains(&norm_pattern))
                        || norm_l_path
                            .as_ref()
                            .is_some_and(|p| p.contains(&norm_pattern))
                        || norm_r_path
                            .as_ref()
                            .is_some_and(|p| p.contains(&norm_pattern))
                })
                .cloned()
                .collect();
        }
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }

    #[allow(dead_code)]
    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    #[allow(dead_code)]
    pub(crate) fn set_rows(&mut self, rows: Vec<FlatRow>) {
        self.rows = rows;
    }

    #[allow(dead_code)]
    pub(crate) fn set_pattern(&mut self, pattern: impl Into<String>) {
        self.pattern = pattern.into();
    }

    #[cfg(test)]
    pub(crate) fn set_diffs_only(&mut self, diffs_only: bool) {
        self.diffs_only = diffs_only;
    }
}

/// Whether an entry is a place the difference jumps stop (Issue #338): the
/// entries the diffs-only filter keeps, except a directory present on both
/// sides, whose `≠` only repeats what its children hold.
fn is_difference_stop(
    left: Option<&FileInfo>,
    right: Option<&FileInfo>,
    state: DiffState,
    has_case_conflict: bool,
    is_ambiguous_case_collision: bool,
) -> bool {
    let both_dirs = left.is_some_and(|f| f.is_dir) && right.is_some_and(|f| f.is_dir);
    has_case_conflict || is_ambiguous_case_collision || (!both_dirs && state.is_known_difference())
}

fn collect_matching_rows(
    root: &AlignedNode,
    pattern: &str,
    diffs_only: bool,
    expanded: &HashMap<PathBuf, bool>,
    out: &mut Vec<FlatRow>,
) {
    let norm_pattern = crate::diff::normalize_for_matching(pattern);
    for child in &root.children {
        collect_matching_rows_rec(child, pattern, &norm_pattern, diffs_only, expanded, out);
    }
}

/// Whether the filter lists `node`: it matches the pattern, and it is a
/// difference when only differences are listed. `norm_pattern` is `pattern`
/// normalized once by the caller.
fn node_matches_filter(
    node: &AlignedNode,
    pattern: &str,
    norm_pattern: &str,
    diffs_only: bool,
) -> bool {
    let diffs_match = if diffs_only {
        node.state.is_known_difference()
            || node.has_case_conflict
            || node.contains_case_conflict
            || node.is_ambiguous_case_collision
    } else {
        true
    };

    let text_match = if pattern.is_empty() {
        true
    } else {
        let norm_name = crate::diff::normalize_for_matching(&node.name);
        let norm_path = crate::diff::normalize_for_matching(&node.relative_path.to_string_lossy());
        let norm_l_name = node
            .left_name
            .as_ref()
            .map(|n| crate::diff::normalize_for_matching(n));
        let norm_r_name = node
            .right_name
            .as_ref()
            .map(|n| crate::diff::normalize_for_matching(n));
        let norm_l_path = node
            .left_relative_path
            .as_ref()
            .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));
        let norm_r_path = node
            .right_relative_path
            .as_ref()
            .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));

        norm_name.contains(norm_pattern)
            || norm_path.contains(norm_pattern)
            || norm_l_name
                .as_ref()
                .is_some_and(|n| n.contains(norm_pattern))
            || norm_r_name
                .as_ref()
                .is_some_and(|n| n.contains(norm_pattern))
            || norm_l_path
                .as_ref()
                .is_some_and(|p| p.contains(norm_pattern))
            || norm_r_path
                .as_ref()
                .is_some_and(|p| p.contains(norm_pattern))
    };

    diffs_match && text_match
}

fn collect_matching_rows_rec(
    node: &AlignedNode,
    pattern: &str,
    norm_pattern: &str,
    diffs_only: bool,
    expanded: &HashMap<PathBuf, bool>,
    out: &mut Vec<FlatRow>,
) {
    if node_matches_filter(node, pattern, norm_pattern, diffs_only) {
        out.push(FlatRow {
            depth: 0,
            relative_path: node.relative_path.clone(),
            name: node.name.clone(),
            left_name_raw: node.left_name.clone(),
            right_name_raw: node.right_name.clone(),
            left_relative_path_raw: node.left_relative_path.clone(),
            right_relative_path_raw: node.right_relative_path.clone(),
            state: node.state,
            left: node.left.clone(),
            right: node.right.clone(),
            is_expanded: expanded
                .get(&node.relative_path)
                .copied()
                .unwrap_or(node.expanded_by_default),
            has_case_conflict: node.has_case_conflict,
            contains_case_conflict: node.contains_case_conflict,
            is_ambiguous_case_collision: node.is_ambiguous_case_collision,
        });
    }

    for child in &node.children {
        collect_matching_rows_rec(child, pattern, norm_pattern, diffs_only, expanded, out);
    }
}

/// The Config screen's own state: the selected row and the view to restore on
/// close. Owned by [`App::config`]/[`App::config_mut`]. Unlike [`HelpState`]/
/// [`DirectoryTreeState`], most Config methods stay on `App` as orchestration:
/// [`App::config_rows`] (the row list `ConfigState`'s selection indexes into)
/// reads `App::detected_diff_tools`, a concern `ConfigState` doesn't own, so
/// [`App::ensure_config_selection`]/`config_select_next`/`config_select_prev`/
/// `config_select_at` build the row list on `App` and hand it to a
/// [`ConfigState`] method that does the pure index math — mirroring how
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
/// It also owns the panes' geometry — the frame's height and text width, and
/// the row counts they wrap to — so every scroll, jump, and stage reads the
/// same numbers the painter does, and each method leaves scroll inside them
/// (ADR-0005). [`crate::view::prepare_frame`] hands it the pane size once per
/// frame; `App` keeps only what needs I/O or settings: loading, saving, and
/// the file pair's paths.
#[derive(Clone, Debug, Default)]
pub struct FileDiffState {
    rows: Vec<crate::diff_view::DiffRow>,
    scroll: usize,
    h_scroll: usize,
    wrap: bool,
    show_full: bool,
    /// Unchanged lines kept around each change when not showing the full
    /// file. Every re-diff reads it here.
    context: usize,
    left_hash: Option<String>,
    right_hash: Option<String>,
    left_line_ending: Option<String>,
    right_line_ending: Option<String>,
    /// Working buffers the diff is computed from. `[` / `]` edit these; nothing
    /// reaches disk until an explicit save (Issue #235).
    left: crate::diff_view::TextBuffer,
    right: crate::diff_view::TextBuffer,
    /// The bytes each side had on disk when the session opened, or when the last
    /// save succeeded. A side is dirty exactly while it differs from its
    /// baseline, and the baseline is what a save checks the file against.
    left_baseline: crate::diff_view::TextBuffer,
    right_baseline: crate::diff_view::TextBuffer,
    /// Working-buffer snapshots taken before each staged hunk, newest last.
    undo_stack: Vec<(crate::diff_view::TextBuffer, crate::diff_view::TextBuffer)>,
    /// The physical row offset `N`/`P` last navigated to, independent of
    /// `scroll`.
    ///
    /// `scroll` doubles as the viewport's render offset, which `clamp_scroll`
    /// pulls back to `max_scroll` every frame — 0 whenever the diff
    /// already fits the viewport. Without this field, navigating to a hunk
    /// trailing near EOF (or any hunk `clamp_scroll` can't fully reach) would
    /// have that navigation silently undone before the next `[`/`]`, staging
    /// whatever hunk `scroll` was clamped back to instead — and a repeat
    /// `N`/`P` would recompute the same jump instead of advancing past it,
    /// since it would start over from the clamped position every time. A raw
    /// offset (not a hunk index) so stepping between two change rows within
    /// one hunk still works. Cleared by manual scrolling or any re-diff, so it
    /// never resolves against stale rows; a stage then pins it to the next
    /// change block.
    nav_scroll: Option<usize>,
    /// Content rows visible in a pane (borders excluded), from the last frame.
    visible_height: usize,
    /// Text columns inside one pane (borders and gutter excluded), from the
    /// last frame. Zero leaves every line unwrapped, as `wrap::lines` paints it.
    content_width: usize,
    /// Longest line (in characters) across `rows`.
    max_line_width: usize,
    /// Physical (post-wrap) row count of `rows` at `content_width`.
    physical_rows: usize,
}

impl FileDiffState {
    /// An empty File Diff that diffs with `context` unchanged lines.
    pub(crate) fn with_context(context: usize) -> Self {
        Self {
            context,
            ..Self::default()
        }
    }

    /// Take the frame's pane size: rows inside the borders, and columns
    /// inside the borders, of which the gutter takes its share. Then keep
    /// scroll inside the content it now shows.
    pub(crate) fn set_frame(&mut self, visible_height: usize, pane_inner_width: usize) {
        self.visible_height = visible_height;
        self.content_width = crate::diff_view::diff_text_width(
            pane_inner_width,
            self.left_line_count(),
            self.right_line_count(),
        );
        self.resync_geometry();
        self.clamp_scroll();
    }

    /// Content rows visible in a pane, from the last frame.
    pub(crate) fn visible_height(&self) -> usize {
        self.visible_height
    }

    /// Text columns inside one pane, from the last frame.
    pub(crate) fn content_width(&self) -> usize {
        self.content_width
    }

    /// Physical (post-wrap) row count of the rows.
    #[cfg(test)]
    pub(crate) fn physical_rows(&self) -> usize {
        self.physical_rows
    }

    /// Longest line (in characters) across the rows.
    #[cfg(test)]
    pub(crate) fn max_line_width(&self) -> usize {
        self.max_line_width
    }

    /// Largest vertical scroll offset that still fills the panes.
    pub(crate) fn max_scroll(&self) -> usize {
        self.physical_rows.saturating_sub(self.visible_height)
    }

    /// Largest horizontal scroll offset that keeps the longest line reachable.
    pub(crate) fn max_h_scroll(&self) -> usize {
        self.max_line_width.saturating_sub(self.content_width)
    }

    /// Recount the rows' longest line and wrapped height at the last frame's
    /// width, after the rows or the wrap setting changed.
    fn resync_geometry(&mut self) {
        self.max_line_width = crate::diff_view::diff_max_line_width(&self.rows);
        self.physical_rows =
            crate::diff_view::diff_total_physical_rows(&self.rows, self.content_width, self.wrap);
    }

    /// Page size for `Ctrl+f` / `Ctrl+b`: the last drawn height, with a
    /// one-row overlap when possible so context isn't completely lost.
    fn page_step(&self) -> usize {
        self.visible_height.saturating_sub(1).max(1)
    }
    /// The current file diff's rows. Read access for rendering.
    pub(crate) fn rows(&self) -> &[crate::diff_view::DiffRow] {
        &self.rows
    }

    /// Total left-file lines (working buffer, falling back to row metadata).
    pub(crate) fn left_line_count(&self) -> usize {
        self.left
            .lines
            .len()
            .max(crate::diff_view::diff_side_line_count(&self.rows, true))
    }

    /// Total right-file lines (working buffer, falling back to row metadata).
    pub(crate) fn right_line_count(&self) -> usize {
        self.right
            .lines
            .len()
            .max(crate::diff_view::diff_side_line_count(&self.rows, false))
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

    /// Replace both sides with freshly loaded content and recompute
    /// `rows`/hashes/line-endings. Loading can fail before this is called,
    /// which leaves `self` untouched.
    pub(crate) fn load(
        &mut self,
        left: crate::diff_view::LoadedText,
        right: crate::diff_view::LoadedText,
    ) {
        self.left = crate::diff_view::TextBuffer::from_text(&left.text);
        self.right = crate::diff_view::TextBuffer::from_text(&right.text);
        self.left_baseline = self.left.clone();
        self.right_baseline = self.right.clone();
        self.undo_stack.clear();
        self.left_hash = left.sha256;
        self.right_hash = right.sha256;
        self.left_line_ending = left.line_ending;
        self.right_line_ending = right.line_ending;
        self.recompute_rows();
    }

    /// Re-diff the working buffers. Every path that changes a buffer or the
    /// full-context flag ends here, so the rows always describe the staged
    /// bytes — and so `nav_scroll` never resolves against stale rows.
    pub(crate) fn recompute_rows(&mut self) {
        self.nav_scroll = None;
        self.rows = crate::diff_view::compare_texts(
            &self.left.to_text(),
            &self.right.to_text(),
            self.show_full,
            self.context,
        );
        self.resync_geometry();
    }

    /// The left working buffer's staged bytes.
    pub(crate) fn left_buffer(&self) -> &crate::diff_view::TextBuffer {
        &self.left
    }

    /// The right working buffer's staged bytes.
    pub(crate) fn right_buffer(&self) -> &crate::diff_view::TextBuffer {
        &self.right
    }

    /// Whether the left side has staged, unsaved edits.
    pub(crate) fn left_dirty(&self) -> bool {
        self.left != self.left_baseline
    }

    /// Whether the right side has staged, unsaved edits.
    pub(crate) fn right_dirty(&self) -> bool {
        self.right != self.right_baseline
    }

    /// Whether either side has staged, unsaved edits.
    pub(crate) fn is_dirty(&self) -> bool {
        self.left_dirty() || self.right_dirty()
    }

    /// Whether there is a staged hunk operation left to undo.
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Stage one hunk copy into the working buffers and re-diff. Nothing is
    /// written; the previous buffers are pushed onto the undo stack first.
    /// Returns whether the hunk actually changed a buffer. `false` means the
    /// copy was a no-op — nothing is pushed onto the undo stack and the rows
    /// are not recomputed, so the caller can report "nothing to stage"
    /// instead of a save that never happened.
    pub(crate) fn stage_hunk(
        &mut self,
        hunk_index: usize,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        let snapshot = (self.left.clone(), self.right.clone());
        let rows = std::mem::take(&mut self.rows);
        let result = crate::diff_view::stage_hunk_copy(
            &mut self.left,
            &mut self.right,
            &rows,
            hunk_index,
            direction,
        );
        self.rows = rows;
        match result {
            Ok(changed) => {
                if changed {
                    self.undo_stack.push(snapshot);
                    self.recompute_rows();
                }
                Ok(changed)
            }
            Err(e) => {
                // Restore in case the splice ran partway.
                self.left = snapshot.0;
                self.right = snapshot.1;
                Err(e)
            }
        }
    }

    /// The rows of the change hunk under the cursor: the one `[` / `]` stage
    /// and the painter highlights.
    pub(crate) fn active_hunk_rows(&self) -> Option<std::ops::Range<usize>> {
        crate::diff_view::active_hunk_rows(
            &self.rows,
            self.nav_scroll,
            self.scroll,
            self.content_width,
            self.wrap,
        )
    }

    /// Index of the change hunk under the cursor, at the geometry painted.
    fn active_hunk(&self) -> Option<usize> {
        crate::diff_view::resolve_active_hunk(
            &self.rows,
            self.nav_scroll,
            self.scroll,
            self.content_width,
            self.wrap,
        )
    }

    /// Stage the change hunk under the cursor in `direction`, then park the
    /// cursor on the next change block. Returns whether a buffer changed; an
    /// error when no change block is under the cursor.
    pub(crate) fn stage_active_hunk(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        let hunk_index = self.active_hunk().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no change block at cursor",
            )
        })?;
        let hunk_start_row = crate::diff_view::diff_hunk_row_ranges(&self.rows)
            .get(hunk_index)
            .map(|r| r.start)
            .unwrap_or(0);
        let changed = self.stage_hunk(hunk_index, direction)?;
        if changed {
            self.select_hunk_after(hunk_start_row);
        }
        Ok(changed)
    }

    /// After a staged hunk lands, park the cursor on the next change block at or
    /// after where it was, falling back to the nearest valid position when the
    /// edit removed every later hunk (Issue #235).
    ///
    /// Pins `nav_scroll` to that next hunk's offset — not just `scroll` — for
    /// the same reason [`FileDiffState::jump_to_change`] does: a hunk trailing
    /// near EOF can sit past `max_scroll`, and `scroll` alone would lose track
    /// of it on the very next frame's clamp.
    fn select_hunk_after(&mut self, previous_row: usize) {
        let offsets =
            crate::diff_view::diff_row_physical_offsets(&self.rows, self.content_width, self.wrap);
        let next = crate::diff_view::diff_hunk_row_ranges(&self.rows)
            .into_iter()
            .find(|range| range.start >= previous_row)
            .and_then(|range| offsets.get(range.start).copied());
        let max_scroll = self.max_scroll();
        match next {
            Some(offset) => {
                self.scroll = offset.min(max_scroll);
                self.nav_scroll = Some(offset);
            }
            None => {
                self.scroll = self.scroll.min(max_scroll);
                self.nav_scroll = None;
            }
        }
    }

    /// Undo the most recent staged hunk operation. Returns false when there is
    /// nothing left to undo.
    pub(crate) fn undo_staged(&mut self) -> bool {
        let Some((left, right)) = self.undo_stack.pop() else {
            return false;
        };
        self.left = left;
        self.right = right;
        self.recompute_rows();
        self.clamp_scroll();
        true
    }

    /// Throw away every staged edit and go back to the session baseline.
    pub(crate) fn discard_staged(&mut self) {
        self.left = self.left_baseline.clone();
        self.right = self.right_baseline.clone();
        self.undo_stack.clear();
        self.recompute_rows();
        self.clamp_scroll();
    }

    /// The bytes the left side had at the session baseline.
    pub(crate) fn left_baseline_text(&self) -> String {
        self.left_baseline.to_text()
    }

    /// The bytes the right side had at the session baseline.
    pub(crate) fn right_baseline_text(&self) -> String {
        self.right_baseline.to_text()
    }

    /// Promote the working buffers to the new baseline after a successful save,
    /// clearing dirty and undo state.
    pub(crate) fn commit_baselines(
        &mut self,
        left_hash: Option<String>,
        right_hash: Option<String>,
    ) {
        self.left_baseline = self.left.clone();
        self.right_baseline = self.right.clone();
        self.undo_stack.clear();
        self.left_hash = left_hash;
        self.right_hash = right_hash;
    }

    /// Flip line wrapping and reset scroll, since the old scroll position no
    /// longer lines up once wrapping changes the layout.
    pub(crate) fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.resync_geometry();
        self.reset_scroll();
    }

    /// Flip full-file vs. diff-only content and re-diff the working buffers.
    /// Infallible: nothing is reloaded from disk, so a context toggle never
    /// throws staged edits away and has nothing to fail at (Issue #235).
    pub(crate) fn toggle_show_full(&mut self) {
        self.show_full = !self.show_full;
        self.recompute_rows();
        self.reset_scroll();
    }

    /// Set the full-file flag directly (vs. [`FileDiffState::toggle_show_full`]'s
    /// flip). Used by [`App::enter_file_diff`] to force diff-only mode before
    /// the first load, and by tests to seed a specific state.
    pub(crate) fn set_show_full(&mut self, on: bool) {
        self.show_full = on;
    }

    /// Line-step down, stopping at [`FileDiffState::max_scroll`]. Shared by
    /// keyboard j/Down and mouse scroll down. Manual movement overrides
    /// wherever `N`/`P` last pinned the cursor.
    pub(crate) fn scroll_down(&mut self) {
        self.nav_scroll = None;
        if self.scroll < self.max_scroll() {
            self.scroll += 1;
        }
    }

    /// Line-step up (no-op at the top). Shared by keyboard k/Up and mouse
    /// scroll up. See [`FileDiffState::scroll_down`] on `nav_scroll`.
    pub(crate) fn scroll_up(&mut self) {
        self.nav_scroll = None;
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Page down (`Ctrl+f`), stopping at [`FileDiffState::max_scroll`]. See
    /// [`FileDiffState::scroll_down`] on `nav_scroll`.
    pub(crate) fn page_down(&mut self) {
        self.nav_scroll = None;
        self.scroll = (self.scroll + self.page_step()).min(self.max_scroll());
    }

    /// Page up (`Ctrl+b`), no-op past the top. See
    /// [`FileDiffState::scroll_down`] on `nav_scroll`.
    pub(crate) fn page_up(&mut self) {
        self.nav_scroll = None;
        self.scroll = self.scroll.saturating_sub(self.page_step());
    }

    /// Horizontal step left, when wrap is off (no-op while wrapping or at the
    /// left edge).
    pub(crate) fn h_scroll_left(&mut self) {
        if !self.wrap && self.h_scroll > 0 {
            self.h_scroll -= 1;
        }
    }

    /// Horizontal step right, when wrap is off, stopping at
    /// [`FileDiffState::max_h_scroll`].
    pub(crate) fn h_scroll_right(&mut self) {
        if !self.wrap && self.h_scroll < self.max_h_scroll() {
            self.h_scroll += 1;
        }
    }

    /// Zero both scroll offsets. Used after wrap/full toggles and on entering
    /// a fresh file diff, where the old scroll position no longer applies.
    pub(crate) fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.h_scroll = 0;
        self.nav_scroll = None;
    }

    /// Pull both scroll offsets back inside the content. Growing the
    /// terminal (or opening a shorter file) can leave them past the end;
    /// without this the next page or arrow key would appear to jump
    /// backwards.
    pub(crate) fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
        self.h_scroll = self.h_scroll.min(self.max_h_scroll());
    }

    /// Reset scroll and clear cached hashes after [`App::swap_paths`] — rows
    /// and line-endings are left for the next `refresh_file_diff` to replace.
    pub(crate) fn reset_for_swap(&mut self) {
        self.scroll = 0;
        self.nav_scroll = None;
        self.left_hash = None;
        self.right_hash = None;
    }

    /// Jump to the next (`forward`) or previous differing block.
    ///
    /// Starts from `nav_scroll` rather than `scroll` when one is pinned. The
    /// per-frame clamp to `max_scroll` (0 once the diff already fits the
    /// viewport) pulls `scroll` back to whatever it can reach; computing the
    /// next jump from that clamped value would recompute the same target on a
    /// repeat `N`/`P` instead of advancing past it.
    ///
    /// Also pins `nav_scroll` to the target offset in addition to setting
    /// `scroll`: `scroll` alone isn't enough when the target trails near EOF,
    /// since that same clamp would otherwise silently pull the next `[`/`]`
    /// back to whatever hunk `scroll` clamps to instead of the one just
    /// navigated to.
    pub(crate) fn jump_to_change(&mut self, forward: bool) {
        let current = self.nav_scroll.unwrap_or(self.scroll);
        if let Some(scroll) = crate::diff_view::jump_to_change_scroll(
            &self.rows,
            current,
            self.content_width,
            self.wrap,
            forward,
        ) {
            self.scroll = scroll;
            self.nav_scroll = Some(scroll);
        }
    }

    /// Set the vertical scroll offset directly, for tests to seed a position.
    #[cfg(test)]
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

    /// Take a frame by its text width rather than its pane width, as tests
    /// that pin the width a diff wraps at need: then resync and clamp, as
    /// [`FileDiffState::set_frame`] does.
    #[cfg(test)]
    pub(crate) fn set_text_frame(&mut self, visible_height: usize, content_width: usize) {
        self.visible_height = visible_height;
        self.content_width = content_width;
        self.resync_geometry();
        self.clamp_scroll();
    }

    /// Seed the frame geometry a test needs without drawing one.
    #[cfg(test)]
    pub(crate) fn set_geometry(
        &mut self,
        visible_height: usize,
        content_width: usize,
        physical_rows: usize,
    ) {
        self.visible_height = visible_height;
        self.content_width = content_width;
        self.physical_rows = physical_rows;
    }

    #[allow(dead_code)]
    pub(crate) fn set_hashes(&mut self, left: Option<String>, right: Option<String>) {
        self.left_hash = left;
        self.right_hash = right;
    }
}

/// The background scan: whether one is in flight, its progress and
/// generation, and the spinner that shows it. Owned by [`App::scan`] /
/// [`App::scan_mut`]. The tree a scan produces belongs to
/// [`DirectoryTreeState`] (ADR-0005).
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
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Advance the TUI animation frame.
    pub(crate) fn tick(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    /// Update the scanned item count from a background progress report.
    pub(crate) fn set_progress(&mut self, count: usize) {
        self.progress_count = count;
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

/// Tree walks behind [`DirectoryTreeState`]'s operations.
impl DirectoryTreeState {
    /// The tree's rows through the expand state, in display order.
    fn flattened(&self) -> Vec<FlatRow> {
        let mut rows = Vec::new();
        if let Some(root) = &self.root_node {
            for child in &root.children {
                self.flatten_node(child, 0, &mut rows);
            }
        }
        rows
    }

    /// The rows to list with no tree: none in production, a test's seed.
    fn unfiltered_without_tree(&self) -> Vec<FlatRow> {
        #[cfg(test)]
        return self.seed_rows.clone();
        #[cfg(not(test))]
        Vec::new()
    }

    fn flatten_node(&self, node: &AlignedNode, depth: usize, rows: &mut Vec<FlatRow>) {
        let is_expanded = self.is_expanded(node);
        rows.push(FlatRow {
            depth,
            relative_path: node.relative_path.clone(),
            name: node.name.clone(),
            left_name_raw: node.left_name.clone(),
            right_name_raw: node.right_name.clone(),
            left_relative_path_raw: node.left_relative_path.clone(),
            right_relative_path_raw: node.right_relative_path.clone(),
            state: node.state,
            left: node.left.clone(),
            right: node.right.clone(),
            is_expanded,
            has_case_conflict: node.has_case_conflict,
            contains_case_conflict: node.contains_case_conflict,
            is_ambiguous_case_collision: node.is_ambiguous_case_collision,
        });
        if is_expanded {
            for child in &node.children {
                self.flatten_node(child, depth + 1, rows);
            }
        }
    }

    /// Whether `node` is shown open: the user's choice for a directory, the
    /// scanner's default for anything the map does not hold.
    fn is_expanded(&self, node: &AlignedNode) -> bool {
        self.expanded
            .get(&node.relative_path)
            .copied()
            .unwrap_or(node.expanded_by_default)
    }

    /// Make `expanded` hold exactly the current tree's directories: one it
    /// already knew keeps the user's choice, a new one takes the scanner's
    /// default, and one no longer in the tree is forgotten.
    fn reconcile_expanded(&mut self) {
        fn walk(
            node: &AlignedNode,
            previous: &HashMap<PathBuf, bool>,
            next: &mut HashMap<PathBuf, bool>,
        ) {
            if DirectoryTreeState::is_dir_node(node) {
                let expanded = previous
                    .get(&node.relative_path)
                    .copied()
                    .unwrap_or(node.expanded_by_default);
                next.insert(node.relative_path.clone(), expanded);
            }
            for child in &node.children {
                walk(child, previous, next);
            }
        }
        let previous = std::mem::take(&mut self.expanded);
        if let Some(root) = &self.root_node {
            for child in &root.children {
                walk(child, &previous, &mut self.expanded);
            }
        }
    }

    /// Whether a filter applies, listing its matches instead of the tree.
    fn is_filtering(&self) -> bool {
        !self.pattern.is_empty() || self.diffs_only
    }

    /// Visit every entry in tree order with whether the jumps stop there: a
    /// difference, not inside a one-sided directory or a type conflict (that
    /// entry is the difference as a whole), and listed by any filter. The one
    /// definition both the jumps and the Palette's gate read. `visit` returns
    /// false to stop the walk early.
    fn walk_difference_stops(&self, mut visit: impl FnMut(&AlignedNode, bool) -> bool) {
        fn walk(
            tree: &DirectoryTreeState,
            filter: Option<(&str, &str)>,
            node: &AlignedNode,
            inside_whole: bool,
            visit: &mut dyn FnMut(&AlignedNode, bool) -> bool,
        ) -> bool {
            let listed = filter.is_none_or(|(pattern, norm_pattern)| {
                node_matches_filter(node, pattern, norm_pattern, tree.diffs_only)
            });
            let stop = listed && !inside_whole && DirectoryTreeState::is_stop_node(node);
            if !visit(node, stop) {
                return false;
            }
            let whole = inside_whole || !DirectoryTreeState::is_both_dirs(node);
            node.children
                .iter()
                .all(|child| walk(tree, filter, child, whole, visit))
        }
        let norm_pattern = crate::diff::normalize_for_matching(&self.pattern);
        let filter = self
            .is_filtering()
            .then_some((self.pattern.as_str(), norm_pattern.as_str()));
        if let Some(root) = &self.root_node {
            for child in &root.children {
                if !walk(self, filter, child, false, &mut visit) {
                    return;
                }
            }
        }
    }

    /// The next difference stop after `from` in tree order — or before it,
    /// when `forward` is false — wrapping around, whatever is expanded. With
    /// no `from`, the search starts from the top.
    fn next_difference(&self, from: Option<&Path>, forward: bool) -> Option<PathBuf> {
        let mut order = Vec::new();
        self.walk_difference_stops(|node, stop| {
            order.push((node.relative_path.clone(), stop));
            true
        });
        let len = order.len();
        let start = from.and_then(|path| order.iter().position(|(p, _)| p == path));
        (1..=len)
            .map(|step| match (start, forward) {
                (Some(i), true) => (i + step) % len,
                (Some(i), false) => (i + len - step) % len,
                (None, true) => step - 1,
                (None, false) => len - step,
            })
            .find(|&i| order[i].1)
            .map(|i| order[i].0.clone())
    }

    /// Whether a jump would find a stop, so the Palette can gate the jumps.
    /// Stops at the first one.
    pub(crate) fn has_difference(&self) -> bool {
        let mut found = false;
        self.walk_difference_stops(|_, stop| {
            found = stop;
            !stop
        });
        found
    }

    fn is_both_dirs(node: &AlignedNode) -> bool {
        node.left.as_ref().is_some_and(|f| f.is_dir)
            && node.right.as_ref().is_some_and(|f| f.is_dir)
    }

    fn is_stop_node(node: &AlignedNode) -> bool {
        is_difference_stop(
            node.left.as_ref(),
            node.right.as_ref(),
            node.state,
            node.has_case_conflict,
            node.is_ambiguous_case_collision,
        )
    }

    /// Expand or collapse the directory at `path`, leaving the rows to the
    /// caller's [`DirectoryTreeState::refresh`].
    fn set_expanded(&mut self, path: &Path, expanded: bool) {
        if let Some(state) = self.expanded.get_mut(path) {
            *state = expanded;
        }
    }

    fn is_dir_node(node: &AlignedNode) -> bool {
        node.left.as_ref().is_some_and(|f| f.is_dir)
            || node.right.as_ref().is_some_and(|f| f.is_dir)
    }

    // Test-only field setters (ADR-0002). Clippy's dead-code pass flags these
    // as unreachable outside `#[cfg(test)]` call sites, so each needs an
    // explicit `#[allow]`.
    #[allow(dead_code)]
    /// Install `node` with its own expand flags, as a test's tree literal
    /// spells them — unlike [`DirectoryTreeState::adopt`], which keeps the
    /// user's choices from the previous tree.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.expanded.clear();
        self.reconcile_expanded();
    }

    #[cfg(test)]
    pub(crate) fn set_flat_rows(&mut self, rows: Vec<FlatRow>) {
        self.seed_rows = rows;
    }

    #[cfg(test)]
    pub(crate) fn push_flat_row(&mut self, row: FlatRow) {
        self.seed_rows.push(row);
    }

    /// Reflatten and relist after a test installs a tree or rows directly.
    #[allow(dead_code)]
    pub(crate) fn refresh_for_test(&mut self, reflatten: bool) {
        if reflatten {
            self.refresh();
        } else {
            self.apply_filter();
        }
    }
}

pub struct App {
    left_path: PathBuf,
    right_path: PathBuf,
    /// The file pair named on the command line, when the session compares two
    /// files instead of two directories (Issue #327). File Diff reads its paths
    /// from here rather than from a Directory Tree row, and there is no tree to
    /// go back to.
    file_pair: Option<crate::target::FilePair>,
    /// Size and modification time of each file-pair side, refreshed whenever
    /// the pair is loaded or saved so drawing never touches the filesystem.
    file_pair_info: (Option<FileInfo>, Option<FileInfo>),
    /// Effective scan mode for this session. Seeded once at bootstrap from the
    /// persisted setting or `--scan-mode` (via [`App::set_scan_mode`], which
    /// deliberately does not persist); changed thereafter only through
    /// [`App::apply_scan_mode`], which persists first (Issue #238).
    scan_mode: crate::settings::ScanMode,
    scan: ScanState,
    view_mode: ViewMode,
    diff: FileDiffState,
    settings: crate::settings::AppSettings,
    /// Where settings changes persist (the config file, or memory in tests).
    store: crate::settings::SettingsStore,
    detected_diff_tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    config: ConfigState,
    exclusion_editor: Option<ExclusionEditorState>,
    palette: PaletteState,
    confirm_modal: Option<ConfirmModal>,
    /// Transient status toast: (message, is_error, created_at)
    status_message: Option<(String, bool, Instant)>,
    directory_tree: DirectoryTreeState,
    /// Which pane has focus, on the Directory Tree and File Diff alike.
    active_side_left: bool,
    /// Separate effective ignore matchers prevent one root's project rules
    /// from affecting the other side (Issue #237).
    left_ignore_matcher: IgnoreMatcher,
    right_ignore_matcher: IgnoreMatcher,
    /// Session-only patterns supplied by repeated `--exclude` flags.
    cli_exclusions: Vec<String>,
    gitignore_override: Option<bool>,
    update_check_enabled: bool,
    /// Effective mouse-capture state for this session: `settings.mouse` unless overridden
    /// by the `--no-mouse` CLI flag. See [`crate::settings::resolve_mouse_enabled`].
    mouse_enabled: bool,
    update_available: Option<String>,
    install_method: crate::upgrade::InstallMethod,
    help: HelpState,
    should_quit: bool,
    /// The runtime key bindings (ADR-0003), with the config's `[keys]`
    /// section applied (Issue #339).
    keymap: crate::keymap::Keymap,
}

impl App {
    /// Open a session on `left` and `right`, from what [`Startup`] resolved:
    /// the settings, keymap, detected tools, install method, and the
    /// command-line overrides. Reads nothing itself.
    ///
    /// [`Startup`]: crate::startup::Startup
    pub fn from_startup(
        left: PathBuf,
        right: PathBuf,
        left_ignore_matcher: IgnoreMatcher,
        right_ignore_matcher: IgnoreMatcher,
        startup: crate::startup::Startup,
    ) -> Self {
        let status_message = startup
            .problem()
            .map(|problem| (problem.toast(), true, Instant::now()));
        let mouse_enabled = startup.mouse_enabled();
        let scan_mode = startup.scan_mode();
        let update_check_enabled = startup.update_check_enabled();
        let crate::startup::Startup {
            settings,
            store,
            keymap,
            detected_diff_tools,
            install_method,
            overrides,
            update_available,
            ..
        } = startup;

        Self {
            left_path: left,
            right_path: right,
            file_pair: None,
            file_pair_info: (None, None),
            scan_mode,
            scan: ScanState::default(),
            view_mode: ViewMode::DirectoryTree,
            diff: FileDiffState::with_context(settings.diff_context),
            settings,
            store,
            detected_diff_tools,
            config: ConfigState::default(),
            exclusion_editor: None,
            palette: PaletteState::default(),
            confirm_modal: None,
            status_message,
            directory_tree: DirectoryTreeState::default(),
            // A session starts on the left pane.
            active_side_left: true,
            left_ignore_matcher,
            right_ignore_matcher,
            cli_exclusions: overrides.exclude,
            gitignore_override: overrides.gitignore,
            update_check_enabled,
            mouse_enabled,
            update_available,
            install_method,
            help: HelpState::default(),
            should_quit: false,
            keymap,
        }
    }

    /// A test's session: [`crate::startup::Startup::for_test`], with no
    /// exclusions.
    #[cfg(test)]
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        Self::for_test(left, right, crate::startup::Startup::for_test())
    }

    /// A test's session on the settings [`crate::test_support::seeded_settings`]
    /// holds — every one not at its default — in memory.
    #[cfg(test)]
    pub(crate) fn seeded(left: PathBuf, right: PathBuf) -> Self {
        let settings = crate::test_support::seeded_settings();
        Self::for_test(
            left,
            right,
            crate::startup::Startup::with_settings(settings),
        )
    }

    /// What the last save put in a test's in-memory store.
    #[cfg(test)]
    pub(crate) fn saved_settings(&self) -> crate::settings::AppSettings {
        self.store.saved().expect("settings were saved")
    }

    /// A test's session from `startup`, with no exclusions.
    #[cfg(test)]
    pub(crate) fn for_test(
        left: PathBuf,
        right: PathBuf,
        startup: crate::startup::Startup,
    ) -> Self {
        let left_ignore = IgnoreMatcher::for_root(left.clone(), &[], true, &[])
            .expect("empty ignore matcher is valid");
        let right_ignore = IgnoreMatcher::for_root(right.clone(), &[], true, &[])
            .expect("empty ignore matcher is valid");
        Self::from_startup(left, right, left_ignore, right_ignore, startup)
    }

    /// The runtime key bindings this session routes and hints from.
    pub(crate) fn keymap(&self) -> &crate::keymap::Keymap {
        &self.keymap
    }

    /// Replace the runtime key bindings, for tests that exercise a remap.
    #[cfg(test)]
    pub(crate) fn set_keymap(&mut self, keymap: crate::keymap::Keymap) {
        self.keymap = keymap;
    }

    /// Apply a finished background scan.
    ///
    /// The scan ends and the Directory Tree adopts its tree together, so a
    /// caller cannot update one without the other. Results from a superseded
    /// [`App::begin_scan`] generation are dropped; returns `false` in that case
    /// and leaves the app untouched.
    pub fn apply_scan_result(&mut self, generation: u64, node: AlignedNode) -> bool {
        if !self.scan.finish(generation) {
            return false;
        }
        self.directory_tree.adopt(node);
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

    pub(crate) fn prepare_tree_viewport(&mut self, visible_height: usize) {
        self.directory_tree.set_visible_height(visible_height);
    }

    pub(crate) fn prepare_diff_viewport(&mut self, visible_height: usize, pane_inner_width: usize) {
        self.diff.set_frame(visible_height, pane_inner_width);
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

    /// Seed the session's effective scan mode without persisting it, as a
    /// `--scan-mode` value does, for tests.
    #[cfg(test)]
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
        if let Err(e) = self.save_settings() {
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

    pub fn ignore_matchers(&self) -> (&IgnoreMatcher, &IgnoreMatcher) {
        (&self.left_ignore_matcher, &self.right_ignore_matcher)
    }

    /// Effective mouse-capture state for this session: `settings.mouse` unless overridden
    /// by the `--no-mouse` CLI flag. See [`crate::settings::resolve_mouse_enabled`].
    pub fn mouse_enabled(&self) -> bool {
        self.mouse_enabled
    }

    /// Whether the background update check is enabled for this session
    /// (`settings.check_updates`, unless the `--no-update-check` CLI flag disabled it).
    pub fn update_check_enabled(&self) -> bool {
        self.update_check_enabled
    }

    /// Newer version string, if a completed update check found one.
    pub fn update_available(&self) -> Option<&str> {
        self.update_available.as_deref()
    }

    /// Set the newer-version hint, for tests. Live check outcomes go through
    /// [`App::apply_update_check_outcome`].
    #[cfg(test)]
    pub(crate) fn set_update_available(&mut self, version: Option<String>) {
        self.update_available = version;
    }

    /// How this binary was installed (Homebrew, Scoop, standalone, ...), used to
    /// tailor the update hint's suggested command.
    pub fn install_method(&self) -> &crate::upgrade::InstallMethod {
        &self.install_method
    }

    pub fn refresh_diff_tools(&mut self) {
        self.detected_diff_tools = crate::diff_tool::detect_diff_tools();
    }

    pub fn resolve_auto_diff_tool(&self) -> Option<crate::diff_tool::ExternalDiffTool> {
        self.detected_diff_tools
            .iter()
            .find(|(_, avail)| *avail)
            .map(|(tool, _)| *tool)
    }

    pub fn resolve_effective_diff_tool(&self) -> Option<crate::diff_tool::ExternalDiffTool> {
        match &self.settings.external_diff_tool {
            crate::settings::DiffToolSetting::Auto => self.resolve_auto_diff_tool(),
            crate::settings::DiffToolSetting::Disabled => None,
            crate::settings::DiffToolSetting::Pinned(tool) => {
                if self
                    .detected_diff_tools
                    .iter()
                    .any(|(t, avail)| t == tool && *avail)
                {
                    Some(*tool)
                } else {
                    None
                }
            }
            crate::settings::DiffToolSetting::Unknown(_) => None,
        }
    }

    /// Build the flat configuration row list (headers + fields).
    pub fn config_rows(&self) -> Vec<ConfigRowKind> {
        let mut rows = vec![ConfigRowKind::Header("External Diff Tool")];
        rows.push(ConfigRowKind::DiffToolAuto);
        rows.push(ConfigRowKind::DiffToolDisabled);
        rows.extend(
            self.detected_diff_tools
                .iter()
                .enumerate()
                .map(|(i, (_, avail))| ConfigRowKind::DiffTool {
                    idx: i,
                    available: *avail,
                }),
        );
        if self.settings.external_diff_tool.unknown_name().is_some() {
            rows.push(ConfigRowKind::DiffToolUnknown);
        }
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
        rows.push(ConfigRowKind::Header("Exclusions"));
        rows.push(ConfigRowKind::RespectGitignore);
        rows.push(ConfigRowKind::GlobalExclusions);
        rows.push(ConfigRowKind::IgnoreSources);
        rows.push(ConfigRowKind::Header("Key Bindings"));
        rows.push(ConfigRowKind::KeyBindings);
        rows
    }

    /// Read access to the persisted settings blob. Mutations go through App methods
    /// (`toggle_theme`, `apply_config_selection`, `adjust_config_selection`) that also
    /// persist through the session's [`crate::settings::SettingsStore`].
    pub fn settings(&self) -> &crate::settings::AppSettings {
        &self.settings
    }

    pub(crate) fn detected_diff_tools(&self) -> &[(crate::diff_tool::ExternalDiffTool, bool)] {
        &self.detected_diff_tools
    }

    pub(crate) fn cli_exclusion_count(&self) -> usize {
        self.cli_exclusions.len()
    }

    pub(crate) fn respect_gitignore(&self) -> bool {
        crate::settings::resolve_respect_gitignore(
            self.settings.respect_gitignore,
            self.gitignore_override,
        )
    }

    /// Persist the settings through the session's store, which refuses to
    /// overwrite a config file that failed to load (Issue #342).
    fn save_settings(&self) -> Result<(), std::io::Error> {
        self.store.save(&self.settings)
    }

    /// Persist a change that has already taken effect for this session, and
    /// say so when it could not be saved: it lasts only until duodiff exits.
    fn save_or_report(&mut self) {
        if let Err(error) = self.save_settings() {
            self.set_status(format!("Cannot save configuration: {error}"), true);
        }
    }

    /// Resolved colour palette for the current [`crate::settings::AppSettings::theme`].
    pub fn theme(&self) -> crate::theme::Theme {
        crate::theme::Theme::for_choice(self.settings.theme)
    }

    /// Flip between the dark and light theme and persist the choice.
    pub fn toggle_theme(&mut self) {
        self.settings.theme = self.settings.theme.toggled();
        self.set_status(format!("Theme: {}", self.settings.theme.label()), false);
        self.save_or_report();
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
            self.refresh_diff_tools();
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
    /// Unlike `App::help_mut`/`tree_list_mut`, every `ConfigState` mutator needs
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
            Some(ConfigRowKind::RespectGitignore) => {
                self.settings.respect_gitignore = !self.settings.respect_gitignore;
                let patterns = self.settings.global_exclusions.clone();
                if let Err(error) = self.rebuild_ignore_matchers(&patterns) {
                    self.settings.respect_gitignore = !self.settings.respect_gitignore;
                    self.set_status(format!("Cannot rebuild exclusions: {error}"), true);
                } else if let Err(error) = self.save_settings() {
                    self.set_status(format!("Cannot save configuration: {error}"), true);
                } else {
                    return true;
                }
            }
            Some(ConfigRowKind::GlobalExclusions) => self.open_exclusion_editor(),
            Some(ConfigRowKind::DiffToolAuto) => {
                self.settings.external_diff_tool = crate::settings::DiffToolSetting::Auto;
                self.save_or_report();
            }
            Some(ConfigRowKind::DiffToolDisabled) => {
                self.settings.external_diff_tool = crate::settings::DiffToolSetting::Disabled;
                self.save_or_report();
            }
            Some(ConfigRowKind::DiffTool {
                idx,
                available: true,
            }) => {
                if let Some((tool, _)) = self.detected_diff_tools.get(*idx) {
                    self.settings.external_diff_tool =
                        crate::settings::DiffToolSetting::Pinned(*tool);
                    self.save_or_report();
                }
            }
            Some(ConfigRowKind::CheckUpdates) => {
                self.settings.check_updates = !self.settings.check_updates;
                self.update_check_enabled = self.settings.check_updates;
                self.save_or_report();
            }
            Some(ConfigRowKind::Mouse) => {
                self.settings.mouse = !self.settings.mouse;
                self.mouse_enabled = self.settings.mouse;
                self.save_or_report();
            }
            Some(ConfigRowKind::Theme) => {
                self.toggle_theme();
            }
            _ => {}
        }
        false
    }

    fn rebuild_ignore_matchers(&mut self, patterns: &[String]) -> Result<(), String> {
        let respect_gitignore = crate::settings::resolve_respect_gitignore(
            self.settings.respect_gitignore,
            self.gitignore_override,
        );
        let left = IgnoreMatcher::for_root(
            self.left_path.clone(),
            patterns,
            respect_gitignore,
            &self.cli_exclusions,
        )?;
        let right = IgnoreMatcher::for_root(
            self.right_path.clone(),
            patterns,
            respect_gitignore,
            &self.cli_exclusions,
        )?;
        self.left_ignore_matcher = left;
        self.right_ignore_matcher = right;
        Ok(())
    }

    pub(crate) fn open_exclusion_editor(&mut self) {
        self.exclusion_editor = Some(ExclusionEditorState {
            draft: self.settings.global_exclusions.clone(),
            ..ExclusionEditorState::default()
        });
    }

    pub(crate) fn exclusion_editor_open(&self) -> bool {
        self.exclusion_editor.is_some()
    }

    pub(crate) fn exclusion_editor(&self) -> Option<&ExclusionEditorState> {
        self.exclusion_editor.as_ref()
    }

    /// Keep the highlighted exclusion in a `visible_rows`-tall list viewport.
    pub(crate) fn sync_exclusion_editor_viewport(&mut self, visible_rows: usize) {
        let Some(editor) = self.exclusion_editor.as_mut() else {
            return;
        };
        if visible_rows == 0 {
            editor.scroll_offset = 0;
            return;
        }
        let max_offset = editor.draft.len().saturating_sub(visible_rows);
        if editor.selected_idx < editor.scroll_offset {
            editor.scroll_offset = editor.selected_idx;
        } else if editor.selected_idx >= editor.scroll_offset + visible_rows {
            editor.scroll_offset = editor.selected_idx + 1 - visible_rows;
        }
        editor.scroll_offset = editor.scroll_offset.min(max_offset);
    }

    pub(crate) fn exclusion_editor_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let Some(editor) = self.exclusion_editor.as_mut() else {
            return false;
        };
        match editor.handle_key(key) {
            ExclusionEditorAction::None => false,
            ExclusionEditorAction::Cancel => {
                self.exclusion_editor = None;
                false
            }
            ExclusionEditorAction::Apply => self.apply_exclusion_editor(),
        }
    }

    fn apply_exclusion_editor(&mut self) -> bool {
        let draft = self
            .exclusion_editor
            .as_ref()
            .expect("apply only while editor is open")
            .draft
            .clone();
        let roots = [self.left_path.clone(), self.right_path.clone()];
        for root in &roots {
            if let Some((index, error)) = draft.iter().enumerate().find_map(|(index, pattern)| {
                IgnoreMatcher::validate_patterns(root, std::slice::from_ref(pattern))
                    .err()
                    .map(|error| (index, error))
            }) {
                self.exclusion_editor
                    .as_mut()
                    .expect("editor remains open after invalid input")
                    .selected_idx = index;
                self.set_status(format!("Invalid exclusion {}: {error}", index + 1), true);
                return false;
            }
        }
        if let Err(error) = self.rebuild_ignore_matchers(&draft) {
            self.set_status(format!("Cannot rebuild exclusions: {error}"), true);
            return false;
        }
        self.settings.global_exclusions = draft;
        if let Err(error) = self.save_settings() {
            self.set_status(format!("Cannot save configuration: {error}"), true);
            return false;
        }
        self.exclusion_editor = None;
        true
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
            self.save_or_report();
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
        self.directory_tree.reset_cursor();
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
    /// `toggle_diff_show_full` and [`App::diff_mut`]; rendering reads through
    /// [`crate::view::diff`]/[`crate::view::diff_layout_inputs`] instead of this directly.
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

    /// Replace the current user's home directory with `~` for status text
    /// and confirmation dialogs.
    pub(crate) fn display_path_with_home_tilde(path: &Path) -> String {
        if let Some(home) = crate::settings::AppSettings::home_dir() {
            if let Ok(rest) = path.strip_prefix(&home) {
                return if rest.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    let rest = rest.to_string_lossy().replace('\\', "/");
                    format!("~/{rest}")
                };
            }
        }
        path.display().to_string()
    }

    /// The currently selected filtered row, if any.
    pub(crate) fn selected_row(&self) -> Option<&FlatRow> {
        self.directory_tree.selected_row()
    }

    /// The Compared pair: the file pair, or the selected row. `None` in a
    /// Directory Tree session with nothing selected.
    pub(crate) fn compared_pair(&self) -> Option<ComparedPair<'_>> {
        match &self.file_pair {
            Some(pair) => Some(ComparedPair::Files(pair)),
            None => self.selected_row().map(|row| ComparedPair::Row {
                row,
                left_root: &self.left_path,
                right_root: &self.right_path,
            }),
        }
    }

    /// Recompute the built-in diff for the file pair File Diff shows.
    ///
    /// Returns `Err` when a side is binary, non-UTF-8, or over the size limit so
    /// callers can surface a toast instead of opening an empty/false view.
    pub fn refresh_file_diff(&mut self) -> Result<(), String> {
        let (left, right) = if let Some(pair) = &self.file_pair {
            let load = |side: &crate::target::FileSide| {
                side.load()
                    .map_err(|cause| format!("{}: {cause}", side.path().display()))
            };
            let loaded = (load(&pair.left)?, load(&pair.right)?);
            self.file_pair_info = (pair.left.info(), pair.right.info());
            loaded
        } else {
            let Some((left_file, right_file)) = self.diff_file_paths() else {
                return Err("no file selected".to_string());
            };
            // "(press D for external diff)" — or the Palette, once the keymap
            // leaves it unbound — names the way out of a file this view
            // cannot show (Issue #339).
            let external_diff_hint = match self
                .keymap
                .key_phrase(crate::commands::Command::ExternalDiff)
            {
                Some(key) => format!(" (press {key} for external diff)"),
                None => " (external diff from the Command Palette)".to_string(),
            };
            let load = |path: &Path| {
                crate::diff_view::LoadedText::from_path(path, &external_diff_hint)
                    .map_err(|e| e.to_string())
            };
            (load(&left_file)?, load(&right_file)?)
        };
        self.diff.load(left, right);
        Ok(())
    }

    /// Open File Diff on a file pair named on the command line (Issue #327).
    ///
    /// The session has no Directory Tree, so leaving File Diff ends it.
    pub fn open_file_pair(&mut self, pair: crate::target::FilePair) -> Result<(), String> {
        self.file_pair = Some(pair);
        self.diff.set_show_full(false);
        self.refresh_file_diff()?;
        self.view_mode = ViewMode::FileDiff;
        self.diff.reset_scroll();
        Ok(())
    }

    /// The file pair named on the command line, when this session compares two
    /// files rather than two directories.
    pub(crate) fn file_pair(&self) -> Option<&crate::target::FilePair> {
        self.file_pair.as_ref()
    }

    /// Size and modification time of each file-pair side, as last loaded.
    pub(crate) fn file_pair_info(&self) -> (Option<&FileInfo>, Option<&FileInfo>) {
        (
            self.file_pair_info.0.as_ref(),
            self.file_pair_info.1.as_ref(),
        )
    }

    /// The two files File Diff shows: the file pair named on the command line,
    /// or the selected row under each root. `None` when there is neither.
    pub(crate) fn diff_file_paths(&self) -> Option<(PathBuf, PathBuf)> {
        self.compared_pair().map(|pair| pair.paths())
    }

    /// Plan an external diff of the Compared pair with the tool the settings
    /// pick from those detected at startup, or say why it cannot run. The gate
    /// and the launch read the same plan, so a tool the gate offered is the
    /// tool that runs (ADR-0003).
    pub(crate) fn plan_external_diff(&self) -> Result<DiffPlan, DiffRefusal> {
        let pair = self.compared_pair();
        if pair.is_some_and(|pair| !pair.can_reopen()) {
            return Err(DiffRefusal::ReadFromPipe);
        }
        let Some(pair) = pair.filter(|pair| pair.are_files()) else {
            return Err(DiffRefusal::NotBothFiles);
        };
        let Some(tool) = self.resolve_effective_diff_tool() else {
            return Err(match &self.settings.external_diff_tool {
                crate::settings::DiffToolSetting::Disabled => DiffRefusal::Disabled,
                crate::settings::DiffToolSetting::Auto => DiffRefusal::NoTool,
                crate::settings::DiffToolSetting::Pinned(_)
                | crate::settings::DiffToolSetting::Unknown(_) => DiffRefusal::ToolMissing,
            });
        };
        let (left, right) = pair.paths();
        Ok(DiffPlan { tool, left, right })
    }

    /// The file the external editor opens: the focused side of the Compared
    /// pair, when that side is a file.
    pub(crate) fn plan_editor(&self) -> Option<PathBuf> {
        let pair = self.compared_pair()?;
        let left = self.active_side_left;
        pair.has_file(left).then(|| {
            let (l, r) = pair.paths();
            if left {
                l
            } else {
                r
            }
        })
    }

    /// What a pending confirmation applies to, so an answer is refused when the
    /// selection moved underneath it. A file pair never moves.
    pub(crate) fn confirmation_subject(&self) -> Option<PathBuf> {
        self.compared_pair().map(|pair| pair.subject())
    }

    /// Flip full-file vs. diff-only content in the diff view, at the
    /// configured context size.
    pub fn toggle_diff_show_full(&mut self) {
        self.diff.toggle_show_full();
    }

    /// Open the built-in File Diff view on the Compared pair. On a load
    /// failure the current view stays and the reason comes back for the
    /// caller to report; the BuiltinDiff gate has already checked the row is
    /// a file.
    pub fn enter_file_diff(&mut self) -> Result<(), String> {
        self.diff.set_show_full(false);
        self.refresh_file_diff()?;
        self.view_mode = ViewMode::FileDiff;
        self.diff.reset_scroll();
        Ok(())
    }

    /// Leave the File Diff view and return to the Directory Tree, or end the
    /// session when File Diff was opened directly on a file pair.
    ///
    /// Shared by Esc/`q`, the mouse close glyph, the post-copy return-to-tree, and
    /// the command palette's "back" action.
    pub fn leave_file_diff(&mut self) {
        if self.file_pair.is_some() {
            self.request_quit();
        } else {
            self.view_mode = ViewMode::DirectoryTree;
        }
    }

    /// Stage the change hunk at the current scroll position in the given
    /// direction. Nothing is written — `[` / `]` edit the working buffers and an
    /// explicit save commits them (Issue #235).
    ///
    /// Returns whether the hunk actually changed a buffer, so the caller can
    /// report "nothing to stage" rather than a save that never happened
    /// (Issue #315) — an ordinary hunk always does, but one whose sides are
    /// already byte-identical (a stale index, or a hunk `stage_hunk_copy`
    /// resolved without changing anything) does not.
    pub fn stage_hunk_at_cursor(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        self.diff.stage_active_hunk(direction)
    }

    /// Undo the most recent staged hunk operation.
    pub fn undo_staged_hunk(&mut self) -> bool {
        self.diff.undo_staged()
    }

    /// The entries the scan listed under `relative_path` on one side, as
    /// `(path relative to that directory, is_dir)` in parent-before-child order.
    ///
    /// `None` when the row is not a directory in the scan model, in which case
    /// the caller copies the single leaf instead. Driving directory copy from
    /// this snapshot is what keeps excluded entries (`.git`, …) and files
    /// created after the scan out of the copy (Issue #235).
    pub fn scanned_subtree_entries(
        &self,
        relative_path: &Path,
        from_left: bool,
    ) -> Option<Vec<(PathBuf, bool)>> {
        let root = self.directory_tree.root_node()?;
        let node = find_node(root, relative_path)?;
        let present = if from_left {
            node.left.as_ref()
        } else {
            node.right.as_ref()
        };
        if !present.is_some_and(|f| f.is_dir) {
            return None;
        }
        let mut entries = Vec::new();
        collect_scanned_entries(node, relative_path, from_left, &mut entries);
        Some(entries)
    }

    /// Relative path of the currently selected filtered row, if any.
    pub fn selected_relative_path(&self) -> Option<PathBuf> {
        self.selected_row().map(|r| r.relative_path.clone())
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

        let left_path = self.left_path.clone();
        let right_path = self.right_path.clone();
        let precise_mode = self.precise_mode();
        let new_node = crate::diff::align_directories(
            &left_path,
            &right_path,
            &scan_rel,
            precise_mode,
            &mut self.left_ignore_matcher,
            &mut self.right_ignore_matcher,
            &mut |_| {},
        )?;

        if !self.directory_tree.graft_subtree(&scan_rel, new_node) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "subtree path not found in tree",
            ));
        }
        Ok(())
    }

    /// The background scan: in flight or not, progress, generation.
    pub(crate) fn scan(&self) -> &ScanState {
        &self.scan
    }

    /// Drive the background scan's own state.
    pub(crate) fn scan_mut(&mut self) -> &mut ScanState {
        &mut self.scan
    }

    /// Which pane has focus.
    pub(crate) fn active_side_left(&self) -> bool {
        self.active_side_left
    }

    pub(crate) fn focus_left_pane(&mut self) {
        self.active_side_left = true;
    }

    pub(crate) fn focus_right_pane(&mut self) {
        self.active_side_left = false;
    }

    pub(crate) fn toggle_active_side(&mut self) {
        self.active_side_left = !self.active_side_left;
    }

    /// The Directory Tree: its tree, expand state, rows, filter, and cursor.
    pub(crate) fn directory_tree(&self) -> &DirectoryTreeState {
        &self.directory_tree
    }

    /// Drive the Directory Tree. Each operation leaves it consistent.
    pub(crate) fn directory_tree_mut(&mut self) -> &mut DirectoryTreeState {
        &mut self.directory_tree
    }

    /// Open the confirm modal with a prompt and the action to run if accepted.
    /// Show a confirmation. `Commands` composes the prompt; `App` holds it as
    /// the data the renderer draws (ADR-0003).
    pub(crate) fn show_confirm(&mut self, modal: ConfirmModal) {
        self.confirm_modal = Some(modal);
    }

    /// Open a yes/no confirmation with a single body line, for tests that need
    /// a pending dialog without caring which Command opened it.
    #[cfg(test)]
    pub fn request_confirm(&mut self, message: impl Into<String>, action: ConfirmAction) {
        self.confirm_modal = Some(ConfirmModal {
            title: "Confirm".to_string(),
            headline: message.into(),
            lines: Vec::new(),
            choices: vec![
                ConfirmChoice {
                    key: 'y',
                    label: "Yes".to_string(),
                    action,
                },
                ConfirmChoice {
                    key: 'n',
                    label: "No".to_string(),
                    action: ConfirmAction::Cancel,
                },
            ],
        });
    }

    /// Absolute, cwd-resolved, lexically normalized form of `path`.
    ///
    /// Deliberately not canonicalized: resolving symlinks would show the user a
    /// different identity from the one the copy actually writes (Issue #235).
    fn absolute_lexical(path: &Path) -> PathBuf {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        crate::actions::normalize_lexically(&joined)
    }

    /// Plan a copy in `direction`, or say why it cannot run. The one place a
    /// copy's preconditions live: the gate, the confirmation, and the effect
    /// all read the plan (ADR-0003).
    pub(crate) fn plan_copy(&self, direction: CopyDirection) -> Result<CopyPlan, CopyRefusal> {
        let left_to_right = direction == CopyDirection::LeftToRight;
        let pair = self.compared_pair().ok_or(CopyRefusal::NoSelection)?;
        if pair.is_ambiguous() {
            return Err(CopyRefusal::AmbiguousCaseCollision);
        }
        if !pair.has_content(left_to_right) {
            return Err(CopyRefusal::NothingToCopy);
        }
        if !pair.is_writable(!left_to_right) {
            return Err(CopyRefusal::ReadOnly);
        }
        if self.view_mode == ViewMode::FileDiff && self.diff.is_dirty() {
            return Err(CopyRefusal::StagedChangesUnsaved);
        }
        let target = match pair {
            ComparedPair::Files(_) => {
                if self.diff.left_hash() == self.diff.right_hash() {
                    return Err(CopyRefusal::AlreadyIdentical);
                }
                CopyTarget::FilePair
            }
            ComparedPair::Row { row, .. } => {
                // The synthetic root row stands for the whole tree.
                if row.relative_path.as_os_str().is_empty() {
                    return Err(CopyRefusal::NothingToCopy);
                }
                if row.state == crate::diff::DiffState::Identical && !row.has_case_conflict {
                    return Err(CopyRefusal::AlreadyIdentical);
                }
                let (src_rel, dst_rel, src_name, dst_name, src_root, dst_root) = if left_to_right {
                    (
                        row.left_relative_path(),
                        row.right_relative_path(),
                        row.left_name(),
                        row.right_name(),
                        &self.left_path,
                        &self.right_path,
                    )
                } else {
                    (
                        row.right_relative_path(),
                        row.left_relative_path(),
                        row.right_name(),
                        row.left_name(),
                        &self.right_path,
                        &self.left_path,
                    )
                };
                CopyTarget::Entry {
                    relative_path: row.relative_path.clone(),
                    source_name: src_name.to_string(),
                    destination_name: dst_name.to_string(),
                    source: src_root.join(src_rel),
                    destination: dst_root.join(dst_rel),
                    destination_root: dst_root.clone(),
                    case_mismatch: row.has_case_conflict && src_name != dst_name,
                    source_is_dir: row
                        .side(left_to_right)
                        .as_ref()
                        .is_some_and(|info| info.is_dir),
                }
            }
        };
        Ok(CopyPlan { direction, target })
    }

    /// Describe what `plan` would do to its destination, for the confirmation.
    ///
    /// The facts only — the operation, both absolute paths, and whether the two
    /// sides spell the name differently. `Commands` turns them into the prompt
    /// the user reads (Issue #284). Paths are deliberately not canonicalized:
    /// resolving symlinks would show a different identity from the one the copy
    /// actually writes (Issue #235). Reads the destination's metadata, so it
    /// runs when a copy is requested, never while drawing.
    pub(crate) fn copy_preview(&self, plan: &CopyPlan) -> CopyPreview {
        match &plan.target {
            CopyTarget::FilePair => {
                let pair = self
                    .file_pair
                    .as_ref()
                    .expect("a file-pair plan comes from a file-pair session");
                let left_to_right = plan.direction == CopyDirection::LeftToRight;
                let (source, destination) = (pair.side(left_to_right), pair.side(!left_to_right));
                CopyPreview {
                    kind: CopyKind::Overwrite,
                    source_name: source.name(),
                    destination_name: destination.name(),
                    source: Self::absolute_lexical(source.path()),
                    destination: Self::absolute_lexical(destination.path()),
                    case_mismatch: false,
                }
            }
            CopyTarget::Entry {
                source_name,
                destination_name,
                source,
                destination,
                case_mismatch,
                source_is_dir,
                ..
            } => {
                let src = Self::absolute_lexical(source);
                let dst = Self::absolute_lexical(destination);
                let dst_meta = std::fs::symlink_metadata(&dst).ok();
                let dst_is_dir = dst_meta
                    .as_ref()
                    .is_some_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink());
                let kind = if dst_meta.is_none() {
                    CopyKind::Create
                } else if *source_is_dir && dst_is_dir {
                    CopyKind::Merge
                } else {
                    CopyKind::Overwrite
                };
                CopyPreview {
                    kind,
                    source_name: source_name.clone(),
                    destination_name: destination_name.clone(),
                    source: src,
                    destination: dst,
                    case_mismatch: *case_mismatch,
                }
            }
        }
    }

    /// Absolute destination paths a save would write, left side first.
    pub fn staged_save_targets(&self) -> Vec<PathBuf> {
        let Some((left_file, right_file)) = self.diff_file_paths() else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        if self.diff.left_dirty() {
            targets.push(Self::absolute_lexical(&left_file));
        }
        if self.diff.right_dirty() {
            targets.push(Self::absolute_lexical(&right_file));
        }
        targets
    }

    /// Check each dirty side against its disk baseline; returns absolute paths
    /// of files that changed on disk underneath the session.
    fn staged_conflicts(&self) -> Vec<PathBuf> {
        let Some((left_file, right_file)) = self.diff_file_paths() else {
            return Vec::new();
        };
        let mut conflicted = Vec::new();
        if self.diff.left_dirty() {
            let path = left_file;
            let on_disk = crate::diff::compute_file_sha256(&path).ok();
            if on_disk.as_deref() != self.diff.left_hash() {
                conflicted.push(Self::absolute_lexical(&path));
            }
        }
        if self.diff.right_dirty() {
            let path = right_file;
            let on_disk = crate::diff::compute_file_sha256(&path).ok();
            if on_disk.as_deref() != self.diff.right_hash() {
                conflicted.push(Self::absolute_lexical(&path));
            }
        }
        conflicted
    }

    /// Write every dirty side, all-or-nothing.
    ///
    /// Each side is staged into a temporary file in its own directory first, so
    /// the visible file is only replaced once both writes are known to have
    /// worked; a failure part-way restores the originals rather than leaving one
    /// side written and the other not (Issue #235).
    ///
    /// Returns [`StagedSave::Conflicted`] with the offending paths when the
    /// on-disk content no longer matches the session baseline; nothing is
    /// written and the caller decides what to ask the user.
    pub fn save_staged(&mut self) -> Result<StagedSave, std::io::Error> {
        if !self.diff.is_dirty() {
            return Ok(StagedSave::Written);
        }
        let conflicted = self.staged_conflicts();
        if !conflicted.is_empty() {
            return Ok(StagedSave::Conflicted(conflicted));
        }
        let Some((left_file, right_file)) = self.diff_file_paths() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no file selected",
            ));
        };

        let mut writes: Vec<(PathBuf, String, String)> = Vec::new();
        if self.diff.left_dirty() {
            writes.push((
                left_file.clone(),
                self.diff.left_buffer().to_text(),
                self.diff.left_baseline_text(),
            ));
        }
        if self.diff.right_dirty() {
            writes.push((
                right_file.clone(),
                self.diff.right_buffer().to_text(),
                self.diff.right_baseline_text(),
            ));
        }

        crate::actions::commit_all_or_nothing(&writes)?;

        // A file-pair side that is not a regular file (the null device, a pipe)
        // was never written and has no file to hash again, so it keeps its hash.
        let pair = self.file_pair.as_ref();
        let rehash = |regular: bool, path: &Path, previous: Option<&str>| {
            if regular {
                crate::diff::compute_file_sha256(path).ok()
            } else {
                previous.map(str::to_string)
            }
        };
        let left_hash = rehash(
            pair.is_none_or(|pair| pair.left.is_regular_file()),
            &left_file,
            self.diff.left_hash(),
        );
        let right_hash = rehash(
            pair.is_none_or(|pair| pair.right.is_regular_file()),
            &right_file,
            self.diff.right_hash(),
        );
        self.diff.commit_baselines(left_hash, right_hash);
        if let Some(pair) = &self.file_pair {
            self.file_pair_info = (pair.left.info(), pair.right.info());
        }
        self.diff.recompute_rows();
        self.diff.clamp_scroll();
        Ok(StagedSave::Written)
    }

    /// Re-read both sides from disk, throwing away the staged edits.
    pub fn reload_discarding_staged(&mut self) -> Result<(), String> {
        self.refresh_file_diff()?;
        self.diff.clamp_scroll();
        Ok(())
    }

    /// Throw away staged edits without touching disk.
    pub fn discard_staged(&mut self) {
        self.diff.discard_staged();
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

    /// Open the Command Palette. Shared by `;`, `Ctrl+p`, and right-click, so all
    /// three land on the same contextual inventory: the query is cleared and the
    /// first enabled action is selected (Issue #239).
    pub(crate) fn open_palette(&mut self) {
        self.palette.open();
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Query edit: append one character, re-filter, and reselect.
    pub(crate) fn palette_type_char(&mut self, c: char) {
        self.palette.push_query_char(c);
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Query edit: drop the trailing character, re-filter, and reselect.
    pub(crate) fn palette_backspace(&mut self) {
        self.palette.pop_query_char();
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Rebuild `palette.items` from the Command inventory, keeping only the
    /// entries whose key or label contains the query — a case-insensitive
    /// substring search, not fuzzy matching. Called on every query edit and once
    /// per frame from `view::prepare_frame`.
    pub(crate) fn refresh_palette_items(&mut self) {
        let query = self.palette.query().to_lowercase();
        let items = crate::commands::inventory_entries(self)
            .into_iter()
            .filter(|a| {
                a.label.to_lowercase().contains(&query) || a.key.to_lowercase().contains(&query)
            })
            .collect();
        self.palette.set_items(items);
    }

    /// Read access for render / hit-test (mode, query, items, selected_idx).
    pub(crate) fn palette(&self) -> &PaletteState {
        &self.palette
    }

    /// Drive the palette's own state: selection, viewport, dismissal.
    pub(crate) fn palette_mut(&mut self) -> &mut PaletteState {
        &mut self.palette
    }

    /// Convenience for the many `if app.palette().visible()` guards.
    pub(crate) fn palette_visible(&self) -> bool {
        self.palette.visible()
    }
}

/// Depth-first lookup of the aligned node at `relative_path`.
fn find_node<'a>(node: &'a AlignedNode, relative_path: &Path) -> Option<&'a AlignedNode> {
    if node.relative_path == relative_path
        || node.left_relative_path.as_deref() == Some(relative_path)
        || node.right_relative_path.as_deref() == Some(relative_path)
    {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, relative_path))
}

/// Flatten `node`'s descendants that exist on the chosen side into paths
/// relative to `base`, parents before children so directories are created first.
fn collect_scanned_entries(
    node: &AlignedNode,
    base: &Path,
    from_left: bool,
    out: &mut Vec<(PathBuf, bool)>,
) {
    for child in &node.children {
        let info = if from_left {
            child.left.as_ref()
        } else {
            child.right.as_ref()
        };
        let Some(info) = info else {
            continue;
        };
        let child_path = if from_left {
            child
                .left_relative_path
                .as_deref()
                .unwrap_or(&child.relative_path)
        } else {
            child
                .right_relative_path
                .as_deref()
                .unwrap_or(&child.relative_path)
        };
        let Ok(relative) = child_path.strip_prefix(base) else {
            continue;
        };
        out.push((relative.to_path_buf(), info.is_dir));
        if info.is_dir {
            collect_scanned_entries(child, base, from_left, out);
        }
    }
}

/// Seams for tests in sibling modules, which cannot reach `App`'s private state
/// but still need to stand up a tree, a row list, or a viewport without running a
/// real scan or a real terminal.
#[cfg(test)]
impl App {
    /// Install a tree and flatten it, as [`App::apply_scan_result`] would.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.directory_tree.set_root_node(node);
        self.directory_tree.refresh_for_test(true);
    }

    /// Reflatten the installed tree and relist it.
    pub(crate) fn flatten_tree(&mut self) {
        self.directory_tree.refresh_for_test(true);
    }

    /// Relist rows a test installed with `set_flat_rows`.
    pub(crate) fn apply_filter(&mut self) {
        self.directory_tree.refresh_for_test(false);
    }

    pub(crate) fn set_active_side_left(&mut self, left: bool) {
        self.active_side_left = left;
    }

    pub(crate) fn set_view_mode(&mut self, view_mode: ViewMode) {
        self.view_mode = view_mode;
    }

    pub(crate) fn set_theme(&mut self, theme: crate::theme::ThemeChoice) {
        self.settings.theme = theme;
    }

    pub(crate) fn set_external_diff_tool(&mut self, tool: crate::settings::DiffToolSetting) {
        self.settings.external_diff_tool = tool;
    }

    pub(crate) fn set_detected_diff_tools(
        &mut self,
        tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    ) {
        self.detected_diff_tools = tools;
    }

    /// Put a staged edit on the left side without going through a hunk copy,
    /// for tests that only need the diff to read as dirty.
    pub(crate) fn stage_left_for_test(&mut self, staged: &str, baseline: &str) {
        self.diff.left = crate::diff_view::TextBuffer::from_text(staged);
        self.diff.left_baseline = crate::diff_view::TextBuffer::from_text(baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{lock_env_tests, ConfigEnvGuard};
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
            ..Default::default()
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
            name: String::new(),
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
                    name: "top_dir".to_string(),
                    relative_path: PathBuf::from("top_dir"),
                    left: Some(FileInfo {
                        is_dir: true,
                        size: 0,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![AlignedNode {
                        name: "nested.txt".to_string(),
                        relative_path: PathBuf::from("top_dir/nested.txt"),
                        left: Some(FileInfo {
                            is_dir: false,
                            size: 10,
                            modified: SystemTime::UNIX_EPOCH,
                        }),
                        right: None,
                        state: DiffState::LeftOnly,
                        children: vec![],
                        expanded_by_default: false,
                        ..Default::default()
                    }],
                    expanded_by_default: true,
                    ..Default::default()
                },
                AlignedNode {
                    name: "top_file.txt".to_string(),
                    relative_path: PathBuf::from("top_file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().set_root_node(node);
        app.flatten_tree();

        // Synthetic root is hidden; top-level entries start at depth 0
        assert_eq!(
            app.directory_tree().flat_rows().len(),
            3,
            "Expected 3 flattened rows"
        );
        assert_eq!(app.directory_tree().flat_rows()[0].name, "top_dir");
        assert_eq!(
            app.directory_tree().flat_rows()[0].depth,
            0,
            "Top-level directory depth should be 0"
        );
        assert_eq!(app.directory_tree().flat_rows()[1].name, "nested.txt");
        assert_eq!(
            app.directory_tree().flat_rows()[1].depth,
            1,
            "Child depth should be 1"
        );
        assert_eq!(app.directory_tree().flat_rows()[2].name, "top_file.txt");
        assert_eq!(
            app.directory_tree().flat_rows()[2].depth,
            0,
            "Top-level file depth should be 0"
        );
    }

    #[test]
    fn test_select_next_prev() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.apply_filter();

        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().select_next();
        assert_eq!(app.directory_tree().selected_idx(), 1);
        app.directory_tree_mut().select_next();
        assert_eq!(app.directory_tree().selected_idx(), 1); // bounds check
        app.directory_tree_mut().select_prev();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().select_prev();
        assert_eq!(app.directory_tree().selected_idx(), 0); // bounds check
    }

    #[test]
    fn test_page_down_up_moves_by_visible_height() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let rows: Vec<FlatRow> = (0..20)
            .map(|i| FlatRow {
                depth: 0,
                relative_path: PathBuf::from(format!("f{i}.txt")),
                name: format!("f{i}.txt"),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            })
            .collect();
        app.directory_tree_mut().set_flat_rows(rows);
        app.apply_filter();
        app.directory_tree.visible_height = 5; // page_step = 4

        app.directory_tree_mut().page_down();
        assert_eq!(app.directory_tree().selected_idx(), 4);
        assert_eq!(app.directory_tree().scroll_offset(), 0); // still visible within first page

        app.directory_tree_mut().page_down();
        assert_eq!(app.directory_tree().selected_idx(), 8);
        assert_eq!(app.directory_tree().scroll_offset(), 4); // selection pushed view down

        app.directory_tree_mut().page_up();
        assert_eq!(app.directory_tree().selected_idx(), 4);

        // Overshoot clamps to last row
        app.directory_tree_mut().set_selected_idx(18);
        app.directory_tree_mut().page_down();
        assert_eq!(app.directory_tree().selected_idx(), 19);

        app.directory_tree_mut().page_up();
        assert_eq!(app.directory_tree().selected_idx(), 15);

        // Empty list is a no-op
        app.directory_tree_mut().set_rows(Vec::new());
        app.directory_tree_mut().set_selected_idx(0);
        app.directory_tree_mut().page_down();
        app.directory_tree_mut().page_up();
        assert_eq!(app.directory_tree().selected_idx(), 0);
    }

    #[test]
    fn test_diff_page_down_up() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.diff_mut().set_geometry(10, 0, 30); // page_step = 9
        app.diff_mut().set_scroll(0);

        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 9);

        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 18);

        // Clamp to max scroll (30 - 10 = 20)
        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 20);

        app.diff_mut().page_up();
        assert_eq!(app.diff().scroll(), 11);

        app.diff_mut().set_scroll(3);
        app.diff_mut().page_up();
        assert_eq!(app.diff().scroll(), 0);
    }

    #[test]
    fn test_begin_scan_bumps_generation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan().generation(), 0);
        assert!(!app.scan().in_progress());

        let g1 = app.scan_mut().begin();
        assert_eq!(g1, 1);
        assert_eq!(app.scan().generation(), 1);
        assert!(app.scan().in_progress());

        let g2 = app.scan_mut().begin();
        assert_eq!(g2, 2);
        assert_eq!(app.scan().generation(), 2);
    }

    /// Issue #250: App tracks scan progress count and ticker animation frames.
    #[test]
    fn test_scan_progress_and_ticker() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan().progress_count(), 0);
        assert_eq!(app.scan().spinner_frame(), 0);

        app.scan_mut().tick();
        assert_eq!(app.scan().spinner_frame(), 1);

        let g = app.scan_mut().begin();
        assert!(app.scan().in_progress());
        assert_eq!(app.scan().progress_count(), 0);

        app.scan_mut().set_progress(75);
        assert_eq!(app.scan().progress_count(), 75);

        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: None,
            right: None,
            state: DiffState::Identical,
            children: vec![],
            ..Default::default()
        };
        assert!(app.apply_scan_result(g, node));
        assert!(!app.scan().in_progress());
        assert_eq!(app.scan().progress_count(), 0);
    }

    /// Issue #252: a finished scan seeds the footer inventory from the tree.
    #[test]
    fn test_tree_footer_summary_counts_the_scanned_tree() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.directory_tree().tree_summary(), None);

        let g = app.scan_mut().begin();
        let node = AlignedNode {
            children: vec![
                AlignedNode {
                    name: "a.txt".into(),
                    relative_path: PathBuf::from("a.txt"),
                    state: DiffState::Identical,
                    ..Default::default()
                },
                AlignedNode {
                    name: "b.txt".into(),
                    relative_path: PathBuf::from("b.txt"),
                    state: DiffState::LeftOnly,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(app.apply_scan_result(g, node));
        assert_eq!(
            app.directory_tree().tree_summary(),
            Some(crate::diff::TreeSummary {
                identical: 1,
                left_only: 1,
                ..Default::default()
            })
        );
        assert!(app.directory_tree().tree_summary().is_some());
    }

    #[test]
    fn test_expand_and_collapse_selected_directory_updates_the_flat_rows() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "dir".to_string(),
                relative_path: PathBuf::from("dir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "child.txt".to_string(),
                    relative_path: PathBuf::from("dir/child.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().set_root_node(node);
        app.flatten_tree();

        assert_eq!(app.directory_tree().flat_rows().len(), 2);
        assert_eq!(app.directory_tree().flat_rows()[0].name, "dir");
        assert_eq!(app.directory_tree().flat_rows()[1].name, "child.txt");

        // select dir and collapse it
        app.directory_tree_mut().set_selected_idx(0);
        app.directory_tree_mut().collapse_selected();

        // dir should now be collapsed, so only dir in flat_rows
        assert_eq!(app.directory_tree().flat_rows().len(), 1);
        assert_eq!(app.directory_tree().flat_rows()[0].name, "dir");

        app.directory_tree_mut().expand_selected();
        assert_eq!(app.directory_tree().flat_rows().len(), 2);
    }

    #[test]
    fn test_expand_collapse_selected() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "dir".to_string(),
                relative_path: PathBuf::from("dir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "child.txt".to_string(),
                    relative_path: PathBuf::from("dir/child.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().set_root_node(node);
        app.flatten_tree();

        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        // collapse dir
        app.directory_tree_mut().set_selected_idx(0);
        app.directory_tree_mut().collapse_selected();
        assert_eq!(app.directory_tree().flat_rows().len(), 1);

        // expand dir again
        app.directory_tree_mut().expand_selected();
        assert_eq!(app.directory_tree().flat_rows().len(), 2);
    }

    #[test]
    fn test_adjust_scroll() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_scroll_offset(2);

        // 1. visible_height == 0 does nothing
        app.directory_tree_mut().set_selected_idx(5);
        app.directory_tree_mut().adjust_scroll(0);
        assert_eq!(app.directory_tree().scroll_offset(), 2);

        // 2. selected_idx < scroll_offset -> scroll_offset becomes selected_idx
        app.directory_tree_mut().set_selected_idx(1);
        app.directory_tree_mut().adjust_scroll(5);
        assert_eq!(app.directory_tree().scroll_offset(), 1);

        // 3. selected_idx >= scroll_offset + visible_height -> scroll_offset adjusts
        app.directory_tree_mut().set_selected_idx(7);
        app.directory_tree_mut().adjust_scroll(5);
        assert_eq!(app.directory_tree().scroll_offset(), 3);

        // 4. selected_idx within view (e.g. 5) -> scroll_offset stays same
        app.directory_tree_mut().set_selected_idx(5);
        app.directory_tree_mut().adjust_scroll(5);
        assert_eq!(app.directory_tree().scroll_offset(), 3);
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
        app.directory_tree_mut().set_selected_idx(5);
        app.directory_tree_mut().set_scroll_offset(3);
        app.diff_mut().set_scroll(2);
        app.diff_mut()
            .set_hashes(Some("abc".to_string()), Some("def".to_string()));

        app.swap_paths();

        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert_eq!(app.directory_tree().scroll_offset(), 0);
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
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("alpha.txt"),
                name: "alpha.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("beta.txt"),
                name: "beta.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("gamma.txt"),
                name: "gamma.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 3);

        // Filter by "alpha"
        app.directory_tree_mut().set_pattern("alpha");
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 1);
        assert_eq!(app.directory_tree().rows()[0].name, "alpha.txt");

        // Clear filter
        app.directory_tree_mut().set_pattern("");
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 3);
    }

    #[test]
    fn test_filter_diffs_only() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("only.txt"),
                name: "only.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        assert_eq!(app.directory_tree().rows().len(), 2);
        assert!(app
            .directory_tree()
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
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("image.png"),
                name: "image.png".to_string(),
                state: DiffState::Unverified(UnverifiedReason::NotCompared),
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        assert_eq!(app.directory_tree().rows().len(), 1);
        assert_eq!(app.directory_tree().rows()[0].name, "image.png");
    }

    #[test]
    fn test_filter_pattern_and_diffs_only_combined() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_a.txt"),
                name: "diff_a.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_b.txt"),
                name: "diff_b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        // Filter by "a" + diffs only → should match "diff_a.txt" only
        app.directory_tree_mut().set_pattern("a");
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        assert_eq!(app.directory_tree().rows().len(), 1);
        assert_eq!(app.directory_tree().rows()[0].name, "diff_a.txt");
    }

    #[test]
    fn test_filter_case_insensitive() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("README.md"),
            name: "README.md".to_string(),
            state: DiffState::Identical,
            left: None,
            right: None,
            ..Default::default()
        }]);
        app.directory_tree_mut().set_pattern("readme");
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 1);
    }

    #[test]
    fn test_apply_filter_preserves_selection_and_scroll() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("c.txt"),
                name: "c.txt".to_string(),
                state: DiffState::RightOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(2);
        app.directory_tree_mut().set_scroll_offset(1);
        app.directory_tree.visible_height = 10;

        // Rebuild without changing filter criteria — keep the same row selected.
        app.apply_filter();
        assert_eq!(app.directory_tree().selected_idx(), 2);
        assert_eq!(
            app.directory_tree().rows()[app.directory_tree().selected_idx()].relative_path,
            PathBuf::from("c.txt")
        );
        assert_eq!(app.directory_tree().scroll_offset(), 1);
    }

    #[test]
    fn test_apply_filter_resets_when_selection_filtered_out() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0); // same.txt
        app.directory_tree_mut().set_scroll_offset(0);

        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        // same.txt is filtered out → fall back to top of remaining list
        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert_eq!(app.directory_tree().rows()[0].name, "diff.txt");
        assert_eq!(app.directory_tree().scroll_offset(), 0);
    }

    #[test]
    fn test_flatten_tree_preserves_selection() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: String::new(),
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
                    expanded_by_default: false,
                    ..Default::default()
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
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().set_root_node(node);
        app.flatten_tree();
        app.directory_tree_mut().set_selected_idx(1); // child_b
        app.directory_tree_mut().set_scroll_offset(1);
        app.directory_tree.visible_height = 10;

        app.flatten_tree();
        assert_eq!(app.directory_tree().selected_idx(), 1);
        assert_eq!(
            app.directory_tree().flat_rows()[app.directory_tree().selected_idx()].name,
            "child_b"
        );
        assert_eq!(app.directory_tree().scroll_offset(), 1);
    }

    #[test]
    fn test_restore_expanded_paths_after_rescan() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let old_tree = AlignedNode {
            name: String::new(),
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
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().set_root_node(old_tree);
        app.flatten_tree();
        let idx = app
            .directory_tree()
            .rows()
            .iter()
            .position(|r| r.relative_path == *"subdir/file.txt")
            .unwrap();
        app.directory_tree_mut().set_selected_idx(idx);

        let expand_states = &app.directory_tree().expanded;
        assert!(!expand_states.contains_key(&PathBuf::from("")));
        assert_eq!(expand_states.get(&PathBuf::from("subdir")), Some(&true));

        // Simulate a fresh scan result that brings the directory back collapsed.
        let new_tree = AlignedNode {
            name: String::new(),
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
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: false,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.directory_tree_mut().adopt(new_tree);

        assert!(app
            .directory_tree()
            .rows()
            .iter()
            .any(|r| r.relative_path == *"subdir/file.txt"));
        assert_eq!(
            app.directory_tree().rows()[app.directory_tree().selected_idx()].relative_path,
            PathBuf::from("subdir/file.txt")
        );
    }

    #[test]
    fn test_open_commit_cancel_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_pattern("abc");

        // DirectoryTreeState::open pre-fills input with committed pattern
        app.directory_tree_mut().open();
        assert!(app.directory_tree().active());
        assert_eq!(app.directory_tree().input(), "abc");

        // Type more
        for c in "def".chars() {
            app.directory_tree_mut().input_mut().insert(c);
        }
        assert_eq!(app.directory_tree().input(), "abcdef");

        // Cancel restores to original pattern
        app.directory_tree_mut().cancel();
        assert!(!app.directory_tree().active());
        assert_eq!(app.directory_tree().input(), "abc");
        assert_eq!(app.directory_tree().pattern(), "abc");

        // Open again and commit
        app.directory_tree_mut().open();
        app.directory_tree_mut().input_mut().set("xyz");
        app.directory_tree_mut().commit();
        assert!(!app.directory_tree().active());
        assert_eq!(app.directory_tree().pattern(), "xyz");
    }

    #[test]
    fn test_clear_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.directory_tree_mut().set_pattern("a");
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        assert_eq!(app.directory_tree().rows().len(), 0);

        app.directory_tree_mut().clear();
        assert!(app.directory_tree().pattern().is_empty());
        assert!(!app.directory_tree().diffs_only());
        assert_eq!(app.directory_tree().rows().len(), 2);
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
        // Header + Auto + Disabled + 2 tools + Updates header + CheckUpdates + Mouse header + Mouse
        // + Theme header + Theme + Diff View header + DiffContext
        // + Scan header + ScanMode + Exclusions header + two controls + provenance
        // + Key Bindings header + the read-only count
        assert_eq!(rows.len(), 21);
        assert!(matches!(
            rows[0],
            ConfigRowKind::Header("External Diff Tool")
        ));
        assert!(matches!(rows[1], ConfigRowKind::DiffToolAuto));
        assert!(matches!(rows[2], ConfigRowKind::DiffToolDisabled));
        assert!(matches!(
            rows[3],
            ConfigRowKind::DiffTool {
                idx: 0,
                available: true
            }
        ));
        assert!(matches!(
            rows[4],
            ConfigRowKind::DiffTool {
                idx: 1,
                available: false
            }
        ));
        assert!(matches!(rows[5], ConfigRowKind::Header("Updates")));
        assert!(matches!(rows[6], ConfigRowKind::CheckUpdates));
        assert!(matches!(rows[7], ConfigRowKind::Header("Mouse")));
        assert!(matches!(rows[8], ConfigRowKind::Mouse));
        assert!(matches!(rows[9], ConfigRowKind::Header("Theme")));
        assert!(matches!(rows[10], ConfigRowKind::Theme));
        assert!(matches!(rows[11], ConfigRowKind::Header("Diff View")));
        assert!(matches!(rows[12], ConfigRowKind::DiffContext));
        assert!(matches!(rows[13], ConfigRowKind::Header("Scan")));
        assert!(matches!(rows[14], ConfigRowKind::ScanMode));
        assert!(matches!(rows[15], ConfigRowKind::Header("Exclusions")));
        assert!(matches!(rows[16], ConfigRowKind::RespectGitignore));
        assert!(matches!(rows[17], ConfigRowKind::GlobalExclusions));
        assert!(matches!(rows[18], ConfigRowKind::IgnoreSources));
        assert!(matches!(rows[19], ConfigRowKind::Header("Key Bindings")));
        assert!(matches!(rows[20], ConfigRowKind::KeyBindings));

        app.config_mut().set_selected_idx(0);
        app.ensure_config_selection();
        assert_eq!(app.config().selected_idx(), 1);

        // Selectable indices: 1 (Auto), 2 (Disabled), 3 (Vim available),
        // 6 (CheckUpdates), 8 (Mouse), 10 (Theme), 12 (DiffContext),
        // 14 (ScanMode), 16 (RespectGitignore), 17 (GlobalExclusions)
        // (4 is Code unavailable -> skipped!)
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 2);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 3);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 6);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 8);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 10);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 12);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 14);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 16);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 17);
        app.config_select_next();
        assert_eq!(app.config().selected_idx(), 1);

        app.config_select_prev();
        assert_eq!(app.config().selected_idx(), 17);

        // Mouse click on unavailable tool is rejected
        assert!(!app.config_select_at(4));
        assert_eq!(app.config().selected_idx(), 17); // unchanged
    }

    #[test]
    fn config_diff_tool_selection_and_unknown_row() {
        // apply_config_selection persists, so the config dir has to be redirected.
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Auto);
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Nvim, false),
        ]);

        // Default setting is Auto
        assert_eq!(
            app.settings().external_diff_tool,
            crate::settings::DiffToolSetting::Auto
        );
        assert_eq!(
            app.resolve_effective_diff_tool(),
            Some(crate::diff_tool::ExternalDiffTool::Vim)
        );

        // Select Disabled (row 2)
        assert!(app.config_select_at(2));
        assert!(!app.apply_config_selection());
        assert_eq!(
            app.settings().external_diff_tool,
            crate::settings::DiffToolSetting::Disabled
        );
        assert_eq!(app.resolve_effective_diff_tool(), None);

        // Select Vim (row 3, available)
        assert!(app.config_select_at(3));
        assert!(!app.apply_config_selection());
        assert_eq!(
            app.settings().external_diff_tool,
            crate::settings::DiffToolSetting::Pinned(crate::diff_tool::ExternalDiffTool::Vim)
        );
        assert_eq!(
            app.resolve_effective_diff_tool(),
            Some(crate::diff_tool::ExternalDiffTool::Vim)
        );

        // If Vim becomes unavailable, Pinned(Vim) resolves to None and does not fall back
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, false),
            (crate::diff_tool::ExternalDiffTool::Nvim, true),
        ]);
        assert_eq!(app.resolve_effective_diff_tool(), None);

        // Set an unknown tool
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Unknown(
            "custom-diff".to_string(),
        ));
        let rows = app.config_rows();
        assert!(rows.contains(&ConfigRowKind::DiffToolUnknown));
        assert_eq!(app.resolve_effective_diff_tool(), None);
    }

    #[test]
    fn config_view_abbreviates_home_directory_in_ignore_sources() {
        let _guard = ConfigEnvGuard::new();
        let home = PathBuf::from(std::env::var("HOME").expect("ConfigEnvGuard sets HOME"));
        let other = PathBuf::from("/opt/other");
        let app = App::new(home.join("Notes"), other.clone());
        assert_eq!(
            App::display_path_with_home_tilde(app.left_path()),
            "~/Notes"
        );
        assert_eq!(
            App::display_path_with_home_tilde(app.right_path()),
            other.display().to_string()
        );

        let app = App::new(home, other);
        assert_eq!(App::display_path_with_home_tilde(app.left_path()), "~");
    }

    #[test]
    fn exclusion_editor_cancel_discards_all_draft_changes() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let saved = app.settings().global_exclusions.clone();
        app.open_exclusion_editor();
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!app.exclusion_editor_open());
        assert_eq!(app.settings().global_exclusions, saved);
    }

    #[test]
    fn exclusion_editor_apply_persists_rules_and_requests_one_rescan() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_exclusion_editor();
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for ch in "*.generated".chars() {
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL,)));
        assert!(!app.exclusion_editor_open());
        assert!(app
            .settings()
            .global_exclusions
            .iter()
            .any(|p| p == "*.generated"));
    }

    #[test]
    fn exclusion_editor_r_restores_builtin_defaults_into_the_draft_without_saving() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let saved = app.settings().global_exclusions.clone();
        app.open_exclusion_editor();
        for _ in 0..saved.len() {
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        }
        assert!(
            app.exclusion_editor()
                .expect("editor open")
                .draft()
                .is_empty(),
            "precondition: every rule was deleted from the draft"
        );

        assert!(!app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)));
        assert!(app.exclusion_editor_open());
        assert_eq!(
            app.exclusion_editor().expect("editor open").draft(),
            crate::settings::AppSettings::default().global_exclusions
        );
        assert_eq!(
            app.settings().global_exclusions,
            saved,
            "r must not persist until Ctrl+s"
        );
    }

    /// Issue #238: applying the Config scan-mode row persists, updates the
    /// effective mode, and asks the caller for exactly one background rescan.
    #[test]
    fn test_config_scan_mode_row_persists_and_requests_a_rescan() {
        use crate::settings::ScanMode;
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
        assert_eq!(app.saved_settings().scan_mode, ScanMode::Fast);
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
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup::from_disk(&_guard),
        );
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
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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

        let root = crate::diff::align_directories_with_shared_matcher(
            left.path(),
            right.path(),
            std::path::Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_root_node(root);
        // Expand nested so file rows are visible after flatten.
        app.directory_tree_mut()
            .set_expanded(Path::new("nested"), true);
        app.flatten_tree();
        let before_len = app.directory_tree().flat_rows().len();

        // Simulate copy left → right of b.txt (now both sides have it).
        write(right.path().join("nested/b.txt"), "only-left").unwrap();
        app.apply_incremental_rescan(std::path::Path::new("nested/b.txt"), false)
            .expect("nested incremental rescan");

        assert!(
            app.directory_tree()
                .flat_rows()
                .iter()
                .any(|r| r.relative_path == *"nested/b.txt"
                    && r.left.is_some()
                    && r.right.is_some()),
            "copied file should appear on both sides after incremental rescan"
        );
        // Unrelated root structure should still be present (not empty rebuild only).
        assert!(app.directory_tree().flat_rows().len() >= before_len);
        assert!(app
            .directory_tree()
            .root_node()
            .unwrap()
            .children
            .iter()
            .any(|c| c.name == "nested"));
    }

    #[test]
    fn test_check_updates_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
        let lock = lock_env_tests();
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
        drop(lock);

        {
            // Exercise the real write path — a file-backed session saving
            // through `apply_config_selection` — redirected to a tempdir.
            let guard = ConfigEnvGuard::new();
            let mut app = App::for_test(
                PathBuf::from("/left"),
                PathBuf::from("/right"),
                crate::startup::Startup::from_disk(&guard),
            );
            let idx = app
                .config_rows()
                .iter()
                .position(|r| matches!(r, ConfigRowKind::Mouse))
                .unwrap();
            app.config_mut().set_selected_idx(idx);
            let _ = app.apply_config_selection();
            let _ = app.apply_config_selection();
        }

        let _lock = lock_env_tests();
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
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
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
            app.saved_settings().scan_mode,
            ScanMode::Fast,
            "apply_scan_mode persists before adopting the mode"
        );
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
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_scroll(1);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        // Staged only: the working buffer changed, disk did not (Issue #235).
        assert!(app.diff().right_dirty());
        assert!(app.diff().right_buffer().to_text().contains("left-line"));
        let on_disk = read_to_string(right_dir.path().join("merge.txt")).unwrap();
        assert!(on_disk.contains("right-line"), "nothing is written yet");

        app.save_staged().expect("save should succeed");
        let right_text = read_to_string(right_dir.path().join("merge.txt")).unwrap();
        assert!(right_text.contains("left-line"));
        assert!(!right_text.contains("right-line"));
        assert!(!app.diff().is_dirty(), "a save clears the dirty state");
    }

    #[test]
    fn test_staged_hunk_undo_restores_the_working_buffers_without_writing() {
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
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 16,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 17,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().unwrap();
        app.diff_mut().set_scroll(1);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .unwrap();
        assert!(app.diff().right_dirty());
        assert!(app.diff().can_undo());

        assert!(app.undo_staged_hunk());
        assert!(!app.diff().is_dirty());
        assert!(!app.diff().can_undo());
        assert_eq!(app.diff().right_buffer().to_text(), "keep\nright-line\n");
        assert_eq!(
            read_to_string(right_dir.path().join("merge.txt")).unwrap(),
            "keep\nright-line\n"
        );
    }

    #[test]
    fn test_save_conflict_offers_only_reload_or_cancel() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
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
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 16,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 17,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().unwrap();
        app.diff_mut().set_scroll(1);
        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .unwrap();
        write(right_dir.path().join("merge.txt"), "external edit\n").unwrap();

        let StagedSave::Conflicted(paths) = app.save_staged().unwrap() else {
            panic!("an external edit must prevent writing");
        };
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("merge.txt"));
        assert!(
            app.diff().is_dirty(),
            "a conflict must keep the staged edit intact"
        );

        assert!(matches!(
            app.save_staged().unwrap(),
            StagedSave::Conflicted(_)
        ));
        app.reload_discarding_staged().unwrap();
        assert!(!app.diff().is_dirty());
        assert_eq!(app.diff().right_buffer().to_text(), "external edit\n");
    }

    #[test]
    fn test_jump_to_next_and_prev_change() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.diff_mut().set_geometry(0, 40, 0);
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

        app.diff_mut().jump_to_change(true);
        assert_eq!(app.diff().scroll(), 1);
        app.diff_mut().jump_to_change(true);
        assert_eq!(app.diff().scroll(), 2);
        app.diff_mut().jump_to_change(false);
        assert_eq!(app.diff().scroll(), 1);
    }

    /// A pane narrower than its gutter leaves no text columns. The painter
    /// then shows every line unwrapped (`wrap::lines` at width 0), so the
    /// jumps must count rows the same way to land where the highlight is.
    #[test]
    fn a_zero_width_pane_jumps_to_the_row_it_paints() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.diff_mut().set_geometry(0, 0, 0);
        app.diff_mut().set_wrap(true);
        let line = |tag, text: &str| {
            Some(DiffLine {
                tag,
                text: text.to_string(),
            })
        };
        let rows = vec![
            DiffRow::from((
                line(ChangeTag::Equal, "a long context line"),
                line(ChangeTag::Equal, "a long context line"),
            )),
            DiffRow::from((
                line(ChangeTag::Delete, "old"),
                line(ChangeTag::Insert, "new"),
            )),
        ];
        let painted = crate::diff_view::diff_row_physical_offsets(&rows, 0, true);
        app.diff_mut().set_rows(rows);

        app.diff_mut().jump_to_change(true);
        assert_eq!(app.diff().scroll(), painted[1]);
    }

    #[test]
    fn test_stage_hunk_at_cursor_targets_hunk_navigated_to_near_eof() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        // A front hunk and a tail hunk (the last line, right at EOF), with
        // enough context between them that the whole diff fits one viewport —
        // `max_scroll()` is 0, so `scroll` alone can't track which hunk
        // `N` last navigated to (Issue: trailing hunks couldn't be staged).
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        // A leading context line keeps the front hunk's offset > the initial
        // scroll (0), so the first `jump_to_change(true)` lands on it rather
        // than skipping straight to the tail hunk.
        write(
            left_dir.path().join("f.txt"),
            "keep1\nfront-old\nkeep2\nkeep3\nkeep4\nkeep5\ntail-old\n",
        )
        .unwrap();
        write(
            right_dir.path().join("f.txt"),
            "keep1\nfront-new\nkeep2\nkeep3\nkeep4\nkeep5\ntail-new\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");

        // Viewport comfortably fits the whole diff.
        app.diff_mut().set_text_frame(20, 40);
        assert_eq!(app.diff().max_scroll(), 0);

        // Navigate past the front hunk to the tail hunk.
        app.diff_mut().jump_to_change(true);
        app.diff_mut().jump_to_change(true);

        // Simulate the next event-loop iteration's prepare_frame call, which
        // used to silently clamp `scroll` (and with it, the active hunk) back
        // to the front hunk before the next keypress was handled.
        app.diff_mut().set_text_frame(20, 40);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        let right_text = app.diff().right_buffer().to_text();
        assert!(
            right_text.contains("tail-old"),
            "staging should replace the tail hunk, not the front one: {right_text:?}"
        );
        assert!(
            right_text.contains("front-new"),
            "the front hunk must stay untouched: {right_text:?}"
        );
    }

    #[test]
    fn test_stage_hunk_at_cursor_targets_hunk_navigated_to_near_eof_in_diff_only_view() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        // Same bug as `..._near_eof`, but in the default Diff Only (collapsed)
        // view — the shape the report reproduced in: a big gap of unchanged
        // lines between the two hunks collapses to a short "…" omitted row, so
        // the whole collapsed diff fits one viewport just like the Full-mode
        // case, but through the omitted-range path rather than a short file.
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let mut left_lines = vec!["keep1".to_string(), "front-old".to_string()];
        let mut right_lines = vec!["keep1".to_string(), "front-new".to_string()];
        for n in 1..=200 {
            left_lines.push(format!("keep{n}"));
            right_lines.push(format!("keep{n}"));
        }
        left_lines.push("tail-old".to_string());
        right_lines.push("tail-new".to_string());
        write(left_dir.path().join("f.txt"), left_lines.join("\n") + "\n").unwrap();
        write(
            right_dir.path().join("f.txt"),
            right_lines.join("\n") + "\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        // Default is Diff Only (show_full = false).
        app.refresh_file_diff().expect("diff should load");

        // The collapsed view (front hunk + context, an omitted gap, tail hunk
        // + context) is far shorter than the raw 203-line file.
        app.diff_mut().set_text_frame(56, 40);
        assert_eq!(app.diff().max_scroll(), 0);
        assert!(
            app.diff().rows().len() < 30,
            "the collapsed view should be much shorter than the raw file: {} rows",
            app.diff().rows().len()
        );

        app.diff_mut().jump_to_change(true);
        app.diff_mut().jump_to_change(true);
        app.diff_mut().set_text_frame(56, 40);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        let right_text = app.diff().right_buffer().to_text();
        assert!(
            right_text.contains("tail-old"),
            "staging should replace the tail hunk, not the front one"
        );
        assert!(
            right_text.contains("front-new"),
            "the front hunk must stay untouched"
        );
    }

    /// Issue #315: two real files whose last line differs only by a trailing
    /// newline — `[` must actually resolve it (not just report success while
    /// leaving the difference in place).
    #[test]
    fn test_stage_hunk_at_cursor_resolves_a_trailing_newline_only_hunk() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("f.txt"), "keep\n```").unwrap();
        write(right_dir.path().join("f.txt"), "keep\n```\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_text_frame(20, 40);

        let changed = app
            .stage_hunk_at_cursor(HunkCopyDirection::RightToLeft)
            .expect("hunk copy should not error");

        assert!(changed, "adopting the trailing newline is a real change");
        assert!(!app.diff().has_changes(), "the diff should now be empty");
        assert_eq!(
            app.diff().left_buffer().to_text(),
            app.diff().right_buffer().to_text()
        );
    }

    /// Issue #315: staging a hunk that turns out to be a no-op must not claim
    /// success — the caller relies on the returned bool to pick its toast.
    #[test]
    fn test_stage_hunk_at_cursor_reports_no_change_for_a_true_no_op() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow, HunkCopyDirection};
        use similar::ChangeTag;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("f.txt"), "same\n").unwrap();
        write(right_dir.path().join("f.txt"), "same\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_text_frame(20, 40);
        // A synthetic no-op hunk the real diff pipeline would never produce,
        // seeded directly to exercise the safety net (see
        // `stage_hunk_copy`'s own no-op test for the same shape).
        app.diff_mut().set_rows(vec![DiffRow::content(
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "same\n".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "same\n".to_string(),
            }),
            Some(0),
            Some(0),
        )]);

        let changed = app
            .stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should not error");

        assert!(!changed);
        assert!(
            !app.diff().is_dirty(),
            "a no-op must not be reported as staged"
        );
    }

    fn flat_row(name: &str) -> FlatRow {
        FlatRow {
            depth: 0,
            relative_path: PathBuf::from(name),
            name: name.to_string(),
            state: DiffState::Identical,
            left: None,
            right: None,
            ..Default::default()
        }
    }

    fn dir_node(name: &str) -> AlignedNode {
        AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: name.to_string(),
                relative_path: PathBuf::from(name),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
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
        assert!(app.directory_tree().rows().is_empty());

        let rows = vec![flat_row("a.txt"), flat_row("b.txt")];
        app.directory_tree_mut().set_rows(rows.clone());

        assert_eq!(app.directory_tree().rows().len(), 2);
        assert_eq!(app.directory_tree().rows()[0].name, "a.txt");
        assert_eq!(app.directory_tree().rows()[1].name, "b.txt");
    }

    #[test]
    fn test_selected_row_none_when_empty_or_out_of_range() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.selected_row().is_none());

        app.directory_tree_mut().set_rows(vec![flat_row("a.txt")]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.selected_row().map(|r| r.name.as_str()), Some("a.txt"));

        app.directory_tree_mut().set_selected_idx(1);
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
        assert!(app.palette().query().is_empty());
        // With no row selected the first few Directory Tree actions are gated,
        // so the selection must skip past them.
        let selected = app.palette().items()[app.palette().selected_idx()].clone();
        assert!(selected.enabled(), "{selected:?}");
        assert!(
            app.palette().items()[..app.palette().selected_idx()]
                .iter()
                .all(|a| !a.enabled()),
            "the first enabled action wins"
        );

        app.palette_type_char('x');
        assert_eq!(app.palette().query(), "x");
        app.palette_mut().close();
        assert!(!app.palette_visible());
        assert!(app.palette().query().is_empty(), "close clears query");

        app.open_palette();
        assert!(app.palette().query().is_empty(), "open clears query");
    }

    #[test]
    fn test_palette_select_next_prev_wraps() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        let keymap = crate::keymap::Keymap::default();
        app.palette_mut().set_items(vec![
            crate::commands::CommandEntry::new("A", crate::commands::Command::Help, &keymap),
            crate::commands::CommandEntry::new("B", crate::commands::Command::Quit, &keymap),
        ]);
        app.palette_mut().set_selected_idx(0);

        app.palette_mut().select_next();
        assert_eq!(app.palette().selected_idx(), 1);
        app.palette_mut().select_next();
        assert_eq!(app.palette().selected_idx(), 0, "wraps around");
        app.palette_mut().select_prev();
        assert_eq!(app.palette().selected_idx(), 1, "wraps backward");
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
                .any(|a| a.command == crate::commands::Command::Quit),
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
        assert!(app.palette().items().is_empty());

        // Fuzzy subsequence matching is explicitly not what this does.
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        assert_eq!(app.palette().query(), "");
        for c in "qit".chars() {
            app.palette_type_char(c);
        }
        assert!(
            app.palette()
                .items
                .iter()
                .all(|a| a.command != crate::commands::Command::Quit),
            "\"qit\" is a subsequence of \"quit\" but not a substring"
        );
    }

    /// Issue #239: a selection past the bottom of a long inventory scrolls into view.
    #[test]
    fn test_sync_palette_viewport_keeps_the_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        let keymap = crate::keymap::Keymap::default();
        app.palette_mut().set_items(
            (0..20)
                .map(|i| {
                    crate::commands::CommandEntry::new(
                        &format!("Action {i}"),
                        crate::commands::Command::Help,
                        &keymap,
                    )
                })
                .collect(),
        );

        app.palette_mut().set_selected_idx(0);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 0);

        // The ninth item is the first that does not fit an 8-row viewport.
        app.palette_mut().set_selected_idx(8);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 1, "scrolls just far enough");

        app.palette_mut().set_selected_idx(19);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 12);

        app.palette_mut().set_selected_idx(0);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 0, "scrolls back up");
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
    fn test_prepare_frame_tree_derives_visible_height() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // 24 rows = 1 top bar + 22 body + 1 footer; the pane's two borders are
        // not content.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 20);

        // A status toast grows the footer by one row, shrinking the body.
        app.set_status("copied", false);
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 19);
    }

    #[test]
    fn test_prepare_frame_tree_keeps_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_flat_rows((0..40).map(|i| flat_row(&format!("f{i}.txt"))).collect());
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(30);

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 20);
        assert_eq!(
            app.directory_tree().scroll_offset(),
            11,
            "selection scrolled into view"
        );
    }

    #[test]
    fn test_prepare_frame_diff_derives_geometry_from_area() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        // 24 rows = 1 header + 1 info bar + 21 body + 1 footer; 80 columns split
        // in half leaves 38 inner columns per pane, minus a 1-digit gutter (6).
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let diff = app.diff();
        assert_eq!(diff.visible_height(), 19);
        assert_eq!(diff.content_width(), 32);
        assert_eq!(diff.max_line_width(), 100);
        assert_eq!(
            diff.physical_rows(),
            1,
            "no wrapping: one logical row is one physical row"
        );
    }

    #[test]
    fn test_collapsed_diff_gutter_uses_file_line_count_not_visible_rows() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let mut left_lines = Vec::new();
        let mut right_lines = Vec::new();
        for i in 0..100 {
            if i == 50 {
                left_lines.push("left-mid");
                right_lines.push("right-mid");
            } else {
                left_lines.push("same");
                right_lines.push("same");
            }
        }
        write(
            left_dir.path().join("big.txt"),
            left_lines.join("\n") + "\n",
        )
        .unwrap();
        write(
            right_dir.path().join("big.txt"),
            right_lines.join("\n") + "\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("big.txt"),
            name: "big.txt".to_string(),
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(false);
        app.refresh_file_diff().expect("diff should load");
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));

        assert!(
            app.diff().rows().len() < 20,
            "collapsed view shows a handful of rows, not the whole file"
        );
        assert_eq!(
            app.diff().content_width(),
            30,
            "100-line files keep a 3-digit gutter (38 inner - 8) even when collapsed"
        );
    }

    #[test]
    fn test_prepare_frame_diff_counts_wrapped_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows(vec![equal_row(&"a".repeat(100)), equal_row("short")]);
        app.diff_mut().set_wrap(true);

        // 100 chars over 32 text columns (38 inner minus the gutter) wraps to 4
        // rows, plus 1 for "short".
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff().physical_rows(), 5);

        // Halving the width re-wraps: 100 chars over 12 text columns is 9 rows.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 40, 24));
        let diff = app.diff();
        assert_eq!(diff.content_width(), 12);
        assert_eq!(diff.physical_rows(), 10);
    }

    #[test]
    fn test_prepare_frame_after_resize_clamps_diff_paging() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows((0..40).map(|i| equal_row(&format!("line {i}"))).collect());

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 18, "page step is visible_height - 1");

        // Growing the terminal shows more rows at once, so the bottom of the
        // document now sits at a smaller scroll offset. The sync itself must pull
        // the current position back inside the new geometry — otherwise the next
        // page-down would appear to scroll backwards.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 40));
        assert_eq!(app.diff().visible_height(), 35);
        assert_eq!(app.diff().scroll(), 5, "clamped to 40 rows - 35 visible");
        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 5, "already at the bottom, stays put");
    }

    #[test]
    fn test_prepare_frame_clamps_horizontal_scroll_to_longest_line() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let max_h_scroll = app.diff().max_h_scroll();
        app.diff_mut().set_h_scroll(max_h_scroll);
        assert_eq!(
            app.diff().h_scroll(),
            68,
            "100 chars less the 32 text columns"
        );

        // Opening a shorter file must not leave the pane scrolled past its end.
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(50))]);
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff().h_scroll(), 18);
    }

    #[test]
    fn test_prepare_frame_ignores_help_and_config_views() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let geometry = |app: &App| {
            let diff = app.diff();
            (
                diff.visible_height(),
                diff.content_width(),
                diff.max_line_width(),
                diff.physical_rows(),
            )
        };
        let tree_viewport = geometry(&app);

        app.open_help();
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 120, 60));
        assert_eq!(
            geometry(&app),
            tree_viewport,
            "Help scrolls by its own drawn lines and must not disturb list geometry"
        );
    }

    #[test]
    fn test_apply_scan_result_updates_tree_flag_and_rows_together() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let generation = app.scan_mut().begin();
        assert!(app.scan().in_progress());

        assert!(app.apply_scan_result(generation, dir_node("root")));

        assert!(!app.scan().in_progress(), "scan is no longer in flight");
        assert_eq!(app.directory_tree().flat_rows().len(), 1);
        assert_eq!(app.directory_tree().flat_rows()[0].name, "root");
        assert_eq!(
            app.directory_tree().rows().len(),
            1,
            "filter view rebuilt too"
        );
    }

    #[test]
    fn test_apply_scan_result_ignores_stale_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.scan_mut().begin();
        app.apply_scan_result(stale, dir_node("first"));
        app.scan_mut().begin();

        assert!(!app.apply_scan_result(stale, dir_node("stale")));

        assert_eq!(
            app.directory_tree().flat_rows()[0].name,
            "first",
            "tree left untouched"
        );
        assert!(app.scan().in_progress(), "still waiting for the newer scan");
    }

    #[test]
    fn test_apply_scan_result_restores_expanded_directories() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let mut node = dir_node("root");
        node.children[0].children.push(AlignedNode {
            name: "leaf.txt".to_string(),
            relative_path: PathBuf::from("root/leaf.txt"),
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            expanded_by_default: false,
            ..Default::default()
        });

        let generation = app.scan_mut().begin();
        app.apply_scan_result(generation, node.clone());
        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        // A rescan returns the subdirectory collapsed; the expand state the user
        // had must survive.
        let mut collapsed = node;
        collapsed.children[0].expanded_by_default = false;
        let generation = app.scan_mut().begin();
        app.apply_scan_result(generation, collapsed);
        assert_eq!(
            app.directory_tree().flat_rows().len(),
            2,
            "root stayed expanded"
        );
    }

    #[test]
    fn test_fail_scan_clears_flag_only_for_current_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.scan_mut().begin();
        app.scan_mut().begin();

        assert!(!app.scan_mut().finish(stale));
        assert!(app.scan().in_progress(), "stale failure changes nothing");

        let current = app.scan().generation();
        assert!(app.scan_mut().finish(current));
        assert!(!app.scan().in_progress());
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
        assert_eq!(modal.headline, "Copy foo.txt to right side?");
        assert!(modal.lines.is_empty());
        assert_eq!(modal.default_action(), Some(ConfirmAction::CopyLeftToRight));
        assert_eq!(modal.cancel_action(), Some(ConfirmAction::Cancel));
    }

    #[test]
    fn test_copy_preview_left_to_right_describes_a_create_when_left_is_present() {
        // Real roots, so the absolute paths the preview builds are the same
        // shape on every platform.
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_flat_rows(vec![{
            let mut row = flat_row_with_sides(Some(file_info(false)), None);
            row.name = "foo.txt".to_string();
            row.state = crate::diff::DiffState::LeftOnly;
            row
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        let preview = app.copy_preview(&app.plan_copy(CopyDirection::LeftToRight).unwrap());

        assert_eq!(preview.kind, CopyKind::Create);
        assert_eq!(preview.source_name, "foo.txt");
        assert_eq!(preview.source, left.path().join("entry"));
        assert_eq!(preview.destination, right.path().join("entry"));
        assert!(!preview.case_mismatch);
    }

    #[test]
    fn test_copy_preview_right_to_left_describes_a_create_when_right_is_present() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_flat_rows(vec![{
            let mut row = flat_row_with_sides(None, Some(file_info(false)));
            row.name = "bar.txt".to_string();
            row.state = crate::diff::DiffState::RightOnly;
            row
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        let preview = app.copy_preview(&app.plan_copy(CopyDirection::RightToLeft).unwrap());

        assert_eq!(preview.kind, CopyKind::Create);
        assert_eq!(preview.source_name, "bar.txt");
        assert_eq!(preview.source, right.path().join("entry"));
        assert_eq!(preview.destination, left.path().join("entry"));
    }

    #[test]
    fn test_plan_copy_refuses_when_the_source_side_is_missing() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![flat_row_with_sides(None, Some(file_info(false)))]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        // Right-only row: copying left-to-right has nothing to copy from.
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NothingToCopy)
        );
    }

    #[test]
    fn test_plan_copy_refuses_when_nothing_is_selected() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NoSelection)
        );
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
        assert!(!app.directory_tree().diffs_only());

        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        assert!(
            app.directory_tree().editing_diffs_only(),
            "the badge follows the draft straight away"
        );
        assert!(
            !app.directory_tree().diffs_only(),
            "but the committed flag is untouched until Enter"
        );

        app.directory_tree_mut().commit();
        assert!(app.directory_tree().diffs_only());
        assert!(app.directory_tree().editing_diffs_only());

        // Toggling it back off and cancelling restores the committed value.
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        assert!(!app.directory_tree().editing_diffs_only());
        app.directory_tree_mut().cancel();
        assert!(app.directory_tree().diffs_only());
        assert!(app.directory_tree().editing_diffs_only());
    }

    #[test]
    fn test_filter_input_mut_allows_key_by_key_editing() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().input_mut().insert('a');
        app.directory_tree_mut().input_mut().insert('b');

        assert_eq!(app.directory_tree().input(), "ab");
    }

    /// Issue #247: Pressing Enter on an identical binary file emits an actionable status toast
    /// instead of silently failing with no feedback.
    #[test]
    fn test_enter_file_diff_on_identical_binary_file_explains_why() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let bin_content = b"PNG\0\r\n\x1a\n\0\0\0\rIHDR";
        write(left_dir.path().join("image.png"), bin_content).unwrap();
        write(right_dir.path().join("image.png"), bin_content).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state: crate::diff::DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: bin_content.len() as u64,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: bin_content.len() as u64,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();

        let error = app
            .enter_file_diff()
            .expect_err("Binary files cannot be opened in built-in diff");
        assert_eq!(
            app.view_mode(),
            ViewMode::DirectoryTree,
            "View mode should stay on DirectoryTree"
        );
        // The reason comes back for the BuiltinDiff Command to report as its
        // failure, rather than as a toast written from here.
        assert_eq!(app.status_toast(), None);
        assert!(
            error.contains("binary file not supported"),
            "the reason should explain binary file not supported: {error}"
        );
        assert!(
            error.contains("image.png"),
            "the reason should mention filename: {error}"
        );
        assert!(
            error.contains("press D for external diff"),
            "the reason should suggest pressing D: {error}"
        );
    }

    #[test]
    fn test_enter_file_diff_on_identical_text_file_opens_diff_view() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("doc.txt"), "hello world\n").unwrap();
        write(right_dir.path().join("doc.txt"), "hello world\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("doc.txt"),
            name: "doc.txt".to_string(),
            state: crate::diff::DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();

        let opened = app.enter_file_diff().is_ok();
        assert!(opened, "Identical text file should open in diff view");
        assert_eq!(
            app.view_mode(),
            ViewMode::FileDiff,
            "View mode should change to FileDiff"
        );
    }

    #[test]
    fn test_empty_tree_produces_no_flat_rows_and_none_selection() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let root = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            state: DiffState::Identical,
            children: vec![],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(root);

        assert!(app.directory_tree().flat_rows().is_empty());
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);
        assert_eq!(app.selected_relative_path(), None);
        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert_eq!(app.directory_tree().scroll_offset(), 0);
    }

    #[test]
    fn test_empty_tree_navigation_and_actions_are_noops() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let root = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(root);

        // Navigation does not change 0 indices or panic
        app.directory_tree_mut().select_next();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().select_prev();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().page_down();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().page_up();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert!(!app.directory_tree_mut().select_row_at(0));
        assert!(!app.directory_tree_mut().select_row_at(5));

        // Diff and edit actions refuse on empty selection
        assert!(app.enter_file_diff().is_err());
        assert!(app.refresh_file_diff().is_err());
        assert!(app
            .stage_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight)
            .is_err());

        // Expand/collapse actions are safe no-ops
        app.directory_tree_mut().expand_selected();
        app.directory_tree_mut().collapse_selected();
        assert!(app.directory_tree().flat_rows().is_empty());

        // Copy actions have nothing to plan
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NoSelection)
        );
        assert_eq!(
            app.plan_copy(CopyDirection::RightToLeft),
            Err(CopyRefusal::NoSelection)
        );
    }

    #[test]
    fn test_plan_copy_refuses_the_synthetic_root_row() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // Even if a synthetic root row were manually injected into flat_rows
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from(""),
            name: String::new(),
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NothingToCopy),
            "the synthetic root is never a copy target"
        );
    }

    #[test]
    fn test_filtering_complete_tree_hides_all_entries_without_exposing_root() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            state: DiffState::Identical,
            children: vec![
                AlignedNode {
                    name: "alpha.txt".to_string(),
                    relative_path: PathBuf::from("alpha.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    state: DiffState::Identical,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
                AlignedNode {
                    name: "beta.txt".to_string(),
                    relative_path: PathBuf::from("beta.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    state: DiffState::Identical,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(node);
        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        // Filter for nonexistent pattern
        app.directory_tree_mut().set_pattern("gamma");
        app.apply_filter();
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);
        assert_eq!(app.directory_tree().selected_idx(), 0);

        // Filter diffs-only when all entries are identical
        app.directory_tree_mut().clear();
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        app.apply_filter();
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);

        // Filtering with pattern "" and diffs-only disabled should match all real entries, not the root
        app.directory_tree_mut().clear();
        app.directory_tree_mut().set_pattern("");
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 2);
        assert_eq!(app.directory_tree().rows()[0].name, "alpha.txt");
        assert_eq!(app.directory_tree().rows()[1].name, "beta.txt");
    }

    #[test]
    fn test_filter_searches_full_tree_ignoring_collapse_state() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "collapsed_folder".to_string(),
                relative_path: PathBuf::from("collapsed_folder"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "deep_target.txt".to_string(),
                    relative_path: PathBuf::from("collapsed_folder/deep_target.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: false, // Collapsed in tree view!
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(node);
        // In tree view, collapsed_folder is collapsed so only 1 row visible
        assert_eq!(app.directory_tree().flat_rows().len(), 1);

        // Filter for "target"
        app.directory_tree_mut().set_pattern("target");
        app.apply_filter();
        // Even though parent was collapsed, full tree was searched and matching row is found!
        assert_eq!(app.directory_tree().rows().len(), 1);
        assert_eq!(app.directory_tree().rows()[0].name, "deep_target.txt");
        assert_eq!(
            app.directory_tree().rows()[0].relative_path,
            PathBuf::from("collapsed_folder/deep_target.txt")
        );
        assert_eq!(app.directory_tree().rows()[0].depth, 0); // Filter results are flat depth 0
    }

    #[test]
    fn test_filter_diffs_only_retains_content_identical_case_conflicts() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            state: DiffState::Identical,
            children: vec![
                AlignedNode {
                    name: "FILE.txt".to_string(),
                    left_name: Some("FILE.txt".to_string()),
                    right_name: Some("file.txt".to_string()),
                    relative_path: PathBuf::from("FILE.txt"),
                    left_relative_path: Some(PathBuf::from("FILE.txt")),
                    right_relative_path: Some(PathBuf::from("file.txt")),
                    has_case_conflict: true,
                    contains_case_conflict: true,
                    state: DiffState::Identical, // Content is identical!
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
                AlignedNode {
                    name: "regular.txt".to_string(),
                    relative_path: PathBuf::from("regular.txt"),
                    has_case_conflict: false,
                    state: DiffState::Identical,
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(node);

        // Turn on diffs-only filter
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        app.apply_filter();

        // Regular identical file is filtered out, but case-conflict identical file is retained!
        assert_eq!(app.directory_tree().rows().len(), 1);
        assert_eq!(app.directory_tree().rows()[0].name, "FILE.txt");
        assert!(app.directory_tree().rows()[0].has_case_conflict);
    }

    #[test]
    fn test_request_copy_rejects_ambiguous_case_collision() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            name: "Foo".to_string(),
            relative_path: PathBuf::from("Foo"),
            is_ambiguous_case_collision: true,
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        // Attempting to copy ambiguous collision to right side
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::AmbiguousCaseCollision)
        );
    }

    /// A both-sided node at `path`; a directory when `children` is `Some`.
    fn tree_entry(path: &str, expanded: bool, children: Option<Vec<AlignedNode>>) -> AlignedNode {
        let is_dir = children.is_some();
        AlignedNode {
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            relative_path: PathBuf::from(path),
            left: Some(file_info(is_dir)),
            right: Some(file_info(is_dir)),
            state: DiffState::Identical,
            expanded_by_default: expanded,
            children: children.unwrap_or_default(),
            ..Default::default()
        }
    }

    fn tree_root(children: Vec<AlignedNode>) -> AlignedNode {
        AlignedNode {
            left: Some(file_info(true)),
            right: Some(file_info(true)),
            expanded_by_default: true,
            children,
            ..Default::default()
        }
    }

    /// `first.txt`, then `a/` holding `a/b/` holding `a/b/deep.txt`, then
    /// `top.txt`; every directory expanded, as the scanner returns both-sided
    /// ones.
    fn nested_tree() -> AlignedNode {
        tree_root(vec![
            tree_entry("first.txt", false, None),
            tree_entry(
                "a",
                true,
                Some(vec![tree_entry(
                    "a/b",
                    true,
                    Some(vec![tree_entry("a/b/deep.txt", false, None)]),
                )]),
            ),
            tree_entry("top.txt", false, None),
        ])
    }

    fn listed_paths(app: &App) -> Vec<String> {
        app.directory_tree()
            .rows()
            .iter()
            // Joined with `/` so the expectations read the same on Windows.
            .map(|row| {
                row.relative_path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect()
    }

    fn selected_path(app: &App) -> PathBuf {
        app.selected_relative_path().expect("a row is selected")
    }

    /// Issue #338: a full rescan hands back both-sided directories expanded, so
    /// one the user collapsed must be put back collapsed, not only the other way.
    #[test]
    fn a_rescan_keeps_a_collapsed_directory_collapsed() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_root_node(nested_tree());
        app.directory_tree_mut()
            .set_expanded(Path::new("a/b"), false);
        app.flatten_tree();

        let generation = app.scan_mut().begin();
        let mut rescanned = nested_tree();
        rescanned.children.push(tree_entry(
            "new",
            true,
            Some(vec![tree_entry("new/x.txt", false, None)]),
        ));
        assert!(app.apply_scan_result(generation, rescanned));
        app.flatten_tree();

        assert_eq!(
            listed_paths(&app),
            ["first.txt", "a", "a/b", "top.txt", "new", "new/x.txt"],
            "a/b stays collapsed; a directory new to the tree keeps the scanner's default"
        );
    }

    /// Issue #338: the partial rescan after a copy grafts a freshly scanned
    /// subtree, which must not reopen a directory the user collapsed.
    #[test]
    fn an_incremental_rescan_keeps_a_collapsed_directory_collapsed() {
        use std::fs::{create_dir_all, write};
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        create_dir_all(left.path().join("nested/inner")).unwrap();
        create_dir_all(right.path().join("nested/inner")).unwrap();
        write(left.path().join("nested/inner/a.txt"), "left").unwrap();

        let root = crate::diff::align_directories_with_shared_matcher(
            left.path(),
            right.path(),
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_root_node(root);
        app.directory_tree_mut()
            .set_expanded(Path::new("nested/inner"), false);
        app.flatten_tree();

        write(right.path().join("nested/inner/a.txt"), "left").unwrap();
        app.apply_incremental_rescan(Path::new("nested"), true)
            .expect("nested incremental rescan");

        assert_eq!(listed_paths(&app), ["nested", "nested/inner"]);
    }

    /// Issue #338: collapse all leaves only the root's entries listed and keeps
    /// the cursor on the nearest ancestor the collapse left visible.
    #[test]
    fn collapse_all_lists_the_top_level_and_moves_the_cursor_to_an_ancestor() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_root_node(nested_tree());
        app.flatten_tree();
        let deep = listed_paths(&app)
            .iter()
            .position(|path| path == "a/b/deep.txt")
            .unwrap();
        app.directory_tree_mut().set_selected_idx(deep);

        app.directory_tree_mut().set_all_expanded(false);

        assert_eq!(listed_paths(&app), ["first.txt", "a", "top.txt"]);
        assert_eq!(selected_path(&app), PathBuf::from("a"));

        app.directory_tree_mut().set_all_expanded(true);

        assert_eq!(
            listed_paths(&app),
            ["first.txt", "a", "a/b", "a/b/deep.txt", "top.txt"]
        );
        assert_eq!(
            selected_path(&app),
            PathBuf::from("a"),
            "expanding keeps the cursor where it was"
        );
    }

    /// Issue #338: expand all opens one-sided directories too, which the scanner
    /// otherwise returns collapsed.
    #[test]
    fn expand_all_opens_one_sided_directories() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let mut only_left = tree_entry(
            "gone",
            false,
            Some(vec![tree_entry("gone/x.txt", false, None)]),
        );
        only_left.right = None;
        only_left.state = DiffState::LeftOnly;
        app.directory_tree_mut()
            .set_root_node(tree_root(vec![only_left]));
        app.flatten_tree();
        assert_eq!(listed_paths(&app), ["gone"]);

        app.directory_tree_mut().set_all_expanded(true);

        assert_eq!(listed_paths(&app), ["gone", "gone/x.txt"]);
    }

    /// `first.txt` (identical), `a/` and `a/b/` collapsed and `≠` only through
    /// `a/b/deep.txt`, a left-only `gone/` holding `gone/x.txt`, an unverified
    /// `maybe.txt`, and a differing `top.txt`.
    fn tree_with_differences() -> AlignedNode {
        let differing = |mut node: AlignedNode| {
            node.state = DiffState::DifferentNewerLeft;
            node
        };
        let mut gone = tree_entry(
            "gone",
            false,
            Some(vec![tree_entry("gone/x.txt", false, None)]),
        );
        gone.right = None;
        gone.state = DiffState::LeftOnly;
        gone.children[0].right = None;
        gone.children[0].state = DiffState::LeftOnly;
        let mut maybe = tree_entry("maybe.txt", false, None);
        maybe.state = DiffState::Unverified(crate::diff::UnverifiedReason::NotCompared);
        tree_root(vec![
            tree_entry("first.txt", false, None),
            differing(tree_entry(
                "a",
                false,
                Some(vec![differing(tree_entry(
                    "a/b",
                    false,
                    Some(vec![differing(tree_entry("a/b/deep.txt", false, None))]),
                ))]),
            )),
            gone,
            maybe,
            differing(tree_entry("top.txt", false, None)),
        ])
    }

    /// Issue #338: the jumps walk the whole tree in order, stopping at the
    /// differences themselves — not the `≠` directories above them, not inside
    /// a one-sided directory, not at unverified rows — and wrap around.
    #[test]
    fn difference_jumps_stop_at_each_difference_and_expand_its_parents() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_root_node(tree_with_differences());
        app.flatten_tree();
        assert_eq!(
            listed_paths(&app),
            ["first.txt", "a", "gone", "maybe.txt", "top.txt"]
        );

        assert!(app.directory_tree_mut().jump_to_difference(true));
        assert_eq!(selected_path(&app), PathBuf::from("a/b/deep.txt"));
        assert_eq!(
            listed_paths(&app),
            [
                "first.txt",
                "a",
                "a/b",
                "a/b/deep.txt",
                "gone",
                "maybe.txt",
                "top.txt"
            ],
            "the directories above the stop are expanded"
        );

        let mut forward = Vec::new();
        for _ in 0..3 {
            assert!(app.directory_tree_mut().jump_to_difference(true));
            forward.push(selected_path(&app));
        }
        assert_eq!(
            forward,
            [
                PathBuf::from("gone"),
                PathBuf::from("top.txt"),
                PathBuf::from("a/b/deep.txt")
            ]
        );

        assert!(app.directory_tree_mut().jump_to_difference(false));
        assert_eq!(selected_path(&app), PathBuf::from("top.txt"));
    }

    /// Issue #338: under a filter the jumps stay within the listed rows and
    /// leave the expand state alone.
    #[test]
    fn difference_jumps_stay_within_a_filtered_list() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_root_node(tree_with_differences());
        app.flatten_tree();
        app.directory_tree_mut().set_pattern("a/b");
        app.apply_filter();
        assert_eq!(listed_paths(&app), ["a/b", "a/b/deep.txt"]);

        assert!(app.directory_tree_mut().jump_to_difference(true));
        assert_eq!(selected_path(&app), PathBuf::from("a/b/deep.txt"));
        assert!(
            !app.directory_tree()
                .flat_rows()
                .iter()
                .any(|row| row.relative_path == *"a/b"),
            "a filtered jump expands nothing"
        );

        app.directory_tree_mut().set_pattern("first");
        app.apply_filter();
        assert!(
            !app.directory_tree_mut().jump_to_difference(true),
            "no stop among the listed rows"
        );
    }

    /// Under a filter the jumps stop where they stop without one: a one-sided
    /// directory is the difference as a whole, not each entry inside it.
    #[test]
    fn a_filtered_jump_skips_the_inside_of_a_one_sided_directory() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_root_node(tree_with_differences());
        app.flatten_tree();
        app.directory_tree_mut().set_diffs_only(true);
        app.apply_filter();
        assert!(listed_paths(&app).contains(&"gone/x.txt".to_string()));

        let mut stops = Vec::new();
        for _ in 0..3 {
            assert!(app.directory_tree_mut().jump_to_difference(true));
            stops.push(selected_path(&app));
        }
        assert_eq!(
            stops,
            [
                PathBuf::from("a/b/deep.txt"),
                PathBuf::from("gone"),
                PathBuf::from("top.txt")
            ]
        );
    }

    /// The Palette offers the jumps only when one would move: under a filter,
    /// a difference the filter hides is not one to jump to.
    #[test]
    fn a_filter_listing_no_difference_has_none_to_jump_to() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut()
            .set_root_node(tree_with_differences());
        app.flatten_tree();
        assert!(app.directory_tree().has_difference());

        app.directory_tree_mut().set_pattern("first");
        app.apply_filter();
        assert!(!app.directory_tree().has_difference());

        app.directory_tree_mut().set_pattern("gone");
        app.apply_filter();
        assert!(app.directory_tree().has_difference());
    }

    /// Issue #342: a config file that fails to parse is named in a startup
    /// toast, and no later save overwrites it with the defaults.
    #[test]
    fn a_broken_config_file_is_reported_and_never_overwritten() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        let broken = "theme = \"blue\"\n";
        std::fs::write(&path, broken).unwrap();

        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );

        let (toast, is_error) = app.status_toast().expect("a startup toast");
        assert!(is_error);
        assert!(
            toast.starts_with("Cannot load the config file: "),
            "{toast}"
        );
        assert!(toast.contains("line 1: "), "{toast}");

        app.toggle_theme();
        assert!(app
            .apply_scan_mode(crate::settings::ScanMode::Precise)
            .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    /// A settings change that cannot be saved says so, and still applies for
    /// this session. The broken config file here refuses every save (#342).
    #[test]
    fn a_setting_that_cannot_be_saved_says_so() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        std::fs::write(&path, "theme = \"blue\"\n").unwrap();
        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );
        let refused = |app: &App| {
            let (toast, is_error) = app.status_toast().expect("a toast");
            assert!(is_error, "{toast}");
            assert!(toast.starts_with("Cannot save configuration: "), "{toast}");
        };

        let theme = app.settings().theme;
        app.toggle_theme();
        refused(&app);
        assert_ne!(app.settings().theme, theme, "the change still applies");

        let mouse = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(mouse);
        let mouse_before = app.settings().mouse;
        app.set_status("", false);
        let _ = app.apply_config_selection();
        refused(&app);
        assert_ne!(
            app.settings().mouse,
            mouse_before,
            "the change still applies"
        );
    }

    /// Issue #339: `[keys]` from the config file drives the App's keymap, an
    /// ignored entry is named in a startup toast, and a save writes the section
    /// back as the user wrote it — the ignored entry included.
    #[test]
    fn config_keys_reach_the_keymap_and_survive_a_save() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        std::fs::write(
            &path,
            "theme = \"light\"\n\n[keys]\ncopy_to_left = \"R\"\ncopy_to_right = \"L\"\nrescan = \"j\"\nhelp = \"x\"\n",
        )
        .unwrap();

        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );

        assert_eq!(
            app.keymap().hint(crate::commands::Command::CopyRightToLeft),
            "R"
        );
        assert_eq!(app.keymap().hint(crate::commands::Command::Refresh), "r");
        assert_eq!(app.keymap().customized_count(), 3);
        assert_eq!(
            app.status_toast(),
            Some((
                "Some key bindings in the config file were ignored: keys.rescan: `j` is handled \
                 by Directory Tree itself and cannot be bound — Fix those [keys] entries; \
                 ignored commands keep their default keys",
                true
            ))
        );

        app.toggle_theme();
        let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(saved["theme"].as_str(), Some("dark"));
        let expected: toml::Table =
            "copy_to_left = \"R\"\ncopy_to_right = \"L\"\nrescan = \"j\"\nhelp = \"x\"\n"
                .parse()
                .unwrap();
        assert_eq!(saved["keys"].as_table(), Some(&expected));
    }

    #[test]
    fn several_ignored_key_bindings_point_at_check() {
        let problems = ["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            crate::startup::key_problem(&problems)
                .map(|p| p.toast())
                .as_deref(),
            Some(
                "Some key bindings in the config file were ignored: a (+2 more — run duodiff \
                 --check) — Fix those [keys] entries; ignored commands keep their default keys"
            )
        );
    }
}
