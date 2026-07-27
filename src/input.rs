//! Keyboard and mouse input routing for the TUI event loop.
use crate::actions::{
    dispatch_key_outcome, execute_confirm_action, execute_palette_action, kick_scan, open_repo_url,
};
use crate::app::{self, App};
use crate::event::AppEvent;
use crate::key_outcome::{diff_launch_outcome, editor_launch_outcome};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::Terminal;

/// Handle a key press. Returns `Ok(true)` if the event loop should quit.
pub async fn handle_key<B: ratatui::backend::Backend>(
    key: KeyEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<bool, Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    // Confirm modal traps all input until dismissed — checked before every other
    // shortcut (including the command palette and theme toggle below) so it behaves
    // identically regardless of which ViewMode it was opened from. Mirrors
    // handle_mouse, which checks `confirm_modal` first for the same reason.
    if app.confirm_modal().is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                execute_confirm_action(app, tx.clone()).await?;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                app.dismiss_confirm();
            }
            _ => {}
        }
        return Ok(false);
    }

    if app.palette_visible() {
        match key.code {
            KeyCode::Esc => {
                app.close_palette();
            }
            KeyCode::Char('q') if app.palette().mode == Some(app::PaletteMode::Menu) => {
                app.hide_palette();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.palette_select_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.palette_select_prev();
            }
            KeyCode::Enter => {
                if app.palette().selected_idx < app.palette().items.len() {
                    let action = app.palette().items[app.palette().selected_idx].clone();
                    if action.enabled {
                        app.close_palette();
                        execute_palette_action(&action, app, terminal, tx.clone()).await?;
                    }
                }
            }
            KeyCode::Backspace => {
                if app.palette().mode == Some(app::PaletteMode::Command) {
                    app.palette_backspace();
                }
            }
            KeyCode::Char(c) => {
                if app.palette().mode == Some(app::PaletteMode::Command) {
                    app.palette_type_char(c);
                } else if let Some(pos) = app
                    .palette()
                    .items
                    .iter()
                    .position(|a| a.key.to_lowercase() == c.to_string().to_lowercase())
                {
                    let action = app.palette().items[pos].clone();
                    if action.enabled {
                        app.hide_palette();
                        execute_palette_action(&action, app, terminal, tx.clone()).await?;
                    }
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    // Global theme toggle: available from every screen except while typing into the
    // filter bar (so `T` can still be typed as a filter character).
    if key.code == KeyCode::Char('T') && !app.filter_active() {
        app.toggle_theme();
        return Ok(false);
    }

    if key.code == KeyCode::Char(';') {
        app.open_palette_menu();
        return Ok(false);
    }
    if key.code == KeyCode::Char('p')
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        app.open_palette_command();
        return Ok(false);
    }

    match app.view_mode() {
        app::ViewMode::DirectoryTree => {
            if app.filter_active() {
                match key.code {
                    KeyCode::Esc => {
                        app.cancel_filter();
                    }
                    KeyCode::Enter => {
                        app.commit_filter();
                    }
                    KeyCode::Char('f') => {
                        app.toggle_diffs_only();
                    }
                    _ => {
                        app.filter_input_mut().apply_edit(key.code);
                    }
                }
            } else {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
                    KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                    KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                    KeyCode::Char('f')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.page_down();
                    }
                    KeyCode::Char('b')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.page_up();
                    }
                    KeyCode::Char(' ') => app.toggle_expand(),
                    KeyCode::Char('h') | KeyCode::Left => app.collapse_selected(),
                    KeyCode::Char('l') | KeyCode::Right => app.expand_selected(),
                    KeyCode::Tab => app.toggle_active_side(),
                    KeyCode::Char('1') => {
                        app.focus_left_pane();
                    }
                    KeyCode::Char('2') => {
                        app.focus_right_pane();
                    }
                    KeyCode::Char('c') => {
                        app.toggle_precise_mode();
                        kick_scan(app, tx.clone());
                    }
                    KeyCode::Char('r') => {
                        kick_scan(app, tx.clone());
                    }
                    KeyCode::Char('s') => {
                        app.swap_paths();
                        app.set_status("Swapped left ↔ right", false);
                        kick_scan(app, tx.clone());
                    }
                    KeyCode::Char('C') => {
                        app.open_config();
                    }
                    KeyCode::Char('/') => {
                        app.open_filter();
                    }
                    KeyCode::Char('?') => {
                        app.open_help();
                    }
                    KeyCode::Backspace
                        if !app.filter_pattern().is_empty() || app.filter_diffs_only() =>
                    {
                        app.clear_filter();
                    }
                    KeyCode::Char('L') if app.selected_row().is_some() => {
                        let row = app.selected_row().unwrap();
                        if row.right.is_some() {
                            app.request_confirm(
                                format!("Copy '{}' to left side?", row.name),
                                app::ConfirmAction::CopyRightToLeft,
                            );
                        }
                    }
                    KeyCode::Char('R') if app.selected_row().is_some() => {
                        let row = app.selected_row().unwrap();
                        if row.left.is_some() {
                            app.request_confirm(
                                format!("Copy '{}' to right side?", row.name),
                                app::ConfirmAction::CopyLeftToRight,
                            );
                        }
                    }
                    KeyCode::Char('D') if app.selected_row().is_some() => {
                        dispatch_key_outcome(
                            diff_launch_outcome(app),
                            terminal,
                            app.mouse_enabled,
                        )?;
                    }
                    KeyCode::Char('E') if app.selected_row().is_some() => {
                        dispatch_key_outcome(
                            editor_launch_outcome(app),
                            terminal,
                            app.mouse_enabled,
                        )?;
                    }
                    KeyCode::Enter if app.selected_row().is_some() => {
                        let row = app.selected_row().unwrap();
                        let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                            || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                        if is_dir {
                            app.toggle_expand();
                        } else {
                            app.enter_file_diff();
                        }
                    }
                    _ => {}
                }
            }
        }
        app::ViewMode::FileDiff => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                app.leave_file_diff();
            }
            KeyCode::Down if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                app.jump_to_next_change();
            }
            KeyCode::Up if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                app.jump_to_prev_change();
            }
            KeyCode::Char('N') => {
                app.jump_to_next_change();
            }
            KeyCode::Char('P') => {
                app.jump_to_prev_change();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                app.diff_scroll_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.diff_scroll_up();
            }
            KeyCode::Char('f')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.diff_page_down();
            }
            KeyCode::Char('b')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.diff_page_up();
            }
            KeyCode::Left => {
                app.diff_h_scroll_left();
            }
            KeyCode::Right => {
                app.diff_h_scroll_right();
            }
            KeyCode::Char('L') | KeyCode::Char('l') if app.selected_row().is_some() => {
                let row = app.selected_row().unwrap();
                if row.right.is_some() {
                    app.request_confirm(
                        format!("Copy '{}' to left side?", row.name),
                        app::ConfirmAction::CopyRightToLeft,
                    );
                }
            }
            KeyCode::Char('R') | KeyCode::Char('r') if app.selected_row().is_some() => {
                let row = app.selected_row().unwrap();
                if row.left.is_some() {
                    app.request_confirm(
                        format!("Copy '{}' to right side?", row.name),
                        app::ConfirmAction::CopyLeftToRight,
                    );
                }
            }
            KeyCode::Char('[') => {
                match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::RightToLeft) {
                    Ok(()) => app.set_status("Copied change block to left".to_string(), false),
                    Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
                }
            }
            KeyCode::Char(']') => {
                match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight) {
                    Ok(()) => app.set_status("Copied change block to right".to_string(), false),
                    Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
                }
            }
            KeyCode::Char('w') => {
                app.toggle_diff_wrap();
            }
            KeyCode::Char('?') => {
                app.open_help();
            }
            KeyCode::Char('C') => {
                app.open_config();
            }
            KeyCode::Char('f') => {
                if let Err(e) = app.toggle_diff_show_full() {
                    app.set_status(format!("Cannot refresh diff: {e}"), true);
                }
            }
            _ => {}
        },
        app::ViewMode::ConfigMenu => match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.close_config(),
            KeyCode::Char('j') | KeyCode::Down => {
                app.config_select_next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.config_select_prev();
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                app.apply_config_selection();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                app.adjust_config_selection(false);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                app.adjust_config_selection(true);
            }
            KeyCode::Char('?') => {
                app.open_help();
            }
            _ => {}
        },
        app::ViewMode::Help => {
            if app.help_index_open() {
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.help_index_select_next();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.help_index_select_prev();
                    }
                    KeyCode::Enter => {
                        app.select_help_topic(app::HelpTopic::all()[app.help_index_sel()]);
                    }
                    KeyCode::Char(c @ '1'..='6') => {
                        app.select_help_topic(app::HelpTopic::all()[(c as u8 - b'1') as usize]);
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        app.close_help();
                    }
                    KeyCode::Char('C') => {
                        app.open_config();
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Char(c @ '1'..='6') => {
                        app.select_help_topic(app::HelpTopic::all()[(c as u8 - b'1') as usize]);
                    }
                    KeyCode::Tab => {
                        app.open_help_index();
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.help_scroll_down();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.help_scroll_up();
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                        app.close_help();
                    }
                    KeyCode::Char('C') => {
                        app.open_config();
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(false)
}

/// Handle a mouse event.
pub async fn handle_mouse<B: ratatui::backend::Backend>(
    mouse: MouseEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    // Confirm modal traps all mouse input until dismissed — checked before every
    // other hit-test (including the top bar and view-mode-specific buttons below)
    // so it behaves identically regardless of which ViewMode it was opened from.
    // Mirrors handle_key, which checks `confirm_modal` first for the same reason.
    if app.confirm_modal().is_some() {
        if let MouseEventKind::Down(crossterm::event::MouseButton::Left) = mouse.kind {
            if let Ok(size) = terminal.size() {
                let size_rect = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                let modal_area = crate::ui::centered_rect(60, 7, size_rect);
                if mouse.row == modal_area.y
                    && mouse.column >= modal_area.x + modal_area.width.saturating_sub(5)
                    && mouse.column < modal_area.x + modal_area.width.saturating_sub(2)
                {
                    app.dismiss_confirm();
                }
            }
        }
        return Ok(());
    }
    if let MouseEventKind::Down(crossterm::event::MouseButton::Left) = mouse.kind {
        if mouse.row == 0 {
            if let Ok(size) = terminal.size() {
                let w = size.width;
                if mouse.column >= w.saturating_sub(17) && mouse.column < w.saturating_sub(9) {
                    app.close_palette();
                    app.open_config();
                    return Ok(());
                } else if mouse.column >= w.saturating_sub(7) {
                    app.close_palette();
                    app.open_help();
                    return Ok(());
                }
            }
        } else if app.palette_visible() {
            if let Ok(size) = terminal.size() {
                let mode = app.palette().mode.unwrap_or(app::PaletteMode::Menu);
                let count = app.palette().items.len();
                let size_rect = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                let popup = crate::ui::palette_popup_rect(mode, count, size_rect);
                let menu_x = popup.x;
                let menu_y = popup.y;
                let pop_w = popup.width;
                let pop_h = popup.height;

                if mouse.column >= menu_x
                    && mouse.column < menu_x + pop_w
                    && mouse.row >= menu_y
                    && mouse.row < menu_y + pop_h
                {
                    // Check close button [x]
                    if mouse.row == menu_y
                        && mouse.column >= menu_x + pop_w.saturating_sub(5)
                        && mouse.column < menu_x + pop_w.saturating_sub(2)
                    {
                        app.close_palette();
                        return Ok(());
                    }

                    let list_start_y = match mode {
                        app::PaletteMode::Menu => menu_y + 1,
                        app::PaletteMode::Command => menu_y + 3,
                    };
                    if mouse.row >= list_start_y && mouse.row < menu_y + pop_h - 1 {
                        let click_idx = (mouse.row - list_start_y) as usize;
                        if click_idx < app.palette().items.len() {
                            let action = app.palette().items[click_idx].clone();
                            if action.enabled {
                                app.close_palette();
                                execute_palette_action(&action, app, terminal, tx.clone()).await?;
                            }
                        }
                    }
                } else {
                    app.close_palette();
                }
            }
            return Ok(());
        } else {
            if let Ok(size) = terminal.size() {
                if app.view_mode() == app::ViewMode::Help {
                    if mouse.row == 1
                        && mouse.column >= size.width.saturating_sub(5)
                        && mouse.column < size.width.saturating_sub(2)
                    {
                        app.close_help();
                        return Ok(());
                    }
                } else if app.view_mode() == app::ViewMode::ConfigMenu {
                    if mouse.row == 1
                        && mouse.column >= size.width.saturating_sub(5)
                        && mouse.column < size.width.saturating_sub(2)
                    {
                        app.close_config();
                        return Ok(());
                    }
                } else if app.view_mode() == app::ViewMode::FileDiff {
                    let row = app.selected_row();
                    let has_changes = app.diff_has_changes();
                    let show_identical =
                        !has_changes && row.is_some_and(|r| r.left.is_some() || r.right.is_some());
                    let header_height = if show_identical { 2 } else { 1 };
                    let body_y = header_height + 1;
                    if mouse.row == body_y as u16
                        && mouse.column >= size.width.saturating_sub(5)
                        && mouse.column < size.width.saturating_sub(2)
                    {
                        app.leave_file_diff();
                        return Ok(());
                    }
                }
            }
        }
    }
    if app.palette_visible() {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.palette_select_next();
                return Ok(());
            }
            MouseEventKind::ScrollUp => {
                app.palette_select_prev();
                return Ok(());
            }
            _ => {}
        }
    }
    match app.view_mode() {
        app::ViewMode::DirectoryTree => match mouse.kind {
            MouseEventKind::ScrollDown => app.select_next(),
            MouseEventKind::ScrollUp => app.select_prev(),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let offset_y = click_y - 2;
                    if offset_y < app.viewport().visible_height {
                        let idx = app.scroll_offset() + offset_y;
                        if app.select_row_at(idx) && app.note_tree_click(idx) {
                            let row = app.selected_row().unwrap();
                            let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                                || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                            if is_dir {
                                app.toggle_expand();
                            } else {
                                app.enter_file_diff();
                            }
                        }
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let offset_y = click_y - 2;
                    if offset_y < app.viewport().visible_height {
                        let idx = app.scroll_offset() + offset_y;
                        if app.select_row_at(idx) {
                            app.open_palette_menu();
                        }
                    }
                }
            }
            _ => {}
        },
        app::ViewMode::FileDiff => match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.diff_scroll_down();
            }
            MouseEventKind::ScrollUp => {
                app.diff_scroll_up();
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette_menu();
            }
            _ => {}
        },
        app::ViewMode::ConfigMenu => match mouse.kind {
            MouseEventKind::ScrollDown => {
                let rows = app.config_rows();
                if matches!(
                    rows.get(app.config_selected_idx()),
                    Some(app::ConfigRowKind::DiffContext)
                ) {
                    app.adjust_config_selection(false);
                } else {
                    app.config_select_next();
                }
            }
            MouseEventKind::ScrollUp => {
                let rows = app.config_rows();
                if matches!(
                    rows.get(app.config_selected_idx()),
                    Some(app::ConfigRowKind::DiffContext)
                ) {
                    app.adjust_config_selection(true);
                } else {
                    app.config_select_prev();
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let row_idx = click_y - 2;
                    if app.config_select_at(row_idx) {
                        app.apply_config_selection();
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette_menu();
            }
            _ => {}
        },
        app::ViewMode::Help => match mouse.kind {
            MouseEventKind::ScrollDown => {
                if app.help_index_open() {
                    app.help_index_select_next();
                } else {
                    app.help_scroll_down();
                }
            }
            MouseEventKind::ScrollUp => {
                if app.help_index_open() {
                    app.help_index_select_prev();
                } else {
                    app.help_scroll_up();
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if app.help_index_open() {
                    let click_y = mouse.row as usize;
                    if click_y >= 2 && click_y < 2 + crate::app::HelpTopic::all().len() {
                        let idx = click_y - 2;
                        app.select_help_topic(crate::app::HelpTopic::all()[idx]);
                    }
                } else if app.help_topic() == app::HelpTopic::About {
                    // Help body starts at screen row 2 (top bar + border); the repo-URL line
                    // sits at `ABOUT_REPO_LINE` within the (possibly scrolled) body content.
                    if let Some(visible_row) =
                        crate::ui::ABOUT_REPO_LINE.checked_sub(app.help_scroll())
                    {
                        if mouse.row == 2 + visible_row && mouse.column >= 3 {
                            open_repo_url(app);
                        }
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette_menu();
            }
            _ => {}
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_filter_bar_edits_cjk_text_by_char_not_byte() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_filter();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        for c in "你好".chars() {
            handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(c),
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }
        assert_eq!(app.filter_input(), "你好");

        // Backspace must remove the whole trailing CJK char, not one UTF-8 byte.
        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Backspace,
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert_eq!(app.filter_input(), "你");
    }

    #[tokio::test]
    async fn test_theme_toggle_key_from_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let quit = handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(!quit);
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Dark);

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Light);
    }

    #[tokio::test]
    async fn test_theme_toggle_key_ignored_while_filtering() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.settings.theme = crate::theme::ThemeChoice::Dark;
        app.open_filter();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('T'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();

        // 'T' should be typed into the filter input, not toggle the theme (and, since
        // no toggle happened, nothing was persisted to the shared config file either).
        assert_eq!(app.settings.theme, crate::theme::ThemeChoice::Dark);
        assert_eq!(app.filter_input(), "T");
    }

    #[tokio::test]
    async fn test_config_menu_mouse_scroll_navigates_and_adjusts_diff_context() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(app::ViewMode::ConfigMenu);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Row layout depends on which external diff tools are detected on the test
        // machine's $PATH, so look up positions rather than hardcoding indices.
        let rows = app.config_rows();
        let mouse_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::Mouse))
            .unwrap();
        let theme_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::Theme))
            .unwrap();
        let diff_context_idx = rows
            .iter()
            .position(|r| matches!(r, app::ConfigRowKind::DiffContext))
            .unwrap();

        app.set_config_selected_idx(mouse_idx);
        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.config_selected_idx(),
            theme_idx,
            "scroll down navigates to next selectable row"
        );

        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.config_selected_idx(),
            mouse_idx,
            "scroll up navigates to previous selectable row"
        );

        // On the Diff context row, scroll adjusts the value instead of navigating.
        app.set_config_selected_idx(diff_context_idx);
        assert_eq!(app.settings.diff_context, 7);
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.settings.diff_context, 8,
            "scroll up increases diff context"
        );
        assert_eq!(
            app.config_selected_idx(),
            diff_context_idx,
            "diff context row stays selected"
        );

        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        handle_mouse(scroll_down, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.settings.diff_context, 6,
            "scroll down decreases diff context"
        );
    }

    #[tokio::test]
    async fn test_help_mouse_scroll_moves_topic_body_and_index_selection() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(app::ViewMode::Help);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        app.set_help_index_open(false);
        app.set_help_scroll(0);
        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help_scroll(), 1, "scroll down advances the topic body");
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help_scroll(), 0, "scroll up rewinds the topic body");
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help_scroll(),
            0,
            "scroll up saturates at 0, no underflow"
        );

        app.set_help_index_open(true);
        app.set_help_index_sel(0);
        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel(),
            1,
            "scroll down moves the index selection"
        );
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel(),
            0,
            "scroll up moves the index selection back"
        );
        handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.help_index_sel(),
            app::HelpTopic::all().len() - 1,
            "scroll up wraps to the last topic"
        );
    }

    #[tokio::test]
    async fn test_palette_mouse_scroll_navigates_items_without_leaking_to_background() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_flat_rows(vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: crate::diff::DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: crate::diff::DiffState::DifferentNewerLeft,
                left: None,
                right: None,
            },
        ]);
        app.apply_filter();
        app.set_selected_idx(0);
        app.open_palette_menu();
        app.set_palette_items(vec![
            app::PaletteAction {
                key: "a".to_string(),
                label: "Action A".to_string(),
                action_id: "a",
                enabled: true,
            },
            app::PaletteAction {
                key: "b".to_string(),
                label: "Action B".to_string(),
                action_id: "b",
                enabled: true,
            },
        ]);
        app.set_palette_selected_idx(0);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let scroll_down = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let scroll_up = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };

        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.palette().selected_idx,
            1,
            "scroll down navigates palette items"
        );
        assert_eq!(
            app.selected_idx(),
            0,
            "scroll must not leak through to the background directory tree"
        );

        handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.palette().selected_idx,
            0,
            "scroll up navigates palette items back"
        );
        assert_eq!(
            app.selected_idx(),
            0,
            "scroll must not leak through to the background directory tree"
        );
    }

    #[tokio::test]
    async fn test_diff_right_arrow_clamps_to_synced_viewport_width_not_terminal_size() {
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        use ratatui::Terminal;
        use similar::ChangeTag;

        // The TestBackend is much wider than the viewport synced below, so the
        // old `terminal.size().width / 2` formula and the real, layout-derived
        // `diff_content_width` disagree sharply. A regression back to deriving
        // the clamp from terminal size would land far past the value asserted
        // here.
        let backend = TestBackend::new(200, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        app.set_diff_rows(vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "a".repeat(100),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "a".repeat(100),
            }),
        ))]);
        app.sync_viewport(Rect::new(0, 0, 40, 24));
        let expected_max_h_scroll = app.viewport().max_diff_h_scroll();
        assert_ne!(
            expected_max_h_scroll, 0,
            "test setup must produce a non-trivial clamp"
        );

        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        for _ in 0..(expected_max_h_scroll + 5) {
            handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Right,
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }

        assert_eq!(
            app.diff_h_scroll(),
            expected_max_h_scroll,
            "Right-arrow must clamp to the synced viewport width, not the terminal's actual width"
        );
    }

    #[tokio::test]
    async fn test_confirm_modal_interception_identical_across_all_view_modes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        for view_mode in [
            crate::app::ViewMode::DirectoryTree,
            crate::app::ViewMode::FileDiff,
            crate::app::ViewMode::ConfigMenu,
            crate::app::ViewMode::Help,
        ] {
            // 'n'/Esc must dismiss the modal and clear the pending action, rather
            // than falling through to that ViewMode's own Esc handling (e.g.
            // ConfigMenu's Esc normally navigates back to config_return_view).
            let backend = TestBackend::new(80, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);
            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Esc,
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                app.confirm_modal().is_none(),
                "{view_mode:?}: Esc must dismiss the confirm modal and clear the pending action"
            );
            assert_eq!(
                app.view_mode(),
                view_mode,
                "{view_mode:?}: dismissing the modal must not itself change the view mode"
            );

            // 'y' must route through execute_confirm_action (which closes the modal
            // unconditionally) rather than that ViewMode's own 'y' handling. The
            // default app has empty `filtered_rows`, so the confirmed action is a
            // no-op and never touches the filesystem — see `execute_confirm_action`.
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);
            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            handle_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('y'),
                    crossterm::event::KeyModifiers::empty(),
                ),
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                app.confirm_modal().is_none(),
                "{view_mode:?}: 'y' must route through execute_confirm_action"
            );
        }
    }

    #[tokio::test]
    async fn test_confirm_modal_interception_identical_across_all_view_modes_for_mouse() {
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        use ratatui::Terminal;
        use similar::ChangeTag;

        // 80x24 terminal -> centered_rect(60, 7, ...) puts the modal at x=10,
        // y=8, so its close glyph occupies columns 65..68 on row 8 (mirrors
        // draw_close_button's `x + width - 5 .. x + width - 2`).
        let modal_close_glyph = (66u16, 8u16);
        // Top-bar Help button (row 0, columns width-7.. for an 80-wide terminal).
        // Reached by the same `mouse.row == 0` branch regardless of view_mode,
        // so a left click here would flip view_mode to Help if the modal
        // weren't intercepting it first.
        let top_bar_help_button = (75u16, 0u16);

        for view_mode in [
            crate::app::ViewMode::DirectoryTree,
            crate::app::ViewMode::FileDiff,
            crate::app::ViewMode::ConfigMenu,
            crate::app::ViewMode::Help,
        ] {
            // Scroll must be swallowed, not fall through to that ViewMode's own
            // scroll handling (tree selection, diff scroll, config/help scroll).
            let backend = TestBackend::new(80, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);

            // Set up state where a non-intercepted ScrollDown would visibly move
            // something, so the assertion below actually distinguishes
            // "swallowed" from "handled but happened to be a no-op".
            match view_mode {
                crate::app::ViewMode::DirectoryTree => {
                    app.set_flat_rows(vec![
                        crate::app::FlatRow {
                            depth: 0,
                            relative_path: PathBuf::from("a.txt"),
                            name: "a.txt".to_string(),
                            state: crate::diff::DiffState::DifferentNewerLeft,
                            left: None,
                            right: None,
                        },
                        crate::app::FlatRow {
                            depth: 0,
                            relative_path: PathBuf::from("b.txt"),
                            name: "b.txt".to_string(),
                            state: crate::diff::DiffState::DifferentNewerLeft,
                            left: None,
                            right: None,
                        },
                    ]);
                    app.apply_filter();
                    app.set_selected_idx(0);
                }
                crate::app::ViewMode::FileDiff => {
                    app.set_diff_rows(
                        (0..50)
                            .map(|i| {
                                DiffRow::from((
                                    Some(DiffLine {
                                        tag: ChangeTag::Equal,
                                        text: format!("line {i}"),
                                    }),
                                    Some(DiffLine {
                                        tag: ChangeTag::Equal,
                                        text: format!("line {i}"),
                                    }),
                                ))
                            })
                            .collect(),
                    );
                    app.sync_viewport(Rect::new(0, 0, 80, 10));
                    assert_ne!(
                        app.viewport().max_diff_scroll(),
                        0,
                        "test setup must produce a non-trivial vertical clamp"
                    );
                    app.set_diff_scroll(0);
                }
                crate::app::ViewMode::ConfigMenu => {
                    app.ensure_config_selection();
                }
                crate::app::ViewMode::Help => {
                    app.set_help_index_open(false);
                    app.set_help_scroll(0);
                }
            }

            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let before_selected_idx = app.selected_idx();
            let before_diff_scroll = app.diff_scroll();
            let before_config_selected_idx = app.config_selected_idx();
            let before_help_scroll = app.help_scroll();
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            handle_mouse(
                crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::ScrollDown,
                    column: 0,
                    row: 0,
                    modifiers: crossterm::event::KeyModifiers::empty(),
                },
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                app.confirm_modal().is_some(),
                "{view_mode:?}: scroll must not dismiss the confirm modal"
            );
            match view_mode {
                crate::app::ViewMode::DirectoryTree => assert_eq!(
                    app.selected_idx(), before_selected_idx,
                    "{view_mode:?}: scroll while the modal is open must not move the tree selection"
                ),
                crate::app::ViewMode::FileDiff => assert_eq!(
                    app.diff_scroll(), before_diff_scroll,
                    "{view_mode:?}: scroll while the modal is open must not move the diff view"
                ),
                crate::app::ViewMode::ConfigMenu => assert_eq!(
                    app.config_selected_idx(), before_config_selected_idx,
                    "{view_mode:?}: scroll while the modal is open must not move the config selection"
                ),
                crate::app::ViewMode::Help => assert_eq!(
                    app.help_scroll(), before_help_scroll,
                    "{view_mode:?}: scroll while the modal is open must not move the help body"
                ),
            }

            // A left click on the top-bar Help button must be swallowed too, not
            // fall through to the `mouse.row == 0` handling that's checked
            // regardless of view_mode.
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);
            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            handle_mouse(
                crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ),
                    column: top_bar_help_button.0,
                    row: top_bar_help_button.1,
                    modifiers: crossterm::event::KeyModifiers::empty(),
                },
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                app.confirm_modal().is_some(),
                "{view_mode:?}: a top-bar button click must not dismiss the confirm modal"
            );
            assert_eq!(
                app.view_mode(), view_mode,
                "{view_mode:?}: a top-bar button click while the modal is open must not change the view mode"
            );

            // Clicking the [x] close glyph must still dismiss the modal.
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);
            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let (tx, _rx) = tokio::sync::mpsc::channel(8);

            handle_mouse(
                crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::Down(
                        crossterm::event::MouseButton::Left,
                    ),
                    column: modal_close_glyph.0,
                    row: modal_close_glyph.1,
                    modifiers: crossterm::event::KeyModifiers::empty(),
                },
                &mut app,
                &mut terminal,
                tx,
            )
            .await
            .unwrap();

            assert!(
                app.confirm_modal().is_none(),
                "{view_mode:?}: clicking the close glyph must dismiss the confirm modal and clear the pending confirm action"
            );
            assert_eq!(
                app.view_mode(), view_mode,
                "{view_mode:?}: dismissing the modal via the close glyph must not itself change the view mode"
            );
        }
    }

    #[tokio::test]
    async fn test_config_close_button_mouse_click_returns_to_file_diff() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        app.open_config();
        assert_eq!(app.view_mode(), crate::app::ViewMode::ConfigMenu);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Click the [x] close button (top-right, row 1, terminal width 80 -> columns
        // 75..77 per draw_close_button). Distinct from the `Esc`/`q` key path fixed above —
        // this exercises the separate mouse click-detection code in handle_mouse.
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 76,
            row: 1,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        // Must land back on FileDiff, not be stranded on DirectoryTree.
        assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
    }
}
