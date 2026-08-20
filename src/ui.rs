use crate::app::{App, FlatRow, HelpTopic, ViewMode};
use crate::diff::DiffState;
use crate::theme::Theme;
use ratatui::{prelude::*, widgets::*};
use std::time::SystemTime;
use unicode_width::UnicodeWidthChar;

/// Format a `SystemTime` as a UTC datetime string (`YYYY-MM-DD HH:MM:SS UTC`).
/// Uses UTC everywhere so we do not need platform-specific localtime (no `libc`).
fn format_system_time(t: &SystemTime) -> String {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            let total_secs = dur.as_secs() as i64;
            let s = total_secs.rem_euclid(60);
            let m = (total_secs / 60).rem_euclid(60);
            let h = (total_secs / 3600).rem_euclid(24);
            let days = total_secs.div_euclid(86400);
            let (y, mo, d) = days_to_date(days);
            format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
        }
        Err(_) => "unknown".to_string(),
    }
}

/// Gregorian civil date for `days_since_epoch` days after 1970-01-01 (UTC).
fn days_to_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let mut y = 1970;
    let mut remaining = days_since_epoch;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md {
            mo = i as i64 + 1;
            break;
        }
        remaining -= md;
    }
    (y, mo, remaining + 1)
}

/// Build a detail info string for the selected row showing modification times
/// and sizes when both sides exist and differ.
///
/// `pub(crate)`: also called from [`App::tree_layout_inputs`] (has_detail), not just
/// [`draw_tree_footer`] — widened rather than re-deriving the same `DiffState` match twice.
/// `(left_tag, right_tag)` marking whichever side is newer, or two empty strings
/// when the timestamps match.
///
/// Derived from the timestamps rather than the `DiffState`, because the
/// `≈` [`DiffState::Unverified`] state deliberately carries no newer-side
/// variant — and `DifferentNewerLeft`/`Right` come from these same two
/// timestamps anyway, so one source of truth serves both.
fn newer_tag(
    left: std::time::SystemTime,
    right: std::time::SystemTime,
) -> (&'static str, &'static str) {
    match left.cmp(&right) {
        std::cmp::Ordering::Greater => (" (newer)", ""),
        std::cmp::Ordering::Less => ("", " (newer)"),
        std::cmp::Ordering::Equal => ("", ""),
    }
}

pub(crate) fn selected_row_detail(row: Option<&FlatRow>) -> Option<(String, String)> {
    let row = row?;
    let unverified_reason = match row.state {
        DiffState::DifferentNewerLeft
        | DiffState::DifferentNewerRight
        | DiffState::DifferentSameTime => None,
        // `≈` rows carry the same size/time detail, plus why the contents were
        // never compared (Issue #232).
        DiffState::Unverified(reason) => Some(reason),
        _ => return None,
    };
    let left = row.left.as_ref()?;
    let right = row.right.as_ref()?;

    let left_time = format_system_time(&left.modified);
    let right_time = format_system_time(&right.modified);

    let (left_tag, right_tag) = newer_tag(left.modified, right.modified);
    let reason = unverified_reason
        .map(|r| format!("  ·  {}", r.detail()))
        .unwrap_or_default();

    if left.is_dir {
        Some((
            format!("{}{}", left_time, left_tag),
            format!("{}{}{}", right_time, right_tag, reason),
        ))
    } else {
        Some((
            format!("{} {}{}", format_size(left.size), left_time, left_tag),
            format!(
                "{} {}{}{}",
                format_size(right.size),
                right_time,
                right_tag,
                reason
            ),
        ))
    }
}

/// Format byte size in a human-friendly form.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Render `input`'s text with a reverse-video block cursor at its real (char, not byte)
/// position, so multi-byte CJK text and mid-string editing show the cursor correctly.
/// `cursor_style` is the caller's accent colour; the cursor block always reverses video
/// regardless, so it stays visible over any text colour.
fn text_input_spans(
    input: &crate::text_input::TextInput,
    cursor_style: Style,
) -> Vec<Span<'static>> {
    let chars: Vec<char> = input.chars().collect();
    let cursor = input.cursor().min(chars.len());
    let rev = cursor_style.add_modifier(Modifier::REVERSED);
    let mut spans = Vec::new();
    let before: String = chars[..cursor].iter().collect();
    if !before.is_empty() {
        spans.push(Span::raw(before));
    }
    if cursor < chars.len() {
        spans.push(Span::styled(chars[cursor].to_string(), rev));
        let after: String = chars[cursor + 1..].iter().collect();
        if !after.is_empty() {
            spans.push(Span::raw(after));
        }
    } else {
        spans.push(Span::styled(" ".to_string(), rev));
    }
    spans
}

/// Pure title-bar state for the shared top chrome (Config / Help shortcuts).
#[derive(Clone, Copy, Debug)]
pub struct TopBarView {
    pub view_mode: ViewMode,
    pub precise_mode: bool,
    pub diff_show_full: bool,
    pub diff_wrap: bool,
    pub theme: Theme,
}

/// Render the shared top bar from an [`App`] (projects via [`App::top_bar_view`]).
pub fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    draw_top_bar_content(f, &app.top_bar_view(), area);
}

// Text spans `draw_top_bar_content`'s right-aligned column renders, named so the
// painter and `top_bar_links`'s hit-test geometry read from the same source and
// cannot drift apart.
const TOPBAR_LEAD: &str = " (";
const TOPBAR_CONFIG_KEY: &str = "C";
const TOPBAR_CONFIG_LABEL: &str = ")onfig";
const TOPBAR_GAP: &str = "  ";
const TOPBAR_HELP_LEAD: &str = "(";
const TOPBAR_HELP_KEY: &str = "?";
const TOPBAR_HELP_LABEL: &str = ")Help";
const TOPBAR_TRAIL: &str = " ";

/// The top bar's `[left title, right Config/Help column]` split. Shared by
/// `draw_top_bar_content` (render) and `top_bar_links` (hit-test) so the column
/// boundary itself — not just the text within it — cannot drift between them.
fn top_bar_columns(area: Rect) -> (Rect, Rect) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(22)])
        .split(area);
    (layout[0], layout[1])
}

