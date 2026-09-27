//! The Command Palette's state: whether it is open, the query, the Commands
//! it lists, and the selection among them.

/// The one Command Palette. `;`, `Ctrl+p`, and right-click all open this same
/// contextual surface — the former Menu / Command split is gone, along with its
/// single-character immediate execution and the `c`/`C` accelerator ambiguity
/// (Issue #239).
#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    visible: bool,
    query: String,
    items: Vec<crate::commands::CommandEntry>,
    selected_idx: usize,
    /// First item row painted in the list viewport, so a selection past the
    /// bottom of a long inventory stays visible.
    scroll_offset: usize,
}

impl PaletteState {
    pub(crate) fn visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn items(&self) -> &[crate::commands::CommandEntry] {
        &self.items
    }

    pub(crate) fn selected_idx(&self) -> usize {
        self.selected_idx
    }

    pub(crate) fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Show the palette with an empty query. The caller fills the inventory,
    /// which only [`App`](super::App) can compute — see [`App::open_palette`](super::App::open_palette).
    pub(crate) fn open(&mut self) {
        self.visible = true;
        self.query.clear();
    }

    /// Dismiss the palette: hidden, query cleared.
    pub(crate) fn close(&mut self) {
        self.visible = false;
        self.query.clear();
    }

    /// Replace the inventory, keeping the selection inside it.
    pub(crate) fn set_items(&mut self, items: Vec<crate::commands::CommandEntry>) {
        self.items = items;
        if self.selected_idx >= self.items.len() {
            self.selected_idx = self.items.len().saturating_sub(1);
        }
    }

    /// Wrap-around next over the inventory (no-op when empty).
    pub(crate) fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected_idx = (self.selected_idx + 1) % self.items.len();
    }

    /// Wrap-around previous over the inventory (no-op when empty).
    pub(crate) fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected_idx = self
            .selected_idx
            .checked_sub(1)
            .unwrap_or(self.items.len() - 1);
    }

    /// Move the selection to the first enabled Command, or to `0` when the
    /// inventory is empty or entirely unavailable.
    pub(crate) fn select_first_enabled(&mut self) {
        self.selected_idx = self.items.iter().position(|a| a.enabled()).unwrap_or(0);
        self.scroll_offset = 0;
    }

    /// Keep the selection inside a `visible_rows`-tall list viewport. Called
    /// once per frame from the render shell, which is the only place that knows
    /// the popup's clamped height.
    pub(crate) fn sync_viewport(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            self.scroll_offset = 0;
            return;
        }
        let max_offset = self.items.len().saturating_sub(visible_rows);
        if self.selected_idx < self.scroll_offset {
            self.scroll_offset = self.selected_idx;
        } else if self.selected_idx >= self.scroll_offset + visible_rows {
            self.scroll_offset = self.selected_idx + 1 - visible_rows;
        }
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Append one character to the query. Re-filtering is the caller's job:
    /// only [`App`](super::App) can rebuild the inventory — see [`App::palette_type_char`](super::App::palette_type_char).
    pub(crate) fn push_query_char(&mut self, c: char) {
        self.query.push(c);
    }

    /// Drop the query's trailing character. Re-filtering is the caller's job.
    pub(crate) fn pop_query_char(&mut self) {
        self.query.pop();
    }

    // Test-only field setter, same role as `DirectoryTreeState`'s. Clippy's dead-code
    // pass flags it as unreachable outside `#[cfg(test)]` call sites, so it
    // needs an explicit `#[allow]` (ADR-0002).
    #[allow(dead_code)]
    pub(crate) fn set_selected_idx(&mut self, idx: usize) {
        self.selected_idx = idx;
    }
}
