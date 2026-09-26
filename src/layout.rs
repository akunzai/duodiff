//! Pure screen geometry shared by frame preparation, rendering, and mouse hit
//! testing.

use crate::commands::Command;
use crate::view::{BaseScreenView, ConfirmChoiceView, ConfirmView, HelpTopicView, ScreenView};
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect};

/// The three regions every screen carries: a top bar naming the screen, the
/// content, and a footer.
///
/// Help and Config read their geometry from here so painting and mouse hit
/// testing cannot disagree about where the content starts (Issue #300).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenLayout {
    pub top_bar: Rect,
    pub body: Rect,
    pub footer: Rect,
}

/// Geometry of the Help screen.
pub fn help_layout(area: Rect) -> ScreenLayout {
    screen_layout(0, area)
}

/// Geometry of the Config screen, which keeps room for its settings list.
pub fn config_layout(area: Rect) -> ScreenLayout {
    screen_layout(5, area)
}

/// One row of top bar, one row of footer, and `min_body` rows of content the
/// footer may not eat into.
fn screen_layout(min_body: u16, area: Rect) -> ScreenLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(min_body),
            Constraint::Length(1),
        ])
        .split(area);
    ScreenLayout {
        top_bar: chunks[0],
        body: chunks[1],
        footer: chunks[2],
    }
}

