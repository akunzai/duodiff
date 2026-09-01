//! Keyboard and mouse input routing for the TUI event loop.
use crate::actions::kick_scan;
use crate::app::{self, App};
use crate::event::AppEvent;
use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::Terminal;

fn present_command_outcome(app: &mut App, outcome: crate::commands::Outcome) {
    match outcome {
        crate::commands::Outcome::Message { text } => app.set_status(text, false),
        // Unavailability is informational: the Command was refused before any
        // effect ran, so it is not styled as an error (Issue #282).
        crate::commands::Outcome::Unavailable { message } => app.set_status(message, false),
        crate::commands::Outcome::Failed { message } => app.set_status(message, true),
        // The prompt reaches the screen here, so every adapter raises a
        // confirmation the same way (Issue #284).
        crate::commands::Outcome::NeedsConfirmation { prompt } => app.show_confirm(prompt),
        crate::commands::Outcome::Completed | crate::commands::Outcome::ExitRequested => {}
    }
}

/// Run one Command and show its outcome on the screen the gesture came from.
fn run_command<B: ratatui::backend::Backend>(
    command: crate::commands::Command,
    app: &mut App,
    terminal: &mut Terminal<B>,
    commands: &mut crate::commands::Commands,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    let mut handoff = crate::commands::RatatuiTerminalHandoff(terminal);
    let outcome = commands.execute(
        app,
        crate::commands::Invocation::Command(command),
        &mut handoff,
    )?;
    present_command_outcome(app, outcome);
    Ok(())
}

/// Run a top bar link's Command, but only where the active screen offers it.
///
/// The links are chrome drawn on every screen, so clicking the one naming the
/// screen you are already on stays the no-op it has always been rather than
/// reporting that the Command does not apply here.
fn run_top_bar_link<B: ratatui::backend::Backend>(
    command: crate::commands::Command,
    app: &mut App,
    terminal: &mut Terminal<B>,
    commands: &mut crate::commands::Commands,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    if commands
        .inventory(app)
        .iter()
        .any(|entry| entry.command == command)
    {
        run_command(command, app, terminal, commands)?;
    }
    Ok(())
}

/// Run the Command a Command Palette row names.
///
/// The popup closes on anything but a refusal: a Command that could not run
/// leaves the inventory open so the user can pick another one instead of
/// reopening the palette (Issue #239).
fn run_palette_command<B: ratatui::backend::Backend>(
    command: crate::commands::Command,
    app: &mut App,
    terminal: &mut Terminal<B>,
    commands: &mut crate::commands::Commands,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    let mut handoff = crate::commands::RatatuiTerminalHandoff(terminal);
    let outcome = commands.execute(
        app,
        crate::commands::Invocation::Command(command),
        &mut handoff,
    )?;
    let unavailable = matches!(&outcome, crate::commands::Outcome::Unavailable { .. });
    present_command_outcome(app, outcome);
    if !unavailable {
        app.palette_mut().close();
    }
    Ok(())
}

/// One keyboard chord bound to a [`Command`].
///
/// `hint` is the label the Command Palette shows for the chord. `None` marks an
/// alias that routes but stays out of the hint, so a Command keeps naming one
/// key even when several reach it.
struct Chord {
    code: KeyCode,
    modifiers: crossterm::event::KeyModifiers,
    hint: Option<&'static str>,
}

impl Chord {
    const fn key(code: KeyCode, hint: &'static str) -> Self {
        Self {
            code,
            modifiers: crossterm::event::KeyModifiers::NONE,
            hint: Some(hint),
        }
    }

    const fn alias(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: crossterm::event::KeyModifiers::NONE,
            hint: None,
        }
    }

    const fn alt(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: crossterm::event::KeyModifiers::ALT,
            hint: None,
        }
    }
}

/// A Command and every chord that reaches it on one screen.
struct Binding {
    command: crate::commands::Command,
    chords: &'static [Chord],
}

/// Modifiers that distinguish one chord from another. Shift is excluded because
/// terminals report it inconsistently for the uppercase bindings (`D`, `L`, `N`).
const SIGNIFICANT_MODIFIERS: crossterm::event::KeyModifiers =
    crossterm::event::KeyModifiers::CONTROL.union(crossterm::event::KeyModifiers::ALT);

/// The keyboard adapter owns the only binding table (ADR-0003): it routes key
/// presses here and the Command Palette reuses its hints through
/// [`key_hint`]. Screens that share a key give it different Commands, so the
/// table is per screen; [`GLOBAL_BINDINGS`] holds the chords every screen answers.
const GLOBAL_BINDINGS: &[Binding] = &[Binding {
    command: crate::commands::Command::ToggleTheme,
    chords: &[Chord::key(KeyCode::Char('T'), "T")],
}];

