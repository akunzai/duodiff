use std::path::PathBuf;
use crate::diff::{AlignedNode, FileInfo, DiffState};

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
}

impl App {
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
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
    use crate::diff::{FileInfo, DiffState};
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
            children: vec![
                AlignedNode {
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
                }
            ],
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
            children: vec![
                AlignedNode {
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
                }
            ],
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
            children: vec![
                AlignedNode {
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
                }
            ],
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
}
