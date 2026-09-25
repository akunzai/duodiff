# File Diff reads its file pair from the comparison target, not the tree row

**Status**: accepted

## Context

duodiff started as a directory comparison tool, so every File Diff operation —
loading, staging, saving, copying, the external diff and editor, and the pane
titles — built its two paths by joining each root with the selected Directory
Tree row. Comparing two files named on the command line (#327) has no tree and
no row.

## Decision

- Startup resolves the two arguments into a comparison target: two
  directories, or one file pair. A file paired with a directory resolves to the
  same-named file inside it. Resolution also loads each file side once, so
  content the built-in diff cannot show fails before the terminal is taken over.
- A file side knows where its bytes come from: a regular file, the null device
  (`/dev/null` everywhere, `NUL` on Windows), or a pipe captured at startup. Only
  a regular file the user can write is writable. Writes and external tools use
  the side's target path — a symlink resolved to its file, the null device under
  the platform's name — while titles show the path as typed.
- `App` keeps the file pair when there is one, next to the two roots it
  replaces, together with each side's size and modification time so drawing
  never touches the filesystem. `FileDiffState` stays about diff content; the
  pair is session identity, like the roots. File Diff and the Commands that act
  on the current pair ask `App` for the two paths instead of reading the
  selected row, so directory sessions behave exactly as before.
- With a file pair, leaving File Diff quits, copy stays on File Diff and reloads
  the pair, and scan requests do nothing. Back and Quit remain distinct
  Commands (ADR-0003); only where Back leads changes.
- Command availability checks side writability, so the Command Palette carries
  the read-only reason and execution revalidates it (ADR-0003). The read-only
  mark reaches the painter through the View seam (ADR-0001).

## Considered options

- **A synthetic one-row tree** rooted at each file's parent directory was
  rejected: it breaks when the two file names differ, and has no parent for the
  null device or a pipe.
- **A separate screen** for file pairs was rejected: it would duplicate File
  Diff's rendering, staging, and saving.

## Consequences

- New File Diff behaviour that needs the pair's paths or what a side is goes
  through `App::compared_pair` (the Compared pair, `CONTEXT.md`), not
  `App::selected_row`.
- A file-pair session has no Directory Tree, so Commands scoped to the tree
  never appear in it.
- Writability is judged once at startup by opening the file for writing. A
  writable file in a directory the user cannot write still fails to save,
  because a save writes a temp file beside it; the failure reaches the user as
  the save's error toast.
