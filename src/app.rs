//! The session's state: [`App`] and the sub-states it composes.
//!
//! Each sub-state with an interface of its own lives in a child module under
//! `app/` beside its tests, and is re-exported here; `App` keeps what spans
//! them (ADR-0002).

mod directory_tree;
mod file_diff;
mod help;
mod palette;

pub use directory_tree::{DirectoryTreeState, FlatRow};
pub use file_diff::FileDiffState;
pub use help::{HelpState, HelpTopic};
pub use palette::PaletteState;

use crate::diff::{AlignedNode, FileInfo};
use crate::ignore::IgnoreMatcher;
#[cfg(test)]
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// Work a change leaves for the event loop, which owns the scan task and
/// the terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Rescan,
    /// Scan only the directory at this path and graft it into the tree.
    RescanSubtree(PathBuf),
    /// Turn the terminal's mouse capture on or off.
    MouseCapture(bool),
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
    scan: crate::scan::ScanState,
    view_mode: ViewMode,
    diff: FileDiffState,
    settings: crate::settings::SettingsState,
    /// Work a change left for the event loop, oldest first.
    requests: Vec<Request>,
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
    update_available: Option<String>,
    /// Where a finished update check records itself.
    update_check_store: crate::upgrade::UpdateCheckStore,
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
        let crate::startup::Startup {
            settings,
            store,
            keymap,
            detected_diff_tools,
            install_method,
            overrides,
            update_available,
            update_check_store,
            ..
        } = startup;

        Self {
            left_path: left,
            right_path: right,
            file_pair: None,
            file_pair_info: (None, None),
            scan: crate::scan::ScanState::default(),
            view_mode: ViewMode::DirectoryTree,
            diff: FileDiffState::with_context(settings.diff_context),
            settings: crate::settings::SettingsState::new(settings, store, &overrides),
            requests: Vec::new(),
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
            update_available,
            update_check_store,
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
        self.settings.stored().expect("settings were saved")
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

    /// Take a progress report from background scan `generation`.
    pub fn apply_scan_progress(&mut self, generation: u64, count: usize) {
        self.scan.progress(generation, count);
    }

    /// Report a failed background scan, unless a newer scan superseded it.
    pub fn apply_scan_error(&mut self, generation: u64, message: &str) {
        if self.scan.finish(generation) {
            self.set_status(format!("Scan failed: {message}"), true);
        }
    }

    /// Apply a finished background update check: record it in the store
    /// and show or clear the newer version. A failed check stays silent and
    /// leaves both alone, so the next launch retries at once.
    pub fn apply_update_check_outcome(&mut self, outcome: crate::upgrade::UpdateCheckOutcome) {
        self.update_check_store
            .record(&outcome, crate::upgrade::now_secs());
        match outcome {
            crate::upgrade::UpdateCheckOutcome::Newer(version) => {
                self.update_available = Some(version);
            }
            crate::upgrade::UpdateCheckOutcome::UpToDate => self.update_available = None,
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

    /// Put `mode` in effect without saving it, as `--scan-mode` does, for
    /// tests.
    #[cfg(test)]
    pub(crate) fn set_scan_mode(&mut self, mode: crate::settings::ScanMode) {
        self.settings.set_scan_mode(mode);
    }

    /// The one scan-mode switch behind the Directory Tree `c` key, the Palette,
    /// and the Config screen, which reports the new mode in a toast.
    pub fn switch_scan_mode(&mut self, mode: crate::settings::ScanMode) {
        self.set_status(format!("Scan mode: {}", mode.label()), false);
        self.change_setting(crate::settings::SettingChange::ScanMode(mode));
    }

    pub fn ignore_matchers(&self) -> (&IgnoreMatcher, &IgnoreMatcher) {
        (&self.left_ignore_matcher, &self.right_ignore_matcher)
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
        match &self.settings.saved().external_diff_tool {
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
        if self
            .settings
            .saved()
            .external_diff_tool
            .unknown_name()
            .is_some()
        {
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

    /// The Settings this session runs with. Changes go through
    /// [`App::change_setting`].
    pub fn settings(&self) -> &crate::settings::SettingsState {
        &self.settings
    }

    pub(crate) fn detected_diff_tools(&self) -> &[(crate::diff_tool::ExternalDiffTool, bool)] {
        &self.detected_diff_tools
    }

    /// Put a Settings change in effect, save it, and carry out what else it
    /// affects: new ignore matchers, and a rescan left for the event loop.
    /// A save that fails is reported; the change lasts until duodiff exits.
    ///
    /// Returns `false`, with an error toast and nothing changed, when the
    /// ignore matchers cannot be built for the change.
    pub(crate) fn change_setting(&mut self, change: crate::settings::SettingChange) -> bool {
        let matchers = if change.reshapes_ignore_rules() {
            let rules = self.settings.ignore_rules_after(&change);
            let built = rules
                .matcher(self.left_path.clone())
                .and_then(|left| Ok((left, rules.matcher(self.right_path.clone())?)));
            match built {
                Ok(matchers) => Some(matchers),
                Err(error) => {
                    self.set_status(format!("Cannot rebuild exclusions: {error}"), true);
                    return false;
                }
            }
        } else {
            None
        };
        let applied = self.settings.apply(change);
        if let Some((left, right)) = matchers {
            self.left_ignore_matcher = left;
            self.right_ignore_matcher = right;
        }
        match applied.effect {
            crate::settings::SettingEffect::None => {}
            crate::settings::SettingEffect::Rescan => self.request_rescan(),
            crate::settings::SettingEffect::MouseCapture(on) => {
                self.request(Request::MouseCapture(on))
            }
            crate::settings::SettingEffect::DiffContext(lines) => self.diff.set_context(lines),
        }
        if let Err(error) = applied.saved {
            self.set_status(format!("Cannot save configuration: {error}"), true);
        }
        true
    }

    /// The terminal refused to switch mouse capture to `wanted`: keep what
    /// it still does in effect, and say so.
    pub(crate) fn mouse_capture_failed(&mut self, wanted: bool, error: std::io::Error) {
        self.settings.set_mouse_in_effect(!wanted);
        self.set_status(format!("Cannot switch mouse support: {error}"), true);
    }

    /// Ask the event loop for a background scan of both roots. A session on
    /// a file pair has no directories to scan, so it never asks (Issue #327).
    pub(crate) fn request_rescan(&mut self) {
        self.request(Request::Rescan);
    }

    /// Ask for the tree to follow a copy of `copied` (a directory when
    /// `copied_is_dir`): a background scan of just the directory it landed in.
    ///
    /// Any other scan in flight or asked for would supersede that one, losing
    /// either it or the copy, so then — and for a copy at the root — the
    /// whole tree is scanned instead.
    pub(crate) fn request_subtree_rescan(&mut self, copied: &Path, copied_is_dir: bool) {
        let path = if copied_is_dir {
            copied.to_path_buf()
        } else {
            copied.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        let busy = self.scan.in_progress()
            || self
                .requests
                .iter()
                .any(|request| matches!(request, Request::Rescan | Request::RescanSubtree(_)));
        if path.as_os_str().is_empty() || busy {
            self.request_rescan();
        } else {
            self.request(Request::RescanSubtree(path));
        }
    }

    /// Apply a finished background scan of the directory at `path`: graft
    /// it into the tree, or scan the whole tree when the directory is no
    /// longer in it. A result from a superseded scan is dropped.
    pub fn apply_subtree_scan_result(&mut self, generation: u64, path: &Path, node: AlignedNode) {
        if !self.scan.finish(generation) {
            return;
        }
        if !self.directory_tree.graft_subtree(path, node) {
            self.request_rescan();
        }
    }

    /// Leave `request` for the event loop, once. A whole-tree rescan
    /// replaces a subtree one, which it covers.
    fn request(&mut self, request: Request) {
        if matches!(request, Request::Rescan | Request::RescanSubtree(_))
            && self.file_pair.is_some()
        {
            return;
        }
        if request == Request::Rescan {
            self.requests
                .retain(|queued| !matches!(queued, Request::RescanSubtree(_)));
        }
        if !self.requests.contains(&request) {
            self.requests.push(request);
        }
    }

    /// The work changes left for the event loop since it last asked.
    pub(crate) fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// The work changes have left for the event loop, for tests.
    #[cfg(test)]
    pub(crate) fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// Flip between the dark and light theme and persist the choice.
    pub fn toggle_theme(&mut self) {
        let theme = self.settings.saved().theme.toggled();
        self.set_status(format!("Theme: {}", theme.label()), false);
        self.change_setting(crate::settings::SettingChange::Theme(theme));
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
    /// Unlike `App::help_mut`/`tree_list_mut`, every selection mutator needs
    /// the row list from [`App::config_rows`], so production code goes through
    /// an `App` orchestration method for those; frame preparation reaches the
    /// scroll through here, and tests seed a selection directly.
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

    /// Act on the selected Config row: switch it, choose it, or open its
    /// editor.
    pub fn apply_config_selection(&mut self) {
        use crate::settings::{DiffToolSetting, SettingChange};
        let rows = self.config_rows();
        let saved = self.settings.saved();
        let change = match rows.get(self.config.selected_idx()) {
            Some(ConfigRowKind::ScanMode) => {
                return self.switch_scan_mode(self.settings.scan_mode().toggled());
            }
            Some(ConfigRowKind::RespectGitignore) => {
                SettingChange::RespectGitignore(!self.settings.respect_gitignore())
            }
            Some(ConfigRowKind::GlobalExclusions) => return self.open_exclusion_editor(),
            Some(ConfigRowKind::DiffToolAuto) => {
                SettingChange::ExternalDiffTool(DiffToolSetting::Auto)
            }
            Some(ConfigRowKind::DiffToolDisabled) => {
                SettingChange::ExternalDiffTool(DiffToolSetting::Disabled)
            }
            Some(ConfigRowKind::DiffTool {
                idx,
                available: true,
            }) => match self.detected_diff_tools.get(*idx) {
                Some((tool, _)) => SettingChange::ExternalDiffTool(DiffToolSetting::Pinned(*tool)),
                None => return,
            },
            Some(ConfigRowKind::CheckUpdates) => SettingChange::CheckUpdates(!saved.check_updates),
            Some(ConfigRowKind::Mouse) => SettingChange::Mouse(!self.settings.mouse()),
            Some(ConfigRowKind::Theme) => return self.toggle_theme(),
            _ => return,
        };
        self.change_setting(change);
    }

    pub(crate) fn open_exclusion_editor(&mut self) {
        self.exclusion_editor = Some(ExclusionEditorState {
            draft: self.settings.saved().global_exclusions.clone(),
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

    pub(crate) fn exclusion_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        let Some(editor) = self.exclusion_editor.as_mut() else {
            return;
        };
        match editor.handle_key(key) {
            ExclusionEditorAction::None => {}
            ExclusionEditorAction::Cancel => self.exclusion_editor = None,
            ExclusionEditorAction::Apply => self.apply_exclusion_editor(),
        }
    }

    fn apply_exclusion_editor(&mut self) {
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
                return;
            }
        }
        if self.change_setting(crate::settings::SettingChange::GlobalExclusions(draft)) {
            self.exclusion_editor = None;
        }
    }

    /// Nudge a numeric config field (currently only [`ConfigRowKind::DiffContext`]) up
    /// or down by one and persist. No-op for non-numeric rows.
    pub fn adjust_config_selection(&mut self, forward: bool) {
        let rows = self.config_rows();
        if let Some(ConfigRowKind::DiffContext) = rows.get(self.config.selected_idx()) {
            let lines = self.settings.saved().diff_context;
            let lines = if forward {
                lines.saturating_add(1)
            } else {
                lines.saturating_sub(1)
            };
            self.change_setting(crate::settings::SettingChange::DiffContext(lines));
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
        // Each matcher reads its own root's ignore files (Issue #237).
        std::mem::swap(
            &mut self.left_ignore_matcher,
            &mut self.right_ignore_matcher,
        );
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
            return Err(match &self.settings.saved().external_diff_tool {
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

    /// The background scan: in flight or not, progress, generation.
    pub(crate) fn scan(&self) -> &crate::scan::ScanState {
        &self.scan
    }

    /// Drive the background scan's own state.
    pub(crate) fn scan_mut(&mut self) -> &mut crate::scan::ScanState {
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

    /// Show a confirmation. `Commands` composes the prompt and shows it
    /// together with the approval it waits for; `App` holds it as the data
    /// the renderer draws (ADR-0003).
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
        crate::write::normalize_lexically(&joined)
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

        crate::write::commit_all_or_nothing(&writes)?;

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

    /// Close the confirm modal. `Commands` closes it as it settles an answer,
    /// together with the approval it was waiting for.
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
        self.settings.saved_mut().theme = theme;
    }

    pub(crate) fn set_external_diff_tool(&mut self, tool: crate::settings::DiffToolSetting) {
        self.settings.saved_mut().external_diff_tool = tool;
    }

    pub(crate) fn set_detected_diff_tools(
        &mut self,
        tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    ) {
        self.detected_diff_tools = tools;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{
        dir_node, equal_row, file_info, flat_row, flat_row_with_sides, listed_paths, nested_tree,
        tree_entry,
    };
    use crate::test_support::{lock_env_tests, ConfigEnvGuard};
    use std::time::SystemTime;

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

        app.apply_scan_progress(g, 75);
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

    /// Each frame hands Help its topic's size, so the bound follows the
    /// terminal and the topic on screen.
    #[test]
    fn a_frame_bounds_the_help_topic_scroll() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_help();
        app.help_mut().select_topic(HelpTopic::DirectoryTree);
        let area = ratatui::layout::Rect::new(0, 0, 100, 24);
        crate::view::prepare_frame(&mut app, area);
        for _ in 0..500 {
            app.help_mut().move_down();
        }
        // Top bar, footer, and the body's two borders leave 20 rows.
        let lines = crate::view::help_lines(&app).len();
        assert_eq!(usize::from(app.help().scroll()), lines - 20);
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

    /// A session on a file pair has no directories, so nothing it does asks
    /// for a scan; a directory session asks once however often it is asked.
    #[test]
    fn only_a_directory_session_requests_a_rescan() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_rescan();
        app.request_rescan();
        assert_eq!(app.requests(), [Request::Rescan]);

        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&left, "a\n").unwrap();
        std::fs::write(&right, "b\n").unwrap();
        let crate::target::ComparisonTarget::Files(pair) =
            crate::target::resolve(&left, &right).unwrap()
        else {
            panic!("two files resolve to a file pair");
        };
        let mut app = App::new(left, right);
        app.open_file_pair(pair).unwrap();
        app.request_rescan();
        assert!(app.requests().is_empty());
    }

    /// Each root keeps its own ignore rules across a swap: a rule in the old
    /// left root's `.duodiffignore` hides nothing in the new left root.
    #[test]
    fn swap_paths_keeps_each_roots_ignore_rules() {
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join(".duodiffignore"), "secret.txt\n").unwrap();
        let matcher =
            |root: &PathBuf| IgnoreMatcher::for_root(root.clone(), &[], true, &[]).unwrap();
        let mut app = App::from_startup(
            left.clone(),
            right.clone(),
            matcher(&left),
            matcher(&right),
            crate::startup::Startup::for_test(),
        );

        app.swap_paths();

        let (new_left, new_right) = app.ignore_matchers();
        let secret = Path::new("secret.txt");
        assert!(!new_left.clone().is_ignored(secret, false).unwrap());
        assert!(new_right.clone().is_ignored(secret, false).unwrap());
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
            app.settings().saved().external_diff_tool,
            crate::settings::DiffToolSetting::Auto
        );
        assert_eq!(
            app.resolve_effective_diff_tool(),
            Some(crate::diff_tool::ExternalDiffTool::Vim)
        );

        // Select Disabled (row 2)
        assert!(app.config_select_at(2));
        app.apply_config_selection();
        assert!(app.take_requests().is_empty());
        assert_eq!(
            app.settings().saved().external_diff_tool,
            crate::settings::DiffToolSetting::Disabled
        );
        assert_eq!(app.resolve_effective_diff_tool(), None);

        // Select Vim (row 3, available)
        assert!(app.config_select_at(3));
        app.apply_config_selection();
        assert!(app.take_requests().is_empty());
        assert_eq!(
            app.settings().saved().external_diff_tool,
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
        let saved = app.settings().saved().global_exclusions.clone();
        app.open_exclusion_editor();
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!app.exclusion_editor_open());
        assert_eq!(app.settings().saved().global_exclusions, saved);
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

        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(app.take_requests(), [Request::Rescan]);
        assert!(!app.exclusion_editor_open());
        assert!(app
            .settings()
            .saved()
            .global_exclusions
            .iter()
            .any(|p| p == "*.generated"));
    }

    #[test]
    fn exclusion_editor_r_restores_builtin_defaults_into_the_draft_without_saving() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let saved = app.settings().saved().global_exclusions.clone();
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

        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(app.requests().is_empty());
        assert!(app.exclusion_editor_open());
        assert_eq!(
            app.exclusion_editor().expect("editor open").draft(),
            crate::settings::AppSettings::default().global_exclusions
        );
        assert_eq!(
            app.settings().saved().global_exclusions,
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
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        app.apply_config_selection();
        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "a scan-mode change needs a rescan"
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        assert_eq!(app.saved_settings().scan_mode, ScanMode::Fast);
        assert!(!app.settings().scan_mode_is_session_override());

        // Rows that do not affect scanning never ask for a rescan.
        let theme_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(theme_idx);
        app.apply_config_selection();
        assert!(app.take_requests().is_empty());
    }

    /// A scan mode that cannot be saved still takes effect and rescans, like
    /// every other setting; the toast says it lasts only this session.
    #[cfg(unix)]
    #[test]
    fn test_scan_mode_save_failure_still_switches_and_rescans() {
        use crate::settings::ScanMode;
        use std::os::unix::fs::PermissionsExt;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup::from_disk(&_guard),
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);

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
        app.apply_config_selection();

        // Restore before asserting so a failure cannot leave the tempdir locked.
        std::fs::set_permissions(&path, original).unwrap();

        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "the new mode rescans even unsaved"
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        let (msg, is_error, _) = app.status_message.clone().unwrap();
        assert!(is_error, "{msg}");
        assert!(msg.starts_with("Cannot save configuration: "), "{msg}");
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
        app.apply_config_selection();
        assert_eq!(app.take_requests(), [Request::Rescan]);
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);

        app.close_config();
        assert_eq!(app.view_mode(), ViewMode::FileDiff);
        assert_eq!(app.diff().rows().len(), 1, "the diff session is preserved");
    }

    #[test]
    fn test_mouse_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.settings().saved().mouse);
        assert!(!app.settings().mouse());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.apply_config_selection();
        assert!(app.settings().saved().mouse);
        assert!(app.settings().mouse());
        assert_eq!(app.take_requests(), [Request::MouseCapture(true)]);

        app.apply_config_selection();
        assert!(!app.settings().saved().mouse);
        assert!(!app.settings().mouse());
        assert_eq!(app.take_requests(), [Request::MouseCapture(false)]);
    }

    /// `--no-mouse` only sets where the session starts: turning mouse
    /// support on in Config turns it on, and saves it.
    #[test]
    fn the_mouse_row_replaces_no_mouse() {
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                overrides: crate::startup::CliOverrides {
                    no_mouse: true,
                    ..Default::default()
                },
                ..crate::startup::Startup::for_test()
            },
        );
        assert!(app.settings().saved().mouse);
        assert!(!app.settings().mouse());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.apply_config_selection();

        assert!(app.settings().mouse());
        assert!(app.saved_settings().mouse);
        assert_eq!(app.take_requests(), [Request::MouseCapture(true)]);
    }

    /// `--no-gitignore` only sets where the session starts: the Config row
    /// switches what the next scan reads, and saves it.
    #[test]
    fn the_gitignore_row_replaces_no_gitignore() {
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                overrides: crate::startup::CliOverrides {
                    gitignore: Some(false),
                    ..Default::default()
                },
                ..crate::startup::Startup::for_test()
            },
        );
        assert!(!app.settings().respect_gitignore());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::RespectGitignore))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.apply_config_selection();

        assert!(app.settings().respect_gitignore());
        assert!(app.saved_settings().respect_gitignore);
        assert_eq!(app.take_requests(), [Request::Rescan]);
    }

    #[test]
    fn test_theme_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Light
        );
        assert_eq!(app.settings().theme(), crate::theme::Theme::LIGHT);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.apply_config_selection();
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Dark
        );
        assert_eq!(app.settings().theme(), crate::theme::Theme::DARK);

        app.apply_config_selection();
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Light
        );
    }

    #[test]
    fn test_diff_context_adjust_persists_and_clamps() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings().saved().diff_context, 7);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        app.adjust_config_selection(true);
        assert_eq!(app.settings().saved().diff_context, 8);
        app.adjust_config_selection(false);
        app.adjust_config_selection(false);
        assert_eq!(app.settings().saved().diff_context, 6);

        // Clamped at 0 (saturating_sub), not underflowing.
        for _ in 0..10 {
            app.adjust_config_selection(false);
        }
        assert_eq!(app.settings().saved().diff_context, 0);

        // Clamped at 50.
        for _ in 0..60 {
            app.adjust_config_selection(true);
        }
        assert_eq!(app.settings().saved().diff_context, 50);
    }

    /// A File Diff open behind Config shows the new context as soon as it
    /// changes, not after the next reload.
    #[test]
    fn a_diff_context_change_redraws_the_open_file_diff() {
        let lines: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let changed = lines.replace("10\n", "ten\n");
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let load = |text: &str| crate::diff_view::LoadedText {
            text: text.to_string(),
            sha256: None,
            line_ending: None,
        };
        app.diff_mut().load(load(&lines), load(&changed));
        let rows_at_default = app.diff().rows().len();

        app.open_config();
        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.adjust_config_selection(true);

        assert_eq!(app.diff().rows().len(), rows_at_default + 2);
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
        let before = app.settings().saved().diff_context;
        app.adjust_config_selection(true);
        assert_eq!(app.settings().saved().diff_context, before);
    }

    /// Finish the background subtree scan `app` asked for, the way the scan
    /// task would, and hand the result back.
    fn finish_subtree_scan(app: &mut App, left: &Path, right: &Path) {
        let requests = app.take_requests();
        let [Request::RescanSubtree(path)] = requests.as_slice() else {
            panic!("a subtree rescan was requested: {requests:?}");
        };
        let path = path.clone();
        let generation = app.scan_mut().begin();
        let node = crate::diff::align_directories_with_shared_matcher(
            left,
            right,
            &path,
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        app.apply_subtree_scan_result(generation, &path, node);
        assert!(!app.scan().in_progress());
    }

    #[test]
    fn test_subtree_rescan_after_copying_a_nested_file() {
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
            .set_expanded_for_test(Path::new("nested"), true);
        app.flatten_tree();
        let before_len = app.directory_tree().flat_rows().len();

        // Simulate copy left → right of b.txt (now both sides have it).
        write(right.path().join("nested/b.txt"), "only-left").unwrap();
        app.request_subtree_rescan(Path::new("nested/b.txt"), false);
        assert_eq!(
            app.requests(),
            [Request::RescanSubtree(PathBuf::from("nested"))],
            "a copied file rescans its directory"
        );
        finish_subtree_scan(&mut app, left.path(), right.path());

        assert!(
            app.directory_tree()
                .flat_rows()
                .iter()
                .any(|r| r.relative_path == *"nested/b.txt"
                    && r.left.is_some()
                    && r.right.is_some()),
            "copied file should appear on both sides after the subtree rescan"
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
        assert!(!app.settings().saved().check_updates);

        // Land on CheckUpdates row and toggle.
        app.open_config();
        while !matches!(
            app.config_rows().get(app.config().selected_idx()),
            Some(ConfigRowKind::CheckUpdates)
        ) {
            app.config_select_next();
        }
        app.apply_config_selection();
        assert!(app.settings().saved().check_updates);

        app.apply_config_selection();
        assert!(!app.settings().saved().check_updates);
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
            app.apply_config_selection();
            app.apply_config_selection();
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
        let _ = app.settings().saved().external_diff_tool;
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
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);
        assert!(app.settings().scan_mode().is_precise());
        assert!(!app.settings().scan_mode_is_session_override());

        // What `--scan-mode fast` does at bootstrap.
        app.set_scan_mode(ScanMode::Fast);
        assert!(!app.settings().scan_mode().is_precise());
        assert_eq!(
            app.settings().saved().scan_mode,
            ScanMode::Precise,
            "the CLI value must not write the config file"
        );
        assert!(app.settings().scan_mode_is_session_override());

        // An in-app change persists, so effective and saved agree again.
        app.switch_scan_mode(ScanMode::Fast);
        assert_eq!(app.settings().saved().scan_mode, ScanMode::Fast);
        assert!(!app.settings().scan_mode_is_session_override());
        assert_eq!(
            app.saved_settings().scan_mode,
            ScanMode::Fast,
            "an in-app change persists"
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
                .items()
                .iter()
                .any(|a| a.command == crate::commands::Command::Quit),
            "an upper-case query must still match the lower-case label"
        );
        assert!(
            app.palette()
                .items()
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
                .items()
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
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Newer(
            "0.9.0".to_string(),
        ));
        assert_eq!(app.update_available(), Some("0.9.0"));
        let recorded = app.update_check_store.load();
        assert_eq!(recorded.latest_seen, "0.9.0");
        assert!(recorded.last_check > 0);

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::UpToDate);
        assert_eq!(app.update_available(), None);
        assert_eq!(app.update_check_store.load().latest_seen, "");

        app.set_update_available(Some("0.7.0".to_string()));
        let before = app.update_check_store.load();
        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Failed);
        assert_eq!(
            app.update_available(),
            Some("0.7.0"),
            "Failed must stay silent and leave the previous hint alone"
        );
        assert_eq!(
            app.update_check_store.load(),
            before,
            "a failed check is not recorded against the throttle"
        );
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

    /// Issue #338: a full rescan hands back both-sided directories expanded, so
    /// one the user collapsed must be put back collapsed, not only the other way.
    #[test]
    fn a_rescan_keeps_a_collapsed_directory_collapsed() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_root_node(nested_tree());
        app.directory_tree_mut()
            .set_expanded_for_test(Path::new("a/b"), false);
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
            listed_paths(app.directory_tree()),
            ["first.txt", "a", "a/b", "top.txt", "new", "new/x.txt"],
            "a/b stays collapsed; a directory new to the tree keeps the scanner's default"
        );
    }

    /// Issue #362: a copy scans its directory in the background, unless that
    /// scan would supersede another — then, as for a copy at the root, the
    /// whole tree is scanned instead.
    #[test]
    fn a_copy_rescans_its_directory_unless_another_scan_would_be_lost() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_subtree_rescan(Path::new("a.txt"), false);
        assert_eq!(app.take_requests(), [Request::Rescan], "a copy at the root");

        app.request_subtree_rescan(Path::new("dir"), true);
        app.request_subtree_rescan(Path::new("other/b.txt"), false);
        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "a second copy would lose the first one's scan"
        );

        app.scan_mut().begin();
        app.request_subtree_rescan(Path::new("dir"), true);
        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "a scan in flight would be lost"
        );
    }

    /// A subtree scan whose directory is gone from the tree by the time it
    /// finishes scans the whole tree; one a newer scan superseded is dropped.
    #[test]
    fn a_subtree_scan_result_that_cannot_be_grafted_rescans_the_tree() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_root_node(AlignedNode::default());
        let stale = app.scan_mut().begin();
        let current = app.scan_mut().begin();

        app.apply_subtree_scan_result(stale, Path::new("gone"), AlignedNode::default());
        assert!(app.requests().is_empty(), "a superseded result is dropped");
        assert!(app.scan().in_progress());

        app.apply_subtree_scan_result(current, Path::new("gone"), AlignedNode::default());
        assert!(!app.scan().in_progress());
        assert_eq!(app.requests(), [Request::Rescan]);
    }

    /// Issue #338: the partial rescan after a copy grafts a freshly scanned
    /// subtree, which must not reopen a directory the user collapsed.
    #[test]
    fn a_subtree_rescan_keeps_a_collapsed_directory_collapsed() {
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
            .set_expanded_for_test(Path::new("nested/inner"), false);
        app.flatten_tree();

        write(right.path().join("nested/inner/a.txt"), "left").unwrap();
        app.request_subtree_rescan(Path::new("nested"), true);
        finish_subtree_scan(&mut app, left.path(), right.path());

        assert_eq!(
            listed_paths(app.directory_tree()),
            ["nested", "nested/inner"]
        );
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
        app.switch_scan_mode(crate::settings::ScanMode::Precise);
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

        let theme = app.settings().saved().theme;
        app.toggle_theme();
        refused(&app);
        assert_ne!(
            app.settings().saved().theme,
            theme,
            "the change still applies"
        );

        let mouse = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(mouse);
        let mouse_before = app.settings().saved().mouse;
        app.set_status("", false);
        app.apply_config_selection();
        refused(&app);
        assert_ne!(
            app.settings().saved().mouse,
            mouse_before,
            "the change still applies"
        );
        app.take_requests();

        let gitignore = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::RespectGitignore))
            .unwrap();
        app.config_mut().set_selected_idx(gitignore);
        let respect_before = app.settings().respect_gitignore();
        app.set_status("", false);
        app.apply_config_selection();
        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "the new rules still rescan"
        );
        refused(&app);
        assert_ne!(app.settings().respect_gitignore(), respect_before);

        app.open_exclusion_editor();
        app.set_status("", false);
        app.exclusion_editor_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('s'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(
            app.take_requests(),
            [Request::Rescan],
            "the edited exclusions still rescan"
        );
        refused(&app);
        assert!(!app.exclusion_editor_open());
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

    /// The external diff and editor plans: what each Command would launch on
    /// the selected row, or why it refuses.
    mod plans {
        use crate::app::{self, App, FlatRow};
        use crate::diff::{DiffState, FileInfo};
        use crate::diff_tool::ExternalDiffTool;
        use std::path::PathBuf;
        use std::time::SystemTime;

        fn file_row(name: &str, left: bool, right: bool, is_dir: bool) -> FlatRow {
            let info = FileInfo {
                is_dir,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            };
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from(name),
                name: name.to_string(),
                state: DiffState::DifferentNewerLeft,
                left: left.then_some(info.clone()),
                right: right.then_some(info),
                ..Default::default()
            }
        }

        #[test]
        fn plan_external_diff_refuses_when_disabled() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, true, false)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::Disabled));
        }

        #[test]
        fn plan_external_diff_refuses_a_directory() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
                ExternalDiffTool::Vim,
            ));
            app.directory_tree_mut()
                .set_rows(vec![file_row("dir", true, true, true)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(
                app.plan_external_diff(),
                Err(app::DiffRefusal::NotBothFiles)
            );
        }

        #[test]
        fn plan_external_diff_refuses_a_single_sided_file() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
                ExternalDiffTool::Vim,
            ));
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, false, false)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(
                app.plan_external_diff(),
                Err(app::DiffRefusal::NotBothFiles)
            );
        }

        #[test]
        /// The tool list is the one detected at startup, which the gate and the
        /// launch both read, so a pinned tool missing then is refused up front.
        fn plan_external_diff_refuses_a_pinned_tool_missing_at_startup() {
            let _guard = crate::test_support::PathEnvGuard::set("/nonexistent_dir_123");
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

            app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
                ExternalDiffTool::Meld,
            ));
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, true, false)]);
            app.directory_tree_mut().set_selected_idx(0);

            assert_eq!(app.plan_external_diff(), Err(app::DiffRefusal::ToolMissing));
        }

        #[test]
        fn plan_external_diff_builds_paths_for_both_sided_file_when_available() {
            let temp = tempfile::tempdir().unwrap();
            let bin_dir = temp.path().join("bin");
            std::fs::create_dir_all(&bin_dir).unwrap();
            #[cfg(windows)]
            let vim_exe = bin_dir.join("vim.exe");
            #[cfg(not(windows))]
            let vim_exe = bin_dir.join("vim");
            std::fs::write(&vim_exe, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&vim_exe).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&vim_exe, perms).unwrap();
            }

            let _guard = crate::test_support::PathEnvGuard::set(&bin_dir);

            let mut app = App::for_test(
                PathBuf::from("/left"),
                PathBuf::from("/right"),
                crate::startup::Startup {
                    detected_diff_tools: crate::diff_tool::detect_diff_tools(),
                    ..crate::startup::Startup::for_test()
                },
            );
            app.set_external_diff_tool(crate::settings::DiffToolSetting::Pinned(
                ExternalDiffTool::Vim,
            ));
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, true, false)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(
                app.plan_external_diff(),
                Ok(app::DiffPlan {
                    tool: ExternalDiffTool::Vim,
                    left: PathBuf::from("/left/a.txt"),
                    right: PathBuf::from("/right/a.txt"),
                })
            );
        }

        #[test]
        fn plan_editor_refuses_a_directory() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.focus_left_pane();
            app.directory_tree_mut()
                .set_rows(vec![file_row("dir", true, false, true)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(app.plan_editor(), None);
        }

        #[test]
        fn plan_editor_follows_active_side() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, true, false)]);
            app.directory_tree_mut().set_selected_idx(0);

            app.focus_left_pane();
            assert_eq!(app.plan_editor(), Some(PathBuf::from("/left/a.txt")));

            app.focus_right_pane();
            assert_eq!(app.plan_editor(), Some(PathBuf::from("/right/a.txt")));
        }

        #[test]
        fn plan_editor_refuses_a_side_with_no_file() {
            let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            app.focus_right_pane();
            app.directory_tree_mut()
                .set_rows(vec![file_row("a.txt", true, false, false)]);
            app.directory_tree_mut().set_selected_idx(0);
            assert_eq!(app.plan_editor(), None);
        }

        #[test]
        fn plans_refuse_when_nothing_is_selected() {
            let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
            assert_eq!(
                app.plan_external_diff(),
                Err(app::DiffRefusal::NotBothFiles)
            );
            assert_eq!(app.plan_editor(), None);
        }
    }
}
