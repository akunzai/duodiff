//! Pure screen geometry shared by frame preparation and rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
/// Single owner of that geometry: painting (`ui::draw_close_button`) and mouse
/// hit testing (`input`) must agree on it.
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
    pub has_detail: bool,
    pub has_status: bool,
    pub has_filter: bool,
    pub has_update: bool,
    pub has_summary: bool,
}

pub struct TreeLayout {
    pub top_bar: Rect,
    pub left: Rect,
    pub indicator: Rect,
    pub right: Rect,
    pub footer: Rect,
}

pub fn tree_layout(inputs: &TreeLayoutInputs, area: Rect) -> TreeLayout {
    let footer_height = 1u16
        + u16::from(inputs.has_detail)
        + u16::from(inputs.has_status)
        + u16::from(inputs.has_filter)
        + u16::from(inputs.has_update)
        + u16::from(inputs.has_summary);
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
    pub has_status: bool,
    pub has_update: bool,
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
    let footer_height =
        if inputs.has_status { 2 } else { 1 } + if inputs.has_update { 1 } else { 0 };
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

/// Center a `width` x `height` popup inside `parent`.
///
/// Single owner of the modal-centering geometry: frame preparation, rendering
/// (`ui::draw_confirm_content`), and hit testing (`input`) must agree on it.
pub(crate) fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
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
                has_status: true,
                has_update: true,
            },
            Rect::new(0, 0, 100, 20),
        );

        assert!(layout.show_identical);
        assert_eq!(layout.notice.height, 1);
        assert_eq!(layout.footer.height, 3);
    }
}
