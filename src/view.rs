//! Borrowed presentation snapshots assembled from application state.

use crate::app::{App, FlatRow, HelpTopic, ViewMode};
use crate::diff::{DiffState, TreeSummary};
use crate::theme::Theme;
use std::path::{Path, PathBuf};
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
            app.prepare_diff_viewport(layout.left.height.saturating_sub(2) as usize, pane_inner);
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
        let layout = crate::layout::palette_layout(app.palette().items().len(), area);
        app.palette_mut().sync_viewport(layout.visible_rows());
    }
}

/// A full frame assembled from one immutable borrow of [`App`].
#[derive(Debug)]
pub struct ScreenView<'a> {
    pub top_bar: TopBarView,
    pub base: BaseScreenView<'a>,
    pub confirm: Option<ConfirmView<'a>>,
    pub palette: Option<PaletteView<'a>>,
}

#[derive(Debug)]
pub enum BaseScreenView<'a> {
    DirectoryTree(TreeScreenView<'a>),
    FileDiff(DiffScreenView<'a>),
    Config(ConfigScreenView<'a>),
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

#[derive(Clone, Debug)]
pub struct TopBarView {
    pub screen: ScreenKind,
    pub precise_mode: bool,
    pub diff_show_full: bool,
    pub diff_wrap: bool,
    pub scan_in_progress: bool,
    pub scan_progress_count: usize,
    pub spinner_frame: usize,
    pub theme: Theme,
    /// The Config/Help links' key, from the keymap; `None` when unbound
    /// (still clickable, just with no key to highlight) (Issue #339).
    pub config_key: Option<String>,
    pub help_key: Option<String>,
}

#[derive(Debug)]
pub struct TreeScreenView<'a> {
    pub content: TreeView<'a>,
    pub footer: FooterView<'a>,
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
    pub left_name: &'a str,
    pub right_name: &'a str,
    pub left_relative_path: &'a Path,
    pub right_relative_path: &'a Path,
    pub state: DiffState,
    pub left: Option<FileInfoView>,
    pub right: Option<FileInfoView>,
    pub is_expanded: bool,
    pub has_case_conflict: bool,
    pub contains_case_conflict: bool,
    pub is_ambiguous_case_collision: bool,
}

impl TreeRowView<'_> {
    /// Whether the footer can render detail for this row.
    pub fn has_detail(self) -> bool {
        let has_left = self.left.is_some();
        let has_right = self.right.is_some();
        (self.is_ambiguous_case_collision && (has_left || has_right)) || (has_left && has_right)
    }
}

impl<'a> From<&'a FlatRow> for TreeRowView<'a> {
    fn from(row: &'a FlatRow) -> Self {
        Self {
            depth: row.depth,
            left_name: row.left_name(),
            right_name: row.right_name(),
            left_relative_path: row.left_relative_path(),
            right_relative_path: row.right_relative_path(),
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

/// One row of a screen's footer. The view lists a screen's rows, top to
/// bottom; the layout sizes the footer from how many there are, and the
/// painter draws each one, so the footer is exactly as tall as what it shows.
#[derive(Clone, Debug)]
pub enum FooterRow<'a> {
    /// The status toast.
    Toast { message: &'a str, is_error: bool },
    /// Size and time of each side of the selected Directory Tree row.
    Detail(TreeRowView<'a>),
    /// The filter being typed.
    FilterInput {
        input: &'a crate::text_input::TextInput,
        diffs_only: bool,
    },
    /// A filter that is applied.
    Filter { pattern: &'a str, diffs_only: bool },
    /// How many entries differ or sit on one side only.
    Summary(TreeSummary),
    /// The way out of File Diff's staged, unsaved edits.
    Staged { can_undo: bool },
    /// A background scan in flight, in place of the Command Palette hint.
    Scanning { count: usize, spinner_frame: usize },
    /// How to open the Command Palette, after File Diff's change keys when
    /// it has changes, and naming right-click where the Directory Tree has it.
    Palette {
        change_keys: bool,
        right_click: bool,
    },
    /// A newer release is available.
    Update { version: &'a str },
}

/// A screen's footer: its rows, and what painting them needs.
#[derive(Clone, Debug)]
pub struct FooterView<'a> {
    pub rows: Vec<FooterRow<'a>>,
    pub theme: Theme,
    pub install_method: &'a crate::upgrade::InstallMethod,
    /// So hints name each Command's real key (Issue #339).
    pub keymap: &'a crate::keymap::Keymap,
}

impl FooterView<'_> {
    /// How many rows the footer takes.
    pub fn height(&self) -> u16 {
        u16::try_from(self.rows.len()).unwrap_or(u16::MAX)
    }
}

#[derive(Debug)]
pub struct DiffScreenView<'a> {
    pub content: DiffView<'a>,
    pub footer: FooterView<'a>,
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
        HelpTopic::all().map(Self::from)
    }

    pub fn title(self) -> &'static str {
        let topic = match self {
            Self::DirectoryTree => HelpTopic::DirectoryTree,
            Self::FileDiff => HelpTopic::FileDiff,
            Self::Config => HelpTopic::Config,
            Self::Mouse => HelpTopic::Mouse,
            Self::General => HelpTopic::General,
            Self::About => HelpTopic::About,
        };
        topic.title()
    }
}

#[derive(Debug)]
pub struct HelpScreenView<'a> {
    pub content: HelpView<'a>,
    pub footer: FooterView<'a>,
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
    /// So the topic body and titles name each Command's real key (Issue #339).
    pub keymap: &'a crate::keymap::Keymap,
}

