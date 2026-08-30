//! Borrowed presentation snapshots assembled from application state.

use crate::app::{App, FlatRow, HelpTopic, ViewMode};
use crate::diff::{DiffState, TreeSummary};
use crate::theme::Theme;
use std::path::Path;
use std::time::SystemTime;

/// Normalize in-memory presentation state before borrowing one immutable frame.
pub fn prepare_frame(app: &mut App, area: ratatui::layout::Rect) {
    match app.view_mode() {
        ViewMode::DirectoryTree => {
            let layout = crate::layout::tree_layout(&tree_layout_inputs(app), area);
            app.prepare_tree_viewport(layout.left.height.saturating_sub(2) as usize);
        }
        ViewMode::FileDiff => {
            let layout = crate::layout::diff_layout(&diff_layout_inputs(app), area);
            let pane_inner = layout.left.width.saturating_sub(2) as usize;
            let content_width = crate::diff_view::diff_text_width(
                pane_inner,
                app.diff().left_line_count(),
                app.diff().right_line_count(),
            );
            app.prepare_diff_viewport(layout.left.height.saturating_sub(2) as usize, content_width);
        }
        ViewMode::ConfigMenu | ViewMode::Help => {}
    }
    if app.view_mode() == ViewMode::ConfigMenu {
        app.ensure_config_selection();
        if let Some(editor) = app.exclusion_editor() {
            let layout = crate::layout::exclusion_editor_layout(editor.draft().len(), area);
            app.sync_exclusion_editor_viewport(layout.visible_rows());
        }
    }
    if app.palette_visible() {
        app.refresh_palette_items();
        let layout = crate::layout::palette_layout(app.palette().items.len(), area);
        app.sync_palette_viewport(layout.visible_rows());
    }
}

/// A full frame assembled from one immutable borrow of [`App`].
#[derive(Debug)]
pub struct ScreenView<'a> {
    pub theme: Theme,
    pub top_bar: TopBarView,
    pub base: BaseScreenView<'a>,
    pub confirm: Option<ConfirmView<'a>>,
    pub palette: Option<PaletteView<'a>>,
}

