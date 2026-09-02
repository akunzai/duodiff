# Change Gates

Apply the gates that match the change:

- **User-visible key, screen, or feature**: update `docs/SHORTCUTS.md` and `help_topic_body` in `src/ui.rs` together.
- **Visual chrome**: confirm the screen in the TUI (or tests). Do not refresh `website/demo.gif` or `website/*.png` on each issue or PR — drop any regenerated demo assets from the change so a milestone does not accumulate screenshot churn. At release time, re-record only if a change since the last recording is both user-visible and appears in the recorded flow; see @RELEASING.md. The one exception: a change that makes the committed assets *wrong* rather than merely dated — a renamed screen, a changed row mark — re-records in the same change, because a demo that contradicts the shipped UI is worse than the churn.
- **User-visible feature or fix**: add one bullet under `## [Unreleased]` in `CHANGELOG.md`; omit it for internal-only or `skip-changelog` changes. **One bullet per line, no blank line between bullets** — match the compact style of every already-released section; don't insert a blank line just because an entry wraps onto multiple sentences.
- **Pull request**: apply exactly one release label: `enhancement`, `bug`, `documentation`, `dependencies`, or `skip-changelog`.
- **Every issue and pull request**: assign the milestone for the release it targets — no issue or PR stays without one. See @docs/agents/issue-tracker.md for the `gh` commands.
