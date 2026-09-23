//! The runtime key bindings: `App`'s single source of truth for which
//! [`Command`] a chord runs and what its display hint says (ADR-0003).
//!
//! [`Keymap::default`] reproduces the fixed tables this module replaces,
//! chord-for-chord and screen-for-screen, so behavior and every rendered
//! hint stay exactly as they were. A later slice adds a `[keys]` config
//! section that builds a non-default `Keymap` for [`crate::app::App::set_keymap`]
//! (Issue #339); this slice only makes the tables an owned, swappable value.

use crate::app::ViewMode;
use crate::commands::Command;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One keyboard chord bound to a [`Command`].
///
/// `shown` marks whether the chord appears in a display hint: an alias or an
/// Alt chord still routes but stays out of the hint, so a Command keeps
/// naming one key even when several chords reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub shown: bool,
}

impl Chord {
    pub const fn key(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
            shown: true,
        }
    }

    pub const fn alias(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::NONE,
            shown: false,
        }
    }

    pub const fn alt(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::ALT,
            shown: false,
        }
    }
}

/// A Command and every chord that reaches it on one screen.
#[derive(Clone, Debug)]
pub struct Binding {
    pub command: Command,
    pub chords: Vec<Chord>,
}

/// Modifiers that distinguish one chord from another. Shift is excluded
/// because terminals report it inconsistently for the uppercase bindings
/// (`D`, `L`, `N`).
const SIGNIFICANT_MODIFIERS: KeyModifiers = KeyModifiers::CONTROL.union(KeyModifiers::ALT);

/// The runtime key bindings `App` owns: every screen's table plus the global
/// one, keyed on [`ViewMode`]. Fields are `pub(crate)` so a test can build a
/// non-default value with struct-update syntax off [`Keymap::default`]
/// (Issue #339) ahead of the `[keys]` config slice that will do the same at
/// runtime.
#[derive(Clone, Debug)]
pub struct Keymap {
    pub(crate) global: Vec<Binding>,
    pub(crate) directory_tree: Vec<Binding>,
    pub(crate) file_diff: Vec<Binding>,
    pub(crate) config_menu: Vec<Binding>,
    pub(crate) help: Vec<Binding>,
}

impl Keymap {
    fn screen_bindings(&self, view_mode: ViewMode) -> &[Binding] {
        match view_mode {
            ViewMode::DirectoryTree => &self.directory_tree,
            ViewMode::FileDiff => &self.file_diff,
            ViewMode::ConfigMenu => &self.config_menu,
            ViewMode::Help => &self.help,
        }
    }

    /// Every table, global first then each screen — the order [`Keymap::hint`]
    /// searches in, same as the free `key_hint` this module replaces.
    fn tables(&self) -> [&[Binding]; 5] {
        [
            &self.global,
            &self.directory_tree,
            &self.file_diff,
            &self.config_menu,
            &self.help,
        ]
    }

    /// The Command a key press resolves to on `view_mode`'s own table, if any.
    /// The global table is a separate check ([`Keymap::global_command_for_key`]):
    /// some screens gate it on other state (e.g. the filter bar editing), so
    /// the keyboard adapter checks it before or instead of the screen table,
    /// not merged into it.
    pub fn command_for_key(&self, view_mode: ViewMode, key: &KeyEvent) -> Option<Command> {
        command_in(self.screen_bindings(view_mode), key)
    }

    /// The Command a key press resolves to on the global table, if any.
    pub fn global_command_for_key(&self, key: &KeyEvent) -> Option<Command> {
        command_in(&self.global, key)
    }