#[derive(Debug)]
pub struct ConfigScreenView<'a> {
    pub content: ConfigView,
    pub exclusion_editor: Option<ExclusionEditorView<'a>>,
    pub footer: FooterView<'a>,
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
        /// Said after the label, such as a session override.
        note: Option<String>,
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
    /// The Back command's key, for the contextual title's "Esc back"; `None`
    /// when unbound (Issue #339).
    pub back_key: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct ExclusionEditorView<'a> {
    pub draft: &'a [String],
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub editing: bool,
    pub input: &'a crate::text_input::TextInput,
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

/// Size and modification time of each side of the file pair File Diff shows.
#[derive(Clone, Copy, Debug)]
pub struct FilePairInfoView {
    pub left: Option<FileInfoView>,
    pub right: Option<FileInfoView>,
}

impl From<&FlatRow> for FilePairInfoView {
    fn from(row: &FlatRow) -> Self {
        Self {
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

#[derive(Clone, Debug)]
pub struct DiffView<'a> {
    pub rows: &'a [crate::diff_view::DiffRow],
    pub wrap: bool,
    pub scroll: usize,
    /// The rows of the change hunk under the cursor, highlighted as active.
    pub active_hunk: Option<std::ops::Range<usize>>,
    pub h_scroll: usize,
    pub visible_height: usize,
    pub content_width: usize,
    pub left_line_count: usize,
    pub right_line_count: usize,
    /// The files each pane shows, for its title.
    pub left_file: PathBuf,
    pub right_file: PathBuf,
    pub info: Option<FilePairInfoView>,
    pub left_hash: Option<&'a str>,
    pub right_hash: Option<&'a str>,
    pub left_line_ending: Option<&'a str>,
    pub right_line_ending: Option<&'a str>,
    pub theme: Theme,
    pub left_dirty: bool,
    pub right_dirty: bool,
    /// A file-pair side staging, saving, and copying cannot write.
    pub left_read_only: bool,
    pub right_read_only: bool,
}

pub fn assemble(app: &App) -> ScreenView<'_> {
    let base = match app.view_mode() {
        ViewMode::DirectoryTree => BaseScreenView::DirectoryTree(tree(app)),
        ViewMode::FileDiff => {
            let footer = footer(app, diff_footer_rows(app));
            BaseScreenView::FileDiff(DiffScreenView {
                content: diff(app),
                layout_inputs: diff_layout_inputs_for(app, &footer),
                footer,
            })
        }
        ViewMode::ConfigMenu => BaseScreenView::Config(ConfigScreenView {
            content: config(app),
            exclusion_editor: exclusion_editor(app),
            footer: footer(app, screen_footer_rows(app)),
        }),
        ViewMode::Help => BaseScreenView::Help(help(app)),
    };
    ScreenView {
        top_bar: top_bar(app),
        base,
        confirm: confirm(app),
        palette: palette(app),
    }
}

pub(crate) fn config(app: &App) -> ConfigView {
    use crate::app::ConfigRowKind;

    let settings = app.settings().saved();
    let detected = app.detected_diff_tools();
    let respect_gitignore = app.settings().respect_gitignore();
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
            ConfigRowKind::Mouse => override_row(
                "Enable mouse support",
                app.settings().mouse(),
                settings.mouse,
            ),
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
                    app.settings().scan_mode().label()
                );
                if app.settings().scan_mode_is_session_override() {
                    label.push_str(&session_override(settings.scan_mode.label()));
                }
                ConfigRow {
                    view: ConfigRowView::Value(label),
                    control: ConfigControl::Toggle,
                }
            }
            ConfigRowKind::RespectGitignore => override_row(
                "Respect .gitignore",
                respect_gitignore,
                settings.respect_gitignore,
            ),
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
                    format!(
                        "        CLI: {} rules",
                        app.settings().cli_exclusion_count()
                    ),
                ]),
                control: ConfigControl::None,
            },
            ConfigRowKind::KeyBindings => ConfigRow {
                view: ConfigRowView::MutedLines(vec![match app.keymap().customized_count() {
                    0 => "      Default keys (set [keys] in config.toml)".to_string(),
                    n => format!("      {n} custom (config.toml)"),
                }]),
                control: ConfigControl::None,
            },
        })
        .collect();
    ConfigView {
        rows,
        selected_idx: app.config().selected_idx(),
        theme: app.settings().theme(),
        back_key: app.keymap().key_phrase(crate::commands::Command::Back),
    }
}

