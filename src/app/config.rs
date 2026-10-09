//! The Config screen's state: the selected row, how far the list is
//! scrolled, the view to return to, and the global exclusion editor.
//!
//! [`ConfigState`] lists its own rows from a borrowed [`ConfigContext`],
//! turns each [`ConfigGesture`] into the [`ConfigIntent`] it asks for, and
//! leaves carrying that out to [`App`](super::App), where a setting takes
//! effect.

use crate::diff_tool::ExternalDiffTool;
use crate::settings::{DiffToolSetting, ScanMode, SettingChange, SettingsState};

/// A row in the flat configuration screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigRowKind {
    Header(&'static str),
    DiffToolAuto,
    DiffToolDisabled,
    DiffTool {
        idx: usize,
        available: bool,
    },
    DiffToolUnknown,
    /// Toggle for [`crate::settings::AppSettings::check_updates`].
    CheckUpdates,
    /// Toggle for [`crate::settings::AppSettings::mouse`].
    Mouse,
    /// Toggle for [`crate::settings::AppSettings::theme`].
    Theme,
    /// Numeric adjust for [`crate::settings::AppSettings::diff_context`] (`h`/`l` or
    /// `Left`/`Right`).
    DiffContext,
    /// Toggle for [`crate::settings::AppSettings::scan_mode`]. Applying it
    /// persists, updates the effective mode, and triggers one background rescan.
    ScanMode,
    /// Toggle for reading `.gitignore` files during scans.
    RespectGitignore,
    /// Opens the dedicated global exclusion list editor.
    GlobalExclusions,
    /// Read-only provenance for project and command-line rule sources.
    IgnoreSources,
    /// Read-only count of the `[keys]` entries in effect (Issue #339); keys
    /// are changed in the config file, not here.
    KeyBindings,
}

#[derive(Clone, Debug, Default)]
pub struct ExclusionEditorState {
    draft: Vec<String>,
    selected_idx: usize,
    scroll_offset: usize,
    input: crate::text_input::TextInput,
    editing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExclusionEditorAction {
    None,
    Apply,
    Cancel,
}

impl ExclusionEditorState {
    /// An editor whose draft starts from `draft`, the saved rules.
    fn new(draft: Vec<String>) -> Self {
        Self {
            draft,
            ..Self::default()
        }
    }

    pub(crate) fn draft(&self) -> &[String] {
        &self.draft
    }

    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub(crate) fn input(&self) -> &crate::text_input::TextInput {
        &self.input
    }

    pub(crate) fn editing(&self) -> bool {
        self.editing
    }

    /// Highlight rule `index`, as when it fails to validate.
    fn select(&mut self, index: usize) {
        self.selected_idx = index;
    }

    /// Keep the highlighted rule in a `visible_rows`-tall list viewport.
    fn sync_viewport(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        let max_offset = self.draft.len().saturating_sub(visible_rows);
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_idx + 1 - visible_rows;
        }
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> ExclusionEditorAction {
        use crossterm::event::{KeyCode, KeyModifiers};

        if self.editing {
            match key.code {
                KeyCode::Enter => self.finish_edit(),
                KeyCode::Esc => self.editing = false,
                code => self.input.apply_edit(code),
            }
            return ExclusionEditorAction::None;
        }
        match key.code {
            KeyCode::Esc => return ExclusionEditorAction::Cancel,
            KeyCode::Char('a') => self.add(),
            KeyCode::Enter => self.begin_edit(),
            KeyCode::Char('d') => self.delete(),
            KeyCode::Char('r') => self.restore_defaults(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_prev(),
            KeyCode::Char('J') => self.move_down(),
            KeyCode::Char('K') => self.move_up(),
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return ExclusionEditorAction::Apply;
            }
            _ => {}
        }
        ExclusionEditorAction::None
    }

    fn add(&mut self) {
        self.draft.push(String::new());
        self.selected_idx = self.draft.len() - 1;
        self.input.clear();
        self.editing = true;
    }

    fn begin_edit(&mut self) {
        if let Some(pattern) = self.draft.get(self.selected_idx) {
            self.input.set(pattern.clone());
            self.editing = true;
        }
    }

