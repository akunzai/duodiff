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
            let outcome = crate::actions::execute_confirm_action(app, action, self.tx.clone())?;
            // A save conflict replaces the approval that got us here with its own
            // dialog, so the pending target follows the new continuation.
            return Ok(if app.confirm_modal().is_some() {
                self.pending_target = app.selected_row().map(|row| row.relative_path.clone());
                Outcome::NeedsConfirmation
            } else {
                outcome
            });
        };
        if command != Command::OpenRepository {
            if let Some(reason) = self
                .inventory(app)
                .into_iter()
                .find(|entry| entry.command == command)
                .and_then(|entry| entry.disabled_reason)
            {
                return Ok(Outcome::Unavailable { reason });
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
            Command::OpenRepository => {
                crate::actions::open_repo_url(self.tx.clone());
                outcome = Outcome::Message {
                    text: "Opening GitHub repository in the browser...".into(),
                    is_error: false,
                };
            }
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
    use crate::commands::{Command as Id, CommandEntry as Entry};

    let mut commands = Vec::new();
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

            commands.push(Entry::gated(
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
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                is_file_pair && effective_diff_tool.is_some(),
                diff_tool_reason,
            ));
            commands.push(Entry::gated(
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
            commands.push(Entry::gated(
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
            commands.push(Entry::gated(
                "Copy the selection to the left pane",
                Id::CopyRightToLeft,
                copy_right_enabled,
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
            let row = app.selected_row();
            let is_file_pair =
                row.is_some_and(|r| !r.is_dir() && r.left.is_some() && r.right.is_some());
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
            commands.push(Entry::gated(
                "Copy the whole left file to the right",
                Id::CopyLeftToRight,
                row.is_some_and(|r| r.left.is_some() && !r.is_ambiguous_case_collision),
                if row.is_some_and(|r| r.is_ambiguous_case_collision) {
                    "cannot copy: ambiguous case collision"
                } else {
                    "nothing on the left side to copy"
                },
            ));
            commands.push(Entry::gated(
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
            commands.push(Entry::gated(
                "Compare with the external diff tool",
                Id::ExternalDiff,
                is_file_pair && effective_diff_tool.is_some(),
                diff_tool_reason,
            ));
            commands.push(Entry::gated(
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
            self.commands
                .execute(
                    &mut self.app,
                    Invocation::Command(command),
                    &mut self.terminal,
                )
                .unwrap()
        }

        fn answer(&mut self, action: app::ConfirmAction) -> Outcome {
            self.commands
                .execute(
                    &mut self.app,
                    Invocation::Confirmation(action),
                    &mut self.terminal,
                )
                .unwrap()
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
                reason: "no row is selected"
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
                reason: "the selected row is a directory"
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

        for command in [Command::Expand, Command::Collapse] {
            assert_eq!(
                harness.run(command),
                Outcome::Unavailable {
                    reason: "the selected row is not a directory"
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
                is_error: false,
            }
        );

        // Unavailability is informational — it carries a reason, not an error.
        harness.app.set_view_mode(ViewMode::FileDiff);
        assert_eq!(
            harness.run(Command::StageLeftToRight),
            Outcome::Unavailable {
                reason: "the two sides have no differing lines"
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

        assert_eq!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation
        );
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
                reason: "external diff is disabled"
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

        assert_eq!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation
        );
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
                reason: "the original command target is no longer selected"
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

        assert_eq!(
            harness.run(Command::CopyLeftToRight),
            Outcome::NeedsConfirmation
        );
        assert_eq!(
            harness.answer(app::ConfirmAction::CopyLeftToRight),
            Outcome::Message {
                text: "Copied 'a.txt'".to_string(),
                is_error: false,
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
                reason: "the original command target is no longer selected"
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
                is_error: false,
            }
        );
        assert!(harness.app.diff().is_dirty());
        (harness, left, right)
    }

    /// Issue #282: leaving File Diff with staged work opens the dirty gate
    /// instead of discarding it, and discarding is an explicit second answer.
    #[test]
    fn back_from_a_dirty_file_diff_confirms_before_leaving() {
        let (mut harness, _left, right) = staged_file_diff();

        assert_eq!(harness.run(Command::Back), Outcome::NeedsConfirmation);
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

        assert_eq!(harness.run(Command::SaveStaged), Outcome::NeedsConfirmation);
        assert_eq!(
            std::fs::read_to_string(right.path().join("merge.txt")).unwrap(),
            "keep\nright-line\n",
            "nothing is written before the answer"
        );

        assert_eq!(
            harness.answer(app::ConfirmAction::SaveStaged),
            Outcome::Message {
                text: "Saved staged changes".to_string(),
                is_error: false,
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
        assert_eq!(harness.run(Command::SaveStaged), Outcome::NeedsConfirmation);

        // Someone else wrote the destination between the prompt and the answer.
        std::fs::write(right.path().join("merge.txt"), "keep\nsomeone-else\n").unwrap();

        assert_eq!(
            harness.answer(app::ConfirmAction::SaveStaged),
            Outcome::NeedsConfirmation,
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
                is_error: false,
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
                reason: "nothing staged to undo"
            }
        );
    }
}