/// What a Config row says while a command-line flag holds a value other
/// than the `saved` one in effect.
fn session_override(saved: &str) -> String {
    format!("  ·  session override; saved default: {saved}")
}

/// A toggle a command-line flag can start from: it shows the value in
/// effect, and says what is saved while the two differ.
fn override_row(label: &'static str, in_effect: bool, saved: bool) -> ConfigRow {
    let mut row = toggle_row(label, in_effect);
    if let ConfigRowView::Toggle { note, .. } = &mut row.view {
        *note = (in_effect != saved).then(|| session_override(if saved { "on" } else { "off" }));
    }
    row
}

fn toggle_row(label: &'static str, enabled: bool) -> ConfigRow {
    ConfigRow {
        view: ConfigRowView::Toggle {
            label,
            enabled,
            note: None,
        },
        control: ConfigControl::Toggle,
    }
}

pub(crate) fn top_bar(app: &App) -> TopBarView {
    TopBarView {
        screen: app.view_mode().into(),
        precise_mode: app.settings().scan_mode().is_precise(),
        diff_show_full: app.diff().show_full(),
        diff_wrap: app.diff().wrap(),
        scan_in_progress: app.scan().in_progress(),
        scan_progress_count: app.scan().progress_count(),
        spinner_frame: app.scan().spinner_frame(),
        theme: app.settings().theme(),
        config_key: app.keymap().key_phrase(crate::commands::Command::Config),
        help_key: app.keymap().key_phrase(crate::commands::Command::Help),
    }
}

