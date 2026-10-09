//! The Help screen's state: the open topic, the topic index, and how far
//! the topic is scrolled.

use super::ViewMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTopic {
    DirectoryTree,
    FileDiff,
    Config,
    Mouse,
    General,
    About,
}

impl HelpTopic {
    /// All topics in index / quick-jump order (`1`-`6` map to these positions).
    pub fn all() -> [HelpTopic; 6] {
        use HelpTopic::*;
        [DirectoryTree, FileDiff, Config, Mouse, General, About]
    }

    /// Short title shown in the index list and the topic-view block title.
    pub fn title(self) -> &'static str {
        match self {
            HelpTopic::DirectoryTree => "Directory Tree",
            HelpTopic::FileDiff => "File Diff",
            HelpTopic::Config => "Config",
            HelpTopic::Mouse => "Mouse",
            HelpTopic::General => "General",
            HelpTopic::About => "About",
        }
    }

    /// The topic to open when `?` is pressed from a given `ViewMode`. `Mouse` and
    /// `General` have no view that maps to them directly; they're reached only via
    /// the index list or a direct number-key jump from within Help.
    pub fn for_view(view: ViewMode) -> HelpTopic {
        match view {
            ViewMode::DirectoryTree => HelpTopic::DirectoryTree,
            ViewMode::FileDiff => HelpTopic::FileDiff,
            ViewMode::ConfigMenu => HelpTopic::Config,
            ViewMode::Help => HelpTopic::General,
        }
    }
}

/// The Help screen's own state: active topic, the topic index overlay, and the
/// view to restore on close. Owned by [`App::help`](super::App::help)/[`App::help_mut`](super::App::help_mut); production
/// code reaches it only through [`App::open_help`](super::App::open_help)/[`App::close_help`](super::App::close_help) (which also
/// move between Screens, which `App` owns) plus the methods here.
#[derive(Clone, Copy, Debug)]
pub struct HelpState {
    topic: HelpTopic,
    index_open: bool,
    index_sel: usize,
    scroll: u16,
    /// The furthest the topic scrolls: its line count less the rows the
    /// last frame showed, so the last line stops at the bottom.
    max_scroll: u16,
}

impl Default for HelpState {
    fn default() -> Self {
        Self {
            topic: HelpTopic::General,
            index_open: false,
            index_sel: 0,
            scroll: 0,
            max_scroll: 0,
        }
    }
}

impl HelpState {
    /// Enter Help on `topic`: sync the index cursor to it, close the index, and
    /// reset scroll. Called by [`App::open_help`](super::App::open_help) with the contextual topic for
    /// the Screen below Help.
    pub(crate) fn enter(&mut self, topic: HelpTopic) {
        self.topic = topic;
        self.index_sel = HelpTopic::all()
            .iter()
            .position(|&t| t == topic)
            .unwrap_or(0);
        self.index_open = false;
        self.scroll = 0;
    }

    /// Leave Help: force `index_open = false`. Unifies the body-Esc and
    /// index-Esc paths (body already has the index closed; setting it again is a
    /// no-op UX-wise). Going back stays on [`App::close_help`](super::App::close_help).
    pub(crate) fn leave(&mut self) {
        self.index_open = false;
    }

    /// Set the active topic body, close the index if open, and reset scroll to 0.
    /// Shared by Enter (index), digit keys (index or body), and mouse topic click.
    pub(crate) fn select_topic(&mut self, topic: HelpTopic) {
        self.topic = topic;
        self.index_open = false;
        self.scroll = 0;
    }

    /// Select the topic at `idx` in `HelpTopic::all()`, if in range. Shared by the
    /// digit-key shortcut, Enter-on-index, and mouse click-on-index-row — the one
    /// deep entry point for resolving a raw index to a topic.
    pub(crate) fn select_topic_by_index(&mut self, idx: usize) -> bool {
        match HelpTopic::all().get(idx) {
            Some(&topic) => {
                self.select_topic(topic);
                true
            }
            None => false,
        }
    }

    /// Open the topic index, syncing `index_sel` to the current topic (Tab).
    pub(crate) fn open_index(&mut self) {
        self.index_sel = HelpTopic::all()
            .iter()
            .position(|&t| t == self.topic)
            .unwrap_or(0);
        self.index_open = true;
    }

    /// Close the topic index only (`index_open = false`), stay on Help.
    /// Symmetric with `open_index`; production Esc uses [`App::close_help`](super::App::close_help) instead.
    /// Currently exercised only by tests — no key/mouse path closes just the index today.
    #[allow(dead_code)]
    pub(crate) fn close_index(&mut self) {
        self.index_open = false;
    }

    /// Wrap-around next over `HelpTopic::all()` for the index cursor.
    /// Shared by keyboard j/k (index mode) and mouse scroll (index mode).
    pub(crate) fn index_select_next(&mut self) {
        self.index_sel = (self.index_sel + 1) % HelpTopic::all().len();
    }

