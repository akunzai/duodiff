# File Diff loads and saves the Compared pair in one place

**Status**: accepted

## Context

Opening a Directory Tree row in File Diff moved to a background load (#391)
so a slow or large file no longer froze the screen. Every other read stayed
synchronous on the UI thread: reloading after a save conflict and reloading
after a file-pair copy both called `App::refresh_file_diff`. The responsiveness
fix therefore covered one of the three ways File Diff reads files. Saving
staged edits sat on `App` and reached into `FileDiffState` for its buffers,
baselines, and hashes, so what a save checks and what a load replaces lived on
both sides of a seam.

## Decision

- `FileDiffSession` (`src/app/file_diff_session.rs`) owns File Diff
  (`GLOSSARY.md`): the content (`FileDiffState`), the load in flight, the row
  it was opened on, and the staged save with its conflict check.
- After startup, File Diff reads the Compared pair through one background
  load. Opening a row, reloading after a save conflict, and reloading after a
  file-pair copy all queue the same job. Only a file pair named on the command
  line is read synchronously, at startup, so content the built-in diff cannot
  show fails before the terminal is taken over (ADR-0004), and that read is
  the one File Diff opens on.
- A reload that fails, or that the user cancels with Back, leaves the
  content and its staged edits as they were. Only a failed open leaves File
  Diff.
- Saving stays synchronous: the confirmation that asked for it needs to know
  at once whether the files were written, conflicted, or failed.
- `App` keeps the file pair and its size and modification time (ADR-0004),
  hands the session the pair's paths, and decides where the screen goes when a
  load finishes. The session never touches `ViewMode`.

## Considered options

- **Keep `refresh_file_diff` for reloads**: rejected; a second load path is
  what let #391 miss two of the three reads.
- **Move loading and saving into `FileDiffState`**: rejected; ADR-0004 keeps
  `FileDiffState` about diff content, and a worker builds that content off the
  UI thread.

## Consequences

- New File Diff behaviour that reads or writes the pair goes through
  `FileDiffSession`; do not add a synchronous read on `App`.
- Tests that need File Diff open queue the load and finish it on the test
  thread (`App::finish_file_diff_load`).
