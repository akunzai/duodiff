//! The Gestures: navigation keys the keyboard adapter answers itself, before
//! any binding is consulted. They are fixed — `[keys]` cannot move them — and
//! [`super::reserved_on`] refuses to bind a Command to one, so the table below
//! is the single list both dispatch and that check read.
//!
//! Only unconditional gestures live here. Keys the adapter takes in some
//! states only — `Enter` on a directory, `Esc` / `Backspace` while a filter is
//! applied, Help's `Enter` and `Tab` — stay in the adapter and stay bindable.

use crate::app::ViewMode;
use crossterm::event::{KeyCode, KeyModifiers};

/// A navigation intent the keyboard adapter carries out on the current screen.
/// Not a Command: it has no Palette entry and cannot be remapped (ADR-0003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    ScrollLeft,
    ScrollRight,
    /// Expand or collapse the selected directory.
    Toggle,
    /// Apply the selected Config row.
    Activate,
    Decrease,
    Increase,
    /// Open the Help topic at this index.
    SelectTopic(usize),
    OpenPalette,
}

/// Which modifiers a gesture's key still answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modifiers {
    /// Any, none included.
    Any,
    /// Any without Alt, which the difference jumps take on the arrows.
    NoAlt,
    /// Any with Ctrl.
    Ctrl,
}

impl Modifiers {
    fn admit(self, modifiers: KeyModifiers) -> bool {
        match self {
            Self::Any => true,
            Self::NoAlt => !modifiers.contains(KeyModifiers::ALT),
            Self::Ctrl => modifiers.contains(KeyModifiers::CONTROL),
        }
    }

    /// The modifiers a Help line names this gesture's key with.
    #[cfg(test)]
    fn shown(self) -> KeyModifiers {
        match self {
            Self::Any | Self::NoAlt => KeyModifiers::NONE,
            Self::Ctrl => KeyModifiers::CONTROL,
        }
    }
}

/// One gesture key. `screen` is `None` for a key every screen answers.
struct Entry {
    screen: Option<ViewMode>,
    code: KeyCode,
    modifiers: Modifiers,
    gesture: Gesture,
}

const fn entry(
    screen: Option<ViewMode>,
    code: KeyCode,
    modifiers: Modifiers,
    gesture: Gesture,
) -> Entry {
    Entry {
        screen,
        code,
        modifiers,
        gesture,
    }
}

const TREE: Option<ViewMode> = Some(ViewMode::DirectoryTree);
const DIFF: Option<ViewMode> = Some(ViewMode::FileDiff);
const CONFIG: Option<ViewMode> = Some(ViewMode::ConfigMenu);
const HELP: Option<ViewMode> = Some(ViewMode::Help);

use Gesture as G;
use KeyCode::{Char, Down, Enter, Left, Right, Up};
use Modifiers::{Any, Ctrl, NoAlt};

const TABLE: &[Entry] = &[
    entry(None, Char(';'), Any, G::OpenPalette),
    entry(None, Char('p'), Ctrl, G::OpenPalette),
    entry(TREE, Char('j'), Any, G::MoveDown),
    entry(TREE, Down, NoAlt, G::MoveDown),
    entry(TREE, Char('k'), Any, G::MoveUp),
    entry(TREE, Up, NoAlt, G::MoveUp),
    entry(TREE, Char('f'), Ctrl, G::PageDown),
    entry(TREE, Char('b'), Ctrl, G::PageUp),
    entry(TREE, Char(' '), Any, G::Toggle),
    entry(DIFF, Char('j'), Any, G::MoveDown),
    entry(DIFF, Down, NoAlt, G::MoveDown),
    entry(DIFF, Char('k'), Any, G::MoveUp),
    entry(DIFF, Up, NoAlt, G::MoveUp),
    entry(DIFF, Char('f'), Ctrl, G::PageDown),
    entry(DIFF, Char('b'), Ctrl, G::PageUp),
    entry(DIFF, Left, Any, G::ScrollLeft),
    entry(DIFF, Right, Any, G::ScrollRight),
    entry(CONFIG, Char('j'), Any, G::MoveDown),
    entry(CONFIG, Down, Any, G::MoveDown),
    entry(CONFIG, Char('k'), Any, G::MoveUp),
    entry(CONFIG, Up, Any, G::MoveUp),
    entry(CONFIG, Char(' '), Any, G::Activate),
    entry(CONFIG, Enter, Any, G::Activate),
    entry(CONFIG, Char('h'), Any, G::Decrease),
    entry(CONFIG, Left, Any, G::Decrease),
    entry(CONFIG, Char('l'), Any, G::Increase),
    entry(CONFIG, Right, Any, G::Increase),
    entry(HELP, Char('j'), Any, G::MoveDown),
    entry(HELP, Down, Any, G::MoveDown),
    entry(HELP, Char('k'), Any, G::MoveUp),
    entry(HELP, Up, Any, G::MoveUp),
    entry(HELP, Char('1'), Any, G::SelectTopic(0)),
    entry(HELP, Char('2'), Any, G::SelectTopic(1)),
    entry(HELP, Char('3'), Any, G::SelectTopic(2)),
    entry(HELP, Char('4'), Any, G::SelectTopic(3)),
    entry(HELP, Char('5'), Any, G::SelectTopic(4)),
    entry(HELP, Char('6'), Any, G::SelectTopic(5)),
];