    /// Wrap-around prev over `HelpTopic::all()` for the index cursor.
    /// Shared by keyboard j/k (index mode) and mouse scroll (index mode).
    pub(crate) fn index_select_prev(&mut self) {
        self.index_sel = self
            .index_sel
            .checked_sub(1)
            .unwrap_or(HelpTopic::all().len() - 1);
    }

    /// Scroll the topic body down by one row. Shared by keyboard j/k (body mode)
    /// and mouse scroll (body mode).
    pub(crate) fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1).min(self.max_scroll);
    }

    /// Take the frame's topic size — the rows it shows and the lines the
    /// topic has — and keep scroll inside it.
    pub(crate) fn set_frame(&mut self, visible_rows: usize, lines: usize) {
        self.max_scroll = u16::try_from(lines.saturating_sub(visible_rows)).unwrap_or(u16::MAX);
        self.scroll = self.scroll.min(self.max_scroll);
    }

    /// Scroll the topic body up by one row (saturating). Shared by keyboard j/k
    /// (body mode) and mouse scroll (body mode).
    pub(crate) fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Move down: index-select-next if the topic index is open, else scroll the
    /// body down. Owns the mode branch so callers (keyboard j/k·Down, mouse
    /// scroll) don't have to re-derive it.
    pub(crate) fn move_down(&mut self) {
        if self.index_open {
            self.index_select_next();
        } else {
            self.scroll_down();
        }
    }

    /// Move up: index-select-prev if the topic index is open, else scroll the
    /// body up. Symmetric with [`HelpState::move_down`].
    pub(crate) fn move_up(&mut self) {
        if self.index_open {
            self.index_select_prev();
        } else {
            self.scroll_up();
        }
    }

    /// The active Help topic body. Read access for `draw_help` / tests.
    pub(crate) fn topic(&self) -> HelpTopic {
        self.topic
    }

    /// Whether the topic index (vs. the topic body) is showing. Read access for
    /// `draw_help` / tests.
    pub(crate) fn index_open(&self) -> bool {
        self.index_open
    }

    /// The index cursor's current position. Read access for `draw_help` / tests.
    pub(crate) fn index_sel(&self) -> usize {
        self.index_sel
    }

    /// The topic body's vertical scroll offset. Read access for `draw_help` / tests.
    pub(crate) fn scroll(&self) -> u16 {
        self.scroll
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_topic(&mut self, topic: HelpTopic) {
        self.topic = topic;
    }

    #[allow(dead_code)]
    pub(crate) fn set_index_open(&mut self, open: bool) {
        self.index_open = open;
    }

    #[allow(dead_code)]
    pub(crate) fn set_index_sel(&mut self, idx: usize) {
        self.index_sel = idx;
    }

    #[allow(dead_code)]
    pub(crate) fn set_scroll(&mut self, scroll: u16) {
        self.scroll = scroll;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Help's topic stops scrolling when its last line reaches the bottom,
    /// and a taller frame pulls a scroll past that back.
    #[test]
    fn help_scrolls_no_further_than_its_last_line() {
        let mut help = HelpState::default();
        help.set_frame(10, 13);
        for _ in 0..10 {
            help.scroll_down();
        }
        assert_eq!(help.scroll(), 3);

        help.set_frame(12, 13);
        assert_eq!(help.scroll(), 1);

        help.set_frame(20, 13);
        help.scroll_down();
        assert_eq!(help.scroll(), 0, "a topic that fits does not scroll");
    }

    #[test]
    fn test_help_topic_all_returns_six_topics_in_order() {
        use HelpTopic::*;
        assert_eq!(
            HelpTopic::all(),
            [DirectoryTree, FileDiff, Config, Mouse, General, About]
        );
    }

    #[test]
    fn test_help_topic_titles_are_distinct_non_empty() {
        let titles: Vec<&str> = HelpTopic::all().iter().map(|t| t.title()).collect();
        for title in &titles {
            assert!(!title.is_empty());
        }
        let mut unique = titles.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), titles.len(), "topic titles must be distinct");
    }

    #[test]
    fn test_help_topic_for_view_maps_each_view_correctly() {
        assert_eq!(
            HelpTopic::for_view(ViewMode::DirectoryTree),
            HelpTopic::DirectoryTree
        );
        assert_eq!(HelpTopic::for_view(ViewMode::FileDiff), HelpTopic::FileDiff);
        assert_eq!(HelpTopic::for_view(ViewMode::ConfigMenu), HelpTopic::Config);
    }

    #[test]
    fn test_help_state_has_expected_defaults() {
        let help = HelpState::default();
        assert_eq!(help.topic(), HelpTopic::General);
        assert!(!help.index_open());
        assert_eq!(help.index_sel(), 0);
        assert_eq!(help.scroll(), 0);
    }
}
