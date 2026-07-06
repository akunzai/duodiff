use ratatui::{
    prelude::*,
    widgets::*,
};
use crate::app::App;
use crate::diff::DiffState;

use crate::app::ViewMode;

pub fn draw(f: &mut Frame, app: &mut App) {
    match app.view_mode {
        ViewMode::DirectoryTree => draw_tree(f, app),
        ViewMode::FileDiff => draw_diff(f, app),
    }
}

pub fn draw_tree(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Body (Left / Indicator / Right)
            Constraint::Length(2), // Footer
        ])
        .split(f.size());

    // Draw Header
    let header_text = vec![
        Line::from(vec![
            Span::styled("Left: ", Style::default().bold()),
            Span::raw(format!("{:?}   ", app.left_path)),
            Span::styled("Right: ", Style::default().bold()),
            Span::raw(format!("{:?}", app.right_path)),
        ]),
        Line::from(vec![
            Span::raw("Mode: "),
            Span::styled(
                if app.precise_mode { "Precise (MD5)" } else { "Fast (Size & Time)" },
                Style::default().fg(Color::Cyan).bold(),
            ),
        ]),
    ];
    let header_paragraph = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header_paragraph, chunks[0]);

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

    for (i, row) in app.flat_rows.iter().enumerate().skip(app.scroll_offset).take(visible_height) {
        let is_selected = i == app.selected_idx;
        let style = if is_selected {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        } else {
            match row.state {
                DiffState::Identical => Style::default().fg(Color::Gray),
                DiffState::DifferentNewerLeft | DiffState::DifferentNewerRight | DiffState::DifferentSameTime => Style::default().fg(Color::Yellow),
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

    let left_list = List::new(left_items)
        .block(Block::default().title("Left Pane").borders(Borders::ALL));
    let indicator_list = List::new(indicator_items)
        .block(Block::default().title("State").borders(Borders::ALL));
    let right_list = List::new(right_items)
        .block(Block::default().title("Right Pane").borders(Borders::ALL));

    f.render_widget(left_list, body_chunks[0]);
    f.render_widget(indicator_list, body_chunks[1]);
    f.render_widget(right_list, body_chunks[2]);

    // Draw Footer
    let footer_txt = if app.scan_in_progress {
        "Scanning in progress... Please wait."
    } else {
        "q:Quit | Tab:Focus Side | Space:Expand | Enter:Diff | c:Mode | r:Refresh"
    };
    let footer_p = Paragraph::new(footer_txt)
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer_p, chunks[2]);
}

pub fn draw_diff(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(f.size());

    let header = Paragraph::new("File Comparison View - Esc/q to return")
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
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
                left_lines.push(Line::from(Span::styled(line.text.trim_end().to_string(), style)));
            } else {
                left_lines.push(Line::from(""));
            }

            if let Some(line) = right_line {
                let style = match line.tag {
                    similar::ChangeTag::Insert => Style::default().fg(Color::Green),
                    _ => Style::default().fg(Color::Gray),
                };
                right_lines.push(Line::from(Span::styled(line.text.trim_end().to_string(), style)));
            } else {
                right_lines.push(Line::from(""));
            }
        }

        let left_p = Paragraph::new(left_lines).block(Block::default().title("Left File").borders(Borders::ALL));
        let right_p = Paragraph::new(right_lines).block(Block::default().title("Right File").borders(Borders::ALL));

        f.render_widget(left_p, body_chunks[0]);
        f.render_widget(right_p, body_chunks[1]);
    }

    let footer_p = Paragraph::new("Esc/q: Back | j/↓: Scroll Down | k/↑: Scroll Up")
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer_p, chunks[2]);
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
        
        assert!(buffer_string.contains("Left Pane"), "Buffer should contain 'Left Pane'");
        assert!(buffer_string.contains("Right Pane"), "Buffer should contain 'Right Pane'");
        assert!(buffer_string.contains("State"), "Buffer should contain 'State'");
    }
}
