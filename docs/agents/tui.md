# TUI Architecture and Invariants

duodiff uses crossterm, ratatui, and tokio. Its state and event loop live in `src/main.rs`, `src/app.rs`, `src/event.rs`, `src/input.rs`, and `src/actions.rs`; diffing lives in `src/diff.rs`, `src/diff_view.rs`, and `src/diff_tool.rs`; rendering and configuration live in `src/ui.rs`, `src/theme.rs`, `src/settings.rs`, and `src/text_input.rs`.

Use `App`, `FlatRow`, and `ViewMode` in `src/app.rs`, `DiffRow` in `src/diff_view.rs`, and `help_topic_body` in `src/ui.rs` as the primary code references.

## Runtime invariants

- **TTY recovery**: Leave raw mode and the alternate screen on every exit path; run the event loop only through `run_app`.
- **Editor handoff**: Leave the TUI before spawning an external diff tool or editor, then restore it immediately.
- **Flat-row render**: Draw from `app.flat_rows`; walking the tree on every frame becomes O(N²).
- **Diff-once**: Populate file-diff rows when entering `FileDiff`; keep file reads and diffing out of the draw loop.
- **Focus green**: The active pane border follows left/right focus through `focus_left_pane`, `focus_right_pane`, and `toggle_active_side`.
- **Modal capture**: While `confirm_modal().is_some()`, route all keyboard and mouse input to the modal; rescan after a confirmed copy.

## Architecture decisions

Read the relevant accepted ADR before changing `App` state shape or UI layout/draw seams, including simplify/ponytail cuts. Surface any conflict instead of silently reversing the decision:

- @docs/adr/0001-defer-app-ui-coupling-from-substate-split.md — App/UI View assembly coupling remains deferred.
- @docs/adr/0002-app-substate-and-view-dual-path.md — Private sub-state and domain methods remain; test fixtures and the View/LayoutInputs dual path remain.
