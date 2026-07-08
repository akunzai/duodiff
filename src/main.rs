use app::App;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use event::{AppEvent, EventHandler};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

pub mod app;
pub mod diff;
pub mod diff_tool;
pub mod diff_view;
pub mod event;
pub mod ignore;
pub mod settings;
pub mod ui;
pub mod update_check;
pub mod upgrade;

#[derive(Parser, Debug)]
#[command(
    name = "duodiff",
    version,
    about = "A cross-platform TUI directory comparison tool"
)]
struct Args {
    /// Left directory to compare
    #[arg(value_name = "LEFT_DIR")]
    left_dir: Option<PathBuf>,
    /// Right directory to compare
    #[arg(value_name = "RIGHT_DIR")]
    right_dir: Option<PathBuf>,
    /// Glob pattern to exclude from comparison. Can be specified multiple times.
    #[arg(short = 'e', long = "exclude", value_name = "PATTERN")]
    exclude: Vec<String>,
    /// Print startup checks without launching the TUI
    #[arg(long, help = "Print startup checks without launching the TUI")]
    check: bool,
    /// Upgrade the running pre-built binary from GitHub Releases (combine with --check or --upgrade-version)
    #[arg(
        long,
        help = "Upgrade the running pre-built binary from GitHub Releases (combine with --check or --upgrade-version)"
    )]
    upgrade: bool,
    /// With --upgrade: install a specific release (v0.1.0 or 0.1.0)
    #[arg(
        long = "upgrade-version",
        value_name = "TAG",
        help = "With --upgrade: install a specific release (v0.1.0 or 0.1.0)"
    )]
    upgrade_version: Option<String>,
    /// Skip the startup check for a newer release for this session
    #[arg(
        long = "no-update-check",
        help = "Skip the startup check for a newer release for this session"
    )]
    no_update_check: bool,
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    events: &mut EventHandler,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    loop {
        if app.should_quit {
            break;
        }
        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        use crossterm::event::KeyCode;
                        if app.palette.visible {
                            match key.code {
                                KeyCode::Esc => {
                                    app.palette.visible = false;
                                    app.palette.query.clear();
                                }
                                KeyCode::Char('q')
                                    if app.palette.mode == Some(app::PaletteMode::Menu) =>
                                {
                                    app.palette.visible = false;
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    if !app.palette.items.is_empty() {
                                        app.palette.selected_idx = (app.palette.selected_idx + 1)
                                            % app.palette.items.len();
                                    }
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    if !app.palette.items.is_empty() {
                                        app.palette.selected_idx = app
                                            .palette
                                            .selected_idx
                                            .checked_sub(1)
                                            .unwrap_or(app.palette.items.len() - 1);
                                    }
                                }
                                KeyCode::Enter => {
                                    if app.palette.selected_idx < app.palette.items.len() {
                                        let action =
                                            app.palette.items[app.palette.selected_idx].clone();
                                        if action.enabled {
                                            app.palette.visible = false;
                                            app.palette.query.clear();
                                            execute_palette_action(
                                                &action,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?;
                                        }
                                    }
                                }
                                KeyCode::Backspace => {
                                    if app.palette.mode == Some(app::PaletteMode::Command) {
                                        app.palette.query.pop();
                                        app.palette.selected_idx = 0;
                                    }
                                }
                                KeyCode::Char(c) => {
                                    if app.palette.mode == Some(app::PaletteMode::Command) {
                                        app.palette.query.push(c);
                                        app.palette.selected_idx = 0;
                                    } else {
                                        if let Some(pos) = app.palette.items.iter().position(|a| {
                                            a.key.to_lowercase() == c.to_string().to_lowercase()
                                        }) {
                                            let action = app.palette.items[pos].clone();
                                            if action.enabled {
                                                app.palette.visible = false;
                                                execute_palette_action(
                                                    &action,
                                                    app,
                                                    terminal,
                                                    tx.clone(),
                                                )
                                                .await?;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                            continue;
                        }

                        if key.code == KeyCode::Char(';') {
                            app.palette.visible = true;
                            app.palette.mode = Some(app::PaletteMode::Menu);
                            app.palette.query.clear();
                            app.palette.selected_idx = 0;
                            continue;
                        }
                        if key.code == KeyCode::Char('p')
                            && key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL)
                        {
                            app.palette.visible = true;
                            app.palette.mode = Some(app::PaletteMode::Command);
                            app.palette.query.clear();
                            app.palette.selected_idx = 0;
                            continue;
                        }

                        match app.view_mode {
                            app::ViewMode::DirectoryTree => {
                                if app.show_confirm_modal {
                                    match key.code {
                                        KeyCode::Char('y')
                                        | KeyCode::Char('Y')
                                        | KeyCode::Enter => {
                                            execute_confirm_action(app, tx.clone()).await?;
                                        }
                                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                            app.show_confirm_modal = false;
                                            app.confirm_modal_action = None;
                                        }
                                        _ => {}
                                    }
                                } else if app.filter_active {
                                    match key.code {
                                        KeyCode::Esc => {
                                            app.cancel_filter();
                                        }
                                        KeyCode::Enter => {
                                            app.commit_filter();
                                        }
                                        KeyCode::Backspace => {
                                            app.filter_input.pop();
                                        }
                                        KeyCode::Char('f') => {
                                            app.filter_diffs_only = !app.filter_diffs_only;
                                        }
                                        KeyCode::Char(c) => {
                                            app.filter_input.push(c);
                                        }
                                        _ => {}
                                    }
                                } else {
                                    match key.code {
                                        KeyCode::Char('q') => break,
                                        KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                                        KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                                        KeyCode::Char(' ') => app.toggle_expand(),
                                        KeyCode::Char('h') | KeyCode::Left => {
                                            app.collapse_selected()
                                        }
                                        KeyCode::Char('l') | KeyCode::Right => {
                                            app.expand_selected()
                                        }
                                        KeyCode::Tab => {
                                            app.active_side_left = !app.active_side_left
                                        }
                                        KeyCode::Char('1') => {
                                            app.focus_left_pane();
                                        }
                                        KeyCode::Char('2') => {
                                            app.focus_right_pane();
                                        }
                                        KeyCode::Char('c') => {
                                            app.precise_mode = !app.precise_mode;
                                            app.scan_in_progress = true;
                                            start_scan_task(
                                                app.left_path.clone(),
                                                app.right_path.clone(),
                                                app.precise_mode,
                                                app.ignore_matcher.clone(),
                                                tx.clone(),
                                            );
                                        }
                                        KeyCode::Char('r') => {
                                            app.scan_in_progress = true;
                                            start_scan_task(
                                                app.left_path.clone(),
                                                app.right_path.clone(),
                                                app.precise_mode,
                                                app.ignore_matcher.clone(),
                                                tx.clone(),
                                            );
                                        }
                                        KeyCode::Char('s') => {
                                            app.swap_paths();
                                            app.set_status("Swapped left ↔ right", false);
                                            app.scan_in_progress = true;
                                            start_scan_task(
                                                app.left_path.clone(),
                                                app.right_path.clone(),
                                                app.precise_mode,
                                                app.ignore_matcher.clone(),
                                                tx.clone(),
                                            );
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
                                            if !app.filter_pattern.is_empty()
                                                || app.filter_diffs_only =>
                                        {
                                            app.clear_filter();
                                        }
                                        KeyCode::Char('L')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            if row.right.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to left side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyRightToLeft);
                                            }
                                        }
                                        KeyCode::Char('R')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            if row.left.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to right side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyLeftToRight);
                                            }
                                        }
                                        KeyCode::Char('D')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            let is_dir = row
                                                .left
                                                .as_ref()
                                                .map(|f| f.is_dir)
                                                .unwrap_or(false)
                                                || row
                                                    .right
                                                    .as_ref()
                                                    .map(|f| f.is_dir)
                                                    .unwrap_or(false);
                                            if !is_dir && row.left.is_some() && row.right.is_some()
                                            {
                                                if let Some(ref tool_str) =
                                                    app.settings.external_diff_tool
                                                {
                                                    if let Ok(tool) =
                                                        diff_tool::ExternalDiffTool::from_str(
                                                            tool_str,
                                                        )
                                                    {
                                                        let left_file =
                                                            app.left_path.join(&row.relative_path);
                                                        let right_file =
                                                            app.right_path.join(&row.relative_path);
                                                        run_external_diff(
                                                            &tool,
                                                            &left_file,
                                                            &right_file,
                                                            terminal,
                                                        )?;
                                                    }
                                                }
                                            }
                                        }
                                        KeyCode::Char('E')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            let file_exists = if app.active_side_left {
                                                row.left
                                                    .as_ref()
                                                    .map(|f| !f.is_dir)
                                                    .unwrap_or(false)
                                            } else {
                                                row.right
                                                    .as_ref()
                                                    .map(|f| !f.is_dir)
                                                    .unwrap_or(false)
                                            };
                                            if file_exists {
                                                let file_path = if app.active_side_left {
                                                    app.left_path.join(&row.relative_path)
                                                } else {
                                                    app.right_path.join(&row.relative_path)
                                                };
                                                run_external_editor(&file_path, terminal)?;
                                            }
                                        }
                                        KeyCode::Enter
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            let is_dir = row
                                                .left
                                                .as_ref()
                                                .map(|f| f.is_dir)
                                                .unwrap_or(false)
                                                || row
                                                    .right
                                                    .as_ref()
                                                    .map(|f| f.is_dir)
                                                    .unwrap_or(false);
                                            if !is_dir {
                                                let left_file =
                                                    app.left_path.join(&row.relative_path);
                                                let right_file =
                                                    app.right_path.join(&row.relative_path);
                                                app.diff_show_full = false;
                                                app.diff_rows = crate::diff_view::compare_files(
                                                    &left_file,
                                                    &right_file,
                                                    app.diff_show_full,
                                                )
                                                .unwrap_or_default();
                                                app.diff_left_hash =
                                                    crate::diff::compute_file_md5(&left_file).ok();
                                                app.diff_right_hash =
                                                    crate::diff::compute_file_md5(&right_file).ok();
                                                app.diff_left_line_ending =
                                                    crate::diff_view::detect_file_line_ending(
                                                        &left_file,
                                                    );
                                                app.diff_right_line_ending =
                                                    crate::diff_view::detect_file_line_ending(
                                                        &right_file,
                                                    );
                                                app.view_mode = app::ViewMode::FileDiff;
                                                app.diff_scroll = 0;
                                                app.diff_h_scroll = 0;
                                            } else {
                                                app.toggle_expand();
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            app::ViewMode::FileDiff => {
                                if app.show_confirm_modal {
                                    match key.code {
                                        KeyCode::Char('y')
                                        | KeyCode::Char('Y')
                                        | KeyCode::Enter => {
                                            execute_confirm_action(app, tx.clone()).await?;
                                        }
                                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                            app.show_confirm_modal = false;
                                            app.confirm_modal_action = None;
                                        }
                                        _ => {}
                                    }
                                } else {
                                    match key.code {
                                        KeyCode::Esc | KeyCode::Char('q') => {
                                            app.view_mode = app::ViewMode::DirectoryTree;
                                        }
                                        KeyCode::Down
                                            if key
                                                .modifiers
                                                .contains(crossterm::event::KeyModifiers::ALT) =>
                                        {
                                            app.jump_to_next_change();
                                        }
                                        KeyCode::Up
                                            if key
                                                .modifiers
                                                .contains(crossterm::event::KeyModifiers::ALT) =>
                                        {
                                            app.jump_to_prev_change();
                                        }
                                        KeyCode::Char('N') => {
                                            app.jump_to_next_change();
                                        }
                                        KeyCode::Char('P') => {
                                            app.jump_to_prev_change();
                                        }
                                        KeyCode::Char('j') | KeyCode::Down => {
                                            let max_scroll = app
                                                .diff_physical_rows
                                                .saturating_sub(app.visible_height);
                                            if app.diff_scroll < max_scroll {
                                                app.diff_scroll += 1;
                                            }
                                        }
                                        KeyCode::Char('k') | KeyCode::Up if app.diff_scroll > 0 => {
                                            app.diff_scroll -= 1;
                                        }
                                        KeyCode::Left => {
                                            if !app.diff_wrap && app.diff_h_scroll > 0 {
                                                app.diff_h_scroll -= 1;
                                            }
                                        }
                                        KeyCode::Right => {
                                            if !app.diff_wrap {
                                                let content_width = (terminal
                                                    .size()
                                                    .map(|s| s.width as usize)
                                                    .unwrap_or(80)
                                                    / 2)
                                                .saturating_sub(2);
                                                let max_h_scroll = app
                                                    .diff_max_line_width
                                                    .saturating_sub(content_width);
                                                if app.diff_h_scroll < max_h_scroll {
                                                    app.diff_h_scroll += 1;
                                                }
                                            }
                                        }
                                        KeyCode::Char('L') | KeyCode::Char('l')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            if row.right.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to left side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyRightToLeft);
                                            }
                                        }
                                        KeyCode::Char('R') | KeyCode::Char('r')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            if row.left.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to right side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyLeftToRight);
                                            }
                                        }
                                        KeyCode::Char('[') => {
                                            match app.copy_hunk_at_cursor(
                                                crate::diff_view::HunkCopyDirection::RightToLeft,
                                            ) {
                                                Ok(()) => app.set_status(
                                                    "Copied change block to left".to_string(),
                                                    false,
                                                ),
                                                Err(e) => app.set_status(
                                                    format!("Hunk copy failed: {}", e),
                                                    true,
                                                ),
                                            }
                                        }
                                        KeyCode::Char(']') => {
                                            match app.copy_hunk_at_cursor(
                                                crate::diff_view::HunkCopyDirection::LeftToRight,
                                            ) {
                                                Ok(()) => app.set_status(
                                                    "Copied change block to right".to_string(),
                                                    false,
                                                ),
                                                Err(e) => app.set_status(
                                                    format!("Hunk copy failed: {}", e),
                                                    true,
                                                ),
                                            }
                                        }
                                        KeyCode::Char('w') => {
                                            app.diff_wrap = !app.diff_wrap;
                                            app.diff_scroll = 0;
                                            app.diff_h_scroll = 0;
                                        }
                                        KeyCode::Char('?') => {
                                            app.open_help();
                                        }
                                        KeyCode::Char('f')
                                            if app.selected_idx < app.filtered_rows.len() =>
                                        {
                                            let row = &app.filtered_rows[app.selected_idx];
                                            let left_file = app.left_path.join(&row.relative_path);
                                            let right_file =
                                                app.right_path.join(&row.relative_path);
                                            app.diff_show_full = !app.diff_show_full;
                                            app.diff_rows = crate::diff_view::compare_files(
                                                &left_file,
                                                &right_file,
                                                app.diff_show_full,
                                            )
                                            .unwrap_or_default();
                                            app.diff_scroll = 0;
                                            app.diff_h_scroll = 0;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            app::ViewMode::ConfigMenu => match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    app.view_mode = app::ViewMode::DirectoryTree
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    app.config_select_next();
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    app.config_select_prev();
                                }
                                KeyCode::Char(' ') | KeyCode::Enter => {
                                    app.apply_config_selection();
                                }
                                KeyCode::Char('?') => {
                                    app.open_help();
                                }
                                _ => {}
                            },
                            app::ViewMode::Help => {
                                if app.help_index_open {
                                    match key.code {
                                        KeyCode::Char('j') | KeyCode::Down => {
                                            app.help_index_sel = (app.help_index_sel + 1)
                                                % app::HelpTopic::all().len();
                                        }
                                        KeyCode::Char('k') | KeyCode::Up => {
                                            app.help_index_sel = app
                                                .help_index_sel
                                                .checked_sub(1)
                                                .unwrap_or(app::HelpTopic::all().len() - 1);
                                        }
                                        KeyCode::Enter => {
                                            app.help_topic =
                                                app::HelpTopic::all()[app.help_index_sel];
                                            app.help_index_open = false;
                                            app.help_scroll = 0;
                                        }
                                        KeyCode::Char(c @ '1'..='6') => {
                                            app.help_topic =
                                                app::HelpTopic::all()[(c as u8 - b'1') as usize];
                                            app.help_index_open = false;
                                            app.help_scroll = 0;
                                        }
                                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                                            app.view_mode = app.help_return_view;
                                            app.help_index_open = false;
                                        }
                                        _ => {}
                                    }
                                } else {
                                    match key.code {
                                        KeyCode::Char(c @ '1'..='6') => {
                                            app.help_topic =
                                                app::HelpTopic::all()[(c as u8 - b'1') as usize];
                                            app.help_scroll = 0;
                                        }
                                        KeyCode::Tab => {
                                            app.help_index_sel = app::HelpTopic::all()
                                                .iter()
                                                .position(|&t| t == app.help_topic)
                                                .unwrap_or(0);
                                            app.help_index_open = true;
                                        }
                                        KeyCode::Char('j') | KeyCode::Down => {
                                            app.help_scroll = app.help_scroll.saturating_add(1);
                                        }
                                        KeyCode::Char('k') | KeyCode::Up => {
                                            app.help_scroll = app.help_scroll.saturating_sub(1);
                                        }
                                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                                            app.view_mode = app.help_return_view;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse)) => {
                    use crossterm::event::MouseEventKind;
                    if let MouseEventKind::Down(crossterm::event::MouseButton::Left) = mouse.kind {
                        if app.show_confirm_modal {
                            if let Ok(size) = terminal.size() {
                                let size_rect =
                                    ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                                let modal_area = crate::ui::centered_rect(60, 7, size_rect);
                                if mouse.row == modal_area.y
                                    && mouse.column
                                        >= modal_area.x + modal_area.width.saturating_sub(5)
                                    && mouse.column
                                        < modal_area.x + modal_area.width.saturating_sub(2)
                                {
                                    app.show_confirm_modal = false;
                                    app.confirm_modal_action = None;
                                    continue;
                                }
                            }
                        }
                        if mouse.row == 0 {
                            if let Ok(size) = terminal.size() {
                                let w = size.width;
                                if mouse.column >= w.saturating_sub(17)
                                    && mouse.column < w.saturating_sub(9)
                                {
                                    app.palette.visible = false;
                                    app.palette.query.clear();
                                    app.open_config();
                                    continue;
                                } else if mouse.column >= w.saturating_sub(7) {
                                    app.palette.visible = false;
                                    app.palette.query.clear();
                                    app.open_help();
                                    continue;
                                }
                            }
                        } else if app.palette.visible {
                            if let Ok(size) = terminal.size() {
                                let mode = app.palette.mode.unwrap_or(app::PaletteMode::Menu);
                                let count = app.palette.items.len();
                                let (pop_w, pop_h) = match mode {
                                    app::PaletteMode::Menu => (50, (count + 2).max(4) as u16),
                                    app::PaletteMode::Command => (55, 12),
                                };
                                let menu_x = size.width.saturating_sub(pop_w) / 2;
                                let menu_y = size.height.saturating_sub(pop_h) / 2;

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
                                        app.palette.visible = false;
                                        app.palette.query.clear();
                                        continue;
                                    }

                                    let list_start_y = match mode {
                                        app::PaletteMode::Menu => menu_y + 1,
                                        app::PaletteMode::Command => menu_y + 3,
                                    };
                                    if mouse.row >= list_start_y && mouse.row < menu_y + pop_h - 1 {
                                        let click_idx = (mouse.row - list_start_y) as usize;
                                        if click_idx < app.palette.items.len() {
                                            let action = app.palette.items[click_idx].clone();
                                            if action.enabled {
                                                app.palette.visible = false;
                                                app.palette.query.clear();
                                                execute_palette_action(
                                                    &action,
                                                    app,
                                                    terminal,
                                                    tx.clone(),
                                                )
                                                .await?;
                                            }
                                        }
                                    }
                                } else {
                                    app.palette.visible = false;
                                    app.palette.query.clear();
                                }
                            }
                            continue;
                        } else {
                            if let Ok(size) = terminal.size() {
                                if app.view_mode == app::ViewMode::Help {
                                    if mouse.row == 1
                                        && mouse.column >= size.width.saturating_sub(5)
                                        && mouse.column < size.width.saturating_sub(2)
                                    {
                                        app.view_mode = app.help_return_view;
                                        continue;
                                    }
                                } else if app.view_mode == app::ViewMode::ConfigMenu {
                                    if mouse.row == 1
                                        && mouse.column >= size.width.saturating_sub(5)
                                        && mouse.column < size.width.saturating_sub(2)
                                    {
                                        app.view_mode = app::ViewMode::DirectoryTree;
                                        continue;
                                    }
                                } else if app.view_mode == app::ViewMode::FileDiff {
                                    let row = app.filtered_rows.get(app.selected_idx);
                                    let has_changes = app.diff_rows.iter().any(|(l, r)| {
                                        l.as_ref().map(|d| d.tag)
                                            == Some(similar::ChangeTag::Delete)
                                            || r.as_ref().map(|d| d.tag)
                                                == Some(similar::ChangeTag::Insert)
                                    });
                                    let show_identical = !has_changes
                                        && row
                                            .is_some_and(|r| r.left.is_some() || r.right.is_some());
                                    let header_height = if show_identical { 2 } else { 1 };
                                    let body_y = header_height + 1;
                                    if mouse.row == body_y as u16
                                        && mouse.column >= size.width.saturating_sub(5)
                                        && mouse.column < size.width.saturating_sub(2)
                                    {
                                        app.view_mode = app::ViewMode::DirectoryTree;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    match app.view_mode {
                        app::ViewMode::DirectoryTree => match mouse.kind {
                            MouseEventKind::ScrollDown => app.select_next(),
                            MouseEventKind::ScrollUp => app.select_prev(),
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                let click_y = mouse.row as usize;
                                if click_y >= 2 {
                                    let offset_y = click_y - 2;
                                    if offset_y < app.visible_height {
                                        let idx = app.scroll_offset + offset_y;
                                        if idx < app.filtered_rows.len() {
                                            let now = std::time::Instant::now();
                                            let is_double_click = Some(idx) == app.last_click_idx
                                                && app.last_click_time.is_some_and(|t| {
                                                    now.duration_since(t)
                                                        < std::time::Duration::from_millis(400)
                                                });

                                            app.selected_idx = idx;

                                            if is_double_click {
                                                let row = &app.filtered_rows[app.selected_idx];
                                                let is_dir = row
                                                    .left
                                                    .as_ref()
                                                    .map(|f| f.is_dir)
                                                    .unwrap_or(false)
                                                    || row
                                                        .right
                                                        .as_ref()
                                                        .map(|f| f.is_dir)
                                                        .unwrap_or(false);
                                                if !is_dir {
                                                    let left_file =
                                                        app.left_path.join(&row.relative_path);
                                                    let right_file =
                                                        app.right_path.join(&row.relative_path);
                                                    app.diff_show_full = false;
                                                    app.diff_rows =
                                                        crate::diff_view::compare_files(
                                                            &left_file,
                                                            &right_file,
                                                            app.diff_show_full,
                                                        )
                                                        .unwrap_or_default();
                                                    app.diff_left_hash =
                                                        crate::diff::compute_file_md5(&left_file)
                                                            .ok();
                                                    app.diff_right_hash =
                                                        crate::diff::compute_file_md5(&right_file)
                                                            .ok();
                                                    app.diff_left_line_ending =
                                                        crate::diff_view::detect_file_line_ending(
                                                            &left_file,
                                                        );
                                                    app.diff_right_line_ending =
                                                        crate::diff_view::detect_file_line_ending(
                                                            &right_file,
                                                        );
                                                    app.view_mode = app::ViewMode::FileDiff;
                                                    app.diff_scroll = 0;
                                                    app.diff_h_scroll = 0;
                                                } else {
                                                    app.toggle_expand();
                                                }
                                                app.last_click_idx = None;
                                                app.last_click_time = None;
                                            } else {
                                                app.last_click_idx = Some(idx);
                                                app.last_click_time = Some(now);
                                            }
                                        }
                                    }
                                }
                            }
                            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                                let click_y = mouse.row as usize;
                                if click_y >= 2 {
                                    let offset_y = click_y - 2;
                                    if offset_y < app.visible_height {
                                        let idx = app.scroll_offset + offset_y;
                                        if idx < app.filtered_rows.len() {
                                            app.selected_idx = idx;
                                            app.palette.visible = true;
                                            app.palette.mode = Some(app::PaletteMode::Menu);
                                            app.palette.query.clear();
                                            app.palette.selected_idx = 0;
                                            app.palette.x = mouse.column;
                                            app.palette.y = mouse.row;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        },
                        app::ViewMode::FileDiff => match mouse.kind {
                            MouseEventKind::ScrollDown => {
                                let max_scroll =
                                    app.diff_rows.len().saturating_sub(app.visible_height);
                                if app.diff_scroll < max_scroll {
                                    app.diff_scroll += 1;
                                }
                            }
                            MouseEventKind::ScrollUp if app.diff_scroll > 0 => {
                                app.diff_scroll -= 1;
                            }
                            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                                app.palette.visible = true;
                                app.palette.mode = Some(app::PaletteMode::Menu);
                                app.palette.query.clear();
                                app.palette.selected_idx = 0;
                                app.palette.x = mouse.column;
                                app.palette.y = mouse.row;
                            }
                            _ => {}
                        },
                        app::ViewMode::ConfigMenu => match mouse.kind {
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                let click_y = mouse.row as usize;
                                if click_y >= 2 {
                                    let row_idx = click_y - 2;
                                    let rows = app.config_rows();
                                    if row_idx < rows.len()
                                        && matches!(rows[row_idx], app::ConfigRowKind::DiffTool(_))
                                    {
                                        app.config_selected_idx = row_idx;
                                        app.apply_config_selection();
                                    }
                                }
                            }
                            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                                app.palette.visible = true;
                                app.palette.mode = Some(app::PaletteMode::Menu);
                                app.palette.query.clear();
                                app.palette.selected_idx = 0;
                                app.palette.x = mouse.column;
                                app.palette.y = mouse.row;
                            }
                            _ => {}
                        },
                        app::ViewMode::Help => match mouse.kind {
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                if app.help_index_open {
                                    let click_y = mouse.row as usize;
                                    if click_y >= 2
                                        && click_y < 2 + crate::app::HelpTopic::all().len()
                                    {
                                        let idx = click_y - 2;
                                        app.help_topic = crate::app::HelpTopic::all()[idx];
                                        app.help_index_open = false;
                                        app.help_scroll = 0;
                                    }
                                } else if app.help_topic == app::HelpTopic::About
                                    && mouse.row == 5
                                    && mouse.column >= 3
                                    && mouse.column < 37
                                {
                                    open_repo_url(app);
                                }
                            }
                            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                                app.palette.visible = true;
                                app.palette.mode = Some(app::PaletteMode::Menu);
                                app.palette.query.clear();
                                app.palette.selected_idx = 0;
                                app.palette.x = mouse.column;
                                app.palette.y = mouse.row;
                            }
                            _ => {}
                        },
                    }
                }
                AppEvent::ScanFinished(node) => {
                    app.root_node = Some(node);
                    app.scan_in_progress = false;
                    app.flatten_tree();
                }
                AppEvent::Error(err) => {
                    return Err(err.into());
                }
                AppEvent::Tick => {
                    // Auto-expire status toast after 4 seconds
                    app.clear_expired_status(std::time::Duration::from_secs(4));
                }
                AppEvent::UpdateCheckOutcome(outcome) => {
                    let now = crate::update_check::now_secs();
                    match outcome {
                        crate::update_check::UpdateCheckOutcome::Newer(version) => {
                            if let Ok(path) = crate::update_check::state_path() {
                                crate::update_check::save_state(
                                    &path,
                                    &crate::update_check::UpdateCheckState {
                                        last_check: now,
                                        latest_seen: version.clone(),
                                    },
                                );
                            }
                            app.update_available = Some(version);
                        }
                        crate::update_check::UpdateCheckOutcome::UpToDate => {
                            if let Ok(path) = crate::update_check::state_path() {
                                crate::update_check::save_state(
                                    &path,
                                    &crate::update_check::UpdateCheckState {
                                        last_check: now,
                                        latest_seen: String::new(),
                                    },
                                );
                            }
                            app.update_available = None;
                        }
                        crate::update_check::UpdateCheckOutcome::Failed => {}
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn run_external_diff<B: ratatui::backend::Backend>(
    tool: &diff_tool::ExternalDiffTool,
    left: &std::path::Path,
    right: &std::path::Path,
    terminal: &mut ratatui::Terminal<B>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    use std::io::IsTerminal;
    let is_terminal = std::io::stdout().is_terminal();
    if is_terminal {
        disable_raw_mode()?;
        execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
    }

    let res = diff_tool::open_diff(tool, left, right);
    if let Err(e) = res {
        eprintln!(
            "Error launching external diff: {}. Press Enter to continue...",
            e
        );
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    } else if matches!(tool, diff_tool::ExternalDiffTool::Difftastic) {
        println!("\nPress Enter to return to duodiff...");
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }

    if is_terminal {
        enable_raw_mode()?;
        execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
    }
    terminal.clear()?;
    Ok(())
}

fn run_external_editor<B: ratatui::backend::Backend>(
    file_path: &std::path::Path,
    terminal: &mut ratatui::Terminal<B>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    use std::io::IsTerminal;
    let is_terminal = std::io::stdout().is_terminal();
    if is_terminal {
        disable_raw_mode()?;
        execute!(
            std::io::stdout(),
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        )?;
    }

    let res = diff_tool::open_editor(file_path);
    if let Err(e) = res {
        eprintln!(
            "Error launching external editor: {}. Press Enter to continue...",
            e
        );
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
    }

    if is_terminal {
        enable_raw_mode()?;
        execute!(
            std::io::stdout(),
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
    }
    terminal.clear()?;
    Ok(())
}

async fn execute_confirm_action(
    app: &mut App,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    app.show_confirm_modal = false;
    if let Some(action) = app.confirm_modal_action.take() {
        if app.selected_idx < app.filtered_rows.len() {
            let row = &app.filtered_rows[app.selected_idx];
            let relative_path = &row.relative_path;
            let name = row.name.clone();

            let src = match action {
                app::ConfirmAction::CopyLeftToRight => app.left_path.join(relative_path),
                app::ConfirmAction::CopyRightToLeft => app.right_path.join(relative_path),
            };
            let dst = match action {
                app::ConfirmAction::CopyLeftToRight => app.right_path.join(relative_path),
                app::ConfirmAction::CopyRightToLeft => app.left_path.join(relative_path),
            };

            // Perform copy — all errors are captured uniformly in `res`
            let res: Result<(), std::io::Error> = if src.is_dir() {
                copy_dir_recursive(&src, &dst)
            } else if src.is_file() {
                (|| {
                    if let Some(parent) = dst.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(&src, &dst).map(|_| ())
                })()
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Source path not found on disk",
                ))
            };

            match res {
                Ok(()) => {
                    app.set_status(format!("Copied '{}'", name), false);
                    // Switch back to DirectoryTree and trigger re-scan
                    app.view_mode = app::ViewMode::DirectoryTree;
                    app.scan_in_progress = true;
                    start_scan_task(
                        app.left_path.clone(),
                        app.right_path.clone(),
                        app.precise_mode,
                        app.ignore_matcher.clone(),
                        tx,
                    );
                }
                Err(e) => {
                    app.set_status(format!("Copy failed: {}", e), true);
                }
            }
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.upgrade {
        crate::upgrade::run(crate::upgrade::Options {
            check_only: args.check,
            version: args.upgrade_version,
        })?;
        return Ok(());
    }

    if args.check && args.left_dir.is_none() && args.right_dir.is_none() {
        println!("duodiff version {} is ready", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let left_dir = match args.left_dir.clone() {
        Some(d) => d,
        None => {
            eprintln!("Error: Missing LEFT_DIR directory argument.");
            std::process::exit(1);
        }
    };
    let right_dir = match args.right_dir.clone() {
        Some(d) => d,
        None => {
            eprintln!("Error: Missing RIGHT_DIR directory argument.");
            std::process::exit(1);
        }
    };

    if !left_dir.is_dir() || !right_dir.is_dir() {
        eprintln!("Both arguments must be valid directories.");
        std::process::exit(1);
    }

    let mut ignore_matcher = crate::ignore::IgnoreMatcher::new();
    ignore_matcher.add_patterns(&args.exclude);
    ignore_matcher.load_from_dir(&left_dir);
    ignore_matcher.load_from_dir(&right_dir);

    // Initialize terminal safely
    let mut terminal = setup_terminal()?;

    let mut app = App::new_with_ignore(left_dir.clone(), right_dir.clone(), ignore_matcher.clone());
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    // Initialize update checker
    app.update_check_enabled = !args.no_update_check && app.settings.check_updates;
    if app.update_check_enabled {
        if let Ok(path) = crate::update_check::state_path() {
            let seen = crate::update_check::load_state(&path).latest_seen;
            if !seen.is_empty() {
                app.update_available =
                    crate::update_check::is_newer(&seen, env!("CARGO_PKG_VERSION"));
            }
        }

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let path_opt = crate::update_check::state_path().ok();
            let due = path_opt.as_ref().is_none_or(|path| {
                crate::update_check::should_check(
                    crate::update_check::load_state(path).last_check,
                    crate::update_check::now_secs(),
                )
            });
            if due {
                let outcome = tokio::task::spawn_blocking(move || {
                    crate::update_check::check(
                        &crate::upgrade::UreqClient,
                        env!("CARGO_PKG_VERSION"),
                    )
                })
                .await
                .unwrap_or(crate::update_check::UpdateCheckOutcome::Failed);
                let _ = tx_clone.send(AppEvent::UpdateCheckOutcome(outcome)).await;
            }
        });
    }

    app.scan_in_progress = true;
    start_scan_task(
        left_dir.clone(),
        right_dir.clone(),
        app.precise_mode,
        ignore_matcher,
        tx.clone(),
    );

    let res = run_app(&mut terminal, &mut app, &mut events, tx.clone()).await;

    // Restore terminal unconditionally
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    );

    res
}

fn setup_terminal(
) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>, Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    if let Err(err) = execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    ) {
        let _ = disable_raw_mode();
        return Err(err.into());
    }
    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(t) => Ok(t),
        Err(err) => {
            let _ = execute!(
                std::io::stdout(),
                LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture
            );
            let _ = disable_raw_mode();
            Err(err.into())
        }
    }
}

