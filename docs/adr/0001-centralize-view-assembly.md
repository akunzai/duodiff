# Defer resolving App ↔ ui.rs coupling in the App sub-state split

**Status**: accepted

Splitting `App`'s fields into owned sub-states (`HelpState` first, tracked in #178, with `ConfigState`/`FilterState`/`DiffState` to follow) does not reduce the bidirectional coupling between `App` and `ui.rs`: `App` still builds `crate::ui::{HelpView, DiffView, TreeView, ConfigView, TopBarView, ConfirmView, PaletteView}` directly (e.g. `App::help_view()`), and `ui.rs` still reaches into `crate::app::{ConfigRowKind, PaletteAction}`. Moving fields into a sub-struct doesn't change which module imports which type — `App::help_view()` still has to construct `crate::ui::HelpView` regardless of whether the fields it reads live flat on `App` or inside `self.help`.

We're treating this coupling as a separate, unaddressed architectural finding (2026-07-28 architecture review, candidate 1b) and are **not** folding a fix for it into #178. Untangling it would mean redesigning how View structs get assembled — a different, larger decision that deserves its own `/grilling` session rather than riding along on a field-ownership refactor.

## Consequences

- Future architecture reviews will keep surfacing this coupling until it's tackled on its own — expected, not a regression introduced by #178.
- Contributors picking up #178's later slices (Config/Filter/Diff) should not scope-creep into "fixing" the coupling as part of those PRs.
