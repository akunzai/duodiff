//! File Diff's state: the rows of the built-in diff, the working buffers
//! staging edits, and the panes' geometry and scroll.

use crate::side::Side;

/// The file-diff content state: the built-in diff's rows, both scroll
/// offsets, the wrap/full-file toggles, and the cached hashes/line-endings
/// shown above the diff panes. Owned by [`App::diff`](super::App::diff)/[`App::diff_mut`](super::App::diff_mut).
///
/// Named `FileDiffState` rather than `DiffState` — [`crate::diff::DiffState`]
/// (the per-row Identical/LeftOnly/... status) already owns that name.
///
/// It also owns the panes' geometry — the frame's height and text width, and
/// the row counts they wrap to — so every scroll, jump, and stage reads the
/// same numbers the painter does, and each method leaves scroll inside them
/// (ADR-0005). [`crate::view::prepare_frame`] hands it the pane size once per
/// frame; `App` keeps only what needs I/O or settings: loading, saving, and
/// the file pair's paths.
#[derive(Clone, Debug, Default)]
pub struct FileDiffState {
    rows: Vec<crate::diff_view::DiffRow>,
    scroll: usize,
    h_scroll: usize,
    wrap: bool,
    show_full: bool,
    /// Unchanged lines kept around each change when not showing the full
    /// file. Every re-diff reads it here, so a new value reaches the rows
    /// through [`FileDiffState::set_context`] alone.
    context: usize,
    left_hash: Option<String>,
    right_hash: Option<String>,
    left_line_ending: Option<String>,
    right_line_ending: Option<String>,
    /// Working buffers the diff is computed from. `[` / `]` edit these; nothing
    /// reaches disk until an explicit save (Issue #235).
    left: crate::diff_view::TextBuffer,
    right: crate::diff_view::TextBuffer,
    /// The bytes each side had on disk when the session opened, or when the last
    /// save succeeded. A side is dirty exactly while it differs from its
    /// baseline, and the baseline is what a save checks the file against.
    left_baseline: crate::diff_view::TextBuffer,
    right_baseline: crate::diff_view::TextBuffer,
    /// Working-buffer snapshots taken before each staged hunk, newest last.
    undo_stack: Vec<(crate::diff_view::TextBuffer, crate::diff_view::TextBuffer)>,
    /// The row `N`/`P` last navigated to, independent of `scroll`.
    ///
    /// `scroll` doubles as the viewport's render offset, which `clamp_scroll`
    /// pulls back to `max_scroll` every frame — 0 whenever the diff
    /// already fits the viewport. Without this field, navigating to a hunk
    /// trailing near EOF (or any hunk `clamp_scroll` can't fully reach) would
    /// have that navigation silently undone before the next `[`/`]`, staging
    /// whatever hunk `scroll` was clamped back to instead — and a repeat
    /// `N`/`P` would recompute the same jump instead of advancing past it,
    /// since it would start over from the clamped position every time. A row
    /// (not a hunk index) so stepping between two change rows within one hunk
    /// still works, and not a physical offset so a resize that rewraps the
    /// rows above it keeps the same row. Cleared by manual scrolling or any
    /// re-diff, so it never resolves against stale rows; a stage then pins it
    /// to the next change block.
    nav_row: Option<usize>,
    /// Content rows visible in a pane (borders excluded), from the last frame.
    visible_height: usize,
    /// Text columns inside one pane (borders and gutter excluded), from the
    /// last frame. Zero leaves every line unwrapped, as `wrap::lines` paints it.
    content_width: usize,
    /// Where each row starts once wrapped at `content_width`, the hunks, and
    /// the longest line; rebuilt when the rows change or wrap differently.
    index: crate::diff_view::RowIndex,
    /// Lines in each file (working buffer, falling back to row metadata),
    /// counted when the rows change.
    line_counts: (usize, usize),
}

impl FileDiffState {
    /// An empty File Diff that diffs with `context` unchanged lines.
    pub(crate) fn with_context(context: usize) -> Self {
        Self {
            context,
            ..Self::default()
        }
    }

