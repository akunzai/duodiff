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
    pub command: Command,
    pub disabled_reason: Option<&'static str>,
}

impl CommandEntry {
    /// The key column comes from the keyboard adapter's binding table, so the
    /// Palette never restates a binding Commands does not own (ADR-0003).
    pub fn new(label: &str, command: Command) -> Self {
        Self {
            key: crate::input::key_hint(command),
            label: label.into(),
            command,
            disabled_reason: None,
        }
    }

    pub fn gated(label: &str, command: Command, available: bool, reason: &'static str) -> Self {
        Self {
            key: crate::input::key_hint(command),
            label: label.into(),
            command,
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

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Completed,
    /// Completed, with one sentence naming what happened.
    Message {
        text: String,
    },
    /// Refused before any effect ran, with the reason. Informational, not an
    /// error — a Command that starts and then breaks reports [`Outcome::Failed`].
    Unavailable {
        message: String,
    },
    Failed {
        message: String,
    },
    /// Nothing ran yet: the user is asked first, with this prompt. The
    /// presenter puts it on screen (Issue #284).
    NeedsConfirmation {
        prompt: app::ConfirmModal,
    },
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
        dispatch_key_outcome::<B, crate::actions::RealTerminalGuard>(outcome, self.0, mouse_enabled)
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
        dispatch_key_outcome::<B, crate::actions::RealTerminalGuard>(outcome, self, mouse_enabled)
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
        match invocation {
            Invocation::Confirmation(action) => self.answer_confirmation(app, action),
            Invocation::Command(command) => self.run_command(app, command, terminal),
        }
    }

    /// Carry out the work a confirm dialog approved.
    ///
    /// The approval names one entry, so it is refused rather than redirected
    /// when the selection moved underneath it (Issue #282).
    fn answer_confirmation(
        &mut self,
        app: &mut App,
        action: app::ConfirmAction,
    ) -> Result<Outcome, Box<dyn std::error::Error>> {
        let target = self.pending_target.take();
        let approved = matches!(action, app::ConfirmAction::Cancel)
            || target.is_some_and(|target| {
                app.selected_row()
                    .is_some_and(|row| row.relative_path == target)
            });
        if !approved {
            // The dialog closes with it: leaving it open would trap the user,
            // since the approval it was showing can never be answered now.
            app.dismiss_confirm();
            return Ok(Outcome::Unavailable {
                message: "The confirmed entry is no longer selected — nothing was changed"
                    .to_string(),
            });
        }
        app.dismiss_confirm();
        let effect = crate::actions::execute_confirm_action(app, action, self.tx.clone())?;
        Ok(self.name_effect(app, effect))
    }

    /// Put the canonical sentence on what a confirmed action did.
    ///
    /// A save conflict is the one effect that answers with another question, so
    /// it asks it here rather than from inside the write.
    fn name_effect(&mut self, app: &App, effect: crate::actions::ConfirmEffect) -> Outcome {
        use crate::actions::ConfirmEffect as Effect;
        match effect {
            Effect::Nothing => Outcome::Completed,
            Effect::Saved => Outcome::Message {
                text: "Saved staged changes".to_string(),
            },
            Effect::SaveConflicted(paths) => self.confirm(app, save_conflict_prompt(&paths)),
            Effect::SaveFailed(error) => Outcome::Failed {
                message: format!("Save failed: {error}"),
            },
            Effect::Reloaded => Outcome::Message {
                text: "Reloaded from disk; staged changes discarded".to_string(),
            },
            Effect::ReloadFailed(error) => Outcome::Failed {
                message: format!("Reload failed: {error}"),
            },
            Effect::Copied(name) => Outcome::Message {
                text: format!("Copied '{name}'"),
            },
            Effect::CopyFailed(error) => Outcome::Failed {
                message: format!("Copy failed: {error}"),
            },
        }
    }

    fn run_command(
        &mut self,
        app: &mut App,
        command: Command,
        terminal: &mut dyn TerminalHandoff,
    ) -> Result<Outcome, Box<dyn std::error::Error>> {
        // Availability is re-read here, not trusted from whenever the inventory
        // was last listed, so a background rescan cannot leave a stale entry
        // runnable. A Command the active screen does not list is refused for
        // that reason alone. The Help repository link is deliberately outside
        // every inventory, so it is the one Command an entry does not gate
        // (Issue #282).
        if command != Command::OpenRepository {
            let Some(entry) = self
                .inventory(app)
                .into_iter()
                .find(|entry| entry.command == command)
            else {
                return Ok(Outcome::Unavailable {
                    message: "That command does not apply to this screen".to_string(),
                });
            };
            if let Some(reason) = entry.disabled_reason {
                return Ok(Outcome::Unavailable {
                    message: format!("{}: {reason}", entry.label),
                });
            }
        }
        let mut outcome = Outcome::Completed;
        match command {
            Command::ExternalDiff => match diff_launch_outcome(app) {
                Ok(launch) => terminal.dispatch(launch, app.mouse_enabled())?,
                Err(message) => outcome = Outcome::Failed { message },
            },
            Command::ExternalEdit => {
                terminal.dispatch(editor_launch_outcome(app), app.mouse_enabled())?
            }
            Command::CopyLeftToRight => {
                outcome = self.request_copy(app, app::CopyDirection::LeftToRight)
            }
            Command::CopyRightToLeft => {
                outcome = self.request_copy(app, app::CopyDirection::RightToLeft)
            }
            Command::BuiltinDiff => {
                app.enter_file_diff();
            }
            Command::SwapPaths => {
                app.swap_paths();
                kick_scan(app, self.tx.clone());
                outcome = Outcome::Message {
                    text: "Swapped left ↔ right".into(),
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
            Command::Filter => app.tree_list_mut().open(),
            Command::Quit => {
                app.request_quit();
                return Ok(Outcome::ExitRequested);
            }
            Command::ToggleWrap => app.diff_mut().toggle_wrap(),
            Command::ToggleFullDiff => app.toggle_diff_show_full(),
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
                        }
                    }
                    Err(error) => {
                        outcome = Outcome::Failed {
                            message: format!("Hunk copy failed: {error}"),
                        }
                    }
                }
            }
            Command::SaveStaged => outcome = self.confirm(app, staged_save_prompt(app)),
            Command::UndoStaged => {
                if !app.undo_staged_hunk() {
                    outcome = Outcome::Message {
                        text: "Nothing to undo".into(),
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
                // Never walk out on unwritten work: the dirty gate asks first
                // (Issue #235).
                app::ViewMode::FileDiff => {
                    if app.diff().is_dirty() {
                        outcome = self.confirm(app, staged_exit_prompt(app));
                    } else {
                        app.leave_file_diff();
                    }
                }
                app::ViewMode::ConfigMenu => app.close_config(),
                _ => app.close_help(),
            },
            Command::OpenRepository => {
                crate::actions::open_repo_url(self.tx.clone());
                outcome = Outcome::Message {
                    text: "Opening GitHub repository in the browser...".into(),
                };
            }
        }
        Ok(outcome)
    }

    /// Ask before doing anything, remembering the entry the answer will apply
    /// to so a selection that moves in the meantime cannot be acted on.
    ///
    /// A save conflict asks on top of the approval that reached it, so the
    /// pending target simply follows whichever question is now waiting.
    fn confirm(&mut self, app: &App, prompt: app::ConfirmModal) -> Outcome {
        self.pending_target = app.selected_row().map(|row| row.relative_path.clone());
        Outcome::NeedsConfirmation { prompt }
    }

    /// Preview a copy and ask about it, or say why there is nothing to ask.
    fn request_copy(&mut self, app: &App, direction: app::CopyDirection) -> Outcome {
        match app.preview_copy(direction) {
            Ok(preview) => self.confirm(app, copy_prompt(&preview, direction)),
            Err(refusal) => refused_copy(refusal),
        }
    }
}

/// The confirmation a copy raises: the operation, both absolute paths, and the
/// warning the destination's current state earns.
fn copy_prompt(preview: &app::CopyPreview, direction: app::CopyDirection) -> app::ConfirmModal {
    let operation = match preview.kind {
        app::CopyKind::Create => "Create",
        app::CopyKind::Overwrite => "Overwrite",
        app::CopyKind::Merge => "Merge",
    };
    let mut lines = vec![
        format!(
            "From   {}",
            App::display_path_with_home_tilde(&preview.source)
        ),
        format!(
            "To     {}",
            App::display_path_with_home_tilde(&preview.destination)
        ),
    ];
    if preview.case_mismatch {
        lines.push(String::new());
        lines.push(format!(
            "Note: Casing mismatch ('{}' vs '{}'). Destination spelling will be preserved.",
            preview.source_name, preview.destination_name
        ));
    }
    match preview.kind {
        app::CopyKind::Merge => {
            lines.push(String::new());
            lines.push(
                "Merges into the existing directory: colliding entries are overwritten, \
                 others are left in place. Only the entries this scan lists are copied."
                    .to_string(),
            );
        }
        app::CopyKind::Overwrite => {
            lines.push(String::new());
            lines.push("The destination already exists and will be replaced.".to_string());
        }
        app::CopyKind::Create => {}
    }
    app::ConfirmModal {
        title: "Confirm copy".to_string(),
        headline: format!("{operation} {}", preview.source_name),
        lines,
        choices: vec![
            app::ConfirmChoice {
                key: 'y',
                label: "Yes".to_string(),
                action: direction.confirmed(),
            },
            app::ConfirmChoice {
                key: 'n',
                label: "No".to_string(),
                action: app::ConfirmAction::Cancel,
            },
        ],
    }
}

/// What a copy refusal says.
///
/// Every one of these refuses before the copy starts, so they are informational
/// rather than errors — the same severity the availability gate already gives
/// an ambiguous case collision (Issue #282).
fn refused_copy(refusal: app::CopyRefusal) -> Outcome {
    match refusal {
        app::CopyRefusal::StagedChangesUnsaved => Outcome::Unavailable {
            message: "Staged changes are unsaved — press s to save or Esc to review them first"
                .to_string(),
        },
        app::CopyRefusal::NothingToCopy => Outcome::Completed,
        app::CopyRefusal::AmbiguousCaseCollision => Outcome::Unavailable {
            message: "Cannot copy: ambiguous case collision".to_string(),
        },
        app::CopyRefusal::AlreadyIdentical => Outcome::Message {
            text: "Files are already identical — nothing to copy".to_string(),
        },
    }
}

fn staged_target_lines(targets: &[std::path::PathBuf]) -> Vec<String> {
    targets
        .iter()
        .map(|target| format!("  {}", App::display_path_with_home_tilde(target)))
        .collect()
}

/// The confirmation a save raises, listing every destination it would write.
fn staged_save_prompt(app: &App) -> app::ConfirmModal {
    app::ConfirmModal {
        title: "Save staged changes".to_string(),
        headline: "Write the staged changes to:".to_string(),
        lines: staged_target_lines(&app.staged_save_targets()),
        choices: vec![
            app::ConfirmChoice {
                key: 's',
                label: "Save".to_string(),
                action: app::ConfirmAction::SaveStaged,
            },
            app::ConfirmChoice {
                key: 'c',
                label: "Cancel".to_string(),
                action: app::ConfirmAction::Cancel,
            },
        ],
    }
}

/// The dirty gate on the way out of a File Diff: save, discard, or stay.
fn staged_exit_prompt(app: &App) -> app::ConfirmModal {
    app::ConfirmModal {
        title: "Staged changes not saved".to_string(),
        headline: "This file diff has staged changes that are not written yet.".to_string(),
        lines: staged_target_lines(&app.staged_save_targets()),
        choices: vec![
            app::ConfirmChoice {
                key: 's',
                label: "Save".to_string(),
                action: app::ConfirmAction::SaveStagedThenLeave,
            },
            app::ConfirmChoice {
                key: 'd',
                label: "Discard".to_string(),
                action: app::ConfirmAction::DiscardStagedThenLeave,
            },
            app::ConfirmChoice {
                key: 'c',
                label: "Cancel".to_string(),
                action: app::ConfirmAction::Cancel,
            },
        ],
    }
}

/// The only two ways out of a save conflict. Force-overwrite is deliberately
/// not on the menu (Issue #235).
fn save_conflict_prompt(conflicted: &[std::path::PathBuf]) -> app::ConfirmModal {
    let mut lines = staged_target_lines(conflicted);
    lines.push(String::new());
    lines.push("Saving would overwrite those changes.".to_string());
    app::ConfirmModal {
        title: "Files changed on disk".to_string(),
        headline: "These files changed on disk since this diff was opened:".to_string(),
        lines,
        choices: vec![
            app::ConfirmChoice {
                key: 'r',
                label: "Reload, discarding staged changes".to_string(),
                action: app::ConfirmAction::ReloadDiscardStaged,
            },
            app::ConfirmChoice {
                key: 'c',
                label: "Cancel".to_string(),
                action: app::ConfirmAction::Cancel,
            },
        ],
    }
}

/// Whether the external diff tool can run on the selected row, and why not.
///
/// Both screens offer the Command against the same row, so they share one
/// answer rather than restating the tool-setting cascade.
fn external_diff_availability(app: &App) -> (bool, &'static str) {
    let is_file_pair = app
        .selected_row()
        .is_some_and(|row| !row.is_dir() && row.left.is_some() && row.right.is_some());
    if !is_file_pair {
        return (false, "needs a file present on both sides");
    }
    let reason = match &app.settings().external_diff_tool {
        crate::settings::DiffToolSetting::Disabled => "external diff is disabled",
        crate::settings::DiffToolSetting::Auto => "no external diff tool is available",
        crate::settings::DiffToolSetting::Pinned(_)
        | crate::settings::DiffToolSetting::Unknown(_) => "external diff tool is not available",
    };
    (app.resolve_effective_diff_tool().is_some(), reason)
}

/// Whether one copy direction can run on the selected row, and why not.
///
/// `absent` names the empty side, so each screen keeps its own wording for a
/// whole entry versus a whole file.
fn copy_availability(app: &App, left_to_right: bool, absent: &'static str) -> (bool, &'static str) {
    let Some(row) = app.selected_row() else {
        return (false, "no row is selected");
    };
    if row.is_ambiguous_case_collision {
        return (false, "cannot copy: ambiguous case collision");
    }
    let source = if left_to_right { &row.left } else { &row.right };
    (source.is_some(), absent)
}

pub(crate) fn inventory_entries(app: &App) -> Vec<CommandEntry> {
    use crate::commands::{Command as Id, CommandEntry as Entry};

    let mut commands = Vec::new();
    match app.view_mode() {
        ViewMode::DirectoryTree => {
            let row = app.selected_row();
            let has_row = row.is_some();
            let is_dir = row.is_some_and(|r| r.is_dir());
            // Every gated Directory Tree action falls back to the same
            // reason when nothing is selected at all.
            let reason = |specific: &'static str| {
                if has_row {
                    specific
                } else {
                    "no row is selected"
                }
            };

            commands.push(Entry::gated(
                "Open the diff view",
                Id::BuiltinDiff,
                row.is_some_and(|r| !r.is_dir()),
                reason("the selected row is a directory"),
            ));
            let (diff_tool_ready, diff_tool_reason) = external_diff_availability(app);
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                diff_tool_ready,
                diff_tool_reason,
            ));
            commands.push(Entry::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                app.active_side_has_file(),
                "the focused pane has no file at this row",
            ));
            let (copy_left, copy_left_reason) =
                copy_availability(app, true, reason("nothing on the left side to copy"));
            commands.push(Entry::gated(
                "Copy the selection to the right pane",
                Id::CopyLeftToRight,
                copy_left,
                copy_left_reason,
            ));
            let (copy_right, copy_right_reason) =
                copy_availability(app, false, reason("nothing on the right side to copy"));
            commands.push(Entry::gated(
                "Copy the selection to the left pane",
                Id::CopyRightToLeft,
                copy_right,
                copy_right_reason,
            ));
            commands.push(Entry::gated(
                "Expand selected directory",
                Id::Expand,
                is_dir,
                reason("the selected row is not a directory"),
            ));
            commands.push(Entry::gated(
                "Collapse selected directory",
                Id::Collapse,
                is_dir,
                reason("the selected row is not a directory"),
            ));
            commands.push(Entry::new("Switch the focused pane", Id::ToggleFocus));
            commands.push(Entry::new("Focus the left pane", Id::FocusLeft));
            commands.push(Entry::new("Focus the right pane", Id::FocusRight));
            commands.push(Entry::new("Filter the tree", Id::Filter));
            commands.push(Entry::new(
                "Swap the left and right directories",
                Id::SwapPaths,
            ));
            commands.push(Entry::new(
                "Switch scan mode (Fast / Precise)",
                Id::ToggleScan,
            ));
            commands.push(Entry::new("Re-scan both directories", Id::Refresh));
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
            ));
            commands.push(Entry::new("Open the Config screen", Id::Config));
            commands.push(Entry::new("Open Help", Id::Help));
            commands.push(Entry::new("Quit", Id::Quit));
        }
        ViewMode::FileDiff => {
            let has_changes = app.diff().has_changes();
            let no_changes = "the two sides have no differing lines";

            commands.push(Entry::gated(
                "Jump to the next change block",
                Id::NextChange,
                has_changes,
                no_changes,
            ));
            commands.push(Entry::gated(
                "Jump to the previous change block",
                Id::PrevChange,
                has_changes,
                no_changes,
            ));
            commands.push(Entry::gated(
                "Stage the change block to the right",
                Id::StageLeftToRight,
                has_changes,
                no_changes,
            ));
            commands.push(Entry::gated(
                "Stage the change block to the left",
                Id::StageRightToLeft,
                has_changes,
                no_changes,
            ));
            let (copy_left, copy_left_reason) =
                copy_availability(app, true, "nothing on the left side to copy");
            commands.push(Entry::gated(
                "Copy the whole left file to the right",
                Id::CopyLeftToRight,
                copy_left,
                copy_left_reason,
            ));
            let (copy_right, copy_right_reason) =
                copy_availability(app, false, "nothing on the right side to copy");
            commands.push(Entry::gated(
                "Copy the whole right file to the left",
                Id::CopyRightToLeft,
                copy_right,
                copy_right_reason,
            ));
            let (diff_tool_ready, diff_tool_reason) = external_diff_availability(app);
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                diff_tool_ready,
                diff_tool_reason,
            ));
            commands.push(Entry::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                app.active_side_has_file(),
                "the focused pane has no file at this row",
            ));
            commands.push(Entry::gated(
                "Save staged changes",
                Id::SaveStaged,
                app.diff().is_dirty(),
                "no staged changes to save",
            ));
            commands.push(Entry::gated(
                "Undo last staged change block",
                Id::UndoStaged,
                app.diff().can_undo(),
                "nothing staged to undo",
            ));
            commands.push(Entry::new("Toggle line wrapping", Id::ToggleWrap));
            commands.push(Entry::new("Toggle full-file context", Id::ToggleFullDiff));
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
            ));
            commands.push(Entry::new("Open the Config screen", Id::Config));
            commands.push(Entry::new("Open Help", Id::Help));
            commands.push(Entry::new("Return to the Directory Tree", Id::Back));
        }
        ViewMode::ConfigMenu | ViewMode::Help => {
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
            ));
            if app.view_mode() == ViewMode::Help {
                commands.push(Entry::new("Open the Config screen", Id::Config));
            } else {
                commands.push(Entry::new("Open Help", Id::Help));
            }
            commands.push(Entry::new("Go back", Id::Back));
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{AlignedNode, DiffState, FileInfo};
    use std::path::PathBuf;
    use std::time::SystemTime;

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

    /// A `Commands` and the `App` it drives, wired to a channel the test can read.
    struct Harness {
        commands: Commands,
        app: App,
        terminal: FakeTerminalHandoff,
        _rx: tokio::sync::mpsc::Receiver<AppEvent>,
    }

    impl Harness {
        fn new() -> Self {
            Self::rooted(PathBuf::from("left"), PathBuf::from("right"))
        }

        fn rooted(left: PathBuf, right: PathBuf) -> Self {
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            Self {
                commands: Commands::new(tx),
                app: App::new(left, right),
                terminal: FakeTerminalHandoff::default(),
                _rx: rx,
            }
        }

        fn run(&mut self, command: Command) -> Outcome {
            let outcome = self
                .commands
                .execute(
                    &mut self.app,
                    Invocation::Command(command),
                    &mut self.terminal,
                )
                .unwrap();
            self.present(outcome)
        }

        fn answer(&mut self, action: app::ConfirmAction) -> Outcome {
            let outcome = self
                .commands
                .execute(
                    &mut self.app,
                    Invocation::Confirmation(action),
                    &mut self.terminal,
                )
                .unwrap();
            self.present(outcome)
        }

        /// Do what the input adapter's presenter does with a prompt, so the
        /// confirmation lifecycle tests see the dialog the user would.
        fn present(&mut self, outcome: Outcome) -> Outcome {
            if let Outcome::NeedsConfirmation { prompt } = &outcome {
                self.app.show_confirm(prompt.clone());
            }
            outcome
        }

        fn inventory(&self) -> Vec<CommandEntry> {
            self.commands.inventory(&self.app)
        }

        fn reason_for(&self, command: Command) -> Option<&'static str> {
            self.inventory()
                .into_iter()
                .find(|entry| entry.command == command)
                .unwrap_or_else(|| panic!("{command:?} is not listed on this screen"))
                .disabled_reason
        }

        fn lists(&self, command: Command) -> bool {
            self.inventory()
                .iter()
                .any(|entry| entry.command == command)
        }
    }

    fn file_info(is_dir: bool) -> FileInfo {
        FileInfo {
            is_dir,
            size: 0,
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn entry_node(name: &str, is_dir: bool, children: Vec<AlignedNode>) -> AlignedNode {
        AlignedNode {
            name: name.to_string(),
            relative_path: PathBuf::from(name),
            left: Some(file_info(is_dir)),
            right: Some(file_info(is_dir)),
            state: DiffState::Identical,
            is_expanded: false,
            children,
            ..Default::default()
        }
    }

    /// A file that differs between the sides, so a copy has something to do.
    fn differing_node(name: &str) -> AlignedNode {
        AlignedNode {
            state: DiffState::DifferentNewerLeft,
            ..entry_node(name, false, Vec::new())
        }
    }

    /// A scan result holding `children` under the (unnamed) root.
    fn scanned(children: Vec<AlignedNode>) -> AlignedNode {
        AlignedNode {
            left: Some(file_info(true)),
            right: Some(file_info(true)),
            state: DiffState::Identical,
            is_expanded: true,
            children,
            ..Default::default()
        }
    }

    #[test]
    fn inventory_keeps_unavailable_commands_visible() {
        let harness = Harness::new();
        assert_eq!(
            harness.reason_for(Command::BuiltinDiff),
            Some("no row is selected")
        );
    }

    /// Issue #282: the inventory does not change shape with the selection, so a
    /// Command belonging to the screen stays listed while it cannot run.
    #[test]
    fn inventory_shape_is_stable_across_selection_changes() {
        let mut harness = Harness::new();
        let empty: Vec<Command> = harness
            .inventory()
            .iter()
            .map(|entry| entry.command)
            .collect();

        harness
            .app
            .set_root_node(scanned(vec![entry_node("a.txt", false, Vec::new())]));
        let selected: Vec<Command> = harness
            .inventory()
            .iter()
            .map(|entry| entry.command)
            .collect();

        assert_eq!(empty, selected);
        assert_eq!(harness.reason_for(Command::BuiltinDiff), None);
    }

    /// Issue #239: each screen lists the Commands that belong to it, and Config
    /// and Help list their applicable Theme / Config / Help / Back entries
    /// rather than the old two-entry fallback.
    #[test]
    fn inventory_membership_is_per_screen() {
        let mut harness = Harness::new();
        for expected in [
            Command::Quit,
            Command::Help,
            Command::Refresh,
            Command::ToggleTheme,
            Command::ToggleFocus,
            Command::FocusLeft,
            Command::Expand,
        ] {
            assert!(
                harness.lists(expected),
                "the Directory Tree must list {expected:?}"
            );
        }

        harness.app.set_view_mode(ViewMode::FileDiff);
        for expected in [
            Command::ToggleWrap,
            Command::ToggleFullDiff,
            Command::NextChange,
            Command::PrevChange,
            Command::StageLeftToRight,
            Command::StageRightToLeft,
            Command::ExternalDiff,
            Command::ExternalEdit,
            Command::Config,
            Command::ToggleTheme,
            Command::Back,
        ] {
            assert!(harness.lists(expected), "File Diff must list {expected:?}");
        }

        harness.app.set_view_mode(ViewMode::ConfigMenu);
        assert_eq!(
            harness
                .inventory()
                .iter()
                .map(|entry| entry.command)
                .collect::<Vec<_>>(),
            vec![Command::ToggleTheme, Command::Help, Command::Back]
        );
        assert!(harness.inventory().iter().all(|entry| entry.enabled()));

        harness.app.set_view_mode(ViewMode::Help);
        assert_eq!(
            harness
                .inventory()
                .iter()
                .map(|entry| entry.command)
                .collect::<Vec<_>>(),
            vec![Command::ToggleTheme, Command::Config, Command::Back]
        );
        assert!(harness.inventory().iter().all(|entry| entry.enabled()));
    }

    /// Issue #239: with nothing selected, the row-dependent Commands stay listed
    /// with their reason while the screen-wide ones remain runnable.
    #[test]
    fn an_empty_tree_gates_the_row_commands_and_leaves_the_rest_runnable() {
        let harness = Harness::new();
        for gated in [
            Command::BuiltinDiff,
            Command::CopyLeftToRight,
            Command::CopyRightToLeft,
            Command::Expand,
        ] {
            assert_eq!(
                harness.reason_for(gated),
                Some("no row is selected"),
                "{gated:?} should stay listed with its reason"
            );
        }
        assert_eq!(harness.reason_for(Command::Quit), None);
    }

    #[test]
    fn inventory_lists_only_the_active_screen_and_never_the_repository_link() {
        let mut harness = Harness::new();
        assert!(harness.lists(Command::Quit));
        assert!(harness.lists(Command::Filter));
        assert!(!harness.lists(Command::Back));
        assert!(!harness.lists(Command::SaveStaged));

        harness.app.set_view_mode(ViewMode::FileDiff);
        assert!(harness.lists(Command::Back));
        assert!(harness.lists(Command::SaveStaged));
        assert!(!harness.lists(Command::Quit));
        assert!(!harness.lists(Command::Filter));

        for view_mode in [
            ViewMode::DirectoryTree,
            ViewMode::FileDiff,
            ViewMode::ConfigMenu,
            ViewMode::Help,
        ] {
            harness.app.set_view_mode(view_mode);
            assert!(
                !harness.lists(Command::OpenRepository),
                "the Help repository link must stay out of the Palette on {view_mode:?}"
            );
        }
    }

    #[test]
    fn execute_revalidates_and_returns_canonical_unavailable_outcome() {
        let mut harness = Harness::new();

        let outcome = harness.run(Command::BuiltinDiff);

        assert_eq!(
            outcome,
            Outcome::Unavailable {
                message: "Open the diff view: no row is selected".to_string()
            }
        );
        assert_eq!(harness.terminal.calls, 0);
    }

    /// Issue #282: a Palette entry captured before a background scan must not
    /// perform an operation the current state no longer allows.
    #[test]
    fn execute_revalidates_against_state_that_changed_after_the_inventory() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![entry_node("a.txt", false, Vec::new())]));
        assert_eq!(harness.reason_for(Command::BuiltinDiff), None);

        // The rescan replaced the file with a directory under the same index.
        harness
            .app
            .set_root_node(scanned(vec![entry_node("a", true, Vec::new())]));

        assert_eq!(
            harness.run(Command::BuiltinDiff),
            Outcome::Unavailable {
                message: "Open the diff view: the selected row is a directory".to_string()
            }
        );
        assert_eq!(harness.app.view_mode(), ViewMode::DirectoryTree);
    }

    /// Issue #282: Expand and Collapse name a target state, so repeating one is
    /// idempotent rather than a toggle.
    #[test]
    fn expand_and_collapse_are_explicit_target_states() {
        let mut harness = Harness::new();
        harness.app.set_root_node(scanned(vec![entry_node(
            "dir",
            true,
            vec![entry_node("child.txt", false, Vec::new())],
        )]));
        assert_eq!(harness.app.flat_rows().len(), 1);

        assert_eq!(harness.run(Command::Expand), Outcome::Completed);
        assert_eq!(harness.app.flat_rows().len(), 2, "the child is now listed");
        assert_eq!(harness.run(Command::Expand), Outcome::Completed);
        assert_eq!(harness.app.flat_rows().len(), 2, "Expand never collapses");

        assert_eq!(harness.run(Command::Collapse), Outcome::Completed);
        assert_eq!(harness.app.flat_rows().len(), 1);
        assert_eq!(harness.run(Command::Collapse), Outcome::Completed);
        assert_eq!(harness.app.flat_rows().len(), 1, "Collapse never expands");
    }

    #[test]
    fn expand_and_collapse_report_why_a_file_row_cannot_take_them() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![entry_node("a.txt", false, Vec::new())]));

        for (command, label) in [
            (Command::Expand, "Expand selected directory"),
            (Command::Collapse, "Collapse selected directory"),
        ] {
            assert_eq!(
                harness.run(command),
                Outcome::Unavailable {
                    message: format!("{label}: the selected row is not a directory")
                }
            );
        }
    }

    /// Issue #282: Back and Quit are distinct Commands — Back leaves a screen,
    /// Quit ends the session, and neither stands in for the other.
    #[test]
    fn back_leaves_the_screen_and_quit_ends_the_session() {
        let mut harness = Harness::new();
        harness.app.open_config();
        assert_eq!(harness.app.view_mode(), ViewMode::ConfigMenu);

        assert_eq!(harness.run(Command::Back), Outcome::Completed);
        assert_eq!(harness.app.view_mode(), ViewMode::DirectoryTree);
        assert!(!harness.app.should_quit(), "Back never quits");

        assert_eq!(harness.run(Command::Quit), Outcome::ExitRequested);
        assert!(harness.app.should_quit());
    }

    #[tokio::test]
    async fn commands_own_their_canonical_message_and_severity() {
        let mut harness = Harness::new();

        assert_eq!(
            harness.run(Command::SwapPaths),
            Outcome::Message {
                text: "Swapped left ↔ right".to_string(),
            }
        );

        // Unavailability is informational — it carries a reason, not an error.
        harness.app.set_view_mode(ViewMode::FileDiff);
        assert_eq!(
            harness.run(Command::StageLeftToRight),
            Outcome::Unavailable {
                message: "Stage the change block to the right: the two sides have no differing \
                          lines"
                    .to_string()
            }
        );
    }

    /// Issue #282: a failure after an effect has begun is an error, distinct
    /// from the informational unavailability of a Command that never ran.
    #[tokio::test]
    async fn an_effect_that_breaks_after_it_starts_is_a_failure() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        // The scan listed the entry, but it is gone from disk by the time the
        // copy is approved.
        harness
            .app
            .set_root_node(scanned(vec![differing_node("gone.txt")]));

        assert!(matches!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation { .. }
        ));
        let outcome = harness.answer(app::ConfirmAction::CopyLeftToRight);

        let Outcome::Failed { message } = outcome else {
            panic!("expected a failure outcome, got {outcome:?}");
        };
        assert!(
            message.starts_with("Copy failed:"),
            "unexpected message: {message}"
        );
    }

    /// Issue #282: external tools run through the borrowed terminal handoff, and
    /// an unavailable Command never reaches it.
    #[test]
    fn the_terminal_handoff_is_used_only_by_an_available_external_command() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![entry_node("a.txt", false, Vec::new())]));

        assert_eq!(harness.run(Command::ExternalEdit), Outcome::Completed);
        assert_eq!(harness.terminal.calls, 1);

        harness
            .app
            .set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
        assert_eq!(
            harness.run(Command::ExternalDiff),
            Outcome::Unavailable {
                message: "Compare with the external diff tool: external diff is disabled"
                    .to_string()
            }
        );
        assert_eq!(
            harness.terminal.calls, 1,
            "no handoff for an unavailable Command"
        );
    }

    #[test]
    fn a_copy_asks_for_confirmation_before_it_touches_anything() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![differing_node("a.txt")]));

        assert!(matches!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation { .. }
        ));
        assert!(harness.app.confirm_modal().is_some());
    }

    #[test]
    fn cancelling_a_confirmation_runs_no_effect() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![differing_node("a.txt")]));
        harness.run(Command::CopyLeftToRight);

        assert_eq!(
            harness.answer(app::ConfirmAction::Cancel),
            Outcome::Completed
        );
        assert!(harness.app.confirm_modal().is_none());
        assert!(
            !harness.app.right_path().join("a.txt").exists(),
            "cancelling must not copy"
        );
    }

    /// Issue #282: confirmation stays tied to the entry the user approved, so a
    /// selection change between prompt and answer cancels instead of redirecting.
    #[test]
    fn a_confirmation_refuses_a_target_that_drifted() {
        let mut harness = Harness::new();
        harness.app.set_root_node(scanned(vec![
            differing_node("a.txt"),
            differing_node("b.txt"),
        ]));
        harness.run(Command::CopyLeftToRight);

        harness.app.set_selected_idx(1);

        assert_eq!(
            harness.answer(app::ConfirmAction::CopyLeftToRight),
            Outcome::Unavailable {
                message: "The confirmed entry is no longer selected — nothing was changed"
                    .to_string()
            }
        );
        // The dialog closes with the refusal: an approval that can never be
        // answered must not trap the user in a modal.
        assert!(harness.app.confirm_modal().is_none());
    }

    /// Issue #282: execution re-evaluates availability, and a Command the
    /// active screen does not list is refused rather than run unchecked.
    #[test]
    fn a_command_outside_the_active_screen_is_refused() {
        let mut harness = Harness::new();
        harness.app.open_config();
        assert!(!harness.lists(Command::Quit));

        assert_eq!(
            harness.run(Command::Quit),
            Outcome::Unavailable {
                message: "That command does not apply to this screen".to_string()
            }
        );
        assert!(!harness.app.should_quit());
        assert_eq!(harness.app.view_mode(), ViewMode::ConfigMenu);
    }

    /// Issue #282: the approval names an entry, so an answer with no pending
    /// continuation is refused instead of passing on a vacuous `None == None`.
    #[test]
    fn an_answer_without_a_pending_approval_is_refused() {
        let mut harness = Harness::new();

        assert_eq!(
            harness.answer(app::ConfirmAction::CopyLeftToRight),
            Outcome::Unavailable {
                message: "The confirmed entry is no longer selected — nothing was changed"
                    .to_string()
            }
        );
    }

    /// Issue #282: a confirmed copy reports one canonical result, and the
    /// approval it consumed cannot be replayed against a new selection.
    #[tokio::test]
    async fn a_confirmed_copy_reports_the_entry_and_consumes_the_approval() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("a.txt"), "left").unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        harness.app.set_root_node(scanned(vec![
            differing_node("a.txt"),
            differing_node("b.txt"),
        ]));

        assert!(matches!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation { .. }
        ));
        assert_eq!(
            harness.answer(app::ConfirmAction::CopyLeftToRight),
            Outcome::Message {
                text: "Copied 'a.txt'".to_string(),
            }
        );
        assert_eq!(
            std::fs::read_to_string(right.path().join("a.txt")).unwrap(),
            "left"
        );

        // The approval is spent: answering again after moving the selection is
        // refused rather than copying a second entry.
        harness.app.set_selected_idx(1);
        assert_eq!(
            harness.answer(app::ConfirmAction::CopyLeftToRight),
            Outcome::Unavailable {
                message: "The confirmed entry is no longer selected — nothing was changed"
                    .to_string()
            }
        );
        assert!(!right.path().join("b.txt").exists());
    }

    /// A File Diff on `merge.txt`, whose first row is an identical `keep` line
    /// and whose second row is the one change block. Reached entirely through
    /// the Commands interface.
    fn merge_file_diff() -> (Harness, tempfile::TempDir, tempfile::TempDir) {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        std::fs::write(right.path().join("merge.txt"), "keep\nright-line\n").unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        harness
            .app
            .set_root_node(scanned(vec![entry_node("merge.txt", false, Vec::new())]));

        assert_eq!(harness.run(Command::BuiltinDiff), Outcome::Completed);
        assert_eq!(harness.app.view_mode(), ViewMode::FileDiff);
        assert_eq!(harness.run(Command::ToggleFullDiff), Outcome::Completed);
        (harness, left, right)
    }

    /// [`merge_file_diff`] with its one change block staged to the right.
    fn staged_file_diff() -> (Harness, tempfile::TempDir, tempfile::TempDir) {
        let (mut harness, left, right) = merge_file_diff();
        harness.app.diff_mut().set_scroll(1);
        assert_eq!(
            harness.run(Command::StageLeftToRight),
            Outcome::Message {
                text: "Staged change block to right — s to save".to_string(),
            }
        );
        assert!(harness.app.diff().is_dirty());
        (harness, left, right)
    }

    /// The prompt a pending confirm dialog is showing.
    fn prompt(harness: &Harness) -> app::ConfirmModal {
        harness
            .app
            .confirm_modal()
            .expect("a confirmation should be pending")
            .clone()
    }

    fn choices(modal: &app::ConfirmModal) -> Vec<(char, &str, app::ConfirmAction)> {
        modal
            .choices
            .iter()
            .map(|choice| (choice.key, choice.label.as_str(), choice.action.clone()))
            .collect()
    }

    /// Issue #284: the copy dialog names the operation and both absolute paths,
    /// so the write is never a surprise.
    #[test]
    fn a_copy_confirmation_names_the_operation_and_both_paths() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("a.txt"), "left").unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        harness
            .app
            .set_root_node(scanned(vec![differing_node("a.txt")]));

        assert!(matches!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation { .. }
        ));
        let modal = prompt(&harness);
        assert_eq!(modal.title, "Confirm copy");
        assert_eq!(modal.headline, "Create a.txt");
        assert!(
            modal.lines[0].starts_with("From   ") && modal.lines[0].ends_with("a.txt"),
            "unexpected source line: {}",
            modal.lines[0]
        );
        assert!(
            modal.lines[1].starts_with("To     ") && modal.lines[1].ends_with("a.txt"),
            "unexpected destination line: {}",
            modal.lines[1]
        );
        assert_eq!(modal.lines.len(), 2, "a create needs no extra warning");
        assert_eq!(
            choices(&modal),
            vec![
                ('y', "Yes", app::ConfirmAction::CopyLeftToRight),
                ('n', "No", app::ConfirmAction::Cancel),
            ]
        );
    }

    /// Issue #284: every dialog abbreviates the user's home as `~`, so a long
    /// path stays legible in a narrow popup.
    #[tokio::test]
    async fn confirm_dialogs_abbreviate_the_home_directory_as_a_tilde() {
        let _guard = crate::test_support::ConfigEnvGuard::new();
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
        let left = home.join("proj-a");
        let right = home.join("proj-b");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join("foo.txt"), "left").unwrap();

        let mut harness = Harness::rooted(left, right);
        harness
            .app
            .set_root_node(scanned(vec![differing_node("foo.txt")]));

        harness.run(Command::CopyLeftToRight);
        let modal = prompt(&harness);
        assert_eq!(modal.lines[0], "From   ~/proj-a/foo.txt");
        assert_eq!(modal.lines[1], "To     ~/proj-b/foo.txt");

        // The staged dialogs abbreviate the same way.
        harness.app.dismiss_confirm();
        harness.app.set_view_mode(ViewMode::FileDiff);
        harness.app.stage_left_for_test("staged\n", "baseline\n");

        assert!(matches!(
            harness.run(Command::Back),
            Outcome::NeedsConfirmation { .. }
        ));
        assert_eq!(prompt(&harness).lines[0], "  ~/proj-a/foo.txt");

        harness.app.dismiss_confirm();
        assert!(matches!(
            harness.run(Command::SaveStaged),
            Outcome::NeedsConfirmation { .. }
        ));
        assert_eq!(prompt(&harness).lines[0], "  ~/proj-a/foo.txt");
    }

    #[test]
    fn a_copy_confirmation_warns_before_replacing_the_destination() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("a.txt"), "left").unwrap();
        std::fs::write(right.path().join("a.txt"), "right").unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        harness
            .app
            .set_root_node(scanned(vec![differing_node("a.txt")]));
        harness.run(Command::CopyLeftToRight);

        let modal = prompt(&harness);
        assert_eq!(modal.headline, "Overwrite a.txt");
        assert_eq!(
            modal.lines.last().unwrap(),
            "The destination already exists and will be replaced."
        );
    }

    #[test]
    fn a_copy_confirmation_explains_a_directory_merge() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::create_dir(left.path().join("dir")).unwrap();
        std::fs::create_dir(right.path().join("dir")).unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        let mut node = entry_node("dir", true, Vec::new());
        node.state = DiffState::DifferentNewerLeft;
        harness.app.set_root_node(scanned(vec![node]));
        harness.run(Command::CopyLeftToRight);

        let modal = prompt(&harness);
        assert_eq!(modal.headline, "Merge dir");
        assert_eq!(
            modal.lines.last().unwrap(),
            "Merges into the existing directory: colliding entries are overwritten, others are \
             left in place. Only the entries this scan lists are copied."
        );
    }

    /// Issue #284: the save dialog lists every destination it would write.
    #[tokio::test]
    async fn a_save_confirmation_lists_every_destination() {
        let (harness, _left, right) = staged_file_diff();
        let mut harness = harness;
        assert!(matches!(
            harness.run(Command::SaveStaged),
            Outcome::NeedsConfirmation { .. }
        ));

        let modal = prompt(&harness);
        assert_eq!(modal.title, "Save staged changes");
        assert_eq!(modal.headline, "Write the staged changes to:");
        assert_eq!(modal.lines.len(), 1, "only the right side is staged");
        assert!(
            modal.lines[0].starts_with("  ") && modal.lines[0].ends_with("merge.txt"),
            "unexpected target line: {}",
            modal.lines[0]
        );
        assert!(modal.lines[0].contains(right.path().to_string_lossy().as_ref()));
        assert_eq!(
            choices(&modal),
            vec![
                ('s', "Save", app::ConfirmAction::SaveStaged),
                ('c', "Cancel", app::ConfirmAction::Cancel),
            ]
        );
    }

    /// Issue #284: the inventory gate, not a check inside the prompt builder,
    /// is what keeps a save with nothing staged from opening a dialog.
    #[test]
    fn a_save_with_nothing_staged_never_opens_a_dialog() {
        let (mut harness, _left, _right) = merge_file_diff();

        assert_eq!(
            harness.run(Command::SaveStaged),
            Outcome::Unavailable {
                message: "Save staged changes: no staged changes to save".to_string()
            }
        );
        assert!(harness.app.confirm_modal().is_none());
    }

    /// Issue #284: the dirty-exit gate offers all three ways out, Save first.
    #[test]
    fn leaving_a_dirty_file_diff_offers_save_discard_and_cancel() {
        let (mut harness, _left, _right) = staged_file_diff();
        assert!(matches!(
            harness.run(Command::Back),
            Outcome::NeedsConfirmation { .. }
        ));

        let modal = prompt(&harness);
        assert_eq!(modal.title, "Staged changes not saved");
        assert_eq!(
            modal.headline,
            "This file diff has staged changes that are not written yet."
        );
        assert_eq!(
            choices(&modal),
            vec![
                ('s', "Save", app::ConfirmAction::SaveStagedThenLeave),
                ('d', "Discard", app::ConfirmAction::DiscardStagedThenLeave),
                ('c', "Cancel", app::ConfirmAction::Cancel),
            ]
        );
    }

    /// Issue #284: the conflict dialog names the files and offers only the two
    /// safe ways out — force-overwrite is deliberately absent.
    #[tokio::test]
    async fn a_save_conflict_names_the_files_that_changed_on_disk() {
        let (mut harness, _left, right) = staged_file_diff();
        harness.run(Command::SaveStaged);
        std::fs::write(right.path().join("merge.txt"), "keep\nsomeone-else\n").unwrap();
        harness.answer(app::ConfirmAction::SaveStaged);

        let modal = prompt(&harness);
        assert_eq!(modal.title, "Files changed on disk");
        assert_eq!(
            modal.headline,
            "These files changed on disk since this diff was opened:"
        );
        assert!(modal.lines[0].trim().ends_with("merge.txt"));
        assert_eq!(modal.lines[1], "");
        assert_eq!(modal.lines[2], "Saving would overwrite those changes.");
        assert_eq!(
            choices(&modal),
            vec![
                (
                    'r',
                    "Reload, discarding staged changes",
                    app::ConfirmAction::ReloadDiscardStaged
                ),
                ('c', "Cancel", app::ConfirmAction::Cancel),
            ]
        );
    }

    /// Issue #284: a copy the scan says is pointless is refused with its reason
    /// instead of opening a dialog.
    #[test]
    fn a_copy_of_an_identical_pair_is_refused_without_a_dialog() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![entry_node("a.txt", false, Vec::new())]));

        assert_eq!(
            harness.run(Command::CopyLeftToRight),
            Outcome::Message {
                text: "Files are already identical — nothing to copy".to_string()
            }
        );
        assert!(harness.app.confirm_modal().is_none());
    }

    #[test]
    fn a_copy_out_of_a_dirty_file_diff_is_refused_until_the_work_is_settled() {
        let (mut harness, _left, _right) = staged_file_diff();

        assert_eq!(
            harness.run(Command::CopyLeftToRight),
            Outcome::Unavailable {
                message: "Staged changes are unsaved — press s to save or Esc to review them first"
                    .to_string()
            }
        );
        assert!(harness.app.confirm_modal().is_none());
    }

    /// Issue #282: leaving File Diff with staged work opens the dirty gate
    /// instead of discarding it, and discarding is an explicit second answer.
    #[test]
    fn back_from_a_dirty_file_diff_confirms_before_leaving() {
        let (mut harness, _left, right) = staged_file_diff();

        assert!(matches!(
            harness.run(Command::Back),
            Outcome::NeedsConfirmation { .. }
        ));
        assert_eq!(
            harness.app.view_mode(),
            ViewMode::FileDiff,
            "the gate holds the screen open"
        );

        assert_eq!(
            harness.answer(app::ConfirmAction::DiscardStagedThenLeave),
            Outcome::Completed
        );
        assert_eq!(harness.app.view_mode(), ViewMode::DirectoryTree);
        assert_eq!(
            std::fs::read_to_string(right.path().join("merge.txt")).unwrap(),
            "keep\nright-line\n",
            "discarding must not write"
        );
    }

    /// Issue #282: staging and saving keep their own verbs, and the save lands
    /// only after the user confirms it.
    #[tokio::test]
    async fn a_confirmed_save_writes_the_staged_sides_and_reports_it_once() {
        let (mut harness, _left, right) = staged_file_diff();

        assert!(matches!(
            harness.run(Command::SaveStaged),
            Outcome::NeedsConfirmation { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(right.path().join("merge.txt")).unwrap(),
            "keep\nright-line\n",
            "nothing is written before the answer"
        );

        assert_eq!(
            harness.answer(app::ConfirmAction::SaveStaged),
            Outcome::Message {
                text: "Saved staged changes".to_string(),
            }
        );
        assert_eq!(
            std::fs::read_to_string(right.path().join("merge.txt")).unwrap(),
            "keep\nleft-line\n"
        );
        assert!(!harness.app.diff().is_dirty());
    }

    /// Issue #282: a disk conflict replaces the approval with a new explicit
    /// confirmation rather than silently reusing the first one.
    #[test]
    fn a_save_conflict_replaces_the_pending_confirmation() {
        let (mut harness, _left, right) = staged_file_diff();
        assert!(matches!(
            harness.run(Command::SaveStaged),
            Outcome::NeedsConfirmation { .. }
        ));

        // Someone else wrote the destination between the prompt and the answer.
        std::fs::write(right.path().join("merge.txt"), "keep\nsomeone-else\n").unwrap();

        assert!(
            matches!(
                harness.answer(app::ConfirmAction::SaveStaged),
                Outcome::NeedsConfirmation { .. }
            ),
            "the approval does not carry over to the changed file"
        );
        assert!(harness.app.confirm_modal().is_some());
        assert_eq!(
            std::fs::read_to_string(right.path().join("merge.txt")).unwrap(),
            "keep\nsomeone-else\n",
            "the conflicting content is left alone"
        );

        assert_eq!(
            harness.answer(app::ConfirmAction::ReloadDiscardStaged),
            Outcome::Message {
                text: "Reloaded from disk; staged changes discarded".to_string(),
            }
        );
        assert!(!harness.app.diff().is_dirty());
    }

    #[test]
    fn undo_reports_when_there_is_nothing_left_to_undo() {
        let (mut harness, _left, _right) = staged_file_diff();

        assert_eq!(harness.run(Command::UndoStaged), Outcome::Completed);
        assert!(!harness.app.diff().is_dirty());
        // With the stack empty the Command is no longer available at all.
        assert_eq!(
            harness.run(Command::UndoStaged),
            Outcome::Unavailable {
                message: "Undo last staged change block: nothing staged to undo".to_string()
            }
        );
    }
}