    /// The display hint for `command`, derived from the binding tables so a
    /// caller never restates a key this `Keymap` does not own: global table
    /// first, then each screen in table order, first binding wins, its shown
    /// chords joined with " / ". "" when `command` has no binding.
    pub fn hint(&self, command: Command) -> String {
        self.tables()
            .into_iter()
            .flatten()
            .find(|binding| binding.command == command)
            .map(|binding| {
                binding
                    .chords
                    .iter()
                    .filter(|chord| chord.shown)
                    .map(format_chord)
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default()
    }

    /// [`Keymap::hint`], `None` when `command` has no key — only possible once
    /// the `[keys]` config slice can unbind a Command — so callers can fall
    /// back to prose naming the Command Palette instead of a key (Issue #339).
    pub fn key_phrase(&self, command: Command) -> Option<String> {
        let hint = self.hint(command);
        (!hint.is_empty()).then_some(hint)
    }
}

fn chord_matches(chord: &Chord, key: &KeyEvent) -> bool {
    key.code == chord.code && key.modifiers & SIGNIFICANT_MODIFIERS == chord.modifiers
}

fn command_in(table: &[Binding], key: &KeyEvent) -> Option<Command> {
    table
        .iter()
        .find(|binding| binding.chords.iter().any(|chord| chord_matches(chord, key)))
        .map(|binding| binding.command)
}

/// One chord's display label: `Char('l')` → "l", `Right` → "Right", `Enter` →
/// "Enter", `Tab` → "Tab", `Esc` → "Esc", Alt+Down → "Alt+Down", Ctrl+x →
/// "Ctrl+x". Reproduces every hint the fixed tables printed before this
/// module owned them (Issue #339).
fn format_chord(chord: &Chord) -> String {
    let key = match chord.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        other => format!("{other:?}"),
    };
    let mut label = String::new();
    if chord.modifiers.contains(KeyModifiers::CONTROL) {
        label.push_str("Ctrl+");
    }
    if chord.modifiers.contains(KeyModifiers::ALT) {
        label.push_str("Alt+");
    }
    label.push_str(&key);
    label
}

/// Every screen answers this chord regardless of `ViewMode` — the theme
/// toggle. Screens that share a key give it different Commands, so the rest
/// of the table is per screen.
fn global_bindings() -> Vec<Binding> {
    vec![Binding {
        command: Command::ToggleTheme,
        chords: vec![Chord::key(KeyCode::Char('T'))],
    }]
}

fn directory_tree_bindings() -> Vec<Binding> {
    vec![
        Binding {
            command: Command::BuiltinDiff,
            chords: vec![Chord::key(KeyCode::Enter)],
        },
        Binding {
            command: Command::ExternalDiff,
            chords: vec![Chord::key(KeyCode::Char('D'))],
        },
        Binding {
            command: Command::ExternalEdit,
            chords: vec![Chord::key(KeyCode::Char('E'))],
        },
        Binding {
            command: Command::CopyLeftToRight,
            chords: vec![Chord::key(KeyCode::Char('R'))],
        },
        Binding {
            command: Command::CopyRightToLeft,
            chords: vec![Chord::key(KeyCode::Char('L'))],
        },
        Binding {
            command: Command::Expand,
            chords: vec![Chord::key(KeyCode::Char('l')), Chord::key(KeyCode::Right)],
        },
        Binding {
            command: Command::Collapse,
            chords: vec![Chord::key(KeyCode::Char('h')), Chord::key(KeyCode::Left)],
        },
        Binding {
            command: Command::NextDifference,
            chords: vec![Chord::key(KeyCode::Char('N')), Chord::alt(KeyCode::Down)],
        },
        Binding {
            command: Command::PrevDifference,
            chords: vec![Chord::key(KeyCode::Char('P')), Chord::alt(KeyCode::Up)],
        },
        Binding {
            command: Command::ExpandAll,
            chords: vec![
                Chord::key(KeyCode::Char('+')),
                // `=` shares the key with `+` on common layouts, so it works
                // without Shift.
                Chord::alias(KeyCode::Char('=')),
            ],
        },
        Binding {
            command: Command::CollapseAll,
            chords: vec![Chord::key(KeyCode::Char('-'))],
        },
        Binding {
            command: Command::ToggleFocus,
            chords: vec![Chord::key(KeyCode::Tab)],
        },
        Binding {
            command: Command::FocusLeft,
            chords: vec![Chord::key(KeyCode::Char('1'))],
        },
        Binding {
            command: Command::FocusRight,
            chords: vec![Chord::key(KeyCode::Char('2'))],
        },
        Binding {
            command: Command::Filter,
            chords: vec![Chord::key(KeyCode::Char('/'))],
        },
        Binding {
            command: Command::SwapPaths,
            chords: vec![Chord::key(KeyCode::Char('s'))],
        },
        Binding {
            command: Command::ToggleScan,
            chords: vec![Chord::key(KeyCode::Char('c'))],
        },
        Binding {
            command: Command::Refresh,
            chords: vec![Chord::key(KeyCode::Char('r'))],
        },
        Binding {
            command: Command::Config,
            chords: vec![Chord::key(KeyCode::Char('C'))],
        },
        Binding {
            command: Command::Help,
            chords: vec![Chord::key(KeyCode::Char('?'))],
        },
        Binding {
            command: Command::Quit,
            chords: vec![Chord::key(KeyCode::Char('q')), Chord::alias(KeyCode::Esc)],
        },
    ]
}

fn file_diff_bindings() -> Vec<Binding> {
    vec![
        Binding {
            command: Command::NextChange,
            chords: vec![Chord::key(KeyCode::Char('N')), Chord::alt(KeyCode::Down)],
        },
        Binding {
            command: Command::PrevChange,
            chords: vec![Chord::key(KeyCode::Char('P')), Chord::alt(KeyCode::Up)],
        },
        Binding {
            command: Command::StageLeftToRight,
            chords: vec![Chord::key(KeyCode::Char(']'))],
        },
        Binding {
            command: Command::StageRightToLeft,
            chords: vec![Chord::key(KeyCode::Char('['))],
        },
        // Whole-file overwrite stays on the uppercase keys only. Lowercase
        // `l`/`r` are harmless in the Directory Tree (expand / re-scan), so
        // binding them to a destructive overwrite here turned tree muscle
        // memory into data loss behind a single `y` (Issue #234).
        Binding {
            command: Command::CopyLeftToRight,
            chords: vec![Chord::key(KeyCode::Char('R'))],
        },
        Binding {
            command: Command::CopyRightToLeft,
            chords: vec![Chord::key(KeyCode::Char('L'))],
        },
        // The palette lists both for File Diff, so they need matching direct
        // bindings here as well as in the Directory Tree (Issue #239).
        Binding {
            command: Command::ExternalDiff,
            chords: vec![Chord::key(KeyCode::Char('D'))],
        },
        Binding {
            command: Command::ExternalEdit,
            chords: vec![Chord::key(KeyCode::Char('E'))],
        },
        Binding {
            command: Command::SaveStaged,
            chords: vec![Chord::key(KeyCode::Char('s'))],
        },
        Binding {
            command: Command::UndoStaged,
            chords: vec![Chord::key(KeyCode::Char('u'))],
        },
        Binding {
            command: Command::ToggleWrap,
            chords: vec![Chord::key(KeyCode::Char('w'))],
        },
        Binding {
            command: Command::ToggleFullDiff,
            chords: vec![Chord::key(KeyCode::Char('f'))],
        },
        Binding {
            command: Command::Config,
            chords: vec![Chord::key(KeyCode::Char('C'))],
        },
        Binding {
            command: Command::Help,
            chords: vec![Chord::key(KeyCode::Char('?'))],
        },
        // Never walk out on unwritten work: the dirty gate opens a
        // Save / Discard / Cancel dialog instead (Issue #235).
        Binding {
            command: Command::Back,
            chords: vec![Chord::key(KeyCode::Esc), Chord::alias(KeyCode::Char('q'))],
        },
    ]
}

fn config_menu_bindings() -> Vec<Binding> {
    vec![
        Binding {
            command: Command::Help,
            chords: vec![Chord::key(KeyCode::Char('?'))],
        },
        Binding {
            command: Command::Back,
            chords: vec![Chord::key(KeyCode::Esc), Chord::alias(KeyCode::Char('q'))],
        },
    ]
}

fn help_bindings() -> Vec<Binding> {
    vec![
        Binding {
            command: Command::Config,
            chords: vec![Chord::key(KeyCode::Char('C'))],
        },
        Binding {
            command: Command::Back,
            chords: vec![
                Chord::key(KeyCode::Esc),
                Chord::alias(KeyCode::Char('q')),
                Chord::alias(KeyCode::Char('?')),
            ],
        },
    ]
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            global: global_bindings(),
            directory_tree: directory_tree_bindings(),
            file_diff: file_diff_bindings(),
            config_menu: config_menu_bindings(),
            help: help_bindings(),
        }
    }
}