/// The gesture `code` with `modifiers` is on `screen`, if any.
pub(super) fn gesture_on(
    screen: ViewMode,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Option<Gesture> {
    TABLE
        .iter()
        .find(|e| {
            e.screen.is_none_or(|s| s == screen) && e.code == code && e.modifiers.admit(modifiers)
        })
        .map(|e| e.gesture)
}

/// Every gesture key as a Help line names it (`Ctrl+f`, `Down`, `Space`),
/// with the screen it belongs to (`None` for every screen).
#[cfg(test)]
pub(crate) fn shown_keys() -> impl Iterator<Item = (Option<ViewMode>, String)> {
    TABLE.iter().map(|e| {
        let chord = super::Chord {
            code: e.code,
            modifiers: e.modifiers.shown(),
            shown: true,
        };
        (e.screen, super::format_chord(&chord))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KeyAction, Keymap};
    use crossterm::event::KeyEvent;

    const SCREENS: [ViewMode; 4] = [
        ViewMode::DirectoryTree,
        ViewMode::FileDiff,
        ViewMode::ConfigMenu,
        ViewMode::Help,
    ];

    /// Every gesture key resolves to its gesture, ahead of any binding, with
    /// every modifier its rule admits and none it refuses.
    #[test]
    fn every_gesture_key_resolves_to_its_gesture() {
        let keymap = Keymap::default();
        for e in TABLE {
            for screen in SCREENS
                .into_iter()
                .filter(|s| e.screen.is_none_or(|t| t == *s))
            {
                for modifiers in [KeyModifiers::NONE, KeyModifiers::CONTROL, KeyModifiers::ALT] {
                    let action = keymap.action_for_key(screen, &KeyEvent::new(e.code, modifiers));
                    if e.modifiers.admit(modifiers) {
                        assert_eq!(
                            action,
                            Some(KeyAction::Gesture(e.gesture)),
                            "{screen:?} {modifiers:?} {:?}",
                            e.code
                        );
                    } else {
                        assert!(
                            !matches!(action, Some(KeyAction::Gesture(_))),
                            "{screen:?} {modifiers:?} {:?}",
                            e.code
                        );
                    }
                }
            }
        }
    }

    /// `docs/CONFIGURATION.md` lists the keys `[keys]` cannot bind, screen by
    /// screen: exactly the gesture keys, in the config file's spelling.
    #[test]
    fn the_configuration_guide_lists_every_gesture_key() {
        let guide = include_str!("../../docs/CONFIGURATION.md");
        let table = guide
            .split("### Keys that cannot be bound")
            .nth(1)
            .expect("the guide has the section");
        let mut listed: Vec<(Option<ViewMode>, String)> = Vec::new();
        for line in table.lines().skip_while(|l| !l.starts_with('|')) {
            if !line.starts_with('|') {
                break;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let screen = match cells[0] {
                "Every screen" => None,
                "Directory Tree" => Some(ViewMode::DirectoryTree),
                "File Diff" => Some(ViewMode::FileDiff),
                "Config" => Some(ViewMode::ConfigMenu),
                "Help" => Some(ViewMode::Help),
                _ => continue,
            };
            let keys: Vec<&str> = cells[1].split('`').skip(1).step_by(2).collect();
            for (i, key) in keys.iter().enumerate() {
                listed.push((screen, key.to_string()));
                // "`1` … `6`" names the digits in between.
                if cells[1].contains(&format!("`{key}` … ")) {
                    let (from, to) = (key.as_bytes()[0], keys[i + 1].as_bytes()[0]);
                    listed.extend((from + 1..to).map(|c| (screen, (c as char).to_string())));
                }
            }
        }
        let mut expected: Vec<(Option<ViewMode>, String)> = shown_keys()
            .map(|(screen, key)| (screen, key.to_lowercase()))
            .collect();
        let order =
            |(screen, key): &(Option<ViewMode>, String)| (format!("{screen:?}"), key.clone());
        listed.sort_by_key(order);
        expected.sort_by_key(order);
        assert_eq!(listed, expected);
    }

    /// Alt+Down and Alt+Up stay free for the difference jumps.
    #[test]
    fn the_alt_arrows_reach_their_bindings() {
        let keymap = Keymap::default();
        for screen in [ViewMode::DirectoryTree, ViewMode::FileDiff] {
            for code in [Down, Up] {
                assert!(matches!(
                    keymap.action_for_key(screen, &KeyEvent::new(code, KeyModifiers::ALT)),
                    Some(KeyAction::Command(_))
                ));
            }
        }
    }
}
