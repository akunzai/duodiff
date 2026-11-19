# Share File Diff across inline and full-screen presentations

**Status**: accepted

## Context

The Directory Tree is gaining an inline File Diff panel below its two panes.
A separate preview session would duplicate `FileDiffSession`'s background
load (ADR-0007) — one read of the pair for the panel, another for full-screen
File Diff on the same row.

## Decision

The inline panel and full-screen File Diff present the same
`FileDiffSession`: one background load per selected row, not one per
presentation.

## Consequences

- The Directory Tree remains the Screen while the panel is inline; opening
  full-screen File Diff is a Screen transition, not a second session.
- View assembly, shared layout and hit testing, and pure painting remain at
  their existing seams (ADR-0001 and ADR-0002). Frame preparation does not
  initiate loads.
- Sessions started on two files still have no Directory Tree (ADR-0004).
