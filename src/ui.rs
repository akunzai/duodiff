use crate::app::{App, FlatRow, ViewMode};
use crate::diff::DiffState;
use ratatui::{prelude::*, widgets::*};
use std::time::SystemTime;

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
fn selected_row_detail(row: Option<&FlatRow>) -> Option<String> {
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
        Some(format!(
            "Left: {}{} | Right: {}{}",
            left_time, left_tag, right_time, right_tag,
        ))
    } else {
        Some(format!(
            "Left: {} {}{} | Right: {} {}{}",
            format_size(left.size),
            left_time,
            left_tag,
            format_size(right.size),
            right_time,
            right_tag,
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

pub fn draw(f: &mut Frame, app: &mut App) {
    match app.view_mode {
        ViewMode::DirectoryTree => {
            draw_tree(f, app);
            if app.context_menu.visible {
                draw_context_menu(f, app);
            }
            if app.show_confirm_modal {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::FileDiff => {
            draw_diff(f, app);
            if app.show_confirm_modal {
                draw_confirm_modal(f, app);
            }
        }
        ViewMode::ConfigMenu => draw_config_menu(f, app),
        ViewMode::ConfigDiffTool => draw_config_diff_tool(f, app),
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

pub fn draw_tree(f: &mut Frame, app: &mut App) {
    let has_detail = selected_row_detail(app.filtered_rows.get(app.selected_idx)).is_some();
    let has_status = app.status_message.is_some();
    let has_filter = app.filter_active;
    let footer_height = match (has_detail, has_status, has_filter) {
        (true, true, true) => 5,
        (true, true, false) => 4,
        (true, false, true) | (false, true, true) => 4,
        (true, false, false) | (false, true, false) | (false, false, true) => 3,
        (false, false, false) => 2,
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),             // Header (1 line + border)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(f.area());

    // Draw Header
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(30), Constraint::Length(16)])
        .split(chunks[0]);

    let header_text = vec![Line::from(vec![
        Span::styled(
            " duodiff ",
            Style::default().fg(Color::Yellow).bold().bg(Color::Blue),
        ),
        Span::raw("  |  "),
        Span::styled(
            if app.precise_mode {
                "Precise (MD5)"
            } else {
                "Fast (Size & Time)"
            },
            Style::default().fg(Color::Cyan).bold(),
        ),
        Span::raw("  |  Focus: "),
        Span::styled(
            if app.active_side_left {
                "Left Pane"
            } else {
                "Right Pane"
            },
            Style::default().fg(Color::Green).bold(),
        ),
    ])];
    let header_paragraph =
        Paragraph::new(header_text).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header_paragraph, header_chunks[0]);

    let config_button = Paragraph::new(Line::from(vec![Span::styled(
        " ⚙️  Config [C] ",
        Style::default().fg(Color::Cyan).bold(),
    )]))
    .alignment(ratatui::layout::Alignment::Right)
    .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(config_button, header_chunks[1]);

    // Draw Body
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(30),   // Left
            Constraint::Length(4), // Indicator (no borders, symbols only)
            Constraint::Min(30),   // Right
        ])
        .split(chunks[1]);

    let visible_height = body_chunks[0].height.saturating_sub(2) as usize;
    app.visible_height = visible_height;
    app.adjust_scroll(visible_height);

    let mut left_items = Vec::new();
    let mut indicator_items = Vec::new();
    let mut right_items = Vec::new();

    // Pad the indicator column with a blank top line so symbols align
    // vertically with items in the bordered left/right panes (which have
    // a top border row).
    indicator_items.push(ListItem::new(""));

    for (i, row) in app
        .filtered_rows
        .iter()
        .enumerate()
        .skip(app.scroll_offset)
        .take(visible_height)
    {
        let is_selected = i == app.selected_idx;
        let style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            match row.state {
                DiffState::Identical => Style::default().fg(Color::Gray),
                DiffState::DifferentNewerLeft
                | DiffState::DifferentNewerRight
                | DiffState::DifferentSameTime => Style::default().fg(Color::Yellow),
                DiffState::LeftOnly => Style::default().fg(Color::Green),
                DiffState::RightOnly => Style::default().fg(Color::Blue),
                DiffState::TypeConflict => Style::default().fg(Color::Red).bold(),
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

    let left_title = format!(" Left: {} ", get_display_path(&app.left_path, 35));
    let right_title = format!(" Right: {} ", get_display_path(&app.right_path, 35));

    let left_border_style = if app.active_side_left {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let right_border_style = if !app.active_side_left {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let left_list = List::new(left_items).block(
        Block::default()
            .title(Span::styled(left_title, Style::default().bold()))
            .border_style(left_border_style)
            .borders(Borders::ALL),
    );

    let indicator_list = List::new(indicator_items);

    let right_list = List::new(right_items).block(
        Block::default()
            .title(Span::styled(right_title, Style::default().bold()))
            .border_style(right_border_style)
            .borders(Borders::ALL),
    );

    f.render_widget(left_list, body_chunks[0]);
    f.render_widget(indicator_list, body_chunks[1]);
    f.render_widget(right_list, body_chunks[2]);

    // Draw Footer
    let row = app.filtered_rows.get(app.selected_idx);
    let is_file_pair = row.is_some_and(|r| {
        let is_dir = r.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || r.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
        !is_dir && r.left.is_some() && r.right.is_some()
    });
    let has_tool = app.settings.external_diff_tool.is_some();
    let is_file_active = row.is_some_and(|r| {
        if app.active_side_left {
            r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
        } else {
            r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
        }
    });

    let footer_txt = if app.scan_in_progress {
        "Scanning in progress... Please wait.".to_string()
    } else {
        let mut btns = "q:Quit | Tab:Focus Side | Space:Expand | Enter:Diff".to_string();
        if let Some(r) = row {
            if r.right.is_some() {
                btns.push_str(" | L:←Copy");
            }
            if r.left.is_some() {
                btns.push_str(" | R:Copy→");
            }
        }
        if has_tool && is_file_pair {
            btns.push_str(" | D:Ext Diff");
        }
        if is_file_active {
            btns.push_str(" | E:Edit File");
        }
        btns.push_str(" | c:Mode | r:Refresh | s:Swap | /:Filter");
        btns
    };

    // Build footer lines (top → bottom: status, detail, filter input, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(Color::Red).bold()
        } else {
            Style::default().fg(Color::Green).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    if let Some(detail) = selected_row_detail(row) {
        footer_lines.push(Line::from(Span::styled(
            detail,
            Style::default().fg(Color::Cyan),
        )));
    }

    // Filter input bar (shown when filter is active or a pattern is committed)
    if app.filter_active {
        let mut filter_spans = vec![
            Span::styled(" Filter: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.filter_input),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ];
        if app.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(Color::Cyan),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    } else if !app.filter_pattern.is_empty() || app.filter_diffs_only {
        let mut filter_spans = vec![
            Span::styled(" Filter: ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.filter_pattern),
            Span::styled(
                "  (/:edit, Backspace at empty:clear)",
                Style::default().fg(Color::DarkGray),
            ),
        ];
        if app.filter_diffs_only {
            filter_spans.push(Span::styled(
                "  [diffs only]",
                Style::default().fg(Color::Cyan),
            ));
        }
        footer_lines.push(Line::from(filter_spans));
    }

    footer_lines.push(Line::from(footer_txt));

    let mut block = Block::default().borders(Borders::TOP);
    if let Some(ref version) = app.update_available {
        let hint = crate::update_check::update_hint(version, &app.install_method);
        block = block.title(Line::from(Span::styled(
            format!(" {} ", hint),
            Style::default().fg(Color::Yellow).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines).block(block);
    f.render_widget(footer_p, chunks[2]);
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

pub fn draw_diff(f: &mut Frame, app: &mut App) {
    let row = app.filtered_rows.get(app.selected_idx);

    // Check if files are identical (no Insert/Delete tags in diff_rows)
    let has_changes = app.diff_rows.iter().any(|(l, r)| {
        l.as_ref().map(|d| d.tag) == Some(similar::ChangeTag::Delete)
            || r.as_ref().map(|d| d.tag) == Some(similar::ChangeTag::Insert)
    });
    let show_identical = !has_changes && row.is_some_and(|r| r.left.is_some() || r.right.is_some());

    let header_height = if show_identical { 2 } else { 1 };
    let footer_height = if app.status_message.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height), // Header
            Constraint::Length(1),             // Info bar (size + MD5)
            Constraint::Min(5),                // Body
            Constraint::Length(footer_height), // Footer
        ])
        .split(f.area());

    // Header: title + optional identical notice (no border)
    let mut header_lines = vec![Line::from("File Comparison View - Esc/q to return")];
    if show_identical {
        header_lines.push(Line::from(Span::styled(
            " ✓ Both files are identical — no differences found.",
            Style::default().fg(Color::Green).bold(),
        )));
    }
    let header = Paragraph::new(header_lines);
    f.render_widget(header, chunks[0]);

    // Info bar: size + MD5 hash for each side, above the pane borders
    let info_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    let left_info =
        build_diff_info_spans(row, true, &app.diff_left_hash, &app.diff_left_line_ending);
    let right_info = build_diff_info_spans(
        row,
        false,
        &app.diff_right_hash,
        &app.diff_right_line_ending,
    );
    f.render_widget(Paragraph::new(left_info), info_chunks[0]);
    f.render_widget(Paragraph::new(right_info), info_chunks[1]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let max_visible = chunks[2].height.saturating_sub(2) as usize;
    app.visible_height = max_visible;

    if let Some(row) = row {
        let mut left_lines = Vec::new();
        let mut right_lines = Vec::new();

        // Diff content lines
        for (i, (left_line, right_line)) in app.diff_rows.iter().enumerate().skip(app.diff_scroll) {
            if i >= app.diff_scroll + max_visible {
                break;
            }
            if let Some(line) = left_line {
                let style = match line.tag {
                    similar::ChangeTag::Delete => Style::default().fg(Color::Red),
                    _ => Style::default().fg(Color::Gray),
                };
                left_lines.push(Line::from(Span::styled(
                    line.text.trim_end().to_string(),
                    style,
                )));
            } else {
                left_lines.push(Line::from(""));
            }

            if let Some(line) = right_line {
                let style = match line.tag {
                    similar::ChangeTag::Insert => Style::default().fg(Color::Green),
                    _ => Style::default().fg(Color::Gray),
                };
                right_lines.push(Line::from(Span::styled(
                    line.text.trim_end().to_string(),
                    style,
                )));
            } else {
                right_lines.push(Line::from(""));
            }
        }

        // Build pane titles: " Left: /truncated/path/file.txt (3d ago) "
        let pane_width = body_chunks[0].width as usize;
        let left_title = build_diff_pane_title(
            "Left",
            &app.left_path.join(&row.relative_path),
            row.left.as_ref().map(|f| &f.modified),
            pane_width,
        );
        let right_title = build_diff_pane_title(
            "Right",
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

        f.render_widget(left_p, body_chunks[0]);
        f.render_widget(right_p, body_chunks[1]);
    }

    let mut footer_text = "Esc/q: Back | j/↓: Scroll Down | k/↑: Scroll Up".to_string();
    if app.selected_idx < app.filtered_rows.len() {
        let row = &app.filtered_rows[app.selected_idx];
        if row.right.is_some() {
            footer_text.push_str(" | L:←Copy");
        }
        if row.left.is_some() {
            footer_text.push_str(" | R:Copy→");
        }
    }

    // Build footer lines (top → bottom: status, keybindings)
    let mut footer_lines: Vec<Line> = Vec::new();

    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(Color::Red).bold()
        } else {
            Style::default().fg(Color::Green).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        footer_lines.push(Line::from(Span::styled(
            format!("{}{}", icon, msg),
            status_style,
        )));
    }

    footer_lines.push(Line::from(footer_text));

    let mut block = Block::default().borders(Borders::TOP);
    if let Some(ref version) = app.update_available {
        let hint = crate::update_check::update_hint(version, &app.install_method);
        block = block.title(Line::from(Span::styled(
            format!(" {} ", hint),
            Style::default().fg(Color::Yellow).bold(),
        )));
    }
    let footer_p = Paragraph::new(footer_lines).block(block);
    f.render_widget(footer_p, chunks[3]);
}

/// Build info spans (size + line ending style + MD5 hash) for the diff view info bar.
fn build_diff_info_spans<'a>(
    row: Option<&'a FlatRow>,
    is_left: bool,
    hash: &'a Option<String>,
    line_ending: &'a Option<String>,
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
                Style::default().fg(Color::DarkGray),
            ));
            spans.push(Span::raw("  "));
        }
    }

    if let Some(le) = line_ending {
        spans.push(Span::styled(
            format!("[{}]", le),
            Style::default().fg(Color::DarkGray),
        ));
        spans.push(Span::raw("  "));
    }

    if let Some(h) = hash {
        spans.push(Span::styled(
            format!("MD5: {}", h),
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::styled("MD5: —", Style::default().fg(Color::DarkGray)));
    }

    Line::from(spans)
}
fn build_diff_pane_title(
    side: &str,
    full_path: &std::path::Path,
    modified: Option<&SystemTime>,
    pane_width: usize,
) -> String {
    let rel_time = modified.map(format_relative_time).unwrap_or_default();
    // Reserve space for " Side: " + " (rel_time) " + borders
    let prefix_len = side.len() + 4; // " Side: "
    let suffix_len = rel_time.len() + 4; // " (rel_time) "
    let max_path = pane_width
        .saturating_sub(prefix_len + suffix_len + 2)
        .max(10);
    let display_path = get_display_path(full_path, max_path);
    format!(" {}: {} ({}) ", side, display_path, rel_time)
}

pub fn draw_config_menu(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(f.area());

    let header = Paragraph::new("duodiff Configuration - Esc/q to return")
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    let items = vec![ListItem::new("1. External Diff Tool").style(
        if app.settings_menu_selected_idx == 0 {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default()
        },
    )];

    let menu_list = List::new(items).block(
        Block::default()
            .title("Configuration Categories")
            .borders(Borders::ALL),
    );
    f.render_widget(menu_list, chunks[1]);

    let footer =
        Paragraph::new("Enter: Select | Esc/q: Back").block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

pub fn draw_config_diff_tool(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(f.area());

    let header = Paragraph::new("Select External Diff Tool - Esc/q to return")
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    let mut items = Vec::new();
    for (i, (tool, is_avail)) in app.detected_diff_tools.iter().enumerate() {
        let is_selected = app.settings.external_diff_tool.as_deref() == Some(tool.as_str());
        let marker = if is_selected { "[x] " } else { "[ ] " };
        let avail_str = if *is_avail {
            "(Available)"
        } else {
            "(Not Found)"
        };
        let style = if i == app.config_diff_tool_selected_idx {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            Style::default()
        };
        items.push(
            ListItem::new(format!("{}{:<5} {}", marker, tool.as_str(), avail_str)).style(style),
        );
    }

    let list = List::new(items).block(
        Block::default()
            .title("Available Diff Tools")
            .borders(Borders::ALL),
    );
    f.render_widget(list, chunks[1]);

    let footer = Paragraph::new("Enter: Save & Back | Esc/q: Cancel")
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn centered_rect(width: u16, height: u16, parent: Rect) -> Rect {
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

pub fn draw_context_menu(f: &mut Frame, app: &mut App) {
    let area = centered_rect(40, 8, f.area());
    f.render_widget(Clear, area);

    let row = app.flat_rows.get(app.selected_idx);
    let is_file_pair = row.is_some_and(|r| {
        let is_dir = r.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || r.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
        !is_dir && r.left.is_some() && r.right.is_some()
    });
    let has_tool = app.settings.external_diff_tool.is_some();
    let can_compare = is_file_pair && has_tool;

    let is_file_active = row.is_some_and(|r| {
        if app.active_side_left {
            r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
        } else {
            r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
        }
    });

    let mut items = Vec::new();
    for (i, item) in app.context_menu.items.iter().enumerate() {
        let mut style = if i == app.context_menu.selected_idx {
            Style::default().bg(Color::Blue).fg(Color::White)
        } else {
            Style::default()
        };

        if i == 0 && !can_compare {
            style = style.fg(Color::DarkGray);
        }
        if i == 1 && !is_file_active {
            style = style.fg(Color::DarkGray);
        }
        items.push(ListItem::new(item.as_str()).style(style));
    }

    let block = Block::default()
        .title(" Actions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

pub fn draw_confirm_modal(f: &mut Frame, app: &mut App) {
    let area = centered_rect(60, 7, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Confirm Action ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let text = vec![
        Line::from(""),
        Line::from(Span::raw(&app.confirm_modal_message)).alignment(Alignment::Center),
        Line::from(""),
        Line::from(Span::styled(
            " [Y] Yes   [N] No (Cancel) ",
            Style::default().fg(Color::Cyan),
        ))
        .alignment(Alignment::Center),
    ];

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    #[test]
    fn test_ui_drawing() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);
        println!("Buffer output:\n{:?}", buffer);

        assert!(
            buffer_string.contains("Left: /left"),
            "Buffer should contain 'Left: /left'"
        );
        assert!(
            buffer_string.contains("Right: /right"),
            "Buffer should contain 'Right: /right'"
        );
        // The State column title was removed; verify indicator symbols render
        assert!(
            !buffer_string.contains("\"State\""),
            "State column title should be removed"
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

        let detail = selected_row_detail(Some(&row));
        assert!(detail.is_some());
        let detail = detail.unwrap();
        assert!(
            detail.contains("(newer)"),
            "Should contain '(newer)': {}",
            detail
        );
        assert!(
            detail.contains("Left:"),
            "Should contain 'Left:': {}",
            detail
        );
        assert!(
            detail.contains("Right:"),
            "Should contain 'Right:': {}",
            detail
        );
        assert!(
            detail.contains("2.0 KB"),
            "Should show left size: {}",
            detail
        );
        assert!(
            detail.contains("1.0 KB"),
            "Should show right size: {}",
            detail
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

        let detail = selected_row_detail(Some(&row)).unwrap();
        assert!(
            detail.contains("(newer)"),
            "Should contain '(newer)': {}",
            detail
        );
        // The right side should be tagged as newer, not the left
        let right_part = detail.split("Right:").nth(1).unwrap();
        assert!(
            right_part.contains("(newer)"),
            "Right side should be newer: {}",
            detail
        );
        let left_part = detail.split("Right:").next().unwrap();
        assert!(
            !left_part.contains("(newer)"),
            "Left side should NOT be newer: {}",
            detail
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

        let detail = selected_row_detail(Some(&row)).unwrap();
        assert!(
            !detail.contains("(newer)"),
            "Same time should not mark either side as newer: {}",
            detail
        );
        assert!(detail.contains("Left:"), "Should contain 'Left:'");
        assert!(detail.contains("Right:"), "Should contain 'Right:'");
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

        let detail = selected_row_detail(Some(&row)).unwrap();
        // Directories should not show file sizes
        assert!(
            !detail.contains("KB") && !detail.contains("MB"),
            "Directory detail should not show size: {}",
            detail
        );
        assert!(detail.contains("(newer)"), "Should mark left as newer");
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

        terminal.draw(|f| draw(f, &mut app)).unwrap();

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
        app.flat_rows.push(FlatRow {
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
        app.selected_idx = 0;

        terminal.draw(|f| draw(f, &mut app)).unwrap();

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
        app.flat_rows.push(FlatRow {
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
        app.selected_idx = 0;
        app.view_mode = ViewMode::FileDiff;

        // diff_rows with only Equal tags → files are identical
        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "hello".to_string(),
            }),
        ))];

        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let buffer_string = format!("{:?}", buffer);

        // Should show full paths for both sides in pane titles
        assert!(
            buffer_string.contains("/left/same.txt"),
            "Diff view should show left full path in title: {}",
            buffer_string
        );
        assert!(
            buffer_string.contains("/right/same.txt"),
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
    fn test_diff_view_no_identical_notice_when_files_differ() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.flat_rows.push(FlatRow {
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
        app.selected_idx = 0;
        app.view_mode = ViewMode::FileDiff;

        // diff_rows with a Delete tag → files differ
        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old line".to_string(),
            }),
            None,
        ))];

        terminal.draw(|f| draw(f, &mut app)).unwrap();

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

        app.flat_rows.push(FlatRow {
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
        app.selected_idx = 0;
        app.view_mode = ViewMode::FileDiff;
        app.diff_left_hash = Some("aabbccdd11223344".to_string());
        app.diff_right_hash = Some("eeff001122334455".to_string());

        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "old".to_string(),
            }),
            None,
        ))];

        terminal.draw(|f| draw(f, &mut app)).unwrap();

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
        let title = build_diff_pane_title("Left", &long_path, Some(&SystemTime::UNIX_EPOCH), 40);
        assert!(
            title.starts_with(" Left: "),
            "Title should start with ' Left: '"
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
        let title = build_diff_pane_title("Left", &short_path, Some(&SystemTime::UNIX_EPOCH), 80);
        assert!(
            title.contains("/left/file.txt"),
            "Short path should not be truncated: {}",
            title
        );
        assert!(
            title.contains("ago"),
            "Title should contain relative time: {}",
            title
        );
    }
}
