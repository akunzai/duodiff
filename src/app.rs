use crate::diff::{AlignedNode, DiffState, FileInfo};
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
}

impl App {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        let mut settings = crate::settings::AppSettings::load();
        let detected_diff_tools = crate::diff_tool::detect_diff_tools();
        if settings.external_diff_tool.is_none() {
            if let Some((tool, _)) = detected_diff_tools.iter().find(|(_, avail)| *avail) {
                settings.external_diff_tool = Some(tool.as_str().to_string());
                let _ = settings.save();
            }
        }

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
        }
    }

    /// Set a transient status message displayed in the footer.
    /// `is_error` = true → red styling, false → green styling.
    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_message = Some((msg.into(), is_error, Instant::now()));
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

    pub fn toggle_expand(&mut self) {
        if self.flat_rows.is_empty() || self.selected_idx >= self.flat_rows.len() {
            return;
        }
        let row = &self.flat_rows[self.selected_idx];
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
        if !self.flat_rows.is_empty() && self.selected_idx < self.flat_rows.len() - 1 {
            self.selected_idx += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    pub fn expand_selected(&mut self) {
        if self.flat_rows.is_empty() || self.selected_idx >= self.flat_rows.len() {
            return;
        }
        let row = &self.flat_rows[self.selected_idx];
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
        if self.flat_rows.is_empty() || self.selected_idx >= self.flat_rows.len() {
            return;
        }
        let row = &self.flat_rows[self.selected_idx];
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
}