#[derive(Debug)]
pub enum BaseScreenView<'a> {
    DirectoryTree(TreeScreenView<'a>),
    FileDiff(DiffScreenView<'a>),
    Config(ConfigScreenView),
    Help(HelpScreenView<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKind {
    DirectoryTree,
    FileDiff,
    Config,
    Help,
}

impl From<ViewMode> for ScreenKind {
    fn from(mode: ViewMode) -> Self {
        match mode {
            ViewMode::DirectoryTree => Self::DirectoryTree,
            ViewMode::FileDiff => Self::FileDiff,
            ViewMode::ConfigMenu => Self::Config,
            ViewMode::Help => Self::Help,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TopBarView {
    pub screen: ScreenKind,
    pub precise_mode: bool,
    pub diff_show_full: bool,
    pub diff_wrap: bool,
    pub scan_in_progress: bool,
    pub scan_progress_count: usize,
    pub spinner_frame: usize,
    pub theme: Theme,
}

#[derive(Debug)]
pub struct TreeScreenView<'a> {
    pub content: TreeView<'a>,
    pub footer: TreeFooterView<'a>,
    pub layout_inputs: crate::layout::TreeLayoutInputs,
}

#[derive(Clone, Copy, Debug)]
pub struct TreeRowsView<'a> {
    rows: &'a [FlatRow],
}

impl<'a> TreeRowsView<'a> {
    pub fn new(rows: &'a [FlatRow]) -> Self {
        Self { rows }
    }

    pub fn is_empty(self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(self) -> usize {
        self.rows.len()
    }

    pub fn iter(self) -> impl Iterator<Item = TreeRowView<'a>> {
        self.rows.iter().map(TreeRowView::from)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TreeRowView<'a> {
    pub depth: usize,
    pub relative_path: &'a Path,
    pub name: &'a str,
    pub left_name: Option<&'a str>,
    pub right_name: Option<&'a str>,
    pub left_relative_path: Option<&'a Path>,
    pub right_relative_path: Option<&'a Path>,
    pub state: DiffState,
    pub left: Option<FileInfoView>,
    pub right: Option<FileInfoView>,
    pub is_expanded: bool,
    pub has_case_conflict: bool,
    pub contains_case_conflict: bool,
    pub is_ambiguous_case_collision: bool,
}

impl<'a> From<&'a FlatRow> for TreeRowView<'a> {
    fn from(row: &'a FlatRow) -> Self {
        Self {
            depth: row.depth,
            relative_path: &row.relative_path,
            name: &row.name,
            left_name: row.left_name.as_deref(),
            right_name: row.right_name.as_deref(),
            left_relative_path: row.left_relative_path.as_deref(),
            right_relative_path: row.right_relative_path.as_deref(),
            state: row.state,
            left: row.left.as_ref().map(FileInfoView::from),
            right: row.right.as_ref().map(FileInfoView::from),
            is_expanded: row.is_expanded,
            has_case_conflict: row.has_case_conflict,
            contains_case_conflict: row.contains_case_conflict,
            is_ambiguous_case_collision: row.is_ambiguous_case_collision,
        }
    }
}

impl<'a> TreeRowView<'a> {
    pub fn left_relative_path(self) -> &'a Path {
        self.left_relative_path
            .or(if self.left.is_some() {
                Some(self.relative_path)
            } else {
                None
            })
            .unwrap_or(self.relative_path)
    }

    pub fn right_relative_path(self) -> &'a Path {
        self.right_relative_path
            .or(if self.right.is_some() {
                Some(self.relative_path)
            } else {
                None
            })
            .unwrap_or(self.relative_path)
    }

    pub fn left_name(self) -> &'a str {
        self.left_name
            .or(if self.left.is_some() {
                Some(self.name)
            } else {
                None
            })
            .unwrap_or(self.name)
    }

    pub fn right_name(self) -> &'a str {
        self.right_name
            .or(if self.right.is_some() {
                Some(self.name)
            } else {
                None
            })
            .unwrap_or(self.name)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TreeView<'a> {
    pub rows: TreeRowsView<'a>,
    pub scroll_offset: usize,
    pub selected_idx: usize,
    pub visible_height: usize,
    pub left_root: &'a Path,
    pub right_root: &'a Path,
    pub active_side_left: bool,
    pub theme: Theme,
    pub is_filter_active: bool,
}

#[derive(Clone, Debug)]
pub struct TreeFooterView<'a> {
    pub row: Option<TreeRowView<'a>>,
    pub status_toast: Option<(&'a str, bool)>,
    pub filter_active: bool,
    pub filter_input: &'a crate::text_input::TextInput,
    pub filter_pattern: &'a str,
    pub filter_diffs_only: bool,
    pub scan_in_progress: bool,
    pub scan_progress_count: usize,
    pub spinner_frame: usize,
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    pub theme: Theme,
    pub summary: Option<TreeSummary>,
}

#[derive(Debug)]
pub struct DiffScreenView<'a> {
    pub content: DiffView<'a>,
    pub layout_inputs: crate::layout::DiffLayoutInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopicView {
    DirectoryTree,
    FileDiff,
    Config,
    Mouse,
    General,
    About,
}

impl From<HelpTopic> for HelpTopicView {
    fn from(topic: HelpTopic) -> Self {
        match topic {
            HelpTopic::DirectoryTree => Self::DirectoryTree,
            HelpTopic::FileDiff => Self::FileDiff,
            HelpTopic::Config => Self::Config,
            HelpTopic::Mouse => Self::Mouse,
            HelpTopic::General => Self::General,
            HelpTopic::About => Self::About,
        }
    }
}

impl HelpTopicView {
    pub fn all() -> [Self; 6] {
        [
            Self::DirectoryTree,
            Self::FileDiff,
            Self::Config,
            Self::Mouse,
            Self::General,
            Self::About,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::DirectoryTree => "Directory Tree",
            Self::FileDiff => "File Diff",
            Self::Config => "Config",
            Self::Mouse => "Mouse",
            Self::General => "General",
            Self::About => "About",
        }
    }
}

#[derive(Debug)]
pub struct HelpScreenView<'a> {
    pub content: HelpView<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct HelpView<'a> {
    pub topic: HelpTopicView,
    pub index_open: bool,
    pub index_sel: usize,
    pub scroll: u16,
    pub theme: Theme,
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
}