/// Rect of a screen's close button within `area`, or `None` when `area` is too
/// narrow to carry one.
///
/// Single owner of that geometry: painting (`ui::draw_close_button`) and
/// [`hit_test`] must agree on it.
pub fn close_button_rect(area: Rect) -> Option<Rect> {
    if area.width < 6 {
        return None;
    }
    Some(Rect {
        x: area.x + area.width.saturating_sub(5),
        y: area.y,
        width: 3,
        height: 1,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct TreeLayoutInputs {
    /// How many rows the footer's view lists.
    pub footer_rows: u16,
}

pub struct TreeLayout {
    pub top_bar: Rect,
    pub left: Rect,
    pub indicator: Rect,
    pub right: Rect,
    pub footer: Rect,
}

pub fn tree_layout(inputs: &TreeLayoutInputs, area: Rect) -> TreeLayout {
    let footer_height = inputs.footer_rows;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(footer_height),
        ])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(30),
            Constraint::Length(4),
            Constraint::Min(30),
        ])
        .split(chunks[1]);
    TreeLayout {
        top_bar: chunks[0],
        left: body[0],
        indicator: body[1],
        right: body[2],
        footer: chunks[2],
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiffLayoutInputs {
    pub has_changes: bool,
    pub row_has_content: bool,
    /// How many rows the footer's view lists.
    pub footer_rows: u16,
}

pub struct DiffLayout {
    pub top_bar: Rect,
    pub notice: Rect,
    pub info_left: Rect,
    pub info_right: Rect,
    pub left: Rect,
    pub right: Rect,
    pub footer: Rect,
    pub show_identical: bool,
}

pub fn diff_layout(inputs: &DiffLayoutInputs, area: Rect) -> DiffLayout {
    let show_identical = !inputs.has_changes && inputs.row_has_content;
    let header_height = if show_identical { 2 } else { 1 };
    let footer_height = inputs.footer_rows;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(footer_height),
        ])
        .split(area);
    let header = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(chunks[0]);
    let info = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);
    DiffLayout {
        top_bar: header[0],
        notice: header[1],
        info_left: info[0],
        info_right: info[1],
        left: body[0],
        right: body[1],
        footer: chunks[3],
        show_identical,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExclusionEditorLayout {
    pub popup: Rect,
    pub hint: Rect,
    pub list: Rect,
}

impl ExclusionEditorLayout {
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

pub fn exclusion_editor_layout(item_count: usize, area: Rect) -> ExclusionEditorLayout {
    const MIN_WIDTH: u16 = 32;
    const MAX_WIDTH: u16 = 96;
    const CHROME_HEIGHT: u16 = 3;
    let width = area
        .width
        .saturating_sub(4)
        .clamp(MIN_WIDTH, MAX_WIDTH)
        .min(area.width);
    let wanted = CHROME_HEIGHT.saturating_add(item_count.max(1) as u16);
    let height = wanted
        .min(area.height.saturating_sub(2))
        .max(CHROME_HEIGHT.min(area.height));
    let popup = centered_rect(width, height, area);
    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let hint = Rect {
        height: inner.height.min(1),
        ..inner
    };
    let list = Rect {
        y: inner.y.saturating_add(hint.height),
        height: inner.height.saturating_sub(hint.height),
        ..inner
    };
    ExclusionEditorLayout { popup, hint, list }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteLayout {
    pub popup: Rect,
    pub query: Rect,
    pub separator: Rect,
    pub list: Rect,
}

pub const PALETTE_MAX_WIDTH: u16 = 96;

impl PaletteLayout {
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

pub fn palette_layout(item_count: usize, area: Rect) -> PaletteLayout {
    const MIN_WIDTH: u16 = 40;
    const CHROME_HEIGHT: u16 = 4;
    let width = (area.width * 4 / 5)
        .clamp(MIN_WIDTH, PALETTE_MAX_WIDTH)
        .min(area.width);
    let wanted_rows = (item_count.max(1) as u16).saturating_add(CHROME_HEIGHT);
    let height = wanted_rows
        .min(area.height)
        .max(CHROME_HEIGHT.min(area.height));
    let popup = centered_rect(width, height, area);
    let inner = Rect {
        x: popup.x.saturating_add(1),
        y: popup.y.saturating_add(1),
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let query = Rect {
        height: inner.height.min(1),
        ..inner
    };
    let separator = Rect {
        y: inner.y.saturating_add(1),
        height: inner.height.saturating_sub(1).min(1),
        ..inner
    };
    let list = Rect {
        y: inner.y.saturating_add(2),
        height: inner.height.saturating_sub(2),
        ..inner
    };
    PaletteLayout {
        popup,
        query,
        separator,
        list,
    }
}

// Fixed spacing around the top bar's right-aligned Config/Help column: a
// leading space, a two-space gap between the links, and a trailing space.
// Named so `ui::draw_top_bar_content` (render) and `top_bar_links` (hit-test)
// read from the same source and cannot drift apart.
const TOPBAR_LEAD_SPACE: u16 = 1;
pub(crate) const TOPBAR_GAP: &str = "  ";
const TOPBAR_TRAIL_SPACE: u16 = 1;

/// The top bar's `[left title, right Config/Help column]` split. Shared by
/// `ui::draw_top_bar_content` (render) and `top_bar_links` (hit-test) so the column
/// boundary itself — not just the text within it — cannot drift between them.
pub(crate) fn top_bar_columns(area: Rect) -> (Rect, Rect) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(22)])
        .split(area);
    (layout[0], layout[1])
}

/// One top bar link's text, derived from the keymap so the label always
/// names the key that actually triggers it (Issue #339). `key` is `None`
/// when the Command has no key — the link still renders, just without a key
/// to highlight, and stays clickable.
///
/// The default keymap's Config ("C") and Help ("?") chords render as the
/// canonical embed (e.g. "(C)onfig"); any other chord — or no chord at all —
/// falls back to `Label (key)` / `Label`, so a remap never claims a key it
/// does not have.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) struct TopBarLink {
    /// `(text, highlighted)` pairs `draw_top_bar_content` renders in order;
    /// `top_bar_links` sums their widths for the same Rect.
    pub(crate) parts: Vec<(String, bool)>,
}

impl TopBarLink {
    fn new(key: Option<&str>, canonical_key: &str, embed_suffix: &str, label: &str) -> Self {
        let parts = match key {
            Some(k) if k == canonical_key => vec![
                ("(".to_string(), false),
                (k.to_string(), true),
                (format!("){embed_suffix}"), false),
            ],
            Some(k) => vec![
                (format!("{label} ("), false),
                (k.to_string(), true),
                (")".to_string(), false),
            ],
            None => vec![(label.to_string(), false)],
        };
        Self { parts }
    }

    fn width(&self) -> u16 {
        self.parts
            .iter()
            .map(|(text, _)| crate::wrap::display_width(text) as u16)
            .sum()
    }
}

pub(crate) fn top_bar_links_text(
    config_key: Option<&str>,
    help_key: Option<&str>,
) -> (TopBarLink, TopBarLink) {
    (
        TopBarLink::new(config_key, "C", "onfig", "Config"),
        TopBarLink::new(help_key, "?", "Help", "Help"),
    )
}

/// The clickable Rects for the top bar's Config/Help links, derived from the
/// same [`TopBarLink`] text `ui::draw_top_bar_content` renders — so the two
/// cannot drift apart. `area` is the top-bar's Rect (row 0, full width) —
/// same `Constraint::Length(22)` right column `draw_top_bar_content` splits
/// out. Each link's Rect covers its own text (e.g. "(C)onfig" or "Config
/// (x)"), not the surrounding lead space / gap / trailing space.
pub struct TopBarLinks {
    pub config: Rect,
    pub help: Rect,
}

pub fn top_bar_links(config_key: Option<&str>, help_key: Option<&str>, area: Rect) -> TopBarLinks {
    let (_, col) = top_bar_columns(area);
    let (config_link, help_link) = top_bar_links_text(config_key, help_key);
    let config_width = config_link.width();
    let help_width = help_link.width();

    let total_width = TOPBAR_LEAD_SPACE
        + config_width
        + TOPBAR_GAP.len() as u16
        + help_width
        + TOPBAR_TRAIL_SPACE;
    let text_start = col.x + col.width.saturating_sub(total_width);

    let config_x = text_start + TOPBAR_LEAD_SPACE;
    let help_x = config_x + config_width + TOPBAR_GAP.len() as u16;

    TopBarLinks {
        config: Rect {
            x: config_x,
            y: col.y,
            width: config_width,
            height: 1,
        },
        help: Rect {
            x: help_x,
            y: col.y,
            width: help_width,
            height: 1,
        },
    }
}

/// 0-indexed row of the clickable repo-URL line within the `About` topic body (see the
/// `HelpTopic::About` arm of `ui::help_topic_body`), which [`hit_test`] turns into
/// [`HitTarget::RepositoryLink`]. Stable regardless of update-check state since the URL line always
/// comes before the optional update-hint line.
pub(crate) const ABOUT_REPO_LINE: u16 = 2;

/// Horizontal and vertical padding between the confirm popup's border and its
/// text, so a path never runs into the frame.
pub(crate) const CONFIRM_PAD_X: u16 = 2;
pub(crate) const CONFIRM_PAD_Y: u16 = 1;
/// The popup tracks three quarters of the terminal between these bounds, so a
/// long path stays on one line without the dialog sprawling on a large screen.
const CONFIRM_MIN_WIDTH: u16 = 52;
const CONFIRM_MAX_WIDTH: u16 = 96;
/// Blank columns between two adjacent choice chips.
pub(crate) const CONFIRM_BUTTON_GAP: usize = 2;

/// One row of the confirm popup's body, in paint order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmRow {
    Headline(String),
    Text(String),
    Blank,
    /// The choices, by index into [`ConfirmView::choices`], sharing one row.
    Choices(std::ops::Range<usize>),
}

