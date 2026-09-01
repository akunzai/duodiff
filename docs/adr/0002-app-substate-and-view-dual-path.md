# App sub-state encapsulation and View dual-path are intentional

**Status**: accepted

## Context

A whole-repo `/ponytail-audit` (#221) flagged several patterns as over-engineering:

- Private fields + getters on `HelpState` / `FilterState` / `ConfigState` / `FileDiffState`
- Arrange-only test setters behind `#[allow(dead_code)]`
- `App::help()` / `help_mut()` (and filter/diff counterparts) as the sub-state surface
- `*View` / `*LayoutInputs` dual-path in `ui.rs` instead of drawing only from `&App`

Landing those cuts as a PR (attempted in #225) **conflicted with already-grilled decisions** (#178, #179, #200, #201). Simplify-only reviews do not load issue history; without an ADR + `AGENTS.md` pointer they will keep re-proposing the reverse.

## Decision

### 1. Sub-state ownership (#178)

`App` **composes** sub-states (`HelpState`, `FilterState`, `ConfigState`, `FileDiffState`, …). Each sub-state:

- Keeps fields **private**
- Owns domain methods (`enter` / `move_down` / `open_index` / `load` / …)
- Is reached from production via `App::help()` / `help_mut()` (and the same pattern for filter/diff/config), **not** by scattering `help_*` fields flat on `App` again

`open_help` / `close_help` (and analogous overlays) stay on `App` when they also touch nav concerns such as `view_mode`, and **delegate** field mutations into the sub-state.

`PaletteState` follows this shape too: private fields, domain methods, and `App` keeping only the parts that need the Command inventory.

### 2. Test fixture setters (#179)

Arrange-only `set_*` helpers used solely to seed tests (often with `#[allow(dead_code)]` because call sites are `#[cfg(test)]`) are a **deliberate** fixture seam. They do **not** invalidate production privatization.

- Do **not** treat them as encapsulation theatre to delete in simplify reviews
- Do **not** replace them by making production fields `pub(crate)` so tests can write fields directly
- Prefer `#[cfg(test)]` where practical; `#[allow(dead_code)]` on same-crate test-only methods is acceptable when that matches existing style

### 3. View / LayoutInputs dual-path (#200, #201, parent #197)

Pure layout helpers (`diff_layout`, `tree_layout`, …) take **`DiffLayoutInputs` / `TreeLayoutInputs`** (and similar), not `&App`. Content draw takes borrowed **`DiffView` / `TreeView` / …** snapshots built by `App::*view()` / layout input builders.

This exists so:

- `App::sync_viewport` and the draw path share the same pure geometry decisions
- UI tests can build views without a full `App`

Do **not** collapse back to “always draw from `&App`” as a drive-by simplification.

### 4. View assembly ownership

The formerly deferred App/UI coupling is resolved by
@docs/adr/0001-centralize-view-assembly.md: `view.rs` assembles
borrowed snapshots, `layout.rs` owns pure geometry, and `ui.rs` paints without
receiving `App`. This preserves the View/LayoutInputs dual path described above.

## Consequences

- Simplify / ponytail / “delete getters” findings that contradict this ADR must be **surfaced as conflicts**, not applied silently.
- Reopening any of the above requires a new grill (or superseding ADR), not an incidental PR.
- Agents loading @AGENTS.md must treat @docs/adr/ as required reading for architecture-touching work.

## References

- #178 (sub-state epic), #179 (test setters), #197 / #200 / #201 (View dual-path)
- #221 / #225 (ponytail-audit; D closed without merge after conflict)
