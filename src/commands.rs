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
    ExpandAll,
    CollapseAll,
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
    NextDifference,
    PrevDifference,
    StageLeftToRight,
    StageRightToLeft,
    Back,
    OpenRepository,
}

/// Each bindable Command's name in the `[keys]` config section (Issue #339).
///
/// Spelled out rather than derived from the variant, so renaming a variant can
/// never break a user's config. The names follow the Palette's verbs.
const CONFIG_NAMES: &[(Command, &str)] = &[
    (Command::BuiltinDiff, "open_diff"),
    (Command::ExternalDiff, "external_diff"),
    (Command::ExternalEdit, "external_edit"),
    (Command::CopyLeftToRight, "copy_to_right"),
    (Command::CopyRightToLeft, "copy_to_left"),
    (Command::Expand, "expand"),
    (Command::Collapse, "collapse"),
    (Command::ExpandAll, "expand_all"),
    (Command::CollapseAll, "collapse_all"),
    (Command::NextDifference, "next_difference"),
    (Command::PrevDifference, "prev_difference"),
    (Command::ToggleFocus, "switch_pane"),
    (Command::FocusLeft, "focus_left"),
    (Command::FocusRight, "focus_right"),
    (Command::Filter, "filter"),
    (Command::SwapPaths, "swap_sides"),
    (Command::ToggleScan, "switch_scan_mode"),
    (Command::Refresh, "rescan"),
    (Command::NextChange, "next_change"),
    (Command::PrevChange, "prev_change"),
    (Command::StageLeftToRight, "stage_to_right"),
    (Command::StageRightToLeft, "stage_to_left"),
    (Command::SaveStaged, "save_staged"),
    (Command::UndoStaged, "undo_staged"),
    (Command::ToggleWrap, "toggle_wrap"),
    (Command::ToggleFullDiff, "toggle_full_context"),
    (Command::ToggleTheme, "switch_theme"),
    (Command::Config, "config"),
    (Command::Help, "help"),
    (Command::Back, "back"),
    (Command::Quit, "quit"),
];

impl Command {
    /// This Command's `[keys]` name, or `None` for one no key can reach (the
    /// Help repository link).
    pub fn config_name(self) -> Option<&'static str> {
        CONFIG_NAMES
            .iter()
            .find(|(command, _)| *command == self)
            .map(|(_, name)| *name)
    }

    /// The Command a `[keys]` name refers to.
    pub fn from_config_name(name: &str) -> Option<Self> {
        CONFIG_NAMES
            .iter()
            .find(|(_, candidate)| *candidate == name)
            .map(|(command, _)| *command)
    }

    /// Every `[keys]` name, in documentation order.
    pub fn config_names() -> impl Iterator<Item = &'static str> {
        CONFIG_NAMES.iter().map(|(_, name)| *name)
    }
}

#[derive(Clone, Debug)]
pub struct CommandEntry {
    pub key: String,
    pub label: String,
    pub command: Command,
    pub disabled_reason: Option<&'static str>,
}

impl CommandEntry {
    /// The key column comes from `App`'s [`crate::keymap::Keymap`], so the
    /// Palette never restates a binding it does not own (ADR-0003).
    pub fn new(label: &str, command: Command, keymap: &crate::keymap::Keymap) -> Self {
        Self {
            key: keymap.hint(command),
            label: label.into(),
            command,
            disabled_reason: None,
        }
    }

