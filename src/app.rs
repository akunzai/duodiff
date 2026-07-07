use crate::diff::{AlignedNode, DiffState, FileInfo};
use crate::ignore::IgnoreMatcher;
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

pub enum ViewMode {
    DirectoryTree,
    FileDiff,
    ConfigMenu,
    ConfigDiffTool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    CopyLeftToRight,
    CopyRightToLeft,
}

#[derive(Clone, Debug, Default)]
pub struct ContextMenuState {
    pub visible: bool,
    pub selected_idx: usize,
    pub x: u16,
    pub y: u16,
    pub items: Vec<String>,
}

pub struct App {
    pub left_path: PathBuf,
    pub right_path: PathBuf,
    pub precise_mode: bool,
    pub root_node: Option<AlignedNode>,
    pub scan_in_progress: bool,
    pub progress_count: usize,
    pub progress_path: String,
    pub flat_rows: Vec<FlatRow>,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub active_side_left: bool,
    pub view_mode: ViewMode,
    pub diff_rows: Vec<crate::diff_view::DiffRow>,
    pub diff_scroll: usize,
    pub visible_height: usize,
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
    pub settings_menu_selected_idx: usize,
    pub config_diff_tool_selected_idx: usize,
    pub context_menu: ContextMenuState,
    pub show_confirm_modal: bool,
    pub confirm_modal_message: String,
    pub confirm_modal_action: Option<ConfirmAction>,
    /// Transient status toast: (message, is_error, created_at)
    pub status_message: Option<(String, bool, Instant)>,
    /// When true, key events are routed to the filter text input.
    pub filter_active: bool,
    /// Current text in the filter input bar.
    pub filter_input: String,
    /// Committed filter pattern applied to flat_rows (set on Enter/ESC).
    pub filter_pattern: String,
    /// When true, only show rows that differ (exclude Identical).
    pub filter_diffs_only: bool,
    /// Filtered view of flat_rows, rebuilt whenever the filter changes.
    pub filtered_rows: Vec<FlatRow>,
    /// Glob-based ignore matcher used during directory scans.
    pub ignore_matcher: IgnoreMatcher,
    pub update_check_enabled: bool,
    pub update_available: Option<String>,
    pub install_method: crate::upgrade::InstallMethod,
}

impl App {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        Self::new_with_ignore(left, right, IgnoreMatcher::default())
    }

    pub fn new_with_ignore(left: PathBuf, right: PathBuf, ignore_matcher: IgnoreMatcher) -> Self {
        let mut settings = crate::settings::AppSettings::load();
        let detected_diff_tools = crate::diff_tool::detect_diff_tools();
        if settings.external_diff_tool.is_none() {
            if let Some((tool, _)) = detected_diff_tools.iter().find(|(_, avail)| *avail) {
                settings.external_diff_tool = Some(tool.as_str().to_string());
                let _ = settings.save();
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
            progress_count: 0,
            progress_path: String::new(),
            flat_rows: Vec::new(),
            selected_idx: 0,
            scroll_offset: 0,
            active_side_left: true,
            view_mode: ViewMode::DirectoryTree,
            diff_rows: Vec::new(),
            diff_scroll: 0,
            visible_height: 0,
            diff_left_hash: None,
            diff_right_hash: None,
            diff_left_line_ending: None,
            diff_right_line_ending: None,
            last_click_idx: None,
            last_click_time: None,
            settings,
            detected_diff_tools,
            settings_menu_selected_idx: 0,
            config_diff_tool_selected_idx: 0,
            context_menu: ContextMenuState {
                visible: false,
                selected_idx: 0,
                x: 0,
                y: 0,
                items: vec![
                    "1. Compare via External Diff Tool".to_string(),
                    "2. Edit via External Editor".to_string(),
                    "3. Edit Configuration".to_string(),
                    "4. Cancel".to_string(),
                ],
            },
            show_confirm_modal: false,
            confirm_modal_message: String::new(),
            confirm_modal_action: None,
            status_message: None,
            filter_active: false,
            filter_input: String::new(),
            filter_pattern: String::new(),
            filter_diffs_only: false,
            filtered_rows: Vec::new(),
            ignore_matcher,
            update_check_enabled: true,
            update_available: None,
            install_method,
        }
    }

    /// Set a transient status message displayed in the footer.
    /// `is_error` = true → red styling, false → green styling.
    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_message = Some((msg.into(), is_error, Instant::now()));
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

    pub fn flatten_tree(&mut self) {
        self.flat_rows.clear();
        if let Some(root) = self.root_node.take() {
            self.flatten_node(&root, 0);
            self.root_node = Some(root);
        }
        if self.selected_idx >= self.flat_rows.len() && !self.flat_rows.is_empty() {
            self.selected_idx = self.flat_rows.len() - 1;
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

    /// Rebuild `filtered_rows` from `flat_rows` using the current filter
    /// pattern and diffs-only flag. Resets selection to the top.
    pub fn apply_filter(&mut self) {
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
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    /// Open the filter input bar, pre-filling with the committed pattern.
    pub fn open_filter(&mut self) {
        self.filter_active = true;
        self.filter_input = self.filter_pattern.clone();
    }

    /// Close the filter input bar, committing the typed text as the pattern.
    pub fn commit_filter(&mut self) {
        self.filter_active = false;
        self.filter_pattern = self.filter_input.clone();
        self.apply_filter();
    }

    /// Close the filter input bar, discarding any uncommitted typing.
    pub fn cancel_filter(&mut self) {
        self.filter_active = false;
        self.filter_input = self.filter_pattern.clone();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
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
    fn test_open_commit_cancel_filter() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.filter_pattern = "abc".to_string();

        // open_filter pre-fills input with committed pattern
        app.open_filter();
        assert!(app.filter_active);
        assert_eq!(app.filter_input, "abc");

        // Type more
        app.filter_input.push_str("def");
        assert_eq!(app.filter_input, "abcdef");

        // Cancel restores to original pattern
        app.cancel_filter();
        assert!(!app.filter_active);
        assert_eq!(app.filter_input, "abc");
        assert_eq!(app.filter_pattern, "abc");

        // Open again and commit
        app.open_filter();
        app.filter_input = "xyz".to_string();
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
}
