# Inline File Diff

## Goal

Show the File Diff for the selected Directory Tree row in a focusable panel
below the two tree panes. This is the existing side-by-side File Diff, not a
separate read-only preview. See proposed [ADR-0008](docs/adr/0008-share-file-diff-across-presentations.md)
for the session and architecture decisions.

## Behavior

- [ ] Start with the inline panel disabled. Add a Command to toggle it; the
  Command has no default key binding and is available from the Command Palette.
- [ ] For a selected file row, load the Compared pair in the background. Show a
  placeholder while loading and a message when the file is unsupported. Do not
  show a stale diff for a different row. Hide the panel for directory rows.
- [ ] Use 50% of the screen height for the panel by default. Provide Commands
  to increase and decrease the height in 5-percentage-point steps, bounded to
  20–80%, with no default key bindings. Persist the chosen percentage. Each
  panel needs at least 5 rows; give the tree priority and hide the inline panel
  if both cannot fit. Never silently discard staged changes on resize.
- [ ] `[3]` focuses the inline File Diff; `[1]` and `[2]` focus the left and
  right tree panes. `Tab` cycles through all three when the panel is available.
  Input and Command availability follow focus. Moving focus alone does not
  close the File Diff session.
- [ ] When focused, provide the existing File Diff capabilities, including
  navigation, staging, undo, saving, copying, wrapping, context, and external
  tools, with the same safeguards and availability rules.
- [ ] Share the same `FileDiffSession` between inline and full-screen File
  Diff. Opening full-screen and returning to the panel preserves staged edits,
  undo history, and scroll position, subject to viewport normalization.
- [ ] Treat changing to another tree row, hiding the panel, or quitting as
  closing the inline session. If changes are staged, offer save, discard, or
  cancel before the transition. Cancel leaves the requested transition
  unapplied. A failed save must not discard the staged changes.
- [ ] If applying a background rescan would close a session with staged
  changes, offer save, discard, or cancel first. After a successful save or
  discard, close the session and reach the same UI state; only the saved file
  contents differ. `Esc` cancels the rescan transition and retains the session
  and edits. On a save conflict, offer cancellation of the transition or discard
  of the staged changes; never overwrite conflicting files. After saving, the
  tree must reflect the saved files rather than an older scan snapshot.
- [ ] Keep the session tied to its opened Compared pair until it is closed;
  tree Commands must not silently target another pair while staged edits are
  retained. A session started on two files continues to have no Directory Tree.

## Verification

- [ ] Test focus and input routing, including mouse and Command Palette paths.
- [ ] Test loading, unsupported files, directory selection, and superseded
  background loads.
- [ ] Test staged-change confirmation for selection, panel toggle, quit, and
  rescan, including cancellation, save failure, and save conflict.
- [ ] Test switching between inline and full-screen File Diff without losing
  staged changes or undo history; test resizing, the 5-row minimum, and
  persisted height limits.
- [ ] Update Help and `docs/SHORTCUTS.md` together, and document configuration
  and Command names. Verify the panel rendering and run `mise run check`.
