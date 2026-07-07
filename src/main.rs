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
pub mod settings;
pub mod ui;

#[derive(Parser, Debug)]
#[command(
    name = "duodiff",
    about = "A cross-platform TUI directory comparison tool"
)]
struct Args {
    left_dir: PathBuf,
    right_dir: PathBuf,
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
        terminal.draw(|f| ui::draw(f, app))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Terminal(crossterm::event::Event::Key(key)) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        use crossterm::event::KeyCode;
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
                                } else if app.context_menu.visible {
                                    match key.code {
                                        KeyCode::Esc | KeyCode::Char('q') => {
                                            app.context_menu.visible = false
                                        }
                                        KeyCode::Char('j') | KeyCode::Down => {
                                            app.context_menu.selected_idx =
                                                (app.context_menu.selected_idx + 1)
                                                    % app.context_menu.items.len();
                                        }
                                        KeyCode::Char('k') | KeyCode::Up => {
                                            app.context_menu.selected_idx = app
                                                .context_menu
                                                .selected_idx
                                                .checked_sub(1)
                                                .unwrap_or(app.context_menu.items.len() - 1);
                                        }
                                        KeyCode::Char('1') => {
                                            trigger_context_menu_action(
                                                0,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?
                                        }
                                        KeyCode::Char('2') => {
                                            trigger_context_menu_action(
                                                1,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?
                                        }
                                        KeyCode::Char('3') => {
                                            trigger_context_menu_action(
                                                2,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?
                                        }
                                        KeyCode::Char('4') => {
                                            app.context_menu.visible = false;
                                        }
                                        KeyCode::Enter => {
                                            trigger_context_menu_action(
                                                app.context_menu.selected_idx,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?
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
                                        KeyCode::Char('c') => {
                                            app.precise_mode = !app.precise_mode;
                                            app.scan_in_progress = true;
                                            start_scan_task(
                                                app.left_path.clone(),
                                                app.right_path.clone(),
                                                app.precise_mode,
                                                tx.clone(),
                                            );
                                        }
                                        KeyCode::Char('r') => {
                                            app.scan_in_progress = true;
                                            start_scan_task(
                                                app.left_path.clone(),
                                                app.right_path.clone(),
                                                app.precise_mode,
                                                tx.clone(),
                                            );
                                        }
                                        KeyCode::Char('C') => {
                                            app.view_mode = app::ViewMode::ConfigMenu;
                                        }
                                        KeyCode::Char('L')
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
                                            if row.right.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to left side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyRightToLeft);
                                            }
                                        }
                                        KeyCode::Char('R')
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
                                            if row.left.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to right side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyLeftToRight);
                                            }
                                        }
                                        KeyCode::Char('D')
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
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
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
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
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
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
                                                app.diff_rows = crate::diff_view::compare_files(
                                                    &left_file,
                                                    &right_file,
                                                )
                                                .unwrap_or_default();
                                                app.diff_left_hash =
                                                    crate::diff::compute_file_md5(&left_file).ok();
                                                app.diff_right_hash =
                                                    crate::diff::compute_file_md5(&right_file).ok();
                                                app.view_mode = app::ViewMode::FileDiff;
                                                app.diff_scroll = 0;
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
                                        KeyCode::Char('j') | KeyCode::Down => {
                                            let max_scroll = app
                                                .diff_rows
                                                .len()
                                                .saturating_sub(app.visible_height);
                                            if app.diff_scroll < max_scroll {
                                                app.diff_scroll += 1;
                                            }
                                        }
                                        KeyCode::Char('k') | KeyCode::Up if app.diff_scroll > 0 => {
                                            app.diff_scroll -= 1;
                                        }
                                        KeyCode::Char('L') | KeyCode::Char('l')
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
                                            if row.right.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to left side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyRightToLeft);
                                            }
                                        }
                                        KeyCode::Char('R') | KeyCode::Char('r')
                                            if app.selected_idx < app.flat_rows.len() =>
                                        {
                                            let row = &app.flat_rows[app.selected_idx];
                                            if row.left.is_some() {
                                                app.show_confirm_modal = true;
                                                app.confirm_modal_message =
                                                    format!("Copy '{}' to right side?", row.name);
                                                app.confirm_modal_action =
                                                    Some(app::ConfirmAction::CopyLeftToRight);
                                            }
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
                                    app.settings_menu_selected_idx = 0;
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    app.settings_menu_selected_idx = 0;
                                }
                                KeyCode::Enter if app.settings_menu_selected_idx == 0 => {
                                    app.view_mode = app::ViewMode::ConfigDiffTool;
                                }
                                _ => {}
                            },
                            app::ViewMode::ConfigDiffTool => match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    app.view_mode = app::ViewMode::ConfigMenu
                                }
                                KeyCode::Char('j') | KeyCode::Down => {
                                    if !app.detected_diff_tools.is_empty() {
                                        app.config_diff_tool_selected_idx =
                                            (app.config_diff_tool_selected_idx + 1)
                                                % app.detected_diff_tools.len();
                                    }
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    if !app.detected_diff_tools.is_empty() {
                                        app.config_diff_tool_selected_idx = app
                                            .config_diff_tool_selected_idx
                                            .checked_sub(1)
                                            .unwrap_or(app.detected_diff_tools.len() - 1);
                                    }
                                }
                                KeyCode::Char(' ') => {
                                    if !app.detected_diff_tools.is_empty() {
                                        let tool = &app.detected_diff_tools
                                            [app.config_diff_tool_selected_idx]
                                            .0;
                                        app.settings.external_diff_tool =
                                            Some(tool.as_str().to_string());
                                        let _ = app.settings.save();
                                    }
                                }
                                KeyCode::Enter => {
                                    if !app.detected_diff_tools.is_empty() {
                                        let tool = &app.detected_diff_tools
                                            [app.config_diff_tool_selected_idx]
                                            .0;
                                        app.settings.external_diff_tool =
                                            Some(tool.as_str().to_string());
                                        let _ = app.settings.save();
                                    }
                                    app.view_mode = app::ViewMode::ConfigMenu;
                                }
                                _ => {}
                            },
                        }
                    }
                }
                AppEvent::Terminal(crossterm::event::Event::Mouse(mouse)) => {
                    use crossterm::event::MouseEventKind;
                    match app.view_mode {
                        app::ViewMode::DirectoryTree => match mouse.kind {
                            MouseEventKind::ScrollDown => app.select_next(),
                            MouseEventKind::ScrollUp => app.select_prev(),
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                let size = terminal.size()?;
                                if mouse.row == 0 && mouse.column >= size.width.saturating_sub(16) {
                                    app.context_menu.visible = false;
                                    app.view_mode = app::ViewMode::ConfigMenu;
                                } else if app.context_menu.visible {
                                    let menu_w = 40;
                                    let menu_h = 8;
                                    let menu_x = size.width.saturating_sub(menu_w) / 2;
                                    let menu_y = size.height.saturating_sub(menu_h) / 2;

                                    if mouse.column >= menu_x
                                        && mouse.column < menu_x + menu_w
                                        && mouse.row >= menu_y
                                        && mouse.row < menu_y + menu_h
                                    {
                                        let item_y = mouse.row as i32 - (menu_y as i32 + 1);
                                        if (0..4).contains(&item_y) {
                                            trigger_context_menu_action(
                                                item_y as usize,
                                                app,
                                                terminal,
                                                tx.clone(),
                                            )
                                            .await?;
                                        }
                                    } else {
                                        app.context_menu.visible = false;
                                    }
                                } else {
                                    let click_y = mouse.row as usize;
                                    if click_y >= 3 {
                                        let offset_y = click_y - 3;
                                        if offset_y < app.visible_height {
                                            let idx = app.scroll_offset + offset_y;
                                            if idx < app.flat_rows.len() {
                                                let now = std::time::Instant::now();
                                                let is_double_click = Some(idx)
                                                    == app.last_click_idx
                                                    && app.last_click_time.is_some_and(|t| {
                                                        now.duration_since(t)
                                                            < std::time::Duration::from_millis(400)
                                                    });

                                                app.selected_idx = idx;

                                                if is_double_click {
                                                    let row = &app.flat_rows[app.selected_idx];
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
                                                        app.diff_rows =
                                                            crate::diff_view::compare_files(
                                                                &left_file,
                                                                &right_file,
                                                            )
                                                            .unwrap_or_default();
                                                        app.diff_left_hash =
                                                            crate::diff::compute_file_md5(
                                                                &left_file,
                                                            )
                                                            .ok();
                                                        app.diff_right_hash =
                                                            crate::diff::compute_file_md5(
                                                                &right_file,
                                                            )
                                                            .ok();
                                                        app.view_mode = app::ViewMode::FileDiff;
                                                        app.diff_scroll = 0;
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
                            }
                            MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
                                let click_y = mouse.row as usize;
                                if click_y >= 3 {
                                    let offset_y = click_y - 3;
                                    if offset_y < app.visible_height {
                                        let idx = app.scroll_offset + offset_y;
                                        if idx < app.flat_rows.len() {
                                            app.selected_idx = idx;
                                            app.context_menu.visible = true;
                                            app.context_menu.x = mouse.column;
                                            app.context_menu.y = mouse.row;
                                            app.context_menu.selected_idx = 0;
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
                            _ => {}
                        },
                        app::ViewMode::ConfigMenu => {
                            if let MouseEventKind::Down(crossterm::event::MouseButton::Left) =
                                mouse.kind
                            {
                                if mouse.row == 4 {
                                    app.settings_menu_selected_idx = 0;
                                    app.view_mode = app::ViewMode::ConfigDiffTool;
                                }
                            }
                        }
                        app::ViewMode::ConfigDiffTool => {
                            if let MouseEventKind::Down(crossterm::event::MouseButton::Left) =
                                mouse.kind
                            {
                                let click_y = mouse.row as usize;
                                if click_y >= 4 {
                                    let idx = click_y - 4;
                                    if idx < app.detected_diff_tools.len() {
                                        app.config_diff_tool_selected_idx = idx;
                                        let tool = &app.detected_diff_tools[idx].0;
                                        app.settings.external_diff_tool =
                                            Some(tool.as_str().to_string());
                                        let _ = app.settings.save();
                                    }
                                }
                            }
                        }
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

async fn trigger_context_menu_action<B: ratatui::backend::Backend>(
    action_idx: usize,
    app: &mut app::App,
    terminal: &mut ratatui::Terminal<B>,
    _tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    app.context_menu.visible = false;
    match action_idx {
        0 => {
            if app.selected_idx < app.flat_rows.len() {
                let row = &app.flat_rows[app.selected_idx];
                let is_dir = row.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
                    || row.right.as_ref().map(|f| f.is_dir).unwrap_or(false);
                if !is_dir && row.left.is_some() && row.right.is_some() {
                    if let Some(ref tool_str) = app.settings.external_diff_tool {
                        if let Ok(tool) = diff_tool::ExternalDiffTool::from_str(tool_str) {
                            let left_file = app.left_path.join(&row.relative_path);
                            let right_file = app.right_path.join(&row.relative_path);
                            run_external_diff(&tool, &left_file, &right_file, terminal)?;
                        }
                    }
                }
            }
        }
        1 => {
            if app.selected_idx < app.flat_rows.len() {
                let row = &app.flat_rows[app.selected_idx];
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
        2 => {
            app.view_mode = app::ViewMode::ConfigMenu;
        }
        _ => {}
    }
    Ok(())
}

async fn execute_confirm_action(
    app: &mut App,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    app.show_confirm_modal = false;
    if let Some(action) = app.confirm_modal_action.take() {
        if app.selected_idx < app.flat_rows.len() {
            let row = &app.flat_rows[app.selected_idx];
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
    if !args.left_dir.is_dir() || !args.right_dir.is_dir() {
        eprintln!("Both arguments must be valid directories.");
        std::process::exit(1);
    }

    // Initialize terminal safely
    let mut terminal = setup_terminal()?;

    let mut app = App::new(args.left_dir.clone(), args.right_dir.clone());
    let (mut events, tx) = EventHandler::new(Duration::from_millis(250));

    app.scan_in_progress = true;
    start_scan_task(
        args.left_dir.clone(),
        args.right_dir.clone(),
        app.precise_mode,
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
    tx: tokio::sync::mpsc::Sender<crate::event::AppEvent>,
) {
    tokio::spawn(async move {
        let root = tokio::task::spawn_blocking(move || {
            crate::diff::align_directories(&left, &right, std::path::Path::new(""), precise)
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

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // Click on the second row (click_y = 4, which maps to index 4 - 3 = 1)
            let mouse_event = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 4,
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

        let (mut events, tx) = EventHandler::new(Duration::from_millis(10));

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            // First click
            let click1 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 3,
                modifiers: crossterm::event::KeyModifiers::empty(),
            });
            let _ = tx_clone.send(AppEvent::Terminal(click1)).await;

            // Second click immediately
            let click2 = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 10,
                row: 3,
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
}
