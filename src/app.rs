//! The session's state: [`App`] and the sub-states it composes.
//!
//! Each sub-state with an interface of its own lives in a child module under
//! `app/` beside its tests, and is re-exported here; `App` keeps what spans
//! them (ADR-0002).

mod compared_pair;
mod config;
mod directory_tree;
mod file_diff;
pub(crate) mod file_diff_session;
mod help;
mod palette;

pub use compared_pair::{
    CopyDirection, CopyKind, CopyPlan, CopyPreview, CopyRefusal, CopyTarget, DiffPlan, DiffRefusal,
};
pub use config::{
    ConfigContext, ConfigGesture, ConfigIntent, ConfigRowKind, ConfigState, ExclusionEditorState,
};
pub use directory_tree::{DirectoryTreeState, FlatRow};
pub use file_diff::FileDiffState;
pub use help::{HelpState, HelpTopic};
pub use palette::PaletteState;

use crate::diff::{AlignedNode, FileInfo};
use crate::ignore::IgnoreMatcher;
use crate::side::{Pair, Side};
#[cfg(test)]
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ViewMode {
    DirectoryTree,
    FileDiff,
    ConfigMenu,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConfirmAction {
    CopyLeftToRight,
    CopyRightToLeft,
    /// Write every dirty working buffer, keeping the File Diff session open.
    SaveStaged,
    /// Write every dirty working buffer, then return to the Directory Tree —
    /// only if the write succeeds.
    SaveStagedThenLeave,
    /// Throw the staged edits away and return to the Directory Tree.
    DiscardStagedThenLeave,
    /// Re-read both sides from disk, discarding the staged edits. The only way
    /// forward when the files changed underneath the session.
    ReloadDiscardStaged,
    /// Close the dialog and do nothing.
    Cancel,
}

/// The result of writing the staged sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StagedSave {
    Written,
    /// Nothing was written: these absolute paths changed on disk since the diff
    /// was opened.
    Conflicted(Vec<PathBuf>),
}

/// A pending confirmation prompt: the message to show and the action to run if accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmModal {
    pub title: String,
    /// The one sentence that states what `Enter` will do. Rendered emphasized,
    /// above the body, so the consequence is legible before the detail is read.
    pub headline: String,
    /// Supporting body lines, already assembled. Rendering wraps them to the
    /// popup width so the dialog stays usable on a narrow terminal (Issue #235).
    pub lines: Vec<String>,
    /// Offered choices, most affirmative first. `Enter` picks the first; `Esc`
    /// picks whichever choice carries [`ConfirmAction::Cancel`].
    pub choices: Vec<ConfirmChoice>,
}

/// One button in a [`ConfirmModal`].
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmChoice {
    /// The letter that selects it, matched case-insensitively.
    pub key: char,
    pub label: String,
    pub action: ConfirmAction,
}

impl ConfirmModal {
    /// The action a typed character selects, if any.
    pub fn action_for_key(&self, typed: char) -> Option<ConfirmAction> {
        let typed = typed.to_ascii_lowercase();
        self.choices
            .iter()
            .find(|c| c.key.to_ascii_lowercase() == typed)
            .map(|c| c.action.clone())
    }

    /// The action `Enter` selects: the first, most affirmative choice.
    pub fn default_action(&self) -> Option<ConfirmAction> {
        self.choices.first().map(|c| c.action.clone())
    }

    /// The action `Esc` selects: the cancel choice, when the dialog offers one.
    pub fn cancel_action(&self) -> Option<ConfirmAction> {
        self.choices
            .iter()
            .find(|c| c.action == ConfirmAction::Cancel)
            .map(|c| c.action.clone())
    }
}

/// One side of the session: the directory compared on it, and the ignore
/// rules read from under it. A root and its rules move together, so one root's
/// project rules never shape the other side (Issue #237).
struct Root {
    path: PathBuf,
    ignore: IgnoreMatcher,
}

pub struct App {
    roots: Pair<Root>,
    /// The file pair named on the command line, when the session compares two
    /// files instead of two directories (Issue #327). File Diff reads its paths
    /// from here rather than from a Directory Tree row, and there is no tree to
    /// go back to.
    file_pair: Option<crate::target::FilePair>,
    /// Size and modification time of each file-pair side, refreshed whenever
    /// the pair is loaded or saved so drawing never touches the filesystem.
    file_pair_info: Pair<Option<FileInfo>>,
    scan: crate::scan::ScanState,
    view_mode: ViewMode,
    file_diff: file_diff_session::FileDiffSession,
    settings: crate::settings::SettingsState,
    /// The mouse capture a Settings change asked the terminal to switch to,
    /// until the event loop, which owns the terminal, takes it.
    mouse_capture: Option<bool>,
    detected_diff_tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    config: ConfigState,
    palette: PaletteState,
    confirm_modal: Option<ConfirmModal>,
    /// Transient status toast: (message, is_error, created_at)
    status_message: Option<(String, bool, Instant)>,
    directory_tree: DirectoryTreeState,
    /// Which pane has focus, on the Directory Tree and File Diff alike.
    active_side: Side,
    update_available: Option<String>,
    /// Where a finished update check records itself.
    update_check_store: crate::upgrade::UpdateCheckStore,
    install_method: crate::upgrade::InstallMethod,
    help: HelpState,
    should_quit: bool,
    /// The runtime key bindings (ADR-0003), with the config's `[keys]`
    /// section applied (Issue #339).
    keymap: crate::keymap::Keymap,
}

impl App {
    /// Open a session on `left` and `right`, from what [`Startup`] resolved:
    /// the settings, keymap, detected tools, install method, and the
    /// command-line overrides. Reads nothing itself.
    ///
    /// [`Startup`]: crate::startup::Startup
    pub fn from_startup(
        left: PathBuf,
        right: PathBuf,
        left_ignore_matcher: IgnoreMatcher,
        right_ignore_matcher: IgnoreMatcher,
        startup: crate::startup::Startup,
    ) -> Self {
        let status_message = startup
            .problem()
            .map(|problem| (problem.toast(), true, Instant::now()));
        let crate::startup::Startup {
            settings,
            store,
            keymap,
            detected_diff_tools,
            install_method,
            overrides,
            update_available,
            update_check_store,
            ..
        } = startup;

        Self {
            roots: Pair::new(
                Root {
                    path: left,
                    ignore: left_ignore_matcher,
                },
                Root {
                    path: right,
                    ignore: right_ignore_matcher,
                },
            ),
            file_pair: None,
            file_pair_info: Pair::default(),
            scan: crate::scan::ScanState::default(),
            view_mode: ViewMode::DirectoryTree,
            file_diff: file_diff_session::FileDiffSession::with_context(settings.diff_context),
            settings: crate::settings::SettingsState::new(settings, store, &overrides),
            mouse_capture: None,
            detected_diff_tools,
            config: ConfigState::default(),
            palette: PaletteState::default(),
            confirm_modal: None,
            status_message,
            directory_tree: DirectoryTreeState::default(),
            // A session starts on the left pane.
            active_side: Side::Left,
            update_available,
            update_check_store,
            install_method,
            help: HelpState::default(),
            should_quit: false,
            keymap,
        }
    }

    /// A test's session: [`crate::startup::Startup::for_test`], with no
    /// exclusions.
    #[cfg(test)]
    pub fn new(left: PathBuf, right: PathBuf) -> Self {
        Self::for_test(left, right, crate::startup::Startup::for_test())
    }

    /// A test's session on the settings [`crate::test_support::seeded_settings`]
    /// holds — every one not at its default — in memory.
    #[cfg(test)]
    pub(crate) fn seeded(left: PathBuf, right: PathBuf) -> Self {
        let settings = crate::test_support::seeded_settings();
        Self::for_test(
            left,
            right,
            crate::startup::Startup::with_settings(settings),
        )
    }

    /// What the last save put in a test's in-memory store.
    #[cfg(test)]
    pub(crate) fn saved_settings(&self) -> crate::settings::AppSettings {
        self.settings.stored().expect("settings were saved")
    }

    /// A test's session from `startup`, with no exclusions.
    #[cfg(test)]
    pub(crate) fn for_test(
        left: PathBuf,
        right: PathBuf,
        startup: crate::startup::Startup,
    ) -> Self {
        let left_ignore = IgnoreMatcher::for_root(left.clone(), &[], true, &[])
            .expect("empty ignore matcher is valid");
        let right_ignore = IgnoreMatcher::for_root(right.clone(), &[], true, &[])
            .expect("empty ignore matcher is valid");
        Self::from_startup(left, right, left_ignore, right_ignore, startup)
    }

    /// The runtime key bindings this session routes and hints from.
    pub(crate) fn keymap(&self) -> &crate::keymap::Keymap {
        &self.keymap
    }

    /// Replace the runtime key bindings, for tests that exercise a remap.
    #[cfg(test)]
    pub(crate) fn set_keymap(&mut self, keymap: crate::keymap::Keymap) {
        self.keymap = keymap;
    }

    /// Apply a finished background scan.
    ///
    /// The scan ends and the Directory Tree adopts its tree together, so a
    /// caller cannot update one without the other. Results from a superseded
    /// [`ScanState::begin`](crate::scan::ScanState::begin) generation are
    /// dropped; returns `false` in that case and leaves the app untouched.
    pub fn apply_scan_result(&mut self, generation: u64, node: AlignedNode) -> bool {
        if !self.scan.finish(generation) {
            return false;
        }
        self.directory_tree.adopt(node);
        true
    }

    /// Take a progress report from background scan `generation`.
    pub fn apply_scan_progress(&mut self, generation: u64, count: usize) {
        self.scan.progress(generation, count);
    }

    /// Report a failed background scan, unless a newer scan superseded it.
    pub fn apply_scan_error(&mut self, generation: u64, message: &str) {
        if self.scan.finish(generation) {
            self.set_status(format!("Scan failed: {message}"), true);
        }
    }

    /// Apply a finished background update check: record it in the store
    /// and show or clear the newer version. A failed check stays silent and
    /// leaves both alone, so the next launch retries at once.
    pub fn apply_update_check_outcome(&mut self, outcome: crate::upgrade::UpdateCheckOutcome) {
        self.update_check_store
            .record(&outcome, crate::upgrade::now_secs());
        match outcome {
            crate::upgrade::UpdateCheckOutcome::Newer(version) => {
                self.update_available = Some(version);
            }
            crate::upgrade::UpdateCheckOutcome::UpToDate => self.update_available = None,
            crate::upgrade::UpdateCheckOutcome::Failed => {}
        }
    }

    pub(crate) fn prepare_tree_viewport(&mut self, visible_height: usize) {
        self.directory_tree.set_visible_height(visible_height);
    }

    pub(crate) fn prepare_diff_viewport(&mut self, visible_height: usize, pane_inner_width: usize) {
        self.diff_mut().set_frame(visible_height, pane_inner_width);
    }

