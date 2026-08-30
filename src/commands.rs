//! Canonical Command inventory, availability, execution, and outcomes.

use crate::actions::{diff_launch_outcome, dispatch_key_outcome, editor_launch_outcome, kick_scan};
use crate::app::{self, App, ViewMode};
use crate::event::AppEvent;

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
    /// The key column comes from the keyboard adapter's binding table, so the
    /// Palette never restates a binding Commands does not own (ADR-0003).
    pub fn new(label: &str, command: Command) -> Self {
        Self {
            key: crate::input::key_hint(command),
            label: label.into(),
            action_id: command,
            disabled_reason: None,
        }
    }

    pub fn gated(label: &str, command: Command, available: bool, reason: &'static str) -> Self {
        Self {
            key: crate::input::key_hint(command),
            label: label.into(),
            action_id: command,
            disabled_reason: (!available).then_some(reason),
        }
    }

    pub fn enabled(&self) -> bool {
        self.disabled_reason.is_none()
    }
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

pub trait TerminalHandoff {
    fn dispatch(
        &mut self,
        outcome: crate::actions::KeyOutcome,
        mouse_enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

pub struct RatatuiTerminalHandoff<'a, B: ratatui::backend::Backend>(
    pub &'a mut ratatui::Terminal<B>,
);

impl<B: ratatui::backend::Backend> TerminalHandoff for RatatuiTerminalHandoff<'_, B>
where
    B::Error: 'static,
{
    fn dispatch(
        &mut self,
        outcome: crate::actions::KeyOutcome,
        mouse_enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        dispatch_key_outcome(outcome, self.0, mouse_enabled)
    }
}

impl<B: ratatui::backend::Backend> TerminalHandoff for ratatui::Terminal<B>
where
    B::Error: 'static,
{
    fn dispatch(
        &mut self,
        outcome: crate::actions::KeyOutcome,
        mouse_enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        dispatch_key_outcome(outcome, self, mouse_enabled)
    }
}

impl Commands {
    pub fn new(tx: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        Self {
            tx,
            pending_target: None,
        }
    }

    pub fn inventory(&self, app: &App) -> Vec<CommandEntry> {
        inventory_entries(app)
    }

    pub fn execute(
        &mut self,
        app: &mut App,
        invocation: Invocation,
        terminal: &mut dyn TerminalHandoff,
    ) -> Result<Outcome, Box<dyn std::error::Error>> {
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
                return Ok(Outcome::Unavailable { reason });
            }
            self.pending_target = None;
            app.dismiss_confirm();
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
                .inventory(app)
                .into_iter()
                .find(|entry| entry.action_id == command)
                .and_then(|entry| entry.disabled_reason)
            {
                return Ok(Outcome::Unavailable { reason });
            }
        }
        let mut outcome = Outcome::Completed;
        match command {
            Command::ExternalDiff => {
                terminal.dispatch(diff_launch_outcome(app), app.mouse_enabled())?
            }
            Command::ExternalEdit => {
                terminal.dispatch(editor_launch_outcome(app), app.mouse_enabled())?
            }
            Command::CopyLeftToRight => app.request_copy(app::ConfirmAction::CopyLeftToRight),
            Command::CopyRightToLeft => app.request_copy(app::ConfirmAction::CopyRightToLeft),
            Command::BuiltinDiff => {
                app.enter_file_diff();
            }
            Command::SwapPaths => {
                app.swap_paths();
                kick_scan(app, self.tx.clone());
                outcome = Outcome::Message {
                    text: "Swapped left ↔ right".into(),
                    is_error: false,
                };
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
                    outcome = Outcome::Failed {
                        message: format!("Cannot refresh diff: {error}"),
                    };
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
                        outcome = Outcome::Message {
                            text: format!("Staged change block to {side} — s to save"),
                            is_error: false,
                        }
                    }
                    Err(error) => {
                        outcome = Outcome::Failed {
                            message: format!("Hunk copy failed: {error}"),
                        }
                    }
                }
            }
            Command::SaveStaged => app.request_save_staged(false),
            Command::UndoStaged => {
                if !app.undo_staged_hunk() {
                    outcome = Outcome::Message {
                        text: "Nothing to undo".into(),
                        is_error: false,
                    };
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
            outcome
        })
    }
}