fn start_scan_task(
    left: PathBuf,
    right: PathBuf,
    precise: bool,
    ignore: crate::ignore::IgnoreMatcher,
    tx: tokio::sync::mpsc::Sender<crate::event::AppEvent>,
) {
    tokio::spawn(async move {
        let root = tokio::task::spawn_blocking(move || {
            crate::diff::align_directories(
                &left,
                &right,
                std::path::Path::new(""),
                precise,
                &ignore,
            )
        })
        .await;

        match root {
            Ok(Ok(node)) => {
                let _ = tx.send(crate::event::AppEvent::ScanFinished(node)).await;
            }
            Ok(Err(err)) => {
                let _ = tx
                    .send(crate::event::AppEvent::Error(err.to_string()))
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(crate::event::AppEvent::Error(err.to_string()))
                    .await;
            }
        }
    });
}

async fn execute_palette_action<B: ratatui::backend::Backend>(
    action: &crate::app::PaletteAction,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    match action.action_id {
        "ext_diff" => {
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                if let Some(ref tool_str) = app.settings.external_diff_tool {
                    if let Ok(tool) = diff_tool::ExternalDiffTool::from_str(tool_str) {
                        let left_file = app.left_path.join(&row.relative_path);
                        let right_file = app.right_path.join(&row.relative_path);
                        run_external_diff(&tool, &left_file, &right_file, terminal)?;
                    }
                }
            }
        }
        "ext_edit" => {
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                let file_exists = if app.active_side_left {
                    row.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                } else {
                    row.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                };
                if file_exists {
                    let file_path = if app.active_side_left {
                        app.left_path.join(&row.relative_path)
                    } else {
                        app.right_path.join(&row.relative_path)
                    };
                    run_external_editor(&file_path, terminal)?;
                }
            }
        }
        "copy_l2r" => {
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                if row.left.is_some() {
                    app.show_confirm_modal = true;
                    app.confirm_modal_message = format!("Copy '{}' to right side?", row.name);
                    app.confirm_modal_action = Some(app::ConfirmAction::CopyLeftToRight);
                }
            }
        }
        "copy_r2l" => {
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                if row.right.is_some() {
                    app.show_confirm_modal = true;
                    app.confirm_modal_message = format!("Copy '{}' to left side?", row.name);
                    app.confirm_modal_action = Some(app::ConfirmAction::CopyRightToLeft);
                }
            }
        }
        "builtin_diff" => {
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                let left_file = app.left_path.join(&row.relative_path);
                let right_file = app.right_path.join(&row.relative_path);
                app.diff_show_full = false;
                app.diff_rows =
                    crate::diff_view::compare_files(&left_file, &right_file, app.diff_show_full)
                        .unwrap_or_default();
                app.diff_left_hash = crate::diff::compute_file_md5(&left_file).ok();
                app.diff_right_hash = crate::diff::compute_file_md5(&right_file).ok();
                app.diff_left_line_ending = crate::diff_view::detect_file_line_ending(&left_file);
                app.diff_right_line_ending = crate::diff_view::detect_file_line_ending(&right_file);
                app.view_mode = app::ViewMode::FileDiff;
                app.diff_scroll = 0;
                app.diff_h_scroll = 0;
            }
        }
        "swap_paths" => {
            app.swap_paths();
            app.scan_in_progress = true;
            start_scan_task(
                app.left_path.clone(),
                app.right_path.clone(),
                app.precise_mode,
                app.ignore_matcher.clone(),
                tx,
            );
        }
        "toggle_scan" => {
            app.precise_mode = !app.precise_mode;
            app.scan_in_progress = true;
            start_scan_task(
                app.left_path.clone(),
                app.right_path.clone(),
                app.precise_mode,
                app.ignore_matcher.clone(),
                tx,
            );
        }
        "refresh" => {
            app.scan_in_progress = true;
            start_scan_task(
                app.left_path.clone(),
                app.right_path.clone(),
                app.precise_mode,
                app.ignore_matcher.clone(),
                tx,
            );
        }
        "config" => {
            app.open_config();
        }
        "help" => {
            app.open_help();
        }
        "filter" => {
            app.filter_active = true;
            app.filter_input.clear();
        }
        "quit" => {
            app.should_quit = true;
        }
        "toggle_wrap" => {
            app.diff_wrap = !app.diff_wrap;
            app.diff_scroll = 0;
            app.diff_h_scroll = 0;
        }
        "toggle_full" => {
            app.diff_show_full = !app.diff_show_full;
            if app.selected_idx < app.filtered_rows.len() {
                let row = &app.filtered_rows[app.selected_idx];
                let left_file = app.left_path.join(&row.relative_path);
                let right_file = app.right_path.join(&row.relative_path);
                app.diff_rows =
                    crate::diff_view::compare_files(&left_file, &right_file, app.diff_show_full)
                        .unwrap_or_default();
            }
            app.diff_scroll = 0;
            app.diff_h_scroll = 0;
        }
        "next_change" => {
            app.jump_to_next_change();
        }
        "prev_change" => {
            app.jump_to_prev_change();
        }
        "copy_hunk_l2r" => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight) {
                Ok(()) => app.set_status("Copied change block to right".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        "copy_hunk_r2l" => {
            match app.copy_hunk_at_cursor(crate::diff_view::HunkCopyDirection::RightToLeft) {
                Ok(()) => app.set_status("Copied change block to left".to_string(), false),
                Err(e) => app.set_status(format!("Hunk copy failed: {}", e), true),
            }
        }
        "back" => {
            if app.view_mode == app::ViewMode::FileDiff {
                app.view_mode = app::ViewMode::DirectoryTree;
            } else {
                app.view_mode = app.help_return_view;
            }
        }
        _ => {}
    }
    Ok(())
}