    /// Diff with `context` unchanged lines from now on, and re-diff the
    /// working buffers when that changes what the rows show.
    pub(crate) fn set_context(&mut self, context: usize) {
        if self.context == context {
            return;
        }
        self.context = context;
        self.recompute_rows();
        self.clamp_scroll();
    }

    /// Take the frame's pane size: rows inside the borders, and columns
    /// inside the borders, of which the gutter takes its share. Then keep
    /// scroll inside the content it now shows.
    pub(crate) fn set_frame(&mut self, visible_height: usize, pane_inner_width: usize) {
        self.visible_height = visible_height;
        self.content_width = crate::diff_view::diff_text_width(
            pane_inner_width,
            self.left_line_count(),
            self.right_line_count(),
        );
        self.sync_index();
        self.clamp_scroll();
    }

    /// Content rows visible in a pane, from the last frame.
    pub(crate) fn visible_height(&self) -> usize {
        self.visible_height
    }

    /// Text columns inside one pane, from the last frame.
    pub(crate) fn content_width(&self) -> usize {
        self.content_width
    }

    /// Physical (post-wrap) row count of the rows.
    #[cfg(test)]
    pub(crate) fn physical_rows(&self) -> usize {
        self.index.physical_rows()
    }

    /// Longest line (in characters) across the rows.
    #[cfg(test)]
    pub(crate) fn max_line_width(&self) -> usize {
        self.index.max_line_width()
    }

    /// Largest vertical scroll offset that still fills the panes.
    pub(crate) fn max_scroll(&self) -> usize {
        self.index
            .physical_rows()
            .saturating_sub(self.visible_height)
    }

    /// Largest horizontal scroll offset that keeps the longest line reachable.
    pub(crate) fn max_h_scroll(&self) -> usize {
        self.index
            .max_line_width()
            .saturating_sub(self.content_width)
    }

    /// Recount what the rows hold after they changed: each file's lines, and
    /// the index at the last frame's width.
    fn rows_changed(&mut self) {
        self.line_counts = (
            self.left
                .lines
                .len()
                .max(crate::diff_view::diff_side_line_count(
                    &self.rows,
                    Side::Left,
                )),
            self.right
                .lines
                .len()
                .max(crate::diff_view::diff_side_line_count(
                    &self.rows,
                    Side::Right,
                )),
        );
        self.index = crate::diff_view::RowIndex::new(&self.rows, self.content_width, self.wrap);
    }

    /// Rebuild the index when the rows now wrap differently from how it was
    /// built, keeping the view on the row `N`/`P` last navigated to.
    fn sync_index(&mut self) {
        if !self.index.is_for(self.content_width, self.wrap) {
            self.index = crate::diff_view::RowIndex::new(&self.rows, self.content_width, self.wrap);
            if let Some(row) = self.nav_row {
                self.scroll = self.index.start(row);
            }
        }
    }

    /// Page size for `Ctrl+f` / `Ctrl+b`: the last drawn height, with a
    /// one-row overlap when possible so context isn't completely lost.
    fn page_step(&self) -> usize {
        self.visible_height.saturating_sub(1).max(1)
    }
    /// The current file diff's rows. Read access for rendering.
    pub(crate) fn rows(&self) -> &[crate::diff_view::DiffRow] {
        &self.rows
    }

    /// The rows the last frame's panes show from `scroll`, and how many of the
    /// first one's wrapped rows are scrolled off the top.
    pub(crate) fn window(&self) -> (std::ops::Range<usize>, usize) {
        self.index.window(self.scroll, self.visible_height)
    }

    /// The change hunks, as ranges of rows.
    pub(crate) fn hunks(&self) -> &[std::ops::Range<usize>] {
        self.index.hunks()
    }

    /// Total left-file lines (working buffer, falling back to row metadata).
    pub(crate) fn left_line_count(&self) -> usize {
        self.line_counts.0
    }

