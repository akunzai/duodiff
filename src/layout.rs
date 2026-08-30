//! Pure screen geometry shared by frame preparation and rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

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
pub struct PopupLayout {
    pub popup: Rect,
    pub hint: Rect,
    pub list: Rect,
}

impl PopupLayout {
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

pub fn exclusion_editor_layout(item_count: usize, area: Rect) -> PopupLayout {
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
    PopupLayout { popup, hint, list }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteLayout {
    pub popup: Rect,
    pub query: Rect,
    pub separator: Rect,
    pub list: Rect,
}

impl PaletteLayout {
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

pub fn palette_layout(item_count: usize, area: Rect) -> PaletteLayout {
    const MIN_WIDTH: u16 = 40;
    const MAX_WIDTH: u16 = 96;
    const CHROME_HEIGHT: u16 = 4;
    let width = (area.width * 4 / 5)
        .clamp(MIN_WIDTH, MAX_WIDTH)
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
