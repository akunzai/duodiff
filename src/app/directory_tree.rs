//! The Directory Tree (`CONTEXT.md`): the two roots' aligned tree, the
//! user's expand state, the rows it lists, the filter, and the cursor.

use crate::diff::{AlignedNode, DiffState, FileInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct FlatRow {
    pub depth: usize,
    pub relative_path: PathBuf,
    pub name: String,
    pub left_name_raw: Option<String>,
    pub right_name_raw: Option<String>,
    pub left_relative_path_raw: Option<PathBuf>,
    pub right_relative_path_raw: Option<PathBuf>,
    pub state: DiffState,
    pub left: Option<FileInfo>,
    pub right: Option<FileInfo>,
    /// Expand state of the underlying node, mirrored so the renderer can draw
    /// the directory disclosure marker without walking the tree.
    pub is_expanded: bool,
    pub has_case_conflict: bool,
    pub contains_case_conflict: bool,
    pub is_ambiguous_case_collision: bool,
}

impl Default for FlatRow {
    fn default() -> Self {
        Self {
            depth: 0,
            relative_path: PathBuf::new(),
            name: String::new(),
            left_name_raw: None,
            right_name_raw: None,
            left_relative_path_raw: None,
            right_relative_path_raw: None,
            state: DiffState::Identical,
            left: None,
            right: None,
            is_expanded: false,
            has_case_conflict: false,
            contains_case_conflict: false,
            is_ambiguous_case_collision: false,
        }
    }
}

impl FlatRow {
    /// The entry on `side`, when that side has one.
    pub(crate) fn side(&self, side: crate::side::Side) -> &Option<FileInfo> {
        match side {
            crate::side::Side::Left => &self.left,
            crate::side::Side::Right => &self.right,
        }
    }

    /// Whether either side of this row is a directory.
    pub(crate) fn is_dir(&self) -> bool {
        self.left.as_ref().map(|f| f.is_dir).unwrap_or(false)
            || self.right.as_ref().map(|f| f.is_dir).unwrap_or(false)
    }

    /// Return the left-specific relative path, falling back to the canonical path.
    pub fn left_relative_path(&self) -> &Path {
        self.left_relative_path_raw
            .as_deref()
            .unwrap_or(&self.relative_path)
    }

    /// Return the right-specific relative path, falling back to the canonical path.
    pub fn right_relative_path(&self) -> &Path {
        self.right_relative_path_raw
            .as_deref()
            .unwrap_or(&self.relative_path)
    }

    /// Return the left-specific basename, falling back to the canonical name.
    pub fn left_name(&self) -> &str {
        self.left_name_raw.as_deref().unwrap_or(&self.name)
    }

    /// Return the right-specific basename, falling back to the canonical name.
    pub fn right_name(&self) -> &str {
        self.right_name_raw.as_deref().unwrap_or(&self.name)
    }
}

/// The Directory Tree: the two roots' aligned tree, the expand state the user
/// chose, the rows it flattens to, the filter over those rows, and the cursor
/// into what is listed. Owned by [`App::directory_tree`](super::App::directory_tree) /
/// [`App::directory_tree_mut`](super::App::directory_tree_mut).
///
/// Every mutating method leaves the tree, the rows, and the cursor consistent
/// before it returns, so no caller has to reflatten or refilter (ADR-0005).
/// The scan that produces the tree is [`crate::scan::ScanState`]'s; this type only adopts
/// its result.
#[derive(Clone, Debug, Default)]
pub struct DirectoryTreeState {
    root_node: Option<AlignedNode>,
    /// Cached leaf-pair inventory for the tree footer (Issue #252).
    /// Recomputed whenever the tree changes, not while drawing.
    tree_summary: Option<crate::diff::TreeSummary>,
    /// Every directory's expand state, keyed by relative path — the user's
    /// choice, kept apart from the scan's output so a rescan cannot lose it.
    /// Holds exactly the directories below the root of the current tree.
    expanded: HashMap<PathBuf, bool>,
    /// Rows a test lists without building a tree (ADR-0002); production
    /// always lists from the tree.
    #[cfg(test)]
    seed_rows: Vec<FlatRow>,
    active: bool,
    input: crate::text_input::TextInput,
    pattern: String,
    /// Committed diffs-only flag — the one the listed rows apply.
    diffs_only: bool,
    /// The editing session's diffs-only value. Mirrors the typed text: it only
    /// updates the badge until Enter commits both together, and Esc restores it
    /// alongside the query (Issue #236).
    draft_diffs_only: bool,
    /// The rows the user sees: the tree, flattened through the expand state or
    /// walked whole through the filter. The only copy of them.
    rows: Vec<FlatRow>,
    /// Cursor into `rows` (Issue #309).
    selected_idx: usize,
    /// First row painted in the list viewport.
    scroll_offset: usize,
    /// Content rows the last frame showed, set by [`crate::view::prepare_frame`].
    visible_height: usize,
    last_click_idx: Option<usize>,
    last_click_time: Option<std::time::Instant>,
}

impl DirectoryTreeState {
    /// The directory-tree selection cursor.
    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    /// The list's vertical scroll offset.
    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// The row under the cursor, if any.
    pub(crate) fn selected_row(&self) -> Option<&FlatRow> {
        self.rows.get(self.selected_idx)
    }

    /// Put the cursor back at the top, as a swap or an emptied list does.
    pub(crate) fn reset_cursor(&mut self) {
        self.selected_idx = 0;
        self.scroll_offset = 0;
    }

    pub(crate) fn select_next(&mut self) {
        if !self.rows.is_empty() && self.selected_idx < self.rows.len() - 1 {
            self.selected_idx += 1;
        }
    }

    pub(crate) fn select_prev(&mut self) {
        if !self.rows.is_empty() && self.selected_idx > 0 {
            self.selected_idx -= 1;
        }
    }

    /// Select row `idx` if in range. Used by mouse left/right click. Does not
    /// change scroll by itself (matches the mouse path; the frame and keyboard
    /// page paths still call [`DirectoryTreeState::adjust_scroll`]).
    pub(crate) fn select_row_at(&mut self, idx: usize) -> bool {
        if idx >= self.rows.len() {
            return false;
        }
        self.selected_idx = idx;
        true
    }

    /// Page size for `Ctrl+f` / `Ctrl+b`: the last drawn height, with a
    /// one-row overlap when possible so context isn't completely lost.
    fn page_step(&self) -> usize {
        self.visible_height.saturating_sub(1).max(1)
    }

    /// Move the cursor down by one page, then keep it on screen.
    pub(crate) fn page_down(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let max_idx = self.rows.len() - 1;
        self.selected_idx = (self.selected_idx + self.page_step()).min(max_idx);
        self.adjust_scroll(self.visible_height);
    }

    /// Move the cursor up by one page, then keep it on screen.
    pub(crate) fn page_up(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        self.selected_idx = self.selected_idx.saturating_sub(self.page_step());
        self.adjust_scroll(self.visible_height);
    }

    /// Content rows the last frame showed.
    pub(crate) fn visible_height(&self) -> usize {
        self.visible_height
    }

    /// Record the frame's list height and keep the cursor inside it.
    pub(crate) fn set_visible_height(&mut self, visible_height: usize) {
        self.visible_height = visible_height;
        self.adjust_scroll(visible_height);
    }

    /// Scroll the minimum needed to keep the cursor inside a `visible_height`
    /// -tall viewport.
    pub(crate) fn adjust_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected_idx - visible_height + 1;
        }
    }

    /// Restore the cursor onto `path` after a recompute, keeping the previous
    /// scroll where the row is still on screen. A path a collapse hid falls
    /// back to its nearest listed ancestor. Returns false when neither is
    /// listed, leaving the cursor at the top.
    pub(crate) fn restore_cursor(
        &mut self,
        path: Option<&std::path::Path>,
        prev_scroll: usize,
        visible_height: usize,
    ) -> bool {
        let found = path.and_then(|path| {
            path.ancestors()
                .take_while(|candidate| !candidate.as_os_str().is_empty())
                .find_map(|candidate| {
                    self.rows.iter().position(|r| {
                        r.relative_path == candidate
                            || r.left_relative_path_raw.as_deref() == Some(candidate)
                            || r.right_relative_path_raw.as_deref() == Some(candidate)
                    })
                })
        });
        match found {
            Some(idx) => {
                self.selected_idx = idx;
                let max_scroll = self.rows.len().saturating_sub(1);
                self.scroll_offset = prev_scroll.min(max_scroll);
                self.adjust_scroll(visible_height);
                true
            }
            None => {
                self.reset_cursor();
                false
            }
        }
    }

    /// True while the filter input bar is open and routing key events.
    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// The committed filter pattern (set on Enter/Esc), lowercase-matched
    /// against row names and paths.
    pub(crate) fn pattern(&self) -> &str {
        &self.pattern
    }

    /// True when the row list should exclude [`DiffState::Identical`] rows.
    pub(crate) fn diffs_only(&self) -> bool {
        self.diffs_only
    }

    /// The diffs-only value to show in the filter bar's badge: the editing
    /// session's draft while the bar is open, the committed flag otherwise.
    pub(crate) fn editing_diffs_only(&self) -> bool {
        if self.active {
            self.draft_diffs_only
        } else {
            self.diffs_only
        }
    }

    /// Flip the editing session's diffs-only flag. Like typed pattern text, this
    /// only reaches `rows` once the filter bar is committed via
    /// [`DirectoryTreeState::commit`].
    pub(crate) fn toggle_diffs_only(&mut self) {
        self.draft_diffs_only = !self.draft_diffs_only;
    }

    /// The filter input bar's text, for rendering.
    pub(crate) fn input(&self) -> &crate::text_input::TextInput {
        &self.input
    }

    /// Mutable access to the filter input bar's text, for key-by-key editing.
    pub(crate) fn input_mut(&mut self) -> &mut crate::text_input::TextInput {
        &mut self.input
    }

    /// The filtered tree rows currently shown in the directory tree. Read
    /// access for the tree render loop's full-list iteration.
    pub(crate) fn rows(&self) -> &[FlatRow] {
        &self.rows
    }

    /// Open the filter input bar, pre-filling with the committed pattern and
    /// diffs-only flag so Esc can restore both.
    pub(crate) fn open(&mut self) {
        self.active = true;
        self.input.set(self.pattern.clone());
        self.draft_diffs_only = self.diffs_only;
    }

    /// Close the filter input bar, committing the typed text and the drafted
    /// diffs-only flag together, and list the rows they keep.
    pub(crate) fn commit(&mut self) {
        self.active = false;
        self.pattern = self.input.to_string();
        self.diffs_only = self.draft_diffs_only;
        self.apply_filter();
    }

    /// Close the filter input bar, discarding any uncommitted typing and any
    /// diffs-only toggle made during the editing session.
    pub(crate) fn cancel(&mut self) {
        self.active = false;
        self.input.set(self.pattern.clone());
        self.draft_diffs_only = self.diffs_only;
    }

    /// Clear the filter entirely (pattern + diffs-only) and list every row.
    pub(crate) fn clear(&mut self) {
        self.pattern.clear();
        self.input.clear();
        self.diffs_only = false;
        self.draft_diffs_only = false;
        self.apply_filter();
    }

    /// List the rows the filter keeps, keeping the cursor on the same row
    /// where it survived the recompute.
    fn apply_filter(&mut self) {
        let prev_path = self.selected_row().map(|r| r.relative_path.clone());
        let prev_scroll = self.scroll_offset;
        self.recompute();
        self.restore_cursor(prev_path.as_deref(), prev_scroll, self.visible_height);
    }

    /// Relist the tree after it changed, and recount the footer summary.
    fn refresh(&mut self) {
        self.tree_summary = self
            .root_node
            .as_ref()
            .map(crate::diff::TreeSummary::from_root);
        // As before a tree existed: rows seeded without one do not survive a
        // tree change.
        #[cfg(test)]
        if self.root_node.is_none() {
            self.seed_rows.clear();
        }
        self.apply_filter();
    }

    pub(crate) fn root_node(&self) -> Option<&AlignedNode> {
        self.root_node.as_ref()
    }

    pub(crate) fn tree_summary(&self) -> Option<crate::diff::TreeSummary> {
        self.tree_summary
    }

    /// Every row the tree flattens to, before the filter applies.
    #[cfg(test)]
    pub(crate) fn flat_rows(&self) -> Vec<FlatRow> {
        match &self.root_node {
            Some(_) => self.flattened(),
            None => self.seed_rows.clone(),
        }
    }

    /// Adopt a finished scan's tree, keeping the expand state the previous
    /// tree carried.
    pub(crate) fn adopt(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.reconcile_expanded();
        self.refresh();
    }

    /// Graft a freshly rescanned subtree into the tree, or adopt it wholesale
    /// when there is no tree yet, keeping every directory's expand state.
    /// Returns false, changing nothing, when `path` is not in the tree.
    pub(crate) fn graft_subtree(&mut self, path: &Path, node: AlignedNode) -> bool {
        let grafted = match self.root_node.as_mut() {
            Some(root) => crate::diff::replace_subtree(root, path, node),
            None => {
                self.root_node = Some(node);
                true
            }
        };
        if grafted {
            self.reconcile_expanded();
            self.refresh();
        }
        grafted
    }

    /// Expand the selected directory.
    pub(crate) fn expand_selected(&mut self) {
        self.set_selected_expanded(true);
    }

    /// Collapse the selected directory. A selection it hides moves to the
    /// directory itself.
    pub(crate) fn collapse_selected(&mut self) {
        self.set_selected_expanded(false);
    }

    fn set_selected_expanded(&mut self, expanded: bool) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if !row.is_dir() {
            return;
        }
        let rel_path = row.relative_path.clone();
        self.set_expanded(&rel_path, expanded);
        self.refresh();
    }

    /// Expand (`true`) or collapse (`false`) every directory below the root,
    /// which stays open. A selection a collapse hides moves to its nearest
    /// listed ancestor.
    pub(crate) fn set_all_expanded(&mut self, expanded: bool) {
        for state in self.expanded.values_mut() {
            *state = expanded;
        }
        self.refresh();
    }

    /// Move the selection to the next difference (or the previous one when
    /// `forward` is false), wrapping around. Without a filter the jump expands
    /// the directories above the stop; with one it moves within the listed
    /// rows and leaves the expand state alone. Returns false when there is no
    /// stop.
    pub(crate) fn jump_to_difference(&mut self, forward: bool) -> bool {
        let current = self.selected_row().map(|r| r.relative_path.clone());
        let Some(target) = self.next_difference(current.as_deref(), forward) else {
            return false;
        };
        if !self.is_filtering() {
            for ancestor in target
                .ancestors()
                .skip(1)
                .take_while(|p| !p.as_os_str().is_empty())
            {
                self.set_expanded(ancestor, true);
            }
            self.refresh();
        }
        if let Some(idx) = self.rows.iter().position(|row| row.relative_path == target) {
            self.select_row_at(idx);
            self.adjust_scroll(self.visible_height);
        }
        true
    }

    /// Record a tree click at `idx` for double-click detection (400ms window).
    /// Returns `true` if this click is a double-click on the same index.
    pub(crate) fn note_click(&mut self, idx: usize) -> bool {
        let now = std::time::Instant::now();
        let is_double_click = Some(idx) == self.last_click_idx
            && self
                .last_click_time
                .is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(400));
        if is_double_click {
            self.last_click_idx = None;
            self.last_click_time = None;
        } else {
            self.last_click_idx = Some(idx);
            self.last_click_time = Some(now);
        }
        is_double_click
    }

    /// Rebuild `rows` from the tree through the expand state, or through the
    /// current pattern and diffs-only flag while a filter applies. Leaves the
    /// cursor to [`DirectoryTreeState::apply_filter`].
    fn recompute(&mut self) {
        let root = self.root_node.as_ref();
        let pattern = &self.pattern;
        let diffs_only = self.diffs_only;

        if pattern.is_empty() && !diffs_only {
            self.rows = match root {
                Some(_) => self.flattened(),
                None => self.unfiltered_without_tree(),
            };
            return;
        }

        if let Some(root_node) = root {
            let mut matches = Vec::new();
            collect_matching_rows(root_node, pattern, diffs_only, &self.expanded, &mut matches);
            self.rows = matches;
        } else {
            let norm_pattern = crate::diff::normalize_for_matching(pattern);
            let source = self.unfiltered_without_tree();
            self.rows = source
                .iter()
                .filter(|row| {
                    if diffs_only
                        && row.state == DiffState::Identical
                        && !row.has_case_conflict
                        && !row.contains_case_conflict
                        && !row.is_ambiguous_case_collision
                    {
                        return false;
                    }
                    if pattern.is_empty() {
                        return true;
                    }
                    let norm_name = crate::diff::normalize_for_matching(&row.name);
                    let norm_path =
                        crate::diff::normalize_for_matching(&row.relative_path.to_string_lossy());
                    let norm_l_name = row
                        .left_name_raw
                        .as_ref()
                        .map(|n| crate::diff::normalize_for_matching(n));
                    let norm_r_name = row
                        .right_name_raw
                        .as_ref()
                        .map(|n| crate::diff::normalize_for_matching(n));
                    let norm_l_path = row
                        .left_relative_path_raw
                        .as_ref()
                        .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));
                    let norm_r_path = row
                        .right_relative_path_raw
                        .as_ref()
                        .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));

                    norm_name.contains(&norm_pattern)
                        || norm_path.contains(&norm_pattern)
                        || norm_l_name
                            .as_ref()
                            .is_some_and(|n| n.contains(&norm_pattern))
                        || norm_r_name
                            .as_ref()
                            .is_some_and(|n| n.contains(&norm_pattern))
                        || norm_l_path
                            .as_ref()
                            .is_some_and(|p| p.contains(&norm_pattern))
                        || norm_r_path
                            .as_ref()
                            .is_some_and(|p| p.contains(&norm_pattern))
                })
                .cloned()
                .collect();
        }
    }

    /// Expand or collapse the directory at `path` without relisting, for
    /// tests that set up an expand state before a rescan.
    #[cfg(test)]
    pub(crate) fn set_expanded_for_test(&mut self, path: &Path, expanded: bool) {
        self.set_expanded(path, expanded);
    }

    // Test-only field setters, same role as `App`'s `set_view_mode`/`set_selected_idx`
    // helpers. Unlike those, clippy's dead-code pass flags these as unreachable
    // outside `#[cfg(test)]` call sites, so each needs an explicit `#[allow]`.
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }

    #[allow(dead_code)]
    pub(crate) fn set_scroll_offset(&mut self, offset: usize) {
        self.scroll_offset = offset;
    }

    #[allow(dead_code)]
    pub(crate) fn set_rows(&mut self, rows: Vec<FlatRow>) {
        self.rows = rows;
    }

    #[allow(dead_code)]
    pub(crate) fn set_pattern(&mut self, pattern: impl Into<String>) {
        self.pattern = pattern.into();
    }

    #[cfg(test)]
    pub(crate) fn set_diffs_only(&mut self, diffs_only: bool) {
        self.diffs_only = diffs_only;
    }
}

