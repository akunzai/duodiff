use crate::diff::{AlignedNode, DiffState, FileInfo};
use crate::ignore::IgnoreMatcher;
use ratatui::layout::Rect;
use std::path::PathBuf;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    Menu,
    Command,
}

#[derive(Clone, Debug)]
pub struct PaletteAction {
    pub key: String,
    pub label: String,
    pub action_id: &'static str,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    pub visible: bool,
    pub mode: Option<PaletteMode>,
    pub query: String,
    pub items: Vec<PaletteAction>,
    pub selected_idx: usize,
    pub x: u16,
    pub y: u16,
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
    /// Longest line (in characters) across the current `diff_rows`.
    pub diff_max_line_width: usize,
    /// Physical (post-wrap) row count of the current `diff_rows`.
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
    pub left_path: PathBuf,
    pub right_path: PathBuf,
    pub precise_mode: bool,
    root_node: Option<AlignedNode>,
    scan_in_progress: bool,
    /// Monotonic counter bumped for every scan start. Stale `ScanFinished` /
    /// scan `Error` events with an older generation are ignored.
    scan_generation: u64,
    flat_rows: Vec<FlatRow>,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub active_side_left: bool,
    pub view_mode: ViewMode,
    diff_rows: Vec<crate::diff_view::DiffRow>,
    pub diff_scroll: usize,
    /// Terminal geometry for the current frame; see [`App::sync_viewport`].
    viewport: Viewport,
    /// When true, show the full file contents in the diff view instead of only differences.
    pub diff_show_full: bool,
    /// When true, wrap long lines in the diff view instead of truncating.
    pub diff_wrap: bool,
    /// Horizontal scroll offset (in columns) for the diff view when wrapping is off.
    pub diff_h_scroll: usize,
    /// Cached MD5 hashes for the files currently shown in the diff view.
    pub diff_left_hash: Option<String>,
    pub diff_right_hash: Option<String>,
    /// Cached line ending styles (e.g. LF, CRLF) for the files shown in the diff view.
    pub diff_left_line_ending: Option<String>,
    pub diff_right_line_ending: Option<String>,
    pub last_click_idx: Option<usize>,
    pub last_click_time: Option<std::time::Instant>,
    pub settings: crate::settings::AppSettings,
    pub detected_diff_tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    /// Selected row index in [`App::config_rows`].
    pub config_selected_idx: usize,
    pub palette: PaletteState,
    confirm_modal: Option<ConfirmModal>,
    /// Transient status toast: (message, is_error, created_at)
    pub status_message: Option<(String, bool, Instant)>,
    /// When true, key events are routed to the filter text input.
    pub filter_active: bool,
    /// Current text in the filter input bar (char-indexed cursor, CJK-safe).
    pub filter_input: crate::text_input::TextInput,
    /// Committed filter pattern applied to flat_rows (set on Enter/ESC).
    pub filter_pattern: String,
    /// When true, only show rows that differ (exclude Identical).
    pub filter_diffs_only: bool,
    /// Filtered view of flat_rows, rebuilt whenever the filter changes.
    pub filtered_rows: Vec<FlatRow>,
    /// Glob-based ignore matcher used during directory scans.
    pub ignore_matcher: IgnoreMatcher,
    pub update_check_enabled: bool,
    /// Effective mouse-capture state for this session: `settings.mouse` unless overridden
    /// by the `--no-mouse` CLI flag. See [`crate::settings::resolve_mouse_enabled`].
    pub mouse_enabled: bool,
    pub update_available: Option<String>,
    pub install_method: crate::upgrade::InstallMethod,
    pub help_topic: HelpTopic,
    pub help_return_view: ViewMode,
    pub config_return_view: ViewMode,
    pub help_index_open: bool,
    pub help_index_sel: usize,
    pub help_scroll: u16,
    pub should_quit: bool,
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
            precise_mode: false,
            root_node: None,
            scan_in_progress: false,
            scan_generation: 0,
            flat_rows: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            active_side_left: true,
            view_mode: ViewMode::DirectoryTree,
            diff_rows: Vec::new(),
            diff_scroll: 0,
            viewport: Viewport::default(),
            diff_show_full: false,
            diff_wrap: false,
            diff_h_scroll: 0,
            diff_left_hash: None,
            diff_right_hash: None,
            diff_left_line_ending: None,
            diff_right_line_ending: None,
            last_click_idx: None,
            last_click_time: None,
            settings,
            detected_diff_tools,
            config_selected_idx: 0,
            palette: PaletteState::default(),
            confirm_modal: None,
            status_message: None,
            filter_active: false,
            filter_input: crate::text_input::TextInput::default(),
            filter_pattern: String::new(),
            filter_diffs_only: false,
            filtered_rows: Vec::new(),
            ignore_matcher,
            update_check_enabled: true,
            mouse_enabled: true,
            update_available: None,
            install_method,
            help_topic: HelpTopic::General,
            help_return_view: ViewMode::DirectoryTree,
            config_return_view: ViewMode::DirectoryTree,
            help_index_open: false,
            help_index_sel: 0,
            help_scroll: 0,
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
                let layout = crate::ui::tree_layout(self, area);
                self.viewport.visible_height = layout.left.height.saturating_sub(2) as usize;
                self.adjust_scroll(self.viewport.visible_height);
            }
            ViewMode::FileDiff => {
                let layout = crate::ui::diff_layout(self, area);
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
        rows
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

    /// Open the Config screen, remembering the current view so `Esc`/`q` can return to it.
    ///
    /// No-op while already on Config — otherwise the top bar's `(C)onfig` mouse click
    /// (reachable from any view, including Config itself) would overwrite
    /// `config_return_view` with `ViewMode::ConfigMenu`, trapping Esc/`q` in Config with no
    /// way out via the keyboard (same failure mode fixed for `open_help`).
    pub fn open_config(&mut self) {
        if self.view_mode == ViewMode::ConfigMenu {
            return;
        }
        self.config_return_view = self.view_mode;
        self.view_mode = ViewMode::ConfigMenu;
        self.ensure_config_selection();
    }

    pub fn ensure_config_selection(&mut self) {
        let rows = self.config_rows();
        if rows.is_empty() {
            self.config_selected_idx = 0;
            return;
        }
        if self.config_selected_idx >= rows.len() || !rows[self.config_selected_idx].is_selectable()
        {
            self.config_selected_idx = rows.iter().position(|r| r.is_selectable()).unwrap_or(0);
        }
    }

    pub fn config_select_next(&mut self) {
        let rows = self.config_rows();
        if rows.is_empty() {
            return;
        }
        let mut next = self.config_selected_idx;
        for _ in 0..rows.len() {
            next = (next + 1) % rows.len();
            if rows[next].is_selectable() {
                self.config_selected_idx = next;
                return;
            }
        }
    }

    pub fn config_select_prev(&mut self) {
        let rows = self.config_rows();
        if rows.is_empty() {
            return;
        }
        let mut prev = self.config_selected_idx;
        for _ in 0..rows.len() {
            prev = prev.checked_sub(1).unwrap_or(rows.len() - 1);
            if rows[prev].is_selectable() {
                self.config_selected_idx = prev;
                return;
            }
        }
    }

    pub fn apply_config_selection(&mut self) {
        let rows = self.config_rows();
        match rows.get(self.config_selected_idx) {
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
    }

    /// Nudge a numeric config field (currently only [`ConfigRowKind::DiffContext`]) up
    /// or down by one and persist. No-op for non-numeric rows.
    pub fn adjust_config_selection(&mut self, forward: bool) {
        let rows = self.config_rows();
        if let Some(ConfigRowKind::DiffContext) = rows.get(self.config_selected_idx) {
            self.settings.diff_context = if forward {
                self.settings.diff_context.saturating_add(1).min(50)
            } else {
                self.settings.diff_context.saturating_sub(1)
            };
            let _ = self.settings.save();
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

    /// Swap the left and right directory paths and reset selection state.
    pub fn swap_paths(&mut self) {
        std::mem::swap(&mut self.left_path, &mut self.right_path);
        self.selected_idx = 0;
        self.scroll_offset = 0;
        self.diff_scroll = 0;
        self.diff_left_hash = None;
        self.diff_right_hash = None;
    }

    /// Clear the status message if it has been visible longer than `duration`.
    pub fn clear_expired_status(&mut self, duration: std::time::Duration) {
        if let Some((_, _, created)) = &self.status_message {
            if created.elapsed() >= duration {
                self.status_message = None;
            }
        }
    }

    /// The current file diff's rows. Read access for rendering.
    pub(crate) fn diff_rows(&self) -> &[crate::diff_view::DiffRow] {
        &self.diff_rows
    }

    /// True when the current file diff has at least one added/removed line.
    pub(crate) fn diff_has_changes(&self) -> bool {
        self.diff_rows
            .iter()
            .any(crate::diff_view::diff_row_is_change)
    }

    /// Jump to the next differing block in the diff view (wraps around).
    pub fn jump_to_next_change(&mut self) {
        let width = self.viewport.diff_content_width.max(1);
        if let Some(scroll) = crate::diff_view::jump_to_change_scroll(
            &self.diff_rows,
            self.diff_scroll,
            width,
            self.diff_wrap,
            true,
        ) {
            self.diff_scroll = scroll;
        }
    }

    /// Jump to the previous differing block in the diff view (wraps around).
    pub fn jump_to_prev_change(&mut self) {
        let width = self.viewport.diff_content_width.max(1);
        if let Some(scroll) = crate::diff_view::jump_to_change_scroll(
            &self.diff_rows,
            self.diff_scroll,
            width,
            self.diff_wrap,
            false,
        ) {
            self.diff_scroll = scroll;
        }
    }

    /// Recompute the built-in diff for the currently selected file pair.
    ///
    /// Returns `Err` when a side is binary, non-UTF-8, or over the size limit so
    /// callers can surface a toast instead of opening an empty/false view.
    pub fn refresh_file_diff(&mut self) -> Result<(), String> {
        if self.selected_idx >= self.filtered_rows.len() {
            return Err("no file selected".to_string());
        }
        let row = &self.filtered_rows[self.selected_idx];
        let left_file = self.left_path.join(&row.relative_path);
        let right_file = self.right_path.join(&row.relative_path);
        self.diff_rows = crate::diff_view::compare_files(
            &left_file,
            &right_file,
            self.diff_show_full,
            self.settings.diff_context,
        )
        .map_err(|e| e.to_string())?;
        self.diff_left_hash = crate::diff::compute_file_md5(&left_file).ok();
        self.diff_right_hash = crate::diff::compute_file_md5(&right_file).ok();
        self.diff_left_line_ending = crate::diff_view::detect_file_line_ending(&left_file);
        self.diff_right_line_ending = crate::diff_view::detect_file_line_ending(&right_file);
        self.resync_diff_geometry();
        Ok(())
    }

    /// Recompute the `diff_rows`-derived half of [`Viewport`] at the last known
    /// content width.
    ///
    /// [`App::sync_viewport`] redoes this every frame; this exists so callers that
    /// replace `diff_rows` mid-frame (loading another file, applying a hunk copy)
    /// can clamp scrolling against the new content instead of the old row count.
    fn resync_diff_geometry(&mut self) {
        self.viewport.diff_max_line_width = crate::diff_view::diff_max_line_width(&self.diff_rows);
        self.viewport.diff_physical_rows = crate::diff_view::diff_total_physical_rows(
            &self.diff_rows,
            self.viewport.diff_content_width,
            self.diff_wrap,
        );
    }

    /// Pull the diff view's scroll offsets back inside the current geometry.
    ///
    /// Growing the terminal (or opening a shorter file) can leave `diff_scroll` /
    /// `diff_h_scroll` past the end of the content; without this the next page or
    /// arrow key would appear to jump backwards.
    fn clamp_diff_scroll(&mut self) {
        self.diff_scroll = self.diff_scroll.min(self.viewport.max_diff_scroll());
        self.diff_h_scroll = self.diff_h_scroll.min(self.viewport.max_diff_h_scroll());
    }

    /// Open the built-in File Diff view for the current selection.
    /// On load failure, keeps the current view and sets an error status toast.
    pub fn enter_file_diff(&mut self) -> bool {
        if self.selected_idx >= self.filtered_rows.len() {
            return false;
        }
        let row = &self.filtered_rows[self.selected_idx];
        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
        if is_dir {
            return false;
        }
        self.diff_show_full = false;
        match self.refresh_file_diff() {
            Ok(()) => {
                self.view_mode = ViewMode::FileDiff;
                self.diff_scroll = 0;
                self.diff_h_scroll = 0;
                true
            }
            Err(e) => {
                self.set_status(format!("Cannot open diff: {e}"), true);
                false
            }
        }
    }

    /// Copy the change hunk at the current scroll position in the given direction.
    pub fn copy_hunk_at_cursor(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<(), std::io::Error> {
        if self.selected_idx >= self.filtered_rows.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no file selected",
            ));
        }
        let width = self.viewport.diff_content_width.max(1);
        let hunk_index = crate::diff_view::hunk_index_at_scroll(
            &self.diff_rows,
            self.diff_scroll,
            width,
            self.diff_wrap,
        )
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no change block at cursor",
            )
        })?;
        let row = &self.filtered_rows[self.selected_idx];
        let left_file = self.left_path.join(&row.relative_path);
        let right_file = self.right_path.join(&row.relative_path);
        let prev_scroll = self.diff_scroll;
        crate::diff_view::apply_hunk_copy(
            &left_file,
            &right_file,
            &self.diff_rows,
            hunk_index,
            direction,
        )?;
        self.refresh_file_diff().map_err(std::io::Error::other)?;
        self.diff_scroll = prev_scroll.min(self.viewport.max_diff_scroll());
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
        self.filtered_rows
            .get(self.selected_idx)
            .map(|r| r.relative_path.clone())
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
            self.precise_mode,
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

    /// Rebuild `filtered_rows` from `flat_rows` using the current filter
    /// pattern and diffs-only flag. Preserves selection and scroll position
    /// by matching the previously selected relative path when still present.
    pub fn apply_filter(&mut self) {
        let prev_path = self.selected_relative_path();
        let prev_scroll = self.scroll_offset;
        let pattern = self.filter_pattern.to_lowercase();
        let diffs_only = self.filter_diffs_only;

        if pattern.is_empty() && !diffs_only {
            self.filtered_rows = self.flat_rows.clone();
        } else {
            self.filtered_rows = self
                .flat_rows
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

        if self.filtered_rows.is_empty() {
            self.selected_idx = 0;
            self.scroll_offset = 0;
            return;
        }

        if let Some(path) = prev_path {
            if let Some(idx) = self
                .filtered_rows
                .iter()
                .position(|r| r.relative_path == path)
            {
                self.selected_idx = idx;
                let max_scroll = self.filtered_rows.len().saturating_sub(1);
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
    ///
    /// No-op while already on Help — otherwise the top bar's `(?)Help` mouse click (reachable
    /// from any view, including Help itself) would overwrite `help_return_view` with
    /// `ViewMode::Help`, trapping Esc/`?`/`q` in Help with no way out via the keyboard.
    pub fn open_help(&mut self) {
        if self.view_mode == ViewMode::Help {
            return;
        }
        self.help_return_view = self.view_mode;
        self.help_topic = HelpTopic::for_view(self.view_mode);
        self.help_index_sel = HelpTopic::all()
            .iter()
            .position(|&t| t == self.help_topic)
            .unwrap_or(0);
        self.help_index_open = false;
        self.help_scroll = 0;
        self.view_mode = ViewMode::Help;
    }

    /// Open the filter input bar, pre-filling with the committed pattern.
    pub fn open_filter(&mut self) {
        self.filter_active = true;
        self.filter_input.set(self.filter_pattern.clone());
    }

    /// Close the filter input bar, committing the typed text as the pattern.
    pub fn commit_filter(&mut self) {
        self.filter_active = false;
        self.filter_pattern = self.filter_input.to_string();
        self.apply_filter();
    }

    /// Close the filter input bar, discarding any uncommitted typing.
    pub fn cancel_filter(&mut self) {
        self.filter_active = false;
        self.filter_input.set(self.filter_pattern.clone());
    }

    /// Clear the filter entirely (pattern + diffs-only).
    pub fn clear_filter(&mut self) {
        self.filter_pattern.clear();
        self.filter_input.clear();
        self.filter_diffs_only = false;
        self.apply_filter();
    }

    pub fn toggle_expand(&mut self) {
        if self.filtered_rows.is_empty() || self.selected_idx >= self.filtered_rows.len() {
            return;
        }
        let row = &self.filtered_rows[self.selected_idx];
        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
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
        if !self.filtered_rows.is_empty() && self.selected_idx < self.filtered_rows.len() - 1 {
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
        if self.filtered_rows.is_empty() {
            return;
        }
        let max_idx = self.filtered_rows.len() - 1;
        self.selected_idx = (self.selected_idx + self.page_step()).min(max_idx);
        self.adjust_scroll(self.viewport.visible_height);
    }

    /// Move the directory-tree selection up by one page (`Ctrl+b`).
    pub fn page_up(&mut self) {
        if self.filtered_rows.is_empty() {
            return;
        }
        self.selected_idx = self.selected_idx.saturating_sub(self.page_step());
        self.adjust_scroll(self.viewport.visible_height);
    }

    /// Scroll the file-diff view down by one page (`Ctrl+f`).
    pub fn diff_page_down(&mut self) {
        self.diff_scroll =
            (self.diff_scroll + self.page_step()).min(self.viewport.max_diff_scroll());
    }

    /// Scroll the file-diff view up by one page (`Ctrl+b`).
    pub fn diff_page_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(self.page_step());
    }

    pub fn expand_selected(&mut self) {
        if self.filtered_rows.is_empty() || self.selected_idx >= self.filtered_rows.len() {
            return;
        }
        let row = &self.filtered_rows[self.selected_idx];
        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
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
        if self.filtered_rows.is_empty() || self.selected_idx >= self.filtered_rows.len() {
            return;
        }
        let row = &self.filtered_rows[self.selected_idx];
        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
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

    pub fn build_palette_actions(&self) -> Vec<PaletteAction> {
        let mut actions = Vec::new();
        match self.view_mode {
            ViewMode::DirectoryTree => {
                let row = self.filtered_rows.get(self.selected_idx);
                let is_file_pair = row.is_some_and(|r| {
                    let is_dir = r.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                        || r.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                    !is_dir && r.left.is_some() && r.right.is_some()
                });
                let is_file_active = row.is_some_and(|r| {
                    if self.active_side_left {
                        r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    } else {
                        r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    }
                });

                actions.push(PaletteAction {
                    key: "D".to_string(),
                    label: "Compare via External Diff Tool".to_string(),
                    action_id: "ext_diff",
                    enabled: is_file_pair && self.settings.external_diff_tool.is_some(),
                });
                actions.push(PaletteAction {
                    key: "E".to_string(),
                    label: "Edit via External Editor".to_string(),
                    action_id: "ext_edit",
                    enabled: is_file_active,
                });
                actions.push(PaletteAction {
                    key: "R".to_string(),
                    label: "Copy Left to Right".to_string(),
                    action_id: "copy_l2r",
                    enabled: row.is_some_and(|r| r.left.is_some()),
                });
                actions.push(PaletteAction {
                    key: "L".to_string(),
                    label: "Copy Right to Left".to_string(),
                    action_id: "copy_r2l",
                    enabled: row.is_some_and(|r| r.right.is_some()),
                });
                actions.push(PaletteAction {
                    key: "Enter".to_string(),
                    label: "Open built-in Diff view".to_string(),
                    action_id: "builtin_diff",
                    enabled: row.is_some_and(|r| {
                        let is_dir = r.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                            || r.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                        !is_dir
                    }),
                });
                actions.push(PaletteAction {
                    key: "s".to_string(),
                    label: "Swap Left/Right Paths".to_string(),
                    action_id: "swap_paths",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "c".to_string(),
                    label: "Toggle Scan Mode (Fast/Precise)".to_string(),
                    action_id: "toggle_scan",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "r".to_string(),
                    label: "Manual Re-scan / Refresh".to_string(),
                    action_id: "refresh",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "C".to_string(),
                    label: "Edit Configuration".to_string(),
                    action_id: "config",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "?".to_string(),
                    label: "Open Help Screen".to_string(),
                    action_id: "help",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "/".to_string(),
                    label: "Open Filter Input".to_string(),
                    action_id: "filter",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "q".to_string(),
                    label: "Quit duodiff".to_string(),
                    action_id: "quit",
                    enabled: true,
                });
            }
            ViewMode::FileDiff => {
                actions.push(PaletteAction {
                    key: "w".to_string(),
                    label: "Toggle Wrap Mode".to_string(),
                    action_id: "toggle_wrap",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "f".to_string(),
                    label: "Toggle Full Content".to_string(),
                    action_id: "toggle_full",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "N".to_string(),
                    label: "Next Change".to_string(),
                    action_id: "next_change",
                    enabled: self.diff_has_changes(),
                });
                actions.push(PaletteAction {
                    key: "P".to_string(),
                    label: "Previous Change".to_string(),
                    action_id: "prev_change",
                    enabled: self.diff_has_changes(),
                });
                actions.push(PaletteAction {
                    key: "]".to_string(),
                    label: "Copy Change Block to Right".to_string(),
                    action_id: "copy_hunk_l2r",
                    enabled: self.diff_has_changes(),
                });
                actions.push(PaletteAction {
                    key: "[".to_string(),
                    label: "Copy Change Block to Left".to_string(),
                    action_id: "copy_hunk_r2l",
                    enabled: self.diff_has_changes(),
                });
                actions.push(PaletteAction {
                    key: "R".to_string(),
                    label: "Copy Whole File Left to Right".to_string(),
                    action_id: "copy_l2r",
                    enabled: self.selected_idx < self.filtered_rows.len()
                        && self.filtered_rows[self.selected_idx].left.is_some(),
                });
                actions.push(PaletteAction {
                    key: "L".to_string(),
                    label: "Copy Whole File Right to Left".to_string(),
                    action_id: "copy_r2l",
                    enabled: self.selected_idx < self.filtered_rows.len()
                        && self.filtered_rows[self.selected_idx].right.is_some(),
                });
                actions.push(PaletteAction {
                    key: "?".to_string(),
                    label: "Open Help Screen".to_string(),
                    action_id: "help",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "Esc".to_string(),
                    label: "Return to Tree View".to_string(),
                    action_id: "back",
                    enabled: true,
                });
            }
            _ => {
                actions.push(PaletteAction {
                    key: "?".to_string(),
                    label: "Open Help Screen".to_string(),
                    action_id: "help",
                    enabled: true,
                });
                actions.push(PaletteAction {
                    key: "Esc".to_string(),
                    label: "Go Back".to_string(),
                    action_id: "back",
                    enabled: true,
                });
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

    pub(crate) fn set_flat_rows(&mut self, rows: Vec<FlatRow>) {
        self.flat_rows = rows;
    }

    pub(crate) fn set_diff_rows(&mut self, rows: Vec<crate::diff_view::DiffRow>) {
        self.diff_rows = rows;
    }

    /// Install a tree and flatten it, as [`App::apply_scan_result`] would.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.flatten_tree();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{lock_env_tests, ConfigEnvGuard, RedirectedConfigDir};
    use std::time::SystemTime;

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

        assert_eq!(app.selected_idx, 0);
        app.select_next();
        assert_eq!(app.selected_idx, 1);
        app.select_next();
        assert_eq!(app.selected_idx, 1); // bounds check
        app.select_prev();
        assert_eq!(app.selected_idx, 0);
        app.select_prev();
        assert_eq!(app.selected_idx, 0); // bounds check
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
        assert_eq!(app.selected_idx, 4);
        assert_eq!(app.scroll_offset, 0); // still visible within first page

        app.page_down();
        assert_eq!(app.selected_idx, 8);
        assert_eq!(app.scroll_offset, 4); // selection pushed view down

        app.page_up();
        assert_eq!(app.selected_idx, 4);

        // Overshoot clamps to last row
        app.selected_idx = 18;
        app.page_down();
        assert_eq!(app.selected_idx, 19);

        app.page_up();
        assert_eq!(app.selected_idx, 15);

        // Empty list is a no-op
        app.filtered_rows.clear();
        app.selected_idx = 0;
        app.page_down();
        app.page_up();
        assert_eq!(app.selected_idx, 0);
    }

    #[test]
    fn test_diff_page_down_up() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.viewport.diff_physical_rows = 30;
        app.viewport.visible_height = 10; // page_step = 9
        app.diff_scroll = 0;

        app.diff_page_down();
        assert_eq!(app.diff_scroll, 9);

        app.diff_page_down();
        assert_eq!(app.diff_scroll, 18);

        // Clamp to max scroll (30 - 10 = 20)
        app.diff_page_down();
        assert_eq!(app.diff_scroll, 20);

        app.diff_page_up();
        assert_eq!(app.diff_scroll, 11);

        app.diff_scroll = 3;
        app.diff_page_up();
        assert_eq!(app.diff_scroll, 0);
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
        app.selected_idx = 0;
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
        app.selected_idx = 0;
        app.collapse_selected();
        assert_eq!(app.flat_rows.len(), 1);

        // expand root again
        app.expand_selected();
        assert_eq!(app.flat_rows.len(), 2);
    }

    #[test]
    fn test_adjust_scroll() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.scroll_offset = 2;

        // 1. visible_height == 0 does nothing
        app.selected_idx = 5;
        app.adjust_scroll(0);
        assert_eq!(app.scroll_offset, 2);

        // 2. selected_idx < scroll_offset -> scroll_offset becomes selected_idx
        app.selected_idx = 1;
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset, 1);

        // 3. selected_idx >= scroll_offset + visible_height -> scroll_offset adjusts
        app.selected_idx = 7;
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset, 3);

        // 4. selected_idx within view (e.g. 5) -> scroll_offset stays same
        app.selected_idx = 5;
        app.adjust_scroll(5);
        assert_eq!(app.scroll_offset, 3);
    }

    #[test]
    fn test_status_message_lifecycle() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // Initially no status
        assert!(app.status_message.is_none());

        // Set an error status
        app.set_status("Copy failed: permission denied", true);
        assert!(app.status_message.is_some());
        let (msg, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(is_error);
        assert!(msg.contains("permission denied"));

        // Should NOT expire with a short duration just after setting
        app.clear_expired_status(std::time::Duration::from_secs(10));
        assert!(app.status_message.is_some());

        // Should expire with zero duration
        app.clear_expired_status(std::time::Duration::ZERO);
        assert!(app.status_message.is_none());

        // Set a success status
        app.set_status("Copied 'file.txt'", false);
        let (_, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(!is_error);
    }

    #[test]
    fn test_swap_paths() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        assert_eq!(app.left_path, PathBuf::from("/left"));
        assert_eq!(app.right_path, PathBuf::from("/right"));

        app.swap_paths();

        assert_eq!(app.left_path, PathBuf::from("/right"));
        assert_eq!(app.right_path, PathBuf::from("/left"));
    }

    #[test]
    fn test_swap_paths_resets_state() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.selected_idx = 5;
        app.scroll_offset = 3;
        app.diff_scroll = 2;
        app.diff_left_hash = Some("abc".to_string());
        app.diff_right_hash = Some("def".to_string());

        app.swap_paths();

        assert_eq!(app.selected_idx, 0);
        assert_eq!(app.scroll_offset, 0);
        assert_eq!(app.diff_scroll, 0);
        assert!(app.diff_left_hash.is_none());
        assert!(app.diff_right_hash.is_none());
    }

    #[test]
    fn test_swap_paths_twice_restores() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.swap_paths();
        app.swap_paths();
        assert_eq!(app.left_path, PathBuf::from("/left"));
        assert_eq!(app.right_path, PathBuf::from("/right"));
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
        assert_eq!(app.filtered_rows.len(), 3);

        // Filter by "alpha"
        app.filter_pattern = "alpha".to_string();
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 1);
        assert_eq!(app.filtered_rows[0].name, "alpha.txt");

        // Clear filter
        app.filter_pattern.clear();
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 3);
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

        app.filter_diffs_only = true;
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 2);
        assert!(app
            .filtered_rows
            .iter()
            .all(|r| r.state != DiffState::Identical));
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
        app.filter_pattern = "a".to_string();
        app.filter_diffs_only = true;
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 1);
        assert_eq!(app.filtered_rows[0].name, "diff_a.txt");
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
        app.filter_pattern = "readme".to_string();
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 1);
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
        app.selected_idx = 2;
        app.scroll_offset = 1;
        app.viewport.visible_height = 10;

        // Rebuild without changing filter criteria — keep the same row selected.
        app.apply_filter();
        assert_eq!(app.selected_idx, 2);
        assert_eq!(
            app.filtered_rows[app.selected_idx].relative_path,
            PathBuf::from("c.txt")
        );
        assert_eq!(app.scroll_offset, 1);
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
        app.selected_idx = 0; // same.txt
        app.scroll_offset = 0;

        app.filter_diffs_only = true;
        app.apply_filter();
        // same.txt is filtered out → fall back to top of remaining list
        assert_eq!(app.selected_idx, 0);
        assert_eq!(app.filtered_rows[0].name, "diff.txt");
        assert_eq!(app.scroll_offset, 0);
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
        app.selected_idx = 2; // child_b
        app.scroll_offset = 1;
        app.viewport.visible_height = 10;

        app.flatten_tree();
        assert_eq!(app.selected_idx, 2);
        assert_eq!(app.flat_rows[app.selected_idx].name, "child_b");
        assert_eq!(app.scroll_offset, 1);
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
        app.selected_idx = app
            .filtered_rows
            .iter()
            .position(|r| r.relative_path == *"subdir/file.txt")
            .unwrap();

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
            .filtered_rows
            .iter()
            .any(|r| r.relative_path == *"subdir/file.txt"));
        assert_eq!(
            app.filtered_rows[app.selected_idx].relative_path,
            PathBuf::from("subdir/file.txt")
        );
    }

    #[test]
    fn test_open_commit_cancel_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.filter_pattern = "abc".to_string();

        // open_filter pre-fills input with committed pattern
        app.open_filter();
        assert!(app.filter_active);
        assert_eq!(app.filter_input, "abc");

        // Type more
        for c in "def".chars() {
            app.filter_input.insert(c);
        }
        assert_eq!(app.filter_input, "abcdef");

        // Cancel restores to original pattern
        app.cancel_filter();
        assert!(!app.filter_active);
        assert_eq!(app.filter_input, "abc");
        assert_eq!(app.filter_pattern, "abc");

        // Open again and commit
        app.open_filter();
        app.filter_input = "xyz".into();
        app.commit_filter();
        assert!(!app.filter_active);
        assert_eq!(app.filter_pattern, "xyz");
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
        app.filter_pattern = "a".to_string();
        app.filter_diffs_only = true;
        app.apply_filter();
        assert_eq!(app.filtered_rows.len(), 0);

        app.clear_filter();
        assert!(app.filter_pattern.is_empty());
        assert!(!app.filter_diffs_only);
        assert_eq!(app.filtered_rows.len(), 2);
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
        assert_eq!(app.help_topic, HelpTopic::General);
        assert_eq!(app.help_return_view, ViewMode::DirectoryTree);
        assert!(!app.help_index_open);
        assert_eq!(app.help_index_sel, 0);
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn test_open_help_sets_contextual_topic_and_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = ViewMode::FileDiff;
        app.help_index_open = true; // prove open_help sets this to false
        app.help_scroll = 7; // prove open_help resets this

        app.open_help();

        assert_eq!(app.help_return_view, ViewMode::FileDiff);
        assert_eq!(app.help_topic, HelpTopic::FileDiff);
        assert!(!app.help_index_open);
        assert_eq!(app.help_scroll, 0);
        assert_eq!(app.view_mode, ViewMode::Help);
    }

    #[test]
    fn test_open_help_while_already_on_help_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = ViewMode::FileDiff;

        app.open_help();
        assert_eq!(app.help_return_view, ViewMode::FileDiff);

        // Calling open_help() again while already on Help (e.g. clicking the top bar's
        // (?)Help hotspot from within Help itself) must be a no-op — otherwise
        // help_return_view would be overwritten with ViewMode::Help, trapping Esc/`?`/q in
        // Help with no keyboard way out.
        app.open_help();
        assert_eq!(app.help_return_view, ViewMode::FileDiff);
        assert_eq!(app.view_mode, ViewMode::Help);
    }

    #[test]
    fn test_open_config_remembers_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = ViewMode::FileDiff;

        app.open_config();

        assert_eq!(app.config_return_view, ViewMode::FileDiff);
        assert_eq!(app.view_mode, ViewMode::ConfigMenu);
    }

    #[test]
    fn test_open_config_while_already_on_config_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = ViewMode::FileDiff;

        app.open_config();
        assert_eq!(app.config_return_view, ViewMode::FileDiff);

        // Calling open_config() again while already on Config (e.g. clicking the top bar's
        // (C)onfig hotspot from within Config itself) must be a no-op — otherwise
        // config_return_view would be overwritten with ViewMode::ConfigMenu, trapping Esc/q
        // in Config with no keyboard way out.
        app.open_config();
        assert_eq!(app.config_return_view, ViewMode::FileDiff);
        assert_eq!(app.view_mode, ViewMode::ConfigMenu);
    }

    #[test]
    fn test_config_rows_and_navigation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.detected_diff_tools = vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ];

        let rows = app.config_rows();
        // Header + 2 tools + Updates header + CheckUpdates + Mouse header + Mouse
        // + Theme header + Theme + Diff View header + DiffContext
        assert_eq!(rows.len(), 11);
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

        app.config_selected_idx = 0;
        app.ensure_config_selection();
        assert_eq!(app.config_selected_idx, 1);

        // Selectable indices: 1, 2, 4, 6, 8, 10
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 2);
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 4);
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 6);
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 8);
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 10);
        app.config_select_next();
        assert_eq!(app.config_selected_idx, 1);

        app.config_select_prev();
        assert_eq!(app.config_selected_idx, 10);
    }

    #[test]
    fn test_mouse_toggle_persists_in_settings() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // App::new() doesn't sync the session-only `mouse_enabled` flag from
        // `settings.mouse` — main.rs does that after construction.
        app.mouse_enabled = app.settings.mouse;
        assert!(!app.settings.mouse);
        assert!(!app.mouse_enabled);

        app.config_selected_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.apply_config_selection();
        assert!(app.settings.mouse);
        assert!(app.mouse_enabled);

        app.apply_config_selection();
        assert!(!app.settings.mouse);
        assert!(!app.mouse_enabled);
    }

    #[test]
    fn test_theme_toggle_persists_in_settings() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
        assert_eq!(app.theme(), crate::theme::Theme::LIGHT);

        app.config_selected_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.apply_config_selection();
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Dark);
        assert_eq!(app.theme(), crate::theme::Theme::DARK);

        app.apply_config_selection();
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
    }

    #[test]
    fn test_diff_context_adjust_persists_and_clamps() {
        let _guard = ConfigEnvGuard::new();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings.diff_context, 7);

        app.config_selected_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();

        app.adjust_config_selection(true);
        assert_eq!(app.settings.diff_context, 8);
        app.adjust_config_selection(false);
        app.adjust_config_selection(false);
        assert_eq!(app.settings.diff_context, 6);

        // Clamped at 0 (saturating_sub), not underflowing.
        for _ in 0..10 {
            app.adjust_config_selection(false);
        }
        assert_eq!(app.settings.diff_context, 0);

        // Clamped at 50.
        for _ in 0..60 {
            app.adjust_config_selection(true);
        }
        assert_eq!(app.settings.diff_context, 50);
    }

    #[test]
    fn test_adjust_config_selection_is_noop_for_non_numeric_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.config_selected_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::CheckUpdates))
            .unwrap();
        let before = app.settings.diff_context;
        app.adjust_config_selection(true);
        assert_eq!(app.settings.diff_context, before);
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
        app.update_check_enabled = app.settings.check_updates;
        assert!(!app.settings.check_updates);
        assert!(!app.update_check_enabled);

        // Land on CheckUpdates row and toggle.
        app.open_config();
        while !matches!(
            app.config_rows().get(app.config_selected_idx),
            Some(ConfigRowKind::CheckUpdates)
        ) {
            app.config_select_next();
        }
        app.apply_config_selection();
        assert!(app.settings.check_updates);
        assert!(app.update_check_enabled);

        app.apply_config_selection();
        assert!(!app.settings.check_updates);
        assert!(!app.update_check_enabled);
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
            app.mouse_enabled = app.settings.mouse;
            app.config_selected_idx = app
                .config_rows()
                .iter()
                .position(|r| matches!(r, ConfigRowKind::Mouse))
                .unwrap();
            app.apply_config_selection();
            app.apply_config_selection();
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
        let _ = app.settings.external_diff_tool;
    }

    #[test]
    fn test_focus_pane_shortcuts() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.active_side_left);

        app.focus_right_pane();
        assert!(!app.active_side_left);

        app.focus_left_pane();
        assert!(app.active_side_left);
    }

    #[test]
    fn test_build_palette_actions_directory_tree() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.view_mode = ViewMode::DirectoryTree;
        let actions = app.build_palette_actions();
        assert!(actions.iter().any(|a| a.action_id == "quit"));
        assert!(actions.iter().any(|a| a.action_id == "help"));
        assert!(actions.iter().any(|a| a.action_id == "refresh"));
    }

    #[test]
    fn test_build_palette_actions_file_diff() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.view_mode = ViewMode::FileDiff;
        let actions = app.build_palette_actions();
        assert!(actions.iter().any(|a| a.action_id == "toggle_wrap"));
        assert!(actions.iter().any(|a| a.action_id == "toggle_full"));
        assert!(actions.iter().any(|a| a.action_id == "next_change"));
        assert!(actions.iter().any(|a| a.action_id == "prev_change"));
        assert!(actions.iter().any(|a| a.action_id == "copy_hunk_l2r"));
        assert!(actions.iter().any(|a| a.action_id == "copy_hunk_r2l"));
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
        app.view_mode = ViewMode::FileDiff;
        app.diff_show_full = true;
        app.refresh_file_diff().expect("diff should load");
        app.diff_scroll = 1;

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
        app.set_diff_rows(vec![
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
        assert_eq!(app.diff_scroll, 1);
        app.jump_to_next_change();
        assert_eq!(app.diff_scroll, 2);
        app.jump_to_prev_change();
        assert_eq!(app.diff_scroll, 1);
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
    fn test_diff_rows_accessor_reflects_set_diff_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.diff_rows().is_empty());

        let rows = vec![equal_row("a"), equal_row("b")];
        app.set_diff_rows(rows.clone());

        assert_eq!(app.diff_rows(), rows.as_slice());
    }

    #[test]
    fn test_diff_has_changes_false_when_all_rows_equal() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_diff_rows(vec![equal_row("a"), equal_row("b")]);

        assert!(!app.diff_has_changes());
    }

    #[test]
    fn test_diff_has_changes_true_when_a_row_differs() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_diff_rows(vec![equal_row("a"), deleted_row("b")]);

        assert!(app.diff_has_changes());
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
        app.selected_idx = 30;

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.viewport().visible_height, 20);
        assert_eq!(app.scroll_offset, 11, "selection scrolled into view");
    }

    #[test]
    fn test_sync_viewport_diff_derives_geometry_from_area() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![equal_row(&"a".repeat(100))]);

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
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![equal_row(&"a".repeat(100)), equal_row("short")]);
        app.diff_wrap = true;

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
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows((0..40).map(|i| equal_row(&format!("line {i}"))).collect());

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        app.diff_page_down();
        assert_eq!(app.diff_scroll, 18, "page step is visible_height - 1");

        // Growing the terminal shows more rows at once, so the bottom of the
        // document now sits at a smaller scroll offset. The sync itself must pull
        // the current position back inside the new geometry — otherwise the next
        // page-down would appear to scroll backwards.
        app.sync_viewport(Rect::new(0, 0, 80, 40));
        assert_eq!(app.viewport().visible_height, 35);
        assert_eq!(app.diff_scroll, 5, "clamped to 40 rows - 35 visible");
        app.diff_page_down();
        assert_eq!(app.diff_scroll, 5, "already at the bottom, stays put");
    }

    #[test]
    fn test_sync_viewport_clamps_horizontal_scroll_to_longest_line() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![equal_row(&"a".repeat(100))]);

        app.sync_viewport(Rect::new(0, 0, 80, 24));
        app.diff_h_scroll = app.viewport().max_diff_h_scroll();
        assert_eq!(app.diff_h_scroll, 62, "100 chars less the 38 on screen");

        // Opening a shorter file must not leave the pane scrolled past its end.
        app.set_diff_rows(vec![equal_row(&"a".repeat(50))]);
        app.sync_viewport(Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff_h_scroll, 12);
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
        assert_eq!(app.filtered_rows.len(), 1, "filter view rebuilt too");
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
}