#[derive(Debug)]
pub struct ConfigScreenView {
    pub content: ConfigView,
    pub exclusion_editor: Option<ExclusionEditorView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigControl {
    None,
    Select,
    Toggle,
    Adjust,
    Unavailable,
}

#[derive(Clone, Debug)]
pub enum ConfigRowView {
    Header(&'static str),
    Choice {
        label: String,
        selected: bool,
        available: bool,
    },
    Toggle {
        label: &'static str,
        enabled: bool,
    },
    Value(String),
    MutedLines(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct ConfigRow {
    pub view: ConfigRowView,
    pub control: ConfigControl,
}

#[derive(Clone, Debug)]
pub struct ConfigView {
    pub rows: Vec<ConfigRow>,
    pub selected_idx: usize,
    pub theme: Theme,
}

#[derive(Clone, Debug)]
pub struct ExclusionEditorView {
    pub draft: Vec<String>,
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub editing: bool,
    pub input: crate::text_input::TextInput,
    pub theme: Theme,
}

#[derive(Clone, Copy, Debug)]
pub struct PaletteView<'a> {
    pub items: &'a [crate::commands::CommandEntry],
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub query: &'a str,
    pub theme: Theme,
}

#[derive(Clone, Copy, Debug)]
pub struct ConfirmChoiceView<'a> {
    pub key: char,
    pub label: &'a str,
}

#[derive(Clone, Debug)]
pub struct ConfirmView<'a> {
    pub title: &'a str,
    pub headline: &'a str,
    pub lines: &'a [String],
    pub choices: Vec<ConfirmChoiceView<'a>>,
    pub theme: Theme,
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
        ViewMode::DirectoryTree => BaseScreenView::DirectoryTree(tree(app)),
        ViewMode::FileDiff => BaseScreenView::FileDiff(DiffScreenView {
            content: diff(app),
            layout_inputs: diff_layout_inputs(app),
        }),
        ViewMode::ConfigMenu => BaseScreenView::Config(ConfigScreenView {
            content: config(app),
            exclusion_editor: exclusion_editor(app),
        }),
        ViewMode::Help => BaseScreenView::Help(help(app)),
    };
    ScreenView {
        theme: app.theme(),
        top_bar: top_bar(app),
        base,
        confirm: confirm(app),
        palette: palette(app),
    }
}