    fn finish_edit(&mut self) {
        if let Some(entry) = self.draft.get_mut(self.selected_idx) {
            *entry = self.input.to_string();
        }
        self.editing = false;
    }

    fn delete(&mut self) {
        if self.draft.is_empty() {
            return;
        }
        self.draft.remove(self.selected_idx);
        self.selected_idx = self.selected_idx.min(self.draft.len().saturating_sub(1));
    }

    fn restore_defaults(&mut self) {
        self.draft = crate::settings::AppSettings::default().global_exclusions;
        self.selected_idx = 0;
        self.editing = false;
        self.input.clear();
    }

    fn select_next(&mut self) {
        if !self.draft.is_empty() {
            self.selected_idx = (self.selected_idx + 1) % self.draft.len();
        }
    }

    fn select_prev(&mut self) {
        if !self.draft.is_empty() {
            self.selected_idx = self
                .selected_idx
                .checked_sub(1)
                .unwrap_or(self.draft.len() - 1);
        }
    }

    fn move_down(&mut self) {
        if self.selected_idx + 1 < self.draft.len() {
            self.draft.swap(self.selected_idx, self.selected_idx + 1);
            self.selected_idx += 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected_idx > 0 {
            self.draft.swap(self.selected_idx, self.selected_idx - 1);
            self.selected_idx -= 1;
        }
    }
}

impl ConfigRowKind {
    pub fn is_selectable(self) -> bool {
        !matches!(
            self,
            ConfigRowKind::Header(_)
                | ConfigRowKind::IgnoreSources
                | ConfigRowKind::KeyBindings
                | ConfigRowKind::DiffToolUnknown
                | ConfigRowKind::DiffTool {
                    available: false,
                    ..
                }
        )
    }
}

/// What the Config screen lists its rows from: the external diff tools found
/// on this machine and the Settings the session runs with.
#[derive(Clone, Copy, Debug)]
pub struct ConfigContext<'a> {
    pub detected_diff_tools: &'a [(ExternalDiffTool, bool)],
    pub settings: &'a SettingsState,
}

/// A Gesture the Config screen answers, from a key or the mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigGesture {
    MoveDown,
    MoveUp,
    /// Switch, choose, or open the selected row.
    Activate,
    /// Nudge the selected numeric value down (`h`).
    Decrease,
    /// Nudge the selected numeric value up (`l`).
    Increase,
    /// The mouse wheel's one axis: it adjusts the Diff context row while
    /// that row is selected — down decreases it — and moves the selection
    /// otherwise.
    WheelDown,
    WheelUp,
    /// A click on row `N`: select it and activate it, when it can be chosen.
    Click(usize),
}

/// What a [`ConfigGesture`] asks of the session. [`App`](super::App) carries
/// it out.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigIntent {
    None,
    Change(SettingChange),
    SwitchScanMode(ScanMode),
    ToggleTheme,
    OpenExclusionEditor,
    /// Save these global exclusions, once they validate against both roots,
    /// and close the editor.
    ApplyExclusions(Vec<String>),
}

/// The Config screen's own state: the selected row, the list's scroll, the
/// view to restore on close, and the exclusion editor while it is open.
/// Owned by [`App::config`](super::App::config)/
/// [`App::config_mut`](super::App::config_mut); `App` drives it through
/// [`App::config_gesture`](super::App::config_gesture).
#[derive(Clone, Debug, Default)]
pub struct ConfigState {
    selected_idx: usize,
    /// How many of the list's painted lines are scrolled off the top.
    scroll: usize,
    exclusion_editor: Option<ExclusionEditorState>,
}