fn open_repo_url(app: &mut App) {
    app.set_status("Opening GitHub repository in the browser...", false);
    let url = "https://github.com/akunzai/duodiff";
    std::thread::spawn(move || {
        let _ = match std::env::consts::OS {
            "macos" => std::process::Command::new("open").arg(url).status(),
            "windows" => std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .status(),
            _ => std::process::Command::new("xdg-open").arg(url).status(),
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AppEvent;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_start_scan_task() {
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        start_scan_task(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
            false,
            crate::ignore::IgnoreMatcher::default(),
            tx,
        );

        let res = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        let opt = res.expect("Timeout waiting for scan result");
        let event = opt.expect("Expected Some(AppEvent::ScanFinished), got None");
        assert!(
            matches!(event, AppEvent::ScanFinished(_)),
            "Expected AppEvent::ScanFinished"
        );
    }

    #[tokio::test]
    async fn test_execute_palette_action() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let (tx, _rx) = tokio::sync::mpsc::channel(1);

        // Test config action
        let action_config = crate::app::PaletteAction {
            key: "C".to_string(),
            label: "Edit Configuration".to_string(),
            action_id: "config",
            enabled: true,
        };
        execute_palette_action(&action_config, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert_eq!(app.view_mode, crate::app::ViewMode::ConfigMenu);

        // Test quit action
        let action_quit = crate::app::PaletteAction {
            key: "q".to_string(),
            label: "Quit".to_string(),
            action_id: "quit",
            enabled: true,
        };
        execute_palette_action(&action_quit, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn test_run_app_pane_focus_number_keys() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert!(app.active_side_left);

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('2'),
                crossterm::event::KeyCode::Char('1'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert!(app.active_side_left);
    }

    #[tokio::test]
    async fn test_run_app_keyboard_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        // Let's send a key event to move down
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let key_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(key_event)).await;

            // And then send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Assert that the 'j' key was processed and app moved down
        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_run_app_mouse_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Scroll down
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::ScrollDown,
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_run_app_mouse_click_navigation() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
            crate::app::FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: crate::diff::DiffState::Identical,
                left: None,
                right: None,
            },
        ];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the second row (click_y = 3, which maps to index 3 - 2 = 1)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 3,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.selected_idx, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert_eq!(app.selected_idx, 1);
    }

    #[tokio::test]
    async fn test_help_index_mouse_click_selects_topic() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::Help;
        app.help_index_open = true;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the 4th item (click_y = 5, maps to index 5 - 2 = 3 which is HelpTopic::Mouse)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(mouse_event)).await;

            // Exit help topic view, then quit from directory tree.
            for code in [
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Mouse);
        assert!(!app.help_index_open);
    }

    #[tokio::test]
    async fn test_run_app_keyboard_expand_collapse() {
        use crate::diff::{AlignedNode, DiffState, FileInfo};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let node = AlignedNode {
            name: "root".to_string(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "child".to_string(),
                relative_path: PathBuf::from("child"),
                left: Some(FileInfo {
                    is_dir: false,
                    size: 10,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![],
                is_expanded: false,
            }],
            is_expanded: true,
        };
        app.root_node = Some(node);
        app.flatten_tree();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Select root (idx = 0) and collapse it using 'h'
            let collapse_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('h'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(collapse_event)).await;

            // Expand it using 'Right' key
            let expand_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Right,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(expand_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.flat_rows.len(), 2);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Since it was collapsed and expanded, flat_rows should be 2 again
        assert_eq!(app.flat_rows.len(), 2);
    }

    #[tokio::test]
    async fn test_run_app_file_diff_navigation() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press Enter to go to FileDiff mode
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Scroll down
            let down_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(down_event)).await;

            // Scroll up
            let up_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('k'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(up_event)).await;

            // Press Esc to exit FileDiff mode
            let esc_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(esc_event)).await;

            // Send 'q' to quit
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        // Initially in DirectoryTree mode
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_run_app_keyboard_diff_tool_launch() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let _guard = crate::diff_tool::TEST_MUTEX
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.settings.external_diff_tool = None;
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 'D' to launch diff tool
            let d_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('D'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(d_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_run_app_keyboard_editor_launch() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let _guard = crate::diff_tool::TEST_MUTEX
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap();
        std::env::remove_var("VISUAL");
        #[cfg(not(target_os = "windows"))]
        std::env::set_var("EDITOR", "true");
        #[cfg(target_os = "windows")]
        std::env::set_var("EDITOR", "cargo --version");

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 'E' to launch editor
            let e_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('E'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(e_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_run_app_mouse_double_click_enters_diff() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::fs;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        fs::write(left_dir.path().join("file.txt"), "hello").unwrap();
        fs::write(right_dir.path().join("file.txt"), "world").unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(crate::diff::FileInfo {
                is_dir: false,
                size: 10,
                modified: std::time::SystemTime::UNIX_EPOCH,
            }),
            right: Some(crate::diff::FileInfo {
                is_dir: false,
                size: 15,
                modified: std::time::SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // First click
            let click1 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(click1)).await;

            // Second click immediately
            let click2 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 2,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(click2)).await;

            // Wait, then quit
            tokio::time::sleep(Duration::from_millis(50)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event.clone())).await;

            // Send a second 'q' to quit the app from DirectoryTree mode
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Should end up back in DirectoryTree mode after the sequence
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        // Verify that it did enter FileDiff mode and populated diff_rows
        assert!(!app.diff_rows.is_empty());
    }

    #[tokio::test]
    async fn test_copy_file_and_directory() {
        use crate::diff::FileInfo;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        // 1. Test copy_dir_recursive helper
        let src_sub = left_dir.path().join("sub");
        std::fs::create_dir_all(&src_sub).unwrap();
        write(src_sub.join("file.txt"), "hello sub").unwrap();

        let dst_sub = right_dir.path().join("sub");
        copy_dir_recursive(&src_sub, &dst_sub).unwrap();

        assert!(dst_sub.join("file.txt").exists());
        assert_eq!(
            read_to_string(dst_sub.join("file.txt")).unwrap(),
            "hello sub"
        );

        // 2. Test execute_confirm_action (CopyLeftToRight)
        write(left_dir.path().join("test_copy.txt"), "copy content").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.selected_idx = 0;
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("test_copy.txt"),
            name: "test_copy.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }];
        app.apply_filter();

        app.show_confirm_modal = true;
        app.confirm_modal_action = Some(app::ConfirmAction::CopyLeftToRight);
        app.confirm_modal_message = "Copy test_copy.txt to right side?".to_string();

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let res = execute_confirm_action(&mut app, tx).await;
        assert!(res.is_ok());

        // Verify the file was copied to the right directory
        let copied_path = right_dir.path().join("test_copy.txt");
        assert!(copied_path.exists());
        assert_eq!(read_to_string(copied_path).unwrap(), "copy content");

        // Verify show_confirm_modal was reset
        assert!(!app.show_confirm_modal);

        // Verify success status message was set
        assert!(app.status_message.is_some());
        let (msg, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(!is_error, "Expected success status, got error");
        assert!(
            msg.contains("test_copy.txt"),
            "Status should mention the file name"
        );

        // Verify re-scan was triggered (message sent to rx)
        let msg = rx.recv().await;
        assert!(msg.is_some());
    }

    #[tokio::test]
    async fn test_copy_error_source_not_found() {
        use crate::diff::FileInfo;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        // Don't create the source file — it doesn't exist on disk
        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.selected_idx = 0;
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("nonexistent.txt"),
            name: "nonexistent.txt".to_string(),
            state: crate::diff::DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 100,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }];
        app.apply_filter();

        app.show_confirm_modal = true;
        app.confirm_modal_action = Some(app::ConfirmAction::CopyLeftToRight);

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let res = execute_confirm_action(&mut app, tx).await;
        // The function itself should not return Err — errors are captured in status
        assert!(res.is_ok());

        // Verify error status message was set
        assert!(app.status_message.is_some());
        let (msg, is_error, _) = app.status_message.as_ref().unwrap();
        assert!(is_error, "Expected error status");
        assert!(
            msg.contains("Copy failed"),
            "Status should indicate failure: {}",
            msg
        );

        // Verify NO re-scan was triggered (channel should be empty)
        assert!(
            rx.try_recv().is_err(),
            "Re-scan should not be triggered on copy failure"
        );
    }

    #[tokio::test]
    async fn test_copy_from_file_diff_view() {
        use crate::diff::FileInfo;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();

        write(left_dir.path().join("file.txt"), "left content").unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // First enter Diff View by pressing Enter
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Wait, then press 'R' to copy left to right
            tokio::time::sleep(Duration::from_millis(50)).await;
            let r_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('R'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(r_event)).await;

            // Wait, then press 'y' to confirm copy
            tokio::time::sleep(Duration::from_millis(50)).await;
            let y_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('y'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(y_event)).await;

            // Wait, then quit TUI
            tokio::time::sleep(Duration::from_millis(50)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        // Run the event loop
        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Verify it switched back to DirectoryTree
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));

        // Verify the file was copied to the right directory
        let copied_path = right_dir.path().join("file.txt");
        assert!(copied_path.exists());
        assert_eq!(read_to_string(copied_path).unwrap(), "left content");
    }

    #[tokio::test]
    async fn test_run_app_keyboard_swap_directories() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from(""),
            name: "root".to_string(),
            state: crate::diff::DiffState::Identical,
            left: None,
            right: None,
        }];
        app.apply_filter();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Press 's' to swap
            let s_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('s'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(s_event)).await;

            // Wait for scan to finish, then quit
            tokio::time::sleep(Duration::from_millis(100)).await;
            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert_eq!(app.left_path, PathBuf::from("left"));
        assert_eq!(app.right_path, PathBuf::from("right"));

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        // Paths should be swapped
        assert_eq!(app.left_path, PathBuf::from("right"));
        assert_eq!(app.right_path, PathBuf::from("left"));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_change_navigation() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("file.txt"),
            name: "file.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();
        app.view_mode = crate::app::ViewMode::FileDiff;
        app.diff_content_width = 38;
        app.diff_rows = vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "header".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "header".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "tail".to_string(),
                }),
                None,
            )),
        ];

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('N'),
                crossterm::event::KeyCode::Char('N'),
                crossterm::event::KeyCode::Char('P'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
    }

    #[tokio::test]
    async fn test_run_app_file_diff_wrap_and_horizontal_scroll() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use similar::ChangeTag;
        use std::time::SystemTime;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.flat_rows = vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("wide.txt"),
            name: "wide.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 15,
                modified: SystemTime::UNIX_EPOCH,
            }),
        }];
        app.apply_filter();

        // Pre-populate diff_rows with a long line so horizontal scrolling is meaningful.
        app.diff_rows = vec![DiffRow::from((
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Equal,
                text: "0123456789abcdefghijklmnopqrstuvwxyz".to_string(),
            }),
        ))];
        app.diff_max_line_width = 36;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Enter FileDiff mode
            let enter_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(enter_event)).await;

            // Toggle wrap mode on
            let w_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(w_event)).await;

            // Toggle wrap mode off
            let w_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(w_event)).await;

            // Scroll right horizontally
            let right_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Right,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(right_event)).await;

            // Scroll left horizontally
            let left_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Left,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(left_event)).await;

            // Exit FileDiff and quit
            let esc_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(esc_event)).await;

            let q_event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyModifiers::empty(),
            ));
            let _ = tx_clone.send(AppEvent::Terminal(q_event)).await;
        });

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        assert!(!app.diff_wrap);
        assert_eq!(app.diff_h_scroll, 0);

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());

        assert!(matches!(app.view_mode, crate::app::ViewMode::DirectoryTree));
        assert!(!app.diff_wrap);
        assert_eq!(app.diff_h_scroll, 0);
    }

    #[tokio::test]
    async fn test_help_opens_from_directory_tree_and_returns_on_esc() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_with_contextual_topic_and_return_view() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.view_mode = crate::app::ViewMode::FileDiff;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Open Help from FileDiff, then unwind back to DirectoryTree to quit:
            // Esc (Help -> FileDiff) -> q (FileDiff -> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // help_topic/help_return_view were set correctly when `?` was pressed from
        // FileDiff, and are still holding those values after the full unwind.
        assert_eq!(app.help_topic, crate::app::HelpTopic::FileDiff);
        assert_eq!(app.help_return_view, crate::app::ViewMode::FileDiff);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_opens_from_config_and_returns_to_directory_tree() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_config();

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? (-> Help) -> Esc (-> Config) -> q (-> DirectoryTree) -> q (break)
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Config);
        assert_eq!(app.help_return_view, crate::app::ViewMode::ConfigMenu);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_digit_key_jumps_topic_without_opening_index() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? (Help, topic=DirectoryTree) -> '4' (topic=Mouse) -> Esc -> q
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Char('4'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Mouse);
        assert!(!app.help_index_open);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_tab_opens_index_at_current_topic_position() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? -> '4' (jump to Mouse, pos 3) -> Tab (open index at sel=3) -> Esc -> q
            // Tests that Tab correctly maps current topic to its position in the index
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Char('4'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After jumping to '4' (Mouse at position 3) and pressing Tab, index should open at sel=3
        assert_eq!(app.help_index_sel, 3);
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
    }

    #[tokio::test]
    async fn test_help_index_navigation_wraps_both_directions() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Up-wrap: ? -> Tab (index open, sel=0) -> k (wraps to sel=4)
            // Down-wrap: j (wraps back from sel=4 to sel=0) -> j (sel=0 to sel=1) -> Esc -> q
            // This final j movement to sel=1 only happens if k/j navigation works;
            // it's a genuinely falsifiable assertion (would fail under old flat-match code).
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Char('k'),
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyCode::Char('j'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        // After 'k' from sel=0, wraps to sel=4 (up wraps to end)
        // After 'j' from sel=4, wraps back to sel=0 (down wraps to start)
        // After 'j' from sel=0, moves to sel=1 (normal forward move)
        // Only the current implementation produces sel=1; old flat-match code never navigates, stays at 0
        assert_eq!(app.help_index_sel, 1);
    }

    #[tokio::test]
    async fn test_help_index_digit_selects_topic_and_closes_index() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // ? -> Tab (open index) -> '3' (select Config, index at position 2) -> Esc -> q
            for code in [
                crossterm::event::KeyCode::Char('?'),
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyCode::Char('3'),
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.help_topic, crate::app::HelpTopic::Config);
        assert!(!app.help_index_open);
    }

    #[tokio::test]
    async fn test_help_esc_from_open_index_exits_help_entirely() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        // Directly seed app into (Help, index open) state, bypassing Tab key processing.
        // This isolates the test to verify Esc handler's help_index_open reset logic.
        // Under old flat-match code, Esc wouldn't reset help_index_open (only view_mode),
        // making assert!(!help_index_open) genuinely fail (RED).
        app.view_mode = crate::app::ViewMode::Help;
        app.help_return_view = crate::app::ViewMode::DirectoryTree;
        app.help_index_open = true;

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Esc (from index-open Help, should reset help_index_open) -> q (break)
            for code in [
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyCode::Char('q'),
            ] {
                let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
                    code,
                    crossterm::event::KeyModifiers::empty(),
                ));
                let _ = tx_clone.send(AppEvent::Terminal(event)).await;
            }
        });

        let res = run_app(&mut terminal, &mut app, &mut events, tx).await;
        assert!(res.is_ok());
        assert_eq!(app.view_mode, crate::app::ViewMode::DirectoryTree);
        // Verify that index mode was properly closed when exiting Help from index-open state.
        // This assertion independently verifies help_index_open reset without relying on Tab working.
        assert!(!app.help_index_open);
    }
}