pub(crate) fn config(app: &App) -> ConfigView {
    use crate::app::ConfigRowKind;

    let settings = app.settings();
    let detected = app.detected_diff_tools();
    let respect_gitignore = app.respect_gitignore();
    let sources = if respect_gitignore {
        ".gitignore + .duodiffignore"
    } else {
        ".gitignore (off) + .duodiffignore"
    };
    let rows = app
        .config_rows()
        .into_iter()
        .map(|row| match row {
            ConfigRowKind::Header(label) => ConfigRow {
                view: ConfigRowView::Header(label),
                control: ConfigControl::None,
            },
            ConfigRowKind::DiffToolAuto => ConfigRow {
                view: ConfigRowView::Choice {
                    label: format!(
                        "Auto ({})",
                        app.resolve_auto_diff_tool()
                            .map(|tool| tool.as_str())
                            .unwrap_or("none")
                    ),
                    selected: settings.external_diff_tool.is_auto(),
                    available: true,
                },
                control: ConfigControl::Select,
            },
            ConfigRowKind::DiffToolDisabled => ConfigRow {
                view: ConfigRowView::Choice {
                    label: "Disabled".to_string(),
                    selected: settings.external_diff_tool.is_disabled(),
                    available: true,
                },
                control: ConfigControl::Select,
            },
            ConfigRowKind::DiffTool { idx, available } => {
                let tool = detected[idx].0;
                ConfigRow {
                    view: ConfigRowView::Choice {
                        label: format!(
                            "{:<5} ({})",
                            tool.as_str(),
                            if available { "Available" } else { "Not Found" }
                        ),
                        selected: settings.external_diff_tool.pinned() == Some(tool),
                        available,
                    },
                    control: if available {
                        ConfigControl::Select
                    } else {
                        ConfigControl::Unavailable
                    },
                }
            }
            ConfigRowKind::DiffToolUnknown => ConfigRow {
                view: ConfigRowView::Choice {
                    label: format!(
                        "Unknown tool: {} (Not Found)",
                        settings
                            .external_diff_tool
                            .unknown_name()
                            .unwrap_or("unknown")
                    ),
                    selected: true,
                    available: false,
                },
                control: ConfigControl::Unavailable,
            },
            ConfigRowKind::CheckUpdates => {
                toggle_row("Check for updates daily", settings.check_updates)
            }
            ConfigRowKind::Mouse => toggle_row("Enable mouse support", settings.mouse),
            ConfigRowKind::Theme => toggle_row(
                "Light theme (off = dark)",
                settings.theme == crate::theme::ThemeChoice::Light,
            ),
            ConfigRowKind::DiffContext => ConfigRow {
                view: ConfigRowView::Value(format!(
                    "      Diff context: {} lines (h/l to adjust)",
                    settings.diff_context
                )),
                control: ConfigControl::Adjust,
            },
            ConfigRowKind::ScanMode => {
                let mut label = format!(
                    "      Scan mode: {} (Enter to switch)",
                    app.scan_mode().label()
                );
                if app.scan_mode_is_session_override() {
                    label.push_str(&format!(
                        "  ·  session override; saved default: {}",
                        app.saved_scan_mode().label()
                    ));
                }
                ConfigRow {
                    view: ConfigRowView::Value(label),
                    control: ConfigControl::Toggle,
                }
            }
            ConfigRowKind::RespectGitignore => toggle_row("Respect .gitignore", respect_gitignore),
            ConfigRowKind::GlobalExclusions => ConfigRow {
                view: ConfigRowView::Value(format!(
                    "      Global exclusions: {} rules (Enter to edit)",
                    settings.global_exclusions.len()
                )),
                control: ConfigControl::Select,
            },
            ConfigRowKind::IgnoreSources => ConfigRow {
                view: ConfigRowView::MutedLines(vec![
                    "      Sources (read-only)".to_string(),
                    format!(
                        "        Left {}/{}",
                        App::display_path_with_home_tilde(app.left_path()),
                        sources
                    ),
                    format!(
                        "        Right {}/{}",
                        App::display_path_with_home_tilde(app.right_path()),
                        sources
                    ),
                    format!("        CLI: {} rules", app.cli_exclusion_count()),
                ]),
                control: ConfigControl::None,
            },
        })
        .collect();
    ConfigView {
        rows,
        selected_idx: app.config().selected_idx(),
        theme: app.theme(),
    }
}

fn toggle_row(label: &'static str, enabled: bool) -> ConfigRow {
    ConfigRow {
        view: ConfigRowView::Toggle { label, enabled },
        control: ConfigControl::Toggle,
    }
}

pub(crate) fn top_bar(app: &App) -> TopBarView {
    TopBarView {
        screen: app.view_mode().into(),
        precise_mode: app.precise_mode(),
        diff_show_full: app.diff().show_full(),
        diff_wrap: app.diff().wrap(),
        scan_in_progress: app.scan_in_progress(),
        scan_progress_count: app.scan_progress_count(),
        spinner_frame: app.spinner_frame(),
        theme: app.theme(),
    }
}