/// Paint the top bar from a hand-built [`TopBarView`] (no full `App`).
pub fn draw_top_bar_content(f: &mut Frame, view: &TopBarView, area: Rect) {
    let theme = view.theme;
    let (left_col, right_col) = top_bar_columns(area);

    let left_text = match view.view_mode {
        ViewMode::DirectoryTree => {
            if view.precise_mode {
                " duodiff - Directory Tree [Precise] ".to_string()
            } else {
                " duodiff - Directory Tree [Fast] ".to_string()
            }
        }
        ViewMode::FileDiff => {
            let context_label = if view.diff_show_full {
                "Full"
            } else {
                "Diff Only"
            };
            let wrap_label = if view.diff_wrap { "Wrap" } else { "No Wrap" };
            format!(" duodiff - File Diff [{}] [{}] ", context_label, wrap_label)
        }
        ViewMode::ConfigMenu => " duodiff - Configuration ".to_string(),
        ViewMode::Help => " duodiff - Help ".to_string(),
    };

    let left_p = Paragraph::new(Line::from(vec![Span::styled(
        left_text,
        Style::default().fg(theme.emphasis).bold(),
    )]));
    f.render_widget(left_p, left_col);

    let right_p = Paragraph::new(Line::from(vec![
        Span::styled(TOPBAR_LEAD, Style::default().fg(theme.muted)),
        Span::styled(TOPBAR_CONFIG_KEY, Style::default().fg(theme.accent).bold()),
        Span::styled(TOPBAR_CONFIG_LABEL, Style::default().fg(theme.muted)),
        Span::raw(TOPBAR_GAP),
        Span::styled(TOPBAR_HELP_LEAD, Style::default().fg(theme.muted)),
        Span::styled(TOPBAR_HELP_KEY, Style::default().fg(theme.accent).bold()),
        Span::styled(TOPBAR_HELP_LABEL, Style::default().fg(theme.muted)),
        Span::raw(TOPBAR_TRAIL),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(right_p, right_col);
}

/// The clickable Rects for the top bar's "(C)onfig"/"(?)Help" links, derived from
/// the same span-width constants `draw_top_bar_content` renders from — so the two
/// cannot drift apart. `area` is the top-bar's Rect (row 0, full width) — same
/// `Constraint::Length(22)` right column `draw_top_bar_content` splits out. Each
/// link's Rect covers its key + label text (e.g. "(C)onfig"), not the surrounding
/// lead space / gap / trailing space.
pub struct TopBarLinks {
    pub config: Rect,
    pub help: Rect,
}

pub fn top_bar_links(area: Rect) -> TopBarLinks {
    let (_, col) = top_bar_columns(area);

    let total_width = (TOPBAR_LEAD.len()
        + TOPBAR_CONFIG_KEY.len()
        + TOPBAR_CONFIG_LABEL.len()
        + TOPBAR_GAP.len()
        + TOPBAR_HELP_LEAD.len()
        + TOPBAR_HELP_KEY.len()
        + TOPBAR_HELP_LABEL.len()
        + TOPBAR_TRAIL.len()) as u16;
    let text_start = col.x + col.width.saturating_sub(total_width);

    let config_x = text_start + TOPBAR_LEAD.len() as u16 - 1; // include TOPBAR_LEAD's '('
    let config_width = 1 + TOPBAR_CONFIG_KEY.len() as u16 + TOPBAR_CONFIG_LABEL.len() as u16;

    let help_x = config_x + config_width + TOPBAR_GAP.len() as u16;
    let help_width = TOPBAR_HELP_LEAD.len() as u16
        + TOPBAR_HELP_KEY.len() as u16
        + TOPBAR_HELP_LABEL.len() as u16;

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

pub fn draw(f: &mut Frame, app: &mut App) {
    // Paint the full canvas so every unfilled cell uses the theme background (no-op for
    // dark theme where bg=Reset; effective for light theme which sets a white canvas).
    f.render_widget(Block::default().style(app.theme().base_style()), f.area());

    match app.view_mode() {
        ViewMode::DirectoryTree => {
            draw_tree(f, app);
            if app.confirm_modal().is_some() {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::FileDiff => {
            draw_diff(f, app);
            if app.confirm_modal().is_some() {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::ConfigMenu => draw_config(f, app),
        ViewMode::Help => draw_help(f, app),
    }

    if app.palette_visible() {
        draw_palette(f, app);
    }
}

fn get_display_path(path: &std::path::Path, max_len: usize) -> String {
    let path_str = path.to_string_lossy();
    if path_str.len() <= max_len {
        return path_str.into_owned();
    }

    let sep = std::path::MAIN_SEPARATOR.to_string();
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|s| !s.is_empty() && s != &sep)
        .collect();

    if components.is_empty() {
        return path_str.into_owned();
    }

    let last = &components[components.len() - 1];
    let mut right_part = last.to_string();
    let mut idx = components.len().saturating_sub(2);
    while idx > 0 {
        let next_part = format!("{}{}{}", components[idx], sep, right_part);
        if next_part.len() + 4 <= max_len {
            right_part = next_part;
            idx -= 1;
        } else {
            break;
        }
    }

    format!("...{}{}", sep, right_part)
}

/// Borrowed render state for the directory-tree **content** region (dual panes + indicator).
///
/// Built by [`App::tree_view`] in production, or hand-assembled in ui tests without
/// a full [`App`]. Top bar and footer stay on the [`draw_tree`] shell.
#[derive(Clone, Copy, Debug)]
pub struct TreeView<'a> {
    /// Filtered tree rows (full list; view applies scroll/selection).
    pub rows: &'a [FlatRow],
    pub scroll_offset: usize,
    pub selected_idx: usize,
    pub visible_height: usize,
    pub left_root: &'a std::path::Path,
    pub right_root: &'a std::path::Path,
    pub active_side_left: bool,
    pub theme: Theme,
}

/// Borrowed render state for the directory-tree **footer** region (status toast, detail
/// line, filter bar, keybindings/scan banner, update hint).
///
/// Built by [`App::tree_footer_view`] in production, or hand-assembled in ui tests without
/// a full [`App`]. Separate from [`TreeView`] (content-only) because the footer needs
/// several more fields than the content pane ever reads — folding them into `TreeView`
/// would make [`draw_tree_content`] receive data it never uses.
#[derive(Clone, Copy, Debug)]
pub struct TreeFooterView<'a> {
    /// Selected tree row (for the width-dependent left/right detail line).
    pub row: Option<&'a FlatRow>,
    pub status_toast: Option<(&'a str, bool)>,
    pub filter_active: bool,
    pub filter_input: &'a crate::text_input::TextInput,
    pub filter_pattern: &'a str,
    pub filter_diffs_only: bool,
    pub scan_in_progress: bool,
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    pub theme: Theme,
}

/// Pure geometry-decision inputs for [`tree_layout`], shared with [`App::sync_viewport`]
/// (via [`App::tree_layout_inputs`]) so the sizing decision and the frame render read the
/// same booleans without either side borrowing `&App`. Same shape as [`DiffLayoutInputs`].
#[derive(Clone, Copy, Debug)]
pub struct TreeLayoutInputs {
    pub has_detail: bool,
    pub has_status: bool,
    pub has_filter: bool,
    pub has_update: bool,
}

/// Regions of the directory-tree screen.
pub struct TreeLayout {
    pub top_bar: Rect,
    /// Left file pane, borders included.
    pub left: Rect,
    /// Narrow column of `=` / `≠` / `⬅` / `➡` symbols between the panes.
    pub indicator: Rect,
    /// Right file pane, borders included.
    pub right: Rect,
    pub footer: Rect,
}

/// Split `area` into the directory-tree screen's regions.
///
/// Shared by [`draw_tree`] (via [`App::tree_layout_inputs`]) and [`App::sync_viewport`],
/// so the rects the renderer draws into and the geometry scrolling is clamped against
/// cannot drift apart.
pub fn tree_layout(inputs: &TreeLayoutInputs, area: Rect) -> TreeLayout {
    let TreeLayoutInputs {
        has_detail,
        has_status,
        has_filter,
        has_update,
    } = *inputs;
    let footer_height = match (has_detail, has_status, has_filter) {
        (true, true, true) => 4,
        (true, true, false) => 3,
        (true, false, true) | (false, true, true) => 3,
        (true, false, false) | (false, true, false) | (false, false, true) => 2,
        (false, false, false) => 1,
    } + if has_update { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // Top Bar (1 line)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(area);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(30),   // Left
            Constraint::Length(4), // Indicator (no borders, symbols only)
            Constraint::Min(30),   // Right
        ])
        .split(chunks[1]);

    TreeLayout {
        top_bar: chunks[0],
        left: body_chunks[0],
        indicator: body_chunks[1],
        right: body_chunks[2],
        footer: chunks[2],
    }
}

/// Render the directory-tree screen.
///
/// Shell: layout + top bar (still need [`App`]). Content and footer paint through
/// [`draw_tree_content`]/[`draw_tree_footer`] with their own [`TreeView`]/[`TreeFooterView`]
/// so ui tests can exercise either region without a full app fixture.
pub fn draw_tree(f: &mut Frame, app: &App) {
    let inputs = app.tree_layout_inputs();
    let layout = tree_layout(&inputs, f.area());

    draw_top_bar(f, app, layout.top_bar);

    let view = app.tree_view();
    draw_tree_content(f, &view, &layout);

    let footer_view = app.tree_footer_view();
    draw_tree_footer(f, &footer_view, &layout);
}

/// Paint the directory-tree footer (status toast, detail line, filter bar,
/// keybindings/scan banner, update hint).
///
/// Same split as [`draw_tree_content`]: no `&App`, just `view` + `layout`. The
/// width-dependent detail-line padding needs `layout.footer.width`, so it computes
/// here rather than earlier — it can't be decided before the `Layout::split` that
/// produces the Rect.
pub fn draw_tree_footer(f: &mut Frame, view: &TreeFooterView<'_>, layout: &TreeLayout) {
    let theme = view.theme;

    let footer_txt = if view.scan_in_progress {
        Line::from("Scanning in progress... Please wait.")
    } else {
        Line::from(vec![
            Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
            Span::raw("or"),
            Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
            Span::raw("Command Palette  ·  right-click anywhere"),
        ])
    };

    // Build footer lines (top → bottom: status, detail, filter input, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error)) = view.status_toast {
        let status_style = if is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    if let Some((left_detail, right_detail)) = selected_row_detail(view.row) {
        let left_len = left_detail.chars().count();
        let right_len = right_detail.chars().count();
        let total_width = layout.footer.width as usize;
        let padding = total_width.saturating_sub(left_len + right_len);
        let space = " ".repeat(padding);
        footer_lines.push(Line::from(vec![
            Span::styled(left_detail, Style::default().fg(theme.accent)),
            Span::raw(space),
            Span::styled(right_detail, Style::default().fg(theme.accent)),
        ]));
    }

    // Filter input bar (shown when filter is active or a pattern is committed)
    if view.filter_active {
        let mut filter_spans = vec![Span::styled(
            " Filter: ",
            Style::default().fg(theme.warn).bold(),
        )];
        filter_spans.extend(text_input_spans(
            view.filter_input,
            Style::default().fg(theme.warn),
        ));
        if view.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    } else if !view.filter_pattern.is_empty() || view.filter_diffs_only {
        let mut filter_spans = vec![
            Span::styled(" Filter: ", Style::default().fg(theme.warn).bold()),
            Span::raw(view.filter_pattern),
            Span::styled(
                "  (/:edit, Esc/Backspace:clear)",
                Style::default().fg(theme.dim),
            ),
        ];
        if view.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    }

    footer_lines.push(footer_txt);

    if let Some(version) = view.update_available {
        let hint = crate::upgrade::update_hint(version, view.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
}

/// Paint the directory-tree content region (left / indicator / right panes).
///
/// Does not touch top bar or footer — those stay on the [`draw_tree`] shell.
pub fn draw_tree_content(f: &mut Frame, view: &TreeView<'_>, layout: &TreeLayout) {
    let theme = view.theme;

    let mut left_items = Vec::new();
    let mut indicator_items = Vec::new();
    let mut right_items = Vec::new();

    // Pad the indicator column with a blank top line so symbols align
    // vertically with items in the bordered left/right panes (which have
    // a top border row).
    indicator_items.push(ListItem::new(""));

    for (i, row) in view
        .rows
        .iter()
        .enumerate()
        .skip(view.scroll_offset)
        .take(view.visible_height)
    {
        let is_selected = i == view.selected_idx;
        let style = if is_selected {
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
        } else {
            match row.state {
                DiffState::Identical => Style::default().fg(theme.muted),
                // Both are yellow warnings, but an established difference is
                // bold so `≠` outweighs the merely-unverified `≈` (Issue #232).
                DiffState::Unverified(_) => Style::default().fg(theme.warn),
                DiffState::DifferentNewerLeft
                | DiffState::DifferentNewerRight
                | DiffState::DifferentSameTime => Style::default().fg(theme.warn).bold(),
                DiffState::LeftOnly => Style::default().fg(theme.success),
                DiffState::RightOnly => Style::default().fg(theme.info),
                DiffState::TypeConflict => Style::default().fg(theme.error).bold(),
            }
        };

        let indent = "  ".repeat(row.depth);

        // Left item
        if let Some(ref left_info) = row.left {
            let icon = if left_info.is_dir { "📁 " } else { "📄 " };
            left_items.push(ListItem::new(format!("{}{}{}", indent, icon, row.name)).style(style));
        } else {
            left_items.push(ListItem::new("").style(style));
        }

        // Indicator
        let symbol = match row.state {
            DiffState::Identical => " =",
            DiffState::Unverified(_) => " ≈",
            DiffState::DifferentNewerLeft
            | DiffState::DifferentNewerRight
            | DiffState::DifferentSameTime => " ≠",
            DiffState::LeftOnly => " ⬅",
            DiffState::RightOnly => " ➡",
            DiffState::TypeConflict => " 💥",
        };
        indicator_items.push(ListItem::new(symbol).style(style));

        // Right item
        if let Some(ref right_info) = row.right {
            let icon = if right_info.is_dir { "📁 " } else { "📄 " };
            right_items.push(ListItem::new(format!("{}{}{}", indent, icon, row.name)).style(style));
        } else {
            right_items.push(ListItem::new("").style(style));
        }
    }

    let left_title = Line::from(vec![
        Span::raw(" "),
        Span::styled("[1] ", Style::default().fg(theme.accent).bold()),
        Span::styled(
            get_display_path(view.left_root, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);
    let right_title = Line::from(vec![
        Span::raw(" "),
        Span::styled("[2] ", Style::default().fg(theme.accent).bold()),
        Span::styled(
            get_display_path(view.right_root, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);

    let left_border_style = if view.active_side_left {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.dim)
    };

    let right_border_style = if !view.active_side_left {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.dim)
    };

    let left_list = List::new(left_items).block(
        Block::default()
            .title(left_title)
            .border_style(left_border_style)
            .borders(Borders::ALL),
    );

    let indicator_list = List::new(indicator_items);

    let right_list = List::new(right_items).block(
        Block::default()
            .title(right_title)
            .border_style(right_border_style)
            .borders(Borders::ALL),
    );

    f.render_widget(left_list, layout.left);
    f.render_widget(indicator_list, layout.indicator);
    f.render_widget(right_list, layout.right);
}

/// Format a `SystemTime` as a relative time string (e.g. "3d ago", "1y ago").
fn format_relative_time(t: &SystemTime) -> String {
    let now = SystemTime::now();
    match now.duration_since(*t) {
        Ok(dur) => {
            let secs = dur.as_secs();
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else if secs < 2_592_000 {
                format!("{}d ago", secs / 86400)
            } else if secs < 31_536_000 {
                format!("{}mo ago", secs / 2_592_000)
            } else {
                format!("{}y ago", secs / 31_536_000)
            }
        }
        Err(_) => format_system_time(t),
    }
}

/// Wrap a single line of text into chunks that fit within `width` display columns.
/// Preserves empty input as a single empty chunk so alignment is maintained.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    wrap_text_with_mask(text, &[], width)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}

/// Extract the visible portion of `text` starting at `h_scroll` character columns.
fn scrolled_text(text: &str, h_scroll: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    text.chars().skip(h_scroll).take(width).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLineHighlight {
    None,
    /// Any row that belongs to a differing (mergeable) hunk.
    ChangeHunk,
    /// The hunk under the current scroll position (target for `[` / `]`).
    ActiveHunk,
    /// The physical row at `diff_scroll`.
    Cursor,
}

fn diff_line_highlight(
    in_change_hunk: bool,
    in_active_hunk: bool,
    is_cursor: bool,
) -> DiffLineHighlight {
    if is_cursor {
        DiffLineHighlight::Cursor
    } else if in_active_hunk {
        DiffLineHighlight::ActiveHunk
    } else if in_change_hunk {
        DiffLineHighlight::ChangeHunk
    } else {
        DiffLineHighlight::None
    }
}

fn apply_diff_line_highlight(style: Style, highlight: DiffLineHighlight, theme: Theme) -> Style {
    match highlight {
        DiffLineHighlight::None => style,
        DiffLineHighlight::ChangeHunk => style.bg(theme.hunk_bg),
        DiffLineHighlight::ActiveHunk => style.bg(theme.active_hunk_bg),
        DiffLineHighlight::Cursor => style.bg(theme.cursor_bg).bold(),
    }
}

#[derive(Clone)]
struct DiffDisplayCell {
    text: String,
    tag: Option<similar::ChangeTag>,
    intraline_mask: Option<Vec<bool>>,
    highlight: DiffLineHighlight,
}

fn diff_tag_base_style(tag: Option<similar::ChangeTag>, theme: Theme) -> Style {
    match tag {
        Some(similar::ChangeTag::Delete) => Style::default().fg(theme.error),
        Some(similar::ChangeTag::Insert) => Style::default().fg(theme.success),
        Some(similar::ChangeTag::Equal) => Style::default().fg(theme.muted),
        None => Style::default(),
    }
}

fn line_from_diff_cell(cell: &DiffDisplayCell, theme: Theme) -> Line<'static> {
    let base =
        apply_diff_line_highlight(diff_tag_base_style(cell.tag, theme), cell.highlight, theme);
    let Some(mask) = &cell.intraline_mask else {
        if cell.text.is_empty() && cell.highlight != DiffLineHighlight::None {
            return Line::from(Span::styled(" ", base));
        }
        return Line::from(Span::styled(cell.text.clone(), base));
    };

    let chars: Vec<char> = cell.text.chars().collect();
    if chars.is_empty() {
        if cell.highlight != DiffLineHighlight::None {
            return Line::from(Span::styled(" ", base));
        }
        return Line::from(Span::raw(""));
    }

    let mut aligned_mask = mask.clone();
    aligned_mask.truncate(chars.len());
    aligned_mask.resize(chars.len(), false);

    let mut spans = Vec::new();
    let mut run_start = 0usize;
    let mut run_highlight = aligned_mask[0];

    for i in 1..=chars.len() {
        if i == chars.len() || aligned_mask[i] != run_highlight {
            let run: String = chars[run_start..i].iter().collect();
            let style = if run_highlight {
                base.bold().underlined()
            } else {
                base.add_modifier(Modifier::DIM)
            };
            spans.push(Span::styled(run, style));
            if i < chars.len() {
                run_start = i;
                run_highlight = aligned_mask[i];
            }
        }
    }

    Line::from(spans)
}

fn wrap_text_with_mask(text: &str, mask: &[bool], width: usize) -> Vec<(String, Vec<bool>)> {
    if width == 0 {
        return vec![(text.to_string(), mask.to_vec())];
    }

    let chars: Vec<char> = text.chars().collect();
    let mut aligned_mask = mask.to_vec();
    aligned_mask.truncate(chars.len());
    aligned_mask.resize(chars.len(), false);

    let mut lines = Vec::new();
    let mut line_chars = Vec::new();
    let mut line_mask = Vec::new();
    let mut line_width = 0usize;

    for (ch, highlighted) in chars.iter().zip(aligned_mask.iter()) {
        let ch_width = if *ch == '\t' {
            4
        } else {
            ch.width().unwrap_or(0)
        };
        if line_width + ch_width > width && !line_chars.is_empty() {
            lines.push((line_chars.iter().collect(), std::mem::take(&mut line_mask)));
            line_chars.clear();
            line_width = 0;
        }
        line_chars.push(*ch);
        line_mask.push(*highlighted);
        line_width += ch_width;
    }

    if !line_chars.is_empty() || lines.is_empty() {
        lines.push((line_chars.into_iter().collect(), line_mask));
    }

    lines
}

fn scrolled_text_with_mask(
    text: &str,
    mask: &[bool],
    h_scroll: usize,
    width: usize,
) -> (String, Vec<bool>) {
    if width == 0 {
        return (String::new(), Vec::new());
    }
    let chars: Vec<char> = text.chars().skip(h_scroll).take(width).collect();
    let visible_mask: Vec<bool> = mask.iter().skip(h_scroll).take(width).copied().collect();
    (chars.into_iter().collect(), visible_mask)
}

fn push_diff_display_cells(
    cells: &mut Vec<DiffDisplayCell>,
    text: Option<&str>,
    tag: Option<similar::ChangeTag>,
    intraline_mask: Option<Vec<bool>>,
    wrap: bool,
    content_width: usize,
    h_scroll: usize,
) {
    let Some(text) = text else {
        cells.push(DiffDisplayCell {
            text: String::new(),
            tag: None,
            intraline_mask: None,
            highlight: DiffLineHighlight::None,
        });
        return;
    };

    if wrap {
        if let Some(mask) = intraline_mask.as_deref() {
            for (chunk, chunk_mask) in wrap_text_with_mask(text, mask, content_width) {
                cells.push(DiffDisplayCell {
                    text: chunk,
                    tag,
                    intraline_mask: Some(chunk_mask),
                    highlight: DiffLineHighlight::None,
                });
            }
        } else {
            for chunk in wrap_text(text, content_width) {
                cells.push(DiffDisplayCell {
                    text: chunk,
                    tag,
                    intraline_mask: None,
                    highlight: DiffLineHighlight::None,
                });
            }
        }
    } else {
        let (visible, visible_mask) = if let Some(mask) = intraline_mask.as_ref() {
            scrolled_text_with_mask(text, mask, h_scroll, content_width)
        } else {
            (scrolled_text(text, h_scroll, content_width), Vec::new())
        };
        cells.push(DiffDisplayCell {
            text: visible,
            tag,
            intraline_mask: if intraline_mask.is_some() {
                Some(visible_mask)
            } else {
                None
            },
            highlight: DiffLineHighlight::None,
        });
    }
}

/// Borrowed render state for the file-diff **content** region (info bar + panes).
///
/// Built by [`App::diff_view`] in production, or hand-assembled in ui tests without
/// standing up a full [`App`]. Top bar and footer stay on the `draw_diff` shell.
#[derive(Clone, Copy, Debug)]
pub struct DiffView<'a> {
    pub rows: &'a [crate::diff_view::DiffRow],
    pub wrap: bool,
    pub scroll: usize,
    pub h_scroll: usize,
    /// Content rows visible in each pane (from [`crate::app::Viewport`]).
    pub visible_height: usize,
    /// Content columns inside one pane (borders excluded).
    pub content_width: usize,
    pub left_root: &'a std::path::Path,
    pub right_root: &'a std::path::Path,
    /// Selected tree row that was opened into the diff (for titles / info bar).
    pub row: Option<&'a FlatRow>,
    pub left_hash: Option<&'a str>,
    pub right_hash: Option<&'a str>,
    pub left_line_ending: Option<&'a str>,
    pub right_line_ending: Option<&'a str>,
    pub theme: Theme,
    /// Active footer toast, if any: `(message, is_error)` (footer content).
    pub status_toast: Option<(&'a str, bool)>,
    /// Whether the two sides have any differing lines (keybinding-hint trimming, footer content).
    pub has_changes: bool,
    /// Latest update version when available (update hint, footer content).
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
    /// Whether each side has staged, unwritten edits. A dirty pane's title is
    /// marked with `*` and the footer offers `s save · u undo` (Issue #235).
    pub left_dirty: bool,
    pub right_dirty: bool,
    /// Whether a staged hunk operation can still be undone.
    pub can_undo: bool,
}

/// Pure geometry-decision inputs for [`diff_layout`], shared with [`App::sync_viewport`]
/// (via [`App::diff_layout_inputs`]) so the sizing decision and the frame render read the
/// same booleans without either side borrowing `&App`.
#[derive(Clone, Copy, Debug)]
pub struct DiffLayoutInputs {
    pub has_changes: bool,
    /// Selected row has content on either side (used with `!has_changes` to show the
    /// "files are identical" notice).
    pub row_has_content: bool,
    pub has_status: bool,
    pub has_update: bool,
}

/// Regions of the file-diff screen.
pub struct DiffLayout {
    pub top_bar: Rect,
    /// Row below the top bar carrying the "files are identical" notice; empty
    /// unless [`DiffLayout::show_identical`].
    pub notice: Rect,
    /// Left half of the info bar (size + SHA-256 + line ending).
    pub info_left: Rect,
    /// Right half of the info bar.
    pub info_right: Rect,
    /// Left diff pane, borders included.
    pub left: Rect,
    /// Right diff pane, borders included.
    pub right: Rect,
    pub footer: Rect,
    /// True when the two sides have no differing lines.
    pub show_identical: bool,
}

/// Split `area` into the file-diff screen's regions.
///
/// Shared by [`draw_diff`] (via [`App::diff_layout_inputs`]) and [`App::sync_viewport`],
/// so the rects the renderer draws into and the geometry scrolling is clamped against
/// cannot drift apart.
pub fn diff_layout(inputs: &DiffLayoutInputs, area: Rect) -> DiffLayout {
    let show_identical = !inputs.has_changes && inputs.row_has_content;

    let header_height = if show_identical { 2 } else { 1 };
    let footer_height =
        if inputs.has_status { 2 } else { 1 } + if inputs.has_update { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Header (Top Bar + optional Identical Msg)
            Constraint::Length(1),             // Info bar (size + SHA-256)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(area);

    let header_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(chunks[0]);

    let info_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    DiffLayout {
        top_bar: header_layout[0],
        notice: header_layout[1],
        info_left: info_chunks[0],
        info_right: info_chunks[1],
        left: body_chunks[0],
        right: body_chunks[1],
        footer: chunks[3],
        show_identical,
    }
}

/// Render the file-diff screen.
///
/// Shell: layout + top bar (still need [`App`]). Content and footer paint through
/// [`draw_diff_content`]/[`draw_diff_footer`] with one shared [`DiffView`] so ui tests
/// can exercise either region without a full app fixture.
pub fn draw_diff(f: &mut Frame, app: &App) {
    let inputs = app.diff_layout_inputs();
    let layout = diff_layout(&inputs, f.area());

    draw_top_bar(f, app, layout.top_bar);

    let view = app.diff_view();
    draw_diff_content(f, &view, &layout);
    draw_diff_footer(f, &view, &layout);
}

/// Paint the file-diff footer (status toast, keybindings, update hint).
///
/// Same split as [`draw_diff_content`]: no `&App`, just `view` + `layout`.
pub fn draw_diff_footer(f: &mut Frame, view: &DiffView<'_>, layout: &DiffLayout) {
    let theme = view.theme;

    // Build footer lines (top → bottom: status, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error)) = view.status_toast {
        let status_style = if is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    let mut footer_spans = vec![
        Span::styled(" N ", Style::default().fg(theme.accent).bold()),
        Span::raw("Next  ·  "),
        Span::styled(" P ", Style::default().fg(theme.accent).bold()),
        Span::raw("Prev  ·  "),
        Span::styled(" [ ", Style::default().fg(theme.accent).bold()),
        Span::raw("Hunk←  ·  "),
        Span::styled(" ] ", Style::default().fg(theme.accent).bold()),
        Span::raw("Hunk→  ·  "),
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ];
    // Staged, unwritten edits get their own hint line so the way out is obvious.
    if view.left_dirty || view.right_dirty {
        let mut staged = vec![
            Span::styled(" s ", Style::default().fg(theme.warn).bold()),
            Span::raw("save  ·  "),
        ];
        if view.can_undo {
            staged.push(Span::styled(" u ", Style::default().fg(theme.warn).bold()));
            staged.push(Span::raw("undo  ·  "));
        }
        staged.push(Span::styled(
            " Esc ",
            Style::default().fg(theme.warn).bold(),
        ));
        staged.push(Span::raw("back"));
        footer_lines.push(Line::from(staged));
    }
    if !view.has_changes {
        footer_spans.drain(0..10);
    }
    footer_lines.push(Line::from(footer_spans));

    if let Some(version) = view.update_available {
        let hint = crate::upgrade::update_hint(version, view.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
}

/// Paint the file-diff content region (identical notice, info bar, dual panes).
///
/// Does not touch top bar or footer — those stay on the [`draw_diff`] shell.
/// Geometry comes from `layout` (shell/`diff_layout`); line data from `view`.
pub fn draw_diff_content(f: &mut Frame, view: &DiffView<'_>, layout: &DiffLayout) {
    let theme = view.theme;
    let show_identical = layout.show_identical;

    if show_identical {
        let msg = Paragraph::new(Line::from(Span::styled(
            " ✓ Both files are identical — no differences found.",
            Style::default().fg(theme.success).bold(),
        )));
        f.render_widget(msg, layout.notice);
    }

    // Info bar: size + SHA-256 hash for each side, above the pane borders
    let left_info =
        build_diff_info_spans(view.row, true, view.left_hash, view.left_line_ending, theme);
    let right_info = build_diff_info_spans(
        view.row,
        false,
        view.right_hash,
        view.right_line_ending,
        theme,
    );
    f.render_widget(Paragraph::new(left_info), layout.info_left);
    f.render_widget(Paragraph::new(right_info), layout.info_right);

    let max_visible = view.visible_height;
    let content_width = view.content_width;

    let Some(row) = view.row else {
        return;
    };

    let mut left_physical: Vec<DiffDisplayCell> = Vec::new();
    let mut right_physical: Vec<DiffDisplayCell> = Vec::new();

    let hunk_row_ranges = crate::diff_view::diff_hunk_row_ranges(view.rows);
    let active_hunk_rows =
        crate::diff_view::hunk_index_at_scroll(view.rows, view.scroll, content_width, view.wrap)
            .and_then(|idx| hunk_row_ranges.get(idx).cloned());

    let mut physical_row = 0usize;
    for (logical_row, (left_line, right_line)) in view.rows.iter().enumerate() {
        let in_change_hunk = hunk_row_ranges
            .iter()
            .any(|range| range.contains(&logical_row));
        let in_active_hunk = active_hunk_rows
            .as_ref()
            .is_some_and(|range| range.contains(&logical_row));
        let left_text = left_line.as_ref().map(|l| l.text.trim_end());
        let right_text = right_line.as_ref().map(|r| r.text.trim_end());
        let left_tag = left_line.as_ref().map(|l| l.tag);
        let right_tag = right_line.as_ref().map(|r| r.tag);

        let replacement = crate::diff_view::is_replacement_pair(left_line, right_line);
        let left_mask = replacement
            .then(|| {
                left_text
                    .zip(right_text)
                    .map(|(left, right)| crate::diff_view::intraline_change_mask(left, right, true))
            })
            .flatten();
        let right_mask = replacement
            .then(|| {
                left_text.zip(right_text).map(|(left, right)| {
                    crate::diff_view::intraline_change_mask(right, left, false)
                })
            })
            .flatten();

        let mut left_chunk = Vec::new();
        let mut right_chunk = Vec::new();
        push_diff_display_cells(
            &mut left_chunk,
            left_text,
            left_tag,
            left_mask,
            view.wrap,
            content_width,
            view.h_scroll,
        );
        push_diff_display_cells(
            &mut right_chunk,
            right_text,
            right_tag,
            right_mask,
            view.wrap,
            content_width,
            view.h_scroll,
        );

        let max_lines = std::cmp::max(left_chunk.len(), right_chunk.len());
        for i in 0..max_lines {
            let highlight = diff_line_highlight(
                in_change_hunk,
                in_active_hunk,
                physical_row + i == view.scroll,
            );
            left_physical.push(
                left_chunk
                    .get(i)
                    .cloned()
                    .map(|mut cell| {
                        cell.highlight = highlight;
                        cell
                    })
                    .unwrap_or(DiffDisplayCell {
                        text: String::new(),
                        tag: left_tag,
                        intraline_mask: None,
                        highlight,
                    }),
            );
            right_physical.push(
                right_chunk
                    .get(i)
                    .cloned()
                    .map(|mut cell| {
                        cell.highlight = highlight;
                        cell
                    })
                    .unwrap_or(DiffDisplayCell {
                        text: String::new(),
                        tag: right_tag,
                        intraline_mask: None,
                        highlight,
                    }),
            );
        }
        physical_row += max_lines;
    }

    let left_lines: Vec<Line> = left_physical
        .into_iter()
        .skip(view.scroll)
        .take(max_visible)
        .map(|cell| line_from_diff_cell(&cell, theme))
        .collect();

    let right_lines: Vec<Line> = right_physical
        .into_iter()
        .skip(view.scroll)
        .take(max_visible)
        .map(|cell| line_from_diff_cell(&cell, theme))
        .collect();

    // Build pane titles: " /truncated/path/file.txt (3d ago) "
    let pane_width = layout.left.width as usize;
    let left_title = build_diff_pane_title(
        &view.left_root.join(&row.relative_path),
        row.left.as_ref().map(|f| &f.modified),
        pane_width,
    );
    let right_title = build_diff_pane_title(
        &view.right_root.join(&row.relative_path),
        row.right.as_ref().map(|f| &f.modified),
        pane_width,
    );

    // A `*` marks a pane whose working buffer has staged, unwritten edits.
    let left_title = if view.left_dirty {
        format!("*{left_title}")
    } else {
        left_title
    };
    let right_title = if view.right_dirty {
        format!("*{right_title}")
    } else {
        right_title
    };

    let left_p = Paragraph::new(left_lines).block(
        Block::default()
            .title(Span::styled(
                left_title,
                Style::default().bold().fg(if view.left_dirty {
                    theme.warn
                } else {
                    theme.fg
                }),
            ))
            .borders(Borders::ALL),
    );
    let right_p = Paragraph::new(right_lines).block(
        Block::default()
            .title(Span::styled(
                right_title,
                Style::default().bold().fg(if view.right_dirty {
                    theme.warn
                } else {
                    theme.fg
                }),
            ))
            .borders(Borders::ALL),
    );

    f.render_widget(left_p, layout.left);
    f.render_widget(right_p, layout.right);
    draw_close_button(f, layout.right);
}

/// Build info spans (size + line ending style + SHA-256 hash) for the diff view info bar.
fn build_diff_info_spans<'a>(
    row: Option<&'a FlatRow>,
    is_left: bool,
    hash: Option<&'a str>,
    line_ending: Option<&'a str>,
    theme: Theme,
) -> Line<'a> {
    let info = row.and_then(|r| {
        if is_left {
            r.left.as_ref()
        } else {
            r.right.as_ref()
        }
    });

    let mut spans = vec![Span::raw(" ")];

    if let Some(fi) = info {
        if !fi.is_dir {
            spans.push(Span::styled(
                format_size(fi.size),
                Style::default().fg(theme.dim),
            ));
            spans.push(Span::raw("  "));
        }
    }

    if let Some(le) = line_ending {
        spans.push(Span::styled(
            format!("[{}]", le),
            Style::default().fg(theme.dim),
        ));
        spans.push(Span::raw("  "));
    }

    if let Some(h) = hash {
        spans.push(Span::styled(
            format!("SHA256: {h}"),
            Style::default().fg(theme.dim),
        ));
    } else {
        spans.push(Span::styled("SHA256: —", Style::default().fg(theme.dim)));
    }

    Line::from(spans)
}
fn build_diff_pane_title(
    full_path: &std::path::Path,
    modified: Option<&SystemTime>,
    pane_width: usize,
) -> String {
    let rel_time = modified.map(format_relative_time).unwrap_or_default();
    // Reserve space for the leading space + " (rel_time) " + borders
    let suffix_len = rel_time.len() + 4; // " (rel_time) "
    let max_path = pane_width.saturating_sub(suffix_len + 2).max(10);
    let display_path = get_display_path(full_path, max_path);
    format!(" {} ({}) ", display_path, rel_time)
}

/// 0-indexed row of the clickable repo-URL line within the `About` topic body (see the
/// `HelpTopic::About` arm of `help_topic_body`) — kept in sync with `handle_mouse`'s click
/// detection in `input.rs`. Stable regardless of update-check state since the URL line always
/// comes before the optional update-hint line.
pub(crate) const ABOUT_REPO_LINE: u16 = 2;

/// Borrowed render state for the Help **body** region (topic list or scrolled body).
///
/// Built by [`App::help_view`]; top bar and footer stay on the [`draw_help`] shell.
#[derive(Clone, Copy, Debug)]
pub struct HelpView<'a> {
    pub topic: HelpTopic,
    pub index_open: bool,
    pub index_sel: usize,
    pub scroll: u16,
    pub theme: Theme,
    /// Latest update version when available (About topic footer line).
    pub update_available: Option<&'a str>,
    pub install_method: &'a crate::upgrade::InstallMethod,
}

fn help_topic_body(
    topic: HelpTopic,
    theme: Theme,
    update_available: Option<&str>,
    install_method: &crate::upgrade::InstallMethod,
) -> Text<'static> {
    match topic {
        HelpTopic::DirectoryTree => Text::from(
            "\
Navigation
  j / Down       move selection down
  k / Up         move selection up
  Ctrl+f         page selection down (about one screen)
  Ctrl+b         page selection up (about one screen)
  h / Left       collapse the selected directory
  l / Right      expand the selected directory
  Space          toggle expand/collapse
  Tab            switch focus between the Left and Right panes
  1 / 2          jump focus directly to the Left / Right pane

Row states
  =              no difference found by the active scan mode
  ≈              content unverified — the bytes were not compared
                 (Fast mode: sizes match but timestamps differ;
                  Precise mode: a side could not be read or hashed)
  ≠              a difference the scan established
  ⬅ / ➡          present on the right / left side only
  💥             one side is a file, the other a directory

Actions
  Enter          open the diff view (or toggle expand, for a directory)
  D              compare the selected file pair with the external diff tool
  E              edit the selected file in $EDITOR/$VISUAL
  L              copy the selected item from the right pane to the left (y/n confirm)
  R              copy the selected item from the left pane to the right (y/n confirm)
  C              open the Config menu
  c              switch Fast / Precise scan mode (persists, then re-scans)
  r              force a manual re-scan
  s              swap the left and right directories
  /              open the filter bar; every printable character is typed
                 into the query (Ctrl+f while typing: diffs-only toggle,
                 committed with the query on Enter)
  ?              show this help
  Esc            clear the applied filter, or quit when none is applied
  q              quit",
        ),
        HelpTopic::FileDiff => Text::from(
            "  Limits         UTF-8 text only, max 10 MiB per side
                 (binary / non-UTF-8 / oversized → toast; use D)
  j / Down       scroll down one line
  k / Up         scroll up one line
  Ctrl+f         page scroll down (about one screen)
  Ctrl+b         page scroll up (about one screen)
  N / Alt+Down   jump to next change block
  P / Alt+Up     jump to previous change block
  Left / Right   scroll horizontally (only while wrap is off)
  Highlighting   mergeable blocks are tinted; the active block and
                 current line are emphasized for `[` / `]` targets
  [              copy the change block under the cursor to the left
  ]              copy the change block under the cursor to the right
  L              copy the whole right file to the left side (y/n confirm)
  R              copy the whole left file to the right side (y/n confirm)
  w              toggle line wrapping
  f              toggle full-file context vs diff-only
  D              compare the same pair with the external diff tool
  E              edit the focused side's file in $EDITOR/$VISUAL
  C              open the Config menu (returns here on Esc/q)
  ?              show this help
  q / Esc        return to the Directory Tree view",
        ),
        HelpTopic::Config => Text::from(
            "  j / k, Down / Up   move the selection
  Enter / Space      select the highlighted external diff tool
                     or toggle Check for updates / Mouse support / Theme
                     / Scan mode (persists, then re-scans in the background)
  T                  toggle light/dark theme from anywhere (persists)
  h / l, Left / Right  adjust the Diff context line count
  ?                  show this help
  q / Esc            return to the screen you opened Config from

  Settings are saved to ~/.config/duodiff/config.toml (honors
  XDG_CONFIG_HOME). See config.example.toml in the repo for every
  field, its default, and what it does.",
        ),
        HelpTopic::Mouse => Text::from(
            "  Left Click     select the clicked row
  Right Click    select a row and open the Command Palette
  Double Click   open diff view for a file, or expand/collapse a directory
  Scroll         scroll the directory tree, diff lines, Config screen, Help
                 topic/index, or the menu/palette list; over the Config
                 screen's Diff context row, scroll adjusts its value

  Mouse is on by default; disable it in Config, in config.toml
  (mouse = false), or for one session with --no-mouse.",
        ),
        HelpTopic::General => Text::from(
            "  ; / Ctrl+p    open the Command Palette (right-click does too);
                 type to search every command for the current screen,
                 Up/Down to select, Enter to run, Esc or Ctrl+p to close
  ?              show this help
  q / Esc        quit (or back, on any sub-screen); in the Directory Tree
                 Esc clears an applied filter before it will quit
  T              toggle light/dark theme (persists across restart)
  Tab            (inside Help) open the topic index list
  1-6            (inside Help) jump straight to a topic",
        ),
        HelpTopic::About => {
            let repo = env!("CARGO_PKG_REPOSITORY")
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let mut lines = vec![
                Line::from(format!("duodiff v{}", env!("CARGO_PKG_VERSION"))),
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        repo.to_string(),
                        Style::default()
                            .fg(theme.fg)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                ]),
                Line::from(""),
            ];
            if let Some(version) = update_available {
                lines.push(Line::from(crate::upgrade::update_hint(
                    version,
                    install_method,
                )));
            }
            Text::from(lines)
        }
    }
}

/// Render the Help screen.
///
/// Shell: top bar + footer. Body paints through [`draw_help_content`].
pub fn draw_help(f: &mut Frame, app: &App) {
    let theme = app.theme();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    draw_top_bar(f, app, chunks[0]);

    let view = app.help_view();
    draw_help_content(f, &view, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

/// Paint the Help body (topic index list or scrolled topic text + close button).
pub fn draw_help_content(f: &mut Frame, view: &HelpView<'_>, body_area: Rect) {
    let theme = view.theme;
    if view.index_open {
        let items: Vec<ListItem> = HelpTopic::all()
            .iter()
            .enumerate()
            .map(|(i, t)| ListItem::new(format!("  {}  {}", i + 1, t.title())))
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title("Help — pick a topic (1-6 / j/k Enter · Esc back)")
                    .borders(Borders::ALL),
            )
            .highlight_style(
                Style::default()
                    .bg(theme.selection_bg)
                    .fg(theme.selection_fg),
            );
        let mut list_state = ListState::default();
        list_state.select(Some(view.index_sel));
        f.render_stateful_widget(list, body_area, &mut list_state);
    } else {
        let title = format!(
            "Help · {} — Tab topics · j/k scroll · Esc back",
            view.topic.title()
        );
        let paragraph = Paragraph::new(help_topic_body(
            view.topic,
            theme,
            view.update_available,
            view.install_method,
        ))
        .scroll((view.scroll, 0))
        .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(paragraph, body_area);
    }

    draw_close_button(f, body_area);
}

/// Render state for the Config **list** region.
///
/// Built by [`App::config_view`] after `ensure_config_selection` (shell-side).
/// `rows` is owned because [`App::config_rows`] already allocates a fresh list.
#[derive(Clone, Debug)]
pub struct ConfigView<'a> {
    pub rows: Vec<crate::app::ConfigRowKind>,
    pub selected_idx: usize,
    pub detected_diff_tools: &'a [(crate::diff_tool::ExternalDiffTool, bool)],
    pub external_diff_tool: Option<&'a str>,
    pub check_updates: bool,
    pub mouse: bool,
    pub theme_choice: crate::theme::ThemeChoice,
    pub diff_context: usize,
    /// Effective scan mode for this session.
    pub scan_mode: crate::settings::ScanMode,
    /// Persisted scan mode. Differs from `scan_mode` only while `--scan-mode`
    /// overrides it, which the row annotates as a session override (Issue #238).
    pub saved_scan_mode: crate::settings::ScanMode,
    pub theme: Theme,
}

/// Render the Config screen.
///
/// Shell: top bar, `ensure_config_selection`, footer. List paints through
/// [`draw_config_content`].
pub fn draw_config(f: &mut Frame, app: &mut App) {
    let theme = app.theme();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Top Bar
            Constraint::Min(5),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_top_bar(f, app, chunks[0]);
    // Mut side effect stays on the shell so content can be pure-read.
    app.ensure_config_selection();
    let view = app.config_view();
    draw_config_content(f, &view, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("or"),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Command Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

/// Paint the Config list + close button (no top bar / footer).
pub fn draw_config_content(f: &mut Frame, view: &ConfigView<'_>, body_area: Rect) {
    let theme = view.theme;
    let mut items = Vec::new();
    for (row_idx, row) in view.rows.iter().enumerate() {
        let style = if row_idx == view.selected_idx {
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
        } else {
            Style::default()
        };
        match row {
            crate::app::ConfigRowKind::Header(label) => {
                items.push(ListItem::new(Line::from(Span::styled(
                    *label,
                    Style::default().fg(theme.warn).bold(),
                ))));
            }
            crate::app::ConfigRowKind::DiffTool(tool_idx) => {
                let (tool, is_avail) = &view.detected_diff_tools[*tool_idx];
                let is_active = view.external_diff_tool == Some(tool.as_str());
                let marker = if is_active { "[x] " } else { "[ ] " };
                let avail_str = if *is_avail {
                    "(Available)"
                } else {
                    "(Not Found)"
                };
                items.push(
                    ListItem::new(format!("  {}{:<5} {}", marker, tool.as_str(), avail_str))
                        .style(style),
                );
            }
            crate::app::ConfigRowKind::CheckUpdates => {
                let marker = if view.check_updates { "[x] " } else { "[ ] " };
                items.push(
                    ListItem::new(format!("  {}Check for updates daily", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::Mouse => {
                let marker = if view.mouse { "[x] " } else { "[ ] " };
                items.push(ListItem::new(format!("  {}Enable mouse support", marker)).style(style));
            }
            crate::app::ConfigRowKind::Theme => {
                let marker = if view.theme_choice == crate::theme::ThemeChoice::Light {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(
                    ListItem::new(format!("  {}Light theme (off = dark)", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::DiffContext => {
                items.push(
                    ListItem::new(format!(
                        "      Diff context: {} lines (h/l to adjust)",
                        view.diff_context
                    ))
                    .style(style),
                );
            }
            crate::app::ConfigRowKind::ScanMode => {
                let mut label = format!(
                    "      Scan mode: {} (Enter to switch)",
                    view.scan_mode.label()
                );
                if view.scan_mode != view.saved_scan_mode {
                    label.push_str(&format!(
                        "  ·  session override; saved default: {}",
                        view.saved_scan_mode.label()
                    ));
                }
                items.push(ListItem::new(label).style(style));
            }
        }
    }

    let list = List::new(items).block(
        Block::default()
            .title("Configuration")
            .borders(Borders::ALL),
    );
    f.render_widget(list, body_area);
    draw_close_button(f, body_area);
}

/// The `[x]` close button's rectangle within `area`, or `None` if `area` is too
/// narrow to fit it. Shared by `draw_close_button` (render) and every close-button
/// hit test, so the two cannot drift apart.
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

pub fn draw_close_button(f: &mut Frame, area: Rect) {
    if let Some(button_area) = close_button_rect(area) {
        f.render_widget(Paragraph::new(Span::raw("[x]")), button_area);
    }
}

pub fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
    let popup_layout = Layout::default()
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
        .split(popup_layout[1])[1]
}

/// The Command Palette popup's geometry, clamped to the terminal.
///
/// One source of truth for both painting ([`draw_palette_content`]) and mouse
/// hit-testing ([`crate::input::handle_mouse`]), so a click can never land on a
/// row the renderer put somewhere else (Issue #239).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteLayout {
    /// The whole popup, borders included.
    pub popup: Rect,
    /// The query input line.
    pub query: Rect,
    /// The rule between the query and the list.
    pub separator: Rect,
    /// The action rows. Its height is how many items fit at once.
    pub list: Rect,
}

impl PaletteLayout {
    /// How many action rows are visible at once.
    pub fn visible_rows(&self) -> usize {
        self.list.height as usize
    }
}

/// Popup width bounds. The palette takes four fifths of the terminal within
/// these limits, so a disabled action's reason still fits beside its label.
const PALETTE_MIN_WIDTH: u16 = 40;
const PALETTE_MAX_WIDTH: u16 = 96;
/// Popup chrome that is never an action row: two borders, query, separator.
const PALETTE_CHROME_HEIGHT: u16 = 4;

/// Lay out the palette popup for `item_count` actions inside `area`, clamping
/// both dimensions so it always fits the terminal.
pub fn palette_layout(item_count: usize, area: Rect) -> PaletteLayout {
    let width = (area.width * 4 / 5)
        .clamp(PALETTE_MIN_WIDTH, PALETTE_MAX_WIDTH)
        .min(area.width);
    // Always keep room for one row — the "No matching commands" notice needs it.
    let wanted_rows = (item_count.max(1) as u16).saturating_add(PALETTE_CHROME_HEIGHT);
    let height = wanted_rows
        .min(area.height)
        .max(PALETTE_CHROME_HEIGHT.min(area.height));
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

/// The dispatch key a palette/menu entry carries. `App::build_palette_actions`
/// constructs it; `actions::execute_palette_action` matches on it exhaustively —
/// adding a variant without a matching dispatch arm is a compile error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteActionId {
    ExternalDiff,
    ToggleTheme,
    ToggleFocus,
    FocusLeft,
    FocusRight,
    ExpandSelected,
    CollapseSelected,
    ExternalEdit,
    CopyLeftToRight,
    CopyRightToLeft,
    BuiltinDiff,
    SwapPaths,
    ToggleScan,
    Refresh,
    Config,
    Help,
    Filter,
    Quit,
    ToggleWrap,
    ToggleFullDiff,
    NextChange,
    PrevChange,
    CopyHunkLeftToRight,
    CopyHunkRightToLeft,
    Back,
}

/// A single palette/menu entry — pure view-model data (what a row looks like and
/// which dispatch key it carries). `App::build_palette_actions` only constructs it;
/// `actions::execute_palette_action` only matches on its `action_id` field — nothing
/// pattern-matches the struct itself, so it lives here rather than on `App` (unlike
/// `ConfigRowKind`, which is genuine `App`-domain selection logic).
#[derive(Clone, Debug)]
pub struct PaletteAction {
    pub key: String,
    pub label: String,
    pub action_id: PaletteActionId,
    /// Why this action cannot run right now, or `None` when it can. Unavailable
    /// actions stay listed with their reason rather than disappearing, so the
    /// inventory a user sees does not change shape with the selection (Issue #239).
    pub disabled_reason: Option<&'static str>,
}

impl PaletteAction {
    /// Always available.
    pub fn new(key: &str, label: &str, action_id: PaletteActionId) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            action_id,
            disabled_reason: None,
        }
    }

    /// Available only when `available`; otherwise listed with `reason`.
    pub fn gated(
        key: &str,
        label: &str,
        action_id: PaletteActionId,
        available: bool,
        reason: &'static str,
    ) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            action_id,
            disabled_reason: (!available).then_some(reason),
        }
    }

    pub fn enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }
}

/// Borrowed render state for the Command Palette popup.
///
/// Built by [`App::palette_view`] after the shell has refreshed the item list and
/// synced the viewport.
#[derive(Clone, Copy, Debug)]
pub struct PaletteView<'a> {
    pub items: &'a [PaletteAction],
    pub selected_idx: usize,
    pub scroll_offset: usize,
    pub query: &'a str,
    pub theme: Theme,
}

/// Shown in place of the list when the query matches nothing. Non-selectable.
pub const PALETTE_NO_MATCH: &str = "No matching commands";

/// Truncate `text` to `max_width` terminal columns, appending `…` when it does
/// not fit. Measured in display width, so CJK and emoji do not overflow the popup.
fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let total: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= max_width {
        return text.to_string();
    }
    // Reserve one column for the ellipsis itself.
    let budget = max_width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Render the palette popup.
///
/// Shell: refresh the item list and sync the viewport against the laid-out list
/// height (both mut). Content paints through [`draw_palette_content`].
pub fn draw_palette(f: &mut Frame, app: &mut App) {
    app.refresh_palette_items();
    let layout = palette_layout(app.palette().items.len(), f.area());
    app.sync_palette_viewport(layout.visible_rows());
    let view = app.palette_view();
    draw_palette_content(f, &view, f.area());
}

/// Paint the palette popup inside `frame_area` (computes its own geometry).
pub fn draw_palette_content(f: &mut Frame, view: &PaletteView<'_>, frame_area: Rect) {
    let theme = view.theme;
    let layout = palette_layout(view.items.len(), frame_area);
    f.render_widget(Clear, layout.popup);

    let block = Block::default()
        .title(Span::styled(" Command Palette ", Style::default().bold()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn));
    f.render_widget(block, layout.popup);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Search: ", Style::default().fg(theme.accent)),
            Span::raw(view.query),
            Span::styled("█", Style::default().fg(theme.emphasis)),
        ])),
        layout.query,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(layout.separator.width as usize),
            Style::default().fg(theme.dim),
        ))),
        layout.separator,
    );

    if view.items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  {}", PALETTE_NO_MATCH),
                Style::default().fg(theme.dim),
            ))),
            layout.list,
        );
        draw_close_button(f, layout.popup);
        return;
    }

    // Two columns of chrome ("  ") plus the key column and its gap.
    let key_width = 10usize;
    let text_budget = (layout.list.width as usize).saturating_sub(key_width + 4);
    let mut list_items = Vec::new();
    for (i, action) in view
        .items
        .iter()
        .enumerate()
        .skip(view.scroll_offset)
        .take(layout.visible_rows())
    {
        let label = match action.disabled_reason {
            Some(reason) => format!("{} — {}", action.label, reason),
            None => action.label.clone(),
        };
        let display_text = format!(
            "  {:<key_width$}  {}",
            truncate_to_width(&action.key, key_width),
            truncate_to_width(&label, text_budget),
            key_width = key_width,
        );
        let mut style = if i == view.selected_idx {
            Style::default().bg(theme.info).fg(theme.selection_fg)
        } else {
            Style::default()
        };
        if !action.enabled() {
            style = style.fg(theme.dim);
        }
        list_items.push(ListItem::new(display_text).style(style));
    }
    f.render_widget(List::new(list_items), layout.list);
    draw_close_button(f, layout.popup);
}