const DIRECTORY_TREE_BINDINGS: &[Binding] = &[
    Binding {
        command: crate::commands::Command::BuiltinDiff,
        chords: &[Chord::key(KeyCode::Enter, "Enter")],
    },
    Binding {
        command: crate::commands::Command::ExternalDiff,
        chords: &[Chord::key(KeyCode::Char('D'), "D")],
    },
    Binding {
        command: crate::commands::Command::ExternalEdit,
        chords: &[Chord::key(KeyCode::Char('E'), "E")],
    },
    Binding {
        command: crate::commands::Command::CopyLeftToRight,
        chords: &[Chord::key(KeyCode::Char('R'), "R")],
    },
    Binding {
        command: crate::commands::Command::CopyRightToLeft,
        chords: &[Chord::key(KeyCode::Char('L'), "L")],
    },
    Binding {
        command: crate::commands::Command::Expand,
        chords: &[
            Chord::key(KeyCode::Char('l'), "l"),
            Chord::key(KeyCode::Right, "Right"),
        ],
    },
    Binding {
        command: crate::commands::Command::Collapse,
        chords: &[
            Chord::key(KeyCode::Char('h'), "h"),
            Chord::key(KeyCode::Left, "Left"),
        ],
    },
    Binding {
        command: crate::commands::Command::ToggleFocus,
        chords: &[Chord::key(KeyCode::Tab, "Tab")],
    },
    Binding {
        command: crate::commands::Command::FocusLeft,
        chords: &[Chord::key(KeyCode::Char('1'), "1")],
    },
    Binding {
        command: crate::commands::Command::FocusRight,
        chords: &[Chord::key(KeyCode::Char('2'), "2")],
    },
    Binding {
        command: crate::commands::Command::Filter,
        chords: &[Chord::key(KeyCode::Char('/'), "/")],
    },
    Binding {
        command: crate::commands::Command::SwapPaths,
        chords: &[Chord::key(KeyCode::Char('s'), "s")],
    },
    Binding {
        command: crate::commands::Command::ToggleScan,
        chords: &[Chord::key(KeyCode::Char('c'), "c")],
    },
    Binding {
        command: crate::commands::Command::Refresh,
        chords: &[Chord::key(KeyCode::Char('r'), "r")],
    },
    Binding {
        command: crate::commands::Command::Config,
        chords: &[Chord::key(KeyCode::Char('C'), "C")],
    },
    Binding {
        command: crate::commands::Command::Help,
        chords: &[Chord::key(KeyCode::Char('?'), "?")],
    },
    Binding {
        command: crate::commands::Command::Quit,
        chords: &[
            Chord::key(KeyCode::Char('q'), "q"),
            Chord::alias(KeyCode::Esc),
        ],
    },
];

const FILE_DIFF_BINDINGS: &[Binding] = &[
    Binding {
        command: crate::commands::Command::NextChange,
        chords: &[
            Chord::key(KeyCode::Char('N'), "N"),
            Chord::alt(KeyCode::Down),
        ],
    },
    Binding {
        command: crate::commands::Command::PrevChange,
        chords: &[Chord::key(KeyCode::Char('P'), "P"), Chord::alt(KeyCode::Up)],
    },
    Binding {
        command: crate::commands::Command::StageLeftToRight,
        chords: &[Chord::key(KeyCode::Char(']'), "]")],
    },
    Binding {
        command: crate::commands::Command::StageRightToLeft,
        chords: &[Chord::key(KeyCode::Char('['), "[")],
    },
    // Whole-file overwrite stays on the uppercase keys only. Lowercase `l`/`r`
    // are harmless in the Directory Tree (expand / re-scan), so binding them to
    // a destructive overwrite here turned tree muscle memory into data loss
    // behind a single `y` (Issue #234).
    Binding {
        command: crate::commands::Command::CopyLeftToRight,
        chords: &[Chord::key(KeyCode::Char('R'), "R")],
    },
    Binding {
        command: crate::commands::Command::CopyRightToLeft,
        chords: &[Chord::key(KeyCode::Char('L'), "L")],
    },
    // The palette lists both for File Diff, so they need matching direct
    // bindings here as well as in the Directory Tree (Issue #239).
    Binding {
        command: crate::commands::Command::ExternalDiff,
        chords: &[Chord::key(KeyCode::Char('D'), "D")],
    },
    Binding {
        command: crate::commands::Command::ExternalEdit,
        chords: &[Chord::key(KeyCode::Char('E'), "E")],
    },
    Binding {
        command: crate::commands::Command::SaveStaged,
        chords: &[Chord::key(KeyCode::Char('s'), "s")],
    },
    Binding {
        command: crate::commands::Command::UndoStaged,
        chords: &[Chord::key(KeyCode::Char('u'), "u")],
    },
    Binding {
        command: crate::commands::Command::ToggleWrap,
        chords: &[Chord::key(KeyCode::Char('w'), "w")],
    },
    Binding {
        command: crate::commands::Command::ToggleFullDiff,
        chords: &[Chord::key(KeyCode::Char('f'), "f")],
    },
    Binding {
        command: crate::commands::Command::Config,
        chords: &[Chord::key(KeyCode::Char('C'), "C")],
    },
    Binding {
        command: crate::commands::Command::Help,
        chords: &[Chord::key(KeyCode::Char('?'), "?")],
    },
    // Never walk out on unwritten work: the dirty gate opens a
    // Save / Discard / Cancel dialog instead (Issue #235).
    Binding {
        command: crate::commands::Command::Back,
        chords: &[
            Chord::key(KeyCode::Esc, "Esc"),
            Chord::alias(KeyCode::Char('q')),
        ],
    },
];