    /// Set a transient status message displayed in the footer.
    /// `is_error` = true → red styling, false → green styling.
    pub fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_message = Some((msg.into(), is_error, Instant::now()));
    }

    /// Active footer toast, if any: `(message, is_error)`.
    ///
    /// Layout uses [`.is_some()`](Option::is_some) for footer height; render and
    /// tests use the payload. Does not expose the private expiry timestamp.
    pub(crate) fn status_toast(&self) -> Option<(&str, bool)> {
        self.status_message
            .as_ref()
            .map(|(msg, is_error, _)| (msg.as_str(), *is_error))
    }

    /// Ask the event loop to exit after the current frame.
    pub fn request_quit(&mut self) {
        self.file_diff.close();
        self.should_quit = true;
    }

    /// Whether the event loop should break on the next iteration.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Put `mode` in effect without saving it, as `--scan-mode` does, for
    /// tests.
    #[cfg(test)]
    pub(crate) fn set_scan_mode(&mut self, mode: crate::settings::ScanMode) {
        self.settings.set_scan_mode(mode);
    }

    /// The one scan-mode switch behind the Directory Tree `c` key, the Palette,
    /// and the Config screen, which reports the new mode in a toast.
    pub fn switch_scan_mode(&mut self, mode: crate::settings::ScanMode) {
        self.set_status(format!("Scan mode: {}", mode.label()), false);
        self.change_setting(crate::settings::SettingChange::ScanMode(mode));
    }

    pub fn ignore_matchers(&self) -> (&IgnoreMatcher, &IgnoreMatcher) {
        (&self.roots.left.ignore, &self.roots.right.ignore)
    }

    /// Newer version string, if a completed update check found one.
    pub fn update_available(&self) -> Option<&str> {
        self.update_available.as_deref()
    }

    /// Set the newer-version hint, for tests. Live check outcomes go through
    /// [`App::apply_update_check_outcome`].
    #[cfg(test)]
    pub(crate) fn set_update_available(&mut self, version: Option<String>) {
        self.update_available = version;
    }

    /// How this binary was installed (Homebrew, Scoop, standalone, ...), used to
    /// tailor the update hint's suggested command.
    pub fn install_method(&self) -> &crate::upgrade::InstallMethod {
        &self.install_method
    }

    pub fn refresh_diff_tools(&mut self) {
        self.detected_diff_tools = crate::diff_tool::detect_diff_tools();
    }

    pub fn resolve_auto_diff_tool(&self) -> Option<crate::diff_tool::ExternalDiffTool> {
        self.detected_diff_tools
            .iter()
            .find(|(_, avail)| *avail)
            .map(|(tool, _)| *tool)
    }

    pub fn resolve_effective_diff_tool(&self) -> Option<crate::diff_tool::ExternalDiffTool> {
        match &self.settings.saved().external_diff_tool {
            crate::settings::DiffToolSetting::Auto => self.resolve_auto_diff_tool(),
            crate::settings::DiffToolSetting::Disabled => None,
            crate::settings::DiffToolSetting::Pinned(tool) => {
                if self
                    .detected_diff_tools
                    .iter()
                    .any(|(t, avail)| t == tool && *avail)
                {
                    Some(*tool)
                } else {
                    None
                }
            }
            crate::settings::DiffToolSetting::Unknown(_) => None,
        }
    }

    /// The Config screen's rows, for a test to find one by kind.
    #[cfg(test)]
    pub(crate) fn config_rows(&self) -> Vec<ConfigRowKind> {
        ConfigState::rows(self.config_context())
    }

    /// The Settings this session runs with. Changes go through
    /// [`App::change_setting`].
    pub fn settings(&self) -> &crate::settings::SettingsState {
        &self.settings
    }

    pub(crate) fn detected_diff_tools(&self) -> &[(crate::diff_tool::ExternalDiffTool, bool)] {
        &self.detected_diff_tools
    }

    /// Put a Settings change in effect, save it, and carry out what else it
    /// affects: new ignore matchers, and a rescan left for the event loop.
    /// A save that fails is reported; the change lasts until duodiff exits.
    ///
    /// Returns `false`, with an error toast and nothing changed, when the
    /// ignore matchers cannot be built for the change.
    pub(crate) fn change_setting(&mut self, change: crate::settings::SettingChange) -> bool {
        let roots = if change.reshapes_ignore_rules() {
            let rules = self.settings.ignore_rules_after(&change);
            let built = self.roots.as_ref().try_map(|root| {
                rules.matcher(root.path.clone()).map(|ignore| Root {
                    path: root.path.clone(),
                    ignore,
                })
            });
            match built {
                Ok(roots) => Some(roots),
                Err(error) => {
                    self.set_status(format!("Cannot rebuild exclusions: {error}"), true);
                    return false;
                }
            }
        } else {
            None
        };
        let applied = self.settings.apply(change);
        if let Some(roots) = roots {
            self.roots = roots;
        }
        match applied.effect {
            crate::settings::SettingEffect::None => {}
            crate::settings::SettingEffect::Rescan => self.request_rescan(),
            crate::settings::SettingEffect::MouseCapture(on) => self.mouse_capture = Some(on),
            crate::settings::SettingEffect::DiffContext(lines) => {
                self.diff_mut().set_context(lines)
            }
        }
        if let Err(error) = applied.saved {
            self.set_status(format!("Cannot save configuration: {error}"), true);
        }
        true
    }

    /// The terminal refused to switch mouse capture to `wanted`: keep what
    /// it still does in effect, and say so.
    pub(crate) fn mouse_capture_failed(&mut self, wanted: bool, error: std::io::Error) {
        self.settings.set_mouse_in_effect(!wanted);
        self.set_status(format!("Cannot switch mouse support: {error}"), true);
    }

    /// Ask the event loop for a background scan of both roots.
    pub(crate) fn request_rescan(&mut self) {
        self.scan.request_full();
    }

    /// Ask for the tree to follow a copy of `copied` (a directory when
    /// `copied_is_dir`): a background scan of just the directory it landed
    /// in, when [`ScanState::request_directory`](crate::scan::ScanState::request_directory)
    /// can keep it to that.
    pub(crate) fn request_subtree_rescan(&mut self, copied: &Path, copied_is_dir: bool) {
        let path = if copied_is_dir {
            copied.to_path_buf()
        } else {
            copied.parent().map(Path::to_path_buf).unwrap_or_default()
        };
        self.scan.request_directory(path);
    }

    /// Apply a finished background scan of the directory at `path`: graft
    /// it into the tree, or scan the whole tree when the directory is no
    /// longer in it. A result from a superseded scan is dropped.
    pub fn apply_subtree_scan_result(&mut self, generation: u64, path: &Path, node: AlignedNode) {
        if !self.scan.finish(generation) {
            return;
        }
        if !self.directory_tree.graft_subtree(path, node) {
            self.request_rescan();
        }
    }

    /// The mouse capture to switch the terminal to, when a change asked for
    /// one since the event loop last looked. Report a refusal with
    /// [`App::mouse_capture_failed`].
    pub(crate) fn take_mouse_capture(&mut self) -> Option<bool> {
        self.mouse_capture.take()
    }

    /// What changes have left for the event loop since it last looked —
    /// the scan to start, and the mouse capture to switch to — taken, so a
    /// test sees only what the next step asks for.
    #[cfg(test)]
    pub(crate) fn take_pending(&mut self) -> (Option<crate::scan::ScanJob>, Option<bool>) {
        (self.scan.take_next(), self.take_mouse_capture())
    }

    /// Flip between the dark and light theme and persist the choice.
    pub fn toggle_theme(&mut self) {
        let theme = self.settings.saved().theme.toggled();
        self.set_status(format!("Theme: {}", theme.label()), false);
        self.change_setting(crate::settings::SettingChange::Theme(theme));
    }

    /// The view currently shown. Production code navigates only through named
    /// transitions (`request_file_diff`, `leave_file_diff`, `open_config`,
    /// `close_config`, `open_help`, `close_help`); this getter is read-only.
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// Open `target` (Config or Help), remembering the current view so `Esc`/`q` can
    /// return to it. No-op (returns `false`) while already on `target` — otherwise the
    /// top bar's mouse click for this overlay (reachable from any view, including the
    /// overlay itself) would overwrite the remembered return view, trapping Esc/`q`
    /// with no way out via the keyboard. Returns `true` when it actually transitioned,
    /// so callers can gate per-screen setup on that.
    fn open_overlay(&mut self, target: ViewMode) -> bool {
        if self.view_mode == target {
            return false;
        }
        match target {
            ViewMode::ConfigMenu => self.config.set_return_view(self.view_mode),
            ViewMode::Help => self.help.set_return_view(self.view_mode),
            _ => unreachable!("open_overlay is only used for the ConfigMenu/Help targets"),
        }
        self.view_mode = target;
        true
    }

    /// Open the Config screen, remembering the current view so `Esc`/`q` can return to it.
    pub fn open_config(&mut self) {
        if self.open_overlay(ViewMode::ConfigMenu) {
            self.refresh_diff_tools();
            let (config, context) = self.config_in_context_mut();
            config.ensure_selection(context);
        }
    }

    /// Leave Config and restore the view remembered by [`App::open_config`].
    ///
    /// Shared by Esc / `q` / mouse close-button on the Config screen. Pure restore:
    /// `view_mode = config's return view` only — no other side effects.
    pub(crate) fn close_config(&mut self) {
        self.view_mode = self.config.return_view();
    }

    /// Read access to the Config screen's own state (selected row, scroll,
    /// return view, exclusion editor). Production code changes it through
    /// [`App::open_config`]/`close_config`/[`App::config_gesture`]/
    /// [`App::exclusion_editor_key`].
    pub(crate) fn config(&self) -> &ConfigState {
        &self.config
    }

    /// Mutable access to the Config screen's own state: frame preparation
    /// reaches the scroll and the editor viewport through here, and tests
    /// seed a selection directly.
    pub(crate) fn config_mut(&mut self) -> &mut ConfigState {
        &mut self.config
    }

    /// What the Config screen lists its rows from.
    pub(crate) fn config_context(&self) -> ConfigContext<'_> {
        ConfigContext {
            detected_diff_tools: &self.detected_diff_tools,
            settings: &self.settings,
        }
    }

    /// The Config screen's state beside what it lists its rows from, for a
    /// caller that updates the one from the other.
    pub(crate) fn config_in_context_mut(&mut self) -> (&mut ConfigState, ConfigContext<'_>) {
        (
            &mut self.config,
            ConfigContext {
                detected_diff_tools: &self.detected_diff_tools,
                settings: &self.settings,
            },
        )
    }

    /// Answer a Config `gesture` and carry out what it asks for.
    pub(crate) fn config_gesture(&mut self, gesture: ConfigGesture) {
        let (config, context) = self.config_in_context_mut();
        let intent = config.handle(gesture, context);
        self.apply_config_intent(intent);
    }

    /// Edit the open exclusion draft with `key`, and carry out what it asks for.
    pub(crate) fn exclusion_editor_key(&mut self, key: crossterm::event::KeyEvent) {
        let intent = self.config.exclusion_editor_key(key);
        self.apply_config_intent(intent);
    }

    fn apply_config_intent(&mut self, intent: ConfigIntent) {
        match intent {
            ConfigIntent::None => {}
            ConfigIntent::Change(change) => {
                self.change_setting(change);
            }
            ConfigIntent::SwitchScanMode(mode) => self.switch_scan_mode(mode),
            ConfigIntent::ToggleTheme => self.toggle_theme(),
            ConfigIntent::OpenExclusionEditor => self.open_exclusion_editor(),
            ConfigIntent::ApplyExclusions(rules) => self.apply_exclusions(rules),
        }
    }

    /// Open the global exclusion editor on the saved rules.
    pub(crate) fn open_exclusion_editor(&mut self) {
        self.config
            .open_exclusion_editor(self.settings.saved().global_exclusions.clone());
    }

    /// Save `rules` as the global exclusions and close the editor, unless a
    /// rule does not hold under either root: then the editor stays open on
    /// that rule and a toast says why.
    fn apply_exclusions(&mut self, rules: Vec<String>) {
        for side in Side::BOTH {
            let root = &self.roots.side(side).path;
            if let Some((index, error)) = rules.iter().enumerate().find_map(|(index, pattern)| {
                IgnoreMatcher::validate_patterns(root, std::slice::from_ref(pattern))
                    .err()
                    .map(|error| (index, error))
            }) {
                self.config.reject_exclusion(index);
                self.set_status(format!("Invalid exclusion {}: {error}", index + 1), true);
                return;
            }
        }
        if self.change_setting(crate::settings::SettingChange::GlobalExclusions(rules)) {
            self.config.close_exclusion_editor();
        }
    }

    /// Left-hand directory being compared. Read access only; mutate via [`App::swap_paths`].
    pub fn left_path(&self) -> &Path {
        &self.roots.left.path
    }

    /// Right-hand directory being compared. Read access only; mutate via [`App::swap_paths`].
    pub fn right_path(&self) -> &Path {
        &self.roots.right.path
    }

    /// Swap the left and right roots, each with its own ignore rules, and
    /// reset selection state.
    pub fn swap_paths(&mut self) {
        self.roots.swap();
        self.directory_tree.reset_cursor();
        self.file_diff.reset_for_swap();
    }

    /// Clear the status message if it has been visible longer than `duration`.
    pub fn clear_expired_status(&mut self, duration: std::time::Duration) {
        if let Some((_, _, created)) = &self.status_message {
            if created.elapsed() >= duration {
                self.status_message = None;
            }
        }
    }

    /// Read access to the file-diff content state (rows, scroll, wrap/full
    /// toggles, cached hashes/line-endings). Loading and saving go through
    /// File Diff's session (`file_diff_session`); rendering reads through
    /// [`crate::view::diff`]/[`crate::view::diff_layout_inputs`] instead of this directly.
    pub(crate) fn diff(&self) -> &FileDiffState {
        self.file_diff.content()
    }

    /// Mutable access to the file-diff content state. See [`App::diff`].
    pub(crate) fn diff_mut(&mut self) -> &mut FileDiffState {
        self.file_diff.content_mut()
    }

    /// Replace the current user's home directory with `~` for status text
    /// and confirmation dialogs.
    pub(crate) fn display_path_with_home_tilde(path: &Path) -> String {
        if let Some(home) = crate::settings::AppSettings::home_dir() {
            if let Ok(rest) = path.strip_prefix(&home) {
                return if rest.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    let rest = rest.to_string_lossy().replace('\\', "/");
                    format!("~/{rest}")
                };
            }
        }
        path.display().to_string()
    }

    /// The currently selected filtered row, if any.
    pub(crate) fn selected_row(&self) -> Option<&FlatRow> {
        self.directory_tree.selected_row()
    }

    /// "(press D for external diff)" — or the Palette, once the keymap leaves
    /// it unbound — names the way out of a file File Diff cannot show
    /// (Issue #339).
    fn external_diff_hint(&self) -> String {
        match self
            .keymap
            .key_phrase(crate::commands::Command::ExternalDiff)
        {
            Some(key) => format!(" (press {key} for external diff)"),
            None => " (external diff from the Command Palette)".to_string(),
        }
    }

    /// Recompute the built-in diff for the file pair File Diff shows.
    ///
    /// Returns `Err` when a side is binary, non-UTF-8, or over the size limit so
    /// callers can surface a toast instead of opening an empty/false view.
    pub fn refresh_file_diff(&mut self) -> Result<(), String> {
        let loaded = if let Some(pair) = &self.file_pair {
            let loaded = pair.as_ref().try_map(|side| {
                side.load()
                    .map_err(|cause| format!("{}: {cause}", side.path().display()))
            })?;
            self.file_pair_info = pair.as_ref().map(crate::target::FileSide::info);
            loaded
        } else {
            let Some(files) = self.diff_file_paths() else {
                return Err("no file selected".to_string());
            };
            let hint = self.external_diff_hint();
            files.try_map(|path| {
                crate::diff_view::LoadedText::from_path(&path, &hint).map_err(|e| e.to_string())
            })?
        };
        self.file_diff.content_mut().load(loaded);
        Ok(())
    }

    /// Open File Diff on a file pair named on the command line (Issue #327).
    ///
    /// The session has no Directory Tree, so leaving File Diff ends it, and
    /// it never scans.
    ///
    /// `loaded` is each side as startup already read it, so nothing is read
    /// again here.
    pub fn open_file_pair(
        &mut self,
        pair: crate::target::FilePair,
        loaded: Pair<crate::diff_view::LoadedText>,
    ) {
        self.file_pair_info = pair.as_ref().map(crate::target::FileSide::info);
        self.file_pair = Some(pair);
        self.scan.never_scan();
        self.file_diff.open_loaded(loaded);
        self.view_mode = ViewMode::FileDiff;
    }

    /// The file pair named on the command line, when this session compares two
    /// files rather than two directories.
    pub(crate) fn file_pair(&self) -> Option<&crate::target::FilePair> {
        self.file_pair.as_ref()
    }

    /// Flip full-file vs. diff-only content in the diff view, at the
    /// configured context size.
    pub fn toggle_diff_show_full(&mut self) {
        self.diff_mut().toggle_show_full();
    }

    /// Open File Diff on the selected Directory Tree row, reading both sides
    /// in the background.
    pub(crate) fn request_file_diff(&mut self) -> Result<(), String> {
        let row = self.selected_row().cloned().ok_or("no file selected")?;
        let files = self.diff_file_paths().ok_or("no file selected")?;
        let hint = self.external_diff_hint();
        let context = self.settings.saved().diff_context;
        self.file_diff.open(row, files, hint, context);
        self.view_mode = ViewMode::FileDiff;
        Ok(())
    }

    /// File Diff's load in flight, if any.
    pub(crate) fn diff_loading(&self) -> &file_diff_session::LoadState {
        self.file_diff.loading()
    }

    /// The File Diff load for the event loop to run, once.
    pub(crate) fn take_file_diff_job(&mut self) -> Option<file_diff_session::LoadJob> {
        self.file_diff.take_job()
    }

    /// Take a finished File Diff load. One that failed to open returns to the
    /// Directory Tree and says why.
    pub(crate) fn apply_file_diff_result(
        &mut self,
        generation: u64,
        result: Result<Box<FileDiffState>, String>,
    ) {
        match self.file_diff.finish(generation, result) {
            file_diff_session::LoadOutcome::Opened | file_diff_session::LoadOutcome::Stale => {}
            file_diff_session::LoadOutcome::OpenFailed(error) => {
                if self.view_mode == ViewMode::FileDiff {
                    self.view_mode = ViewMode::DirectoryTree;
                }
                if self.config.return_view() == ViewMode::FileDiff {
                    self.config.set_return_view(ViewMode::DirectoryTree);
                }
                if self.help.return_view() == ViewMode::FileDiff {
                    self.help.set_return_view(ViewMode::DirectoryTree);
                }
                self.set_status(format!("Cannot open diff: {error}"), true);
            }
        }
    }

    /// Leave the File Diff view and return to the Directory Tree, or end the
    /// session when File Diff was opened directly on a file pair.
    ///
    /// Shared by Esc/`q`, the mouse close glyph, the post-copy return-to-tree, and
    /// the command palette's "back" action.
    pub fn leave_file_diff(&mut self) {
        self.file_diff.close();
        if self.file_pair.is_some() {
            self.request_quit();
        } else {
            self.view_mode = ViewMode::DirectoryTree;
        }
    }

    /// Stage the change hunk at the current scroll position in the given
    /// direction. Nothing is written — `[` / `]` edit the working buffers and an
    /// explicit save commits them (Issue #235).
    ///
    /// Returns whether the hunk actually changed a buffer, so the caller can
    /// report "nothing to stage" rather than a save that never happened
    /// (Issue #315) — an ordinary hunk always does, but one whose sides are
    /// already byte-identical (a stale index, or a hunk `stage_hunk_copy`
    /// resolved without changing anything) does not.
    pub fn stage_hunk_at_cursor(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        self.diff_mut().stage_active_hunk(direction)
    }

    /// Undo the most recent staged hunk operation.
    pub fn undo_staged_hunk(&mut self) -> bool {
        self.diff_mut().undo_staged()
    }

    /// The entries the scan listed under `relative_path` on one side, as
    /// `(path relative to that directory, is_dir)` in parent-before-child order.
    ///
    /// `None` when the row is not a directory in the scan model, in which case
    /// the caller copies the single leaf instead. Driving directory copy from
    /// this snapshot is what keeps excluded entries (`.git`, …) and files
    /// created after the scan out of the copy (Issue #235).
    pub fn scanned_subtree_entries(
        &self,
        relative_path: &Path,
        side: Side,
    ) -> Option<Vec<(PathBuf, bool)>> {
        let root = self.directory_tree.root_node()?;
        let node = find_node(root, relative_path)?;
        let present = match side {
            Side::Left => node.left.as_ref(),
            Side::Right => node.right.as_ref(),
        };
        if !present.is_some_and(|f| f.is_dir) {
            return None;
        }
        let mut entries = Vec::new();
        collect_scanned_entries(node, relative_path, side, &mut entries);
        Some(entries)
    }

    /// Relative path of the currently selected filtered row, if any.
    pub fn selected_relative_path(&self) -> Option<PathBuf> {
        self.selected_row().map(|r| r.relative_path.clone())
    }

    /// The background scan: in flight or not, progress, generation.
    pub(crate) fn scan(&self) -> &crate::scan::ScanState {
        &self.scan
    }

    /// Drive the background scan's own state.
    pub(crate) fn scan_mut(&mut self) -> &mut crate::scan::ScanState {
        &mut self.scan
    }

    /// Which pane has focus.
    pub(crate) fn active_side_left(&self) -> bool {
        self.active_side == Side::Left
    }

    pub(crate) fn focus_left_pane(&mut self) {
        self.active_side = Side::Left;
    }

    pub(crate) fn focus_right_pane(&mut self) {
        self.active_side = Side::Right;
    }

    pub(crate) fn toggle_active_side(&mut self) {
        self.active_side = self.active_side.other();
    }

    /// The Directory Tree: its tree, expand state, rows, filter, and cursor.
    pub(crate) fn directory_tree(&self) -> &DirectoryTreeState {
        &self.directory_tree
    }

    /// Drive the Directory Tree. Each operation leaves it consistent.
    pub(crate) fn directory_tree_mut(&mut self) -> &mut DirectoryTreeState {
        &mut self.directory_tree
    }

    /// Show a confirmation. `Commands` composes the prompt and shows it
    /// together with the approval it waits for; `App` holds it as the data
    /// the renderer draws (ADR-0003).
    pub(crate) fn show_confirm(&mut self, modal: ConfirmModal) {
        self.confirm_modal = Some(modal);
    }

    /// Open a yes/no confirmation with a single body line, for tests that need
    /// a pending dialog without caring which Command opened it.
    #[cfg(test)]
    pub fn request_confirm(&mut self, message: impl Into<String>, action: ConfirmAction) {
        self.confirm_modal = Some(ConfirmModal {
            title: "Confirm".to_string(),
            headline: message.into(),
            lines: Vec::new(),
            choices: vec![
                ConfirmChoice {
                    key: 'y',
                    label: "Yes".to_string(),
                    action,
                },
                ConfirmChoice {
                    key: 'n',
                    label: "No".to_string(),
                    action: ConfirmAction::Cancel,
                },
            ],
        });
    }

    /// Absolute destination paths a save would write, left side first.
    pub fn staged_save_targets(&self) -> Vec<PathBuf> {
        match self.diff_file_paths() {
            Some(files) => self.file_diff.save_targets(&files),
            None => Vec::new(),
        }
    }

    /// Write every dirty side, all-or-nothing; see
    /// [`FileDiffSession::save`](file_diff_session::FileDiffSession::save).
    pub fn save_staged(&mut self) -> Result<StagedSave, std::io::Error> {
        if !self.diff().is_dirty() {
            return Ok(StagedSave::Written);
        }
        let Some(files) = self.diff_file_paths() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no file selected",
            ));
        };
        // A file-pair side that is not a regular file (the null device, a pipe)
        // was never written and has no file to hash again.
        let rehash = Pair::from_fn(|side| {
            self.file_pair
                .as_ref()
                .is_none_or(|pair| pair.side(side).is_regular_file())
        });
        let saved = self.file_diff.save(&files, rehash)?;
        if saved == StagedSave::Written {
            if let Some(pair) = &self.file_pair {
                self.file_pair_info = pair.as_ref().map(crate::target::FileSide::info);
            }
        }
        Ok(saved)
    }

    /// Re-read both sides from disk, throwing away the staged edits.
    pub fn reload_discarding_staged(&mut self) -> Result<(), String> {
        self.refresh_file_diff()?;
        self.diff_mut().clamp_scroll();
        Ok(())
    }

    /// Throw away staged edits without touching disk.
    pub fn discard_staged(&mut self) {
        self.diff_mut().discard_staged();
    }

    /// Close the confirm modal. `Commands` closes it as it settles an answer,
    /// together with the approval it was waiting for.
    pub fn dismiss_confirm(&mut self) {
        self.confirm_modal = None;
    }

    /// The pending confirm modal, if one is open. Read access for rendering.
    pub fn confirm_modal(&self) -> Option<&ConfirmModal> {
        self.confirm_modal.as_ref()
    }

    /// Open the Help screen, remembering the current view so `Esc`/`q`/`?` can
    /// return to it, and jumping straight to that view's contextual topic body (the topic
    /// index is only shown once the user explicitly presses Tab).
    pub fn open_help(&mut self) {
        if !self.open_overlay(ViewMode::Help) {
            return;
        }
        let topic = HelpTopic::for_view(self.help.return_view());
        self.help.enter(topic);
    }

    /// Leave Help: restore `view_mode` from the Help state's remembered return
    /// view and close the topic index. Unifies the body-Esc and index-Esc paths
    /// (body already has the index closed; closing it again is a no-op UX-wise).
    pub(crate) fn close_help(&mut self) {
        self.view_mode = self.help.return_view();
        self.help.leave();
    }

    /// Read access to the Help screen's own state (active topic, topic index,
    /// scroll, return view). Production code drives it through [`App::open_help`]/
    /// [`App::close_help`] plus [`HelpState`]'s own methods (see `input.rs`).
    pub(crate) fn help(&self) -> &HelpState {
        &self.help
    }

    /// Mutable access to the Help screen's own state. See [`App::help`].
    pub(crate) fn help_mut(&mut self) -> &mut HelpState {
        &mut self.help
    }

    /// Open the Command Palette. Shared by `;`, `Ctrl+p`, and right-click, so all
    /// three land on the same contextual inventory: the query is cleared and the
    /// first enabled action is selected (Issue #239).
    pub(crate) fn open_palette(&mut self) {
        self.palette.open();
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Query edit: append one character, re-filter, and reselect.
    pub(crate) fn palette_type_char(&mut self, c: char) {
        self.palette.push_query_char(c);
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Query edit: drop the trailing character, re-filter, and reselect.
    pub(crate) fn palette_backspace(&mut self) {
        self.palette.pop_query_char();
        self.refresh_palette_items();
        self.palette.select_first_enabled();
    }

    /// Rebuild `palette.items` from the Command inventory, keeping only the
    /// entries whose key or label contains the query — a case-insensitive
    /// substring search, not fuzzy matching. Called on every query edit and once
    /// per frame from `view::prepare_frame`.
    pub(crate) fn refresh_palette_items(&mut self) {
        let query = self.palette.query().to_lowercase();
        let items = crate::commands::inventory_entries(self)
            .into_iter()
            .filter(|a| {
                a.label.to_lowercase().contains(&query) || a.key.to_lowercase().contains(&query)
            })
            .collect();
        self.palette.set_items(items);
    }

    /// Read access for render / hit-test (mode, query, items, selected_idx).
    pub(crate) fn palette(&self) -> &PaletteState {
        &self.palette
    }

    /// Drive the palette's own state: selection, viewport, dismissal.
    pub(crate) fn palette_mut(&mut self) -> &mut PaletteState {
        &mut self.palette
    }

    /// Convenience for the many `if app.palette().visible()` guards.
    pub(crate) fn palette_visible(&self) -> bool {
        self.palette.visible()
    }
}

/// Depth-first lookup of the aligned node at `relative_path`.
fn find_node<'a>(node: &'a AlignedNode, relative_path: &Path) -> Option<&'a AlignedNode> {
    if node.relative_path == relative_path
        || node.left_relative_path.as_deref() == Some(relative_path)
        || node.right_relative_path.as_deref() == Some(relative_path)
    {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node(child, relative_path))
}

