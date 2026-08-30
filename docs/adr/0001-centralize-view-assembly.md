# Centralize View assembly between App state and UI rendering

**Status**: accepted

`App` owns application and domain state. Rendering needs borrowed, frame-consistent
snapshots of that state, but assembling those snapshots in `app.rs` made App know UI
types while `ui.rs` also knew App types. The repeated builders had no single owner and
allowed screen preparation, geometry, and painting to drift.

## Decision

- `view.rs` owns borrowed presentation DTOs and translates `App` state into one
  `ScreenView` per frame.
- Each frame runs `view::prepare_frame(&mut App, Rect)`, then
  `view::assemble(&App)`, then `ui::draw(&ScreenView)`. Preparation is limited to
  deterministic in-memory normalization and viewport synchronization.
- `layout.rs` owns pure layout inputs and geometry calculations shared by frame
  preparation, rendering, and hit testing.
- `ui.rs` paints View DTOs and does not receive `App` or App-owned types.
- App-owned rows and screen/config/confirmation vocabulary are projected at the
  View seam. Stable types from independent modules may pass through directly.
- Large tree and diff collections remain borrowed; assembly must not clone data in
  proportion to their size.
- Base screens aggregate their narrow content/footer DTOs. Confirm and Command
  Palette remain orthogonal overlays; the exclusion editor belongs to Config.

## Consequences

- Rendering tests can continue to hand-build narrow View DTOs without constructing
  a full `App`.
- `prepare_frame` must not perform I/O, navigation, or user-visible transitions.
- New screens add their translation in `view.rs`, geometry in `layout.rs`, and
  painting in `ui.rs`; they must not reintroduce `App::*_view()` builders.
- The private sub-state and fixture decisions in ADR-0002 remain unchanged.