pub(crate) fn inventory_entries(app: &App) -> Vec<CommandEntry> {
    use crate::commands::{Command as Id, CommandEntry as A};

    let mut actions = Vec::new();
    match app.view_mode() {
        ViewMode::DirectoryTree => {
            let row = app.selected_row();
            let has_row = row.is_some();
            let is_dir = row.is_some_and(|r| r.is_dir());
            let is_file_pair =
                row.is_some_and(|r| !r.is_dir() && r.left.is_some() && r.right.is_some());
            let is_file_active = row.is_some_and(|r| {
                if app.active_side_left() {
                    r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                } else {
                    r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                }
            });
            // Every gated Directory Tree action falls back to the same
            // reason when nothing is selected at all.
            let reason = |specific: &'static str| {
                if has_row {
                    specific
                } else {
                    "no row is selected"
                }
            };

            actions.push(A::gated(
                "Open the diff view",
                Id::BuiltinDiff,
                row.is_some_and(|r| !r.is_dir()),
                reason("the selected row is a directory"),
            ));
            let effective_diff_tool = app.resolve_effective_diff_tool();
            let diff_tool_reason = if !is_file_pair {
                "needs a file present on both sides"
            } else {
                match &app.settings().external_diff_tool {
                    crate::settings::DiffToolSetting::Disabled => "external diff is disabled",
                    crate::settings::DiffToolSetting::Auto => "no external diff tool is available",
                    crate::settings::DiffToolSetting::Pinned(_) => {
                        "external diff tool is not available"
                    }
                    crate::settings::DiffToolSetting::Unknown(_) => {
                        "external diff tool is not available"
                    }
                }
            };
            actions.push(A::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                is_file_pair && effective_diff_tool.is_some(),
                diff_tool_reason,
            ));
            actions.push(A::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                is_file_active,
                "the focused pane has no file at this row",
            ));
            let copy_left_enabled =
                row.is_some_and(|r| r.left.is_some() && !r.is_ambiguous_case_collision);
            let copy_left_reason = if row.is_some_and(|r| r.is_ambiguous_case_collision) {
                "cannot copy: ambiguous case collision"
            } else {
                reason("nothing on the left side to copy")
            };
            actions.push(A::gated(
                "Copy the selection to the right pane",
                Id::CopyLeftToRight,
                copy_left_enabled,
                copy_left_reason,
            ));

            let copy_right_enabled =
                row.is_some_and(|r| r.right.is_some() && !r.is_ambiguous_case_collision);
            let copy_right_reason = if row.is_some_and(|r| r.is_ambiguous_case_collision) {
                "cannot copy: ambiguous case collision"
            } else {
                reason("nothing on the right side to copy")
            };
            actions.push(A::gated(
                "Copy the selection to the left pane",
                Id::CopyRightToLeft,
                copy_right_enabled,
                copy_right_reason,
            ));
            actions.push(A::gated(
                "Expand selected directory",
                Id::Expand,
                is_dir,
                reason("the selected row is not a directory"),
            ));
            actions.push(A::gated(
                "Collapse selected directory",
                Id::Collapse,
                is_dir,
                reason("the selected row is not a directory"),
            ));
            actions.push(A::new("Switch the focused pane", Id::ToggleFocus));
            actions.push(A::new("Focus the left pane", Id::FocusLeft));
            actions.push(A::new("Focus the right pane", Id::FocusRight));
            actions.push(A::new("Filter the tree", Id::Filter));
            actions.push(A::new("Swap the left and right directories", Id::SwapPaths));
            actions.push(A::new("Switch scan mode (Fast / Precise)", Id::ToggleScan));
            actions.push(A::new("Re-scan both directories", Id::Refresh));
            actions.push(A::new("Switch the light and dark theme", Id::ToggleTheme));
            actions.push(A::new("Open the Config screen", Id::Config));
            actions.push(A::new("Open Help", Id::Help));
            actions.push(A::new("Quit", Id::Quit));
        }
        ViewMode::FileDiff => {
            let has_changes = app.diff().has_changes();
            let row = app.selected_row();
            let is_file_pair =
                row.is_some_and(|r| !r.is_dir() && r.left.is_some() && r.right.is_some());
            let no_changes = "the two sides have no differing lines";

            actions.push(A::gated(
                "Jump to the next change block",
                Id::NextChange,
                has_changes,
                no_changes,
            ));
            actions.push(A::gated(
                "Jump to the previous change block",
                Id::PrevChange,
                has_changes,
                no_changes,
            ));
            actions.push(A::gated(
                "Stage the change block to the right",
                Id::StageLeftToRight,
                has_changes,
                no_changes,
            ));
            actions.push(A::gated(
                "Stage the change block to the left",
                Id::StageRightToLeft,
                has_changes,
                no_changes,
            ));
            actions.push(A::gated(
                "Copy the whole left file to the right",
                Id::CopyLeftToRight,
                row.is_some_and(|r| r.left.is_some() && !r.is_ambiguous_case_collision),
                if row.is_some_and(|r| r.is_ambiguous_case_collision) {
                    "cannot copy: ambiguous case collision"
                } else {
                    "nothing on the left side to copy"
                },
            ));
            actions.push(A::gated(
                "Copy the whole right file to the left",
                Id::CopyRightToLeft,
                row.is_some_and(|r| r.right.is_some() && !r.is_ambiguous_case_collision),
                if row.is_some_and(|r| r.is_ambiguous_case_collision) {
                    "cannot copy: ambiguous case collision"
                } else {
                    "nothing on the right side to copy"
                },
            ));
            let effective_diff_tool = app.resolve_effective_diff_tool();
            let diff_tool_reason = if !is_file_pair {
                "needs a file present on both sides"
            } else {
                match &app.settings().external_diff_tool {
                    crate::settings::DiffToolSetting::Disabled => "external diff is disabled",
                    crate::settings::DiffToolSetting::Auto => "no external diff tool is available",
                    crate::settings::DiffToolSetting::Pinned(_) => {
                        "external diff tool is not available"
                    }
                    crate::settings::DiffToolSetting::Unknown(_) => {
                        "external diff tool is not available"
                    }
                }
            };
            actions.push(A::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                is_file_pair && effective_diff_tool.is_some(),
                diff_tool_reason,
            ));
            actions.push(A::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                row.is_some_and(|r| {
                    if app.active_side_left() {
                        r.left.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    } else {
                        r.right.as_ref().map(|f| !f.is_dir).unwrap_or(false)
                    }
                }),
                "the focused pane has no file at this row",
            ));
            actions.push(A::gated(
                "Save staged changes",
                Id::SaveStaged,
                app.diff().is_dirty(),
                "no staged changes to save",
            ));
            actions.push(A::gated(
                "Undo last staged change block",
                Id::UndoStaged,
                app.diff().can_undo(),
                "nothing staged to undo",
            ));
            actions.push(A::new("Toggle line wrapping", Id::ToggleWrap));
            actions.push(A::new("Toggle full-file context", Id::ToggleFullDiff));
            actions.push(A::new("Switch the light and dark theme", Id::ToggleTheme));
            actions.push(A::new("Open the Config screen", Id::Config));
            actions.push(A::new("Open Help", Id::Help));
            actions.push(A::new("Return to the Directory Tree", Id::Back));
        }
        ViewMode::ConfigMenu | ViewMode::Help => {
            actions.push(A::new("Switch the light and dark theme", Id::ToggleTheme));
            if app.view_mode() == ViewMode::Help {
                actions.push(A::new("Open the Config screen", Id::Config));
            } else {
                actions.push(A::new("Open Help", Id::Help));
            }
            actions.push(A::new("Go back", Id::Back));
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[derive(Default)]
    struct FakeTerminalHandoff {
        calls: usize,
    }

    impl TerminalHandoff for FakeTerminalHandoff {
        fn dispatch(
            &mut self,
            _outcome: crate::actions::KeyOutcome,
            _mouse_enabled: bool,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.calls += 1;
            Ok(())
        }
    }

    #[test]
    fn inventory_keeps_unavailable_commands_visible() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let commands = Commands::new(tx);
        let app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let open = commands
            .inventory(&app)
            .into_iter()
            .find(|entry| entry.action_id == Command::BuiltinDiff)
            .unwrap();
        assert_eq!(open.disabled_reason, Some("no row is selected"));
    }

    #[test]
    fn execute_revalidates_and_returns_canonical_unavailable_outcome() {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut commands = Commands::new(tx);
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        let mut terminal = FakeTerminalHandoff::default();

        let outcome = commands
            .execute(
                &mut app,
                Invocation::Command(Command::BuiltinDiff),
                &mut terminal,
            )
            .unwrap();

        assert_eq!(
            outcome,
            Outcome::Unavailable {
                reason: "no row is selected"
            }
        );
        assert_eq!(terminal.calls, 0);
    }
}