/// Whether an entry is a place the difference jumps stop (Issue #338): the
/// entries the diffs-only filter keeps, except a directory present on both
/// sides, whose `≠` only repeats what its children hold.
fn is_difference_stop(
    left: Option<&FileInfo>,
    right: Option<&FileInfo>,
    state: DiffState,
    has_case_conflict: bool,
    is_ambiguous_case_collision: bool,
) -> bool {
    let both_dirs = left.is_some_and(|f| f.is_dir) && right.is_some_and(|f| f.is_dir);
    has_case_conflict || is_ambiguous_case_collision || (!both_dirs && state.is_known_difference())
}

fn collect_matching_rows(
    root: &AlignedNode,
    pattern: &str,
    diffs_only: bool,
    expanded: &HashMap<PathBuf, bool>,
    out: &mut Vec<FlatRow>,
) {
    let norm_pattern = crate::diff::normalize_for_matching(pattern);
    for child in &root.children {
        collect_matching_rows_rec(child, pattern, &norm_pattern, diffs_only, expanded, out);
    }
}

/// Whether the filter lists `node`: it matches the pattern, and it is a
/// difference when only differences are listed. `norm_pattern` is `pattern`
/// normalized once by the caller.
fn node_matches_filter(
    node: &AlignedNode,
    pattern: &str,
    norm_pattern: &str,
    diffs_only: bool,
) -> bool {
    let diffs_match = if diffs_only {
        node.state.is_known_difference()
            || node.has_case_conflict
            || node.contains_case_conflict
            || node.is_ambiguous_case_collision
    } else {
        true
    };

    let text_match = if pattern.is_empty() {
        true
    } else {
        let norm_name = crate::diff::normalize_for_matching(&node.name);
        let norm_path = crate::diff::normalize_for_matching(&node.relative_path.to_string_lossy());
        let norm_l_name = node
            .left_name
            .as_ref()
            .map(|n| crate::diff::normalize_for_matching(n));
        let norm_r_name = node
            .right_name
            .as_ref()
            .map(|n| crate::diff::normalize_for_matching(n));
        let norm_l_path = node
            .left_relative_path
            .as_ref()
            .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));
        let norm_r_path = node
            .right_relative_path
            .as_ref()
            .map(|p| crate::diff::normalize_for_matching(&p.to_string_lossy()));

        norm_name.contains(norm_pattern)
            || norm_path.contains(norm_pattern)
            || norm_l_name
                .as_ref()
                .is_some_and(|n| n.contains(norm_pattern))
            || norm_r_name
                .as_ref()
                .is_some_and(|n| n.contains(norm_pattern))
            || norm_l_path
                .as_ref()
                .is_some_and(|p| p.contains(norm_pattern))
            || norm_r_path
                .as_ref()
                .is_some_and(|p| p.contains(norm_pattern))
    };

    diffs_match && text_match
}