    /// Total right-file lines (working buffer, falling back to row metadata).
    pub(crate) fn right_line_count(&self) -> usize {
        self.line_counts.1
    }

    /// True when the current file diff has at least one added/removed line.
    pub(crate) fn has_changes(&self) -> bool {
        !self.index.hunks().is_empty()
    }

    /// The file-diff view's vertical scroll offset.
    #[cfg(test)]
    pub(crate) fn scroll(&self) -> usize {
        self.scroll
    }

    /// The file-diff view's horizontal scroll offset (used when wrap is off).
    pub(crate) fn h_scroll(&self) -> usize {
        self.h_scroll
    }

    /// Whether long lines wrap in the file-diff view.
    pub(crate) fn wrap(&self) -> bool {
        self.wrap
    }

    /// Whether the file-diff view shows the full file rather than collapsed hunks.
    pub(crate) fn show_full(&self) -> bool {
        self.show_full
    }

    /// SHA-256 hash of the left side's file, if it loaded successfully.
    pub(crate) fn left_hash(&self) -> Option<&str> {
        self.left_hash.as_deref()
    }

    /// SHA-256 hash of the right side's file, if it loaded successfully.
    pub(crate) fn right_hash(&self) -> Option<&str> {
        self.right_hash.as_deref()
    }

    /// Detected line-ending style of the left side's file, if any.
    pub(crate) fn left_line_ending(&self) -> Option<&str> {
        self.left_line_ending.as_deref()
    }

    /// Detected line-ending style of the right side's file, if any.
    pub(crate) fn right_line_ending(&self) -> Option<&str> {
        self.right_line_ending.as_deref()
    }

    /// Replace both sides with freshly loaded content and recompute
    /// `rows`/hashes/line-endings. Loading can fail before this is called,
    /// which leaves `self` untouched.
    pub(crate) fn load(
        &mut self,
        left: crate::diff_view::LoadedText,
        right: crate::diff_view::LoadedText,
    ) {
        self.left = crate::diff_view::TextBuffer::from_text(&left.text);
        self.right = crate::diff_view::TextBuffer::from_text(&right.text);
        self.left_baseline = self.left.clone();
        self.right_baseline = self.right.clone();
        self.undo_stack.clear();
        self.left_hash = left.sha256;
        self.right_hash = right.sha256;
        self.left_line_ending = left.line_ending;
        self.right_line_ending = right.line_ending;
        self.recompute_rows();
    }

    /// Re-diff the working buffers. Every path that changes a buffer or the
    /// full-context flag ends here, so the rows always describe the staged
    /// bytes — and so `nav_row` never resolves against stale rows.
    pub(crate) fn recompute_rows(&mut self) {
        self.nav_row = None;
        self.rows = crate::diff_view::compare_texts(
            &self.left.to_text(),
            &self.right.to_text(),
            self.show_full,
            self.context,
        );
        self.rows_changed();
    }

    /// The left working buffer's staged bytes.
    pub(crate) fn left_buffer(&self) -> &crate::diff_view::TextBuffer {
        &self.left
    }

    /// The right working buffer's staged bytes.
    pub(crate) fn right_buffer(&self) -> &crate::diff_view::TextBuffer {
        &self.right
    }

    /// Whether the left side has staged, unsaved edits.
    pub(crate) fn left_dirty(&self) -> bool {
        self.left != self.left_baseline
    }

    /// Whether the right side has staged, unsaved edits.
    pub(crate) fn right_dirty(&self) -> bool {
        self.right != self.right_baseline
    }

    /// Whether either side has staged, unsaved edits.
    pub(crate) fn is_dirty(&self) -> bool {
        self.left_dirty() || self.right_dirty()
    }