impl ConfigState {
    /// The Config screen's rows, headers and fields, for `context`.
    pub(crate) fn rows(context: ConfigContext<'_>) -> Vec<ConfigRowKind> {
        let mut rows = vec![ConfigRowKind::Header("External Diff Tool")];
        rows.push(ConfigRowKind::DiffToolAuto);
        rows.push(ConfigRowKind::DiffToolDisabled);
        rows.extend(
            context
                .detected_diff_tools
                .iter()
                .enumerate()
                .map(|(i, (_, avail))| ConfigRowKind::DiffTool {
                    idx: i,
                    available: *avail,
                }),
        );
        if context
            .settings
            .saved()
            .external_diff_tool
            .unknown_name()
            .is_some()
        {
            rows.push(ConfigRowKind::DiffToolUnknown);
        }
        rows.push(ConfigRowKind::Header("Updates"));
        rows.push(ConfigRowKind::CheckUpdates);
        rows.push(ConfigRowKind::Header("Mouse"));
        rows.push(ConfigRowKind::Mouse);
        rows.push(ConfigRowKind::Header("Theme"));
        rows.push(ConfigRowKind::Theme);
        rows.push(ConfigRowKind::Header("Diff View"));
        rows.push(ConfigRowKind::DiffContext);
        rows.push(ConfigRowKind::Header("Scan"));
        rows.push(ConfigRowKind::ScanMode);
        rows.push(ConfigRowKind::Header("Exclusions"));
        rows.push(ConfigRowKind::RespectGitignore);
        rows.push(ConfigRowKind::GlobalExclusions);
        rows.push(ConfigRowKind::IgnoreSources);
        rows.push(ConfigRowKind::Header("Key Bindings"));
        rows.push(ConfigRowKind::KeyBindings);
        rows
    }