pub(crate) fn tree(app: &App) -> TreeScreenView<'_> {
    let filter = app.filter();
    let row = app.selected_row().map(TreeRowView::from);
    TreeScreenView {
        content: TreeView {
            rows: TreeRowsView::new(filter.rows()),
            scroll_offset: app.scroll_offset(),
            selected_idx: app.selected_idx(),
            visible_height: app.viewport().visible_height,
            left_root: app.left_path(),
            right_root: app.right_path(),
            active_side_left: app.active_side_left(),
            theme: app.theme(),
            is_filter_active: !filter.pattern().is_empty() || filter.diffs_only(),
        },
        footer: TreeFooterView {
            row,
            status_toast: app.status_toast(),
            filter_active: filter.active(),
            filter_input: filter.input(),
            filter_pattern: filter.pattern(),
            filter_diffs_only: filter.editing_diffs_only(),
            scan_in_progress: app.scan_in_progress(),
            scan_progress_count: app.scan_progress_count(),
            spinner_frame: app.spinner_frame(),
            update_available: app.update_available(),
            install_method: app.install_method(),
            theme: app.theme(),
            summary: app.tree_summary(),
        },
        layout_inputs: tree_layout_inputs(app),
    }
}

pub(crate) fn help(app: &App) -> HelpScreenView<'_> {
    let help = app.help();
    HelpScreenView {
        content: HelpView {
            topic: help.topic().into(),
            index_open: help.index_open(),
            index_sel: help.index_sel(),
            scroll: help.scroll(),
            theme: app.theme(),
            update_available: app.update_available(),
            install_method: app.install_method(),
        },
    }
}

pub(crate) fn exclusion_editor(app: &App) -> Option<ExclusionEditorView> {
    app.exclusion_editor().map(|editor| ExclusionEditorView {
        draft: editor.draft().to_vec(),
        selected_idx: editor.selected_idx(),
        scroll_offset: editor.scroll_offset(),
        editing: editor.editing(),
        input: editor.input().clone(),
        theme: app.theme(),
    })
}

pub(crate) fn palette(app: &App) -> Option<PaletteView<'_>> {
    app.palette_visible().then(|| {
        let palette = app.palette();
        PaletteView {
            items: &palette.items,
            selected_idx: palette.selected_idx,
            scroll_offset: palette.scroll_offset,
            query: &palette.query,
            theme: app.theme(),
        }
    })
}

pub(crate) fn confirm(app: &App) -> Option<ConfirmView<'_>> {
    app.confirm_modal().map(|modal| ConfirmView {
        title: &modal.title,
        headline: &modal.headline,
        lines: &modal.lines,
        choices: modal
            .choices
            .iter()
            .map(|choice| ConfirmChoiceView {
                key: choice.key,
                label: &choice.label,
            })
            .collect(),
        theme: app.theme(),
    })
}

pub(crate) fn diff_layout_inputs(app: &App) -> crate::layout::DiffLayoutInputs {
    let row = app.selected_row();
    crate::layout::DiffLayoutInputs {
        has_changes: app.diff().has_changes(),
        row_has_content: row.is_some_and(|row| row.left.is_some() || row.right.is_some()),
        has_status: app.status_toast().is_some(),
        has_update: app.update_available().is_some(),
    }
}

pub(crate) fn tree_layout_inputs(app: &App) -> crate::layout::TreeLayoutInputs {
    let row = app.selected_row().map(TreeRowView::from);
    let filter = app.filter();
    crate::layout::TreeLayoutInputs {
        has_detail: row.is_some_and(|row| {
            row.is_ambiguous_case_collision || (row.left.is_some() && row.right.is_some())
        }),
        has_status: app.status_toast().is_some(),
        has_filter: filter.active(),
        has_update: app.update_available().is_some(),
        has_summary: app.tree_summary().is_some(),
    }
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