fn collect_matching_rows_rec(
    node: &AlignedNode,
    pattern: &str,
    norm_pattern: &str,
    diffs_only: bool,
    expanded: &HashMap<PathBuf, bool>,
    out: &mut Vec<FlatRow>,
) {
    if node_matches_filter(node, pattern, norm_pattern, diffs_only) {
        out.push(FlatRow {
            depth: 0,
            relative_path: node.relative_path.clone(),
            name: node.name.clone(),
            left_name_raw: node.left_name.clone(),
            right_name_raw: node.right_name.clone(),
            left_relative_path_raw: node.left_relative_path.clone(),
            right_relative_path_raw: node.right_relative_path.clone(),
            state: node.state,
            left: node.left.clone(),
            right: node.right.clone(),
            is_expanded: expanded
                .get(&node.relative_path)
                .copied()
                .unwrap_or(node.expanded_by_default),
            has_case_conflict: node.has_case_conflict,
            contains_case_conflict: node.contains_case_conflict,
            is_ambiguous_case_collision: node.is_ambiguous_case_collision,
        });
    }

    for child in &node.children {
        collect_matching_rows_rec(child, pattern, norm_pattern, diffs_only, expanded, out);
    }
}

/// Tree walks behind [`DirectoryTreeState`]'s operations.
impl DirectoryTreeState {
    /// The tree's rows through the expand state, in display order.
    fn flattened(&self) -> Vec<FlatRow> {
        let mut rows = Vec::new();
        if let Some(root) = &self.root_node {
            for child in &root.children {
                self.flatten_node(child, 0, &mut rows);
            }
        }
        rows
    }