/// Flatten `node`'s descendants that exist on the chosen side into paths
/// relative to `base`, parents before children so directories are created first.
fn collect_scanned_entries(
    node: &AlignedNode,
    base: &Path,
    side: Side,
    out: &mut Vec<(PathBuf, bool)>,
) {
    for child in &node.children {
        let (info, relative_path) = match side {
            Side::Left => (child.left.as_ref(), &child.left_relative_path),
            Side::Right => (child.right.as_ref(), &child.right_relative_path),
        };
        let Some(info) = info else {
            continue;
        };
        let child_path = relative_path.as_deref().unwrap_or(&child.relative_path);
        let Ok(relative) = child_path.strip_prefix(base) else {
            continue;
        };
        out.push((relative.to_path_buf(), info.is_dir));
        if info.is_dir {
            collect_scanned_entries(child, base, side, out);
        }
    }
}

/// Seams for tests in sibling modules, which cannot reach `App`'s private state
/// but still need to stand up a tree, a row list, or a viewport without running a
/// real scan or a real terminal.
#[cfg(test)]
impl App {
    /// Run the queued File Diff load on this thread and apply its result, as
    /// the event loop would once the worker finished.
    pub(crate) fn finish_file_diff_load(&mut self) {
        let job = self.take_file_diff_job().expect("a queued File Diff load");
        let generation = job.generation;
        self.apply_file_diff_result(generation, job.load());
    }

