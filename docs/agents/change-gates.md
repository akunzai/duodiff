# Change Gates

Apply the gates that match the change:

- **User-visible key, screen, or feature**: update `docs/SHORTCUTS.md` and `help_topic_body` in `src/ui.rs` together.
- **Visual chrome**: confirm the screen in the TUI (or tests). Do not refresh `website/demo.gif` or `website/*.png` on each issue or PR — drop any regenerated demo assets from the change so a milestone does not accumulate screenshot churn. Re-record once when cutting the release; see @RELEASING.md.
- **User-visible feature or fix**: add one bullet under `## [Unreleased]` in `CHANGELOG.md`; omit it for internal-only or `skip-changelog` changes.
- **Pull request**: apply exactly one release label: `enhancement`, `bug`, `documentation`, `dependencies`, or `skip-changelog`.
- **Every issue and pull request**: assign the milestone for the release it targets — no issue or PR stays without one. See @docs/agents/issue-tracker.md for the `gh` commands.