/// Borrowed confirm-dialog state (message + theme).
#[derive(Clone, Copy, Debug)]
pub struct ConfirmView<'a> {
    pub title: &'a str,
    pub lines: &'a [String],
    pub choices: &'a [crate::app::ConfirmChoice],
    pub theme: Theme,
}

/// Render the confirm modal from an [`App`].
pub fn draw_confirm_modal(f: &mut Frame, app: &App) {
    let view = app.confirm_view();
    draw_confirm_content(f, &view, f.area());
}

/// Paint the confirm popup (no full `App` required).
///
/// Sizes itself to the body and clamps to the terminal, and wraps every body
/// line, so a long path or a three-button dialog stays readable at narrow
/// widths (Issue #235).
pub fn draw_confirm_content(f: &mut Frame, view: &ConfirmView<'_>, frame_area: Rect) {
    let theme = view.theme;
    let width = (frame_area.width * 4 / 5)
        .clamp(30, 78)
        .min(frame_area.width);
    // Two borders, a blank line, the button row, and one more blank line.
    let inner_width = width.saturating_sub(4).max(1) as usize;

    let mut body: Vec<Line> = Vec::new();
    for line in view.lines {
        if line.is_empty() {
            body.push(Line::from(""));
            continue;
        }
        for chunk in wrap_plain(line, inner_width) {
            body.push(Line::from(Span::raw(chunk)));
        }
    }
    body.push(Line::from(""));

    let mut button_spans = Vec::new();
    for choice in view.choices {
        button_spans.push(Span::styled(
            format!(" [{}] {} ", choice.key.to_ascii_uppercase(), choice.label),
            Style::default().fg(theme.accent).bold(),
        ));
    }
    if !button_spans.is_empty() {
        body.push(Line::from(button_spans).alignment(Alignment::Center));
    }

    let height = (body.len() as u16 + 3).min(frame_area.height);
    let area = centered_rect(width, height, frame_area);
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {} ", view.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn));

    f.render_widget(
        Paragraph::new(body)
            .block(block)
            .style(Style::default().fg(theme.fg)),
        area,
    );
}

