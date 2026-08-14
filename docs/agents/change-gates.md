# Change Gates

Apply the gates that match the change:

- **User-visible key, screen, or feature**: update `docs/SHORTCUTS.md` and `help_topic_body` in `src/ui.rs` together.
- **Visual chrome**: run `mise run demo` and include the refreshed `website/demo.gif` and `website/*.png`; behavior-only changes do not need this.
- **User-visible feature or fix**: add one bullet under `## [Unreleased]` in `CHANGELOG.md`; omit it for internal-only or `skip-changelog` changes.
- **Pull request**: apply exactly one release label: `enhancement`, `bug`, `documentation`, `dependencies`, or `skip-changelog`.
