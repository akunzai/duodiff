# The Directory Tree is apart from the scan that produces it

**Status**: accepted

## Context

#311 moved the last flat cluster of `App` fields into `ScanState`, grouped by
the scan: the aligned tree and its flattened rows sat beside the in-flight
flag, progress, generation, and spinner. The filter, the rows it keeps, and the
cursor were already `TreeListState` (#309).

That split put one concept on both sides of a seam. Every change to the tree —
adopting a scan, a partial rescan, expand, collapse, collapse all, jumping to a
difference — had to be followed by reflattening in `ScanState` and refiltering
in `TreeListState`, and only `App` held both. Six doc comments told the caller
it was their job. Expand state lived on the scan's output, so every rescan had
to snapshot and restore it; #338 was that restore missing a case.

## Decision

- `DirectoryTreeState` owns the Directory Tree (`CONTEXT.md`): the aligned
  tree, the user's expand state, the rows, the filter, the cursor, double-click
  detection, and the list's visible height. Every mutating method leaves them
  consistent before it returns.
- Expand state is the user's, keyed by path in the Directory Tree. The scan's
  `AlignedNode` carries only `expanded_by_default` — open for a directory on
  both sides, closed for a one-sided directory and everything below it — which a directory takes the
  first time the Directory Tree sees it. A rescan cannot lose a choice.
- `ScanState` owns only the background scan's lifecycle: in flight, progress,
  generation, spinner. A finished scan hands its tree to the Directory Tree.
- Pane focus belongs to the session, on `App`: File Diff uses it too, and a
  session comparing two files has no Directory Tree (ADR-0004).
- `App` keeps only what needs I/O or another sub-state: the partial rescan
  walks the filesystem and hands the Directory Tree a subtree to graft.

## Considered options

- **Grouping by the scan (#311)**: rejected for the reason above — the tree's
  content and its listed rows are one concept, the scan's progress another.
- **One struct holding the scan's lifecycle too**: rejected; the lifecycle
  changes on every progress tick and has no bearing on what is listed.

## Consequences

- Simplify reviews that propose regrouping the tree with the scan contradict
  this ADR and must surface it as a conflict rather than apply it.
- New Directory Tree operations go on `DirectoryTreeState` and end consistent;
  they do not add an `App` method that mutates and then reflattens.
- `Viewport.visible_height` serves File Diff only; the Directory Tree keeps its
  own.
