use crate::app::{App, FlatRow, HelpTopic, PaletteMode, ViewMode};
use crate::diff::DiffState;
use crate::theme::Theme;
use ratatui::{prelude::*, widgets::*};
use std::time::SystemTime;
use unicode_width::UnicodeWidthChar;

/// Format a `SystemTime` as a local datetime string (YYYY-MM-DD HH:MM:SS).
fn format_system_time(t: &SystemTime) -> String {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => {
            // Apply local UTC offset (best-effort using libc localtime)
            let secs = dur.as_secs() as i64;
            #[cfg(unix)]
            {
                let mut tm: libc::tm = unsafe { std::mem::zeroed() };
                unsafe { libc::localtime_r(&secs, &mut tm) };
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    tm.tm_year + 1900,
                    tm.tm_mon + 1,
                    tm.tm_mday,
                    tm.tm_hour,
                    tm.tm_min,
                    tm.tm_sec,
                )
            }
            #[cfg(not(unix))]
            {
                // Fallback: UTC display
                let total_secs = secs;
                let s = total_secs % 60;
                let m = (total_secs / 60) % 60;
                let h = (total_secs / 3600) % 24;
                let days = total_secs / 86400;
                // Approximate date from days since epoch
                let (y, mo, d) = days_to_date(days);
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", y, mo, d, h, m, s)
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(not(unix))]
fn days_to_date(days_since_epoch: i64) -> (i64, i64, i64) {
    // Simplified Gregorian date calculation from days since 1970-01-01
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
fn selected_row_detail(row: Option<&FlatRow>) -> Option<(String, String)> {
    let row = row?;
    match row.state {
        DiffState::DifferentNewerLeft
        | DiffState::DifferentNewerRight
        | DiffState::DifferentSameTime => {}
        _ => return None,
    }
    let left = row.left.as_ref()?;
    let right = row.right.as_ref()?;

    let left_time = format_system_time(&left.modified);
    let right_time = format_system_time(&right.modified);

    let (left_tag, right_tag) = match row.state {
        DiffState::DifferentNewerLeft => (" (newer)", ""),
        DiffState::DifferentNewerRight => ("", " (newer)"),
        _ => ("", ""),
    };

    if left.is_dir {
        Some((
            format!("{}{}", left_time, left_tag),
            format!("{}{}", right_time, right_tag),
        ))
    } else {
        Some((
            format!("{} {}{}", format_size(left.size), left_time, left_tag),
            format!("{} {}{}", format_size(right.size), right_time, right_tag),
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

pub fn draw_top_bar(f: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme();
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(22)])
        .split(area);

    let left_text = match app.view_mode {
        ViewMode::DirectoryTree => {
            if app.precise_mode {
                " duodiff - Directory Tree [Precise] ".to_string()
            } else {
                " duodiff - Directory Tree [Fast] ".to_string()
            }
        }
        ViewMode::FileDiff => {
            let context_label = if app.diff_show_full {
                "Full"
            } else {
                "Diff Only"
            };
            let wrap_label = if app.diff_wrap { "Wrap" } else { "No Wrap" };
            format!(" duodiff - File Diff [{}] [{}] ", context_label, wrap_label)
        }
        ViewMode::ConfigMenu => " duodiff - Configuration ".to_string(),
        ViewMode::Help => " duodiff - Help ".to_string(),
    };

    let left_p = Paragraph::new(Line::from(vec![Span::styled(
        left_text,
        Style::default().fg(theme.emphasis).bold(),
    )]));
    f.render_widget(left_p, layout[0]);

    let right_p = Paragraph::new(Line::from(vec![
        Span::styled(" (", Style::default().fg(theme.muted)),
        Span::styled("C", Style::default().fg(theme.accent).bold()),
        Span::styled(")onfig", Style::default().fg(theme.muted)),
        Span::raw("  "),
        Span::styled("(", Style::default().fg(theme.muted)),
        Span::styled("?", Style::default().fg(theme.accent).bold()),
        Span::styled(")Help", Style::default().fg(theme.muted)),
        Span::raw(" "),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(right_p, layout[1]);
}

pub fn draw(f: &mut Frame, app: &mut App) {
    // Paint the full canvas so every unfilled cell uses the theme background (no-op for
    // dark theme where bg=Reset; effective for light theme which sets a white canvas).
    f.render_widget(Block::default().style(app.theme().base_style()), f.area());

    match app.view_mode {
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
/// Shared by [`draw_tree`] and [`App::sync_viewport`], so the rects the renderer
/// draws into and the geometry scrolling is clamped against cannot drift apart.
pub fn tree_layout(app: &App, area: Rect) -> TreeLayout {
    let has_detail = selected_row_detail(app.selected_row()).is_some();
    let has_status = app.status_message.is_some();
    let has_filter = app.filter_active();
    let has_update = app.update_available.is_some();
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
/// Takes `&App`: geometry is produced by [`App::sync_viewport`] before the frame
/// starts, so drawing stays a pure read of application state.
pub fn draw_tree(f: &mut Frame, app: &App) {
    let theme = app.theme();
    let layout = tree_layout(app, f.area());

    // Draw Top Bar
    draw_top_bar(f, app, layout.top_bar);

    // Geometry comes from `App::sync_viewport`, which the event loop runs before
    // this draw; rendering only reads it.
    let visible_height = app.viewport().visible_height;

    let mut left_items = Vec::new();
    let mut indicator_items = Vec::new();
    let mut right_items = Vec::new();

    // Pad the indicator column with a blank top line so symbols align
    // vertically with items in the bordered left/right panes (which have
    // a top border row).
    indicator_items.push(ListItem::new(""));

    for (i, row) in app
        .filtered_rows()
        .iter()
        .enumerate()
        .skip(app.scroll_offset())
        .take(visible_height)
    {
        let is_selected = i == app.selected_idx();
        let style = if is_selected {
            Style::default()
                .bg(theme.selection_bg)
                .fg(theme.selection_fg)
        } else {
            match row.state {
                DiffState::Identical => Style::default().fg(theme.muted),
                DiffState::DifferentNewerLeft
                | DiffState::DifferentNewerRight
                | DiffState::DifferentSameTime => Style::default().fg(theme.warn),
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
            get_display_path(&app.left_path, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);
    let right_title = Line::from(vec![
        Span::raw(" "),
        Span::styled("[2] ", Style::default().fg(theme.accent).bold()),
        Span::styled(
            get_display_path(&app.right_path, 31),
            Style::default().bold(),
        ),
        Span::raw(" "),
    ]);

    let left_border_style = if app.active_side_left {
        Style::default().fg(theme.border_focus)
    } else {
        Style::default().fg(theme.dim)
    };

    let right_border_style = if !app.active_side_left {
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

    // Draw Footer
    let row = app.selected_row();

    let footer_txt = if app.scan_in_progress() {
        Line::from("Scanning in progress... Please wait.")
    } else {
        Line::from(vec![
            Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
            Span::raw("Menu  ·  "),
            Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
            Span::raw("Palette"),
        ])
    };

    // Build footer lines (top → bottom: status, detail, filter input, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    if let Some((left_detail, right_detail)) = selected_row_detail(row) {
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
    if app.filter_active() {
        let mut filter_spans = vec![Span::styled(
            " Filter: ",
            Style::default().fg(theme.warn).bold(),
        )];
        filter_spans.extend(text_input_spans(
            app.filter_input(),
            Style::default().fg(theme.warn),
        ));
        if app.filter_diffs_only() {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    } else if !app.filter_pattern().is_empty() || app.filter_diffs_only() {
        let mut filter_spans = vec![
            Span::styled(" Filter: ", Style::default().fg(theme.warn).bold()),
            Span::raw(app.filter_pattern()),
            Span::styled(
                "  (/:edit, Backspace at empty:clear)",
                Style::default().fg(theme.dim),
            ),
        ];
        if app.filter_diffs_only() {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(theme.accent),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    }

    footer_lines.push(footer_txt);

    if let Some(ref version) = app.update_available {
        let hint = crate::update_check::update_hint(version, &app.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
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

/// Regions of the file-diff screen.
pub struct DiffLayout {
    pub top_bar: Rect,
    /// Row below the top bar carrying the "files are identical" notice; empty
    /// unless [`DiffLayout::show_identical`].
    pub notice: Rect,
    /// Left half of the info bar (size + MD5 + line ending).
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
/// Shared by [`draw_diff`] and [`App::sync_viewport`], so the rects the renderer
/// draws into and the geometry scrolling is clamped against cannot drift apart.
pub fn diff_layout(app: &App, area: Rect) -> DiffLayout {
    let row = app.selected_row();

    let has_changes = app.diff_has_changes();
    let show_identical = !has_changes && row.is_some_and(|r| r.left.is_some() || r.right.is_some());

    let header_height = if show_identical { 2 } else { 1 };
    let has_update = app.update_available.is_some();
    let footer_height =
        if app.status_message.is_some() { 2 } else { 1 } + if has_update { 1 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Header (Top Bar + optional Identical Msg)
            Constraint::Length(1),             // Info bar (size + MD5)
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
/// Takes `&App` for the same reason as [`draw_tree`].
pub fn draw_diff(f: &mut Frame, app: &App) {
    let theme = app.theme();
    let row = app.selected_row();
    let layout = diff_layout(app, f.area());
    let show_identical = layout.show_identical;

    draw_top_bar(f, app, layout.top_bar);

    if show_identical {
        let msg = Paragraph::new(Line::from(Span::styled(
            " ✓ Both files are identical — no differences found.",
            Style::default().fg(theme.success).bold(),
        )));
        f.render_widget(msg, layout.notice);
    }

    // Info bar: size + MD5 hash for each side, above the pane borders
    let left_info = build_diff_info_spans(
        row,
        true,
        &app.diff_left_hash,
        &app.diff_left_line_ending,
        theme,
    );
    let right_info = build_diff_info_spans(
        row,
        false,
        &app.diff_right_hash,
        &app.diff_right_line_ending,
        theme,
    );
    f.render_widget(Paragraph::new(left_info), layout.info_left);
    f.render_widget(Paragraph::new(right_info), layout.info_right);

    // Geometry comes from `App::sync_viewport`, which the event loop runs before
    // this draw; rendering only reads it.
    let viewport = app.viewport();
    let max_visible = viewport.visible_height;
    let content_width = viewport.diff_content_width;

    if let Some(row) = row {
        let mut left_physical: Vec<DiffDisplayCell> = Vec::new();
        let mut right_physical: Vec<DiffDisplayCell> = Vec::new();

        let hunk_row_ranges = crate::diff_view::diff_hunk_row_ranges(app.diff_rows());
        let active_hunk_rows = crate::diff_view::hunk_index_at_scroll(
            app.diff_rows(),
            app.diff_scroll(),
            content_width,
            app.diff_wrap,
        )
        .and_then(|idx| hunk_row_ranges.get(idx).cloned());

        let mut physical_row = 0usize;
        for (logical_row, (left_line, right_line)) in app.diff_rows().iter().enumerate() {
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
                    left_text.zip(right_text).map(|(left, right)| {
                        crate::diff_view::intraline_change_mask(left, right, true)
                    })
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
                app.diff_wrap,
                content_width,
                app.diff_h_scroll(),
            );
            push_diff_display_cells(
                &mut right_chunk,
                right_text,
                right_tag,
                right_mask,
                app.diff_wrap,
                content_width,
                app.diff_h_scroll(),
            );

            let max_lines = std::cmp::max(left_chunk.len(), right_chunk.len());
            for i in 0..max_lines {
                let highlight = diff_line_highlight(
                    in_change_hunk,
                    in_active_hunk,
                    physical_row + i == app.diff_scroll(),
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
            .skip(app.diff_scroll())
            .take(max_visible)
            .map(|cell| line_from_diff_cell(&cell, theme))
            .collect();

        let right_lines: Vec<Line> = right_physical
            .into_iter()
            .skip(app.diff_scroll())
            .take(max_visible)
            .map(|cell| line_from_diff_cell(&cell, theme))
            .collect();

        // Build pane titles: " /truncated/path/file.txt (3d ago) "
        let pane_width = layout.left.width as usize;
        let left_title = build_diff_pane_title(
            &app.left_path.join(&row.relative_path),
            row.left.as_ref().map(|f| &f.modified),
            pane_width,
        );
        let right_title = build_diff_pane_title(
            &app.right_path.join(&row.relative_path),
            row.right.as_ref().map(|f| &f.modified),
            pane_width,
        );

        let left_p = Paragraph::new(left_lines).block(
            Block::default()
                .title(Span::styled(left_title, Style::default().bold()))
                .borders(Borders::ALL),
        );
        let right_p = Paragraph::new(right_lines).block(
            Block::default()
                .title(Span::styled(right_title, Style::default().bold()))
                .borders(Borders::ALL),
        );

        f.render_widget(left_p, layout.left);
        f.render_widget(right_p, layout.right);
        draw_close_button(f, layout.right);
    }

    // Build footer lines (top → bottom: status, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.success).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    let has_changes = app.diff_has_changes();
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
        Span::raw("Menu  ·  "),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Palette"),
    ];
    if !has_changes {
        footer_spans.drain(0..10);
    }
    footer_lines.push(Line::from(footer_spans));

    if let Some(ref version) = app.update_available {
        let hint = crate::update_check::update_hint(version, &app.install_method);
        footer_lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(theme.warn).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines);
    f.render_widget(footer_p, layout.footer);
}

/// Build info spans (size + line ending style + MD5 hash) for the diff view info bar.
fn build_diff_info_spans<'a>(
    row: Option<&'a FlatRow>,
    is_left: bool,
    hash: &'a Option<String>,
    line_ending: &'a Option<String>,
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
            format!("MD5: {}", h),
            Style::default().fg(theme.dim),
        ));
    } else {
        spans.push(Span::styled("MD5: —", Style::default().fg(theme.dim)));
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

fn help_topic_body(topic: HelpTopic, app: &App, theme: Theme) -> Text<'static> {
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

Actions
  Enter          open the diff view (or toggle expand, for a directory)
  D              compare the selected file pair with the external diff tool
  E              edit the selected file in $EDITOR/$VISUAL
  L              copy the selected item from the right pane to the left (y/n confirm)
  R              copy the selected item from the left pane to the right (y/n confirm)
  C              open the Config menu
  c              toggle Fast / Precise scan mode (re-scans)
  r              force a manual re-scan
  s              swap the left and right directories
  /              open the filter bar (f while typing: diffs-only toggle)
  ?              show this help
  q / Esc        quit",
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
  l / L          copy the whole right file to the left side (y/n confirm)
  r / R          copy the whole left file to the right side (y/n confirm)
  w              toggle line wrapping
  f              toggle full-file context vs diff-only
  C              open the Config menu (returns here on Esc/q)
  ?              show this help
  q / Esc        return to the Directory Tree view",
        ),
        HelpTopic::Config => Text::from(
            "  j / k, Down / Up   move the selection
  Enter / Space      select the highlighted external diff tool
                     or toggle Check for updates / Mouse support / Theme
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
  Right Click    select a row and open the context menu
  Double Click   open diff view for a file, or expand/collapse a directory
  Scroll         scroll the directory tree, diff lines, Config screen, Help
                 topic/index, or the menu/palette list; over the Config
                 screen's Diff context row, scroll adjusts its value

  Mouse is on by default; disable it in Config, in config.toml
  (mouse = false), or for one session with --no-mouse.",
        ),
        HelpTopic::General => Text::from(
            "  ?              show this help
  q / Esc        quit (or back, on any sub-screen)
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
            if let Some(ref version) = app.update_available {
                lines.push(Line::from(crate::update_check::update_hint(
                    version,
                    &app.install_method,
                )));
            }
            Text::from(lines)
        }
    }
}

pub fn draw_help(f: &mut Frame, app: &mut App) {
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

    let body_area = chunks[1];
    if app.help_index_open() {
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
        list_state.select(Some(app.help_index_sel()));
        f.render_stateful_widget(list, body_area, &mut list_state);
    } else {
        let title = format!(
            "Help · {} — Tab topics · j/k scroll · Esc back",
            app.help_topic().title()
        );
        let paragraph = Paragraph::new(help_topic_body(app.help_topic(), app, theme))
            .scroll((app.help_scroll(), 0))
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(paragraph, body_area);
    }

    draw_close_button(f, body_area);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("Menu  ·  "),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

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
    app.ensure_config_selection();

    let rows = app.config_rows();
    let mut items = Vec::new();
    for (row_idx, row) in rows.iter().enumerate() {
        match row {
            crate::app::ConfigRowKind::Header(label) => {
                items.push(ListItem::new(Line::from(Span::styled(
                    *label,
                    Style::default().fg(theme.warn).bold(),
                ))));
            }
            crate::app::ConfigRowKind::DiffTool(tool_idx) => {
                let (tool, is_avail) = &app.detected_diff_tools[*tool_idx];
                let is_active = app.settings.external_diff_tool.as_deref() == Some(tool.as_str());
                let marker = if is_active { "[x] " } else { "[ ] " };
                let avail_str = if *is_avail {
                    "(Available)"
                } else {
                    "(Not Found)"
                };
                let style = if row_idx == app.config_selected_idx {
                    Style::default()
                        .bg(theme.selection_bg)
                        .fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                items.push(
                    ListItem::new(format!("  {}{:<5} {}", marker, tool.as_str(), avail_str))
                        .style(style),
                );
            }
            crate::app::ConfigRowKind::CheckUpdates => {
                let marker = if app.settings.check_updates {
                    "[x] "
                } else {
                    "[ ] "
                };
                let style = if row_idx == app.config_selected_idx {
                    Style::default()
                        .bg(theme.selection_bg)
                        .fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                items.push(
                    ListItem::new(format!("  {}Check for updates daily", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::Mouse => {
                let marker = if app.settings.mouse { "[x] " } else { "[ ] " };
                let style = if row_idx == app.config_selected_idx {
                    Style::default()
                        .bg(theme.selection_bg)
                        .fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                items.push(ListItem::new(format!("  {}Enable mouse support", marker)).style(style));
            }
            crate::app::ConfigRowKind::Theme => {
                let marker = if app.settings.theme == crate::theme::ThemeChoice::Light {
                    "[x] "
                } else {
                    "[ ] "
                };
                let style = if row_idx == app.config_selected_idx {
                    Style::default()
                        .bg(theme.selection_bg)
                        .fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                items.push(
                    ListItem::new(format!("  {}Light theme (off = dark)", marker)).style(style),
                );
            }
            crate::app::ConfigRowKind::DiffContext => {
                let style = if row_idx == app.config_selected_idx {
                    Style::default()
                        .bg(theme.selection_bg)
                        .fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                items.push(
                    ListItem::new(format!(
                        "      Diff context: {} lines (h/l to adjust)",
                        app.settings.diff_context
                    ))
                    .style(style),
                );
            }
        }
    }

    let list = List::new(items).block(
        Block::default()
            .title("Configuration")
            .borders(Borders::ALL),
    );
    f.render_widget(list, chunks[1]);
    draw_close_button(f, chunks[1]);

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" ; ", Style::default().fg(theme.accent).bold()),
        Span::raw("Menu  ·  "),
        Span::styled(" Ctrl+p ", Style::default().fg(theme.accent).bold()),
        Span::raw("Palette"),
    ]));
    f.render_widget(footer, chunks[2]);
}

pub fn draw_close_button(f: &mut Frame, area: Rect) {
    if area.width >= 6 {
        let button_area = Rect {
            x: area.x + area.width.saturating_sub(5),
            y: area.y,
            width: 3,
            height: 1,
        };
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

pub fn draw_palette(f: &mut Frame, app: &mut App) {
    let theme = app.theme();
    app.refresh_palette_items();
    let palette = app.palette();
    let mode = palette.mode.unwrap_or(PaletteMode::Menu);
    let count = palette.items.len();

    let (pop_w, pop_h) = match mode {
        PaletteMode::Menu => (50, (count + 2).max(4) as u16),
        PaletteMode::Command => (55, 12),
    };

    let area = centered_rect(pop_w, pop_h, f.area());
    f.render_widget(Clear, area);

    let title = match mode {
        PaletteMode::Menu => " Menu ".to_string(),
        PaletteMode::Command => " Palette (Ctrl+p) ".to_string(),
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().bold()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn));

    match mode {
        PaletteMode::Menu => {
            let mut list_items = Vec::new();
            for (i, action) in palette.items.iter().enumerate() {
                let display_text = format!("  {:<5}  {}", action.key, action.label);
                let mut style = if i == palette.selected_idx {
                    Style::default().bg(theme.info).fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                if !action.enabled {
                    style = style.fg(theme.dim);
                }
                list_items.push(ListItem::new(display_text).style(style));
            }
            let list = List::new(list_items).block(block);
            f.render_widget(list, area);
            draw_close_button(f, area);
        }
        PaletteMode::Command => {
            // Internal layout of command palette
            let inner_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Query input line
                    Constraint::Length(1), // Separator line
                    Constraint::Min(0),    // List of matches
                ])
                .split(block.inner(area));

            // Query text paragraph
            let query_text = Line::from(vec![
                Span::styled(" Query: ", Style::default().fg(theme.accent)),
                Span::raw(&palette.query),
                Span::styled("█", Style::default().fg(theme.emphasis)),
            ]);
            f.render_widget(Paragraph::new(query_text), inner_chunks[0]);

            // Separator
            let separator = Paragraph::new(Line::from(vec![Span::styled(
                "─".repeat(inner_chunks[1].width as usize),
                Style::default().fg(theme.dim),
            )]));
            f.render_widget(separator, inner_chunks[1]);

            // List of matching actions
            let mut list_items = Vec::new();
            for (i, action) in palette.items.iter().enumerate() {
                let display_text = format!("  {:<5}  {}", action.key, action.label);
                let mut style = if i == palette.selected_idx {
                    Style::default().bg(theme.info).fg(theme.selection_fg)
                } else {
                    Style::default()
                };
                if !action.enabled {
                    style = style.fg(theme.dim);
                }
                list_items.push(ListItem::new(display_text).style(style));
            }
            let list = List::new(list_items);
            f.render_widget(list, inner_chunks[2]);

            // Render block borders around the entire popup
            f.render_widget(block, area);
            draw_close_button(f, area);
        }
    }
}

pub fn draw_confirm_modal(f: &mut Frame, app: &mut App) {
    let theme = app.theme();
    let message = app
        .confirm_modal()
        .map(|modal| modal.message.as_str())
        .unwrap_or_default();
    let area = centered_rect(60, 7, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Confirm Action ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warn));

    let text = vec![
        Line::from(""),
        Line::from(Span::raw(message)).alignment(Alignment::Center),
        Line::from(""),
        Line::from(Span::styled(
            " [Y] Yes   [N] No (Cancel) ",
            Style::default().fg(theme.accent),
        ))
        .alignment(Alignment::Center),
    ];

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
    draw_close_button(f, area);
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
        app.view_mode = ViewMode::Help;
        app.select_help_topic(crate::app::HelpTopic::DirectoryTree);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            buffer_string.contains("Help · Directory Tree — Tab topics · j/k scroll · Esc back"),
            "Help topic-body header should show the topic title and operation hints"
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
        app.view_mode = ViewMode::Help;
        app.select_help_topic(crate::app::HelpTopic::FileDiff);

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
        app.view_mode = ViewMode::Help;
        app.set_help_index_open(true);

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
        app.detected_diff_tools = vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Code, false),
        ];
        app.view_mode = ViewMode::ConfigMenu;

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

    #[test]
    fn test_draw_tree_footer_mentions_help_key() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        draw_frame(&mut terminal, &mut app);

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
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        draw_frame(&mut terminal, &mut app);

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
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // Inject a row with a difference so the detail line appears in the footer
        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);

        draw_frame(&mut terminal, &mut app);

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
        app.view_mode = ViewMode::FileDiff;

        // diff_rows with only Equal tags → files are identical
        app.set_diff_rows(vec![DiffRow::from((
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
        let left_path = app.left_path.join("same.txt");
        let right_path = app.right_path.join("same.txt");
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

    #[test]
    fn test_diff_view_intraline_highlight_splits_replacement_line() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "let foo = 1;".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "let bar = 1;".to_string(),
            }),
        ))]);

        draw_frame(&mut terminal, &mut app);

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
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.view_mode = ViewMode::FileDiff;

        // diff_rows with a Delete tag → files differ
        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old line".to_string(),
            }),
            None,
        ))]);

        draw_frame(&mut terminal, &mut app);

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        assert!(
            !buffer_string.contains("identical"),
            "Diff view should NOT show identical notice when files differ: {}",
            buffer_string
        );
    }

    #[test]
    fn test_diff_view_shows_size_and_md5_above_border() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.view_mode = ViewMode::FileDiff;
        app.diff_left_hash = Some("aabbccdd11223344".to_string());
        app.diff_right_hash = Some("eeff001122334455".to_string());

        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old".to_string(),
            }),
            None,
        ))]);

        draw_frame(&mut terminal, &mut app);

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
        // MD5 hashes should be displayed
        assert!(
            buffer_string.contains("MD5: aabbccdd11223344"),
            "Diff view should show left MD5 hash: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("MD5: eeff001122334455"),
            "Diff view should show right MD5 hash: {}",
            buffer_string
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

        let backend = TestBackend::new(40, 30);
        let mut terminal = Terminal::new(backend).unwrap();
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
        app.view_mode = ViewMode::FileDiff;

        // One logical row with a long line (52 chars). At 40-column terminal,
        // content width is ~18, so wrapping should produce multiple physical rows.
        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "this is a very long line that exceeds the pane width".to_string(),
            }),
        ))]);

        app.diff_wrap = false;
        draw_frame(&mut terminal, &mut app);
        let no_wrap_rows = app.viewport().diff_physical_rows;

        app.diff_wrap = true;
        draw_frame(&mut terminal, &mut app);
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
        app.view_mode = ViewMode::FileDiff;
        app.diff_wrap = false;
        app.set_diff_h_scroll(5);

        // Longer than the 38-column pane, so an offset of 5 is a legal scroll
        // position rather than one `sync_viewport` would clamp away.
        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJ".to_string(),
            }),
        ))]);

        draw_frame(&mut terminal, &mut app);

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
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // Set explicitly: other tests in this binary persist `settings.theme` to the
        // real config file, and `App::new` reloads from disk, so this assertion (which
        // hardcodes the dark-theme Rgb values below) must not depend on load order.
        app.settings.theme = crate::theme::ThemeChoice::Dark;

        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![
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
        ]);
        // Cursor on context line; the nearest change hunk row should still be emphasized.
        app.set_diff_scroll(0);

        draw_frame(&mut terminal, &mut app);

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
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.settings.theme = crate::theme::ThemeChoice::Light;

        app.push_flat_row(FlatRow {
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
        });
        app.apply_filter();
        app.set_selected_idx(0);
        app.view_mode = ViewMode::FileDiff;
        app.set_diff_rows(vec![
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
        ]);
        // Cursor on context line, same as the dark-theme equivalent test, so the
        // nearest change hunk (not the cursor row) is the one under assertion.
        app.set_diff_scroll(0);

        draw_frame(&mut terminal, &mut app);

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
        app.settings.theme = crate::theme::ThemeChoice::Light;

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
        app.settings.theme = crate::theme::ThemeChoice::Dark;
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
        app.settings.theme = crate::theme::ThemeChoice::Light;
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
        app.settings.theme = crate::theme::ThemeChoice::Dark;
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
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

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
        app.view_mode = ViewMode::FileDiff;
        app.diff_wrap = true;

        app.set_diff_rows(vec![DiffRow::from((
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
}