    /// The rows to list with no tree: none in production, a test's seed.
    fn unfiltered_without_tree(&self) -> Vec<FlatRow> {
        #[cfg(test)]
        return self.seed_rows.clone();
        #[cfg(not(test))]
        Vec::new()
    }

    fn flatten_node(&self, node: &AlignedNode, depth: usize, rows: &mut Vec<FlatRow>) {
        let is_expanded = self.is_expanded(node);
        rows.push(FlatRow {
            depth,
            relative_path: node.relative_path.clone(),
            name: node.name.clone(),
            left_name_raw: node.left_name.clone(),
            right_name_raw: node.right_name.clone(),
            left_relative_path_raw: node.left_relative_path.clone(),
            right_relative_path_raw: node.right_relative_path.clone(),
            state: node.state,
            left: node.left.clone(),
            right: node.right.clone(),
            is_expanded,
            has_case_conflict: node.has_case_conflict,
            contains_case_conflict: node.contains_case_conflict,
            is_ambiguous_case_collision: node.is_ambiguous_case_collision,
        });
        if is_expanded {
            for child in &node.children {
                self.flatten_node(child, depth + 1, rows);
            }
        }
    }

    /// Whether `node` is shown open: the user's choice for a directory, the
    /// scanner's default for anything the map does not hold.
    fn is_expanded(&self, node: &AlignedNode) -> bool {
        self.expanded
            .get(&node.relative_path)
            .copied()
            .unwrap_or(node.expanded_by_default)
    }

    /// Make `expanded` hold exactly the current tree's directories: one it
    /// already knew keeps the user's choice, a new one takes the scanner's
    /// default, and one no longer in the tree is forgotten.
    fn reconcile_expanded(&mut self) {
        fn walk(
            node: &AlignedNode,
            previous: &HashMap<PathBuf, bool>,
            next: &mut HashMap<PathBuf, bool>,
        ) {
            if DirectoryTreeState::is_dir_node(node) {
                let expanded = previous
                    .get(&node.relative_path)
                    .copied()
                    .unwrap_or(node.expanded_by_default);
                next.insert(node.relative_path.clone(), expanded);
            }
            for child in &node.children {
                walk(child, previous, next);
            }
        }
        let previous = std::mem::take(&mut self.expanded);
        if let Some(root) = &self.root_node {
            for child in &root.children {
                walk(child, &previous, &mut self.expanded);
            }
        }
    }

    /// Whether a filter applies, listing its matches instead of the tree.
    fn is_filtering(&self) -> bool {
        !self.pattern.is_empty() || self.diffs_only
    }

    /// Visit every entry in tree order with whether the jumps stop there: a
    /// difference, not inside a one-sided directory or a type conflict (that
    /// entry is the difference as a whole), and listed by any filter. The one
    /// definition both the jumps and the Palette's gate read. `visit` returns
    /// false to stop the walk early.
    fn walk_difference_stops(&self, mut visit: impl FnMut(&AlignedNode, bool) -> bool) {
        fn walk(
            tree: &DirectoryTreeState,
            filter: Option<(&str, &str)>,
            node: &AlignedNode,
            inside_whole: bool,
            visit: &mut dyn FnMut(&AlignedNode, bool) -> bool,
        ) -> bool {
            let listed = filter.is_none_or(|(pattern, norm_pattern)| {
                node_matches_filter(node, pattern, norm_pattern, tree.diffs_only)
            });
            let stop = listed && !inside_whole && DirectoryTreeState::is_stop_node(node);
            if !visit(node, stop) {
                return false;
            }
            let whole = inside_whole || !DirectoryTreeState::is_both_dirs(node);
            node.children
                .iter()
                .all(|child| walk(tree, filter, child, whole, visit))
        }
        let norm_pattern = crate::diff::normalize_for_matching(&self.pattern);
        let filter = self
            .is_filtering()
            .then_some((self.pattern.as_str(), norm_pattern.as_str()));
        if let Some(root) = &self.root_node {
            for child in &root.children {
                if !walk(self, filter, child, false, &mut visit) {
                    return;
                }
            }
        }
    }

