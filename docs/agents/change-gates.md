# Change Gates

Apply the gates that match the change:

- **User-visible key, screen, or feature**: update `docs/SHORTCUTS.md` and `topic_lines` in `src/help.rs` together.
- **Visual chrome**: confirm the screen in the TUI (or tests). Do not refresh `website/demo.gif` or `website/*.png` on each issue or PR — drop any regenerated demo assets from the change so a milestone does not accumulate screenshot churn. At release time, re-record only if a change since the last recording is both user-visible and appears in the recorded flow; see `RELEASING.md`. The one exception: a change that makes the committed assets *wrong* rather than merely dated — a renamed screen, a changed row mark — re-records in the same change, because a demo that contradicts the shipped UI is worse than the churn.
- **User-visible feature or fix**: describe the user-facing outcome in the PR title; it becomes a GitHub release-note entry. See `RELEASING.md` for how notes are generated and reviewed.
- **Pull request**: apply exactly one release label: `enhancement`, `bug`, `documentation`, `dependencies`, or `skip-changelog` (exclude the PR from release notes).
- **Every issue and pull request**: assign the milestone for the release it targets — no issue or PR stays without one. See `docs/agents/issue-tracker.md` for the `gh` commands.
