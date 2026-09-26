//! What the Help screen says on each topic, as lines of text. The painter
//! styles them; the frame preparation counts them to bound scrolling; hit
//! testing finds the repository link among them.

use crate::app::HelpTopic;

/// One line of a Help topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelpLine {
    Text(String),
    /// The repository URL, which a click opens in the browser. Painted after
    /// a two-column indent.
    Link(String),
}

/// The digit keys that jump straight to a topic, such as `1-6`.
pub fn topic_keys() -> String {
    format!("1-{}", HelpTopic::all().len())
}

/// The row of the repository link among `lines`, if they have one.
pub fn link_row(lines: &[HelpLine]) -> Option<usize> {
    lines
        .iter()
        .position(|line| matches!(line, HelpLine::Link(_)))
}

fn text_lines(text: &str) -> Vec<HelpLine> {
    text.lines()
        .map(|line| HelpLine::Text(line.to_string()))
        .collect()
}

/// The description column every Directory Tree / File Diff / General Help
/// action line lines up on (2-space indent + this key-cell width = column 17).
const HELP_COL: usize = 15;
/// Same idea for the Config topic, whose longest key ("Enter / Space") needs
/// a wider cell (2-space indent + this width = column 21).
const HELP_CONFIG_COL: usize = 19;

/// Pad a Help line's key cell to `width` characters so the description stays
/// aligned; a key at least as wide as `width` still gets two spaces before
/// the description rather than colliding with it (Issue #339).
fn help_key_col(key: &str, width: usize) -> String {
    let key_width = crate::wrap::display_width(key);
    if key_width + 2 <= width {
        format!("{key}{}", " ".repeat(width - key_width))
    } else {
        format!("{key}  ")
    }
}

/// A Help action line's key cell: the Command's hint, or a Palette
/// placeholder when it has no key (Issue #339).
fn help_key(keymap: &crate::keymap::Keymap, command: crate::commands::Command) -> String {
    keymap
        .key_phrase(command)
        .unwrap_or_else(|| "(Command Palette)".to_string())
}

/// `text` while `command` keeps its default keys, "" once `[keys]` rebinds
/// it: Help prose naming a default-only alias (`q`, `=`, `Alt+Down`) would
/// otherwise advertise a chord that no longer runs it (Issue #339).
fn default_alias(
    keymap: &crate::keymap::Keymap,
    command: crate::commands::Command,
    text: &str,
) -> String {
    if keymap.is_customized(command) {
        String::new()
    } else {
        text.to_string()
    }
}

/// `help_key_col(help_key(keymap, command), HELP_COL)`.
fn help_line_key(keymap: &crate::keymap::Keymap, command: crate::commands::Command) -> String {
    help_key_col(&help_key(keymap, command), HELP_COL)
}