    /// Install a tree and flatten it, as [`App::apply_scan_result`] would.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.directory_tree.set_root_node(node);
        self.directory_tree.refresh_for_test(true);
    }

    /// Reflatten the installed tree and relist it.
    pub(crate) fn flatten_tree(&mut self) {
        self.directory_tree.refresh_for_test(true);
    }

    /// Relist rows a test installed with `set_flat_rows`.
    pub(crate) fn apply_filter(&mut self) {
        self.directory_tree.refresh_for_test(false);
    }

    pub(crate) fn set_active_side_left(&mut self, left: bool) {
        self.active_side = if left { Side::Left } else { Side::Right };
    }

    pub(crate) fn set_view_mode(&mut self, view_mode: ViewMode) {
        self.view_mode = view_mode;
    }

    pub(crate) fn set_theme(&mut self, theme: crate::theme::ThemeChoice) {
        self.settings.saved_mut().theme = theme;
    }

    pub(crate) fn set_external_diff_tool(&mut self, tool: crate::settings::DiffToolSetting) {
        self.settings.saved_mut().external_diff_tool = tool;
    }

    pub(crate) fn set_detected_diff_tools(
        &mut self,
        tools: Vec<(crate::diff_tool::ExternalDiffTool, bool)>,
    ) {
        self.detected_diff_tools = tools;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffState, FileInfo};
    use crate::test_support::{
        dir_node, equal_row, file_info, flat_row, flat_row_with_sides, listed_paths, nested_tree,
        tree_entry,
    };
    use crate::test_support::{lock_env_tests, ConfigEnvGuard};
    use std::time::SystemTime;

    #[test]
    fn test_begin_scan_bumps_generation() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan().generation(), 0);
        assert!(!app.scan().in_progress());

        let g1 = app.scan_mut().begin();
        assert_eq!(g1, 1);
        assert_eq!(app.scan().generation(), 1);
        assert!(app.scan().in_progress());

        let g2 = app.scan_mut().begin();
        assert_eq!(g2, 2);
        assert_eq!(app.scan().generation(), 2);
    }

    /// Issue #250: App tracks scan progress count and ticker animation frames.
    #[test]
    fn test_scan_progress_and_ticker() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.scan().progress_count(), 0);
        assert_eq!(app.scan().spinner_frame(), 0);

        app.scan_mut().tick();
        assert_eq!(app.scan().spinner_frame(), 1);

        let g = app.scan_mut().begin();
        assert!(app.scan().in_progress());
        assert_eq!(app.scan().progress_count(), 0);

        app.apply_scan_progress(g, 75);
        assert_eq!(app.scan().progress_count(), 75);

        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: None,
            right: None,
            state: DiffState::Identical,
            children: vec![],
            ..Default::default()
        };
        assert!(app.apply_scan_result(g, node));
        assert!(!app.scan().in_progress());
        assert_eq!(app.scan().progress_count(), 0);
    }

    /// Issue #252: a finished scan seeds the footer inventory from the tree.
    #[test]
    fn test_tree_footer_summary_counts_the_scanned_tree() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        assert_eq!(app.directory_tree().tree_summary(), None);

        let g = app.scan_mut().begin();
        let node = AlignedNode {
            children: vec![
                AlignedNode {
                    name: "a.txt".into(),
                    relative_path: PathBuf::from("a.txt"),
                    state: DiffState::Identical,
                    ..Default::default()
                },
                AlignedNode {
                    name: "b.txt".into(),
                    relative_path: PathBuf::from("b.txt"),
                    state: DiffState::LeftOnly,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(app.apply_scan_result(g, node));
        assert_eq!(
            app.directory_tree().tree_summary(),
            Some(crate::diff::TreeSummary {
                identical: 1,
                left_only: 1,
                ..Default::default()
            })
        );
        assert!(app.directory_tree().tree_summary().is_some());
    }

    #[test]
    fn test_status_message_lifecycle() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));

        // Initially no status
        assert!(app.status_toast().is_none());

        // Set an error status
        app.set_status("Copy failed: permission denied", true);
        assert!(app.status_toast().is_some());
        let (msg, is_error) = app.status_toast().unwrap();
        assert!(is_error);
        assert!(msg.contains("permission denied"));

        // Should NOT expire with a short duration just after setting
        app.clear_expired_status(std::time::Duration::from_secs(10));
        assert!(app.status_toast().is_some());

        // Should expire with zero duration
        app.clear_expired_status(std::time::Duration::ZERO);
        assert!(app.status_toast().is_none());

        // Set a success status
        app.set_status("Copied 'file.txt'", false);
        let (_, is_error) = app.status_toast().unwrap();
        assert!(!is_error);
    }

    /// Each frame hands Help its topic's size, so the bound follows the
    /// terminal and the topic on screen.
    #[test]
    fn a_frame_bounds_the_help_topic_scroll() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_help();
        app.help_mut().select_topic(HelpTopic::DirectoryTree);
        let area = ratatui::layout::Rect::new(0, 0, 100, 24);
        crate::view::prepare_frame(&mut app, area);
        for _ in 0..500 {
            app.help_mut().move_down();
        }
        // Top bar, footer, and the body's two borders leave 20 rows.
        let lines = crate::view::help_lines(&app).len();
        assert_eq!(usize::from(app.help().scroll()), lines - 20);
    }

    #[test]
    fn test_swap_paths() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        assert_eq!(app.left_path(), PathBuf::from("/left"));
        assert_eq!(app.right_path(), PathBuf::from("/right"));

        app.swap_paths();

        assert_eq!(app.left_path(), PathBuf::from("/right"));
        assert_eq!(app.right_path(), PathBuf::from("/left"));
    }

    #[test]
    fn test_swap_paths_resets_state() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_selected_idx(5);
        app.directory_tree_mut().set_scroll_offset(3);
        app.diff_mut().set_scroll(2);
        app.diff_mut()
            .set_hashes(Some("abc".to_string()), Some("def".to_string()));

        app.swap_paths();

        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert_eq!(app.directory_tree().scroll_offset(), 0);
        assert_eq!(app.diff().scroll(), 0);
        assert!(app.diff().hash(Side::Left).is_none());
        assert!(app.diff().hash(Side::Right).is_none());
    }

    /// A session on a file pair has no directories, so nothing it does asks
    /// for a scan; a directory session asks once however often it is asked.
    #[test]
    fn only_a_directory_session_requests_a_rescan() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_rescan();
        app.request_rescan();
        assert_eq!(app.take_pending(), (Some(crate::scan::ScanJob::Full), None));

        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&left, "a\n").unwrap();
        std::fs::write(&right, "b\n").unwrap();
        let crate::target::ComparisonTarget::Files(pair, loaded) =
            crate::target::resolve(&left, &right).unwrap()
        else {
            panic!("two files resolve to a file pair");
        };
        let mut app = App::new(left, right);
        app.open_file_pair(pair, loaded);
        app.request_rescan();
        assert_eq!(app.take_pending(), (None, None));
    }

    /// File Diff opens on the sides startup read to check them, rather than
    /// reading each file a second time.
    #[test]
    fn a_file_pair_opens_on_what_startup_read() {
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&left, "a\n").unwrap();
        std::fs::write(&right, "b\n").unwrap();
        let crate::target::ComparisonTarget::Files(pair, loaded) =
            crate::target::resolve(&left, &right).unwrap()
        else {
            panic!("two files resolve to a file pair");
        };
        std::fs::write(&left, "changed after startup\n").unwrap();
        let mut app = App::new(left, right);
        app.open_file_pair(pair, loaded);
        assert_eq!(app.diff().buffer(Side::Left).lines, vec!["a".to_string()]);
    }

    /// A file pair's panes are titled with the paths as typed, while reads
    /// and writes go through a symlink to its file (ADR-0004).
    #[cfg(unix)]
    #[test]
    fn a_file_pair_titles_its_panes_with_the_paths_as_typed() {
        let dir = tempfile::tempdir().unwrap();
        let (real, link, other) = (
            dir.path().join("real.txt"),
            dir.path().join("link.txt"),
            dir.path().join("other.txt"),
        );
        std::fs::write(&real, "a\n").unwrap();
        std::fs::write(&other, "b\n").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let crate::target::ComparisonTarget::Files(pair, loaded) =
            crate::target::resolve(&link, &other).unwrap()
        else {
            panic!("two files resolve to a file pair");
        };
        let mut app = App::new(link.clone(), other.clone());
        app.open_file_pair(pair, loaded);

        let compared = app.compared_pair().unwrap();
        assert_eq!(compared.titles(), Pair::new(link, other));
        assert_eq!(compared.path(Side::Left), real.canonicalize().unwrap());
    }

    /// Each root keeps its own ignore rules across a swap: a rule in the old
    /// left root's `.duodiffignore` hides nothing in the new left root.
    #[test]
    fn swap_paths_keeps_each_roots_ignore_rules() {
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(left.join(".duodiffignore"), "secret.txt\n").unwrap();
        let matcher =
            |root: &PathBuf| IgnoreMatcher::for_root(root.clone(), &[], true, &[]).unwrap();
        let mut app = App::from_startup(
            left.clone(),
            right.clone(),
            matcher(&left),
            matcher(&right),
            crate::startup::Startup::for_test(),
        );

        app.swap_paths();

        let (new_left, new_right) = app.ignore_matchers();
        let secret = Path::new("secret.txt");
        assert!(!new_left.clone().is_ignored(secret, false).unwrap());
        assert!(new_right.clone().is_ignored(secret, false).unwrap());
    }

    #[test]
    fn test_swap_paths_twice_restores() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.swap_paths();
        app.swap_paths();
        assert_eq!(app.left_path(), PathBuf::from("/left"));
        assert_eq!(app.right_path(), PathBuf::from("/right"));
    }

    #[test]
    fn test_open_help_sets_contextual_topic_and_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.help_mut().set_index_open(true); // prove open_help sets this to false
        app.help_mut().set_scroll(7); // prove open_help resets this

        app.open_help();

        assert_eq!(app.help().return_view(), ViewMode::FileDiff);
        assert_eq!(app.help().topic(), HelpTopic::FileDiff);
        assert!(!app.help().index_open());
        assert_eq!(app.help().scroll(), 0);
        assert_eq!(app.view_mode(), ViewMode::Help);
    }

    #[test]
    fn test_open_help_while_already_on_help_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_help();
        assert_eq!(app.help().return_view(), ViewMode::FileDiff);

        // Calling open_help() again while already on Help (e.g. clicking the top bar's
        // (?)Help hotspot from within Help itself) must be a no-op — otherwise
        // help_return_view would be overwritten with ViewMode::Help, trapping Esc/`?`/q in
        // Help with no keyboard way out.
        app.open_help();
        assert_eq!(app.help().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::Help);
    }

    #[test]
    fn test_open_help_index_syncs_selection_to_current_topic() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_help();
        app.help_mut().set_topic(HelpTopic::Mouse);

        app.help_mut().open_index();

        assert!(app.help().index_open());
        assert_eq!(app.help().index_sel(), 3);
    }

    #[test]
    fn test_close_help_index_stays_on_help_and_closes_index_only() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.open_help();
        app.help_mut().open_index();
        assert!(app.help().index_open());

        app.help_mut().close_index();

        assert!(!app.help().index_open());
        assert_eq!(
            app.view_mode(),
            ViewMode::Help,
            "unlike close_help, closing just the index must not leave Help"
        );
    }

    #[test]
    fn test_open_config_remembers_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();

        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);
    }

    #[test]
    fn test_open_config_while_already_on_config_does_not_trap_keyboard_exit() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);

        // Calling open_config() again while already on Config (e.g. clicking the top bar's
        // (C)onfig hotspot from within Config itself) must be a no-op — otherwise
        // config().return_view() would be overwritten with ViewMode::ConfigMenu, trapping Esc/q
        // in Config with no keyboard way out.
        app.open_config();
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);
    }

    #[test]
    fn test_close_config_restores_return_view() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.set_view_mode(ViewMode::FileDiff);

        app.open_config();
        assert_eq!(app.view_mode(), ViewMode::ConfigMenu);

        app.close_config();
        assert_eq!(app.view_mode(), ViewMode::FileDiff);
        assert_eq!(app.config().return_view(), ViewMode::FileDiff);
    }

    #[test]
    fn config_diff_tool_selection_and_unknown_row() {
        // activating a row persists, so the config dir has to be redirected.
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Auto);
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, true),
            (crate::diff_tool::ExternalDiffTool::Nvim, false),
        ]);

        // Default setting is Auto
        assert_eq!(
            app.settings().saved().external_diff_tool,
            crate::settings::DiffToolSetting::Auto
        );
        assert_eq!(
            app.resolve_effective_diff_tool(),
            Some(crate::diff_tool::ExternalDiffTool::Vim)
        );

        // Select Disabled (row 2)
        app.config_gesture(ConfigGesture::Click(2));
        assert_eq!(app.config().selected_idx(), 2);
        assert_eq!(app.take_pending(), (None, None));
        assert_eq!(
            app.settings().saved().external_diff_tool,
            crate::settings::DiffToolSetting::Disabled
        );
        assert_eq!(app.resolve_effective_diff_tool(), None);

        // Select Vim (row 3, available)
        app.config_gesture(ConfigGesture::Click(3));
        assert_eq!(app.config().selected_idx(), 3);
        assert_eq!(app.take_pending(), (None, None));
        assert_eq!(
            app.settings().saved().external_diff_tool,
            crate::settings::DiffToolSetting::Pinned(crate::diff_tool::ExternalDiffTool::Vim)
        );
        assert_eq!(
            app.resolve_effective_diff_tool(),
            Some(crate::diff_tool::ExternalDiffTool::Vim)
        );

        // If Vim becomes unavailable, Pinned(Vim) resolves to None and does not fall back
        app.set_detected_diff_tools(vec![
            (crate::diff_tool::ExternalDiffTool::Vim, false),
            (crate::diff_tool::ExternalDiffTool::Nvim, true),
        ]);
        assert_eq!(app.resolve_effective_diff_tool(), None);

        // Set an unknown tool
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Unknown(
            "custom-diff".to_string(),
        ));
        let rows = app.config_rows();
        assert!(rows.contains(&ConfigRowKind::DiffToolUnknown));
        assert_eq!(app.resolve_effective_diff_tool(), None);
    }

    #[test]
    fn config_view_abbreviates_home_directory_in_ignore_sources() {
        let _guard = ConfigEnvGuard::new();
        let home = PathBuf::from(std::env::var("HOME").expect("ConfigEnvGuard sets HOME"));
        let other = PathBuf::from("/opt/other");
        let app = App::new(home.join("Notes"), other.clone());
        assert_eq!(
            App::display_path_with_home_tilde(app.left_path()),
            "~/Notes"
        );
        assert_eq!(
            App::display_path_with_home_tilde(app.right_path()),
            other.display().to_string()
        );

        let app = App::new(home, other);
        assert_eq!(App::display_path_with_home_tilde(app.left_path()), "~");
    }

    #[test]
    fn exclusion_editor_apply_persists_rules_and_requests_one_rescan() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_exclusion_editor();
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for ch in "*.generated".chars() {
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(app.take_pending(), (Some(crate::scan::ScanJob::Full), None));
        assert!(app.config().exclusion_editor().is_none());
        assert!(app
            .settings()
            .saved()
            .global_exclusions
            .iter()
            .any(|p| p == "*.generated"));
    }

    #[test]
    fn an_invalid_exclusion_keeps_the_editor_open_on_it() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let saved = app.settings().saved().global_exclusions.clone();
        app.open_exclusion_editor();
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for ch in "[z-a]".chars() {
            app.exclusion_editor_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.exclusion_editor_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        app.exclusion_editor_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let editor = app.config().exclusion_editor().expect("editor stays open");
        assert_eq!(
            editor.selected_idx(),
            saved.len(),
            "the bad rule is highlighted"
        );
        let (msg, is_error, _) = app.status_message.clone().unwrap();
        assert!(is_error, "{msg}");
        assert!(
            msg.starts_with(&format!("Invalid exclusion {}: ", saved.len() + 1)),
            "{msg}"
        );
        assert_eq!(app.take_pending(), (None, None));
        assert_eq!(app.settings().saved().global_exclusions, saved);
    }

    /// Issue #238: applying the Config scan-mode row persists, updates the
    /// effective mode, and asks the caller for exactly one background rescan.
    #[test]
    fn test_config_scan_mode_row_persists_and_requests_a_rescan() {
        use crate::settings::ScanMode;
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
            "a scan-mode change needs a rescan"
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        assert_eq!(app.saved_settings().scan_mode, ScanMode::Fast);
        assert!(!app.settings().scan_mode_is_session_override());

        // Rows that do not affect scanning never ask for a rescan.
        let theme_idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(theme_idx);
        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(app.take_pending(), (None, None));
    }

    /// A scan mode that cannot be saved still takes effect and rescans, like
    /// every other setting; the toast says it lasts only this session.
    #[cfg(unix)]
    #[test]
    fn test_scan_mode_save_failure_still_switches_and_rescans() {
        use crate::settings::ScanMode;
        use std::os::unix::fs::PermissionsExt;

        let _guard = ConfigEnvGuard::new();
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup::from_disk(&_guard),
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);

        // Make the seeded config file read-only so `save()`'s truncating write fails.
        let path = crate::settings::AppSettings::config_path().unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        let mut locked = original.clone();
        locked.set_mode(0o444);
        std::fs::set_permissions(&path, locked).unwrap();

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);

        // Restore before asserting so a failure cannot leave the tempdir locked.
        std::fs::set_permissions(&path, original).unwrap();

        assert_eq!(
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
            "the new mode rescans even unsaved"
        );
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);
        let (msg, is_error, _) = app.status_message.clone().unwrap();
        assert!(is_error, "{msg}");
        assert!(msg.starts_with("Cannot save configuration: "), "{msg}");
    }

    /// Issue #238: changing scan mode from Config while a File Diff session is
    /// open must not discard that session.
    #[test]
    fn test_scan_mode_change_from_file_diff_keeps_the_diff_session() {
        use crate::settings::ScanMode;
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows(vec![crate::diff_view::DiffRow::from((
                Some(crate::diff_view::DiffLine {
                    tag: similar::ChangeTag::Equal,
                    text: "kept".to_string(),
                }),
                None,
            ))]);
        app.open_config();

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::ScanMode))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(app.take_pending(), (Some(crate::scan::ScanJob::Full), None));
        assert_eq!(app.settings().scan_mode(), ScanMode::Fast);

        app.close_config();
        assert_eq!(app.view_mode(), ViewMode::FileDiff);
        assert_eq!(app.diff().rows().len(), 1, "the diff session is preserved");
    }

    #[test]
    fn test_mouse_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.settings().saved().mouse);
        assert!(!app.settings().mouse());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);
        assert!(app.settings().saved().mouse);
        assert!(app.settings().mouse());
        assert_eq!(app.take_pending(), (None, Some(true)));

        app.config_gesture(ConfigGesture::Activate);
        assert!(!app.settings().saved().mouse);
        assert!(!app.settings().mouse());
        assert_eq!(app.take_pending(), (None, Some(false)));
    }

    /// `--no-mouse` only sets where the session starts: turning mouse
    /// support on in Config turns it on, and saves it.
    #[test]
    fn the_mouse_row_replaces_no_mouse() {
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                overrides: crate::startup::CliOverrides {
                    no_mouse: true,
                    ..Default::default()
                },
                ..crate::startup::Startup::for_test()
            },
        );
        assert!(app.settings().saved().mouse);
        assert!(!app.settings().mouse());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);

        assert!(app.settings().mouse());
        assert!(app.saved_settings().mouse);
        assert_eq!(app.take_pending(), (None, Some(true)));
    }

    /// `--no-gitignore` only sets where the session starts: the Config row
    /// switches what the next scan reads, and saves it.
    #[test]
    fn the_gitignore_row_replaces_no_gitignore() {
        let mut app = App::for_test(
            PathBuf::from("/left"),
            PathBuf::from("/right"),
            crate::startup::Startup {
                overrides: crate::startup::CliOverrides {
                    gitignore: Some(false),
                    ..Default::default()
                },
                ..crate::startup::Startup::for_test()
            },
        );
        assert!(!app.settings().respect_gitignore());

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::RespectGitignore))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);

        assert!(app.settings().respect_gitignore());
        assert!(app.saved_settings().respect_gitignore);
        assert_eq!(app.take_pending(), (Some(crate::scan::ScanJob::Full), None));
    }

    #[test]
    fn test_theme_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Light
        );
        assert_eq!(app.settings().theme(), crate::theme::Theme::LIGHT);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Theme))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Dark
        );
        assert_eq!(app.settings().theme(), crate::theme::Theme::DARK);

        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(
            app.settings().saved().theme,
            crate::theme::ThemeChoice::Light
        );
    }

    #[test]
    fn test_diff_context_adjust_persists_and_clamps() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert_eq!(app.settings().saved().diff_context, 7);

        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();
        app.config_mut().set_selected_idx(idx);

        app.config_gesture(ConfigGesture::Increase);
        assert_eq!(app.settings().saved().diff_context, 8);
        app.config_gesture(ConfigGesture::Decrease);
        app.config_gesture(ConfigGesture::Decrease);
        assert_eq!(app.settings().saved().diff_context, 6);

        // Clamped at 0 (saturating_sub), not underflowing.
        for _ in 0..10 {
            app.config_gesture(ConfigGesture::Decrease);
        }
        assert_eq!(app.settings().saved().diff_context, 0);

        // Clamped at 50.
        for _ in 0..60 {
            app.config_gesture(ConfigGesture::Increase);
        }
        assert_eq!(app.settings().saved().diff_context, 50);
    }

    /// A File Diff open behind Config shows the new context as soon as it
    /// changes, not after the next reload.
    #[test]
    fn a_diff_context_change_redraws_the_open_file_diff() {
        let lines: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let changed = lines.replace("10\n", "ten\n");
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let load = |text: &str| crate::diff_view::LoadedText {
            text: text.to_string(),
            sha256: None,
            line_ending: None,
        };
        app.diff_mut().load(Pair::new(load(&lines), load(&changed)));
        let rows_at_default = app.diff().rows().len();

        app.open_config();
        let idx = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::DiffContext))
            .unwrap();
        app.config_mut().set_selected_idx(idx);
        app.config_gesture(ConfigGesture::Increase);

        assert_eq!(app.diff().rows().len(), rows_at_default + 2);
    }

    /// Finish the background subtree scan `app` asked for, the way the scan
    /// task would, and hand the result back. Returns the directory scanned.
    fn finish_subtree_scan(app: &mut App, left: &Path, right: &Path) -> PathBuf {
        let pending = app.take_pending();
        let (Some(crate::scan::ScanJob::Directory(path)), None) = pending else {
            panic!("a subtree rescan was requested: {pending:?}");
        };
        let generation = app.scan_mut().begin();
        let node = crate::diff::align_directories_with_shared_matcher(
            left,
            right,
            &path,
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        app.apply_subtree_scan_result(generation, &path, node);
        assert!(!app.scan().in_progress());
        path
    }

    #[test]
    fn test_subtree_rescan_after_copying_a_nested_file() {
        use std::fs::{create_dir_all, write};
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        create_dir_all(left.path().join("nested")).unwrap();
        create_dir_all(right.path().join("nested")).unwrap();
        write(left.path().join("nested/a.txt"), "left").unwrap();
        write(right.path().join("nested/a.txt"), "right-old").unwrap();
        write(left.path().join("nested/b.txt"), "only-left").unwrap();

        let root = crate::diff::align_directories_with_shared_matcher(
            left.path(),
            right.path(),
            std::path::Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();

        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_root_node(root);
        // Expand nested so file rows are visible after flatten.
        app.directory_tree_mut()
            .set_expanded_for_test(Path::new("nested"), true);
        app.flatten_tree();
        let before_len = app.directory_tree().flat_rows().len();

        // Simulate copy left → right of b.txt (now both sides have it).
        write(right.path().join("nested/b.txt"), "only-left").unwrap();
        app.request_subtree_rescan(Path::new("nested/b.txt"), false);
        assert_eq!(
            finish_subtree_scan(&mut app, left.path(), right.path()),
            PathBuf::from("nested"),
            "a copied file rescans its directory"
        );

        assert!(
            app.directory_tree()
                .flat_rows()
                .iter()
                .any(|r| r.relative_path == *"nested/b.txt"
                    && r.left.is_some()
                    && r.right.is_some()),
            "copied file should appear on both sides after the subtree rescan"
        );
        // Unrelated root structure should still be present (not empty rebuild only).
        assert!(app.directory_tree().flat_rows().len() >= before_len);
        assert!(app
            .directory_tree()
            .root_node()
            .unwrap()
            .children
            .iter()
            .any(|c| c.name == "nested"));
    }

    #[test]
    fn test_check_updates_toggle_persists_in_settings() {
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.settings().saved().check_updates);

        // Land on CheckUpdates row and toggle.
        app.open_config();
        while !matches!(
            app.config_rows().get(app.config().selected_idx()),
            Some(ConfigRowKind::CheckUpdates)
        ) {
            app.config_gesture(ConfigGesture::MoveDown);
        }
        app.config_gesture(ConfigGesture::Activate);
        assert!(app.settings().saved().check_updates);

        app.config_gesture(ConfigGesture::Activate);
        assert!(!app.settings().saved().check_updates);
    }

    #[test]
    fn test_config_tests_never_touch_real_config_file() {
        // Hold the env lock while reading the *real* (unredirected) config
        // path, so a concurrently running guarded test can't be mid-redirect.
        let lock = lock_env_tests();
        let real_path = crate::settings::AppSettings::config_search_paths()
            .into_iter()
            .next();
        let snapshot = |p: &Option<PathBuf>| {
            p.as_ref().map(|p| {
                (
                    p.exists(),
                    std::fs::metadata(p).ok().and_then(|m| m.modified().ok()),
                )
            })
        };
        let before = snapshot(&real_path);
        drop(lock);

        {
            // Exercise the real write path — a file-backed session saving
            // through a Config gesture — redirected to a tempdir.
            let guard = ConfigEnvGuard::new();
            let mut app = App::for_test(
                PathBuf::from("/left"),
                PathBuf::from("/right"),
                crate::startup::Startup::from_disk(&guard),
            );
            let idx = app
                .config_rows()
                .iter()
                .position(|r| matches!(r, ConfigRowKind::Mouse))
                .unwrap();
            app.config_mut().set_selected_idx(idx);
            app.config_gesture(ConfigGesture::Activate);
            app.config_gesture(ConfigGesture::Activate);
        }

        let _lock = lock_env_tests();
        let after = snapshot(&real_path);
        assert_eq!(
            before, after,
            "exercising the config save path must not modify the real config file"
        );
    }

    #[test]
    fn test_first_run_detect_does_not_require_saved_config() {
        // Auto-pick in memory is fine; the important contract is we no longer
        // force-save on construction (save failures / missing home still OK).
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // If a tool was auto-detected it lives only in the in-memory settings
        // until the user confirms via Config — load() may still return default
        // when no file exists, which is acceptable.
        let _ = app.settings().saved().external_diff_tool;
    }

    #[test]
    fn test_focus_pane_shortcuts() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.active_side_left());

        app.focus_right_pane();
        assert!(!app.active_side_left());

        app.focus_left_pane();
        assert!(app.active_side_left());
    }

    #[test]
    fn test_toggle_active_side_flips_focus() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.active_side_left(), "starts on left");

        app.toggle_active_side();
        assert!(!app.active_side_left());

        app.toggle_active_side();
        assert!(app.active_side_left());

        // Test-only setter for fixtures that should not go through focus_* intent.
        app.set_active_side_left(false);
        assert!(!app.active_side_left());
        app.toggle_active_side();
        assert!(app.active_side_left());
    }

    #[test]
    fn test_request_quit_sets_should_quit() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.should_quit());

        app.request_quit();
        assert!(app.should_quit());
    }

    /// Issue #238: `--scan-mode` seeds the session only. It never writes the
    /// config, Config annotates the mismatch as a session override, and the
    /// first in-app change persists and clears the annotation.
    #[test]
    fn test_cli_scan_mode_overrides_the_session_without_persisting() {
        use crate::settings::ScanMode;
        let mut app = App::seeded(PathBuf::from("/left"), PathBuf::from("/right"));
        // The seeded config persists Precise, so that is the effective mode.
        assert_eq!(app.settings().scan_mode(), ScanMode::Precise);
        assert!(app.settings().scan_mode().is_precise());
        assert!(!app.settings().scan_mode_is_session_override());

        // What `--scan-mode fast` does at bootstrap.
        app.set_scan_mode(ScanMode::Fast);
        assert!(!app.settings().scan_mode().is_precise());
        assert_eq!(
            app.settings().saved().scan_mode,
            ScanMode::Precise,
            "the CLI value must not write the config file"
        );
        assert!(app.settings().scan_mode_is_session_override());

        // An in-app change persists, so effective and saved agree again.
        app.switch_scan_mode(ScanMode::Fast);
        assert_eq!(app.settings().saved().scan_mode, ScanMode::Fast);
        assert!(!app.settings().scan_mode_is_session_override());
        assert_eq!(
            app.saved_settings().scan_mode,
            ScanMode::Fast,
            "an in-app change persists"
        );
    }

    #[test]
    fn test_copy_hunk_at_cursor_updates_target_file() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        write(right_dir.path().join("merge.txt"), "keep\nright-line\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_scroll(1);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        // Staged only: the working buffer changed, disk did not (Issue #235).
        assert!(app.diff().dirty(Side::Right));
        assert!(app
            .diff()
            .buffer(Side::Right)
            .to_text()
            .contains("left-line"));
        let on_disk = read_to_string(right_dir.path().join("merge.txt")).unwrap();
        assert!(on_disk.contains("right-line"), "nothing is written yet");

        app.save_staged().expect("save should succeed");
        let right_text = read_to_string(right_dir.path().join("merge.txt")).unwrap();
        assert!(right_text.contains("left-line"));
        assert!(!right_text.contains("right-line"));
        assert!(!app.diff().is_dirty(), "a save clears the dirty state");
    }

    #[test]
    fn test_staged_hunk_undo_restores_the_working_buffers_without_writing() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::{read_to_string, write};
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        write(right_dir.path().join("merge.txt"), "keep\nright-line\n").unwrap();
        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 16,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 17,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().unwrap();
        app.diff_mut().set_scroll(1);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .unwrap();
        assert!(app.diff().dirty(Side::Right));
        assert!(app.diff().can_undo());

        assert!(app.undo_staged_hunk());
        assert!(!app.diff().is_dirty());
        assert!(!app.diff().can_undo());
        assert_eq!(
            app.diff().buffer(Side::Right).to_text(),
            "keep\nright-line\n"
        );
        assert_eq!(
            read_to_string(right_dir.path().join("merge.txt")).unwrap(),
            "keep\nright-line\n"
        );
    }

    #[test]
    fn test_save_conflict_offers_only_reload_or_cancel() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("merge.txt"), "keep\nleft-line\n").unwrap();
        write(right_dir.path().join("merge.txt"), "keep\nright-line\n").unwrap();
        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 16,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 17,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().unwrap();
        app.diff_mut().set_scroll(1);
        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .unwrap();
        write(right_dir.path().join("merge.txt"), "external edit\n").unwrap();

        let StagedSave::Conflicted(paths) = app.save_staged().unwrap() else {
            panic!("an external edit must prevent writing");
        };
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("merge.txt"));
        assert!(
            app.diff().is_dirty(),
            "a conflict must keep the staged edit intact"
        );

        assert!(matches!(
            app.save_staged().unwrap(),
            StagedSave::Conflicted(_)
        ));
        app.reload_discarding_staged().unwrap();
        assert!(!app.diff().is_dirty());
        assert_eq!(app.diff().buffer(Side::Right).to_text(), "external edit\n");
    }

    #[test]
    fn test_stage_hunk_at_cursor_targets_hunk_navigated_to_near_eof() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        // A front hunk and a tail hunk (the last line, right at EOF), with
        // enough context between them that the whole diff fits one viewport —
        // `max_scroll()` is 0, so `scroll` alone can't track which hunk
        // `N` last navigated to (Issue: trailing hunks couldn't be staged).
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        // A leading context line keeps the front hunk's offset > the initial
        // scroll (0), so the first `jump_to_change(true)` lands on it rather
        // than skipping straight to the tail hunk.
        write(
            left_dir.path().join("f.txt"),
            "keep1\nfront-old\nkeep2\nkeep3\nkeep4\nkeep5\ntail-old\n",
        )
        .unwrap();
        write(
            right_dir.path().join("f.txt"),
            "keep1\nfront-new\nkeep2\nkeep3\nkeep4\nkeep5\ntail-new\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");

        // Viewport comfortably fits the whole diff.
        app.diff_mut().set_text_frame(20, 40);
        assert_eq!(app.diff().max_scroll(), 0);

        // Navigate past the front hunk to the tail hunk.
        app.diff_mut().jump_to_change(true);
        app.diff_mut().jump_to_change(true);

        // Simulate the next event-loop iteration's prepare_frame call, which
        // used to silently clamp `scroll` (and with it, the active hunk) back
        // to the front hunk before the next keypress was handled.
        app.diff_mut().set_text_frame(20, 40);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        let right_text = app.diff().buffer(Side::Right).to_text();
        assert!(
            right_text.contains("tail-old"),
            "staging should replace the tail hunk, not the front one: {right_text:?}"
        );
        assert!(
            right_text.contains("front-new"),
            "the front hunk must stay untouched: {right_text:?}"
        );
    }

    #[test]
    fn test_stage_hunk_at_cursor_targets_hunk_navigated_to_near_eof_in_diff_only_view() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        // Same bug as `..._near_eof`, but in the default Diff Only (collapsed)
        // view — the shape the report reproduced in: a big gap of unchanged
        // lines between the two hunks collapses to a short "…" omitted row, so
        // the whole collapsed diff fits one viewport just like the Full-mode
        // case, but through the omitted-range path rather than a short file.
        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let mut left_lines = vec!["keep1".to_string(), "front-old".to_string()];
        let mut right_lines = vec!["keep1".to_string(), "front-new".to_string()];
        for n in 1..=200 {
            left_lines.push(format!("keep{n}"));
            right_lines.push(format!("keep{n}"));
        }
        left_lines.push("tail-old".to_string());
        right_lines.push("tail-new".to_string());
        write(left_dir.path().join("f.txt"), left_lines.join("\n") + "\n").unwrap();
        write(
            right_dir.path().join("f.txt"),
            right_lines.join("\n") + "\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        // Default is Diff Only (show_full = false).
        app.refresh_file_diff().expect("diff should load");

        // The collapsed view (front hunk + context, an omitted gap, tail hunk
        // + context) is far shorter than the raw 203-line file.
        app.diff_mut().set_text_frame(56, 40);
        assert_eq!(app.diff().max_scroll(), 0);
        assert!(
            app.diff().rows().len() < 30,
            "the collapsed view should be much shorter than the raw file: {} rows",
            app.diff().rows().len()
        );

        app.diff_mut().jump_to_change(true);
        app.diff_mut().jump_to_change(true);
        app.diff_mut().set_text_frame(56, 40);

        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should stage");

        let right_text = app.diff().buffer(Side::Right).to_text();
        assert!(
            right_text.contains("tail-old"),
            "staging should replace the tail hunk, not the front one"
        );
        assert!(
            right_text.contains("front-new"),
            "the front hunk must stay untouched"
        );
    }

    /// Issue #315: two real files whose last line differs only by a trailing
    /// newline — `[` must actually resolve it (not just report success while
    /// leaving the difference in place).
    #[test]
    fn test_stage_hunk_at_cursor_resolves_a_trailing_newline_only_hunk() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("f.txt"), "keep\n```").unwrap();
        write(right_dir.path().join("f.txt"), "keep\n```\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_text_frame(20, 40);

        let changed = app
            .stage_hunk_at_cursor(HunkCopyDirection::RightToLeft)
            .expect("hunk copy should not error");

        assert!(changed, "adopting the trailing newline is a real change");
        assert!(!app.diff().has_changes(), "the diff should now be empty");
        assert_eq!(
            app.diff().buffer(Side::Left).to_text(),
            app.diff().buffer(Side::Right).to_text()
        );
    }

    /// Issue #315: staging a hunk that turns out to be a no-op must not claim
    /// success — the caller relies on the returned bool to pick its toast.
    #[test]
    fn test_stage_hunk_at_cursor_reports_no_change_for_a_true_no_op() {
        use crate::diff::FileInfo;
        use crate::diff_view::{DiffLine, DiffRow, HunkCopyDirection};
        use similar::ChangeTag;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("f.txt"), "same\n").unwrap();
        write(right_dir.path().join("f.txt"), "same\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("f.txt"),
            name: "f.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().expect("diff should load");
        app.diff_mut().set_text_frame(20, 40);
        // A synthetic no-op hunk the real diff pipeline would never produce,
        // seeded directly to exercise the safety net (see
        // `stage_hunk_copy`'s own no-op test for the same shape).
        app.diff_mut().set_rows(vec![DiffRow::content(
            Some(DiffLine {
                tag: ChangeTag::Delete,
                text: "same\n".to_string(),
            }),
            Some(DiffLine {
                tag: ChangeTag::Insert,
                text: "same\n".to_string(),
            }),
            Some(0),
            Some(0),
        )]);

        let changed = app
            .stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .expect("hunk copy should not error");

        assert!(!changed);
        assert!(
            !app.diff().is_dirty(),
            "a no-op must not be reported as staged"
        );
    }

    #[test]
    fn test_selected_row_none_when_empty_or_out_of_range() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.selected_row().is_none());

        app.directory_tree_mut().set_rows(vec![flat_row("a.txt")]);
        app.directory_tree_mut().set_selected_idx(0);
        assert_eq!(app.selected_row().map(|r| r.name.as_str()), Some("a.txt"));

        app.directory_tree_mut().set_selected_idx(1);
        assert!(app.selected_row().is_none());
    }

    /// Issue #239: every launcher opens the same surface, and every open clears
    /// the query and lands on the first enabled action.
    #[test]
    fn test_open_palette_clears_the_query_and_selects_the_first_enabled_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(!app.palette_visible());

        app.open_palette();
        assert!(app.palette_visible());
        assert!(app.palette().query().is_empty());
        // With no row selected the first few Directory Tree actions are gated,
        // so the selection must skip past them.
        let selected = app.palette().items()[app.palette().selected_idx()].clone();
        assert!(selected.enabled(), "{selected:?}");
        assert!(
            app.palette().items()[..app.palette().selected_idx()]
                .iter()
                .all(|a| !a.enabled()),
            "the first enabled action wins"
        );

        app.palette_type_char('x');
        assert_eq!(app.palette().query(), "x");
        app.palette_mut().close();
        assert!(!app.palette_visible());
        assert!(app.palette().query().is_empty(), "close clears query");

        app.open_palette();
        assert!(app.palette().query().is_empty(), "open clears query");
    }

    #[test]
    fn test_palette_select_next_prev_wraps() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        let keymap = crate::keymap::Keymap::default();
        app.palette_mut().set_items(vec![
            crate::commands::CommandEntry::new("A", crate::commands::Command::Help, &keymap),
            crate::commands::CommandEntry::new("B", crate::commands::Command::Quit, &keymap),
        ]);
        app.palette_mut().set_selected_idx(0);

        app.palette_mut().select_next();
        assert_eq!(app.palette().selected_idx(), 1);
        app.palette_mut().select_next();
        assert_eq!(app.palette().selected_idx(), 0, "wraps around");
        app.palette_mut().select_prev();
        assert_eq!(app.palette().selected_idx(), 1, "wraps backward");
    }

    /// Issue #239: case-insensitive substring search, not fuzzy matching.
    #[test]
    fn test_palette_search_is_case_insensitive_substring() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::DirectoryTree);
        app.open_palette();
        for c in "QUIT".chars() {
            app.palette_type_char(c);
        }

        assert!(
            app.palette()
                .items()
                .iter()
                .any(|a| a.command == crate::commands::Command::Quit),
            "an upper-case query must still match the lower-case label"
        );
        assert!(
            app.palette()
                .items()
                .iter()
                .all(|a| a.label.to_lowercase().contains("quit")
                    || a.key.to_lowercase().contains("quit")),
            "every remaining item must match the query"
        );

        // A query that matches nothing empties the list; the popup renders its
        // own non-selectable notice rather than a stale selection.
        for c in "zzz".chars() {
            app.palette_type_char(c);
        }
        assert!(app.palette().items().is_empty());

        // Fuzzy subsequence matching is explicitly not what this does.
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        app.palette_backspace();
        assert_eq!(app.palette().query(), "");
        for c in "qit".chars() {
            app.palette_type_char(c);
        }
        assert!(
            app.palette()
                .items()
                .iter()
                .all(|a| a.command != crate::commands::Command::Quit),
            "\"qit\" is a subsequence of \"quit\" but not a substring"
        );
    }

    /// Issue #239: a selection past the bottom of a long inventory scrolls into view.
    #[test]
    fn test_sync_palette_viewport_keeps_the_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.open_palette();
        let keymap = crate::keymap::Keymap::default();
        app.palette_mut().set_items(
            (0..20)
                .map(|i| {
                    crate::commands::CommandEntry::new(
                        &format!("Action {i}"),
                        crate::commands::Command::Help,
                        &keymap,
                    )
                })
                .collect(),
        );

        app.palette_mut().set_selected_idx(0);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 0);

        // The ninth item is the first that does not fit an 8-row viewport.
        app.palette_mut().set_selected_idx(8);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 1, "scrolls just far enough");

        app.palette_mut().set_selected_idx(19);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 12);

        app.palette_mut().set_selected_idx(0);
        app.palette_mut().sync_viewport(8);
        assert_eq!(app.palette().scroll_offset(), 0, "scrolls back up");
    }

    #[test]
    fn test_prepare_frame_tree_derives_visible_height() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        // 24 rows = 1 top bar + 22 body + 1 footer; the pane's two borders are
        // not content.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 20);

        // A status toast grows the footer by one row, shrinking the body.
        app.set_status("copied", false);
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 19);
    }

    #[test]
    fn test_prepare_frame_diff_derives_geometry_from_area() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        // 24 rows = 1 header + 1 info bar + 21 body + 1 footer; 80 columns split
        // in half leaves 38 inner columns per pane, minus a 1-digit gutter (6).
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let diff = app.diff();
        assert_eq!(diff.visible_height(), 19);
        assert_eq!(diff.content_width(), 32);
        assert_eq!(diff.max_line_width(), 100);
        assert_eq!(
            diff.physical_rows(),
            1,
            "no wrapping: one logical row is one physical row"
        );
    }

    #[test]
    fn test_collapsed_diff_gutter_uses_file_line_count_not_visible_rows() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let mut left_lines = Vec::new();
        let mut right_lines = Vec::new();
        for i in 0..100 {
            if i == 50 {
                left_lines.push("left-mid");
                right_lines.push("right-mid");
            } else {
                left_lines.push("same");
                right_lines.push("same");
            }
        }
        write(
            left_dir.path().join("big.txt"),
            left_lines.join("\n") + "\n",
        )
        .unwrap();
        write(
            right_dir.path().join("big.txt"),
            right_lines.join("\n") + "\n",
        )
        .unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("big.txt"),
            name: "big.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_show_full(false);
        app.refresh_file_diff().expect("diff should load");
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));

        assert!(
            app.diff().rows().len() < 20,
            "collapsed view shows a handful of rows, not the whole file"
        );
        assert_eq!(
            app.diff().content_width(),
            30,
            "100-line files keep a 3-digit gutter (38 inner - 8) even when collapsed"
        );
    }

    #[test]
    fn test_prepare_frame_diff_counts_wrapped_rows() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows(vec![equal_row(&"a".repeat(100)), equal_row("short")]);
        app.diff_mut().set_wrap(true);

        // 100 chars over 32 text columns (38 inner minus the gutter) wraps to 4
        // rows, plus 1 for "short".
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff().physical_rows(), 5);

        // Halving the width re-wraps: 100 chars over 12 text columns is 9 rows.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 40, 24));
        let diff = app.diff();
        assert_eq!(diff.content_width(), 12);
        assert_eq!(diff.physical_rows(), 10);
    }

    #[test]
    fn test_prepare_frame_after_resize_clamps_diff_paging() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut()
            .set_rows((0..40).map(|i| equal_row(&format!("line {i}"))).collect());

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 18, "page step is visible_height - 1");

        // Growing the terminal shows more rows at once, so the bottom of the
        // document now sits at a smaller scroll offset. The sync itself must pull
        // the current position back inside the new geometry — otherwise the next
        // page-down would appear to scroll backwards.
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 40));
        assert_eq!(app.diff().visible_height(), 35);
        assert_eq!(app.diff().scroll(), 5, "clamped to 40 rows - 35 visible");
        app.diff_mut().page_down();
        assert_eq!(app.diff().scroll(), 5, "already at the bottom, stays put");
    }

    #[test]
    fn test_prepare_frame_clamps_horizontal_scroll_to_longest_line() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_view_mode(ViewMode::FileDiff);
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(100))]);

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let max_h_scroll = app.diff().max_h_scroll();
        app.diff_mut().set_h_scroll(max_h_scroll);
        assert_eq!(
            app.diff().h_scroll(),
            68,
            "100 chars less the 32 text columns"
        );

        // Opening a shorter file must not leave the pane scrolled past its end.
        app.diff_mut().set_rows(vec![equal_row(&"a".repeat(50))]);
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.diff().h_scroll(), 18);
    }

    #[test]
    fn test_prepare_frame_ignores_help_and_config_views() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let geometry = |app: &App| {
            let diff = app.diff();
            (
                diff.visible_height(),
                diff.content_width(),
                diff.max_line_width(),
                diff.physical_rows(),
            )
        };
        let tree_viewport = geometry(&app);

        app.open_help();
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 120, 60));
        assert_eq!(
            geometry(&app),
            tree_viewport,
            "Help scrolls by its own drawn lines and must not disturb list geometry"
        );
    }

    #[test]
    fn test_apply_scan_result_updates_tree_flag_and_rows_together() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let generation = app.scan_mut().begin();
        assert!(app.scan().in_progress());

        assert!(app.apply_scan_result(generation, dir_node("root")));

        assert!(!app.scan().in_progress(), "scan is no longer in flight");
        assert_eq!(app.directory_tree().flat_rows().len(), 1);
        assert_eq!(app.directory_tree().flat_rows()[0].name, "root");
        assert_eq!(
            app.directory_tree().rows().len(),
            1,
            "filter view rebuilt too"
        );
    }

    #[test]
    fn test_apply_scan_result_ignores_stale_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.scan_mut().begin();
        app.apply_scan_result(stale, dir_node("first"));
        app.scan_mut().begin();

        assert!(!app.apply_scan_result(stale, dir_node("stale")));

        assert_eq!(
            app.directory_tree().flat_rows()[0].name,
            "first",
            "tree left untouched"
        );
        assert!(app.scan().in_progress(), "still waiting for the newer scan");
    }

    #[test]
    fn test_apply_scan_result_restores_expanded_directories() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let mut node = dir_node("root");
        node.children[0].children.push(AlignedNode {
            name: "leaf.txt".to_string(),
            relative_path: PathBuf::from("root/leaf.txt"),
            left: Some(FileInfo {
                is_dir: false,
                size: 1,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            expanded_by_default: false,
            ..Default::default()
        });

        let generation = app.scan_mut().begin();
        app.apply_scan_result(generation, node.clone());
        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        // A rescan returns the subdirectory collapsed; the expand state the user
        // had must survive.
        let mut collapsed = node;
        collapsed.children[0].expanded_by_default = false;
        let generation = app.scan_mut().begin();
        app.apply_scan_result(generation, collapsed);
        assert_eq!(
            app.directory_tree().flat_rows().len(),
            2,
            "root stayed expanded"
        );
    }

    #[test]
    fn test_fail_scan_clears_flag_only_for_current_generation() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let stale = app.scan_mut().begin();
        app.scan_mut().begin();

        assert!(!app.scan_mut().finish(stale));
        assert!(app.scan().in_progress(), "stale failure changes nothing");

        let current = app.scan().generation();
        assert!(app.scan_mut().finish(current));
        assert!(!app.scan().in_progress());
    }

    #[test]
    fn test_apply_update_check_outcome_updates_hint_state_per_outcome() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Newer(
            "0.9.0".to_string(),
        ));
        assert_eq!(app.update_available(), Some("0.9.0"));
        let recorded = app.update_check_store.load();
        assert_eq!(recorded.latest_seen, "0.9.0");
        assert!(recorded.last_check > 0);

        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::UpToDate);
        assert_eq!(app.update_available(), None);
        assert_eq!(app.update_check_store.load().latest_seen, "");

        app.set_update_available(Some("0.7.0".to_string()));
        let before = app.update_check_store.load();
        app.apply_update_check_outcome(crate::upgrade::UpdateCheckOutcome::Failed);
        assert_eq!(
            app.update_available(),
            Some("0.7.0"),
            "Failed must stay silent and leave the previous hint alone"
        );
        assert_eq!(
            app.update_check_store.load(),
            before,
            "a failed check is not recorded against the throttle"
        );
    }

    #[test]
    fn test_request_confirm_opens_modal_with_message_and_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        assert!(app.confirm_modal().is_none());

        app.request_confirm(
            "Copy foo.txt to right side?",
            ConfirmAction::CopyLeftToRight,
        );

        let modal = app.confirm_modal().expect("modal should be open");
        assert_eq!(modal.headline, "Copy foo.txt to right side?");
        assert!(modal.lines.is_empty());
        assert_eq!(modal.default_action(), Some(ConfirmAction::CopyLeftToRight));
        assert_eq!(modal.cancel_action(), Some(ConfirmAction::Cancel));
    }

    #[test]
    fn test_copy_preview_left_to_right_describes_a_create_when_left_is_present() {
        // Real roots, so the absolute paths the preview builds are the same
        // shape on every platform.
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_flat_rows(vec![{
            let mut row = flat_row_with_sides(Some(file_info(false)), None);
            row.name = "foo.txt".to_string();
            row.state = crate::diff::DiffState::LeftOnly;
            row
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        let preview = app.copy_preview(&app.plan_copy(CopyDirection::LeftToRight).unwrap());

        assert_eq!(preview.kind, CopyKind::Create);
        assert_eq!(preview.source_name, "foo.txt");
        assert_eq!(preview.source, left.path().join("entry"));
        assert_eq!(preview.destination, right.path().join("entry"));
        assert!(!preview.case_mismatch);
    }

    #[test]
    fn test_copy_preview_right_to_left_describes_a_create_when_right_is_present() {
        let left = tempfile::tempdir().unwrap();
        let right = tempfile::tempdir().unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_flat_rows(vec![{
            let mut row = flat_row_with_sides(None, Some(file_info(false)));
            row.name = "bar.txt".to_string();
            row.state = crate::diff::DiffState::RightOnly;
            row
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        let preview = app.copy_preview(&app.plan_copy(CopyDirection::RightToLeft).unwrap());

        assert_eq!(preview.kind, CopyKind::Create);
        assert_eq!(preview.source_name, "bar.txt");
        assert_eq!(preview.source, right.path().join("entry"));
        assert_eq!(preview.destination, left.path().join("entry"));
    }

    #[test]
    fn test_plan_copy_refuses_when_the_source_side_is_missing() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_flat_rows(vec![flat_row_with_sides(None, Some(file_info(false)))]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        // Right-only row: copying left-to-right has nothing to copy from.
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NothingToCopy)
        );
    }

    #[test]
    fn test_plan_copy_refuses_when_nothing_is_selected() {
        let app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));

        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NoSelection)
        );
    }

    #[test]
    fn test_dismiss_confirm_closes_modal_and_discards_action() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_confirm(
            "Copy foo.txt to right side?",
            ConfirmAction::CopyLeftToRight,
        );

        app.dismiss_confirm();

        assert!(app.confirm_modal().is_none());
    }

    /// Issue #247: Pressing Enter on an identical binary file emits an actionable status toast
    /// instead of silently failing with no feedback.
    #[test]
    fn test_opening_an_identical_binary_file_explains_why() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        let bin_content = b"PNG\0\r\n\x1a\n\0\0\0\rIHDR";
        write(left_dir.path().join("image.png"), bin_content).unwrap();
        write(right_dir.path().join("image.png"), bin_content).unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("image.png"),
            name: "image.png".to_string(),
            state: crate::diff::DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: bin_content.len() as u64,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: bin_content.len() as u64,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();

        app.request_file_diff().unwrap();
        app.finish_file_diff_load();
        assert_eq!(
            app.view_mode(),
            ViewMode::DirectoryTree,
            "View mode should return to DirectoryTree"
        );
        let (error, is_error) = app.status_toast().expect("the failure is reported");
        assert!(is_error);
        assert!(
            error.contains("binary file not supported"),
            "the reason should explain binary file not supported: {error}"
        );
        assert!(
            error.contains("image.png"),
            "the reason should mention filename: {error}"
        );
        assert!(
            error.contains("press D for external diff"),
            "the reason should suggest pressing D: {error}"
        );
    }

    #[test]
    fn test_opening_an_identical_text_file_opens_file_diff() {
        use crate::diff::FileInfo;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left_dir = tempdir().unwrap();
        let right_dir = tempdir().unwrap();
        write(left_dir.path().join("doc.txt"), "hello world\n").unwrap();
        write(right_dir.path().join("doc.txt"), "hello world\n").unwrap();

        let mut app = App::new(
            left_dir.path().to_path_buf(),
            right_dir.path().to_path_buf(),
        );
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("doc.txt"),
            name: "doc.txt".to_string(),
            state: crate::diff::DiffState::Identical,
            left: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 12,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();

        app.request_file_diff().unwrap();
        app.finish_file_diff_load();
        assert_eq!(app.status_toast(), None);
        assert_eq!(
            app.view_mode(),
            ViewMode::FileDiff,
            "View mode should change to FileDiff"
        );
    }

    #[test]
    fn test_empty_tree_produces_no_flat_rows_and_none_selection() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let root = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            state: DiffState::Identical,
            children: vec![],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(root);

        assert!(app.directory_tree().flat_rows().is_empty());
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);
        assert_eq!(app.selected_relative_path(), None);
        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert_eq!(app.directory_tree().scroll_offset(), 0);
    }

    #[test]
    fn test_empty_tree_navigation_and_actions_are_noops() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let root = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(root);

        // Navigation does not change 0 indices or panic
        app.directory_tree_mut().select_next();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().select_prev();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().page_down();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        app.directory_tree_mut().page_up();
        assert_eq!(app.directory_tree().selected_idx(), 0);
        assert!(!app.directory_tree_mut().select_row_at(0));
        assert!(!app.directory_tree_mut().select_row_at(5));

        // Diff and edit actions refuse on empty selection
        assert!(app.request_file_diff().is_err());
        assert!(app.refresh_file_diff().is_err());
        assert!(app
            .stage_hunk_at_cursor(crate::diff_view::HunkCopyDirection::LeftToRight)
            .is_err());

        // Expand/collapse actions are safe no-ops
        app.directory_tree_mut().expand_selected();
        app.directory_tree_mut().collapse_selected();
        assert!(app.directory_tree().flat_rows().is_empty());

        // Copy actions have nothing to plan
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NoSelection)
        );
        assert_eq!(
            app.plan_copy(CopyDirection::RightToLeft),
            Err(CopyRefusal::NoSelection)
        );
    }

    #[test]
    fn test_plan_copy_refuses_the_synthetic_root_row() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        // Even if a synthetic root row were manually injected into flat_rows
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from(""),
            name: String::new(),
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::NothingToCopy),
            "the synthetic root is never a copy target"
        );
    }

    #[test]
    fn test_filtering_complete_tree_hides_all_entries_without_exposing_root() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            state: DiffState::Identical,
            children: vec![
                AlignedNode {
                    name: "alpha.txt".to_string(),
                    relative_path: PathBuf::from("alpha.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    state: DiffState::Identical,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
                AlignedNode {
                    name: "beta.txt".to_string(),
                    relative_path: PathBuf::from("beta.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    state: DiffState::Identical,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        app.set_root_node(node);
        assert_eq!(app.directory_tree().flat_rows().len(), 2);

        // Filter for nonexistent pattern
        app.directory_tree_mut().set_pattern("gamma");
        app.apply_filter();
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);
        assert_eq!(app.directory_tree().selected_idx(), 0);

        // Filter diffs-only when all entries are identical
        app.directory_tree_mut().clear();
        app.directory_tree_mut().open();
        app.directory_tree_mut().toggle_diffs_only();
        app.directory_tree_mut().commit();
        app.apply_filter();
        assert!(app.directory_tree().rows().is_empty());
        assert_eq!(app.selected_row(), None);

        // Filtering with pattern "" and diffs-only disabled should match all real entries, not the root
        app.directory_tree_mut().clear();
        app.directory_tree_mut().set_pattern("");
        app.apply_filter();
        assert_eq!(app.directory_tree().rows().len(), 2);
        assert_eq!(app.directory_tree().rows()[0].name, "alpha.txt");
        assert_eq!(app.directory_tree().rows()[1].name, "beta.txt");
    }

    #[test]
    fn test_request_copy_rejects_ambiguous_case_collision() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut().set_flat_rows(vec![FlatRow {
            name: "Foo".to_string(),
            relative_path: PathBuf::from("Foo"),
            is_ambiguous_case_collision: true,
            state: DiffState::LeftOnly,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            ..Default::default()
        }]);
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(0);

        // Attempting to copy ambiguous collision to right side
        assert_eq!(
            app.plan_copy(CopyDirection::LeftToRight),
            Err(CopyRefusal::AmbiguousCaseCollision)
        );
    }

    /// Issue #338: a full rescan hands back both-sided directories expanded, so
    /// one the user collapsed must be put back collapsed, not only the other way.
    #[test]
    fn a_rescan_keeps_a_collapsed_directory_collapsed() {
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.directory_tree_mut().set_root_node(nested_tree());
        app.directory_tree_mut()
            .set_expanded_for_test(Path::new("a/b"), false);
        app.flatten_tree();

        let generation = app.scan_mut().begin();
        let mut rescanned = nested_tree();
        rescanned.children.push(tree_entry(
            "new",
            true,
            Some(vec![tree_entry("new/x.txt", false, None)]),
        ));
        assert!(app.apply_scan_result(generation, rescanned));
        app.flatten_tree();

        assert_eq!(
            listed_paths(app.directory_tree()),
            ["first.txt", "a", "a/b", "top.txt", "new", "new/x.txt"],
            "a/b stays collapsed; a directory new to the tree keeps the scanner's default"
        );
    }

    /// A copy at the root changes the root directory, so the whole tree is
    /// scanned; the rules for which scan runs are `ScanState`'s.
    #[test]
    fn a_copy_at_the_root_rescans_the_tree() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.request_subtree_rescan(Path::new("a.txt"), false);
        assert_eq!(
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
            "a copy at the root"
        );

        app.request_subtree_rescan(Path::new("dir"), true);
        assert_eq!(
            app.take_pending(),
            (
                Some(crate::scan::ScanJob::Directory(PathBuf::from("dir"))),
                None
            ),
            "a copied directory rescans itself"
        );
    }

    /// A subtree scan whose directory is gone from the tree by the time it
    /// finishes scans the whole tree; one a newer scan superseded is dropped.
    #[test]
    fn a_subtree_scan_result_that_cannot_be_grafted_rescans_the_tree() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.set_root_node(AlignedNode::default());
        let stale = app.scan_mut().begin();
        let current = app.scan_mut().begin();

        app.apply_subtree_scan_result(stale, Path::new("gone"), AlignedNode::default());
        assert_eq!(
            app.take_pending(),
            (None, None),
            "a superseded result is dropped"
        );
        assert!(app.scan().in_progress());

        app.apply_subtree_scan_result(current, Path::new("gone"), AlignedNode::default());
        assert!(!app.scan().in_progress());
        assert_eq!(app.take_pending(), (Some(crate::scan::ScanJob::Full), None));
    }

    /// Issue #338: the partial rescan after a copy grafts a freshly scanned
    /// subtree, which must not reopen a directory the user collapsed.
    #[test]
    fn a_subtree_rescan_keeps_a_collapsed_directory_collapsed() {
        use std::fs::{create_dir_all, write};
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        create_dir_all(left.path().join("nested/inner")).unwrap();
        create_dir_all(right.path().join("nested/inner")).unwrap();
        write(left.path().join("nested/inner/a.txt"), "left").unwrap();

        let root = crate::diff::align_directories_with_shared_matcher(
            left.path(),
            right.path(),
            Path::new(""),
            false,
            &IgnoreMatcher::default(),
        )
        .unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.directory_tree_mut().set_root_node(root);
        app.directory_tree_mut()
            .set_expanded_for_test(Path::new("nested/inner"), false);
        app.flatten_tree();

        write(right.path().join("nested/inner/a.txt"), "left").unwrap();
        app.request_subtree_rescan(Path::new("nested"), true);
        finish_subtree_scan(&mut app, left.path(), right.path());

        assert_eq!(
            listed_paths(app.directory_tree()),
            ["nested", "nested/inner"]
        );
    }

    /// Issue #342: a config file that fails to parse is named in a startup
    /// toast, and no later save overwrites it with the defaults.
    #[test]
    fn a_broken_config_file_is_reported_and_never_overwritten() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        let broken = "theme = \"blue\"\n";
        std::fs::write(&path, broken).unwrap();

        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );

        let (toast, is_error) = app.status_toast().expect("a startup toast");
        assert!(is_error);
        assert!(
            toast.starts_with("Cannot load the config file: "),
            "{toast}"
        );
        assert!(toast.contains("line 1: "), "{toast}");

        app.toggle_theme();
        app.switch_scan_mode(crate::settings::ScanMode::Precise);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    /// A settings change that cannot be saved says so, and still applies for
    /// this session. The broken config file here refuses every save (#342).
    #[test]
    fn a_setting_that_cannot_be_saved_says_so() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        std::fs::write(&path, "theme = \"blue\"\n").unwrap();
        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );
        let refused = |app: &App| {
            let (toast, is_error) = app.status_toast().expect("a toast");
            assert!(is_error, "{toast}");
            assert!(toast.starts_with("Cannot save configuration: "), "{toast}");
        };

        let theme = app.settings().saved().theme;
        app.toggle_theme();
        refused(&app);
        assert_ne!(
            app.settings().saved().theme,
            theme,
            "the change still applies"
        );

        let mouse = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::Mouse))
            .unwrap();
        app.config_mut().set_selected_idx(mouse);
        let mouse_before = app.settings().saved().mouse;
        app.set_status("", false);
        app.config_gesture(ConfigGesture::Activate);
        refused(&app);
        assert_ne!(
            app.settings().saved().mouse,
            mouse_before,
            "the change still applies"
        );
        app.take_pending();

        let gitignore = app
            .config_rows()
            .iter()
            .position(|r| matches!(r, ConfigRowKind::RespectGitignore))
            .unwrap();
        app.config_mut().set_selected_idx(gitignore);
        let respect_before = app.settings().respect_gitignore();
        app.set_status("", false);
        app.config_gesture(ConfigGesture::Activate);
        assert_eq!(
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
            "the new rules still rescan"
        );
        refused(&app);
        assert_ne!(app.settings().respect_gitignore(), respect_before);

        app.open_exclusion_editor();
        app.set_status("", false);
        app.exclusion_editor_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('s'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert_eq!(
            app.take_pending(),
            (Some(crate::scan::ScanJob::Full), None),
            "the edited exclusions still rescan"
        );
        refused(&app);
        assert!(app.config().exclusion_editor().is_none());
    }

    /// Issue #339: `[keys]` from the config file drives the App's keymap, an
    /// ignored entry is named in a startup toast, and a save writes the section
    /// back as the user wrote it — the ignored entry included.
    #[test]
    fn config_keys_reach_the_keymap_and_survive_a_save() {
        let _guard = ConfigEnvGuard::new();
        let path = crate::settings::AppSettings::config_path().unwrap();
        std::fs::write(
            &path,
            "theme = \"light\"\n\n[keys]\ncopy_to_left = \"R\"\ncopy_to_right = \"L\"\nrescan = \"j\"\nhelp = \"x\"\n",
        )
        .unwrap();

        let mut app = App::for_test(
            PathBuf::from("left"),
            PathBuf::from("right"),
            crate::startup::Startup::from_disk(&_guard),
        );

        assert_eq!(
            app.keymap().hint(crate::commands::Command::CopyRightToLeft),
            "R"
        );
        assert_eq!(app.keymap().hint(crate::commands::Command::Refresh), "r");
        assert_eq!(app.keymap().customized_count(), 3);
        assert_eq!(
            app.status_toast(),
            Some((
                "Some key bindings in the config file were ignored: keys.rescan: `j` is handled \
                 by Directory Tree itself and cannot be bound — Fix those [keys] entries; \
                 ignored commands keep their default keys",
                true
            ))
        );

        app.toggle_theme();
        let saved: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(saved["theme"].as_str(), Some("dark"));
        let expected: toml::Table =
            "copy_to_left = \"R\"\ncopy_to_right = \"L\"\nrescan = \"j\"\nhelp = \"x\"\n"
                .parse()
                .unwrap();
        assert_eq!(saved["keys"].as_table(), Some(&expected));
    }

    #[test]
    fn several_ignored_key_bindings_point_at_check() {
        let problems = ["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            crate::startup::key_problem(&problems)
                .map(|p| p.toast())
                .as_deref(),
            Some(
                "Some key bindings in the config file were ignored: a (+2 more — run duodiff \
                 --check) — Fix those [keys] entries; ignored commands keep their default keys"
            )
        );
    }

    #[test]
    fn test_prepare_frame_tree_keeps_selection_visible() {
        let mut app = App::new(PathBuf::from("/left"), PathBuf::from("/right"));
        app.directory_tree_mut()
            .set_flat_rows((0..40).map(|i| flat_row(&format!("f{i}.txt"))).collect());
        app.apply_filter();
        app.directory_tree_mut().set_selected_idx(30);

        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        assert_eq!(app.directory_tree().visible_height(), 20);
        assert_eq!(
            app.directory_tree().scroll_offset(),
            11,
            "selection scrolled into view"
        );
    }
}