pub(crate) fn tree(app: &App) -> TreeScreenView<'_> {
    let filter = app.directory_tree();
    let footer = footer(app, tree_footer_rows(app));
    TreeScreenView {
        content: TreeView {
            rows: TreeRowsView::new(filter.rows()),
            scroll_offset: app.directory_tree().scroll_offset(),
            selected_idx: app.directory_tree().selected_idx(),
            visible_height: app.directory_tree().visible_height(),
            left_root: app.left_path(),
            right_root: app.right_path(),
            active_side_left: app.active_side_left(),
            theme: app.settings().theme(),
            is_filter_active: !filter.pattern().is_empty() || filter.diffs_only(),
        },
        layout_inputs: crate::layout::TreeLayoutInputs {
            footer_rows: footer.height(),
        },
        footer,
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
            theme: app.settings().theme(),
            update_available: app.update_available(),
            install_method: app.install_method(),
            keymap: app.keymap(),
        },
        footer: footer(app, screen_footer_rows(app)),
    }
}

pub(crate) fn exclusion_editor(app: &App) -> Option<ExclusionEditorView<'_>> {
    app.exclusion_editor().map(|editor| ExclusionEditorView {
        draft: editor.draft(),
        selected_idx: editor.selected_idx(),
        scroll_offset: editor.scroll_offset(),
        editing: editor.editing(),
        input: editor.input(),
        theme: app.settings().theme(),
    })
}

pub(crate) fn palette(app: &App) -> Option<PaletteView<'_>> {
    app.palette_visible().then(|| {
        let palette = app.palette();
        PaletteView {
            items: palette.items(),
            selected_idx: palette.selected_idx(),
            scroll_offset: palette.scroll_offset(),
            query: palette.query(),
            theme: app.settings().theme(),
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
        theme: app.settings().theme(),
    })
}

pub(crate) fn diff_layout_inputs(app: &App) -> crate::layout::DiffLayoutInputs {
    diff_layout_inputs_for(app, &footer(app, diff_footer_rows(app)))
}

fn diff_layout_inputs_for(app: &App, footer: &FooterView<'_>) -> crate::layout::DiffLayoutInputs {
    let row = app.selected_row();
    crate::layout::DiffLayoutInputs {
        has_changes: app.diff().has_changes(),
        row_has_content: app.file_pair().is_some()
            || row.is_some_and(|row| row.left.is_some() || row.right.is_some()),
        footer_rows: footer.height(),
    }
}

pub(crate) fn tree_layout_inputs(app: &App) -> crate::layout::TreeLayoutInputs {
    crate::layout::TreeLayoutInputs {
        footer_rows: footer(app, tree_footer_rows(app)).height(),
    }
}

fn footer<'a>(app: &'a App, rows: Vec<FooterRow<'a>>) -> FooterView<'a> {
    FooterView {
        rows,
        theme: app.settings().theme(),
        install_method: app.install_method(),
        keymap: app.keymap(),
    }
}

fn toast_row(app: &App) -> Option<FooterRow<'_>> {
    app.status_toast()
        .map(|(message, is_error)| FooterRow::Toast { message, is_error })
}

fn update_row(app: &App) -> Option<FooterRow<'_>> {
    app.update_available()
        .map(|version| FooterRow::Update { version })
}

/// The Directory Tree's footer, top to bottom: toast, the selected row's
/// detail, the filter, the summary, the Command Palette hint or scan
/// progress, and the update notice.
pub(crate) fn tree_footer_rows(app: &App) -> Vec<FooterRow<'_>> {
    let tree = app.directory_tree();
    let mut rows: Vec<FooterRow<'_>> = toast_row(app).into_iter().collect();
    if let Some(row) = app
        .selected_row()
        .map(TreeRowView::from)
        .filter(|row| row.has_detail())
    {
        rows.push(FooterRow::Detail(row));
    }
    if tree.active() {
        rows.push(FooterRow::FilterInput {
            input: tree.input(),
            diffs_only: tree.editing_diffs_only(),
        });
    } else if !tree.pattern().is_empty() || tree.editing_diffs_only() {
        rows.push(FooterRow::Filter {
            pattern: tree.pattern(),
            diffs_only: tree.editing_diffs_only(),
        });
    }
    if let Some(summary) = tree.tree_summary() {
        rows.push(FooterRow::Summary(summary));
    }
    rows.push(if app.scan().in_progress() {
        FooterRow::Scanning {
            count: app.scan().progress_count(),
            spinner_frame: app.scan().spinner_frame(),
        }
    } else {
        FooterRow::Palette {
            change_keys: false,
            right_click: true,
        }
    });
    rows.extend(update_row(app));
    rows
}