/// Geometry of the confirm popup: its rect and the wrapped body that sized it.
///
/// The painter draws these rows and hit testing reads the same rect, so the
/// close button is where it is painted (the popup's height follows the body).
#[derive(Clone, Debug)]
pub struct ConfirmLayout {
    pub popup: Rect,
    pub rows: Vec<ConfirmRow>,
}

pub fn confirm_layout(view: &ConfirmView<'_>, area: Rect) -> ConfirmLayout {
    let width = (area.width * 3 / 4)
        .clamp(CONFIRM_MIN_WIDTH, CONFIRM_MAX_WIDTH)
        .min(area.width);
    // Two borders plus the horizontal padding on each side.
    let inner_width = (width as usize)
        .saturating_sub(2 + 2 * CONFIRM_PAD_X as usize)
        .max(1);

    let mut rows = Vec::new();
    if !view.headline.is_empty() {
        for chunk in crate::wrap::lines(view.headline, inner_width) {
            rows.push(ConfirmRow::Headline(chunk));
        }
        if !view.lines.is_empty() {
            rows.push(ConfirmRow::Blank);
        }
    }
    for line in view.lines {
        if line.is_empty() {
            rows.push(ConfirmRow::Blank);
            continue;
        }
        for chunk in crate::wrap::lines_with_hanging_indent(line, inner_width) {
            rows.push(ConfirmRow::Text(chunk));
        }
    }
    if !rows.is_empty() {
        rows.push(ConfirmRow::Blank);
    }
    rows.extend(
        confirm_choice_rows(&view.choices, inner_width)
            .into_iter()
            .map(ConfirmRow::Choices),
    );

    // Two borders plus the vertical padding above and below the body.
    let height = (rows.len() as u16 + 2 + 2 * CONFIRM_PAD_Y).min(area.height);
    ConfirmLayout {
        popup: centered_rect(width, height, area),
        rows,
    }
}