const CONFIG_MENU_BINDINGS: &[Binding] = &[
    Binding {
        command: crate::commands::Command::Help,
        chords: &[Chord::key(KeyCode::Char('?'), "?")],
    },
    Binding {
        command: crate::commands::Command::Back,
        chords: &[
            Chord::key(KeyCode::Esc, "Esc"),
            Chord::alias(KeyCode::Char('q')),
        ],
    },
];

const HELP_BINDINGS: &[Binding] = &[
    Binding {
        command: crate::commands::Command::Config,
        chords: &[Chord::key(KeyCode::Char('C'), "C")],
    },
    Binding {
        command: crate::commands::Command::Back,
        chords: &[
            Chord::key(KeyCode::Esc, "Esc"),
            Chord::alias(KeyCode::Char('q')),
            Chord::alias(KeyCode::Char('?')),
        ],
    },
];

fn screen_bindings(view_mode: app::ViewMode) -> &'static [Binding] {
    match view_mode {
        app::ViewMode::DirectoryTree => DIRECTORY_TREE_BINDINGS,
        app::ViewMode::FileDiff => FILE_DIFF_BINDINGS,
        app::ViewMode::ConfigMenu => CONFIG_MENU_BINDINGS,
        app::ViewMode::Help => HELP_BINDINGS,
    }
}

fn chord_matches(chord: &Chord, key: &KeyEvent) -> bool {
    key.code == chord.code && key.modifiers & SIGNIFICANT_MODIFIERS == chord.modifiers
}

fn command_in(table: &'static [Binding], key: &KeyEvent) -> Option<crate::commands::Command> {
    table
        .iter()
        .find(|binding| binding.chords.iter().any(|chord| chord_matches(chord, key)))
        .map(|binding| binding.command)
}

/// The Command a key press resolves to on `view_mode`, if any.
fn command_for_key(view_mode: app::ViewMode, key: &KeyEvent) -> Option<crate::commands::Command> {
    command_in(screen_bindings(view_mode), key)
}

/// The display hint for `command`, derived from the binding table so the
/// Command Palette never restates a key the keyboard adapter owns. Commands
/// with no chord — the Help repository link — have no hint.
pub(crate) fn key_hint(command: crate::commands::Command) -> String {
    [
        GLOBAL_BINDINGS,
        DIRECTORY_TREE_BINDINGS,
        FILE_DIFF_BINDINGS,
        CONFIG_MENU_BINDINGS,
        HELP_BINDINGS,
    ]
    .into_iter()
    .flatten()
    .find(|binding| binding.command == command)
    .map(|binding| {
        binding
            .chords
            .iter()
            .filter_map(|chord| chord.hint)
            .collect::<Vec<_>>()
            .join(" / ")
    })
    .unwrap_or_default()
}

/// Handle a key press. Returns `Ok(true)` if the event loop should quit.
#[cfg(test)]
pub async fn handle_key<B: ratatui::backend::Backend>(
    key: KeyEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<bool, Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    let mut commands = crate::commands::Commands::new(tx.clone());
    handle_key_with_commands(key, app, terminal, tx, &mut commands).await
}