    /// Whether there is a staged hunk operation left to undo.
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Stage one hunk copy into the working buffers and re-diff. Nothing is
    /// written; the previous buffers are pushed onto the undo stack first.
    /// Returns whether the hunk actually changed a buffer. `false` means the
    /// copy was a no-op — nothing is pushed onto the undo stack and the rows
    /// are not recomputed, so the caller can report "nothing to stage"
    /// instead of a save that never happened.
    pub(crate) fn stage_hunk(
        &mut self,
        hunk_index: usize,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        let snapshot = (self.left.clone(), self.right.clone());
        let rows = std::mem::take(&mut self.rows);
        let result = crate::diff_view::stage_hunk_copy(
            &mut self.left,
            &mut self.right,
            &rows,
            hunk_index,
            direction,
        );
        self.rows = rows;
        match result {
            Ok(changed) => {
                if changed {
                    self.undo_stack.push(snapshot);
                    self.recompute_rows();
                }
                Ok(changed)
            }
            Err(e) => {
                // Restore in case the splice ran partway.
                self.left = snapshot.0;
                self.right = snapshot.1;
                Err(e)
            }
        }
    }

    /// The rows of the change hunk under the cursor: the one `[` / `]` stage
    /// and the painter highlights.
    pub(crate) fn active_hunk_rows(&self) -> Option<std::ops::Range<usize>> {
        self.index.hunk_rows_at(self.cursor())
    }

    /// Index of the change hunk under the cursor, at the geometry painted.
    fn active_hunk(&self) -> Option<usize> {
        self.index.hunk_at(self.cursor())
    }

    /// The physical row the cursor is on: where `N`/`P` last navigated, as
    /// the rows wrap now, over `scroll`, which the per-frame clamp can pull
    /// away from a hunk trailing near EOF.
    fn cursor(&self) -> usize {
        self.nav_row
            .map_or(self.scroll, |row| self.index.start(row))
    }

