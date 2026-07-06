use std::path::PathBuf;
use crate::diff::AlignedNode;

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
    pub flat_rows: Vec<(usize, PathBuf, String, bool)>, // depth, relative_path, display_name, is_dir
    pub selected_idx: usize,
    pub active_side_left: bool,
    pub view_mode: ViewMode,
    pub diff_content: Option<(String, String)>,
    pub diff_scroll: usize,
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
            active_side_left: true,
            view_mode: ViewMode::DirectoryTree,
            diff_content: None,
            diff_scroll: 0,
        }
    }

    pub fn flatten_tree(&mut self) {
        self.flat_rows.clear();
        if let Some(ref root) = self.root_node {
            Self::flatten_node(&mut self.flat_rows, root, 0);
        }
        if self.selected_idx >= self.flat_rows.len() && !self.flat_rows.is_empty() {
            self.selected_idx = self.flat_rows.len() - 1;
        }
    }

    fn flatten_node(flat_rows: &mut Vec<(usize, PathBuf, String, bool)>, node: &AlignedNode, depth: usize) {
        flat_rows.push((
            depth,
            node.relative_path.clone(),
            node.name.clone(),
            node.left.as_ref().map(|l| l.is_dir).unwrap_or_else(|| {
                node.right.as_ref().map(|r| r.is_dir).unwrap_or(false)
            }),
        ));
        if node.is_expanded {
            for child in &node.children {
                Self::flatten_node(flat_rows, child, depth + 1);
            }
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
        assert_eq!(app.flat_rows[0].2, "root");
        assert_eq!(app.flat_rows[1].2, "child");
        assert_eq!(app.flat_rows[1].0, 1, "Child depth should be 1");
    }
}
