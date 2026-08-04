# duodiff — Agent Guidelines

Index-driven entrypoint. Prefer **Rich References** (`@path`) over prose; offload SOPs via lazy load.

## Quick Commands

- Build: `cargo build`
- Test: `cargo test` · Single: `cargo test <name>`
- Run: `cargo run -- <left_dir> <right_dir>`
- Lint/Format: `cargo clippy && cargo fmt`
- Verify gate: `mise run check` (see @CONTRIBUTING.md)
- Demo: `mise run demo` → `website/demo.gif` + `website/*.png`
  - GIF / PNG only: `mise run demo-gif` / `mise run demo-png`
  - New shot: `SHOTS` in `scripts/demo/shots.sh` + `scripts/demo/shots/<name>.json`

## Architecture

TUI directory-diff (crossterm + ratatui + tokio). Modules under `@src/`:

| Area | Modules |
|---|---|
| State / loop | `main` (CLI + `run_app`), `app`, `event`, `input`, `actions` |
| Diff domain | `diff` (tree scan/align), `diff_view` (line diff / `similar`), `diff_tool` |
| Render / config | `ui`, `theme`, `settings` (`config.toml`), `text_input` |
| Ship / meta | `upgrade` (self-upgrade + startup update check); `website/` (Pages); `docs/` (INSTALL, SHORTCUTS) |

**Rich refs:** `@src/app.rs` (`App`, `FlatRow`, `ViewMode`) · `@src/diff_view.rs` (`DiffRow`) · `@src/ui.rs` (`help_topic_body`)

### ADRs (required before architecture refactors)

Read `@docs/adr/` before changing App state shape, ui layout/draw seams, or applying simplify/ponytail “delete this layer” cuts. **Contradicting an accepted ADR → surface the conflict and stop or grill; do not reverse silently.**

| ADR | One-line |
|---|---|
| @docs/adr/0001-defer-app-ui-coupling-from-substate-split.md | App↔ui View assembly coupling deferred |
| @docs/adr/0002-app-substate-and-view-dual-path.md | Sub-state private + domain methods; test fixtures stay; View/LayoutInputs dual-path stays |

## Project Invariants (jargon)

TUI constraints not obvious from types alone. App/ui **structure** decisions live in ADRs above — do not re-derive them as “yagni.”

| Term | Rule |
|---|---|
| **TTY recovery** | Leave raw mode + alternate screen on every exit path; event loop only via `run_app`. |
| **Editor handoff** | Leave TUI before spawning external diff/editor; restore immediately after. |
| **Flat-row render** | Draw from `app.flat_rows` only — never walk the tree each frame (avoids O(N²)). |
| **Diff-once** | Fill file-diff rows when entering `FileDiff`; never read/diff inside the draw loop. |
| **Focus green** | Active pane border follows left/right focus (`focus_left_pane` / `focus_right_pane` / `toggle_active_side`). |
| **Modal capture** | While `confirm_modal().is_some()`, all keys/mouse go to the modal; confirmed copy triggers re-scan. |

## Self-Reflection

When solving a problem reveals non-obvious knowledge (gotchas, hidden configs, env var quirks):
1. **Candidate**: Distill into a concise, non-derivable rule (≤ 2 bullets, context-tagged).
2. **Promote**: Confirm with user, then write to topic doc or fallback `@docs/lessons-learned.md` — never inline in `AGENTS.md`.
3. **Prune**: Periodically drop stale entries.

## Conventions

- Commits: Conventional Commits (EN); amend same-scope follow-ups (no `fix typo` noise).
- **PR release label** (required): `enhancement` \| `bug` \| `documentation` \| `dependencies` \| `skip-changelog` — groups notes via `.github/release.yml`.
- **Docs lockstep**: user-facing key/screen/feature → `@docs/SHORTCUTS.md` + `help_topic_body` (`@src/ui.rs`) same PR.
- **Demo refresh**: visual chrome delta → `mise run demo` same PR (skip if behavior-only).
- **Changelog bullet**: user-facing feat/fix → one line under `## [Unreleased]` in `@CHANGELOG.md` (skip for `skip-changelog` / internal-only).
- Versioning / release cadence: stay `0.x` while UX still moves; mid-cycle bump `Cargo.toml` only; stamp CHANGELOG version/date at cut — full SOP @RELEASING.md.

## Agent skills (lazy)

- Issue tracker (`gh`): @docs/agents/issue-tracker.md
- Triage labels: @docs/agents/triage-labels.md
- Domain / glossary: @docs/agents/domain.md (`CONTEXT.md` + `@docs/adr/`)
- Lessons learned: @docs/lessons-learned.md

## Claude Code Compatibility

`CLAUDE.md` → symlink to this file. Edit `AGENTS.md` only; do not replace the link.
