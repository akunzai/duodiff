# duodiff — Agent Guidelines

duodiff is a Rust TUI for comparing and synchronizing directory trees.

## Verification

- Run the gate, capture evidence, and see what it does not cover: @docs/agents/verification.md.

## Pointers

- TUI architecture and invariants: read @docs/agents/tui.md before changing state, rendering, input, editor handoff, or diff loading.
- Domain language and architecture decisions: follow @docs/agents/domain.md and the relevant @docs/adr before architecture work.
- User-visible behavior gates: follow @docs/agents/change-gates.md for shortcuts, screens, visual chrome, and changelog updates.
- Voice, marks, screen naming, and README shape: follow @docs/agents/design.md before adding or rewording any user-visible string.
- Demo recording and screenshots: @docs/demo.md
- Releases and versioning: @RELEASING.md
- GitHub issues and labels: @docs/agents/issue-tracker.md and @docs/agents/triage-labels.md
- Pull request shape, tests-with-behavior, and review readiness: @docs/agents/pull-request.md
- Non-obvious environment gotchas: @docs/agents/lessons-learned.md

## Prevent Recurrence

- **Candidate**: Name who hits this again, in which file, on what change. No such scenario, nothing to propose.
- **Promote**: Offer the first tier that reaches them and only that one, pending confirmation — enforce it (assert/type/test) with its size quoted, else a comment at that site, else an agent-facing doc (`docs/agents/<topic>.md`, else `docs/agents/lessons-learned.md`) with one `@path` line under Pointers and one sentence on why the tiers above cannot hold it.
- **Prune**: When adding to a file, audit the rest of it in the same pass. Drop entries once stale (obsolete version, now enforced, duplicated, or a transcript) — not by a fixed count.

## Claude Code Compatibility

`CLAUDE.md` is a symbolic link pointing to `AGENTS.md`. Edit `AGENTS.md` directly.