    /// The currently selected config row index. Read access for rendering / tests.
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }

    /// Scroll as little as keeps `reveal` — the selected row's lines and the
    /// header above it — within `height` of the list's `total` lines. When
    /// `reveal` is taller than `height`, its end wins: the selected row.
    pub(crate) fn set_frame(
        &mut self,
        reveal: std::ops::Range<usize>,
        height: usize,
        total: usize,
    ) {
        if reveal.start < self.scroll {
            self.scroll = reveal.start;
        }
        if reveal.end > self.scroll + height {
            self.scroll = reveal.end - height;
        }
        self.scroll = self.scroll.min(total.saturating_sub(height));
    }

    /// Point the selection at a row `context` lists as selectable, falling
    /// back to the first one (or 0 if there is none).
    pub(crate) fn ensure_selection(&mut self, context: ConfigContext<'_>) {
        let rows = Self::rows(context);
        if rows.is_empty() {
            self.selected_idx = 0;
            return;
        }
        if self.selected_idx >= rows.len() || !rows[self.selected_idx].is_selectable() {
            self.selected_idx = rows.iter().position(|r| r.is_selectable()).unwrap_or(0);
        }
    }

    /// Answer `gesture` over the rows `context` lists: move the selection,
    /// or say what the selected row asks for.
    pub(crate) fn handle(
        &mut self,
        gesture: ConfigGesture,
        context: ConfigContext<'_>,
    ) -> ConfigIntent {
        let rows = Self::rows(context);
        let on_diff_context = matches!(
            rows.get(self.selected_idx),
            Some(ConfigRowKind::DiffContext)
        );
        match gesture {
            ConfigGesture::MoveDown => self.select_next(&rows),
            ConfigGesture::MoveUp => self.select_prev(&rows),
            ConfigGesture::Activate => return self.activate(&rows, context),
            ConfigGesture::Decrease => return self.adjust(&rows, context, false),
            ConfigGesture::Increase => return self.adjust(&rows, context, true),
            ConfigGesture::WheelDown if on_diff_context => {
                return self.adjust(&rows, context, false)
            }
            ConfigGesture::WheelUp if on_diff_context => return self.adjust(&rows, context, true),
            ConfigGesture::WheelDown => self.select_next(&rows),
            ConfigGesture::WheelUp => self.select_prev(&rows),
            ConfigGesture::Click(idx) => {
                if self.select_at(idx, &rows) {
                    return self.activate(&rows, context);
                }
            }
        }
        ConfigIntent::None
    }

    /// Wrap-around next selectable row in `rows`.
    fn select_next(&mut self, rows: &[ConfigRowKind]) {
        if rows.is_empty() {
            return;
        }
        let mut next = self.selected_idx;
        for _ in 0..rows.len() {
            next = (next + 1) % rows.len();
            if rows[next].is_selectable() {
                self.selected_idx = next;
                return;
            }
        }
    }

    /// Wrap-around previous selectable row in `rows`.
    fn select_prev(&mut self, rows: &[ConfigRowKind]) {
        if rows.is_empty() {
            return;
        }
        let mut prev = self.selected_idx;
        for _ in 0..rows.len() {
            prev = prev.checked_sub(1).unwrap_or(rows.len() - 1);
            if rows[prev].is_selectable() {
                self.selected_idx = prev;
                return;
            }
        }
    }

    /// Select row `idx` in `rows` if it exists and `is_selectable()`; otherwise
    /// no-op. Returns whether the selection was accepted.
    fn select_at(&mut self, idx: usize, rows: &[ConfigRowKind]) -> bool {
        if idx < rows.len() && rows[idx].is_selectable() {
            self.selected_idx = idx;
            true
        } else {
            false
        }
    }

    /// What the selected row asks for when chosen: switch it, choose it, or
    /// open its editor.
    fn activate(&self, rows: &[ConfigRowKind], context: ConfigContext<'_>) -> ConfigIntent {
        let settings = context.settings;
        let change = match rows.get(self.selected_idx) {
            Some(ConfigRowKind::ScanMode) => {
                return ConfigIntent::SwitchScanMode(settings.scan_mode().toggled());
            }
            Some(ConfigRowKind::RespectGitignore) => {
                SettingChange::RespectGitignore(!settings.respect_gitignore())
            }
            Some(ConfigRowKind::GlobalExclusions) => return ConfigIntent::OpenExclusionEditor,
            Some(ConfigRowKind::DiffToolAuto) => {
                SettingChange::ExternalDiffTool(DiffToolSetting::Auto)
            }
            Some(ConfigRowKind::DiffToolDisabled) => {
                SettingChange::ExternalDiffTool(DiffToolSetting::Disabled)
            }
            Some(ConfigRowKind::DiffTool {
                idx,
                available: true,
            }) => match context.detected_diff_tools.get(*idx) {
                Some((tool, _)) => SettingChange::ExternalDiffTool(DiffToolSetting::Pinned(*tool)),
                None => return ConfigIntent::None,
            },
            Some(ConfigRowKind::CheckUpdates) => {
                SettingChange::CheckUpdates(!settings.saved().check_updates)
            }
            Some(ConfigRowKind::Mouse) => SettingChange::Mouse(!settings.mouse()),
            Some(ConfigRowKind::Theme) => return ConfigIntent::ToggleTheme,
            _ => return ConfigIntent::None,
        };
        ConfigIntent::Change(change)
    }

    /// Nudge the selected numeric field (only [`ConfigRowKind::DiffContext`])
    /// up or down by one. Nothing for any other row.
    fn adjust(
        &self,
        rows: &[ConfigRowKind],
        context: ConfigContext<'_>,
        forward: bool,
    ) -> ConfigIntent {
        if let Some(ConfigRowKind::DiffContext) = rows.get(self.selected_idx) {
            let lines = context.settings.saved().diff_context;
            let lines = if forward {
                lines.saturating_add(1)
            } else {
                lines.saturating_sub(1)
            };
            ConfigIntent::Change(SettingChange::DiffContext(lines))
        } else {
            ConfigIntent::None
        }
    }

    /// Open the global exclusion editor on a draft of `rules`, the saved ones.
    pub(crate) fn open_exclusion_editor(&mut self, rules: Vec<String>) {
        self.exclusion_editor = Some(ExclusionEditorState::new(rules));
    }

    /// The global exclusion editor, while it is open over the Config screen.
    pub(crate) fn exclusion_editor(&self) -> Option<&ExclusionEditorState> {
        self.exclusion_editor.as_ref()
    }

    /// Edit the open exclusion draft with `key`. `Esc` closes the editor and
    /// throws the draft away; `Ctrl+s` asks for the draft to be applied.
    pub(crate) fn exclusion_editor_key(&mut self, key: crossterm::event::KeyEvent) -> ConfigIntent {
        let Some(editor) = self.exclusion_editor.as_mut() else {
            return ConfigIntent::None;
        };
        match editor.handle_key(key) {
            ExclusionEditorAction::None => ConfigIntent::None,
            ExclusionEditorAction::Cancel => {
                self.exclusion_editor = None;
                ConfigIntent::None
            }
            ExclusionEditorAction::Apply => ConfigIntent::ApplyExclusions(editor.draft.clone()),
        }
    }

    /// Keep the open editor on the draft, highlighting rule `index`, which
    /// failed to validate.
    pub(crate) fn reject_exclusion(&mut self, index: usize) {
        if let Some(editor) = self.exclusion_editor.as_mut() {
            editor.select(index);
        }
    }

    /// Close the exclusion editor once its draft is saved.
    pub(crate) fn close_exclusion_editor(&mut self) {
        self.exclusion_editor = None;
    }

    /// Keep the highlighted exclusion in a `visible_rows`-tall list viewport.
    pub(crate) fn sync_exclusion_editor_viewport(&mut self, visible_rows: usize) {
        if let Some(editor) = self.exclusion_editor.as_mut() {
            editor.sync_viewport(visible_rows);
        }
    }

    // Test-only field setter, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags this as unreachable
    // outside `#[cfg(test)]` call sites, so it needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AppSettings, SettingsStore};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn settings() -> SettingsState {
        SettingsState::new(
            AppSettings::default(),
            SettingsStore::memory(),
            &crate::startup::CliOverrides::default(),
        )
    }

    /// Vim found, Code not.
    const TOOLS: [(ExternalDiffTool, bool); 2] = [
        (ExternalDiffTool::Vim, true),
        (ExternalDiffTool::Code, false),
    ];

    fn context(settings: &SettingsState) -> ConfigContext<'_> {
        ConfigContext {
            detected_diff_tools: &TOOLS,
            settings,
        }
    }

    fn index_of(context: ConfigContext<'_>, kind: ConfigRowKind) -> usize {
        ConfigState::rows(context)
            .iter()
            .position(|row| *row == kind)
            .unwrap()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn rows_list_every_field_and_navigation_skips_what_cannot_be_chosen() {
        let settings = settings();
        let context = context(&settings);

        let rows = ConfigState::rows(context);
        // Header + Auto + Disabled + 2 tools + Updates header + CheckUpdates + Mouse header + Mouse
        // + Theme header + Theme + Diff View header + DiffContext
        // + Scan header + ScanMode + Exclusions header + two controls + provenance
        // + Key Bindings header + the read-only count
        assert_eq!(rows.len(), 21);
        assert!(matches!(
            rows[0],
            ConfigRowKind::Header("External Diff Tool")
        ));
        assert!(matches!(rows[1], ConfigRowKind::DiffToolAuto));
        assert!(matches!(rows[2], ConfigRowKind::DiffToolDisabled));
        assert!(matches!(
            rows[3],
            ConfigRowKind::DiffTool {
                idx: 0,
                available: true
            }
        ));
        assert!(matches!(
            rows[4],
            ConfigRowKind::DiffTool {
                idx: 1,
                available: false
            }
        ));
        assert!(matches!(rows[5], ConfigRowKind::Header("Updates")));
        assert!(matches!(rows[6], ConfigRowKind::CheckUpdates));
        assert!(matches!(rows[7], ConfigRowKind::Header("Mouse")));
        assert!(matches!(rows[8], ConfigRowKind::Mouse));
        assert!(matches!(rows[9], ConfigRowKind::Header("Theme")));
        assert!(matches!(rows[10], ConfigRowKind::Theme));
        assert!(matches!(rows[11], ConfigRowKind::Header("Diff View")));
        assert!(matches!(rows[12], ConfigRowKind::DiffContext));
        assert!(matches!(rows[13], ConfigRowKind::Header("Scan")));
        assert!(matches!(rows[14], ConfigRowKind::ScanMode));
        assert!(matches!(rows[15], ConfigRowKind::Header("Exclusions")));
        assert!(matches!(rows[16], ConfigRowKind::RespectGitignore));
        assert!(matches!(rows[17], ConfigRowKind::GlobalExclusions));
        assert!(matches!(rows[18], ConfigRowKind::IgnoreSources));
        assert!(matches!(rows[19], ConfigRowKind::Header("Key Bindings")));
        assert!(matches!(rows[20], ConfigRowKind::KeyBindings));

        let mut config = ConfigState::default();
        config.ensure_selection(context);
        assert_eq!(config.selected_idx(), 1);

        // Selectable indices: 1 (Auto), 2 (Disabled), 3 (Vim available),
        // 6 (CheckUpdates), 8 (Mouse), 10 (Theme), 12 (DiffContext),
        // 14 (ScanMode), 16 (RespectGitignore), 17 (GlobalExclusions)
        // (4 is Code unavailable -> skipped!)
        for expected in [2, 3, 6, 8, 10, 12, 14, 16, 17, 1] {
            assert_eq!(
                config.handle(ConfigGesture::MoveDown, context),
                ConfigIntent::None
            );
            assert_eq!(config.selected_idx(), expected);
        }

        config.handle(ConfigGesture::MoveUp, context);
        assert_eq!(config.selected_idx(), 17);

        // A click on the unavailable tool is refused, and asks for nothing.
        assert_eq!(
            config.handle(ConfigGesture::Click(4), context),
            ConfigIntent::None
        );
        assert_eq!(config.selected_idx(), 17); // unchanged
    }

    #[test]
    fn adjusting_a_row_that_is_not_a_number_asks_for_nothing() {
        let settings = settings();
        let context = context(&settings);
        let mut config = ConfigState::default();
        config.set_selected_idx(index_of(context, ConfigRowKind::CheckUpdates));

        assert_eq!(
            config.handle(ConfigGesture::Increase, context),
            ConfigIntent::None
        );
        assert_eq!(
            config.handle(ConfigGesture::Decrease, context),
            ConfigIntent::None
        );
    }

    /// The wheel has one axis: on the Diff context row it adjusts the value
    /// (down decreases it) and keeps the row selected; elsewhere it moves.
    #[test]
    fn the_wheel_adjusts_diff_context_and_moves_the_selection_elsewhere() {
        let settings = settings();
        let context = context(&settings);
        let lines = settings.saved().diff_context;
        let diff_context = index_of(context, ConfigRowKind::DiffContext);
        let mut config = ConfigState::default();
        config.set_selected_idx(diff_context);

        assert_eq!(
            config.handle(ConfigGesture::WheelUp, context),
            ConfigIntent::Change(SettingChange::DiffContext(lines + 1))
        );
        assert_eq!(
            config.handle(ConfigGesture::WheelDown, context),
            ConfigIntent::Change(SettingChange::DiffContext(lines - 1))
        );
        assert_eq!(config.selected_idx(), diff_context);

        let theme = index_of(context, ConfigRowKind::Theme);
        let scan_mode = index_of(context, ConfigRowKind::ScanMode);
        config.set_selected_idx(theme);
        assert_eq!(
            config.handle(ConfigGesture::WheelDown, context),
            ConfigIntent::None
        );
        assert_eq!(config.selected_idx(), diff_context);
        config.set_selected_idx(scan_mode);
        config.handle(ConfigGesture::WheelUp, context);
        assert_eq!(config.selected_idx(), diff_context);
    }

    #[test]
    fn exclusion_editor_cancel_discards_all_draft_changes() {
        let mut config = ConfigState::default();
        config.open_exclusion_editor(vec!["target".to_string()]);
        for code in [
            KeyCode::Char('a'),
            KeyCode::Char('x'),
            KeyCode::Enter,
            KeyCode::Esc,
        ] {
            assert_eq!(config.exclusion_editor_key(key(code)), ConfigIntent::None);
        }

        assert!(config.exclusion_editor().is_none());
    }

    #[test]
    fn exclusion_editor_r_restores_builtin_defaults_into_the_draft_without_saving() {
        let saved = vec!["target".to_string(), "*.log".to_string()];
        let mut config = ConfigState::default();
        config.open_exclusion_editor(saved.clone());
        for _ in 0..saved.len() {
            config.exclusion_editor_key(key(KeyCode::Char('d')));
        }
        assert!(
            config
                .exclusion_editor()
                .expect("editor open")
                .draft()
                .is_empty(),
            "precondition: every rule was deleted from the draft"
        );

        assert_eq!(
            config.exclusion_editor_key(key(KeyCode::Char('r'))),
            ConfigIntent::None,
            "r must not ask to save until Ctrl+s"
        );
        assert_eq!(
            config.exclusion_editor().expect("editor open").draft(),
            AppSettings::default().global_exclusions
        );
    }
}
