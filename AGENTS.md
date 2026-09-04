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

When solving a problem reveals non-obvious knowledge:

1. **Candidate**: Distill a concise, non-derivable rule in at most two context-tagged bullets and propose it to the user.
2. **Promote**: On explicit confirmation, place it where whoever would break the rule must pass — first tier that applies, never in two places at once:
   - **Enforce it** when the fix is already in hand: an assert, a type, or a test leaves nothing to remember.
   - **Comment at the site that must be passed**: the constant or declaration a future change has to touch.
   - **Agent-facing doc** when no single site owns it: merge into an existing doc in `docs/agents/`, or fall back to `docs/agents/lessons-learned.md`. Keep one `@path` line under Pointers.
3. **Prune**: Propose removing entries once obsolete, enforced by tooling, duplicated, or reduced to a debugging transcript.

## Claude Code Compatibility

`CLAUDE.md` is a symbolic link to this file. Edit `AGENTS.md` directly.