/// The footer of Config and Help: the toast, then the Command Palette hint.
pub(crate) fn screen_footer_rows(app: &App) -> Vec<FooterRow<'_>> {
    let mut rows: Vec<FooterRow<'_>> = toast_row(app).into_iter().collect();
    rows.push(FooterRow::Palette {
        change_keys: false,
        right_click: false,
    });
    rows
}

/// File Diff's footer, top to bottom: toast, the staged-edits hint, the
/// change keys and Command Palette hint, and the update notice.
pub(crate) fn diff_footer_rows(app: &App) -> Vec<FooterRow<'_>> {
    let diff = app.diff();
    let mut rows: Vec<FooterRow<'_>> = toast_row(app).into_iter().collect();
    if diff.left_dirty() || diff.right_dirty() {
        rows.push(FooterRow::Staged {
            can_undo: diff.can_undo(),
        });
    }
    rows.push(FooterRow::Palette {
        change_keys: diff.has_changes(),
        right_click: false,
    });
    rows.extend(update_row(app));
    rows
}

pub(crate) fn diff(app: &App) -> DiffView<'_> {
    let diff = app.diff();
    let pair = app.file_pair();
    // A file pair's titles show the paths as typed; a Directory Tree row's show
    // the row under each root.
    let ((left_file, right_file), info) = match pair {
        Some(pair) => {
            let (left, right) = app.file_pair_info();
            (
                (
                    pair.left.path().to_path_buf(),
                    pair.right.path().to_path_buf(),
                ),
                Some(FilePairInfoView {
                    left: left.map(FileInfoView::from),
                    right: right.map(FileInfoView::from),
                }),
            )
        }
        None => (
            app.diff_file_paths().unwrap_or_default(),
            app.selected_row().map(FilePairInfoView::from),
        ),
    };
    DiffView {
        rows: diff.rows(),
        wrap: diff.wrap(),
        scroll: diff.scroll(),
        active_hunk: diff.active_hunk_rows(),
        h_scroll: diff.h_scroll(),
        visible_height: diff.visible_height(),
        content_width: diff.content_width(),
        left_line_count: diff.left_line_count(),
        right_line_count: diff.right_line_count(),
        left_file,
        right_file,
        info,
        left_hash: diff.left_hash(),
        right_hash: diff.right_hash(),
        left_line_ending: diff.left_line_ending(),
        right_line_ending: diff.right_line_ending(),
        theme: app.settings().theme(),
        left_dirty: diff.left_dirty(),
        right_dirty: diff.right_dirty(),
        left_read_only: pair.is_some_and(|pair| !pair.left.is_writable()),
        right_read_only: pair.is_some_and(|pair| !pair.right.is_writable()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ViewMode};
    use std::path::PathBuf;

    fn file_info() -> crate::diff::FileInfo {
        crate::diff::FileInfo {
            is_dir: false,
            size: 1,
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    /// A session started with `--no-mouse` shows mouse support off and says
    /// the saved value is on; once Config changes it, the note is gone.
    #[test]
    fn the_mouse_row_says_when_no_mouse_holds_it_off() {
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
        let mouse_row = |app: &App| {
            let idx = app
                .config_rows()
                .iter()
                .position(|row| *row == crate::app::ConfigRowKind::Mouse)
                .unwrap();
            match config(app).rows.swap_remove(idx).view {
                ConfigRowView::Toggle { enabled, note, .. } => (enabled, note),
                other => panic!("not a toggle: {other:?}"),
            }
        };

        assert_eq!(
            mouse_row(&app),
            (
                false,
                Some("  ·  session override; saved default: on".to_string())
            )
        );

        app.open_config();
        let idx = app
            .config_rows()
            .iter()
            .position(|row| *row == crate::app::ConfigRowKind::Mouse)
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.apply_config_selection();
        assert_eq!(mouse_row(&app), (true, None));
    }

    #[test]
    fn tree_row_view_projects_resolved_side_names_and_paths() {
        let row = FlatRow {
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            left_name_raw: Some("FILE.TXT".to_string()),
            right_name_raw: Some("file.txt".to_string()),
            left_relative_path_raw: Some(PathBuf::from("DIR/FILE.TXT")),
            right_relative_path_raw: Some(PathBuf::from("dir/file.txt")),
            ..Default::default()
        };

        let view = TreeRowView::from(&row);

        assert_eq!(view.left_name, "FILE.TXT");
        assert_eq!(view.right_name, "file.txt");
        assert_eq!(view.left_relative_path, Path::new("DIR/FILE.TXT"));
        assert_eq!(view.right_relative_path, Path::new("dir/file.txt"));

        let fallback_row = FlatRow {
            relative_path: PathBuf::from("nested/file.txt"),
            name: "file.txt".to_string(),
            ..Default::default()
        };
        let fallback_view = TreeRowView::from(&fallback_row);

        assert_eq!(fallback_view.left_name, "file.txt");
        assert_eq!(fallback_view.right_name, "file.txt");
        assert_eq!(
            fallback_view.left_relative_path,
            Path::new("nested/file.txt")
        );
        assert_eq!(
            fallback_view.right_relative_path,
            Path::new("nested/file.txt")
        );
    }

    #[test]
    fn ordinary_tree_row_has_detail_only_when_both_sides_exist() {
        let paired = FlatRow {
            left: Some(file_info()),
            right: Some(file_info()),
            ..Default::default()
        };
        assert!(TreeRowView::from(&paired).has_detail());

        let left_only = FlatRow {
            left: Some(file_info()),
            ..Default::default()
        };
        assert!(!TreeRowView::from(&left_only).has_detail());

        let right_only = FlatRow {
            right: Some(file_info()),
            ..Default::default()
        };
        assert!(!TreeRowView::from(&right_only).has_detail());

        let neither = FlatRow::default();
        assert!(!TreeRowView::from(&neither).has_detail());
    }

    #[test]
    fn ambiguous_tree_row_has_detail_when_either_or_both_sides_exist() {
        let left_collision = FlatRow {
            left: Some(file_info()),
            is_ambiguous_case_collision: true,
            ..Default::default()
        };
        assert!(TreeRowView::from(&left_collision).has_detail());

        let right_collision = FlatRow {
            right: Some(file_info()),
            is_ambiguous_case_collision: true,
            ..Default::default()
        };
        assert!(TreeRowView::from(&right_collision).has_detail());

        let paired_collision = FlatRow {
            left: Some(file_info()),
            right: Some(file_info()),
            is_ambiguous_case_collision: true,
            ..Default::default()
        };
        assert!(TreeRowView::from(&paired_collision).has_detail());
    }

    #[test]
    fn ambiguous_tree_row_without_sides_has_no_detail() {
        let collision = FlatRow {
            is_ambiguous_case_collision: true,
            ..Default::default()
        };

        assert!(!TreeRowView::from(&collision).has_detail());
    }

    #[test]
    fn tree_layout_inputs_follow_the_selected_row_detail_contract() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            is_ambiguous_case_collision: true,
            ..Default::default()
        }]);
        app.apply_filter();

        assert!(!tree_footer_rows(&app)
            .iter()
            .any(|row| matches!(row, FooterRow::Detail(_))));
    }

    #[test]
    fn exclusion_editor_view_borrows_editor_state() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_exclusion_editor();
        let editor = app.exclusion_editor().expect("editor open");

        let view = exclusion_editor(&app).expect("editor view");

        assert!(std::ptr::eq(view.draft, editor.draft()));
        assert!(std::ptr::eq(view.input, editor.input()));
    }

    #[test]
    fn assemble_projects_file_diff_without_ui_types() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        let screen = assemble(&app);

        assert!(matches!(screen.base, BaseScreenView::FileDiff(_)));
    }
}
