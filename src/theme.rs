use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};

/// Which built-in colour theme to use. Set `theme = "light"` in `config.toml` for
/// light-background terminals; the default `"dark"` suits dark-background terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

impl ThemeChoice {
    pub fn toggled(self) -> Self {
        match self {
            ThemeChoice::Dark => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Dark => "dark",
            ThemeChoice::Light => "light",
        }
    }
}

/// Semantic colour palette for the TUI. Render code (`ui.rs`) uses these fields rather than
/// hard-coded `Color::*` values so that swapping themes only requires changing `Theme::for_choice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Canvas background — `Color::Reset` (terminal native) for dark, `Color::White` for light.
    pub bg: Color,
    /// Default foreground text on `bg`.
    pub fg: Color,
    /// Hint keys, labels, timestamps (was `Color::Cyan`).
    pub accent: Color,
    /// Unfocused borders, separators, disabled items, muted placeholders (was `Color::DarkGray`).
    pub dim: Color,
    /// Identical rows, diff equal-context lines (was `Color::Gray`).
    pub muted: Color,
    /// Focused directory-tree pane border (was `Color::Green`).
    pub border_focus: Color,
    /// Selected row background in the tree / config lists (was `Color::DarkGray`).
    pub selection_bg: Color,
    /// Selected row foreground in the tree / config / palette lists (was `Color::White`).
    pub selection_fg: Color,
    /// OK status, LeftOnly rows, diff insertions (was `Color::Green`).
    pub success: Color,
    /// Error status, TypeConflict rows, diff deletions (was `Color::Red`).
    pub error: Color,
    /// Different rows, config/palette headers and borders, filter label, update hint (was `Color::Yellow`).
    pub warn: Color,
    /// RightOnly rows, About link, palette selection background (was `Color::Blue`).
    pub info: Color,
    /// Top-bar title, palette query cursor block (was `Color::White`).
    pub emphasis: Color,
    /// Background tint for a mergeable diff hunk.
    pub hunk_bg: Color,
    /// Background tint for the active (targeted) diff hunk.
    pub active_hunk_bg: Color,
    /// Background tint for the current diff cursor line.
    pub cursor_bg: Color,
}

impl Theme {
    pub const DARK: Theme = Theme {
        bg: Color::Reset,
        fg: Color::Reset,
        accent: Color::Cyan,
        dim: Color::DarkGray,
        muted: Color::Gray,
        border_focus: Color::Green,
        selection_bg: Color::DarkGray,
        selection_fg: Color::White,
        success: Color::Green,
        error: Color::Red,
        warn: Color::Yellow,
        info: Color::Blue,
        emphasis: Color::White,
        hunk_bg: Color::Rgb(32, 32, 48),
        active_hunk_bg: Color::Rgb(48, 48, 88),
        cursor_bg: Color::Rgb(64, 64, 64),
    };

    pub const LIGHT: Theme = Theme {
        bg: Color::White,
        fg: Color::Black,
        accent: Color::Indexed(30), // #008787 dark teal
        dim: Color::DarkGray,
        muted: Color::Indexed(240),       // #585858 dim grey
        border_focus: Color::Indexed(28), // #008700 dark green
        selection_bg: Color::DarkGray,
        selection_fg: Color::White,
        success: Color::Indexed(28), // #008700 dark green
        error: Color::Indexed(124),  // #af0000 dark red
        warn: Color::Indexed(94),    // #875f00 dark amber
        info: Color::Indexed(19),    // #0000af dark blue
        emphasis: Color::Black,
        hunk_bg: Color::Rgb(222, 222, 235),
        active_hunk_bg: Color::Rgb(205, 205, 240),
        cursor_bg: Color::Rgb(225, 225, 175),
    };

    pub fn for_choice(choice: ThemeChoice) -> Theme {
        match choice {
            ThemeChoice::Dark => Theme::DARK,
            ThemeChoice::Light => Theme::LIGHT,
        }
    }

    /// Base style for Block / List / Paragraph widgets: sets canvas `bg` and default `fg`
    /// so every cell drawn by the widget shows the theme background instead of the terminal's
    /// native colour. No-op for dark theme (`Color::Reset` = terminal default).
    pub fn base_style(self) -> Style {
        Style::default().bg(self.bg).fg(self.fg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggled_flips_between_dark_and_light() {
        assert_eq!(ThemeChoice::Dark.toggled(), ThemeChoice::Light);
        assert_eq!(ThemeChoice::Light.toggled(), ThemeChoice::Dark);
    }

    #[test]
    fn for_choice_maps_to_matching_palette() {
        assert_eq!(Theme::for_choice(ThemeChoice::Dark), Theme::DARK);
        assert_eq!(Theme::for_choice(ThemeChoice::Light), Theme::LIGHT);
    }

    #[test]
    fn dark_theme_matches_previous_hardcoded_defaults() {
        // Regression guard: the default theme must render byte-identical to the
        // pre-theme hardcoded colours so existing snapshot-style UI tests stay valid.
        assert_eq!(Theme::DARK.accent, Color::Cyan);
        assert_eq!(Theme::DARK.border_focus, Color::Green);
        assert_eq!(Theme::DARK.selection_bg, Color::DarkGray);
        assert_eq!(Theme::DARK.selection_fg, Color::White);
        assert_eq!(Theme::DARK.hunk_bg, Color::Rgb(32, 32, 48));
        assert_eq!(Theme::DARK.active_hunk_bg, Color::Rgb(48, 48, 88));
        assert_eq!(Theme::DARK.cursor_bg, Color::Rgb(64, 64, 64));
    }

    #[test]
    fn theme_choice_round_trips_through_toml() {
        // `toml` requires a table at the document root, so serialize via a wrapper
        // (mirrors how `ThemeChoice` is actually embedded in `AppSettings`).
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            theme: ThemeChoice,
        }
        let serialized = toml::to_string(&Wrapper {
            theme: ThemeChoice::Light,
        })
        .unwrap();
        assert_eq!(serialized.trim(), "theme = \"light\"");
        let parsed: Wrapper = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.theme, ThemeChoice::Light);
    }

    #[test]
    fn theme_choice_defaults_to_dark() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Dark);
    }
}