/// The label a choice chip shows, e.g. ` [Y] Yes `.
pub(crate) fn confirm_chip_text(choice: &ConfirmChoiceView<'_>) -> String {
    format!(" [{}] {} ", choice.key.to_ascii_uppercase(), choice.label)
}

/// Group the choice chips into rows, wrapping onto further rows rather than
/// clipping when they do not all fit — every way out of a dialog has to stay
/// reachable on a small terminal (Issue #235).
pub(crate) fn confirm_choice_rows(
    choices: &[ConfirmChoiceView<'_>],
    inner_width: usize,
) -> Vec<std::ops::Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut used = 0usize;
    for (i, choice) in choices.iter().enumerate() {
        let chip_width = crate::wrap::display_width(&confirm_chip_text(choice));
        if i > start && used + CONFIRM_BUTTON_GAP + chip_width > inner_width {
            rows.push(start..i);
            start = i;
            used = 0;
        } else if i > start {
            used += CONFIRM_BUTTON_GAP;
        }
        used += chip_width;
    }
    if start < choices.len() {
        rows.push(start..choices.len());
    }
    rows
}

/// What a mouse position lands on in a painted frame.
///
/// Names the thing under the pointer, not what to do about it: the mouse
/// adapter maps each target to a Command or a state change, because that
/// depends on application state this module does not see (ADR-0003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTarget {
    /// Inside the Confirm popup or the exclusion editor's reach but on nothing
    /// clickable: a modal absorbs every position it does not claim.
    Modal,
    ConfirmClose,
    TopBarLink(Command),
    /// Inside the Command Palette but on nothing clickable.
    Palette,
    PaletteClose,
    /// An index into the palette's items, scroll applied; may be past the end.
    PaletteItem(usize),
    PaletteOutside,
    /// The close button of Help, Config, or File Diff.
    ScreenClose,
    /// An index into the Directory Tree's rows, scroll applied; may be past the end.
    TreeRow(usize),
    /// An index into the Config rows; may be past the end.
    ConfigRow(usize),
    /// An index into the Help topic index; may be past the end.
    HelpTopic(usize),
    RepositoryLink,
}