pub async fn handle_key_with_commands<B: ratatui::backend::Backend>(
    key: KeyEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
    commands: &mut crate::commands::Commands,
) -> Result<bool, Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    // Confirm modal traps all input until dismissed — checked before every other
    // shortcut (including the command palette and theme toggle below) so it behaves
    // identically regardless of which ViewMode it was opened from. Mirrors
    // handle_mouse, which checks `confirm_modal` first for the same reason.
    if let Some(modal) = app.confirm_modal() {
        // Enter takes the first, most affirmative choice; Esc takes Cancel;
        // a typed letter takes the matching choice (Issue #235).
        let chosen = match key.code {
            KeyCode::Enter => modal.default_action(),
            KeyCode::Esc => modal.cancel_action(),
            KeyCode::Char(c) => modal.action_for_key(c),
            _ => None,
        };
        if let Some(action) = chosen {
            let mut handoff = crate::commands::RatatuiTerminalHandoff(terminal);
            let outcome = commands.execute(
                app,
                crate::commands::Invocation::Confirmation(action),
                &mut handoff,
            )?;
            present_command_outcome(app, outcome);
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
            app.palette_mut().close();
            return Ok(false);
        }
        match key.code {
            KeyCode::Esc => {
                app.palette_mut().close();
            }
            KeyCode::Down => {
                app.palette_mut().select_next();
            }
            KeyCode::Up => {
                app.palette_mut().select_prev();
            }
            KeyCode::Enter => {
                let selected = app.palette().selected_idx();
                if let Some(entry) = app.palette().items().get(selected).cloned() {
                    run_palette_command(entry.command, app, terminal, commands)?;
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

    // The exclusion editor is a modal editing session: it captures every key
    // before Config/global shortcuts can act on the underlying screen.
    if app.exclusion_editor_open() {
        if app.exclusion_editor_key(key) {
            kick_scan(app, tx.clone());
        }
        return Ok(false);
    }

    // Global bindings (the theme toggle) reach every screen, except while typing
    // into the filter bar so `T` can still be typed as a filter character.
    if !app.tree_list().active() {
        if let Some(command) = command_in(GLOBAL_BINDINGS, &key) {
            run_command(command, app, terminal, commands)?;
            return Ok(false);
        }
    }

    // Both palette launchers yield to the filter bar, which keeps complete input
    // capture while it is open: `;` must be typeable (Issue #236) and no launcher
    // may interrupt a text editor (Issue #239).
    if !app.tree_list().active() {
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
            if app.tree_list().active() {
                match key.code {
                    KeyCode::Esc => {
                        app.tree_list_mut().cancel();
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
                        app.tree_list_mut().toggle_diffs_only();
                    }
                    _ => {
                        app.tree_list_mut().input_mut().apply_edit(key.code);
                    }
                }
            } else {
                match key.code {
                    // Esc is layered: while a filter is applied it is the natural
                    // "cancel / clear" gesture, so it must clear the filter rather
                    // than fall through to the least reversible action available.
                    // Only with nothing left to dismiss does it quit (Issue #233).
                    KeyCode::Esc
                        if !app.tree_list().pattern().is_empty()
                            || app.tree_list().diffs_only() =>
                    {
                        app.clear_filter();
                    }
                    KeyCode::Char('j') | KeyCode::Down => app.tree_list_mut().select_next(),
                    KeyCode::Char('k') | KeyCode::Up => app.tree_list_mut().select_prev(),
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
                    KeyCode::Backspace
                        if !app.tree_list().pattern().is_empty()
                            || app.tree_list().diffs_only() =>
                    {
                        app.clear_filter();
                    }
                    // Space, and Enter on a directory, are toggle gestures: the
                    // adapter reads the current state and picks the explicit
                    // Expand or Collapse target, because there is no toggle
                    // Command to invoke (ADR-0003).
                    KeyCode::Char(' ') | KeyCode::Enter
                        if app.selected_row().is_some_and(|row| row.is_dir()) =>
                    {
                        let command = if app.selected_row().is_some_and(|row| row.is_expanded) {
                            crate::commands::Command::Collapse
                        } else {
                            crate::commands::Command::Expand
                        };
                        run_command(command, app, terminal, commands)?;
                    }
                    // Space on a file row has nothing to expand and no binding.
                    KeyCode::Char(' ') => {}
                    _ => {
                        if let Some(command) = command_for_key(app::ViewMode::DirectoryTree, &key) {
                            run_command(command, app, terminal, commands)?;
                        }
                    }
                }
            }
        }
        app::ViewMode::FileDiff => match key.code {
            // Alt+Down / Alt+Up are bound to the change-block jumps, so the
            // arrow keys scroll only without that modifier.
            KeyCode::Char('j') => {
                app.diff_scroll_down();
            }
            KeyCode::Down if !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
                app.diff_scroll_down();
            }
            KeyCode::Char('k') => {
                app.diff_mut().scroll_up();
            }
            KeyCode::Up if !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) => {
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
            _ => {
                if let Some(command) = command_for_key(app::ViewMode::FileDiff, &key) {
                    run_command(command, app, terminal, commands)?;
                }
            }
        },
        app::ViewMode::ConfigMenu => match key.code {
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
            _ => {
                if let Some(command) = command_for_key(app::ViewMode::ConfigMenu, &key) {
                    run_command(command, app, terminal, commands)?;
                }
            }
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
            _ => {
                if let Some(command) = command_for_key(app::ViewMode::Help, &key) {
                    run_command(command, app, terminal, commands)?;
                }
            }
        },
    }
    Ok(false)
}

/// Handle a mouse event.
#[cfg(test)]
pub async fn handle_mouse<B: ratatui::backend::Backend>(
    mouse: MouseEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B::Error: 'static,
{
    let mut commands = crate::commands::Commands::new(tx.clone());
    handle_mouse_with_commands(mouse, app, terminal, tx, &mut commands).await
}

pub async fn handle_mouse_with_commands<B: ratatui::backend::Backend>(
    mouse: MouseEvent,
    app: &mut App,
    terminal: &mut Terminal<B>,
    tx: tokio::sync::mpsc::Sender<AppEvent>,
    commands: &mut crate::commands::Commands,
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
                let modal_area = crate::layout::centered_rect(60, 7, size_rect);
                if mouse.row == modal_area.y
                    && mouse.column >= modal_area.x + modal_area.width.saturating_sub(5)
                    && mouse.column < modal_area.x + modal_area.width.saturating_sub(2)
                {
                    let mut handoff = crate::commands::RatatuiTerminalHandoff(terminal);
                    let outcome = commands.execute(
                        app,
                        crate::commands::Invocation::Confirmation(app::ConfirmAction::Cancel),
                        &mut handoff,
                    )?;
                    present_command_outcome(app, outcome);
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
                    app.palette_mut().close();
                    run_top_bar_link(crate::commands::Command::Config, app, terminal, commands)?;
                    return Ok(());
                } else if links.help.x <= mouse.column
                    && mouse.column < links.help.x + links.help.width
                {
                    app.palette_mut().close();
                    run_top_bar_link(crate::commands::Command::Help, app, terminal, commands)?;
                    return Ok(());
                }
            }
        } else if app.palette_visible() {
            if let Ok(size) = terminal.size() {
                let frame = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                // Same geometry the renderer used, so a click cannot land on a
                // row painted somewhere else (Issue #239).
                let layout = crate::layout::palette_layout(app.palette().items().len(), frame);
                let popup = layout.popup;

                let inside = mouse.column >= popup.x
                    && mouse.column < popup.x + popup.width
                    && mouse.row >= popup.y
                    && mouse.row < popup.y + popup.height;
                if !inside {
                    app.palette_mut().close();
                    return Ok(());
                }

                if let Some(button) = crate::layout::close_button_rect(popup) {
                    if mouse.row == button.y
                        && mouse.column >= button.x
                        && mouse.column < button.x + button.width
                    {
                        app.palette_mut().close();
                        return Ok(());
                    }
                }

                if mouse.row >= layout.list.y && mouse.row < layout.list.y + layout.list.height {
                    let clicked =
                        app.palette().scroll_offset() + (mouse.row - layout.list.y) as usize;
                    if let Some(entry) = app.palette().items().get(clicked).cloned() {
                        run_palette_command(entry.command, app, terminal, commands)?;
                    }
                }
            }
            return Ok(());
        } else {
            if let Ok(size) = terminal.size() {
                // Help and Config paint their close button against the body rect
                // their own layout function returns; read the same one here so the
                // two cannot drift apart (#300).
                type ScreenLayoutFn = fn(ratatui::prelude::Rect) -> crate::layout::ScreenLayout;
                let screen_layout: Option<ScreenLayoutFn> = match app.view_mode() {
                    app::ViewMode::Help => Some(crate::layout::help_layout),
                    app::ViewMode::ConfigMenu => Some(crate::layout::config_layout),
                    _ => None,
                };
                if let Some(screen_layout) = screen_layout {
                    let size_rect = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                    let body_area = screen_layout(size_rect).body;
                    if let Some(button) = crate::layout::close_button_rect(body_area) {
                        if mouse.row == button.y
                            && mouse.column >= button.x
                            && mouse.column < button.x + button.width
                        {
                            run_command(crate::commands::Command::Back, app, terminal, commands)?;
                            return Ok(());
                        }
                    }
                } else if app.view_mode() == app::ViewMode::FileDiff {
                    let size_rect = ratatui::prelude::Rect::new(0, 0, size.width, size.height);
                    let inputs = crate::view::diff_layout_inputs(app);
                    let layout = crate::layout::diff_layout(&inputs, size_rect);
                    // `draw_close_button` paints against `layout.right` (see ui.rs), so the
                    // hit test reads the same rect rather than `layout.left` — both share
                    // the same `y` today (a horizontal split), but `right` is what's true by
                    // construction, not by coincidence.
                    if mouse.row == layout.right.y
                        && mouse.column >= size.width.saturating_sub(5)
                        && mouse.column < size.width.saturating_sub(2)
                    {
                        // Same dirty gate as `q` / `Esc` and the palette's Back.
                        run_command(crate::commands::Command::Back, app, terminal, commands)?;
                        return Ok(());
                    }
                }
            }
        }
    }
    if app.palette_visible() {
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.palette_mut().select_next();
                return Ok(());
            }
            MouseEventKind::ScrollUp => {
                app.palette_mut().select_prev();
                return Ok(());
            }
            _ => {}
        }
    }
    match app.view_mode() {
        app::ViewMode::DirectoryTree => match mouse.kind {
            MouseEventKind::ScrollDown => app.tree_list_mut().select_next(),
            MouseEventKind::ScrollUp => app.tree_list_mut().select_prev(),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let click_y = mouse.row as usize;
                if click_y >= 2 {
                    let offset_y = click_y - 2;
                    if offset_y < app.viewport().visible_height {
                        let idx = app.tree_list().scroll_offset() + offset_y;
                        if app.tree_list_mut().select_row_at(idx) && app.note_tree_click(idx) {
                            let row = app.selected_row().unwrap();
                            if row.is_dir() {
                                let command = if row.is_expanded {
                                    crate::commands::Command::Collapse
                                } else {
                                    crate::commands::Command::Expand
                                };
                                run_command(command, app, terminal, commands)?;
                            } else {
                                run_command(
                                    crate::commands::Command::BuiltinDiff,
                                    app,
                                    terminal,
                                    commands,
                                )?;
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
                        let row = app.tree_list().scroll_offset() + offset_y;
                        app.tree_list_mut().select_row_at(row);
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
                            run_command(
                                crate::commands::Command::OpenRepository,
                                app,
                                terminal,
                                commands,
                            )?;
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

    /// The keyboard adapter owns the bindings, so the Palette's key column is
    /// derived from the same table that routes the press (ADR-0003).
    #[test]
    fn key_hint_names_the_display_chords_of_a_binding() {
        use crate::commands::Command;

        assert_eq!(key_hint(Command::Expand), "l / Right");
        assert_eq!(key_hint(Command::Collapse), "h / Left");
        assert_eq!(key_hint(Command::ToggleFocus), "Tab");
        assert_eq!(key_hint(Command::ToggleTheme), "T");
        // Aliases route without widening the hint.
        assert_eq!(key_hint(Command::Quit), "q");
        assert_eq!(key_hint(Command::Back), "Esc");
        assert_eq!(key_hint(Command::NextChange), "N");
        // The Help repository link has no binding, so it advertises no key.
        assert_eq!(key_hint(Command::OpenRepository), "");
    }

    #[test]
    fn every_listed_command_is_bound_on_the_screen_that_lists_it() {
        for view_mode in [
            app::ViewMode::DirectoryTree,
            app::ViewMode::FileDiff,
            app::ViewMode::ConfigMenu,
            app::ViewMode::Help,
        ] {
            let mut app_state = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app_state.set_view_mode(view_mode);
            for entry in crate::commands::inventory_entries(&app_state) {
                let bound = screen_bindings(view_mode)
                    .iter()
                    .chain(GLOBAL_BINDINGS)
                    .any(|binding| binding.command == entry.command);
                assert!(
                    bound,
                    "{:?} is listed on {view_mode:?} without a binding",
                    entry.command
                );
                assert!(
                    !entry.key.is_empty(),
                    "{:?} is listed on {view_mode:?} without a key hint",
                    entry.command
                );
            }
        }
    }

    #[test]
    fn no_screen_binds_one_chord_to_two_commands() {
        for table in [
            GLOBAL_BINDINGS,
            DIRECTORY_TREE_BINDINGS,
            FILE_DIFF_BINDINGS,
            CONFIG_MENU_BINDINGS,
            HELP_BINDINGS,
        ] {
            let mut seen: Vec<(KeyCode, crossterm::event::KeyModifiers)> = Vec::new();
            for binding in table {
                for chord in binding.chords {
                    let chord_key = (chord.code, chord.modifiers);
                    assert!(
                        !seen.contains(&chord_key),
                        "{:?} re-binds {:?}",
                        binding.command,
                        chord.code
                    );
                    seen.push(chord_key);
                }
            }
        }
    }

    #[test]
    fn command_lookup_distinguishes_a_modifier_chord_from_its_bare_key() {
        use crate::commands::Command;
        use crossterm::event::KeyModifiers;

        assert_eq!(
            command_for_key(
                app::ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)
            ),
            Some(Command::NextChange)
        );
        // Plain Down scrolls; it must not resolve to a Command at all.
        assert_eq!(
            command_for_key(
                app::ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Down, KeyModifiers::empty())
            ),
            None
        );
        // Shift is how terminals report the uppercase bindings, so it is ignored.
        assert_eq!(
            command_for_key(
                app::ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)
            ),
            Some(Command::NextChange)
        );
        // Ctrl+f pages the diff; only the bare `f` toggles full-file context.
        assert_eq!(
            command_for_key(
                app::ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    #[test]
    fn the_same_key_resolves_per_screen() {
        use crate::commands::Command;
        use crossterm::event::KeyModifiers;

        let question = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty());
        assert_eq!(
            command_for_key(app::ViewMode::DirectoryTree, &question),
            Some(Command::Help)
        );
        // Inside Help, `?` closes the screen it would otherwise open.
        assert_eq!(
            command_for_key(app::ViewMode::Help, &question),
            Some(Command::Back)
        );

        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert_eq!(
            command_for_key(app::ViewMode::DirectoryTree, &escape),
            Some(Command::Quit)
        );
        assert_eq!(
            command_for_key(app::ViewMode::FileDiff, &escape),
            Some(Command::Back)
        );
    }

    #[tokio::test]
    async fn test_filter_bar_edits_cjk_text_by_char_not_byte() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.tree_list_mut().open();
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
        assert_eq!(app.tree_list().input(), "你好");

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
        assert_eq!(app.tree_list().input(), "你");
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
        app.tree_list_mut().open();
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
        assert_eq!(app.tree_list().input(), "T");
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
                ..Default::default()
            },
            crate::app::FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: crate::diff::DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        app.apply_filter();
        app.tree_list_mut().set_selected_idx(0);
        app.open_palette();
        app.palette_mut().set_items(vec![
            crate::commands::CommandEntry {
                key: "a".to_string(),
                label: "Action A".to_string(),
                command: crate::commands::Command::Help,
                disabled_reason: None,
            },
            crate::commands::CommandEntry {
                key: "b".to_string(),
                label: "Action B".to_string(),
                command: crate::commands::Command::Quit,
                disabled_reason: None,
            },
        ]);
        app.palette_mut().set_selected_idx(0);
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
            app.palette().selected_idx(),
            1,
            "scroll down navigates palette items"
        );
        assert_eq!(
            app.tree_list().selected_idx(),
            0,
            "scroll must not leak through to the background directory tree"
        );

        handle_mouse(scroll_up, &mut app, &mut terminal, tx)
            .await
            .unwrap();
        assert_eq!(
            app.palette().selected_idx(),
            0,
            "scroll up navigates palette items back"
        );
        assert_eq!(
            app.tree_list().selected_idx(),
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
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 40, 24));
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
                            ..Default::default()
                        },
                        crate::app::FlatRow {
                            depth: 0,
                            relative_path: PathBuf::from("b.txt"),
                            name: "b.txt".to_string(),
                            state: crate::diff::DiffState::DifferentNewerLeft,
                            left: None,
                            right: None,
                            ..Default::default()
                        },
                    ]);
                    app.apply_filter();
                    app.tree_list_mut().set_selected_idx(0);
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
                    crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 10));
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
            let before_selected_idx = app.tree_list().selected_idx();
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
                    app.tree_list().selected_idx(), before_selected_idx,
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

    /// The links are chrome on every screen, so the one naming the screen you
    /// are on is a no-op rather than a refusal toast (Issue #282).
    #[tokio::test]
    async fn test_topbar_link_for_the_active_screen_does_nothing() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let links = crate::ui::top_bar_links(ratatui::prelude::Rect::new(0, 0, 80, 1));

        for (view_mode, column) in [
            (app::ViewMode::ConfigMenu, links.config.x),
            (app::ViewMode::Help, links.help.x),
        ] {
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.set_view_mode(view_mode);
            let click = crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column,
                row: 0,
                modifiers: crossterm::event::KeyModifiers::empty(),
            };
            handle_mouse(click, &mut app, &mut terminal, tx.clone())
                .await
                .unwrap();

            assert_eq!(app.view_mode(), view_mode);
            assert_eq!(app.status_toast(), None, "{view_mode:?} link toasted");
        }
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
        // must derive this row from `layout::diff_layout` — the single source of
        // truth shared with `ui::draw_diff_content`/`view::prepare_frame` — instead of an
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
    async fn test_every_file_diff_exit_path_gates_dirty_staged_changes() {
        use crate::diff::FileInfo;
        use crate::diff_view::HunkCopyDirection;
        use ratatui::backend::TestBackend;
        use ratatui::prelude::Rect;
        use ratatui::Terminal;
        use std::fs::write;
        use std::time::SystemTime;
        use tempfile::tempdir;

        let left = tempdir().unwrap();
        let right = tempdir().unwrap();
        write(left.path().join("merge.txt"), "keep\nleft\n").unwrap();
        write(right.path().join("merge.txt"), "keep\nright\n").unwrap();
        let mut app = App::new(left.path().to_path_buf(), right.path().to_path_buf());
        app.set_flat_rows(vec![crate::app::FlatRow {
            depth: 0,
            relative_path: PathBuf::from("merge.txt"),
            name: "merge.txt".to_string(),
            state: crate::diff::DiffState::DifferentNewerLeft,
            left: Some(FileInfo {
                is_dir: false,
                size: 10,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: Some(FileInfo {
                is_dir: false,
                size: 11,
                modified: SystemTime::UNIX_EPOCH,
            }),
            ..Default::default()
        }]);
        app.apply_filter();
        app.set_view_mode(crate::app::ViewMode::FileDiff);
        app.diff_mut().set_show_full(true);
        app.refresh_file_diff().unwrap();
        app.diff_mut().set_scroll(1);
        app.stage_hunk_at_cursor(HunkCopyDirection::LeftToRight)
            .unwrap();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        crate::view::prepare_frame(&mut app, Rect::new(0, 0, 80, 24));
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        for key in [KeyCode::Char('q'), KeyCode::Esc] {
            handle_key(
                KeyEvent::new(key, crossterm::event::KeyModifiers::empty()),
                &mut app,
                &mut terminal,
                tx.clone(),
            )
            .await
            .unwrap();
            assert!(
                app.confirm_modal().is_some(),
                "{key:?} must open the dirty exit gate"
            );
            assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
            app.dismiss_confirm();
        }

        let layout = crate::layout::diff_layout(
            &crate::view::diff_layout_inputs(&app),
            Rect::new(0, 0, 80, 24),
        );
        handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: 76,
                row: layout.right.y,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(
            app.confirm_modal().is_some(),
            "[x] must open the dirty exit gate"
        );
        app.dismiss_confirm();

        // The Palette's own Back entry goes through the same adapter as a key,
        // so the gate has to open there too.
        app.open_palette();
        app.palette_mut()
            .set_items(vec![crate::commands::CommandEntry::new(
                "Return to the Directory Tree",
                crate::commands::Command::Back,
            )]);
        app.palette_mut().set_selected_idx(0);
        handle_key(
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::empty()),
            &mut app,
            &mut terminal,
            tx.clone(),
        )
        .await
        .unwrap();
        assert!(
            app.confirm_modal().is_some(),
            "Palette Back must open the dirty exit gate"
        );
        assert_eq!(app.view_mode(), crate::app::ViewMode::FileDiff);
    }

    /// Column of the `[x]` a screen painted, and the row it landed on, read out
    /// of the rendered buffer rather than assumed.
    fn painted_close_button(
        terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    ) -> (u16, u16) {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        for y in 0..area.height {
            // The button hugs the right edge; anything further left is content.
            for x in area.width.saturating_sub(6)..area.width.saturating_sub(2) {
                if buffer[(x, y)].symbol() == "["
                    && buffer[(x + 1, y)].symbol() == "x"
                    && buffer[(x + 2, y)].symbol() == "]"
                {
                    return (x + 1, y);
                }
            }
        }
        panic!("no close button was painted");
    }

    async fn click_painted_close_button(app: &mut App, width: u16, height: u16) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        crate::view::prepare_frame(app, terminal.size().unwrap().into());
        let screen = crate::view::assemble(app);
        terminal.draw(|f| crate::ui::draw(f, &screen)).unwrap();

        let (column, row) = painted_close_button(&terminal);
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        handle_mouse(click, app, &mut terminal, tx).await.unwrap();
    }

    /// Clicking where Help actually painted its close button must leave the
    /// screen: painting and hit testing both read `layout::help_layout`, and a
    /// hard-coded hit test would miss the button at one of these sizes (#300).
    #[tokio::test]
    async fn test_help_close_button_click_lands_where_it_is_painted() {
        for (width, height) in [(80u16, 24u16), (60, 12), (100, 40)] {
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.open_help();
            assert_eq!(app.view_mode(), crate::app::ViewMode::Help);
            click_painted_close_button(&mut app, width, height).await;
            assert_eq!(
                app.view_mode(),
                crate::app::ViewMode::DirectoryTree,
                "Help close button missed at {width}x{height}"
            );
        }
    }

    /// Same contract for Config, which reserves a taller body than Help does.
    #[tokio::test]
    async fn test_config_close_button_click_lands_where_it_is_painted() {
        for (width, height) in [(80u16, 24u16), (60, 12), (100, 40)] {
            let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app.open_config();
            assert_eq!(app.view_mode(), crate::app::ViewMode::ConfigMenu);
            click_painted_close_button(&mut app, width, height).await;
            assert_eq!(
                app.view_mode(),
                crate::app::ViewMode::DirectoryTree,
                "Config close button missed at {width}x{height}"
            );
        }
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.tree_list_mut().set_selected_idx(0);
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
        // hit-test (rather than one reading `layout::diff_layout`) would miss this.
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.tree_list_mut().set_selected_idx(0);
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
                app.confirm_modal().and_then(|m| m.default_action()),
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
        app.tree_list_mut().open();
        app.tree_list_mut().input_mut().set("iis".to_string());
        app.commit_filter();
        assert_eq!(app.tree_list().pattern(), "iis");

        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!quit, "Esc must not quit while a filter pattern is applied");
        assert!(app.tree_list().pattern().is_empty());

        // Diffs-only alone is dismissible too.
        app.tree_list_mut().toggle_diffs_only();
        app.commit_filter();
        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!quit, "Esc must not quit while diffs-only is applied");
        assert!(!app.tree_list().diffs_only());

        // Nothing left to dismiss — Esc falls through to quit.
        let quit = handle_key(esc, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!quit, "Commands request exit through App state");
        assert!(
            app.should_quit(),
            "Esc must request quit once nothing remains"
        );

        // `q` is unlayered: it quits even with a filter applied.
        let mut app = App::new(PathBuf::from("left"), PathBuf::from("right"));
        app.tree_list_mut().open();
        app.tree_list_mut().input_mut().set("iis".to_string());
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
        assert!(!quit, "Commands request exit through App state");
        assert!(app.should_quit(), "`q` must still request quit directly");
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
        app.tree_list_mut().open();
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

        assert_eq!(app.tree_list().input(), "config;F");
        assert!(
            !app.palette_visible(),
            "`;` must be typed, not open the menu, while the filter bar is open"
        );
        assert!(
            !app.tree_list().editing_diffs_only(),
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
        app.tree_list_mut().open();
        let (tx, _rx) = tokio::sync::mpsc::channel(8);

        let ctrl_f = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('f'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        handle_key(ctrl_f, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(
            app.tree_list().editing_diffs_only(),
            "the badge follows Ctrl+f"
        );
        assert!(
            !app.tree_list().diffs_only(),
            "nothing is applied until the query is committed"
        );
        assert_eq!(
            app.tree_list().input(),
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
        assert!(app.tree_list().diffs_only(), "Enter commits both together");

        // Esc restores the diffs-only value from before the editing session.
        app.tree_list_mut().open();
        handle_key(ctrl_f, &mut app, &mut terminal, tx.clone())
            .await
            .unwrap();
        assert!(!app.tree_list().editing_diffs_only());
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
            app.tree_list().diffs_only(),
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
        assert_eq!(app.palette().query(), "j;k");

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
        assert!(app.palette().query().is_empty());

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

        app.tree_list_mut().open();
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
        app.tree_list_mut().cancel();

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
                    ..Default::default()
                })
                .collect(),
        );
        app.apply_filter();
        crate::view::prepare_frame(&mut app, ratatui::layout::Rect::new(0, 0, 80, 24));
        app.tree_list_mut().set_selected_idx(0);
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

        assert_eq!(
            app.tree_list().selected_idx(),
            2,
            "the pointed row is selected first"
        );
        assert!(app.palette_visible());
        assert!(
            app.palette()
                .items()
                .iter()
                .any(|a| a.command == crate::commands::Command::BuiltinDiff && a.enabled()),
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
        app.palette_mut()
            .set_items(vec![crate::commands::CommandEntry::gated(
                "Open built-in Diff view",
                crate::commands::Command::BuiltinDiff,
                false,
                "no row is selected",
            )]);
        app.palette_mut().set_selected_idx(0);
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
        assert!(!is_error, "unavailability is informational: {msg}");
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
        let visible_rows = crate::layout::palette_layout(
            app.palette().items().len(),
            ratatui::layout::Rect::new(0, 0, 100, 12),
        )
        .visible_rows();
        assert!(
            app.palette().items().len() > visible_rows,
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

        crate::view::prepare_frame(&mut app, terminal.size().unwrap().into());

        let selected = app.palette().selected_idx();
        let offset = app.palette().scroll_offset();
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
            ..Default::default()
        }]);
        app.apply_filter();
        app.tree_list_mut().set_selected_idx(0);
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

        // `D` shares the same dispatch line; with external diff tool disabled
        // it is a harmless no-op that keeps the diff session.
        app.set_external_diff_tool(crate::settings::DiffToolSetting::Disabled);
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
