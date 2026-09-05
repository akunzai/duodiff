# duodiff — Agent Guidelines

duodiff is a Rust TUI for comparing and synchronizing directory trees.

## Verification

- Run the full gate with `mise run check`; see @CONTRIBUTING.md for setup and individual Cargo commands.

## Pointers

- TUI architecture and invariants: read @docs/agents/tui.md before changing state, rendering, input, editor handoff, or diff loading.
- Domain language and architecture decisions: follow @docs/agents/domain.md and the relevant @docs/adr/ before architecture work.
- User-visible behavior gates: follow @docs/agents/change-gates.md for shortcuts, screens, visual chrome, and changelog updates.
- Voice, marks, screen naming, and README shape: follow @docs/agents/design.md before adding or rewording any user-visible string.
- Demo recording and screenshots: @docs/demo.md
- Releases and versioning: @RELEASING.md
- GitHub issues and labels: @docs/agents/issue-tracker.md and @docs/agents/triage-labels.md
- Non-obvious environment gotchas: @docs/agents/lessons-learned.md

## Self-Reflection

- **Candidate**: Distill a non-obvious gotcha into ≤ 2 context-tagged bullets. Propose it before writing.
- **Promote**: On confirmation, put it where whoever would break it must already pass — enforce it (assert/type/test) when the fix is in hand, else a comment at that site, else an agent-facing doc (merge an existing topic doc, else `docs/agents/<topic>.md`, else `docs/agents/lessons-learned.md`) with one `@path` line under Pointers. Never both.
- **Prune**: When adding to a file, audit the rest of it in the same pass. Drop entries once stale (obsolete version, now enforced, duplicated, or a transcript) — not by a fixed count.

## Claude Code Compatibility

`CLAUDE.md` is a symbolic link pointing to `AGENTS.md`. Edit `AGENTS.md` directly.
