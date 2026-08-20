//! Keyboard and mouse input routing for the TUI event loop.
use crate::actions::{
    diff_launch_outcome, dispatch_key_outcome, editor_launch_outcome, execute_confirm_action,
    execute_palette_action, kick_scan, open_repo_url,
};
use crate::app::{self, App};
use crate::event::AppEvent;
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

    // The one Command Palette traps input while open: plain characters always
    // edit the query (so `j`, `k`, and `;` are searchable), arrows move the
    // selection, Enter runs the selected action, and there is no single-character
    // immediate execution any more (Issue #239).
    if app.palette_visible() {
        if key.code == KeyCode::Char('p')
            && key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
        {
            // Ctrl+p toggles the open palette closed.
            app.close_palette();
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => {
                app.close_palette();
            }
            KeyCode::Down => {
                app.palette_select_next();
            }
            KeyCode::Up => {
                app.palette_select_prev();
            }
            KeyCode::Enter => {
                if let Some(action) = app.palette().items.get(app.palette().selected_idx).cloned() {
                    match action.disabled_reason {
                        None => {
                            app.close_palette();
                            execute_palette_action(&action, app, terminal, tx.clone()).await?;
                        }
                        // Say why instead of doing nothing. A background rescan can
                        // disable the highlighted row underneath the open palette,
                        // so a silent no-op would look like a broken key.
                        Some(why) => {
                            app.set_status(format!("{}: {why}", action.label), true);
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                app.palette_backspace();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.palette_type_char(c);
            }
            _ => {}
        }
        return Ok(false);
    }

    // Global theme toggle: available from every screen except while typing into the
    // filter bar (so `T` can still be typed as a filter character).
    if key.code == KeyCode::Char('T') && !app.filter().active() {
        app.toggle_theme();
        return Ok(false);
    }

    // Both palette launchers yield to the filter bar, which keeps complete input
    // capture while it is open: `;` must be typeable (Issue #236) and no launcher
    // may interrupt a text editor (Issue #239).
    if !app.filter().active() {
        let ctrl = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        if key.code == KeyCode::Char(';') || (ctrl && key.code == KeyCode::Char('p')) {
            app.open_palette();
            return Ok(false);
        }
    }

    match app.view_mode() {
        app::ViewMode::DirectoryTree => {
            if app.filter().active() {
                match key.code {
                    KeyCode::Esc => {
                        app.filter_mut().cancel();
                    }
                    KeyCode::Enter => {
                        app.commit_filter();
                    }
                    // Diffs-only lives on a modifier chord so that plain `f` — and
                    // every other unmodified printable character — reaches the
                    // query. Filtering for `config`, `footer`, or `Fast` was
                    // otherwise impossible (Issue #236).
                    KeyCode::Char('f')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        app.filter_mut().toggle_diffs_only();
                    }
                    _ => {
                        app.filter_mut().input_mut().apply_edit(key.code);
                    }
                }
            } else {
                match key.code {
                    KeyCode::Char('q') => return Ok(true),
                    // Esc is layered: while a filter is applied it is the natural
                    // "cancel / clear" gesture, so it must clear the filter rather
                    // than fall through to the least reversible action available.
                    // Only with nothing left to dismiss does it quit (Issue #233).
                    KeyCode::Esc
                        if !app.filter().pattern().is_empty() || app.filter().diffs_only() =>
                    {
                        app.clear_filter();
                    }
                    KeyCode::Esc => return Ok(true),
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
                        if app.switch_scan_mode(app.scan_mode().toggled()) {
                            kick_scan(app, tx.clone());
                        }
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
                        app.filter_mut().open();
                    }
                    KeyCode::Char('?') => {
                        app.open_help();
                    }
                    KeyCode::Backspace
                        if !app.filter().pattern().is_empty() || app.filter().diffs_only() =>
                    {
                        app.clear_filter();
                    }
                    KeyCode::Char('L') if app.selected_row().is_some() => {
                        app.request_copy(app::ConfirmAction::CopyRightToLeft);
                    }
                    KeyCode::Char('R') if app.selected_row().is_some() => {
                        app.request_copy(app::ConfirmAction::CopyLeftToRight);
                    }
                    KeyCode::Char('D') if app.selected_row().is_some() => {
                        dispatch_key_outcome(
                            diff_launch_outcome(app),
                            terminal,
                            app.mouse_enabled(),
                        )?;
                    }
                    KeyCode::Char('E') if app.selected_row().is_some() => {
                        dispatch_key_outcome(
                            editor_launch_outcome(app),
                            terminal,
                            app.mouse_enabled(),
                        )?;
                    }
                    KeyCode::Enter if app.selected_row().is_some() => {
                        let row = app.selected_row().unwrap();
                        if row.is_dir() {
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
                app.diff_mut().scroll_up();
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
                app.diff_mut().h_scroll_left();
            }
            KeyCode::Right => {
                app.diff_h_scroll_right();
            }
            // Whole-file overwrite stays on the uppercase keys only. Lowercase `l`/`r`
            // are harmless in the Directory Tree (expand / re-scan), so binding them to
            // a destructive overwrite here turned tree muscle memory into data loss
            // behind a single `y` (Issue #234).
            KeyCode::Char('L') if app.selected_row().is_some() => {
                app.request_copy(app::ConfirmAction::CopyRightToLeft);
            }
            KeyCode::Char('R') if app.selected_row().is_some() => {
                app.request_copy(app::ConfirmAction::CopyLeftToRight);
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
                app.diff_mut().toggle_wrap();
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
            // The palette lists both for File Diff, so they need matching direct
            // bindings here as well as in the Directory Tree (Issue #239).
            KeyCode::Char('D') if app.selected_row().is_some() => {
                dispatch_key_outcome(diff_launch_outcome(app), terminal, app.mouse_enabled())?;
            }
            KeyCode::Char('E') if app.selected_row().is_some() => {
                dispatch_key_outcome(editor_launch_outcome(app), terminal, app.mouse_enabled())?;
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
                if app.apply_config_selection() {
                    kick_scan(app, tx.clone());
                }
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
        app::ViewMode::Help => match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                app.help_mut().move_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                app.help_mut().move_up();
            }
            KeyCode::Char(c @ '1'..='6') => {
                app.help_mut()
                    .select_topic_by_index((c as u8 - b'1') as usize);
            }
            KeyCode::Enter if app.help().index_open() => {
                let idx = app.help().index_sel();
                app.help_mut().select_topic_by_index(idx);
            }
            KeyCode::Tab if !app.help().index_open() => {
                app.help_mut().open_index();
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                app.close_help();
            }
            KeyCode::Char('C') => {
                app.open_config();
            }
            _ => {}
        },
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
                let bar_area = ratatui::prelude::Rect::new(0, 0, size.width, 1);
                let links = crate::ui::top_bar_links(bar_area);
                if links.config.x <= mouse.column
                    && mouse.column < links.config.x + links.config.width
                {
                    app.close_palette();
                    app.open_config();
                    return Ok(());
                } else if links.help.x <= mouse.column
                    && mouse.column < links.help.x + links.help.width
                {
                    app.close_palette();
                    app.open_help();
                    return Ok(());
                }
            }
        } else if app.palette_visible() {
            if let Ok(size) = terminal.size() {
                let frame = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                // Same geometry the renderer used, so a click cannot land on a
                // row painted somewhere else (Issue #239).
                let layout = crate::ui::palette_layout(app.palette().items.len(), frame);
                let popup = layout.popup;

                let inside = mouse.column >= popup.x
                    && mouse.column < popup.x + popup.width
                    && mouse.row >= popup.y
                    && mouse.row < popup.y + popup.height;
                if !inside {
                    app.close_palette();
                    return Ok(());
                }

                if let Some(button) = crate::ui::close_button_rect(popup) {
                    if mouse.row == button.y
                        && mouse.column >= button.x
                        && mouse.column < button.x + button.width
                    {
                        app.close_palette();
                        return Ok(());
                    }
                }

                if mouse.row >= layout.list.y && mouse.row < layout.list.y + layout.list.height {
                    let clicked =
                        app.palette().scroll_offset + (mouse.row - layout.list.y) as usize;
                    if let Some(action) = app.palette().items.get(clicked).cloned() {
                        if action.enabled() {
                            app.close_palette();
                            execute_palette_action(&action, app, terminal, tx.clone()).await?;
                        }
                    }
                }
            }
            return Ok(());
        } else {
            if let Ok(size) = terminal.size() {
                if app.view_mode() == app::ViewMode::Help {
                    // `draw_help_content` paints its close button against the content
                    // chunk (row 1, full width) — read the same rect through
                    // `close_button_rect` here so the two cannot drift apart, same
                    // principle as the FileDiff branch below (fixed via `ui::diff_layout`
                    // in #182).
                    let body_area = ratatui::prelude::Rect::new(0, 1, size.width, 1);
                    if let Some(button) = crate::ui::close_button_rect(body_area) {
                        if mouse.row == button.y
                            && mouse.column >= button.x
                            && mouse.column < button.x + button.width
                        {
                            app.close_help();
                            return Ok(());
                        }
                    }
                } else if app.view_mode() == app::ViewMode::ConfigMenu {
                    // Same shape as the Help branch above — `draw_config_content`
                    // paints its close button against the same row-1, full-width chunk.
                    let body_area = ratatui::prelude::Rect::new(0, 1, size.width, 1);
                    if let Some(button) = crate::ui::close_button_rect(body_area) {
                        if mouse.row == button.y
                            && mouse.column >= button.x
                            && mouse.column < button.x + button.width
                        {
                            app.close_config();
                            return Ok(());
                        }
                    }
                } else if app.view_mode() == app::ViewMode::FileDiff {
                    let size_rect = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                    let inputs = app.diff_layout_inputs();
                    let layout = crate::ui::diff_layout(&inputs, size_rect);
                    // `draw_close_button` paints against `layout.right` (see ui.rs), so the
                    // hit test reads the same rect rather than `layout.left` — both share
                    // the same `y` today (a horizontal split), but `right` is what's true by
                    // construction, not by coincidence.
                    if mouse.row == layout.right.y
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
                            if row.is_dir() {
                                app.toggle_expand();
                            } else {
                                app.enter_file_diff();
                            }
                        }
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                // Select the pointed row first so the inventory is built for it,
                // then open the palette regardless of whether the click landed on
                // a row at all (Issue #239).
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let offset_y = click_y - 2;
                    if offset_y < app.viewport().visible_height {
                        app.select_row_at(app.scroll_offset() + offset_y);
                    }
                }
                app.open_palette();
            }
            _ => {}
        },
        app::ViewMode::FileDiff => match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.diff_scroll_down();
            }
            MouseEventKind::ScrollUp => {
                app.diff_mut().scroll_up();
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette();
            }
            _ => {}
        },
        app::ViewMode::ConfigMenu => match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.config_scroll(true);
            }
            MouseEventKind::ScrollUp => {
                app.config_scroll(false);
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let row_idx = click_y - 2;
                    if app.config_select_at(row_idx) && app.apply_config_selection() {
                        kick_scan(app, tx.clone());
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette();
            }
            _ => {}
        },
        app::ViewMode::Help => match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.help_mut().move_down();
            }
            MouseEventKind::ScrollUp => {
                app.help_mut().move_up();
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if app.help().index_open() {
                    let click_y = mouse.row as usize;
                    if click_y >= 2 {
                        app.help_mut().select_topic_by_index(click_y - 2);
                    }
                } else if app.help().topic() == app::HelpTopic::About {
                    // Help body starts at screen row 2 (top bar + border); the repo-URL line
                    // sits at `ABOUT_REPO_LINE` within the (possibly scrolled) body content.
                    if let Some(visible_row) =
                        crate::ui::ABOUT_REPO_LINE.checked_sub(app.help().scroll())
                    {
                        if mouse.row == 2 + visible_row && mouse.column >= 3 {
                            open_repo_url(app);
                        }
                    }
                }
            }
            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                app.open_palette();
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
        app.filter_mut().open();
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
        assert_eq!(app.filter().input(), "你好");

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
        assert_eq!(app.filter().input(), "你");
    }

    #[tokio::test]
    async fn test_theme_toggle_key_from_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Light);
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
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Dark);

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
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Light);
    }

    #[tokio::test]
    async fn test_theme_toggle_key_ignored_while_filtering() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_theme(crate::theme::ThemeChoice::Dark);
        app.filter_mut().open();
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
        assert_eq!(app.settings().theme, crate::theme::ThemeChoice::Dark);
        assert_eq!(app.filter().input(), "T");
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

        app.config_mut().set_selected_idx(mouse_idx);
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
            app.config().selected_idx(),
            theme_idx,
            "scroll down navigates to next selectable row"
        );

        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.config().selected_idx(),
            mouse_idx,
            "scroll up navigates to previous selectable row"
        );

        // On the Diff context row, scroll adjusts the value instead of navigating.
        app.config_mut().set_selected_idx(diff_context_idx);
        assert_eq!(app.settings().diff_context, 7);
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.settings().diff_context,
            8,
            "scroll up increases diff context"
        );
        assert_eq!(
            app.config().selected_idx(),
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
            app.settings().diff_context,
            6,
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

        app.help_mut().set_index_open(false);
        app.help_mut().set_scroll(0);
        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help().scroll(),
            1,
            "scroll down advances the topic body"
        );
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.help().scroll(), 0, "scroll up rewinds the topic body");
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help().scroll(),
            0,
            "scroll up saturates at 0, no underflow"
        );

        app.help_mut().set_index_open(true);
        app.help_mut().set_index_sel(0);
        handle_mouse(scroll_down, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help().index_sel(),
            1,
            "scroll down moves the index selection"
        );
        handle_mouse(scroll_up, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(
            app.help().index_sel(),
            0,
            "scroll up moves the index selection back"
        );
        handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.help().index_sel(),
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
        app.open_palette();
        app.set_palette_items(vec![
            crate::ui::PaletteAction {
                key: "a".to_string(),
                label: "Action A".to_string(),
                action_id: crate::ui::PaletteActionId::Help,
                disabled_reason: None,
            },
            crate::ui::PaletteAction {
                key: "b".to_string(),
                label: "Action B".to_string(),
                action_id: crate::ui::PaletteActionId::Quit,
                disabled_reason: None,
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
        app.diff_mut().set_rows(vec![DiffRow::from((
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
            app.diff().h_scroll(),
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
            // ConfigMenu's Esc normally navigates back to config().return_view()).
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
            // default app has an empty `filter().rows()`, so the confirmed action is
            // a no-op and never touches the filesystem — see `execute_confirm_action`.
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
                    app.diff_mut().set_rows(
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
                    app.diff_mut().set_scroll(0);
                }
                crate::app::ViewMode::ConfigMenu => {
                    app.ensure_config_selection();
                }
                crate::app::ViewMode::Help => {
                    app.help_mut().set_index_open(false);
                    app.help_mut().set_scroll(0);
                }
            }

            app.request_confirm("prompt", crate::app::ConfirmAction::CopyLeftToRight);
            let before_selected_idx = app.selected_idx();
            let before_diff_scroll = app.diff().scroll();
            let before_config_selected_idx = app.config().selected_idx();
            let before_help_scroll = app.help().scroll();
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
                    app.diff().scroll(), before_diff_scroll,
                    "{view_mode:?}: scroll while the modal is open must not move the diff view"
                ),
                crate::app::ViewMode::ConfigMenu => assert_eq!(
                    app.config().selected_idx(), before_config_selected_idx,
                    "{view_mode:?}: scroll while the modal is open must not move the config selection"
                ),
                crate::app::ViewMode::Help => assert_eq!(
                    app.help().scroll(), before_help_scroll,
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

    #[tokio::test]
    async fn test_topbar_config_link_mouse_click_opens_config() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Read the same rect `top_bar_links` computes rather than hardcoding a
        // column, so this test can't drift from the geometry it's exercising.
        let links = crate::ui::top_bar_links(ratatui::prelude::Rect::new(0, 0, 80, 1));
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: links.config.x,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        assert_eq!(app.view_mode(), crate::app::ViewMode::ConfigMenu);
    }

    #[tokio::test]
    async fn test_topbar_help_link_mouse_click_opens_help() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let links = crate::ui::top_bar_links(ratatui::prelude::Rect::new(0, 0, 80, 1));
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: links.help.x,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        assert_eq!(app.view_mode(), crate::app::ViewMode::Help);
    }

    #[tokio::test]
    async fn test_file_diff_close_button_mouse_click_returns_to_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Click the [x] close button. With no selected row/diff content, the
        // "identical" notice row is absent, so the header collapses to a single
        // row and the close button sits at row 2 (terminal width 80 -> columns
        // 75..77 per draw_close_button). Regression test for #182: the hit test
        // must derive this row from `ui::diff_layout` — the single source of
        // truth shared with `draw_diff`/`App::sync_viewport` — instead of an
        // independent, second copy of the header-height calculation.
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 76,
            row: 2,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_file_diff_close_button_mouse_click_accounts_for_identical_notice_row() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let file_info = crate::diff::FileInfo {
            is_dir: false,
            size: 3,
            modified: SystemTime::UNIX_EPOCH,
        };
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("same.txt"),
            name: "same.txt".to_string(),
            state: crate::diff::DiffState::Identical,
            left: Some(file_info.clone()),
            right: Some(file_info),
        }]);
        app.apply_filter();
        app.set_selected_idx(0);
        app.diff_mut()
            .set_rows(vec![crate::diff_view::DiffRow::from((
                Some(crate::diff_view::DiffLine {
                    tag: similar::ChangeTag::Equal,
                    text: "same".to_string(),
                }),
                Some(crate::diff_view::DiffLine {
                    tag: similar::ChangeTag::Equal,
                    text: "same".to_string(),
                }),
            ))]);
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        // Both sides are present with no differing lines, so `diff_layout` renders
        // the "files are identical" notice, growing the header to 2 rows and
        // pushing the close button down to row 3 instead of row 2. A hard-coded
        // hit-test (rather than one reading `ui::diff_layout`) would miss this.
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 76,
            row: 3,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_mouse(click, &mut app, &mut terminal, tx)
            .await
            .unwrap();

        assert_eq!(app.view_mode(), crate::app::ViewMode::DirectoryTree);
    }

    /// Issue #234: in the Directory Tree `l`/`r` expand and re-scan, so carrying that
    /// muscle memory into File Diff must not arm a whole-file overwrite. Only the
    /// uppercase twins may request a copy.
    #[tokio::test]
    async fn test_file_diff_lowercase_l_and_r_do_not_request_a_whole_file_copy() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let file_info = crate::diff::FileInfo {
            is_dir: false,
            size: 3,
            modified: SystemTime::UNIX_EPOCH,
        };
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("a.txt"),
            name: "a.txt".to_string(),
            state: crate::diff::DiffState::DifferentSameTime,
            left: Some(file_info.clone()),
            right: Some(file_info),
        }]);
        app.apply_filter();
        app.set_selected_idx(0);
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        for c in ['l', 'r'] {
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
            assert!(
                app.confirm_modal().is_none(),
                "lowercase `{c}` must stay unbound in File Diff"
            );
        }

        for (c, expected) in [
            ('L', app::ConfirmAction::CopyRightToLeft),
            ('R', app::ConfirmAction::CopyLeftToRight),
        ] {
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
            assert_eq!(
                app.confirm_modal().map(|m| m.action.clone()),
                Some(expected),
                "uppercase `{c}` must still arm the whole-file copy"
            );
            app.dismiss_confirm();
        }
    }

    /// Issue #233: with a filter applied, `Esc` is the natural "cancel / clear"
    /// gesture, so it must clear the filter instead of quitting the app outright.
    /// With nothing left to dismiss it still quits.
    #[tokio::test]
    async fn test_esc_clears_an_applied_filter_before_it_quits() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );

        // A committed pattern is dismissible, so Esc clears it rather than quitting.
        app.filter_mut().open();
        app.filter_mut().input_mut().set("iis".to_string());
        app.commit_filter();
        assert_eq!(app.filter().pattern(), "iis");

        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!quit, "Esc must not quit while a filter pattern is applied");
        assert!(app.filter().pattern().is_empty());

        // Diffs-only alone is dismissible too.
        app.filter_mut().toggle_diffs_only();
        app.commit_filter();
        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!quit, "Esc must not quit while diffs-only is applied");
        assert!(!app.filter().diffs_only());

        // Nothing left to dismiss — Esc falls through to quit.
        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(quit, "Esc must quit once there is nothing left to dismiss");

        // `q` is unlayered: it quits even with a filter applied.
        app.filter_mut().open();
        app.filter_mut().input_mut().set("iis".to_string());
        app.commit_filter();
        let quit = handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert!(quit, "`q` must still quit directly");
    }

    /// Issue #236: while the filter bar is open, every unmodified printable
    /// character must reach the query — `f` toggled diffs-only and `;` opened the
    /// menu, so filtering for `config`, `footer`, or `Fast` was impossible.
    #[tokio::test]
    async fn test_filter_input_accepts_every_printable_character() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.filter_mut().open();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        for c in "config;F".chars() {
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

        assert_eq!(app.filter().input(), "config;F");
        assert!(
            !app.palette_visible(),
            "`;` must be typed, not open the menu, while the filter bar is open"
        );
        assert!(
            !app.filter().editing_diffs_only(),
            "plain `f`/`F` must not toggle diffs-only any more"
        );
    }

    /// Issue #236: diffs-only moved to `Ctrl+f`, and commits together with the query.
    #[tokio::test]
    async fn test_filter_ctrl_f_drafts_diffs_only_and_commits_with_the_query() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.filter_mut().open();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let ctrl_f = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('f'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        handle_key(ctrl_f, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(
            app.filter().editing_diffs_only(),
            "the badge follows Ctrl+f"
        );
        assert!(
            !app.filter().diffs_only(),
            "nothing is applied until the query is committed"
        );
        assert_eq!(
            app.filter().input(),
            "",
            "Ctrl+f must not leave an `f` in the query"
        );

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(app.filter().diffs_only(), "Enter commits both together");

        // Esc restores the diffs-only value from before the editing session.
        app.filter_mut().open();
        handle_key(ctrl_f, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!app.filter().editing_diffs_only());
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
            app.filter().diffs_only(),
            "Esc restores the committed diffs-only value"
        );
    }

    /// Issue #238: the Directory Tree `c` key runs the one atomic flow — persist,
    /// adopt, and start exactly one background rescan.
    #[tokio::test]
    async fn test_scan_mode_key_persists_and_starts_exactly_one_rescan() {
        use crate::settings::ScanMode;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let _guard = crate::test_support::ConfigEnvGuard::new();
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan_mode(), ScanMode::Precise);
        let before = app.scan_generation();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();

        assert_eq!(app.scan_mode(), ScanMode::Fast);
        assert_eq!(
            crate::settings::AppSettings::load().scan_mode,
            ScanMode::Fast,
            "the new mode is persisted before it takes effect"
        );
        assert_eq!(
            app.scan_generation(),
            before + 1,
            "exactly one background rescan"
        );
    }

    /// Issue #239: every launcher opens the same palette, plain characters always
    /// edit the query, and `Ctrl+p` toggles the open palette closed.
    #[tokio::test]
    async fn test_palette_launchers_and_query_input() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let press = |code, modifiers| crossterm::event::KeyEvent::new(code, modifiers);
        let none = crossterm::event::KeyModifiers::empty();
        let ctrl = crossterm::event::KeyModifiers::CONTROL;

        // `;` opens it.
        handle_key(
            press(KeyCode::Char(';'), none),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(app.palette_visible());

        // Plain characters — including `j`, `k`, and `;` — edit the query rather
        // than navigating or re-launching.
        for c in "j;k".chars() {
            handle_key(
                press(KeyCode::Char(c), none),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }
        assert_eq!(app.palette().query, "j;k");

        // Ctrl+p toggles the open palette closed.
        handle_key(
            press(KeyCode::Char('p'), ctrl),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(!app.palette_visible());

        // Ctrl+p opens the same surface, with the query cleared.
        handle_key(
            press(KeyCode::Char('p'), ctrl),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(app.palette_visible());
        assert!(app.palette().query.is_empty());

        // Esc closes.
        handle_key(press(KeyCode::Esc, none), &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert!(!app.palette_visible());
    }

    /// Issue #239: the filter bar and the confirm modal keep complete input
    /// capture — no launcher may interrupt them.
    #[tokio::test]
    async fn test_palette_launchers_never_interrupt_a_text_input_or_modal() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let ctrl = crossterm::event::KeyModifiers::CONTROL;
        let none = crossterm::event::KeyModifiers::empty();

        app.filter_mut().open();
        for (code, modifiers) in [(KeyCode::Char(';'), none), (KeyCode::Char('p'), ctrl)] {
            handle_key(
                crossterm::event::KeyEvent::new(code, modifiers),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
            assert!(!app.palette_visible(), "the filter bar keeps input capture");
        }
        app.filter_mut().cancel();

        app.request_confirm("Overwrite?", app::ConfirmAction::CopyLeftToRight);
        for (code, modifiers) in [(KeyCode::Char(';'), none), (KeyCode::Char('p'), ctrl)] {
            handle_key(
                crossterm::event::KeyEvent::new(code, modifiers),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
            assert!(!app.palette_visible(), "the modal keeps input capture");
            assert!(app.confirm_modal().is_some());
        }
    }

    /// Issue #239: a right-click in the Directory Tree selects the pointed row
    /// first, so the palette is built for that row.
    #[tokio::test]
    async fn test_right_click_selects_the_pointed_row_before_opening_the_palette() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let info = FileInfo {
            is_dir: false,
            size: 1,
            modified: SystemTime::UNIX_EPOCH,
        };
        app.set_flat_rows(
            ["a.txt", "b.txt", "c.txt"]
                .iter()
                .map(|name| crate::app::FlatRow {
                    depth: 0,
                    relative_path: PathBuf::from(name),
                    name: name.to_string(),
                    state: crate::diff::DiffState::DifferentSameTime,
                    left: Some(info.clone()),
                    right: Some(info.clone()),
                })
                .collect(),
        );
        app.apply_filter();
        app.sync_viewport(ratatui::layout::Rect::new(0, 0, 80, 24));
        app.set_selected_idx(0);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        handle_mouse(
            crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Right),
                column: 10,
                // Row 2 is the first tree row; row 4 is the third.
                row: 4,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();

        assert_eq!(app.selected_idx(), 2, "the pointed row is selected first");
        assert!(app.palette_visible());
        assert!(
            app.palette()
                .items
                .iter()
                .any(|a| a.action_id == crate::ui::PaletteActionId::BuiltinDiff && a.enabled()),
            "the inventory is built for the newly selected file row"
        );
    }

    /// Issue #239: Enter over an unavailable command says why instead of doing
    /// nothing — a background rescan can disable the highlighted row while the
    /// palette is open, and a silent no-op would look like a broken key.
    #[tokio::test]
    async fn test_palette_enter_on_an_unavailable_command_reports_the_reason() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_palette();
        app.set_palette_items(vec![crate::ui::PaletteAction::gated(
            "Enter",
            "Open built-in Diff view",
            crate::ui::PaletteActionId::BuiltinDiff,
            false,
            "no row is selected",
        )]);
        app.set_palette_selected_idx(0);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();

        assert!(app.palette_visible(), "the palette stays open");
        let (msg, is_error) = app.status_toast().unwrap();
        assert!(is_error, "{msg}");
        assert!(msg.contains("no row is selected"), "{msg}");
    }

    /// Issue #239: the mouse wheel drives the same selection and viewport the
    /// keyboard does, on an inventory taller than the popup.
    #[tokio::test]
    async fn test_palette_mouse_wheel_scrolls_the_viewport_on_a_long_inventory() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        // A short terminal makes the real Directory Tree inventory overflow.
        let backend = TestBackend::new(100, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_palette();
        let visible_rows = crate::ui::palette_layout(
            app.palette().items.len(),
            ratatui::layout::Rect::new(0, 0, 100, 12),
        )
        .visible_rows();
        assert!(
            app.palette().items.len() > visible_rows,
            "the test needs an inventory taller than the popup"
        );

        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        for _ in 0..visible_rows {
            handle_mouse(
                crossterm::event::MouseEvent {
                    kind: crossterm::event::MouseEventKind::ScrollDown,
                    column: 0,
                    row: 0,
                    modifiers: crossterm::event::KeyModifiers::empty(),
                },
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
        }

        // The render shell is what syncs the viewport, so go through it.
        terminal
            .draw(|f| crate::ui::draw_palette(f, &mut app))
            .unwrap();

        let selected = app.palette().selected_idx;
        let offset = app.palette().scroll_offset;
        assert!(
            offset > 0,
            "the wheel scrolled the viewport, not just the cursor"
        );
        assert!(
            (offset..offset + visible_rows).contains(&selected),
            "selection {selected} must stay inside the {visible_rows}-row window at {offset}"
        );
    }

    /// Issue #239: File Diff gained direct `D` / `E` bindings to match the palette
    /// entries. `$EDITOR` is mocked with a command whose effect on the file is
    /// observable, so this proves the key really reaches the editor handoff.
    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_file_diff_e_key_launches_the_external_editor() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let _guard = crate::test_support::lock_env_tests();
        std::env::remove_var("VISUAL");
        std::env::set_var("EDITOR", "truncate -s 0");

        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("a.txt"), "content").unwrap();
        std::fs::write(right.path().join("a.txt"), "other").unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        let info = FileInfo {
            is_dir: false,
            size: 7,
            modified: SystemTime::UNIX_EPOCH,
        };
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("a.txt"),
            name: "a.txt".to_string(),
            state: crate::diff::DiffState::DifferentSameTime,
            left: Some(info.clone()),
            right: Some(info),
        }]);
        app.apply_filter();
        app.set_selected_idx(0);
        app.focus_left_pane();
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('E'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(left.path().join("a.txt")).unwrap(),
            "",
            "`E` in File Diff must hand the focused side's file to $EDITOR"
        );
        assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);

        // `D` shares the same dispatch line; with no external diff tool selected
        // it is a harmless no-op that keeps the diff session.
        app.set_external_diff_tool(None);
        handle_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('D'),
                crossterm::event::KeyModifiers::empty(),
            ),
            &mut app,
            &mut terminal,
            tx,
        )
        .await
        .unwrap();
        assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
    }
}
