# Share File Diff across inline and full-screen presentations

**Status**: proposed

## Context

The Directory Tree needs an inline File Diff that can stage and save changes,
not a read-only preview. Keeping a separate preview and full-screen session
would duplicate loading and editing state, or require staged changes and undo
history to be transferred between them.

## Decision

- The inline panel and full-screen File Diff present the same
  `FileDiffSession` (ADR-0007). Switching between them does not close or reload
  the session; staged changes, undo history, and scroll position carry across.
  Viewport changes may require scroll normalization.
- Changing the Directory Tree selection to a different row, hiding the panel,
  or quitting closes the inline session. If it has staged changes, use the
  existing save/discard/cancel protection before closing it. Cancel leaves the
  intended change unapplied. Moving focus alone does not close the session.
- A directory row has no inline panel. A file row opens through File Diff's
  background load (ADR-0007); while loading, the panel shows a placeholder.
  An unsupported file shows a message in the panel. A failed inline open does
  not navigate away from the Directory Tree; the existing full-screen failure
  behavior remains unchanged.
- A background rescan must not silently retarget or discard staged changes.
  If applying its result would close a dirty session, ask whether to save or
  discard first. After either successful choice, close the session and proceed
  to the same resulting UI state; only the saved file contents differ. Esc
  cancels the rescan transition and keeps the session and its edits. If saving
  conflicts, offer cancellation of the transition or discard of the staged
  changes; do not overwrite the conflicting files. Other save failures also
  keep the session and its edits. The session remains tied to the pair it
  opened until the choice is resolved (ADR-0004 and ADR-0007).

## Consequences

- The Directory Tree remains the Screen while the panel is inline; opening the
  full-screen File Diff is a Screen transition, not a second session. Back from
  the full-screen presentation returns to the inline one without closing it.
- Focus determines whether tree or File Diff input applies. Command inventory,
  availability, execution, and confirmations remain centralized (ADR-0003),
  including confirmation before a selection gesture replaces a dirty session.
- View assembly, shared layout and hit testing, and pure painting remain at
  their existing seams (ADR-0001 and ADR-0002). Frame preparation does not
  initiate loads or navigation. File Diff continues to own its viewport
  geometry (ADR-0005).
- After a save, the tree must reflect the saved files rather than an older scan
  snapshot, while reaching the same UI state as the discard path.
- Sessions started on two files still have no Directory Tree (ADR-0004).
