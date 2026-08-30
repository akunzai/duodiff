//! Canonical Command inventory, availability, execution, and outcomes.

use crate::actions::{diff_launch_outcome, dispatch_key_outcome, editor_launch_outcome, kick_scan};
use crate::app::{self, App};
use crate::event::AppEvent;
use ratatui::Terminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    ExternalDiff,
    SaveStaged,
    UndoStaged,
    ToggleTheme,
    ToggleFocus,
    FocusLeft,
    FocusRight,
    Expand,
    Collapse,
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
    StageLeftToRight,
    StageRightToLeft,
    Back,
    OpenRepository,
}

#[derive(Clone, Debug)]
pub struct CommandEntry {
    pub key: String,
    pub label: String,
    pub action_id: Command,
    pub disabled_reason: Option<&'static str>,
}

impl CommandEntry {
    pub fn new(key: &str, label: &str, command: Command) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            action_id: command,
            disabled_reason: None,
        }
    }

    pub fn gated(
        key: &str,
        label: &str,
        command: Command,
        available: bool,
        reason: &'static str,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            action_id: command,
            disabled_reason: (!available).then_some(reason),
        }
    }

    pub fn enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Palette,
}

#[derive(Clone, Debug)]
pub enum Invocation {
    Command(Command),
    Confirmation(app::ConfirmAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Message { text: String, is_error: bool },
    Unavailable { reason: &'static str },
    Failed { message: String },
    NeedsConfirmation,
    ExitRequested,
}

pub struct Commands {
    tx: tokio::sync::mpsc::Sender<AppEvent>,
    pending_target: Option<std::path::PathBuf>,
}

impl Commands {
    pub fn new(tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        Self {
            tx,
            pending_target: None,
        }
    }

    pub fn inventory(&self, app: &App, _surface: Surface) -> Vec<CommandEntry> {
        app.build_palette_actions()
    }

    pub fn execute<B: ratatui::backend::Backend>(
        &mut self,
        app: &mut App,
        invocation: Invocation,
        terminal: &mut Terminal<B>,
    ) -> Result<Outcome, Box<dyn std::error::Error>>
    where
        B::Error: 'static,
    {
        let Invocation::Command(command) = invocation else {
            let Invocation::Confirmation(action) = invocation else {
                unreachable!()
            };
            if !matches!(action, app::ConfirmAction::Cancel)
                && self.pending_target.as_deref()
                    != app.selected_row().map(|row| row.relative_path.as_path())
            {
                self.pending_target = None;
                let reason = "the original command target is no longer selected";
                app.set_status(reason, false);
                return Ok(Outcome::Unavailable { reason });
            }
            self.pending_target = None;
            crate::actions::execute_confirm_action(app, action, self.tx.clone())?;
            return Ok(if app.confirm_modal().is_some() {
                self.pending_target = app.selected_row().map(|row| row.relative_path.clone());
                Outcome::NeedsConfirmation
            } else {
                Outcome::Completed
            });
        };
        if command != Command::OpenRepository {
            if let Some(reason) = self
                .inventory(app, Surface::Palette)
                .into_iter()
                .find(|entry| entry.action_id == command)
                .and_then(|entry| entry.disabled_reason)
            {
                app.set_status(reason, false);
                return Ok(Outcome::Unavailable { reason });
            }
        }
        match command {
            Command::ExternalDiff => {
                dispatch_key_outcome(diff_launch_outcome(app), terminal, app.mouse_enabled())?
            }
            Command::ExternalEdit => {
                dispatch_key_outcome(editor_launch_outcome(app), terminal, app.mouse_enabled())?
            }
            Command::CopyLeftToRight => app.request_copy(app::ConfirmAction::CopyLeftToRight),
            Command::CopyRightToLeft => app.request_copy(app::ConfirmAction::CopyRightToLeft),
            Command::BuiltinDiff => {
                app.enter_file_diff();
            }
            Command::SwapPaths => {
                app.swap_paths();
                app.set_status("Swapped left ↔ right", false);
                kick_scan(app, self.tx.clone());
            }
            Command::ToggleScan => {
                if app.switch_scan_mode(app.scan_mode().toggled()) {
                    kick_scan(app, self.tx.clone());
                }
            }
            Command::Refresh => kick_scan(app, self.tx.clone()),
            Command::Config => app.open_config(),
            Command::Help => app.open_help(),
            Command::Filter => app.filter_mut().open(),
            Command::Quit => {
                app.request_quit();
                return Ok(Outcome::ExitRequested);
            }
            Command::ToggleWrap => app.diff_mut().toggle_wrap(),
            Command::ToggleFullDiff => {
                if let Err(error) = app.toggle_diff_show_full() {
                    app.set_status(format!("Cannot refresh diff: {error}"), true);
                }
            }
            Command::NextChange => app.jump_to_next_change(),
            Command::PrevChange => app.jump_to_prev_change(),
            Command::StageLeftToRight | Command::StageRightToLeft => {
                let (direction, side) = if command == Command::StageLeftToRight {
                    (crate::diff_view::HunkCopyDirection::LeftToRight, "right")
                } else {
                    (crate::diff_view::HunkCopyDirection::RightToLeft, "left")
                };
                match app.stage_hunk_at_cursor(direction) {
                    Ok(()) => {
                        app.set_status(format!("Staged change block to {side} — s to save"), false)
                    }
                    Err(error) => app.set_status(format!("Hunk copy failed: {error}"), true),
                }
            }
            Command::SaveStaged => app.request_save_staged(false),
            Command::UndoStaged => {
                if !app.undo_staged_hunk() {
                    app.set_status("Nothing to undo", false);
                }
            }
            Command::ToggleTheme => app.toggle_theme(),
            Command::ToggleFocus => app.toggle_active_side(),
            Command::FocusLeft => app.focus_left_pane(),
            Command::FocusRight => app.focus_right_pane(),
            Command::Expand => app.expand_selected(),
            Command::Collapse => app.collapse_selected(),
            Command::Back => match app.view_mode() {
                app::ViewMode::FileDiff => {
                    if !app.guard_staged_exit() {
                        app.leave_file_diff();
                    }
                }
                app::ViewMode::ConfigMenu => app.close_config(),
                _ => app.close_help(),
            },
            Command::OpenRepository => crate::actions::open_repo_url(app),
        }
        Ok(if app.confirm_modal().is_some() {
            self.pending_target = app.selected_row().map(|row| row.relative_path.clone());
            Outcome::NeedsConfirmation
        } else {
            Outcome::Completed
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn inventory_keeps_unavailable_commands_visible() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let commands = Commands::new(tx);
        let app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let open = commands
            .inventory(&app, Surface::Palette)
            .into_iter()
            .find(|entry| entry.action_id == Command::BuiltinDiff)
            .unwrap();
        assert_eq!(open.disabled_reason, Some("no row is selected"));
    }
}
