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
}
