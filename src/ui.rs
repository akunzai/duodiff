use crate::app::App;
use crate::diff::DiffState;
use ratatui::{prelude::*, widgets::*};

use crate::app::ViewMode;

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
    let footer_height = if app.status_message.is_some() { 3 } else { 2 };
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
            Constraint::Percentage(47), // Left
            Constraint::Percentage(6),  // Indicator
            Constraint::Percentage(47), // Right
        ])
        .split(chunks[1]);

    let visible_height = body_chunks[0].height.saturating_sub(2) as usize;
    app.visible_height = visible_height;
    app.adjust_scroll(visible_height);

    let mut left_items = Vec::new();
    let mut indicator_items = Vec::new();
    let mut right_items = Vec::new();

    for (i, row) in app
        .flat_rows
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
            DiffState::DifferentNewerLeft => " ≠ (L)",
            DiffState::DifferentNewerRight => " ≠ (R)",
            DiffState::DifferentSameTime => " ≠",
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

    let indicator_list =
        List::new(indicator_items).block(Block::default().title("State").borders(Borders::ALL));

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
    let row = app.flat_rows.get(app.selected_idx);
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
        btns.push_str(" | c:Mode | r:Refresh");
        btns
    };
    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(Color::Red).bold()
        } else {
            Style::default().fg(Color::Green).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        let lines = vec![
            Line::from(Span::styled(format!("{}{}", icon, msg), status_style)),
            Line::from(footer_txt),
        ];
        let footer_p = Paragraph::new(lines).block(Block::default().borders(Borders::TOP));
        f.render_widget(footer_p, chunks[2]);
    } else {
        let footer_p = Paragraph::new(footer_txt).block(Block::default().borders(Borders::TOP));
        f.render_widget(footer_p, chunks[2]);
    }
}

pub fn draw_diff(f: &mut Frame, app: &mut App) {
    let footer_height = if app.status_message.is_some() { 3 } else { 2 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(footer_height),
        ])
        .split(f.area());

    let header = Paragraph::new("File Comparison View - Esc/q to return")
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let max_visible = chunks[1].height.saturating_sub(2) as usize;
    app.visible_height = max_visible;

    if app.selected_idx < app.flat_rows.len() {
        let mut left_lines = Vec::new();
        let mut right_lines = Vec::new();

        // Simple paginated scroll index
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

        let file_name = app.flat_rows[app.selected_idx]
            .relative_path
            .to_string_lossy();
        let left_title = format!(" Left: {} ", file_name);
        let right_title = format!(" Right: {} ", file_name);

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
    if app.selected_idx < app.flat_rows.len() {
        let row = &app.flat_rows[app.selected_idx];
        if row.right.is_some() {
            footer_text.push_str(" | L:←Copy");
        }
        if row.left.is_some() {
            footer_text.push_str(" | R:Copy→");
        }
    }

    if let Some((msg, is_error, _)) = &app.status_message {
        let status_style = if *is_error {
            Style::default().fg(Color::Red).bold()
        } else {
            Style::default().fg(Color::Green).bold()
        };
        let icon = if *is_error { "✗ " } else { "✓ " };
        let lines = vec![
            Line::from(Span::styled(format!("{}{}", icon, msg), status_style)),
            Line::from(footer_text),
        ];
        let footer_p = Paragraph::new(lines).block(Block::default().borders(Borders::TOP));
        f.render_widget(footer_p, chunks[2]);
    } else {
        let footer_p = Paragraph::new(footer_text).block(Block::default().borders(Borders::TOP));
        f.render_widget(footer_p, chunks[2]);
    }
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
        assert!(
            buffer_string.contains("State"),
            "Buffer should contain 'State'"
        );
    }
}
