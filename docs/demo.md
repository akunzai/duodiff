# Demo recording harness

Regenerates the README demo (`website/demo.gif`) and still screenshots
(`website/tree-view.png`) by driving the **real** `duodiff` binary against
**fake** data using [`tcut`](https://tcut.amanv.dev), fully scripted and
reproducible in TypeScript — no manual keypresses.

```bash
mise run demo
```

or directly:

```bash
tcut scripts/demo.video.ts
```

That builds `duodiff`, records a headless session with Ghostty, renders
`website/demo.gif`, and captures still PNG snapshots.

## Why this exists

A TUI is hard to screenshot consistently by hand, and recording against real
directories leaks private paths and drifts every run. This harness pins the
data and the keystrokes, so:

- **Re-record after a UI change** — run `mise run demo` to get a fresh,
  identical-framing GIF and screenshots.
- **Deterministic fixtures** — the same fake left/right directory trees every time.
- **Single script** — driven by `scripts/demo.video.ts` using `tcut`'s
  screen-asserting driver and snapshot capability (`t.snapshot()`).

## How it works

The entire harness is self-contained in `scripts/demo.video.ts`:
1. Seeds sample comparison directory trees with identical files, timestamp-only diffs, left/right-newer diffs, and mergeable hunks directly using Node.js filesystem APIs with fixed `mtime` timestamps.
2. Isolates config in a temporary `$DUODIFF_DEMO_HOME` and `$XDG_CONFIG_HOME` workspace (cleaned up on exit).
3. Launches the release binary in headless Ghostty via `tcut`, driving key sequences, assertions (`t.wait`), and generating both the demo GIF (`website/demo.gif`) and still PNG snapshot (`website/tree-view.png`).

## Storyboard

Side-by-side tree comparison → tree snapshot (`website/tree-view.png`) → focus
panes (`2`, `1`) → slash filter (`/merge` + `Enter`) → diff view (`Enter`) →
merge right hunk (`]`) → next diff (`N`) → merge right hunk (`]`) → back to
tree (`Esc`) → quit (`q`).

## Requirements

`mise install` (from the repo root) provisions `cargo` and `tcut` from the
pinned [`mise.toml`](../mise.toml).