    /// The next difference stop after `from` in tree order — or before it,
    /// when `forward` is false — wrapping around, whatever is expanded. With
    /// no `from`, the search starts from the top.
    fn next_difference(&self, from: Option<&Path>, forward: bool) -> Option<PathBuf> {
        let mut order = Vec::new();
        self.walk_difference_stops(|node, stop| {
            order.push((node.relative_path.clone(), stop));
            true
        });
        let len = order.len();
        let start = from.and_then(|path| order.iter().position(|(p, _)| p == path));
        (1..=len)
            .map(|step| match (start, forward) {
                (Some(i), true) => (i + step) % len,
                (Some(i), false) => (i + len - step) % len,
                (None, true) => step - 1,
                (None, false) => len - step,
            })
            .find(|&i| order[i].1)
            .map(|i| order[i].0.clone())
    }

    /// Whether a jump would find a stop, so the Palette can gate the jumps.
    /// Stops at the first one.
    pub(crate) fn has_difference(&self) -> bool {
        let mut found = false;
        self.walk_difference_stops(|_, stop| {
            found = stop;
            !stop
        });
        found
    }

    fn is_both_dirs(node: &AlignedNode) -> bool {
        node.left.as_ref().is_some_and(|f| f.is_dir)
            && node.right.as_ref().is_some_and(|f| f.is_dir)
    }

    fn is_stop_node(node: &AlignedNode) -> bool {
        is_difference_stop(
            node.left.as_ref(),
            node.right.as_ref(),
            node.state,
            node.has_case_conflict,
            node.is_ambiguous_case_collision,
        )
    }

    /// Expand or collapse the directory at `path`, leaving the rows to the
    /// caller's [`DirectoryTreeState::refresh`].
    fn set_expanded(&mut self, path: &Path, expanded: bool) {
        if let Some(state) = self.expanded.get_mut(path) {
            *state = expanded;
        }
    }

    fn is_dir_node(node: &AlignedNode) -> bool {
        node.left.as_ref().is_some_and(|f| f.is_dir)
            || node.right.as_ref().is_some_and(|f| f.is_dir)
    }

    // Test-only field setters (ADR-0002). Clippy's dead-code pass flags these
    // as unreachable outside `#[cfg(test)]` call sites, so each needs an
    // explicit `#[allow]`.
    #[allow(dead_code)]
    /// Install `node` with its own expand flags, as a test's tree literal
    /// spells them — unlike [`DirectoryTreeState::adopt`], which keeps the
    /// user's choices from the previous tree.
    pub(crate) fn set_root_node(&mut self, node: AlignedNode) {
        self.root_node = Some(node);
        self.expanded.clear();
        self.reconcile_expanded();
    }

    #[cfg(test)]
    pub(crate) fn set_flat_rows(&mut self, rows: Vec<FlatRow>) {
        self.seed_rows = rows;
    }

    #[cfg(test)]
    pub(crate) fn push_flat_row(&mut self, row: FlatRow) {
        self.seed_rows.push(row);
    }