/// Resolve `(column, row)` against `screen` painted into `area`, topmost first
/// in the order `ui::draw` stacks them: Confirm, the exclusion editor, the
/// Command Palette, the top bar, then the base screen. A top bar link outside
/// the palette stays reachable while it is open.
pub fn hit_test(screen: &ScreenView<'_>, area: Rect, column: u16, row: u16) -> Option<HitTarget> {
    let at = Position::new(column, row);
    if let Some(confirm) = &screen.confirm {
        let popup = confirm_layout(confirm, area).popup;
        return Some(if close_button_contains(popup, at) {
            HitTarget::ConfirmClose
        } else {
            HitTarget::Modal
        });
    }
    if let BaseScreenView::Config(config) = &screen.base {
        if config.exclusion_editor.is_some() {
            return Some(HitTarget::Modal);
        }
    }

    // The palette is painted over everything but Confirm, the top bar
    // included when it runs the full height.
    let palette = screen
        .palette
        .as_ref()
        .map(|palette| (palette, palette_layout(palette.items.len(), area)));
    if let Some((palette, layout)) = &palette {
        if layout.popup.contains(at) {
            return Some(if close_button_contains(layout.popup, at) {
                HitTarget::PaletteClose
            } else if layout.list.contains(at) {
                HitTarget::PaletteItem(palette.scroll_offset + usize::from(row - layout.list.y))
            } else {
                HitTarget::Palette
            });
        }
    }

    let top_bar = match &screen.base {
        BaseScreenView::DirectoryTree(view) => tree_layout(&view.layout_inputs, area).top_bar,
        BaseScreenView::FileDiff(view) => diff_layout(&view.layout_inputs, area).top_bar,
        BaseScreenView::Config(_) => config_layout(area).top_bar,
        BaseScreenView::Help(_) => help_layout(area).top_bar,
    };
    if top_bar.contains(at) {
        let links = top_bar_links(
            screen.top_bar.config_key.as_deref(),
            screen.top_bar.help_key.as_deref(),
            top_bar,
        );
        return if links.config.contains(at) {
            Some(HitTarget::TopBarLink(Command::Config))
        } else if links.help.contains(at) {
            Some(HitTarget::TopBarLink(Command::Help))
        } else {
            None
        };
    }
    if palette.is_some() {
        return Some(HitTarget::PaletteOutside);
    }

    match &screen.base {
        BaseScreenView::DirectoryTree(view) => {
            // Rows span the full width: both panes and the indicator between
            // them show the same row.
            let list = inner(tree_layout(&view.layout_inputs, area).left);
            let offset = usize::from(row.checked_sub(list.y)?);
            (offset < view.content.visible_height)
                .then(|| HitTarget::TreeRow(view.content.scroll_offset + offset))
        }
        BaseScreenView::FileDiff(view) => {
            // With nothing to show, the painter draws no panes and no close
            // button, so nothing here is a target.
            view.content.info?;
            let layout = diff_layout(&view.layout_inputs, area);
            close_button_contains(layout.right, at).then_some(HitTarget::ScreenClose)
        }
        BaseScreenView::Config(_) => {
            let body = config_layout(area).body;
            if close_button_contains(body, at) {
                return Some(HitTarget::ScreenClose);
            }
            let list = inner(body);
            rows_contain(list, row).then(|| HitTarget::ConfigRow(usize::from(row - list.y)))
        }
        BaseScreenView::Help(view) => {
            let body = help_layout(area).body;
            if close_button_contains(body, at) {
                return Some(HitTarget::ScreenClose);
            }
            let text = inner(body);
            if view.content.index_open {
                return rows_contain(text, row)
                    .then(|| HitTarget::HelpTopic(usize::from(row - text.y)));
            }
            if view.content.topic != HelpTopicView::About {
                return None;
            }
            // The URL follows the line's two-column indent.
            let line = ABOUT_REPO_LINE.checked_sub(view.content.scroll)?;
            (row == text.y + line && column >= text.x + 2).then_some(HitTarget::RepositoryLink)
        }
    }
}

fn close_button_contains(area: Rect, at: Position) -> bool {
    close_button_rect(area).is_some_and(|button| button.contains(at))
}

/// `area` less its one-cell border.
fn inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

fn rows_contain(area: Rect, row: u16) -> bool {
    row >= area.y && row < area.y + area.height
}