/// Hard-wrap `text` to `width` display columns on character boundaries.
fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(c);
        used += w;
    }
    out.push(current);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    /// Render one frame the way the event loop does: sync the viewport for the
    /// current terminal size first, then draw. Drawing without the sync would
    /// render against stale (on the first frame, zero-sized) geometry.
    fn draw_frame(terminal: &mut Terminal<TestBackend>, app: &mut App) {
        app.sync_viewport(terminal.size().unwrap().into());
        terminal.draw(|f| draw(f, app)).unwrap();
    }

    /// `(visible_height, content_width)` for a [`DiffLayout`]'s left pane, the same way
    /// `App::sync_viewport` derives it — used by diff-content tests that build their own
    /// `DiffView`/`DiffLayout` via [`diff_layout`] instead of a full `App`.
    fn diff_content_geometry(layout: &DiffLayout) -> (usize, usize) {
        (
            layout.left.height.saturating_sub(2) as usize,
            layout.left.width.saturating_sub(2) as usize,
        )
    }

    /// Owned data backing a hand-built [`DiffView`] for content-only diff tests, so each
    /// test only spells out what it actually varies (rows, theme, hashes, ...) instead of
    /// repeating the same defaulted fields (`left_root`/`right_root`/`install_method`/etc.).
    struct DiffViewFixture {
        rows: Vec<crate::diff_view::DiffRow>,
        flat: FlatRow,
        left_root: PathBuf,
        right_root: PathBuf,
        method: crate::upgrade::InstallMethod,
        theme: Theme,
        left_hash: Option<String>,
        right_hash: Option<String>,
    }

    impl DiffViewFixture {
        fn new(rows: Vec<crate::diff_view::DiffRow>, flat: FlatRow) -> Self {
            Self {
                rows,
                flat,
                left_root: PathBuf::from("/left"),
                right_root: PathBuf::from("/right"),
                method: crate::upgrade::InstallMethod::Standalone,
                theme: Theme::DARK,
                left_hash: None,
                right_hash: None,
            }
        }

        /// Same rule `FileDiffState::has_changes` uses: at least one added/removed line.
        fn has_changes(&self) -> bool {
            self.rows.iter().any(crate::diff_view::diff_row_is_change)
        }

        fn view(
            &self,
            wrap: bool,
            scroll: usize,
            h_scroll: usize,
            visible_height: usize,
            content_width: usize,
        ) -> DiffView<'_> {
            DiffView {
                rows: &self.rows,
                wrap,
                scroll,
                h_scroll,
                visible_height,
                content_width,
                left_root: &self.left_root,
                right_root: &self.right_root,
                row: Some(&self.flat),
                left_hash: self.left_hash.as_deref(),
                right_hash: self.right_hash.as_deref(),
                left_line_ending: None,
                right_line_ending: None,
                theme: self.theme,
                status_toast: None,
                has_changes: self.has_changes(),
                update_available: None,
                install_method: &self.method,
                left_dirty: false,
                right_dirty: false,
                can_undo: false,
            }
        }
    }

    #[test]
    fn test_ui_drawing() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        println!("Buffer output:\n{:?}", buffer);

        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("/left"),
            "Left pane title should show [1] before the path"
        );
        assert!(
            buffer_string.contains("[2]") && buffer_string.contains("/right"),
            "Right pane title should show [2] before the path"
        );
        // The State column title was removed; verify indicator symbols render
        assert!(
            !buffer_string.contains("\"State\""),
            "State column title should be removed"
        );
    }

    #[test]
    fn test_draw_help_topic_body_shows_title_and_bindings() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut()
            .select_topic(crate::app::HelpTopic::DirectoryTree);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Help · Directory Tree — Tab topics · j/k scroll · Esc back"),
            "Help topic-body header should show the topic title and operation hints"
        );
    }

    /// Content seam: top bar from a hand-built [`TopBarView`] (no full `App`).
    #[test]
    fn test_draw_top_bar_content_without_full_app() {
        let backend = TestBackend::new(80, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = TopBarView {
            view_mode: ViewMode::DirectoryTree,
            precise_mode: true,
            diff_show_full: false,
            diff_wrap: false,
            theme: Theme::DARK,
        };
        let area = Rect::new(0, 0, 80, 1);

        terminal
            .draw(|f| draw_top_bar_content(f, &view, area))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Directory Tree") && buffer_string.contains("Precise"),
            "top bar content should show precise tree title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Config") || buffer_string.contains("Help"),
            "top bar content should show Config/Help hints: {buffer_string}"
        );
    }

    /// Content seam: confirm dialog from a hand-built [`ConfirmView`] (no full `App`).
    #[test]
    fn test_draw_confirm_content_without_full_app() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let choices = vec![
            crate::app::ConfirmChoice {
                key: 'y',
                label: "Yes".to_string(),
                action: crate::app::ConfirmAction::CopyLeftToRight,
            },
            crate::app::ConfirmChoice {
                key: 'n',
                label: "No (Cancel)".to_string(),
                action: crate::app::ConfirmAction::Cancel,
            },
        ];
        let lines = vec!["Copy foo.txt to right side?".to_string()];
        let view = ConfirmView {
            title: "Confirm Action",
            lines: &lines,
            choices: &choices,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_confirm_content(f, &view, f.area()))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Confirm Action"),
            "confirm content should show title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Copy foo.txt to right side?"),
            "confirm content should show message: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[Y]") && buffer_string.contains("[N]"),
            "confirm content should show y/n hints: {buffer_string}"
        );
    }

    /// Content seam: palette Menu from a hand-built [`PaletteView`] (no full `App`).
    #[test]
    fn test_draw_palette_content_without_full_app() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![
            PaletteAction {
                key: "q".to_string(),
                label: "Quit".to_string(),
                action_id: PaletteActionId::Quit,
                disabled_reason: None,
            },
            PaletteAction {
                key: "?".to_string(),
                label: "Help".to_string(),
                action_id: PaletteActionId::Help,
                disabled_reason: None,
            },
        ];
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Command Palette"),
            "palette content should show its title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("Quit") && buffer_string.contains("Help"),
            "palette content should list actions: {buffer_string}"
        );
    }

    /// Content seam: Help body from a hand-built [`HelpView`] only (no full `App`).
    #[test]
    fn test_draw_help_content_without_full_app() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let method = crate::upgrade::InstallMethod::Standalone;
        let view = HelpView {
            topic: HelpTopic::DirectoryTree,
            index_open: false,
            index_sel: 0,
            scroll: 0,
            theme: Theme::DARK,
            update_available: None,
            install_method: &method,
        };
        let body_area = Rect::new(0, 1, 120, 17);

        terminal
            .draw(|f| draw_help_content(f, &view, body_area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Help · Directory Tree"),
            "help content should show topic title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("j / Down"),
            "help content should list topic bindings: {buffer_string}"
        );
    }

    #[test]
    fn test_draw_help_topic_body_first_line_keeps_leading_indent() {
        // Regression test: `"\` line-continuation in a Rust string literal strips ALL
        // leading whitespace off the following line, not just the newline. Topics whose
        // first content line is an indented key entry (not a header like DirectoryTree's
        // "Navigation") must not lose that indentation relative to the rest of the block.
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut().select_topic(crate::app::HelpTopic::FileDiff);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("  j / Down       scroll down one line"),
            "FileDiff topic's first content line should keep its 2-space indent, matching every other line in the block"
        );
    }

    #[test]
    fn test_draw_help_index_shows_all_six_topic_titles() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::Help);
        app.help_mut().set_index_open(true);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        for title in crate::app::HelpTopic::all().iter().map(|t| t.title()) {
            assert!(
                buffer_string.contains(title),
                "Help index should list topic '{title}'"
            );
        }
    }

    #[test]
    fn test_draw_config_shows_flat_header_and_tools() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ]);
        app.set_view_mode(ViewMode::ConfigMenu);

        draw_frame(&mut terminal, &mut app);

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Configuration"),
            "Config screen title should be shown"
        );
        assert!(
            buffer_string.contains("External Diff Tool"),
            "Config header row should be shown inline"
        );
        assert!(
            buffer_string.contains("vim") && buffer_string.contains("code"),
            "Diff tool fields should render in the same list"
        );
        assert!(
            !buffer_string.contains("Configuration Categories"),
            "Old category menu should be removed"
        );
    }

    /// Content seam: Config list from a hand-built [`ConfigView`] (no full `App`).
    #[test]
    fn test_draw_config_content_without_full_app() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools = vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ];
        let view = ConfigView {
            rows: vec![
                crate::app::ConfigRowKind::Header("External Diff Tool"),
                crate::app::ConfigRowKind::DiffTool(0),
                crate::app::ConfigRowKind::DiffTool(1),
                crate::app::ConfigRowKind::Header("Updates"),
                crate::app::ConfigRowKind::CheckUpdates,
            ],
            selected_idx: 1,
            detected_diff_tools: &tools,
            external_diff_tool: Some("vim"),
            check_updates: true,
            mouse: true,
            theme_choice: crate::theme::ThemeChoice::Dark,
            diff_context: 3,
            scan_mode: crate::settings::ScanMode::Fast,
            saved_scan_mode: crate::settings::ScanMode::Fast,
            theme: Theme::DARK,
        };
        let body_area = Rect::new(0, 1, 120, 16);

        terminal
            .draw(|f| draw_config_content(f, &view, body_area))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Configuration"),
            "config content should show title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("External Diff Tool"),
            "config content should show header: {buffer_string}"
        );
        assert!(
            buffer_string.contains("vim") && buffer_string.contains("code"),
            "config content should list tools: {buffer_string}"
        );
    }

    /// Content seam: dual panes + indicator from a hand-built [`TreeView`] only
    /// (no `App`, no top bar / footer). Part of #128 fixture-cost goal.
    #[test]
    fn test_draw_tree_content_without_full_app() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: DiffState::Identical,
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
            },
            FlatRow {
                depth: 1,
                relative_path: PathBuf::from("only-left.txt"),
                name: "only-left.txt".to_string(),
                state: DiffState::LeftOnly,
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
            },
        ];
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 1,
            visible_height: 15,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 55, 16),
            indicator: Rect::new(55, 1, 4, 16),
            right: Rect::new(59, 1, 61, 16),
            footer: Rect::new(0, 17, 120, 3),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("/left"),
            "tree content should show left pane path title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("[2]") && buffer_string.contains("/right"),
            "tree content should show right pane path title: {buffer_string}"
        );
        assert!(
            buffer_string.contains("only-left.txt"),
            "tree content should list the LeftOnly row: {buffer_string}"
        );
    }

    #[test]
    fn test_draw_tree_footer_mentions_help_key() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: false,
            has_status: false,
            has_filter: false,
            has_update: false,
        };
        let area = Rect::new(0, 0, 120, 20);
        let layout = tree_layout(&inputs, area);
        let top_bar_view = TopBarView {
            view_mode: ViewMode::DirectoryTree,
            precise_mode: false,
            diff_show_full: false,
            diff_wrap: false,
            theme: Theme::DARK,
        };
        let tree_view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: layout.left.height.saturating_sub(2) as usize,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
        };
        let footer_view = TreeFooterView {
            row: None,
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| {
                draw_top_bar_content(f, &top_bar_view, layout.top_bar);
                draw_tree_content(f, &tree_view, &layout);
                draw_tree_footer(f, &footer_view, &layout);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("(?)Help"),
            "Top bar should hint at the ? Help key"
        );
        assert!(
            buffer_string.contains("[1]") && buffer_string.contains("[2]"),
            "Pane titles should show [1]/[2] focus shortcuts"
        );
        assert!(
            !buffer_string.contains("Left  ·") && !buffer_string.contains("Right  ·"),
            "Footer should not duplicate 1/2 pane focus hints"
        );
    }

    #[test]
    fn test_selected_row_detail_newer_left() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            left_detail.contains("(newer)"),
            "Left side should contain '(newer)': {}",
            left_detail
        );
        assert!(
            !right_detail.contains("(newer)"),
            "Right side should not contain '(newer)': {}",
            right_detail
        );
        assert!(
            left_detail.contains("2.0 KB"),
            "Should show left size: {}",
            left_detail
        );
        assert!(
            right_detail.contains("1.0 KB"),
            "Should show right size: {}",
            right_detail
        );
    }

    #[test]
    fn test_selected_row_detail_identical_returns_none() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };

        assert!(selected_row_detail(Some(&row)).is_none());
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_selected_row_detail_newer_right() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerRight,
            left: Some(FileInfo {
                is_dir: false,
                size: 512,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            right_detail.contains("(newer)"),
            "Right side should contain '(newer)': {}",
            right_detail
        );
        assert!(
            !left_detail.contains("(newer)"),
            "Left side should not contain '(newer)': {}",
            left_detail
        );
        assert!(
            left_detail.contains("512 B"),
            "Should show left size: {}",
            left_detail
        );
        assert!(
            right_detail.contains("2.0 KB"),
            "Should show right size: {}",
            right_detail
        );
    }

    #[test]
    fn test_selected_row_detail_same_time() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentSameTime,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: mtime,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: mtime,
            }),
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            !left_detail.contains("(newer)"),
            "Left side should not mark as newer: {}",
            left_detail
        );
        assert!(
            !right_detail.contains("(newer)"),
            "Right side should not mark as newer: {}",
            right_detail
        );
        assert!(left_detail.contains("2.0 KB"), "Should contain left size");
        assert!(right_detail.contains("1.0 KB"), "Should contain right size");
    }

    #[test]
    fn test_selected_row_detail_directory() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("subdir"),
            name: "subdir".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
        };

        let (left_detail, right_detail) = selected_row_detail(Some(&row)).unwrap();
        assert!(
            !left_detail.contains("KB") && !left_detail.contains("MB"),
            "Left detail should not show size: {}",
            left_detail
        );
        assert!(
            !right_detail.contains("KB") && !right_detail.contains("MB"),
            "Right detail should not show size: {}",
            right_detail
        );
        assert!(left_detail.contains("(newer)"), "Should mark left as newer");
        assert!(
            !right_detail.contains("(newer)"),
            "Should not mark right as newer"
        );
    }

    #[test]
    fn test_selected_row_detail_none_for_single_sided() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;

        // LeftOnly should return None
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("only_left.txt"),
            name: "only_left.txt".to_string(),
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        };
        assert!(selected_row_detail(Some(&row)).is_none());

        // RightOnly should return None
        let row = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("only_right.txt"),
            name: "only_right.txt".to_string(),
            state: DiffState::RightOnly,
            left: None,
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        assert!(selected_row_detail(Some(&row)).is_none());
    }

    #[test]
    fn test_selected_row_detail_none_for_missing_row() {
        assert!(selected_row_detail(None).is_none());
    }

    #[test]
    fn test_state_column_does_not_show_side_indicators() {
        // After the readability improvement, the State column should NOT contain
        // (L) or (R) side markers — that info moved to the footer detail line.
        // Content-only concern (the State/indicator column), no footer involved.
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows: Vec<FlatRow> = Vec::new();
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 17,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 58, 18),
            indicator: Rect::new(58, 1, 4, 18),
            right: Rect::new(62, 1, 58, 18),
            footer: Rect::new(0, 19, 120, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        // The old "(L)" / "(R)" indicators should no longer appear in the State column
        assert!(
            !buffer_string.contains("(L)"),
            "State column should not contain '(L)' anymore"
        );
        assert!(
            !buffer_string.contains("(R)"),
            "State column should not contain '(R)' anymore"
        );
    }

    #[test]
    fn test_footer_detail_line_shown_for_different_file() {
        use crate::diff::FileInfo;
        use std::time::{Duration, SystemTime};

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        // A row with a difference so the detail line appears in the footer.
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000),
            }),
        };
        let method = crate::upgrade::InstallMethod::Standalone;
        let filter_input = crate::text_input::TextInput::default();

        let inputs = TreeLayoutInputs {
            has_detail: true,
            has_status: false,
            has_filter: false,
            has_update: false,
        };
        let layout = tree_layout(&inputs, Rect::new(0, 0, 120, 20));
        let view = TreeFooterView {
            row: Some(&flat),
            status_toast: None,
            filter_active: false,
            filter_input: &filter_input,
            filter_pattern: "",
            filter_diffs_only: false,
            scan_in_progress: false,
            update_available: None,
            install_method: &method,
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_tree_footer(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("(newer)"),
            "Footer should show '(newer)' tag for the detail line: {}",
            buffer_string
        );
    }

    #[test]
    fn test_diff_view_shows_file_paths_and_identical_notice() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // Inject an identical file pair
        app.push_flat_row(FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.set_view_mode(ViewMode::FileDiff);

        // diff rows with only Equal tags → files are identical
        app.diff_mut().set_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
        ))]);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);

        // Should show full paths for both sides in pane titles (OS-agnostic separators).
        let left_path = app.left_path().join("same.txt");
        let right_path = app.right_path().join("same.txt");
        assert!(
            buffer_string.contains(left_path.to_string_lossy().as_ref()),
            "Diff view should show left full path in title: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains(right_path.to_string_lossy().as_ref()),
            "Diff view should show right full path in title: {}",
            buffer_string
        );
        // Should show the identical notice
        assert!(
            buffer_string.contains("identical"),
            "Diff view should show identical notice: {}",
            buffer_string
        );
        // Should show relative time in title
        assert!(
            buffer_string.contains("ago"),
            "Diff view title should show relative time: {}",
            buffer_string
        );
    }

    /// Content seam: paint info bar + panes from a hand-built [`DiffView`] only
    /// (no `App`, no top bar / footer). Guards the #128 fixture-cost goal.
    #[test]
    fn test_draw_diff_content_without_full_app() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let method = crate::upgrade::InstallMethod::Standalone;
        let view = DiffView {
            rows: &rows,
            wrap: false,
            scroll: 0,
            h_scroll: 0,
            visible_height: 20,
            content_width: 50,
            left_root: &left_root,
            right_root: &right_root,
            row: Some(&flat),
            left_hash: Some("aabbccdd11223344"),
            right_hash: Some("aabbccdd11223344"),
            left_line_ending: Some("LF"),
            right_line_ending: Some("LF"),
            theme: Theme::DARK,
            status_toast: None,
            has_changes: false,
            update_available: None,
            install_method: &method,
            left_dirty: false,
            right_dirty: false,
            can_undo: false,
        };
        // Fixed geometry for a 120×28 content shell (notice + info + panes).
        let layout = DiffLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            notice: Rect::new(0, 1, 120, 1),
            info_left: Rect::new(0, 2, 60, 1),
            info_right: Rect::new(60, 2, 60, 1),
            left: Rect::new(0, 3, 60, 22),
            right: Rect::new(60, 3, 60, 22),
            footer: Rect::new(0, 25, 120, 3),
            show_identical: true,
        };

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("identical"),
            "content-only draw should show identical notice: {buffer_string}"
        );
        assert!(
            buffer_string.contains("same.txt") || buffer_string.contains("/left"),
            "content-only draw should show pane path titles: {buffer_string}"
        );
        assert!(
            buffer_string.contains("aabbccdd11223344"),
            "content-only draw should show SHA256 on the info bar: {buffer_string}"
        );
    }

    #[test]
    fn test_diff_view_intraline_highlight_splits_replacement_line() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "let foo = 1;".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "let bar = 1;".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.rs"),
            name: "file.rs".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("foo") && buffer_string.contains("bar"),
            "Replacement line content should render: {buffer_string}"
        );
        assert!(
            buffer_string.contains("underline")
                || buffer_string.contains("Underlined")
                || buffer_string.contains("UNDERLINED"),
            "Changed spans should use underline styling: {buffer_string}"
        );
    }

    #[test]
    fn test_diff_view_no_identical_notice_when_files_differ() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // diff rows with a Delete tag → files differ
        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old line".to_string(),
            }),
            None,
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            !buffer_string.contains("identical"),
            "Diff view should NOT show identical notice when files differ: {}",
            buffer_string
        );
    }

    #[test]
    fn test_diff_view_shows_size_and_sha256_above_border() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old".to_string(),
            }),
            None,
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 2048,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let mut fixture = DiffViewFixture::new(rows, flat);
        fixture.left_hash = Some("aabbccdd11223344".to_string());
        fixture.right_hash = Some("eeff001122334455".to_string());

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        // Size info should appear above the pane borders in the info bar
        assert!(
            buffer_string.contains("2.0 KB"),
            "Diff view should show left size in info bar: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("1.0 KB"),
            "Diff view should show right size in info bar: {}",
            buffer_string
        );
        // SHA-256 hashes should be displayed
        assert!(
            buffer_string.contains("SHA256: aabbccdd11223344"),
            "Diff view should show left SHA-256 hash: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("SHA256: eeff001122334455"),
            "Diff view should show right SHA-256 hash: {}",
            buffer_string
        );
    }

    #[test]
    fn test_format_system_time_is_utc() {
        use std::time::{Duration, SystemTime};
        assert_eq!(
            format_system_time(&SystemTime::UNIX_EPOCH),
            "1970-01-01 00:00:00 UTC"
        );
        // 1970-01-01 01:02:03 UTC
        assert_eq!(
            format_system_time(&(SystemTime::UNIX_EPOCH + Duration::from_secs(3723))),
            "1970-01-01 01:02:03 UTC"
        );
        // 1970-01-02 00:00:00 UTC
        assert_eq!(
            format_system_time(&(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400))),
            "1970-01-02 00:00:00 UTC"
        );
    }

    #[test]
    fn test_format_relative_time() {
        use std::time::{Duration, SystemTime};

        let now = SystemTime::now();
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(30))),
            "just now"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(300))),
            "5m ago"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(7200))),
            "2h ago"
        );
        assert_eq!(
            format_relative_time(&(now - Duration::from_secs(259_200))),
            "3d ago"
        );
    }

    #[test]
    fn test_build_diff_pane_title_truncates_long_path() {
        use std::time::SystemTime;
        let long_path =
            std::path::PathBuf::from("/very/long/path/that/exceeds/the/pane/width/file.txt");
        let title = build_diff_pane_title(&long_path, Some(&SystemTime::UNIX_EPOCH), 40);
        assert!(
            !title.contains("Left:") && !title.contains("Right:"),
            "Title should not contain a Left:/Right: prefix: {}",
            title
        );
        assert!(title.contains("ago"), "Title should contain relative time");
        // Long path should be truncated with "..."
        assert!(
            title.contains("..."),
            "Long path should be truncated: {}",
            title
        );
    }

    #[test]
    fn test_build_diff_pane_title_short_path() {
        use std::time::SystemTime;
        let short_path = std::path::PathBuf::from("/left/file.txt");
        let title = build_diff_pane_title(&short_path, Some(&SystemTime::UNIX_EPOCH), 80);
        assert!(
            title.contains("/left/file.txt"),
            "Short path should not be truncated: {}",
            title
        );
        assert!(
            !title.contains("Left:") && !title.contains("Right:"),
            "Title should not contain a Left:/Right: prefix: {}",
            title
        );
        assert!(
            title.contains("ago"),
            "Title should contain relative time: {}",
            title
        );
    }

    #[test]
    fn test_wrap_text_splits_long_lines() {
        let text = "abcdefghijklmnopqrstuvwxyz";
        let wrapped = wrap_text(text, 10);
        assert_eq!(wrapped, vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }

    #[test]
    fn test_wrap_text_preserves_short_lines() {
        let text = "hello";
        let wrapped = wrap_text(text, 10);
        assert_eq!(wrapped, vec!["hello"]);
    }

    #[test]
    fn test_wrap_text_empty_input() {
        let wrapped = wrap_text("", 10);
        assert_eq!(wrapped, vec![""]);
    }

    #[test]
    fn test_scrolled_text_basic() {
        assert_eq!(scrolled_text("hello world", 0, 5), "hello");
        assert_eq!(scrolled_text("hello world", 6, 5), "world");
        assert_eq!(scrolled_text("hello world", 20, 5), "");
    }

    #[test]
    fn test_diff_view_wrap_mode_increases_physical_rows() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        // Assertion is on `app.viewport()`, computed entirely by `App::sync_viewport`
        // (via `resync_diff_geometry`) — no rendering needed, so no `Terminal`/`draw`.
        let area = Rect::new(0, 0, 40, 30);
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.push_flat_row(FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.set_view_mode(ViewMode::FileDiff);

        // One logical row with a long line (52 chars). At 40-column terminal,
        // content width is ~18, so wrapping should produce multiple physical rows.
        app.diff_mut().set_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
        ))]);

        app.diff_mut().set_wrap(false);
        app.sync_viewport(area);
        let no_wrap_rows = app.viewport().diff_physical_rows;

        app.diff_mut().set_wrap(true);
        app.sync_viewport(area);
        let wrap_rows = app.viewport().diff_physical_rows;

        assert_eq!(
            no_wrap_rows, 1,
            "Without wrapping one logical row is one physical row"
        );
        assert!(
            wrap_rows > no_wrap_rows,
            "Wrapping should produce more physical rows: {} > {}",
            wrap_rows,
            no_wrap_rows
        );
    }

    #[test]
    fn test_diff_view_horizontal_scroll_offset() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        // Longer than the 38-column pane, so an offset of 5 is a legal scroll
        // position rather than one `sync_viewport` would clamp away.
        let rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
        ))];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let fixture = DiffViewFixture::new(rows, flat);

        // No changes (all Equal rows) → identical notice shown, same as `App` would compute.
        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 80, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        let view = fixture.view(false, 0, 5, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("56789abcdefghijklmno"),
            "Horizontally scrolled content should start after the offset: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("01234"),
            "Content before the horizontal scroll offset should not be visible: {}",
            buffer_string
        );
    }

    #[test]
    fn test_diff_line_highlight_priority() {
        assert_eq!(
            diff_line_highlight(true, true, true),
            DiffLineHighlight::Cursor
        );
        assert_eq!(
            diff_line_highlight(true, true, false),
            DiffLineHighlight::ActiveHunk
        );
        assert_eq!(
            diff_line_highlight(true, false, false),
            DiffLineHighlight::ChangeHunk
        );
        assert_eq!(
            diff_line_highlight(false, false, false),
            DiffLineHighlight::None
        );
    }

    #[test]
    fn test_diff_view_highlights_mergeable_blocks_and_cursor() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old-line".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new-line".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
            )),
        ];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let fixture = DiffViewFixture::new(rows, flat);

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        // Cursor on context line (scroll: 0); the nearest change hunk row should
        // still be emphasized.
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Rgb(48, 48, 88)"),
            "Active mergeable hunk should use emphasized background: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("Rgb(64, 64, 64)"),
            "Cursor line should use distinct background: {}",
            buffer_string
        );
    }

    #[test]
    fn test_light_theme_changes_diff_hunk_background() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "context".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old-line".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new-line".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "tail".to_string(),
                }),
            )),
        ];
        let flat = FlatRow {
            depth: 0,
            relative_path: PathBuf::from("diff.txt"),
            name: "diff.txt".to_string(),
            state: DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
        };
        let mut fixture = DiffViewFixture::new(rows, flat);
        fixture.theme = Theme::LIGHT;

        let inputs = DiffLayoutInputs {
            has_changes: fixture.has_changes(),
            row_has_content: true,
            has_status: false,
            has_update: false,
        };
        let layout = diff_layout(&inputs, Rect::new(0, 0, 120, 30));
        let (visible_height, content_width) = diff_content_geometry(&layout);
        // Cursor on context line (scroll: 0), same as the dark-theme equivalent test,
        // so the nearest change hunk (not the cursor row) is the one under assertion.
        let view = fixture.view(false, 0, 0, visible_height, content_width);

        terminal
            .draw(|f| draw_diff_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("Rgb(205, 205, 240)"),
            "Light theme should use its own active-hunk background, not the dark default: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("Rgb(48, 48, 88)"),
            "Light theme must not fall back to the dark-theme active-hunk background: {}",
            buffer_string
        );
    }

    #[test]
    fn test_light_theme_changes_top_bar_title_colour() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Light);

        draw_frame(&mut terminal, &mut app);

        let light_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            light_buffer_string.contains("Black"),
            "Light theme top-bar title should use a dark (Black) foreground: {}",
            light_buffer_string
        );

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // Set explicitly rather than relying on the loaded default: other tests in this
        // binary persist `settings.theme` to the real config file, and `App::new` reloads
        // from disk, so a bare default here would be flaky under parallel test execution.
        app.set_theme(crate::theme::ThemeChoice::Dark);
        draw_frame(&mut terminal, &mut app);
        let dark_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            !dark_buffer_string.contains("Black"),
            "Dark theme top-bar title should not use Black: {}",
            dark_buffer_string
        );
    }

    #[test]
    fn test_light_theme_paints_full_canvas_background() {
        // Regression guard: `draw()` must paint the whole frame with the theme's canvas
        // background before drawing any view, otherwise cells left unpainted by inner
        // widgets (e.g. gaps between panes) keep the terminal's native colour instead of
        // showing the theme's chosen background (Issue: Light theme background stayed
        // terminal-native because nothing called `Theme::base_style()`).
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Light);
        draw_frame(&mut terminal, &mut app);
        let light_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            light_buffer_string.contains("bg: White"),
            "Light theme should paint the canvas background White: {}",
            light_buffer_string
        );

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_theme(crate::theme::ThemeChoice::Dark);
        draw_frame(&mut terminal, &mut app);
        let dark_buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            !dark_buffer_string.contains("bg: White"),
            "Dark theme should not paint the canvas background White: {}",
            dark_buffer_string
        );
    }

    #[test]
    fn test_diff_view_header_shows_wrap_state() {
        // "Wrap" is painted by the shared top bar (`TopBarView`/`draw_top_bar_content`),
        // not the diff content/footer — no `App` or `DiffView` needed for this one.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let view = TopBarView {
            view_mode: ViewMode::FileDiff,
            precise_mode: false,
            diff_show_full: false,
            diff_wrap: true,
            theme: Theme::DARK,
        };
        let area = Rect::new(0, 0, 80, 1);

        terminal
            .draw(|f| draw_top_bar_content(f, &view, area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Wrap"),
            "Header should show Wrap state: {}",
            buffer_string
        );
    }

    #[test]
    fn test_draw_close_button() {
        let backend = TestBackend::new(20, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                draw_close_button(f, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("[x]"),
            "Buffer should contain close button [x]"
        );
    }

    /// Issue #232: `≈` needs its own indicator glyph, distinct from both `=` and `≠`.
    #[test]
    fn test_tree_indicator_shows_approx_symbol_for_unverified_rows() {
        use crate::diff::{FileInfo, UnverifiedReason};
        use std::time::{Duration, SystemTime};

        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        let rows = vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state: DiffState::Unverified(UnverifiedReason::NotCompared),
            left: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(600),
            }),
        }];
        let left_root = PathBuf::from("/left");
        let right_root = PathBuf::from("/right");
        let view = TreeView {
            rows: &rows,
            scroll_offset: 0,
            selected_idx: 0,
            visible_height: 17,
            left_root: &left_root,
            right_root: &right_root,
            active_side_left: true,
            theme: Theme::DARK,
        };
        let layout = TreeLayout {
            top_bar: Rect::new(0, 0, 120, 1),
            left: Rect::new(0, 1, 58, 18),
            indicator: Rect::new(58, 1, 4, 18),
            right: Rect::new(62, 1, 58, 18),
            footer: Rect::new(0, 19, 120, 1),
        };

        terminal
            .draw(|f| draw_tree_content(f, &view, &layout))
            .unwrap();

        let buffer_string = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer_string.contains("≈"),
            "unverified rows must render the `≈` indicator: {}",
            buffer_string
        );
        assert!(
            !buffer_string.contains("≠"),
            "an unverified row must not claim a difference: {}",
            buffer_string
        );
    }

    /// Issue #232: the reason a pair is `≈` belongs in the selected-row details.
    #[test]
    fn test_selected_row_detail_exposes_the_unverified_reason() {
        use crate::diff::{FileInfo, UnverifiedReason};
        use std::time::{Duration, SystemTime};

        let row = |state| FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state,
            left: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1024,
                modified: SystemTime::UNIX_EPOCH + Duration::from_secs(600),
            }),
        };

        let fast = row(DiffState::Unverified(UnverifiedReason::NotCompared));
        let (left, right) = selected_row_detail(Some(&fast)).unwrap();
        assert!(right.contains("content unverified (fast scan)"), "{right}");
        // The newer side is still derived from the timestamps the row holds.
        assert!(!left.contains("(newer)"), "{left}");
        assert!(right.contains("(newer)"), "{right}");

        let failed = row(DiffState::Unverified(UnverifiedReason::HashFailed));
        let (_, right) = selected_row_detail(Some(&failed)).unwrap();
        assert!(
            right.contains("content unverified (read failed)"),
            "{right}"
        );

        // Established differences keep their existing detail line, reason-free.
        let different = row(DiffState::DifferentNewerRight);
        let (_, right) = selected_row_detail(Some(&different)).unwrap();
        assert!(!right.contains("content unverified"), "{right}");
    }

    /// Issue #238: while `--scan-mode` overrides the saved default, the Config row
    /// says so; once they agree again the annotation is gone.
    #[test]
    fn test_draw_config_content_annotates_a_scan_mode_session_override() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)> = Vec::new();
        let body_area = Rect::new(0, 1, 120, 16);

        let render = |terminal: &mut Terminal<TestBackend>,
                      scan_mode: crate::settings::ScanMode,
                      saved_scan_mode: crate::settings::ScanMode| {
            let view = ConfigView {
                rows: vec![
                    crate::app::ConfigRowKind::Header("Scan"),
                    crate::app::ConfigRowKind::ScanMode,
                ],
                selected_idx: 1,
                detected_diff_tools: &tools,
                external_diff_tool: None,
                check_updates: true,
                mouse: true,
                theme_choice: crate::theme::ThemeChoice::Dark,
                diff_context: 3,
                scan_mode,
                saved_scan_mode,
                theme: Theme::DARK,
            };
            terminal
                .draw(|f| draw_config_content(f, &view, body_area))
                .unwrap();
            format!("{:?}", terminal.backend().buffer())
        };

        let overridden = render(
            &mut terminal,
            crate::settings::ScanMode::Precise,
            crate::settings::ScanMode::Fast,
        );
        assert!(overridden.contains("Scan mode: Precise"), "{overridden}");
        assert!(
            overridden.contains("session override; saved default: Fast"),
            "{overridden}"
        );

        let in_sync = render(
            &mut terminal,
            crate::settings::ScanMode::Fast,
            crate::settings::ScanMode::Fast,
        );
        assert!(in_sync.contains("Scan mode: Fast"), "{in_sync}");
        assert!(
            !in_sync.contains("session override"),
            "the annotation disappears once the values agree: {in_sync}"
        );
    }

    /// Issue #239: an empty result set shows a non-selectable notice, not a blank list.
    #[test]
    fn test_draw_palette_content_shows_no_match_notice() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<PaletteAction> = Vec::new();
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "zzz",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(buffer.contains(PALETTE_NO_MATCH), "{buffer}");
    }

    /// Issue #239: unavailable rows stay listed with the reason they cannot run.
    #[test]
    fn test_draw_palette_content_shows_the_disabled_reason() {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let items = vec![PaletteAction::gated(
            "D",
            "Compare via External Diff Tool",
            PaletteActionId::ExternalDiff,
            false,
            "no external diff tool is configured",
        )];
        let view = PaletteView {
            items: &items,
            selected_idx: 0,
            scroll_offset: 0,
            query: "",
            theme: Theme::DARK,
        };

        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(
            buffer.contains("Compare via External Diff Tool"),
            "{buffer}"
        );
        assert!(
            buffer.contains("no external diff tool is configured"),
            "{buffer}"
        );
    }

    /// Issue #239: the popup clamps to the terminal, and its geometry is the one
    /// source of truth for both painting and hit-testing.
    #[test]
    fn test_palette_layout_clamps_to_the_terminal() {
        // Roomy terminal: the popup stops at its maximum width and grows to fit.
        let roomy = palette_layout(6, Rect::new(0, 0, 120, 40));
        assert_eq!(roomy.popup.width, PALETTE_MAX_WIDTH);
        assert_eq!(roomy.visible_rows(), 6);
        assert!(roomy.popup.x + roomy.popup.width <= 120);
        assert!(roomy.popup.y + roomy.popup.height <= 40);

        // Narrow, short terminal: never wider or taller than the screen.
        let tiny = palette_layout(40, Rect::new(0, 0, 24, 10));
        assert_eq!(tiny.popup.width, 24);
        assert!(tiny.popup.height <= 10);
        assert!(tiny.popup.x + tiny.popup.width <= 24);
        assert!(tiny.popup.y + tiny.popup.height <= 10);
        assert_eq!(tiny.visible_rows(), 6, "10 rows minus 4 rows of chrome");

        // A long inventory never grows past the screen either.
        let long = palette_layout(200, Rect::new(0, 0, 120, 40));
        assert!(long.popup.height <= 40);

        // The list always keeps room for the no-match notice.
        let empty = palette_layout(0, Rect::new(0, 0, 120, 40));
        assert_eq!(empty.visible_rows(), 1);
    }

    /// Issue #239: long labels truncate by display width, so CJK cannot overflow.
    #[test]
    fn test_truncate_to_width_measures_display_columns() {
        assert_eq!(truncate_to_width("short", 10), "short");
        assert_eq!(truncate_to_width("", 10), "");
        assert_eq!(truncate_to_width("abcdef", 0), "");
        assert_eq!(truncate_to_width("abcdef", 4), "abc…");

        // Each CJK character is two columns wide, so only two fit in five
        // columns once the ellipsis takes one.
        let cjk = "比較目錄樹狀圖";
        let truncated = truncate_to_width(cjk, 5);
        assert_eq!(truncated, "比較…");
        let width: usize = truncated.chars().map(|c| c.width().unwrap_or(0)).sum();
        assert!(width <= 5, "{truncated} is {width} columns");
    }

    /// Issue #239: the ninth and later items are reachable and rendered.
    #[test]
    fn test_draw_palette_content_renders_items_past_the_first_screenful() {
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let items: Vec<PaletteAction> = (0..20)
            .map(|i| {
                PaletteAction::new(
                    &i.to_string(),
                    &format!("Action {i}"),
                    PaletteActionId::Help,
                )
            })
            .collect();
        let layout = palette_layout(items.len(), Rect::new(0, 0, 100, 12));
        assert!(
            layout.visible_rows() < items.len(),
            "the test needs a viewport smaller than the inventory"
        );

        let view = PaletteView {
            items: &items,
            selected_idx: 12,
            scroll_offset: 12 + 1 - layout.visible_rows(),
            query: "",
            theme: Theme::DARK,
        };
        terminal
            .draw(|f| draw_palette_content(f, &view, f.area()))
            .unwrap();

        let buffer = format!("{:?}", terminal.backend().buffer());
        assert!(buffer.contains("Action 12"), "{buffer}");
        assert!(!buffer.contains("Action 0 "), "{buffer}");
    }
}
