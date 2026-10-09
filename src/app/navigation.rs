//! Which Screen (`GLOSSARY.md`) is shown, and where Back leads from it.

use super::ViewMode;

/// The Screens opened on the way to the current one, oldest first. The last is
/// shown; Back closes it and shows the one below. A Screen appears once: opening
/// one already on the way back returns to it rather than stacking it again, so
/// Back always walks out.
#[derive(Clone, Debug)]
pub(crate) struct Navigation {
    /// Never empty.
    screens: Vec<ViewMode>,
}

impl Navigation {
    /// A session that starts on `screen`.
    pub(crate) fn starting_on(screen: ViewMode) -> Self {
        Self {
            screens: vec![screen],
        }
    }

    /// The Screen shown.
    pub(crate) fn current(&self) -> ViewMode {
        *self.screens.last().expect("navigation is never empty")
    }

    /// Show `screen`, with Back returning to the current one. When `screen` is
    /// already on the way back, return to it instead. Whether the Screen shown
    /// changed.
    pub(crate) fn open(&mut self, screen: ViewMode) -> bool {
        if self.current() == screen {
            return false;
        }
        match self.screens.iter().position(|&open| open == screen) {
            Some(index) => self.screens.truncate(index + 1),
            None => self.screens.push(screen),
        }
        true
    }

    /// The Screen Back returns to, if any.
    pub(crate) fn below(&self) -> Option<ViewMode> {
        self.screens
            .len()
            .checked_sub(2)
            .map(|index| self.screens[index])
    }

    /// Take `screen` off the way back, showing the one below it if it was
    /// shown — for a Screen that could not open. The last Screen stays.
    pub(crate) fn remove(&mut self, screen: ViewMode) {
        if self.screens.len() > 1 {
            self.screens.retain(|&open| open != screen);
        }
    }

    /// Close the current Screen and show the one below. `false`, changing
    /// nothing, when it is the last one: Back from there ends the session.
    pub(crate) fn back(&mut self) -> bool {
        if self.screens.len() == 1 {
            return false;
        }
        self.screens.pop();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ViewMode::{ConfigMenu, DirectoryTree, FileDiff, Help};

    fn walk_back(mut navigation: Navigation) -> Vec<ViewMode> {
        let mut shown = vec![navigation.current()];
        while navigation.back() {
            shown.push(navigation.current());
        }
        shown
    }

    #[test]
    fn back_retraces_the_screens_opened() {
        let mut navigation = Navigation::starting_on(DirectoryTree);
        assert!(navigation.open(FileDiff));
        assert!(navigation.open(ConfigMenu));
        assert!(navigation.open(Help));
        assert_eq!(
            walk_back(navigation),
            vec![Help, ConfigMenu, FileDiff, DirectoryTree]
        );
    }

    #[test]
    fn reopening_a_screen_on_the_way_back_returns_to_it() {
        let mut navigation = Navigation::starting_on(DirectoryTree);
        navigation.open(ConfigMenu);
        navigation.open(Help);
        assert!(navigation.open(ConfigMenu));
        assert_eq!(walk_back(navigation), vec![ConfigMenu, DirectoryTree]);
    }

    #[test]
    fn a_screen_removed_from_the_way_back_is_skipped() {
        let mut navigation = Navigation::starting_on(DirectoryTree);
        navigation.open(FileDiff);
        navigation.open(Help);
        navigation.remove(FileDiff);
        assert_eq!(walk_back(navigation), vec![Help, DirectoryTree]);

        let mut navigation = Navigation::starting_on(DirectoryTree);
        navigation.open(FileDiff);
        navigation.remove(FileDiff);
        assert_eq!(
            navigation.current(),
            DirectoryTree,
            "the screen below shows"
        );
    }

    #[test]
    fn below_is_the_screen_back_returns_to() {
        let mut navigation = Navigation::starting_on(FileDiff);
        assert_eq!(navigation.below(), None);
        navigation.open(Help);
        assert_eq!(navigation.below(), Some(FileDiff));
    }

    #[test]
    fn back_from_the_only_screen_changes_nothing() {
        let mut navigation = Navigation::starting_on(FileDiff);
        assert!(!navigation.back(), "the caller ends the session");
        assert_eq!(navigation.current(), FileDiff);
    }

    #[test]
    fn opening_the_screen_shown_changes_nothing() {
        let mut navigation = Navigation::starting_on(DirectoryTree);
        navigation.open(Help);
        assert!(!navigation.open(Help));
        assert_eq!(walk_back(navigation), vec![Help, DirectoryTree]);
    }
}