    /// Stage the change hunk under the cursor in `direction`, then park the
    /// cursor on the next change block. Returns whether a buffer changed; an
    /// error when no change block is under the cursor.
    pub(crate) fn stage_active_hunk(
        &mut self,
        direction: crate::diff_view::HunkCopyDirection,
    ) -> Result<bool, std::io::Error> {
        let hunk_index = self.active_hunk().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no change block at cursor",
            )
        })?;
        let hunk_start_row = self.index.hunks()[hunk_index].start;
        let changed = self.stage_hunk(hunk_index, direction)?;
        if changed {
            self.select_hunk_after(hunk_start_row);
        }
        Ok(changed)
    }

    /// After a staged hunk lands, park the cursor on the next change block at or
    /// after where it was, falling back to the nearest valid position when the
    /// edit removed every later hunk (Issue #235).
    ///
    /// Pins `nav_row` to that next hunk's first row — not just `scroll` — for
    /// the same reason [`FileDiffState::jump_to_change`] does: a hunk trailing
    /// near EOF can sit past `max_scroll`, and `scroll` alone would lose track
    /// of it on the very next frame's clamp.
    fn select_hunk_after(&mut self, previous_row: usize) {
        let next = self
            .index
            .hunks()
            .iter()
            .find(|range| range.start >= previous_row)
            .map(|range| range.start);
        let max_scroll = self.max_scroll();
        match next {
            Some(row) => {
                self.scroll = self.index.start(row).min(max_scroll);
                self.nav_row = Some(row);
            }
            None => {
                self.scroll = self.scroll.min(max_scroll);
                self.nav_row = None;
            }
        }
    }

    /// Undo the most recent staged hunk operation. Returns false when there is
    /// nothing left to undo.
    pub(crate) fn undo_staged(&mut self) -> bool {
        let Some((left, right)) = self.undo_stack.pop() else {
            return false;
        };
        self.left = left;
        self.right = right;
        self.recompute_rows();
        self.clamp_scroll();
        true
    }

    /// Throw away every staged edit and go back to the session baseline.
    pub(crate) fn discard_staged(&mut self) {
        self.left = self.left_baseline.clone();
        self.right = self.right_baseline.clone();
        self.undo_stack.clear();
        self.recompute_rows();
        self.clamp_scroll();
    }

    /// The bytes the left side had at the session baseline.
    pub(crate) fn left_baseline_text(&self) -> String {
        self.left_baseline.to_text()
    }

    /// The bytes the right side had at the session baseline.
    pub(crate) fn right_baseline_text(&self) -> String {
        self.right_baseline.to_text()
    }

    /// Promote the working buffers to the new baseline after a successful save,
    /// clearing dirty and undo state.
    pub(crate) fn commit_baselines(
        &mut self,
        left_hash: Option<String>,
        right_hash: Option<String>,
    ) {
        self.left_baseline = self.left.clone();
        self.right_baseline = self.right.clone();
        self.undo_stack.clear();
        self.left_hash = left_hash;
        self.right_hash = right_hash;
    }

    /// Flip line wrapping and reset scroll, since the old scroll position no
    /// longer lines up once wrapping changes the layout.
    pub(crate) fn toggle_wrap(&mut self) {
        self.wrap = !self.wrap;
        self.sync_index();
        self.reset_scroll();
    }

    /// Flip full-file vs. diff-only content and re-diff the working buffers.
    /// Infallible: nothing is reloaded from disk, so a context toggle never
    /// throws staged edits away and has nothing to fail at (Issue #235).
    pub(crate) fn toggle_show_full(&mut self) {
        self.show_full = !self.show_full;
        self.recompute_rows();
        self.reset_scroll();
    }

    /// Set the full-file flag directly (vs. [`FileDiffState::toggle_show_full`]'s
    /// flip). Used by [`App::enter_file_diff`](super::App::enter_file_diff) to force diff-only mode before
    /// the first load, and by tests to seed a specific state.
    pub(crate) fn set_show_full(&mut self, on: bool) {
        self.show_full = on;
    }

    /// Line-step down, stopping at [`FileDiffState::max_scroll`]. Shared by
    /// keyboard j/Down and mouse scroll down. Manual movement overrides
    /// wherever `N`/`P` last pinned the cursor.
    pub(crate) fn scroll_down(&mut self) {
        self.nav_row = None;
        if self.scroll < self.max_scroll() {
            self.scroll += 1;
        }
    }

    /// Line-step up (no-op at the top). Shared by keyboard k/Up and mouse
    /// scroll up. See [`FileDiffState::scroll_down`] on `nav_row`.
    pub(crate) fn scroll_up(&mut self) {
        self.nav_row = None;
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Page down (`Ctrl+f`), stopping at [`FileDiffState::max_scroll`]. See
    /// [`FileDiffState::scroll_down`] on `nav_row`.
    pub(crate) fn page_down(&mut self) {
        self.nav_row = None;
        self.scroll = (self.scroll + self.page_step()).min(self.max_scroll());
    }

    /// Page up (`Ctrl+b`), no-op past the top. See
    /// [`FileDiffState::scroll_down`] on `nav_row`.
    pub(crate) fn page_up(&mut self) {
        self.nav_row = None;
        self.scroll = self.scroll.saturating_sub(self.page_step());
    }

    /// Horizontal step left, when wrap is off (no-op while wrapping or at the
    /// left edge).
    pub(crate) fn h_scroll_left(&mut self) {
        if !self.wrap && self.h_scroll > 0 {
            self.h_scroll -= 1;
        }
    }

    /// Horizontal step right, when wrap is off, stopping at
    /// [`FileDiffState::max_h_scroll`].
    pub(crate) fn h_scroll_right(&mut self) {
        if !self.wrap && self.h_scroll < self.max_h_scroll() {
            self.h_scroll += 1;
        }
    }

    /// Zero both scroll offsets. Used after wrap/full toggles and on entering
    /// a fresh file diff, where the old scroll position no longer applies.
    pub(crate) fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.h_scroll = 0;
        self.nav_row = None;
    }

    /// Pull both scroll offsets back inside the content. Growing the
    /// terminal (or opening a shorter file) can leave them past the end;
    /// without this the next page or arrow key would appear to jump
    /// backwards.
    pub(crate) fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
        self.h_scroll = self.h_scroll.min(self.max_h_scroll());
    }

    /// Reset scroll and clear cached hashes after [`App::swap_paths`](super::App::swap_paths) — rows
    /// and line-endings are left for the next `refresh_file_diff` to replace.
    pub(crate) fn reset_for_swap(&mut self) {
        self.scroll = 0;
        self.nav_row = None;
        self.left_hash = None;
        self.right_hash = None;
    }

    /// Jump to the next (`forward`) or previous differing block.
    ///
    /// Starts from `nav_row` rather than `scroll` when one is pinned. The
    /// per-frame clamp to `max_scroll` (0 once the diff already fits the
    /// viewport) pulls `scroll` back to whatever it can reach; computing the
    /// next jump from that clamped value would recompute the same target on a
    /// repeat `N`/`P` instead of advancing past it.
    ///
    /// Also pins `nav_row` to the target row in addition to setting
    /// `scroll`: `scroll` alone isn't enough when the target trails near EOF,
    /// since that same clamp would otherwise silently pull the next `[`/`]`
    /// back to whatever hunk `scroll` clamps to instead of the one just
    /// navigated to.
    pub(crate) fn jump_to_change(&mut self, forward: bool) {
        if let Some(scroll) = self.index.jump(self.cursor(), forward) {
            self.scroll = scroll;
            self.nav_row = Some(self.index.row_at(scroll));
        }
    }

    /// Set the vertical scroll offset directly, for tests to seed a position.
    #[cfg(test)]
    pub(crate) fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_rows(&mut self, rows: Vec<crate::diff_view::DiffRow>) {
        self.rows = rows;
        self.rows_changed();
    }

    #[allow(dead_code)]
    pub(crate) fn set_h_scroll(&mut self, scroll: usize) {
        self.h_scroll = scroll;
    }

    #[allow(dead_code)]
    pub(crate) fn set_wrap(&mut self, on: bool) {
        self.wrap = on;
        self.sync_index();
    }

    /// Take a frame by its text width rather than its pane width, as tests
    /// that pin the width a diff wraps at need: then resync and clamp, as
    /// [`FileDiffState::set_frame`] does.
    #[cfg(test)]
    pub(crate) fn set_text_frame(&mut self, visible_height: usize, content_width: usize) {
        self.visible_height = visible_height;
        self.content_width = content_width;
        self.sync_index();
        self.clamp_scroll();
    }

    /// Put a staged edit on the left side without going through a hunk copy,
    /// for tests that only need the diff to read as dirty.
    #[cfg(test)]
    pub(crate) fn stage_left_for_test(&mut self, staged: &str, baseline: &str) {
        self.left = crate::diff_view::TextBuffer::from_text(staged);
        self.left_baseline = crate::diff_view::TextBuffer::from_text(baseline);
        self.rows_changed();
    }

    #[allow(dead_code)]
    pub(crate) fn set_hashes(&mut self, left: Option<String>, right: Option<String>) {
        self.left_hash = left;
        self.right_hash = right;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{deleted_row, equal_row};

    #[test]
    fn test_diff_page_down_up() {
        let mut diff = FileDiffState::default();
        diff.set_rows((0..30).map(|_| equal_row("x")).collect());
        diff.set_text_frame(10, 0); // page_step = 9
        diff.set_scroll(0);

        diff.page_down();
        assert_eq!(diff.scroll(), 9);

        diff.page_down();
        assert_eq!(diff.scroll(), 18);

        // Clamp to max scroll (30 - 10 = 20)
        diff.page_down();
        assert_eq!(diff.scroll(), 20);

        diff.page_up();
        assert_eq!(diff.scroll(), 11);

        diff.set_scroll(3);
        diff.page_up();
        assert_eq!(diff.scroll(), 0);
    }

    #[test]
    fn test_jump_to_next_and_prev_change() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let mut diff = FileDiffState::default();
        diff.set_text_frame(0, 40);
        diff.set_rows(vec![
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "ctx".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Equal,
                    text: "ctx".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "old".to_string(),
                }),
                Some(DiffLine {
                    tag: ChangeTag::Insert,
                    text: "new".to_string(),
                }),
            )),
            DiffRow::from((
                Some(DiffLine {
                    tag: ChangeTag::Delete,
                    text: "bye".to_string(),
                }),
                None,
            )),
        ]);

        diff.jump_to_change(true);
        assert_eq!(diff.scroll(), 1);
        diff.jump_to_change(true);
        assert_eq!(diff.scroll(), 2);
        diff.jump_to_change(false);
        assert_eq!(diff.scroll(), 1);
    }

    /// A hunk reached with `N` stays the one under the cursor when the
    /// terminal is resized and the rows above it wrap to a different height.
    #[test]
    fn a_resize_keeps_the_hunk_navigated_to() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let line = |tag, text: &str| {
            Some(DiffLine {
                tag,
                text: text.to_string(),
            })
        };
        let context = "context that wraps onto several rows at a narrow width";
        let mut diff = FileDiffState::default();
        diff.set_wrap(true);
        diff.set_rows(vec![
            DiffRow::from((
                line(ChangeTag::Equal, context),
                line(ChangeTag::Equal, context),
            )),
            DiffRow::from((
                line(ChangeTag::Delete, "old a"),
                line(ChangeTag::Insert, "new a"),
            )),
            DiffRow::from((
                line(ChangeTag::Equal, context),
                line(ChangeTag::Equal, context),
            )),
            DiffRow::from((
                line(ChangeTag::Delete, "old b"),
                line(ChangeTag::Insert, "new b"),
            )),
        ]);
        diff.set_text_frame(2, 10);
        diff.jump_to_change(true);
        assert_eq!(diff.active_hunk_rows(), Some(1..2));

        // Unwrapped, each row takes one line: hunk A starts on the second.
        diff.set_text_frame(2, 80);
        assert_eq!(diff.active_hunk_rows(), Some(1..2));
        assert_eq!(diff.scroll(), 1, "the view follows the hunk");
        diff.jump_to_change(true);
        assert_eq!(diff.active_hunk_rows(), Some(3..4));
    }

    /// A pane narrower than its gutter leaves no text columns. The painter
    /// then shows every line unwrapped (`wrap::lines` at width 0), so the
    /// jumps must count rows the same way to land where the highlight is.
    #[test]
    fn a_zero_width_pane_jumps_to_the_row_it_paints() {
        use crate::diff_view::{DiffLine, DiffRow};
        use similar::ChangeTag;

        let mut diff = FileDiffState::default();
        diff.set_text_frame(0, 0);
        diff.set_wrap(true);
        let line = |tag, text: &str| {
            Some(DiffLine {
                tag,
                text: text.to_string(),
            })
        };
        let rows = vec![
            DiffRow::from((
                line(ChangeTag::Equal, "a long context line"),
                line(ChangeTag::Equal, "a long context line"),
            )),
            DiffRow::from((
                line(ChangeTag::Delete, "old"),
                line(ChangeTag::Insert, "new"),
            )),
        ];
        let painted = crate::diff_view::RowIndex::new(&rows, 0, true);
        diff.set_rows(rows);

        diff.jump_to_change(true);
        assert_eq!(diff.scroll(), painted.start(1));
    }

    #[test]
    fn test_diff_rows_accessor_reflects_set_rows() {
        let mut diff = FileDiffState::default();
        assert!(diff.rows().is_empty());

        let rows = vec![equal_row("a"), equal_row("b")];
        diff.set_rows(rows.clone());

        assert_eq!(diff.rows(), rows.as_slice());
    }

    #[test]
    fn test_diff_has_changes_false_when_all_rows_equal() {
        let mut diff = FileDiffState::default();
        diff.set_rows(vec![equal_row("a"), equal_row("b")]);

        assert!(!diff.has_changes());
    }

    #[test]
    fn test_diff_has_changes_true_when_a_row_differs() {
        let mut diff = FileDiffState::default();
        diff.set_rows(vec![equal_row("a"), deleted_row("b")]);

        assert!(diff.has_changes());
    }
}
