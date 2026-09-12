# Verification

How an agent exercises a change in this repo before it reaches review.
Setup narrative for humans lives in CONTRIBUTING.md; this file holds
only what an agent needs.

## The gate

```sh
mise run check
```

<!-- drift:forge github -->
<!-- drift:entrypoint-cmd mise run check -->
<!-- drift:file mise.toml -->

It never prompts, and exits non-zero on the first failure. `mise.toml`
holds what it runs and in what order; CONTRIBUTING.md carries the same
steps as native Cargo commands for an environment without mise.

**Proof the binary comes up**: `duodiff --check` with no directory
arguments prints `duodiff version <X.Y.Z> is ready` and exits 0 without
touching the terminal. A green build is not that proof.

## Checks

| What | Command |
| --- | --- |
| Full gate | `mise run check` |
| One stage of it | `mise tasks` lists them |
| Release build | `cargo build --locked --release` |
| Binary startup | `./target/release/duodiff --check` |
| Interactive run | `cargo run -- <left_dir> <right_dir>` |

The interactive run needs a real terminal. An agent confirms on-screen
behaviour in a Herdr pane instead — see Real-terminal checks below, which
also says when that pass is required — and falls back to the headless
harness under Capturing evidence when Herdr is unavailable.

## Human prerequisites

Run once, by a person. The gate fails until they are done.

- [ ] Install a stable Rust toolchain with `rustfmt` and `clippy`, or
      run `mise install` from the repo root, which provisions both plus
      `tcut` from the pinned `mise.toml`.
- [ ] Authenticate `gh` (`gh auth login`) for any issue or pull request
      operation.

<!-- drift:file .github/workflows/ci.yml -->

CI runs the same gate on Ubuntu and re-runs `cargo test --locked` on
Windows and macOS, so a platform-specific path can pass locally and fail
there.

## Ports

Not applicable. duodiff is a terminal application with no listening
service, so several agents can run the gate in the same repo at once.

The one shared resource is process-global environment state. Tests that
mutate `$EDITOR` or `$VISUAL` serialize through
`crate::diff_tool::TEST_MUTEX`, and any test reaching `settings.save()`
needs a `ConfigEnvGuard` — see `docs/agents/lessons-learned.md`.

## Changes that need a deployed environment

None. duodiff ships as a binary and a crate; there is no environment to
deploy to. Release verification — the GitHub release assets and the
crates.io publish — happens after the tag, per `RELEASING.md`.

## Capturing evidence

- Headless TUI capture: `tcut scripts/demo.video.ts`, driven through
  `mise run demo`. It builds the release binary, records a scripted
  session in headless Ghostty, and writes `website/demo.gif` and
  `website/tree-view.png`. See `docs/demo.md`.
- A terminal capture taken here carries this machine's username and home
  paths, exactly as a shared environment would. Assert on the frame, a
  row mark, or the fixture data, and crop or mask the rest.
- **Those output paths are committed assets**, and re-recording them
  belongs to release time — `docs/agents/change-gates.md` has the rule.
  To take evidence for a review, copy the produced file out and
  `git checkout -- website/`.

## Real-terminal checks in a Herdr pane

**Required, not optional**, when a change touches the terminal seam the
test suite cannot reach:

- `src/main.rs`, `src/actions.rs`, `src/event.rs` — entering and leaving
  raw mode and the alternate screen. That seam sits behind `is_terminal()`
  and `cfg!(test)` guards, so `cargo test` never executes it.
- `src/diff_tool.rs` — external editor and diff tool handoff.
- A `crossterm` or `ratatui` bump in `Cargo.toml`; see the `NO_COLOR`
  note below for why.

Any other change stops at the gate. This binds agents and maintainers
only. CONTRIBUTING.md asks contributors for `mise run check` and nothing
more, because Herdr is not in the pinned toolchain and CI cannot run it.

Check availability first. Both must succeed:

```sh
test "${HERDR_ENV:-}" = 1 && command -v herdr
```

Outside a Herdr-managed pane, record the behaviour under Not verified
instead.

The Herdr skill owns pane mechanics — splitting, focus, cleanup. What is
duodiff's: run the binary against throwaway fixtures under a temporary
`HOME` and `XDG_CONFIG_HOME`, so the developer's real config survives,
and keep the fixtures out of any path that names them.

```sh
herdr pane run <pane> "env HOME=$FX/home XDG_CONFIG_HOME=$FX/home/.config \
  ./target/debug/duodiff $FX/left $FX/right"
herdr pane wait-output <pane> --regex '<a row you expect>' --timeout 30000
```

Read back with the source that fits: `--source visible --format ansi`
when colour or a focused border is the evidence, `--format text` for row
content. Drive the TUI with `herdr pane send-text <pane> "<key>"`.

What this covers, and what proves each one:

| Behaviour | Proof |
| --- | --- |
| Raw mode and alternate screen | The tree renders in the pane at all |
| Focus green (`docs/agents/tui.md`) | An ansi read shows SGR `38;5;2` on the focused pane border and `38;5;8` on the other; they swap on `1` / `2` |
| `NO_COLOR` | Launch under `NO_COLOR=1`; an ansi read contains zero SGR sequences |
| TTY recovery on exit | After `q`, a following `herdr pane run` executes and its output matches |
| Editor handoff | Point `$EDITOR` and `$VISUAL` at a script that appends its arguments to a path, press `E`, confirm the file names the selected side's absolute path, then confirm the tree is drawn again |

**`NO_COLOR` is not duodiff's own code.** Nothing under `src/` mentions
it; crossterm honors it in its style layer. The behaviour
`docs/agents/design.md` promises therefore rests on a dependency, and a
crossterm bump can drop it with every test still green.

Bake the editor stub's output path into the script rather than passing it
through an environment variable the stub may not inherit. A stub that
writes nothing looks identical to a handoff that never happened.

## Not verified

A gap you could have closed is not a gap. Run the check whose dependency you
have already seen running, and report a check you skipped as untried, rather
than recording it here as one this repo cannot run.

- **Mouse capture negotiation** and **true-colour rendering across
  emulators**. A Herdr pane exercises one emulator on one host.
- **Windows paths** — ignore-separator handling, install-method
  detection, `$EDITOR` invocation. CI is the first place they run.
- **The upgrade path** in `src/upgrade`, which reaches GitHub Releases
  over the network and is not driven by the gate.