    /// Reflatten and relist after a test installs a tree or rows directly.
    #[allow(dead_code)]
    pub(crate) fn refresh_for_test(&mut self, reflatten: bool) {
        if reflatten {
            self.refresh();
        } else {
            self.apply_filter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{
        file_info, flat_row, flat_row_with_sides, listed_paths, nested_tree, selected_path,
        tree_entry, tree_root, tree_with_differences,
    };
    use std::time::SystemTime;

    /// Install a tree and flatten it, as a finished scan would.
    fn install(tree: &mut DirectoryTreeState, node: AlignedNode) {
        tree.set_root_node(node);
        tree.refresh_for_test(true);
    }

    #[test]
    fn test_flat_row_is_dir_true_when_either_side_is_a_directory() {
        assert!(flat_row_with_sides(Some(file_info(true)), Some(file_info(true))).is_dir());
        assert!(flat_row_with_sides(Some(file_info(true)), Some(file_info(false))).is_dir());
        assert!(flat_row_with_sides(None, Some(file_info(true))).is_dir());
        assert!(flat_row_with_sides(Some(file_info(true)), None).is_dir());
    }

    #[test]
    fn test_flat_row_is_dir_false_when_both_sides_are_files_or_missing() {
        assert!(!flat_row_with_sides(Some(file_info(false)), Some(file_info(false))).is_dir());
        assert!(!flat_row_with_sides(None, Some(file_info(false))).is_dir());
        assert!(!flat_row_with_sides(Some(file_info(false)), None).is_dir());
        assert!(!flat_row_with_sides(None, None).is_dir());
    }

    #[test]
    fn test_flatten_tree() {
        let mut tree = DirectoryTreeState::default();
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![
                AlignedNode {
                    name: "top_dir".to_string(),
                    relative_path: PathBuf::from("top_dir"),
                    left: Some(FileInfo {
                        is_dir: true,
                        size: 0,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![AlignedNode {
                        name: "nested.txt".to_string(),
                        relative_path: PathBuf::from("top_dir/nested.txt"),
                        left: Some(FileInfo {
                            is_dir: false,
                            size: 10,
                            modified: SystemTime::UNIX_EPOCH,
                        }),
                        right: None,
                        state: DiffState::LeftOnly,
                        children: vec![],
                        expanded_by_default: false,
                        ..Default::default()
                    }],
                    expanded_by_default: true,
                    ..Default::default()
                },
                AlignedNode {
                    name: "top_file.txt".to_string(),
                    relative_path: PathBuf::from("top_file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);

        // Synthetic root is hidden; top-level entries start at depth 0
        assert_eq!(tree.flat_rows().len(), 3, "Expected 3 flattened rows");
        assert_eq!(tree.flat_rows()[0].name, "top_dir");
        assert_eq!(
            tree.flat_rows()[0].depth,
            0,
            "Top-level directory depth should be 0"
        );
        assert_eq!(tree.flat_rows()[1].name, "nested.txt");
        assert_eq!(tree.flat_rows()[1].depth, 1, "Child depth should be 1");
        assert_eq!(tree.flat_rows()[2].name, "top_file.txt");
        assert_eq!(
            tree.flat_rows()[2].depth,
            0,
            "Top-level file depth should be 0"
        );
    }

    #[test]
    fn test_select_next_prev() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from(""),
                name: "root".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 1,
                relative_path: PathBuf::from("child"),
                name: "child".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        tree.refresh_for_test(false);

        assert_eq!(tree.selected_idx(), 0);
        tree.select_next();
        assert_eq!(tree.selected_idx(), 1);
        tree.select_next();
        assert_eq!(tree.selected_idx(), 1); // bounds check
        tree.select_prev();
        assert_eq!(tree.selected_idx(), 0);
        tree.select_prev();
        assert_eq!(tree.selected_idx(), 0); // bounds check
    }

    #[test]
    fn test_page_down_up_moves_by_visible_height() {
        let mut tree = DirectoryTreeState::default();
        let rows: Vec<FlatRow> = (0..20)
            .map(|i| FlatRow {
                depth: 0,
                relative_path: PathBuf::from(format!("f{i}.txt")),
                name: format!("f{i}.txt"),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            })
            .collect();
        tree.set_flat_rows(rows);
        tree.refresh_for_test(false);
        tree.visible_height = 5; // page_step = 4

        tree.page_down();
        assert_eq!(tree.selected_idx(), 4);
        assert_eq!(tree.scroll_offset(), 0); // still visible within first page

        tree.page_down();
        assert_eq!(tree.selected_idx(), 8);
        assert_eq!(tree.scroll_offset(), 4); // selection pushed view down

        tree.page_up();
        assert_eq!(tree.selected_idx(), 4);

        // Overshoot clamps to last row
        tree.set_selected_idx(18);
        tree.page_down();
        assert_eq!(tree.selected_idx(), 19);

        tree.page_up();
        assert_eq!(tree.selected_idx(), 15);

        // Empty list is a no-op
        tree.set_rows(Vec::new());
        tree.set_selected_idx(0);
        tree.page_down();
        tree.page_up();
        assert_eq!(tree.selected_idx(), 0);
    }

    #[test]
    fn test_expand_and_collapse_selected_directory_updates_the_flat_rows() {
        let mut tree = DirectoryTreeState::default();
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "dir".to_string(),
                relative_path: PathBuf::from("dir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "child.txt".to_string(),
                    relative_path: PathBuf::from("dir/child.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);

        assert_eq!(tree.flat_rows().len(), 2);
        assert_eq!(tree.flat_rows()[0].name, "dir");
        assert_eq!(tree.flat_rows()[1].name, "child.txt");

        // select dir and collapse it
        tree.set_selected_idx(0);
        tree.collapse_selected();

        // dir should now be collapsed, so only dir in flat_rows
        assert_eq!(tree.flat_rows().len(), 1);
        assert_eq!(tree.flat_rows()[0].name, "dir");

        tree.expand_selected();
        assert_eq!(tree.flat_rows().len(), 2);
    }

    #[test]
    fn test_expand_collapse_selected() {
        let mut tree = DirectoryTreeState::default();
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "dir".to_string(),
                relative_path: PathBuf::from("dir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "child.txt".to_string(),
                    relative_path: PathBuf::from("dir/child.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);

        assert_eq!(tree.flat_rows().len(), 2);

        // collapse dir
        tree.set_selected_idx(0);
        tree.collapse_selected();
        assert_eq!(tree.flat_rows().len(), 1);

        // expand dir again
        tree.expand_selected();
        assert_eq!(tree.flat_rows().len(), 2);
    }

    #[test]
    fn test_adjust_scroll() {
        let mut tree = DirectoryTreeState::default();
        tree.set_scroll_offset(2);

        // 1. visible_height == 0 does nothing
        tree.set_selected_idx(5);
        tree.adjust_scroll(0);
        assert_eq!(tree.scroll_offset(), 2);

        // 2. selected_idx < scroll_offset -> scroll_offset becomes selected_idx
        tree.set_selected_idx(1);
        tree.adjust_scroll(5);
        assert_eq!(tree.scroll_offset(), 1);

        // 3. selected_idx >= scroll_offset + visible_height -> scroll_offset adjusts
        tree.set_selected_idx(7);
        tree.adjust_scroll(5);
        assert_eq!(tree.scroll_offset(), 3);

        // 4. selected_idx within view (e.g. 5) -> scroll_offset stays same
        tree.set_selected_idx(5);
        tree.adjust_scroll(5);
        assert_eq!(tree.scroll_offset(), 3);
    }

    #[test]
    fn test_filter_by_pattern() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("alpha.txt"),
                name: "alpha.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("beta.txt"),
                name: "beta.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("gamma.txt"),
                name: "gamma.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        tree.refresh_for_test(false);
        assert_eq!(tree.rows().len(), 3);

        // Filter by "alpha"
        tree.set_pattern("alpha");
        tree.refresh_for_test(false);
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].name, "alpha.txt");

        // Clear filter
        tree.set_pattern("");
        tree.refresh_for_test(false);
        assert_eq!(tree.rows().len(), 3);
    }

    #[test]
    fn test_filter_diffs_only() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("only.txt"),
                name: "only.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        assert_eq!(tree.rows().len(), 2);
        assert!(tree.rows().iter().all(|r| r.state != DiffState::Identical));
    }

    /// Issue #232: `≈` rows are unresolved, not identical — diffs-only must keep
    /// them so switching to Precise mode from the filtered view is possible.
    #[test]
    fn test_filter_diffs_only_retains_unverified_rows() {
        use crate::diff::UnverifiedReason;

        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("image.png"),
                name: "image.png".to_string(),
                state: DiffState::Unverified(UnverifiedReason::NotCompared),
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].name, "image.png");
    }

    #[test]
    fn test_filter_pattern_and_diffs_only_combined() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_a.txt"),
                name: "diff_a.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff_b.txt"),
                name: "diff_b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);

        // Filter by "a" + diffs only → should match "diff_a.txt" only
        tree.set_pattern("a");
        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].name, "diff_a.txt");
    }