/// What Help says on `topic`, one line per row: keys come from `keymap`,
/// and About names `update_available` when a check found a newer release.
pub fn topic_lines(
    topic: HelpTopic,
    update_available: Option<&str>,
    install_method: &crate::upgrade::InstallMethod,
    keymap: &crate::keymap::Keymap,
) -> Vec<HelpLine> {
    use crate::commands::Command;
    match topic {
        HelpTopic::DirectoryTree => text_lines(&format!(
            "\
Navigation
  j / Down       move selection down
  k / Up         move selection up
  Ctrl+f         page selection down (about one screen)
  Ctrl+b         page selection up (about one screen)
  {collapse}collapse the selected directory
  {expand}expand the selected directory
  Space          toggle expand/collapse
  {collapse_expand_all}collapse / expand every directory{expand_all_alias}
  {next_prev_difference}jump to the next / previous difference, expanding
                 the directories above it{next_prev_difference_alias}
  {toggle_focus}switch focus between the left and right panes
  {focus_left_right}jump focus directly to the left / right pane

Row states
  =              no difference found by the active scan mode
  ≈              content unverified — the bytes were not compared
                 (Fast mode: sizes match but timestamps differ;
                  Precise mode: a side could not be read or hashed)
  ≠              a difference the scan established
  < / >          present on the left / right side only
  !              one side is a file, the other a directory
  Aa             case-only path mismatch or collision

Row shape
  ▸ / ▾          a collapsed / expanded directory; a trailing / marks
                 a directory even where the marker does not apply
  (none)         a file, indented in line with its siblings

Scale
  n/N            selected row and visible-row count, on each pane's
                 bottom border (follows the filter, not collapse)
  footer         whole-tree inventory (differ / left-only /
                 right-only / unverified / identical), independent
                 of collapse and filter; trailing units drop on
                 narrow terminals

Actions
  {builtin_diff}open the diff view (or toggle expand, for a directory)
  {external_diff}compare the selected file pair with the external diff tool
  {external_edit}edit the selected file in $EDITOR/$VISUAL
  {copy_right_to_left}copy the selected item from the right pane to the left (y/n confirm)
  {copy_left_to_right}copy the selected item from the left pane to the right (y/n confirm)
  {config}open the Config screen
  {toggle_scan}switch Fast / Precise scan mode (persists, then re-scans)
  {refresh}force a manual re-scan
  {swap_paths}swap the left and right directories
  {filter}open the filter bar; every printable character is typed
                 into the query (Ctrl+f: toggle diffs-only,
                 Enter: apply, Esc: cancel)
  {help}show this help
  Esc            clear the applied filter, or quit when none is applied
  {quit}quit",
            collapse = help_line_key(keymap, Command::Collapse),
            expand = help_line_key(keymap, Command::Expand),
            expand_all_alias = default_alias(keymap, Command::ExpandAll, " (= also expands)"),
            next_prev_difference_alias = if keymap.is_customized(Command::PrevDifference) {
                String::new()
            } else {
                default_alias(keymap, Command::NextDifference, " (also Alt+Down / Alt+Up)")
            },
            collapse_expand_all = help_key_col(
                &format!(
                    "{} / {}",
                    help_key(keymap, Command::CollapseAll),
                    help_key(keymap, Command::ExpandAll)
                ),
                HELP_COL
            ),
            next_prev_difference = help_key_col(
                &format!(
                    "{} / {}",
                    help_key(keymap, Command::NextDifference),
                    help_key(keymap, Command::PrevDifference)
                ),
                HELP_COL
            ),
            toggle_focus = help_line_key(keymap, Command::ToggleFocus),
            focus_left_right = help_key_col(
                &format!(
                    "{} / {}",
                    help_key(keymap, Command::FocusLeft),
                    help_key(keymap, Command::FocusRight)
                ),
                HELP_COL
            ),
            builtin_diff = help_line_key(keymap, Command::BuiltinDiff),
            external_diff = help_line_key(keymap, Command::ExternalDiff),
            external_edit = help_line_key(keymap, Command::ExternalEdit),
            copy_right_to_left = help_line_key(keymap, Command::CopyRightToLeft),
            copy_left_to_right = help_line_key(keymap, Command::CopyLeftToRight),
            config = help_line_key(keymap, Command::Config),
            toggle_scan = help_line_key(keymap, Command::ToggleScan),
            refresh = help_line_key(keymap, Command::Refresh),
            swap_paths = help_line_key(keymap, Command::SwapPaths),
            filter = help_line_key(keymap, Command::Filter),
            help = help_line_key(keymap, Command::Help),
            quit = help_line_key(keymap, Command::Quit),
        )),
        HelpTopic::FileDiff => text_lines(&format!(
            "  Limits         UTF-8 text only, max 10 MiB per side
                 (binary / non-UTF-8 / oversized → toast; use D)
  read-only      a pane titled read-only (/dev/null, a pipe, or a file
                 you cannot write) refuses [ / ] / s / L / R into it
  j / Down       scroll down one line
  k / Up         scroll up one line
  Ctrl+f         page scroll down (about one screen)
  Ctrl+b         page scroll up (about one screen)
  {next_change}jump to next change block
  {prev_change}jump to previous change block
  Left / Right   scroll horizontally (only while wrap is off)
  Gutters        1-based source line numbers; - deleted, + inserted,
                 blank for context, … for an omitted collapsed range
  Highlighting   mergeable blocks are tinted; the active block and
                 current line are emphasized for `{stage_right_to_left}` / `{stage_left_to_right}` targets
  {stage_right_to_left_line}stage the change block under the cursor to the left
  {stage_left_to_right_line}stage the change block under the cursor to the right
                 (repeatable — stage more blocks, then {save_staged} saves them all;
                 a `*` marks each dirty pane title until then)
  {save_staged_line}save every staged side (shows the paths with home as ~,
                 then confirms)
  {undo_staged}undo the last staged change block
  {copy_right_to_left}copy the whole right file to the left side (confirm)
  {copy_left_to_right}copy the whole left file to the right side (confirm)
                 (both are blocked while staged changes are unsaved)
  {toggle_wrap}toggle line wrapping
  {toggle_full_diff}toggle full-file context vs diff-only
  {external_diff}compare the same pair with the external diff tool
  {external_edit}edit the focused side's file in $EDITOR/$VISUAL
  {config}open the Config screen (returns here on {back}{back_alias})
  {help}show this help
  {quit_or_back}return to the Directory Tree view, or quit when duodiff
                 was started on two files",
            next_change = help_key_col(
                &format!(
                    "{}{}",
                    help_key(keymap, Command::NextChange),
                    default_alias(keymap, Command::NextChange, " / Alt+Down")
                ),
                HELP_COL
            ),
            prev_change = help_key_col(
                &format!(
                    "{}{}",
                    help_key(keymap, Command::PrevChange),
                    default_alias(keymap, Command::PrevChange, " / Alt+Up")
                ),
                HELP_COL
            ),
            stage_right_to_left = help_key(keymap, Command::StageRightToLeft),
            stage_left_to_right = help_key(keymap, Command::StageLeftToRight),
            stage_right_to_left_line = help_line_key(keymap, Command::StageRightToLeft),
            stage_left_to_right_line = help_line_key(keymap, Command::StageLeftToRight),
            save_staged = help_key(keymap, Command::SaveStaged),
            save_staged_line = help_line_key(keymap, Command::SaveStaged),
            undo_staged = help_line_key(keymap, Command::UndoStaged),
            copy_right_to_left = help_line_key(keymap, Command::CopyRightToLeft),
            copy_left_to_right = help_line_key(keymap, Command::CopyLeftToRight),
            toggle_wrap = help_line_key(keymap, Command::ToggleWrap),
            toggle_full_diff = help_line_key(keymap, Command::ToggleFullDiff),
            external_diff = help_line_key(keymap, Command::ExternalDiff),
            external_edit = help_line_key(keymap, Command::ExternalEdit),
            config = help_line_key(keymap, Command::Config),
            back = help_key(keymap, Command::Back),
            help = help_line_key(keymap, Command::Help),
            quit_or_back = help_key_col(
                &format!(
                    "{}{}",
                    default_alias(keymap, Command::Back, "q / "),
                    help_key(keymap, Command::Back)
                ),
                HELP_COL
            ),
            back_alias = default_alias(keymap, Command::Back, "/q"),
        )),
        HelpTopic::Config => text_lines(&format!(
            "  j / k, Down / Up   move the selection (skips unavailable tools)
  Enter / Space      select Auto, Disabled, or an available tool,
                     or toggle Check for updates / Mouse support / Theme
                     / Scan mode / Respect .gitignore; Global exclusions opens
                     a list editor (a add, Enter edit, d delete, r restore
                     defaults, J/K reorder, Ctrl+s apply + one rescan, Esc cancel;
                     the list grows with the terminal and scrolls with the selection)
  {toggle_theme}toggle light/dark theme from anywhere (persists)
  h / l, Left / Right  adjust the Diff context line count
  {help}show this help
  {quit_or_back}return to the screen you opened Config from

  External diff tool choices: Auto (resolves the first launchable tool
  by fixed priority: vim, nvim, code, meld, bcomp, smerge, ksdiff, difft),
  Disabled, or a pinned tool. Unavailable tools are shown as [-] and
  cannot be selected.

  Settings are saved to ~/.config/duodiff/config.toml (honors
  XDG_CONFIG_HOME). See config.example.toml in the repo for every
  field, its default, and what it does.",
            toggle_theme = help_key_col(&help_key(keymap, Command::ToggleTheme), HELP_CONFIG_COL),
            help = help_key_col(&help_key(keymap, Command::Help), HELP_CONFIG_COL),
            quit_or_back = help_key_col(
                &format!(
                    "{}{}",
                    default_alias(keymap, Command::Back, "q / "),
                    help_key(keymap, Command::Back)
                ),
                HELP_CONFIG_COL
            ),
        )),
        HelpTopic::Mouse => text_lines(
            "  Left Click     select the clicked row
  Right Click    select a row and open the Command Palette
  Double Click   open diff view for a file, or expand/collapse a directory
  Scroll         scroll the directory tree, diff lines, Config screen, Help
                 topic/index, or the menu/palette list; over the Config
                 screen's Diff context row, scroll adjusts its value

  Mouse is on by default; disable it in Config, in config.toml
  (mouse = false), or for one session with --no-mouse.",
        ),
        HelpTopic::General => text_lines(&format!(
            "  ; / Ctrl+p    open the Command Palette (right-click does too);
                 type to search every command for the current screen,
                 Up/Down to select, Enter to run, Esc or Ctrl+p to close
  {help}show this help
  {quit_or_back}quit (or back, on any sub-screen); in the Directory Tree
                 Esc clears an applied filter before it will quit
  {toggle_theme}toggle light/dark theme (persists across restart)
  Tab            (inside Help) open the topic index list
  {jump}(inside Help) jump straight to a topic
  j / k, Down / Up
                 (inside Help) scroll the topic, or move in the index",
            help = help_line_key(keymap, Command::Help),
            quit_or_back = help_key_col(
                &format!(
                    "{} / {}",
                    help_key(keymap, Command::Quit),
                    help_key(keymap, Command::Back)
                ),
                HELP_COL
            ),
            toggle_theme = help_line_key(keymap, Command::ToggleTheme),
            jump = help_key_col(&topic_keys(), HELP_COL),
        )),
        HelpTopic::About => {
            let repo = env!("CARGO_PKG_REPOSITORY")
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let mut lines = vec![
                HelpLine::Text(format!("duodiff v{}", env!("CARGO_PKG_VERSION"))),
                HelpLine::Text(String::new()),
                HelpLine::Link(repo.to_string()),
                HelpLine::Text(String::new()),
            ];
            if let Some(version) = update_available {
                lines.push(HelpLine::Text(crate::upgrade::update_hint(
                    version,
                    install_method,
                )));
            }
            lines
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A topic's lines as plain text, the link included.
    fn texts(topic: HelpTopic, keymap: &crate::keymap::Keymap) -> Vec<String> {
        topic_lines(
            topic,
            None,
            &crate::upgrade::InstallMethod::Standalone,
            keymap,
        )
        .into_iter()
        .map(|line| match line {
            HelpLine::Text(text) => text,
            HelpLine::Link(url) => format!("  {url}"),
        })
        .collect()
    }

    /// The keys a topic's key column names: the cell before the two-space gap
    /// on each line, split on " / " and ", ", with a digit range such as
    /// "1-6" expanded.
    fn keys_named(topic: HelpTopic) -> Vec<String> {
        let mut keys = Vec::new();
        for text in texts(topic, &crate::keymap::Keymap::default()) {
            let cell = text.trim_start().split("  ").next().unwrap_or_default();
            for key in cell.split(" / ").flat_map(|part| part.split(", ")) {
                let key = key.trim();
                match key.split_once('-') {
                    Some((from, to)) if from.len() == 1 && to.len() == 1 => {
                        let (from, to) = (from.as_bytes()[0], to.as_bytes()[0]);
                        keys.extend((from..=to).map(|c| (c as char).to_string()));
                    }
                    _ => keys.push(key.to_string()),
                }
            }
        }
        keys
    }

    /// Help lists every gesture key on the topic for its screen, the General
    /// topic covering Help itself and the keys every screen shares — so the
    /// gesture table and the hand-written topics cannot drift apart.
    #[test]
    fn help_topics_name_every_gesture_key() {
        for (screen, key) in crate::keymap::gesture_keys() {
            let topic = match screen {
                Some(crate::app::ViewMode::DirectoryTree) => HelpTopic::DirectoryTree,
                Some(crate::app::ViewMode::FileDiff) => HelpTopic::FileDiff,
                Some(crate::app::ViewMode::ConfigMenu) => HelpTopic::Config,
                Some(crate::app::ViewMode::Help) | None => HelpTopic::General,
            };
            assert!(
                keys_named(topic).contains(&key),
                "{topic:?} does not name {key}: {:?}",
                keys_named(topic)
            );
        }
    }

    /// About's link is the repository, and hit testing finds it whether or
    /// not an update notice follows.
    #[test]
    fn abouts_link_is_the_repository() {
        let repo = env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://");
        for update in [None, Some("9.9.9")] {
            let lines = topic_lines(
                HelpTopic::About,
                update,
                &crate::upgrade::InstallMethod::Standalone,
                &crate::keymap::Keymap::default(),
            );
            let row = link_row(&lines).expect("About has a link");
            assert_eq!(lines[row], HelpLine::Link(repo.to_string()));
        }
    }

    /// The digit keys cover every topic, in the General topic as in the
    /// index title.
    #[test]
    fn the_topic_keys_cover_every_topic() {
        assert_eq!(topic_keys(), format!("1-{}", HelpTopic::all().len()));
        assert!(keys_named(HelpTopic::General).contains(&HelpTopic::all().len().to_string()));
    }

    /// Issue #339: Help names a default-only alias (`q`, `=`, `Alt+Down`) only
    /// while its Command keeps the defaults; once `[keys]` rebinds it, that
    /// alias no longer runs it and is not advertised.
    #[test]
    fn help_drops_default_only_aliases_of_a_remapped_command() {
        let body = |topic, keymap: &crate::keymap::Keymap| texts(topic, keymap).join("\n");
        let defaults = crate::keymap::Keymap::default();
        let (remapped, problems) = crate::keymap::Keymap::with_overrides(
            &"back = \"x\"\nnext_change = \"n\"\nexpand_all = \"*\"\n"
                .parse::<toml::Table>()
                .unwrap(),
        );
        assert_eq!(problems, Vec::<String>::new());

        let diff = body(HelpTopic::FileDiff, &defaults);
        assert!(diff.contains("N / Alt+Down"), "{diff}");
        assert!(diff.contains("  q / Esc "), "{diff}");
        let diff = body(HelpTopic::FileDiff, &remapped);
        assert!(!diff.contains("Alt+Down"), "{diff}");
        assert!(!diff.contains("q / "), "{diff}");
        assert!(
            diff.contains("\n  x              return to the Directory Tree"),
            "{diff}"
        );
        assert!(diff.contains("(returns here on x)"), "{diff}");

        assert!(body(HelpTopic::DirectoryTree, &defaults).contains("(= also expands)"));
        let tree = body(HelpTopic::DirectoryTree, &remapped);
        assert!(!tree.contains("= also expands"), "{tree}");
        assert!(tree.contains("- / *"), "{tree}");
    }
}