/// Center a `width` x `height` popup inside `parent`.
///
/// Private so every popup's rect comes from its own layout function, which
/// painting and [`hit_test`] share.
fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((parent.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(parent);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((parent.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(rows[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, ViewMode};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    fn app_on(view_mode: ViewMode) -> App {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        if view_mode == ViewMode::FileDiff {
            // File Diff paints its panes, and the close button on them, only
            // for a row with content.
            let info = crate::diff::FileInfo {
                is_dir: false,
                size: 3,
                modified: std::time::SystemTime::UNIX_EPOCH,
            };
            app.directory_tree_mut()
                .set_flat_rows(vec![crate::app::FlatRow {
                    relative_path: PathBuf::from("a.txt"),
                    name: "a.txt".to_string(),
                    state: crate::diff::DiffState::DifferentNewerLeft,
                    left: Some(info.clone()),
                    right: Some(info),
                    ..Default::default()
                }]);
            app.apply_filter();
            app.directory_tree_mut().set_selected_idx(0);
        }
        app.set_view_mode(view_mode);
        crate::view::prepare_frame(&mut app, AREA);
        app
    }

    /// Paint `screen` the way the event loop does and return every cell that
    /// starts a `[x]` close button: one set into a border, unlike Config's
    /// `[x]` checkboxes.
    fn painted_close_buttons(screen: &ScreenView<'_>) -> Vec<Position> {
        let mut terminal = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, screen)).unwrap();
        let buffer = terminal.backend().buffer();
        let symbol = |x: u16, y: u16| buffer.cell((x, y)).map(|c| c.symbol().to_string());
        let mut found = Vec::new();
        for y in 0..AREA.height {
            for x in 1..AREA.width.saturating_sub(2) {
                if symbol(x - 1, y).as_deref() == Some("─")
                    && symbol(x, y).as_deref() == Some("[")
                    && symbol(x + 1, y).as_deref() == Some("x")
                    && symbol(x + 2, y).as_deref() == Some("]")
                {
                    found.push(Position::new(x, y));
                }
            }
        }
        found
    }

    fn hit(screen: &ScreenView<'_>, at: Position) -> Option<HitTarget> {
        hit_test(screen, AREA, at.x, at.y)
    }

    /// Every close button the painter draws is a close target on all three of
    /// its cells, on every screen that has one.
    #[test]
    fn a_painted_close_button_is_a_close_target() {
        for view_mode in [ViewMode::FileDiff, ViewMode::ConfigMenu, ViewMode::Help] {
            let app = app_on(view_mode);
            let screen = crate::view::assemble(&app);
            let buttons = painted_close_buttons(&screen);
            assert_eq!(buttons.len(), 1, "{view_mode:?}: {buttons:?}");
            for dx in 0..3 {
                let at = Position::new(buttons[0].x + dx, buttons[0].y);
                assert_eq!(
                    hit(&screen, at),
                    Some(HitTarget::ScreenClose),
                    "{view_mode:?}"
                );
            }
        }

        let mut app = app_on(ViewMode::DirectoryTree);
        app.open_palette();
        crate::view::prepare_frame(&mut app, AREA);
        let screen = crate::view::assemble(&app);
        let popup = palette_layout(screen.palette.unwrap().items.len(), AREA).popup;
        let buttons: Vec<_> = painted_close_buttons(&screen)
            .into_iter()
            .filter(|at| popup.contains(*at))
            .collect();
        assert_eq!(buttons.len(), 1, "{buttons:?}");
        assert_eq!(hit(&screen, buttons[0]), Some(HitTarget::PaletteClose));
    }

    /// A File Diff with nothing to show paints no panes, so no close button,
    /// and the spot where one would sit is not a target.
    #[test]
    fn an_empty_file_diff_has_no_close_target() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);
        crate::view::prepare_frame(&mut app, AREA);
        let screen = crate::view::assemble(&app);
        assert!(painted_close_buttons(&screen).is_empty());
        let right = diff_layout(&crate::view::diff_layout_inputs(&app), AREA).right;
        let spot = close_button_rect(right).unwrap();
        assert_eq!(hit(&screen, spot.as_position()), None);
    }

    /// The Confirm popup sizes itself to its body, so a body taller than a
    /// one-line prompt moves the close button — and the target moves with it.
    #[test]
    fn the_confirm_close_target_follows_the_painted_close_button() {
        let app = app_on(ViewMode::DirectoryTree);
        let lines = vec![
            "From   /a/very/long/source/path/that/wraps/onto/a/second/row/of/the/popup.txt"
                .to_string(),
            String::new(),
            "To     /dst.txt".to_string(),
        ];
        let choices = vec![
            ConfirmChoiceView {
                key: 'y',
                label: "Copy",
            },
            ConfirmChoiceView {
                key: 'n',
                label: "Cancel",
            },
        ];
        let mut screen = crate::view::assemble(&app);
        screen.confirm = Some(ConfirmView {
            title: "Confirm",
            headline: "Replace the destination?",
            lines: &lines,
            choices,
            theme: crate::theme::Theme::DARK,
        });

        let popup = confirm_layout(screen.confirm.as_ref().unwrap(), AREA).popup;
        assert!(popup.height > 7, "the body must be taller than a prompt");
        let buttons: Vec<_> = painted_close_buttons(&screen)
            .into_iter()
            .filter(|at| popup.contains(*at))
            .collect();
        assert_eq!(buttons.len(), 1, "the popup paints its close button");
        assert_eq!(hit(&screen, buttons[0]), Some(HitTarget::ConfirmClose));

        // Every other position, the top bar included, is absorbed.
        for (x, y) in [(0, 0), (75, 0), (popup.x, popup.y), (40, 23)] {
            assert_eq!(hit(&screen, Position::new(x, y)), Some(HitTarget::Modal));
        }
    }

    #[test]
    fn the_exclusion_editor_absorbs_every_position() {
        let mut app = app_on(ViewMode::ConfigMenu);
        app.open_exclusion_editor();
        let screen = crate::view::assemble(&app);
        for y in 0..AREA.height {
            for x in [0, 40, 76] {
                assert_eq!(hit(&screen, Position::new(x, y)), Some(HitTarget::Modal));
            }
        }
    }

    /// The palette covers the top bar when it runs the full height; where it
    /// does not, the top bar links stay reachable while it is open.
    #[test]
    fn the_palette_is_hit_above_the_top_bar_it_covers() {
        let tall = Rect::new(0, 0, 80, 60);
        let mut app = app_on(ViewMode::DirectoryTree);
        app.open_palette();
        crate::view::prepare_frame(&mut app, tall);
        let screen = crate::view::assemble(&app);
        let items = screen.palette.as_ref().unwrap().items.len();
        let layout = palette_layout(items, tall);
        assert!(
            layout.popup.y > 0,
            "a tall terminal leaves the top bar clear"
        );
        let links = top_bar_links(Some("C"), Some("?"), Rect::new(0, 0, tall.width, 1));
        let hit_tall = |at: Position| hit_test(&screen, tall, at.x, at.y);

        assert_eq!(
            hit_tall(links.config.as_position()),
            Some(HitTarget::TopBarLink(Command::Config))
        );
        assert_eq!(
            hit_tall(links.help.as_position()),
            Some(HitTarget::TopBarLink(Command::Help))
        );
        assert_eq!(hit_tall(Position::new(0, 0)), None);
        assert_eq!(
            hit_tall(Position::new(0, tall.height - 1)),
            Some(HitTarget::PaletteOutside)
        );
        assert_eq!(
            hit_tall(layout.list.as_position()),
            Some(HitTarget::PaletteItem(0))
        );
        assert_eq!(
            hit_tall(layout.query.as_position()),
            Some(HitTarget::Palette)
        );

        // On a short terminal the palette's top border is the first row.
        let mut app = app_on(ViewMode::DirectoryTree);
        app.open_palette();
        crate::view::prepare_frame(&mut app, AREA);
        let screen = crate::view::assemble(&app);
        let layout = palette_layout(screen.palette.as_ref().unwrap().items.len(), AREA);
        assert_eq!(layout.popup.y, 0, "the palette must cover the top bar");
        let close = close_button_rect(layout.popup).unwrap();
        assert_eq!(
            hit(&screen, close.as_position()),
            Some(HitTarget::PaletteClose)
        );
    }

    /// Tree rows start under the pane border and stop at the viewport, across
    /// both panes and the indicator column between them.
    #[test]
    fn tree_rows_fill_the_viewport_below_the_pane_border() {
        let app = app_on(ViewMode::DirectoryTree);
        let screen = crate::view::assemble(&app);
        let pane = tree_layout(&crate::view::tree_layout_inputs(&app), AREA).left;
        let visible = app.directory_tree().visible_height() as u16;
        assert!(visible > 0);

        assert_eq!(hit(&screen, Position::new(3, pane.y)), None);
        for x in [1, AREA.width / 2, AREA.width - 2] {
            assert_eq!(
                hit(&screen, Position::new(x, pane.y + 1)),
                Some(HitTarget::TreeRow(0))
            );
        }
        assert_eq!(
            hit(&screen, Position::new(3, pane.y + visible)),
            Some(HitTarget::TreeRow(visible as usize - 1))
        );
        assert_eq!(hit(&screen, Position::new(3, pane.y + 1 + visible)), None);
    }

    #[test]
    fn config_rows_start_under_the_body_border() {
        let app = app_on(ViewMode::ConfigMenu);
        let screen = crate::view::assemble(&app);
        let body = config_layout(AREA).body;

        assert_eq!(
            hit(&screen, Position::new(3, body.y + 1)),
            Some(HitTarget::ConfigRow(0))
        );
        assert_eq!(
            hit(&screen, Position::new(3, body.y + 4)),
            Some(HitTarget::ConfigRow(3))
        );
        // The bottom border and the footer below it are not rows.
        assert_eq!(hit(&screen, Position::new(3, body.bottom() - 1)), None);
        assert_eq!(hit(&screen, Position::new(3, AREA.height - 1)), None);
    }

    #[test]
    fn help_hits_a_topic_in_the_index_and_the_repository_link_in_about() {
        let mut app = app_on(ViewMode::Help);
        app.help_mut().set_index_open(true);
        let screen = crate::view::assemble(&app);
        let text = inner(help_layout(AREA).body);
        assert_eq!(
            hit(&screen, Position::new(5, text.y + 2)),
            Some(HitTarget::HelpTopic(2))
        );

        let mut app = app_on(ViewMode::Help);
        app.help_mut().set_index_open(false);
        app.help_mut().select_topic(crate::app::HelpTopic::About);
        let screen = crate::view::assemble(&app);
        let link_row = text.y + ABOUT_REPO_LINE;
        assert_eq!(
            hit(&screen, Position::new(text.x + 2, link_row)),
            Some(HitTarget::RepositoryLink)
        );
        assert_eq!(hit(&screen, Position::new(text.x + 1, link_row)), None);
        assert_eq!(hit(&screen, Position::new(text.x + 2, link_row + 1)), None);
    }

    /// An unbound Config still renders — and stays clickable — as the bare
    /// label, with no key to highlight (Issue #339).
    #[test]
    fn top_bar_link_with_no_key_renders_the_bare_label() {
        let link = TopBarLink::new(None, "C", "onfig", "Config");
        assert_eq!(link.parts, vec![("Config".to_string(), false)]);
        assert_eq!(link.width(), 6);
    }

    #[test]
    fn screen_layout_gives_the_body_everything_between_top_bar_and_footer() {
        for layout in [
            help_layout(Rect::new(0, 0, 80, 24)),
            config_layout(Rect::new(0, 0, 80, 24)),
        ] {
            assert_eq!(layout.top_bar, Rect::new(0, 0, 80, 1));
            assert_eq!(layout.body, Rect::new(0, 1, 80, 22));
            assert_eq!(layout.footer, Rect::new(0, 23, 80, 1));
        }
    }

    #[test]
    fn help_layout_keeps_its_footer_on_a_short_terminal() {
        let layout = help_layout(Rect::new(0, 0, 80, 3));
        assert_eq!(layout.top_bar.height, 1);
        assert_eq!(layout.body.height, 1);
        assert_eq!(layout.footer, Rect::new(0, 2, 80, 1));
    }

    #[test]
    fn config_layout_reserves_room_for_its_settings_list() {
        // Config asks for five body rows; Help asks for none, so on a terminal
        // that cannot satisfy both the two screens differ on purpose.
        let short = Rect::new(0, 0, 80, 6);
        assert!(config_layout(short).body.height >= help_layout(short).body.height);
        assert_eq!(config_layout(Rect::new(0, 0, 80, 20)).body.height, 18);
    }

    #[test]
    fn close_button_sits_inside_the_top_right_of_its_area() {
        let button = close_button_rect(Rect::new(0, 1, 80, 22)).expect("wide enough");
        assert_eq!(button, Rect::new(75, 1, 3, 1));
    }

    #[test]
    fn close_button_is_dropped_when_the_area_is_too_narrow() {
        assert_eq!(close_button_rect(Rect::new(0, 1, 5, 22)), None);
    }

    #[test]
    fn diff_layout_reserves_identical_notice_and_footer_rows() {
        let layout = diff_layout(
            &DiffLayoutInputs {
                has_changes: false,
                row_has_content: true,
                footer_rows: 3,
            },
            Rect::new(0, 0, 100, 20),
        );

        assert!(layout.show_identical);
        assert_eq!(layout.notice.height, 1);
        assert_eq!(layout.footer.height, 3);
    }
}