/// A non-default `Keymap` several test modules share (`keymap`, `commands`,
/// `ui`) to exercise the same remap across every seam it touches — the
/// Palette key column, a Help topic line, the diff footer / staged line, the
/// top bar, and a toast (Issue #339): `copy_to_right` on `L`, `copy_to_left`
/// on `R` (swapped from the default), `config` on `x`, and `save_staged`
/// unbound.
#[cfg(test)]
pub(crate) fn sample_remapped_keymap() -> Keymap {
    let mut keymap = Keymap::default();
    for binding in &mut keymap.directory_tree {
        match binding.command {
            Command::CopyLeftToRight => binding.chords = vec![Chord::key(KeyCode::Char('L'))],
            Command::CopyRightToLeft => binding.chords = vec![Chord::key(KeyCode::Char('R'))],
            Command::Config => binding.chords = vec![Chord::key(KeyCode::Char('x'))],
            _ => {}
        }
    }
    for binding in &mut keymap.file_diff {
        match binding.command {
            Command::CopyLeftToRight => binding.chords = vec![Chord::key(KeyCode::Char('L'))],
            Command::CopyRightToLeft => binding.chords = vec![Chord::key(KeyCode::Char('R'))],
            Command::SaveStaged => binding.chords = Vec::new(),
            _ => {}
        }
    }
    keymap
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use std::path::PathBuf;

    /// Every Command that has a default binding reproduces today's hint
    /// exactly — the main acceptance signal that this refactor changes no
    /// behavior (Issue #339).
    #[test]
    fn default_keymap_reproduces_every_command_hint() {
        let keymap = Keymap::default();
        let expected = [
            (Command::ExternalDiff, "D"),
            (Command::SaveStaged, "s"),
            (Command::UndoStaged, "u"),
            (Command::ToggleTheme, "T"),
            (Command::ToggleFocus, "Tab"),
            (Command::FocusLeft, "1"),
            (Command::FocusRight, "2"),
            (Command::Expand, "l / Right"),
            (Command::Collapse, "h / Left"),
            (Command::ExpandAll, "+"),
            (Command::CollapseAll, "-"),
            (Command::ExternalEdit, "E"),
            (Command::CopyLeftToRight, "R"),
            (Command::CopyRightToLeft, "L"),
            (Command::BuiltinDiff, "Enter"),
            (Command::SwapPaths, "s"),
            (Command::ToggleScan, "c"),
            (Command::Refresh, "r"),
            (Command::Config, "C"),
            (Command::Help, "?"),
            (Command::Filter, "/"),
            (Command::Quit, "q"),
            (Command::ToggleWrap, "w"),
            (Command::ToggleFullDiff, "f"),
            (Command::NextChange, "N"),
            (Command::PrevChange, "P"),
            (Command::NextDifference, "N"),
            (Command::PrevDifference, "P"),
            (Command::StageLeftToRight, "]"),
            (Command::StageRightToLeft, "["),
            (Command::Back, "Esc"),
            (Command::OpenRepository, ""),
        ];
        for (command, hint) in expected {
            assert_eq!(keymap.hint(command), hint, "{command:?}");
        }
    }

    #[test]
    fn key_phrase_is_none_exactly_when_the_hint_is_empty() {
        let keymap = Keymap::default();
        assert_eq!(keymap.key_phrase(Command::Quit), Some("q".to_string()));
        assert_eq!(keymap.key_phrase(Command::OpenRepository), None);
    }

    #[test]
    fn every_listed_command_is_bound_on_the_screen_that_lists_it() {
        let keymap = Keymap::default();
        for view_mode in [
            ViewMode::DirectoryTree,
            ViewMode::FileDiff,
            ViewMode::ConfigMenu,
            ViewMode::Help,
        ] {
            let mut app_state = App::new(PathBuf::from("left"), PathBuf::from("right"));
            app_state.set_view_mode(view_mode);
            for entry in crate::commands::inventory_entries(&app_state) {
                let bound = keymap
                    .screen_bindings(view_mode)
                    .iter()
                    .chain(&keymap.global)
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
        let keymap = Keymap::default();
        for table in [
            &keymap.global,
            &keymap.directory_tree,
            &keymap.file_diff,
            &keymap.config_menu,
            &keymap.help,
        ] {
            let mut seen: Vec<(KeyCode, KeyModifiers)> = Vec::new();
            for binding in table {
                for chord in &binding.chords {
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
        let keymap = Keymap::default();

        assert_eq!(
            keymap.command_for_key(
                ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)
            ),
            Some(Command::NextChange)
        );
        // Plain Down scrolls; it must not resolve to a Command at all.
        assert_eq!(
            keymap.command_for_key(
                ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Down, KeyModifiers::empty())
            ),
            None
        );
        // Shift is how terminals report the uppercase bindings, so it is ignored.
        assert_eq!(
            keymap.command_for_key(
                ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT)
            ),
            Some(Command::NextChange)
        );
        // Ctrl+f pages the diff; only the bare `f` toggles full-file context.
        assert_eq!(
            keymap.command_for_key(
                ViewMode::FileDiff,
                &KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)
            ),
            None
        );
    }

    /// Issue #338: `=` reaches Expand all without Shift, but the hint names `+`;
    /// `N` / `P` jump between differences as they jump between change blocks.
    #[test]
    fn the_issue_338_keys_route_in_the_directory_tree() {
        let keymap = Keymap::default();

        for (key, modifiers, expected) in [
            ('-', KeyModifiers::empty(), Command::CollapseAll),
            ('+', KeyModifiers::SHIFT, Command::ExpandAll),
            ('=', KeyModifiers::empty(), Command::ExpandAll),
            ('N', KeyModifiers::SHIFT, Command::NextDifference),
            ('P', KeyModifiers::SHIFT, Command::PrevDifference),
        ] {
            assert_eq!(
                keymap.command_for_key(
                    ViewMode::DirectoryTree,
                    &KeyEvent::new(KeyCode::Char(key), modifiers)
                ),
                Some(expected),
                "{key:?}"
            );
        }
        assert_eq!(keymap.hint(Command::ExpandAll), "+");
        assert_eq!(
            keymap.command_for_key(
                ViewMode::DirectoryTree,
                &KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)
            ),
            Some(Command::NextDifference)
        );
        assert_eq!(keymap.hint(Command::NextDifference), "N");
        assert_eq!(keymap.hint(Command::PrevDifference), "P");
        assert_eq!(keymap.hint(Command::CollapseAll), "-");
    }

    #[test]
    fn the_same_key_resolves_per_screen() {
        let keymap = Keymap::default();

        let question = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::empty());
        assert_eq!(
            keymap.command_for_key(ViewMode::DirectoryTree, &question),
            Some(Command::Help)
        );
        // Inside Help, `?` closes the screen it would otherwise open.
        assert_eq!(
            keymap.command_for_key(ViewMode::Help, &question),
            Some(Command::Back)
        );

        let escape = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert_eq!(
            keymap.command_for_key(ViewMode::DirectoryTree, &escape),
            Some(Command::Quit)
        );
        assert_eq!(
            keymap.command_for_key(ViewMode::FileDiff, &escape),
            Some(Command::Back)
        );
    }

    /// A `Keymap` with a chord this repo's crossterm build does not name a
    /// short label for still formats — `KeyCode`'s `Debug` is the fallback,
    /// exercised here via a function key no screen actually binds.
    #[test]
    fn format_chord_falls_back_to_debug_for_uncommon_keys() {
        let keymap = Keymap {
            directory_tree: vec![Binding {
                command: Command::Refresh,
                chords: vec![Chord::key(KeyCode::F(5))],
            }],
            ..Keymap::default()
        };
        assert_eq!(keymap.hint(Command::Refresh), "F5");
    }

    /// A remapped Keymap drives routing: the new chord reaches the Command,
    /// and the old default chord no longer does (Issue #339).
    #[test]
    fn a_remapped_keymap_routes_the_new_key_and_not_the_old_one() {
        let keymap = Keymap {
            directory_tree: vec![Binding {
                command: Command::CopyRightToLeft,
                chords: vec![Chord::key(KeyCode::Char('R'))],
            }],
            ..Keymap::default()
        };
        assert_eq!(
            keymap.command_for_key(
                ViewMode::DirectoryTree,
                &KeyEvent::new(KeyCode::Char('R'), KeyModifiers::empty())
            ),
            Some(Command::CopyRightToLeft)
        );
        assert_eq!(
            keymap.command_for_key(
                ViewMode::DirectoryTree,
                &KeyEvent::new(KeyCode::Char('L'), KeyModifiers::empty())
            ),
            None
        );
    }
}