    pub fn gated(
        label: &str,
        command: Command,
        available: bool,
        reason: &'static str,
        keymap: &crate::keymap::Keymap,
    ) -> Self {
        Self {
            key: keymap.hint(command),
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
            || target.is_some_and(|target| app.confirmation_subject() == Some(target));
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
            Command::Filter => app.directory_tree_mut().open(),
            Command::Quit => {
                app.request_quit();
                return Ok(Outcome::ExitRequested);
            }
            Command::ToggleWrap => app.diff_mut().toggle_wrap(),
            Command::ToggleFullDiff => app.toggle_diff_show_full(),
            Command::NextChange => app.diff_mut().jump_to_change(true),
            Command::PrevChange => app.diff_mut().jump_to_change(false),
            // Availability already found a stop (`has_difference`), so the
            // jump always moves.
            Command::NextDifference | Command::PrevDifference => {
                app.directory_tree_mut()
                    .jump_to_difference(command == Command::NextDifference);
            }
            Command::StageLeftToRight | Command::StageRightToLeft => {
                let (direction, side) = if command == Command::StageLeftToRight {
                    (crate::diff_view::HunkCopyDirection::LeftToRight, "right")
                } else {
                    (crate::diff_view::HunkCopyDirection::RightToLeft, "left")
                };
                match app.stage_hunk_at_cursor(direction) {
                    Ok(true) => {
                        let save = match app.keymap().key_phrase(Command::SaveStaged) {
                            Some(key) => format!("then {key} to save"),
                            None => "then save from the Command Palette".to_string(),
                        };
                        outcome = Outcome::Message {
                            text: format!("Staged change block to {side} — stage more, {save}"),
                        }
                    }
                    Ok(false) => {
                        outcome = Outcome::Message {
                            text: "Nothing to stage — already identical".to_string(),
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
            Command::Expand => app.directory_tree_mut().expand_selected(),
            Command::Collapse => app.directory_tree_mut().collapse_selected(),
            Command::ExpandAll => app.directory_tree_mut().set_all_expanded(true),
            Command::CollapseAll => app.directory_tree_mut().set_all_expanded(false),
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
        self.pending_target = app.confirmation_subject();
        Outcome::NeedsConfirmation { prompt }
    }

    /// Preview a copy and ask about it, or say why there is nothing to ask.
    fn request_copy(&mut self, app: &App, direction: app::CopyDirection) -> Outcome {
        match app.preview_copy(direction) {
            Ok(preview) => self.confirm(app, copy_prompt(&preview, direction)),
            Err(refusal) => refused_copy(refusal, app.keymap()),
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
fn refused_copy(refusal: app::CopyRefusal, keymap: &crate::keymap::Keymap) -> Outcome {
    match refusal {
        app::CopyRefusal::StagedChangesUnsaved => {
            let save = match keymap.key_phrase(Command::SaveStaged) {
                Some(key) => format!("press {key} to save"),
                None => "save from the Command Palette".to_string(),
            };
            let review = match keymap.key_phrase(Command::Back) {
                Some(key) => format!("{key} to review them first"),
                None => "review them from the Command Palette first".to_string(),
            };
            Outcome::Unavailable {
                message: format!("Staged changes are unsaved — {save} or {review}"),
            }
        }
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
    if let Some(pair) = app.file_pair() {
        if !pair.left.can_reopen() || !pair.right.can_reopen() {
            return (false, "a side was read from a pipe");
        }
    } else if !app
        .selected_row()
        .is_some_and(|row| !row.is_dir() && row.left.is_some() && row.right.is_some())
    {
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

/// Whether a change block can be staged into one side, and why not: there must
/// be a change, and a file-pair side must be writable (Issue #327).
fn stage_availability(
    app: &App,
    into_left: bool,
    no_changes: &'static str,
) -> (bool, &'static str) {
    if let Some(pair) = app.file_pair() {
        let (target, read_only) = if into_left {
            (&pair.left, "the left side is read-only")
        } else {
            (&pair.right, "the right side is read-only")
        };
        if !target.is_writable() {
            return (false, read_only);
        }
    }
    (app.diff().has_changes(), no_changes)
}

/// Whether one copy direction can run on the selected row, and why not.
///
/// `absent` names the empty side, so each screen keeps its own wording for a
/// whole entry versus a whole file.
fn copy_availability(app: &App, left_to_right: bool, absent: &'static str) -> (bool, &'static str) {
    if let Some(pair) = app.file_pair() {
        let (source, destination, read_only) = if left_to_right {
            (&pair.left, &pair.right, "the right side is read-only")
        } else {
            (&pair.right, &pair.left, "the left side is read-only")
        };
        // Copying the null device would only empty the other file, the same
        // refusal a Directory Tree row gives an absent side.
        if source.is_null_device() {
            return (false, absent);
        }
        return (destination.is_writable(), read_only);
    }
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
    let keymap = app.keymap();

    let mut commands = Vec::new();
    match app.view_mode() {
        ViewMode::DirectoryTree => {
            let row = app.selected_row();
            let has_row = row.is_some();
            let edit_unavailable = "the focused pane has no file at this row";
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
                keymap,
            ));
            let (diff_tool_ready, diff_tool_reason) = external_diff_availability(app);
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                diff_tool_ready,
                diff_tool_reason,
                keymap,
            ));
            commands.push(Entry::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                app.active_side_has_file(),
                edit_unavailable,
                keymap,
            ));
            let (copy_left, copy_left_reason) =
                copy_availability(app, true, reason("nothing on the left side to copy"));
            commands.push(Entry::gated(
                "Copy the selection to the right pane",
                Id::CopyLeftToRight,
                copy_left,
                copy_left_reason,
                keymap,
            ));
            let (copy_right, copy_right_reason) =
                copy_availability(app, false, reason("nothing on the right side to copy"));
            commands.push(Entry::gated(
                "Copy the selection to the left pane",
                Id::CopyRightToLeft,
                copy_right,
                copy_right_reason,
                keymap,
            ));
            commands.push(Entry::gated(
                "Expand selected directory",
                Id::Expand,
                is_dir,
                reason("the selected row is not a directory"),
                keymap,
            ));
            commands.push(Entry::gated(
                "Collapse selected directory",
                Id::Collapse,
                is_dir,
                reason("the selected row is not a directory"),
                keymap,
            ));
            let has_differences = app.directory_tree().has_difference();
            let no_differences = if app.directory_tree().pattern().is_empty()
                && !app.directory_tree().diffs_only()
            {
                "the two trees have no differences"
            } else {
                "the filtered list has no differences"
            };
            commands.push(Entry::gated(
                "Jump to the next difference",
                Id::NextDifference,
                has_differences,
                no_differences,
                keymap,
            ));
            commands.push(Entry::gated(
                "Jump to the previous difference",
                Id::PrevDifference,
                has_differences,
                no_differences,
                keymap,
            ));
            // A filter lists its matches flat, whatever is expanded, so the
            // bulk commands would change nothing the user can see.
            let unfiltered =
                app.directory_tree().pattern().is_empty() && !app.directory_tree().diffs_only();
            let filtered = "a filter is applied — clear it first";
            commands.push(Entry::gated(
                "Expand all directories",
                Id::ExpandAll,
                unfiltered,
                filtered,
                keymap,
            ));
            commands.push(Entry::gated(
                "Collapse all directories",
                Id::CollapseAll,
                unfiltered,
                filtered,
                keymap,
            ));
            commands.push(Entry::new(
                "Switch the focused pane",
                Id::ToggleFocus,
                keymap,
            ));
            commands.push(Entry::new("Focus the left pane", Id::FocusLeft, keymap));
            commands.push(Entry::new("Focus the right pane", Id::FocusRight, keymap));
            commands.push(Entry::new("Filter the tree", Id::Filter, keymap));
            commands.push(Entry::new(
                "Swap the left and right directories",
                Id::SwapPaths,
                keymap,
            ));
            commands.push(Entry::new(
                "Switch scan mode (Fast / Precise)",
                Id::ToggleScan,
                keymap,
            ));
            commands.push(Entry::new("Re-scan both directories", Id::Refresh, keymap));
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
                keymap,
            ));
            commands.push(Entry::new("Open the Config screen", Id::Config, keymap));
            commands.push(Entry::new("Open Help", Id::Help, keymap));
            commands.push(Entry::new("Quit", Id::Quit, keymap));
        }
        ViewMode::FileDiff => {
            let has_changes = app.diff().has_changes();
            let no_changes = "the two sides have no differing lines";
            let edit_unavailable = if app.file_pair().is_some() {
                "the focused pane has no file to edit"
            } else {
                "the focused pane has no file at this row"
            };

            commands.push(Entry::gated(
                "Jump to the next change block",
                Id::NextChange,
                has_changes,
                no_changes,
                keymap,
            ));
            commands.push(Entry::gated(
                "Jump to the previous change block",
                Id::PrevChange,
                has_changes,
                no_changes,
                keymap,
            ));
            let (stage_right, stage_right_reason) = stage_availability(app, false, no_changes);
            commands.push(Entry::gated(
                "Stage the change block to the right",
                Id::StageLeftToRight,
                stage_right,
                stage_right_reason,
                keymap,
            ));
            let (stage_left, stage_left_reason) = stage_availability(app, true, no_changes);
            commands.push(Entry::gated(
                "Stage the change block to the left",
                Id::StageRightToLeft,
                stage_left,
                stage_left_reason,
                keymap,
            ));
            let (copy_left, copy_left_reason) =
                copy_availability(app, true, "nothing on the left side to copy");
            commands.push(Entry::gated(
                "Copy the whole left file to the right",
                Id::CopyLeftToRight,
                copy_left,
                copy_left_reason,
                keymap,
            ));
            let (copy_right, copy_right_reason) =
                copy_availability(app, false, "nothing on the right side to copy");
            commands.push(Entry::gated(
                "Copy the whole right file to the left",
                Id::CopyRightToLeft,
                copy_right,
                copy_right_reason,
                keymap,
            ));
            let (diff_tool_ready, diff_tool_reason) = external_diff_availability(app);
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                diff_tool_ready,
                diff_tool_reason,
                keymap,
            ));
            commands.push(Entry::gated(
                "Edit in the external editor",
                Id::ExternalEdit,
                app.active_side_has_file(),
                edit_unavailable,
                keymap,
            ));
            commands.push(Entry::gated(
                "Save staged changes",
                Id::SaveStaged,
                app.diff().is_dirty(),
                "no staged changes to save",
                keymap,
            ));
            commands.push(Entry::gated(
                "Undo last staged change block",
                Id::UndoStaged,
                app.diff().can_undo(),
                "nothing staged to undo",
                keymap,
            ));
            commands.push(Entry::new("Toggle line wrapping", Id::ToggleWrap, keymap));
            commands.push(Entry::new(
                "Toggle full-file context",
                Id::ToggleFullDiff,
                keymap,
            ));
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
                keymap,
            ));
            commands.push(Entry::new("Open the Config screen", Id::Config, keymap));
            commands.push(Entry::new("Open Help", Id::Help, keymap));
            // A file pair opened from the command line has no tree to return
            // to, so Back ends the session (Issue #327).
            let back = if app.file_pair().is_some() {
                "Quit"
            } else {
                "Return to the Directory Tree"
            };
            commands.push(Entry::new(back, Id::Back, keymap));
        }
        ViewMode::ConfigMenu | ViewMode::Help => {
            commands.push(Entry::new(
                "Switch the light and dark theme",
                Id::ToggleTheme,
                keymap,
            ));
            if app.view_mode() == ViewMode::Help {
                commands.push(Entry::new("Open the Config screen", Id::Config, keymap));
            } else {
                commands.push(Entry::new("Open Help", Id::Help, keymap));
            }
            commands.push(Entry::new("Go back", Id::Back, keymap));
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
        launched: Vec<crate::actions::KeyOutcome>,
    }

    impl TerminalHandoff for FakeTerminalHandoff {
        fn dispatch(
            &mut self,
            outcome: crate::actions::KeyOutcome,
            _mouse_enabled: bool,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.calls += 1;
            self.launched.push(outcome);
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
            expanded_by_default: false,
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
            expanded_by_default: true,
            children,
            ..Default::default()
        }
    }

    /// Issue #339: every Command a key can reach has one stable `[keys]` name,
    /// and the name leads back to it.
    #[test]
    fn every_bound_command_has_a_unique_config_name() {
        let keymap = crate::keymap::Keymap::default();
        for binding in [
            &keymap.global,
            &keymap.directory_tree,
            &keymap.file_diff,
            &keymap.config_menu,
            &keymap.help,
        ]
        .into_iter()
        .flatten()
        {
            let name = binding
                .command
                .config_name()
                .unwrap_or_else(|| panic!("{:?} has no config name", binding.command));
            assert_eq!(Command::from_config_name(name), Some(binding.command));
        }

        let mut names: Vec<&str> = Command::config_names().collect();
        let listed = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), listed, "config names are unique");
        assert_eq!(Command::OpenRepository.config_name(), None);
        assert_eq!(Command::from_config_name("BuiltinDiff"), None);
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
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 1);

        assert_eq!(harness.run(Command::Expand), Outcome::Completed);
        assert_eq!(
            harness.app.directory_tree().flat_rows().len(),
            2,
            "the child is now listed"
        );
        assert_eq!(harness.run(Command::Expand), Outcome::Completed);
        assert_eq!(
            harness.app.directory_tree().flat_rows().len(),
            2,
            "Expand never collapses"
        );

        assert_eq!(harness.run(Command::Collapse), Outcome::Completed);
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 1);
        assert_eq!(harness.run(Command::Collapse), Outcome::Completed);
        assert_eq!(
            harness.app.directory_tree().flat_rows().len(),
            1,
            "Collapse never expands"
        );
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

    /// Issue #338: the bulk Commands reach every directory, not only the
    /// selected one, and are target states like Expand and Collapse.
    #[test]
    fn expand_all_and_collapse_all_reach_every_directory() {
        let mut harness = Harness::new();
        harness.app.set_root_node(scanned(vec![entry_node(
            "outer",
            true,
            vec![entry_node(
                "outer/inner",
                true,
                vec![entry_node("outer/inner/leaf.txt", false, Vec::new())],
            )],
        )]));
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 1);

        assert_eq!(harness.run(Command::ExpandAll), Outcome::Completed);
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 3);
        assert_eq!(harness.run(Command::ExpandAll), Outcome::Completed);
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 3);

        assert_eq!(harness.run(Command::CollapseAll), Outcome::Completed);
        assert_eq!(harness.app.directory_tree().flat_rows().len(), 1);
    }

    /// Issue #338: a filter lists its matches flat whatever is expanded, so the
    /// bulk Commands stay listed but refuse rather than change hidden state.
    #[test]
    fn expand_all_and_collapse_all_wait_for_the_filter_to_clear() {
        let mut harness = Harness::new();
        harness.app.set_root_node(scanned(vec![entry_node(
            "dir",
            true,
            vec![entry_node("child.txt", false, Vec::new())],
        )]));
        harness.app.directory_tree_mut().set_pattern("child");
        harness.app.apply_filter();

        for (command, label) in [
            (Command::ExpandAll, "Expand all directories"),
            (Command::CollapseAll, "Collapse all directories"),
        ] {
            assert_eq!(
                harness.run(command),
                Outcome::Unavailable {
                    message: format!("{label}: a filter is applied — clear it first")
                }
            );
        }
        assert_eq!(
            harness.app.directory_tree().flat_rows().len(),
            1,
            "nothing expanded"
        );
    }

    /// Issue #338: the difference jumps stay listed on an identical tree with
    /// the reason, and a filter that lists no difference says so.
    #[test]
    fn difference_jumps_explain_when_there_is_nowhere_to_go() {
        let mut harness = Harness::new();
        harness
            .app
            .set_root_node(scanned(vec![entry_node("same.txt", false, Vec::new())]));
        assert_eq!(
            harness.run(Command::NextDifference),
            Outcome::Unavailable {
                message: "Jump to the next difference: the two trees have no differences"
                    .to_string()
            }
        );

        harness.app.set_root_node(scanned(vec![
            entry_node("same.txt", false, Vec::new()),
            differing_node("changed.txt"),
        ]));
        harness.app.directory_tree_mut().set_pattern("same");
        harness.app.apply_filter();
        assert_eq!(
            harness.run(Command::PrevDifference),
            Outcome::Unavailable {
                message: "Jump to the previous difference: the filtered list has no differences"
                    .to_string()
            }
        );

        harness.app.directory_tree_mut().set_pattern("");
        harness.app.apply_filter();
        assert_eq!(harness.run(Command::NextDifference), Outcome::Completed);
        assert_eq!(
            harness.app.selected_relative_path(),
            Some(PathBuf::from("changed.txt"))
        );
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

        harness.app.directory_tree_mut().set_selected_idx(1);

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
        harness.app.directory_tree_mut().set_selected_idx(1);
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
                text: "Staged change block to right — stage more, then s to save".to_string(),
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

    /// The Palette key column is derived from `App`'s keymap, so a remap
    /// reaches it without Commands restating any binding (ADR-0003, Issue
    /// #339).
    #[test]
    fn palette_key_column_reflects_a_remapped_keymap() {
        let mut harness = Harness::new();
        harness
            .app
            .set_keymap(crate::keymap::sample_remapped_keymap());
        let key_for = |harness: &Harness, command: Command| {
            harness
                .inventory()
                .into_iter()
                .find(|entry| entry.command == command)
                .unwrap()
                .key
        };
        assert_eq!(key_for(&harness, Command::CopyLeftToRight), "L");
        assert_eq!(key_for(&harness, Command::CopyRightToLeft), "R");
        assert_eq!(key_for(&harness, Command::Config), "x");

        harness.app.set_view_mode(ViewMode::FileDiff);
        assert_eq!(key_for(&harness, Command::SaveStaged), "");
    }

    /// A staged-change toast falls back to "the Command Palette" when the
    /// keymap leaves `SaveStaged` unbound (Issue #339).
    #[test]
    fn staged_toast_falls_back_to_the_command_palette_when_save_staged_is_unbound() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        std::fs::write(left.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        std::fs::write(right.path().join("merge.txt"), "keep\nright-line\n").unwrap();

        let mut harness = Harness::rooted(left.path().to_path_buf(), right.path().to_path_buf());
        harness
            .app
            .set_keymap(crate::keymap::sample_remapped_keymap());
        harness
            .app
            .set_root_node(scanned(vec![entry_node("merge.txt", false, Vec::new())]));
        assert_eq!(harness.run(Command::BuiltinDiff), Outcome::Completed);
        assert_eq!(harness.run(Command::ToggleFullDiff), Outcome::Completed);
        harness.app.diff_mut().set_scroll(1);

        assert_eq!(
            harness.run(Command::StageLeftToRight),
            Outcome::Message {
                text:
                    "Staged change block to right — stage more, then save from the Command Palette"
                        .to_string(),
            }
        );
    }

    /// Direct file comparison (Issue #327): File Diff opened on a file pair.
    mod file_pair {
        use super::*;
        use std::path::Path;

        fn opened(left: &Path, right: &Path) -> Harness {
            let mut harness = Harness::new();
            let crate::target::ComparisonTarget::Files(pair) =
                crate::target::resolve(left, right).unwrap()
            else {
                panic!("expected a file pair");
            };
            harness.app.open_file_pair(pair).unwrap();
            harness
        }

        fn commands(harness: &Harness) -> Vec<Command> {
            harness
                .inventory()
                .iter()
                .map(|entry| entry.command)
                .collect()
        }

        #[test]
        fn file_diff_lists_the_same_commands_and_back_quits() {
            let dir = tempfile::tempdir().unwrap();
            let (left, right) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
            std::fs::write(&left, "a\n").unwrap();
            std::fs::write(&right, "b\n").unwrap();
            let harness = opened(&left, &right);
            let mut tree = Harness::new();
            tree.app.set_view_mode(ViewMode::FileDiff);

            assert_eq!(commands(&harness), commands(&tree));
            let back = harness
                .inventory()
                .into_iter()
                .find(|entry| entry.command == Command::Back)
                .unwrap();
            assert_eq!(back.label, "Quit");
        }

        #[test]
        fn external_edit_opens_the_focused_side_of_the_pair() {
            let dir = tempfile::tempdir().unwrap();
            let (left, right) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
            std::fs::write(&left, "a\n").unwrap();
            std::fs::write(&right, "b\n").unwrap();
            let mut harness = opened(&left, &right);
            harness.app.focus_right_pane();

            assert_eq!(harness.run(Command::ExternalEdit), Outcome::Completed);

            assert_eq!(
                harness.terminal.launched,
                vec![crate::actions::KeyOutcome::LaunchEditor {
                    path: std::fs::canonicalize(&right).unwrap()
                }]
            );
        }

        #[test]
        fn the_null_device_cannot_be_edited() {
            let dir = tempfile::tempdir().unwrap();
            let right = dir.path().join("added.txt");
            std::fs::write(&right, "new\n").unwrap();
            let harness = opened(Path::new("/dev/null"), &right);

            assert_eq!(
                harness.reason_for(Command::ExternalEdit),
                Some("the focused pane has no file to edit")
            );
        }

        #[test]
        fn external_diff_is_gated_only_by_the_tool_for_files_on_disk() {
            let dir = tempfile::tempdir().unwrap();
            let right = dir.path().join("added.txt");
            std::fs::write(&right, "new\n").unwrap();
            let mut harness = opened(Path::new("/dev/null"), &right);
            harness
                .app
                .set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);

            assert_eq!(
                harness.reason_for(Command::ExternalDiff),
                Some("external diff is disabled")
            );
        }

        #[cfg(unix)]
        #[test]
        fn external_diff_needs_both_sides_on_disk() {
            let dir = tempfile::tempdir().unwrap();
            let left = crate::test_support::fifo_with(dir.path(), "left", "piped\n");
            let right = dir.path().join("b.txt");
            std::fs::write(&right, "b\n").unwrap();
            let harness = opened(&left, &right);

            assert_eq!(
                harness.reason_for(Command::ExternalDiff),
                Some("a side was read from a pipe")
            );
        }
    }
}
