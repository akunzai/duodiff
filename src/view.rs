//! Borrowed presentation snapshots assembled from application state.

use crate::app::{App, FlatRow, ViewMode};
use crate::diff::DiffState;
use crate::theme::Theme;
use std::path::Path;
use std::time::SystemTime;

/// A full frame assembled from one immutable borrow of [`App`].
#[derive(Debug)]
pub struct ScreenView<'a> {
    pub base: BaseScreenView<'a>,
}

#[derive(Debug)]
pub enum BaseScreenView<'a> {
    DirectoryTree,
    FileDiff(DiffScreenView<'a>),
    Config,
    Help,
}

#[derive(Debug)]
pub struct DiffScreenView<'a> {
    pub content: DiffView<'a>,
}

/// The selected tree entry projected into the data needed by diff rendering.
#[derive(Clone, Copy, Debug)]
pub struct SelectedRowView<'a> {
    pub relative_path: &'a Path,
    pub state: DiffState,
    pub left: Option<FileInfoView>,
    pub right: Option<FileInfoView>,
}

impl<'a> From<&'a FlatRow> for SelectedRowView<'a> {
    fn from(row: &'a FlatRow) -> Self {
        Self {
            relative_path: &row.relative_path,
            state: row.state,
            left: row.left.as_ref().map(FileInfoView::from),
            right: row.right.as_ref().map(FileInfoView::from),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FileInfoView {
    pub size: u64,
    pub modified: SystemTime,
    pub is_dir: bool,
}

impl From<&crate::diff::FileInfo> for FileInfoView {
    fn from(info: &crate::diff::FileInfo) -> Self {
        Self {
            size: info.size,
            modified: info.modified,
            is_dir: info.is_dir,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiffView<'a> {
    pub rows: &'a [crate::diff_view::DiffRow],
    pub wrap: bool,
    pub scroll: usize,
    pub h_scroll: usize,
    pub visible_height: usize,
    pub content_width: usize,
    pub left_line_count: usize,
    pub right_line_count: usize,
    pub left_root: &'a Path,
    pub right_root: &'a Path,
    pub row: Option<SelectedRowView<'a>>,
    pub left_hash: Option<&'a str>,
    pub right_hash: Option<&'a str>,
    pub left_line_ending: Option<&'a str>,
    pub right_line_ending: Option<&'a str>,
    pub theme: Theme,
    pub status_toast: Option<(&'a str, bool)>,
    pub has_changes: bool,
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    pub left_dirty: bool,
    pub right_dirty: bool,
    pub can_undo: bool,
}

pub fn assemble(app: &App) -> ScreenView<'_> {
    let base = match app.view_mode() {
        ViewMode::DirectoryTree => BaseScreenView::DirectoryTree,
        ViewMode::FileDiff => BaseScreenView::FileDiff(DiffScreenView { content: diff(app) }),
        ViewMode::ConfigMenu => BaseScreenView::Config,
        ViewMode::Help => BaseScreenView::Help,
    };
    ScreenView { base }
}

pub(crate) fn diff(app: &App) -> DiffView<'_> {
    let diff = app.diff();
    let viewport = app.viewport();
    DiffView {
        rows: diff.rows(),
        wrap: diff.wrap(),
        scroll: diff.scroll(),
        h_scroll: diff.h_scroll(),
        visible_height: viewport.visible_height,
        content_width: viewport.diff_content_width,
        left_line_count: diff.left_line_count(),
        right_line_count: diff.right_line_count(),
        left_root: app.left_path(),
        right_root: app.right_path(),
        row: app.selected_row().map(SelectedRowView::from),
        left_hash: diff.left_hash(),
        right_hash: diff.right_hash(),
        left_line_ending: diff.left_line_ending(),
        right_line_ending: diff.right_line_ending(),
        theme: app.theme(),
        status_toast: app.status_toast(),
        has_changes: diff.has_changes(),
        update_available: app.update_available(),
        install_method: app.install_method(),
        left_dirty: diff.left_dirty(),
        right_dirty: diff.right_dirty(),
        can_undo: diff.can_undo(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ViewMode};
    use std::path::PathBuf;

    #[test]
    fn assemble_projects_file_diff_without_ui_types() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        let screen = assemble(&app);

        assert!(matches!(screen.base, BaseScreenView::FileDiff(_)));
    }
}