    #[test]
    fn test_filter_case_insensitive() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![FlatRow {
            depth: 0,
            relative_path: PathBuf::from("README.md"),
            name: "README.md".to_string(),
            state: DiffState::Identical,
            left: None,
            right: None,
            ..Default::default()
        }]);
        tree.set_pattern("readme");
        tree.refresh_for_test(false);
        assert_eq!(tree.rows().len(), 1);
    }

    #[test]
    fn test_apply_filter_preserves_selection_and_scroll() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::LeftOnly,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("c.txt"),
                name: "c.txt".to_string(),
                state: DiffState::RightOnly,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        tree.refresh_for_test(false);
        tree.set_selected_idx(2);
        tree.set_scroll_offset(1);
        tree.visible_height = 10;

        // Rebuild without changing filter criteria — keep the same row selected.
        tree.refresh_for_test(false);
        assert_eq!(tree.selected_idx(), 2);
        assert_eq!(
            tree.rows()[tree.selected_idx()].relative_path,
            PathBuf::from("c.txt")
        );
        assert_eq!(tree.scroll_offset(), 1);
    }

    #[test]
    fn test_apply_filter_resets_when_selection_filtered_out() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("same.txt"),
                name: "same.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("diff.txt"),
                name: "diff.txt".to_string(),
                state: DiffState::DifferentNewerLeft,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        tree.refresh_for_test(false);
        tree.set_selected_idx(0); // same.txt
        tree.set_scroll_offset(0);

        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        // same.txt is filtered out → fall back to top of remaining list
        assert_eq!(tree.selected_idx(), 0);
        assert_eq!(tree.rows()[0].name, "diff.txt");
        assert_eq!(tree.scroll_offset(), 0);
    }

    #[test]
    fn test_flatten_tree_preserves_selection() {
        let mut tree = DirectoryTreeState::default();
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![
                AlignedNode {
                    name: "child_a".to_string(),
                    relative_path: PathBuf::from("child_a"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
                AlignedNode {
                    name: "child_b".to_string(),
                    relative_path: PathBuf::from("child_b"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);
        tree.set_selected_idx(1); // child_b
        tree.set_scroll_offset(1);
        tree.visible_height = 10;

        tree.refresh_for_test(true);
        assert_eq!(tree.selected_idx(), 1);
        assert_eq!(tree.flat_rows()[tree.selected_idx()].name, "child_b");
        assert_eq!(tree.scroll_offset(), 1);
    }

    #[test]
    fn test_restore_expanded_paths_after_rescan() {
        let mut tree = DirectoryTreeState::default();
        let old_tree = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "subdir".to_string(),
                relative_path: PathBuf::from("subdir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "file.txt".to_string(),
                    relative_path: PathBuf::from("subdir/file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: true,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, old_tree);
        let idx = tree
            .rows()
            .iter()
            .position(|r| r.relative_path == *"subdir/file.txt")
            .unwrap();
        tree.set_selected_idx(idx);

        let expand_states = &tree.expanded;
        assert!(!expand_states.contains_key(&PathBuf::from("")));
        assert_eq!(expand_states.get(&PathBuf::from("subdir")), Some(&true));

        // Simulate a fresh scan result that brings the directory back collapsed.
        let new_tree = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "subdir".to_string(),
                relative_path: PathBuf::from("subdir"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "file.txt".to_string(),
                    relative_path: PathBuf::from("subdir/file.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 5,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: false,
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        tree.adopt(new_tree);

        assert!(tree
            .rows()
            .iter()
            .any(|r| r.relative_path == *"subdir/file.txt"));
        assert_eq!(
            tree.rows()[tree.selected_idx()].relative_path,
            PathBuf::from("subdir/file.txt")
        );
    }

    #[test]
    fn test_open_commit_cancel_filter() {
        let mut tree = DirectoryTreeState::default();
        tree.set_pattern("abc");

        // DirectoryTreeState::open pre-fills input with committed pattern
        tree.open();
        assert!(tree.active());
        assert_eq!(tree.input(), "abc");

        // Type more
        for c in "def".chars() {
            tree.input_mut().insert(c);
        }
        assert_eq!(tree.input(), "abcdef");

        // Cancel restores to original pattern
        tree.cancel();
        assert!(!tree.active());
        assert_eq!(tree.input(), "abc");
        assert_eq!(tree.pattern(), "abc");

        // Open again and commit
        tree.open();
        tree.input_mut().set("xyz");
        tree.commit();
        assert!(!tree.active());
        assert_eq!(tree.pattern(), "xyz");
    }

    #[test]
    fn test_clear_filter() {
        let mut tree = DirectoryTreeState::default();
        tree.set_flat_rows(vec![
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("a.txt"),
                name: "a.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
            FlatRow {
                depth: 0,
                relative_path: PathBuf::from("b.txt"),
                name: "b.txt".to_string(),
                state: DiffState::Identical,
                left: None,
                right: None,
                ..Default::default()
            },
        ]);
        tree.set_pattern("a");
        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        assert_eq!(tree.rows().len(), 0);

        tree.clear();
        assert!(tree.pattern().is_empty());
        assert!(!tree.diffs_only());
        assert_eq!(tree.rows().len(), 2);
    }

    #[test]
    fn test_filter_rows_accessor_reflects_set_rows() {
        let mut tree = DirectoryTreeState::default();
        assert!(tree.rows().is_empty());

        let rows = vec![flat_row("a.txt"), flat_row("b.txt")];
        tree.set_rows(rows.clone());

        assert_eq!(tree.rows().len(), 2);
        assert_eq!(tree.rows()[0].name, "a.txt");
        assert_eq!(tree.rows()[1].name, "b.txt");
    }

    /// Issue #236: the diffs-only toggle is drafted like the typed query — the
    /// badge updates immediately, but only Enter commits it, and Esc restores
    /// the value from before the editing session.
    #[test]
    fn test_diffs_only_is_drafted_until_commit_and_restored_on_cancel() {
        let mut tree = DirectoryTreeState::default();
        assert!(!tree.diffs_only());

        tree.open();
        tree.toggle_diffs_only();
        assert!(
            tree.editing_diffs_only(),
            "the badge follows the draft straight away"
        );
        assert!(
            !tree.diffs_only(),
            "but the committed flag is untouched until Enter"
        );

        tree.commit();
        assert!(tree.diffs_only());
        assert!(tree.editing_diffs_only());

        // Toggling it back off and cancelling restores the committed value.
        tree.open();
        tree.toggle_diffs_only();
        assert!(!tree.editing_diffs_only());
        tree.cancel();
        assert!(tree.diffs_only());
        assert!(tree.editing_diffs_only());
    }

    #[test]
    fn test_filter_input_mut_allows_key_by_key_editing() {
        let mut tree = DirectoryTreeState::default();
        tree.input_mut().insert('a');
        tree.input_mut().insert('b');

        assert_eq!(tree.input(), "ab");
    }

    #[test]
    fn test_filter_searches_full_tree_ignoring_collapse_state() {
        let mut tree = DirectoryTreeState::default();
        let node = AlignedNode {
            name: String::new(),
            relative_path: PathBuf::from(""),
            left: Some(FileInfo {
                is_dir: true,
                size: 0,
                modified: SystemTime::UNIX_EPOCH,
            }),
            right: None,
            state: DiffState::LeftOnly,
            children: vec![AlignedNode {
                name: "collapsed_folder".to_string(),
                relative_path: PathBuf::from("collapsed_folder"),
                left: Some(FileInfo {
                    is_dir: true,
                    size: 0,
                    modified: SystemTime::UNIX_EPOCH,
                }),
                right: None,
                state: DiffState::LeftOnly,
                children: vec![AlignedNode {
                    name: "deep_target.txt".to_string(),
                    relative_path: PathBuf::from("collapsed_folder/deep_target.txt"),
                    left: Some(FileInfo {
                        is_dir: false,
                        size: 10,
                        modified: SystemTime::UNIX_EPOCH,
                    }),
                    right: None,
                    state: DiffState::LeftOnly,
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                }],
                expanded_by_default: false, // Collapsed in tree view!
                ..Default::default()
            }],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);
        // In tree view, collapsed_folder is collapsed so only 1 row visible
        assert_eq!(tree.flat_rows().len(), 1);

        // Filter for "target"
        tree.set_pattern("target");
        tree.refresh_for_test(false);
        // Even though parent was collapsed, full tree was searched and matching row is found!
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].name, "deep_target.txt");
        assert_eq!(
            tree.rows()[0].relative_path,
            PathBuf::from("collapsed_folder/deep_target.txt")
        );
        assert_eq!(tree.rows()[0].depth, 0); // Filter results are flat depth 0
    }

    #[test]
    fn test_filter_diffs_only_retains_content_identical_case_conflicts() {
        let mut tree = DirectoryTreeState::default();
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
                    name: "FILE.txt".to_string(),
                    left_name: Some("FILE.txt".to_string()),
                    right_name: Some("file.txt".to_string()),
                    relative_path: PathBuf::from("FILE.txt"),
                    left_relative_path: Some(PathBuf::from("FILE.txt")),
                    right_relative_path: Some(PathBuf::from("file.txt")),
                    has_case_conflict: true,
                    contains_case_conflict: true,
                    state: DiffState::Identical, // Content is identical!
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
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
                AlignedNode {
                    name: "regular.txt".to_string(),
                    relative_path: PathBuf::from("regular.txt"),
                    has_case_conflict: false,
                    state: DiffState::Identical,
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
                    children: vec![],
                    expanded_by_default: false,
                    ..Default::default()
                },
            ],
            expanded_by_default: true,
            ..Default::default()
        };
        install(&mut tree, node);

        // Turn on diffs-only filter
        tree.open();
        tree.toggle_diffs_only();
        tree.commit();
        tree.refresh_for_test(false);

        // Regular identical file is filtered out, but case-conflict identical file is retained!
        assert_eq!(tree.rows().len(), 1);
        assert_eq!(tree.rows()[0].name, "FILE.txt");
        assert!(tree.rows()[0].has_case_conflict);
    }

    /// Issue #338: collapse all leaves only the root's entries listed and keeps
    /// the cursor on the nearest ancestor the collapse left visible.
    #[test]
    fn collapse_all_lists_the_top_level_and_moves_the_cursor_to_an_ancestor() {
        let mut tree = DirectoryTreeState::default();
        install(&mut tree, nested_tree());
        let deep = listed_paths(&tree)
            .iter()
            .position(|path| path == "a/b/deep.txt")
            .unwrap();
        tree.set_selected_idx(deep);

        tree.set_all_expanded(false);

        assert_eq!(listed_paths(&tree), ["first.txt", "a", "top.txt"]);
        assert_eq!(selected_path(&tree), PathBuf::from("a"));

        tree.set_all_expanded(true);

        assert_eq!(
            listed_paths(&tree),
            ["first.txt", "a", "a/b", "a/b/deep.txt", "top.txt"]
        );
        assert_eq!(
            selected_path(&tree),
            PathBuf::from("a"),
            "expanding keeps the cursor where it was"
        );
    }

    /// Issue #338: expand all opens one-sided directories too, which the scanner
    /// otherwise returns collapsed.
    #[test]
    fn expand_all_opens_one_sided_directories() {
        let mut tree = DirectoryTreeState::default();
        let mut only_left = tree_entry(
            "gone",
            false,
            Some(vec![tree_entry("gone/x.txt", false, None)]),
        );
        only_left.right = None;
        only_left.state = DiffState::LeftOnly;
        install(&mut tree, tree_root(vec![only_left]));
        assert_eq!(listed_paths(&tree), ["gone"]);

        tree.set_all_expanded(true);

        assert_eq!(listed_paths(&tree), ["gone", "gone/x.txt"]);
    }

    /// Issue #338: the jumps walk the whole tree in order, stopping at the
    /// differences themselves — not the `≠` directories above them, not inside
    /// a one-sided directory, not at unverified rows — and wrap around.
    #[test]
    fn difference_jumps_stop_at_each_difference_and_expand_its_parents() {
        let mut tree = DirectoryTreeState::default();
        install(&mut tree, tree_with_differences());
        assert_eq!(
            listed_paths(&tree),
            ["first.txt", "a", "gone", "maybe.txt", "top.txt"]
        );

        assert!(tree.jump_to_difference(true));
        assert_eq!(selected_path(&tree), PathBuf::from("a/b/deep.txt"));
        assert_eq!(
            listed_paths(&tree),
            [
                "first.txt",
                "a",
                "a/b",
                "a/b/deep.txt",
                "gone",
                "maybe.txt",
                "top.txt"
            ],
            "the directories above the stop are expanded"
        );

        let mut forward = Vec::new();
        for _ in 0..3 {
            assert!(tree.jump_to_difference(true));
            forward.push(selected_path(&tree));
        }
        assert_eq!(
            forward,
            [
                PathBuf::from("gone"),
                PathBuf::from("top.txt"),
                PathBuf::from("a/b/deep.txt")
            ]
        );

        assert!(tree.jump_to_difference(false));
        assert_eq!(selected_path(&tree), PathBuf::from("top.txt"));
    }

    /// Issue #338: under a filter the jumps stay within the listed rows and
    /// leave the expand state alone.
    #[test]
    fn difference_jumps_stay_within_a_filtered_list() {
        let mut tree = DirectoryTreeState::default();
        install(&mut tree, tree_with_differences());
        tree.set_pattern("a/b");
        tree.refresh_for_test(false);
        assert_eq!(listed_paths(&tree), ["a/b", "a/b/deep.txt"]);

        assert!(tree.jump_to_difference(true));
        assert_eq!(selected_path(&tree), PathBuf::from("a/b/deep.txt"));
        assert!(
            !tree
                .flat_rows()
                .iter()
                .any(|row| row.relative_path == *"a/b"),
            "a filtered jump expands nothing"
        );

        tree.set_pattern("first");
        tree.refresh_for_test(false);
        assert!(
            !tree.jump_to_difference(true),
            "no stop among the listed rows"
        );
    }

    /// Under a filter the jumps stop where they stop without one: a one-sided
    /// directory is the difference as a whole, not each entry inside it.
    #[test]
    fn a_filtered_jump_skips_the_inside_of_a_one_sided_directory() {
        let mut tree = DirectoryTreeState::default();
        install(&mut tree, tree_with_differences());
        tree.set_diffs_only(true);
        tree.refresh_for_test(false);
        assert!(listed_paths(&tree).contains(&"gone/x.txt".to_string()));

        let mut stops = Vec::new();
        for _ in 0..3 {
            assert!(tree.jump_to_difference(true));
            stops.push(selected_path(&tree));
        }
        assert_eq!(
            stops,
            [
                PathBuf::from("a/b/deep.txt"),
                PathBuf::from("gone"),
                PathBuf::from("top.txt")
            ]
        );
    }

    /// The Palette offers the jumps only when one would move: under a filter,
    /// a difference the filter hides is not one to jump to.
    #[test]
    fn a_filter_listing_no_difference_has_none_to_jump_to() {
        let mut tree = DirectoryTreeState::default();
        install(&mut tree, tree_with_differences());
        assert!(tree.has_difference());

        tree.set_pattern("first");
        tree.refresh_for_test(false);
        assert!(!tree.has_difference());

        tree.set_pattern("gone");
        tree.refresh_for_test(false);
        assert!(tree.has_difference());
    }
}
