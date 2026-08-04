# Lessons Learned

Context-tagged gotchas and non-obvious durable facts for duodiff.

- **[crossterm / non-TTY]** Guard raw mode & alt-screen with `stdout().is_terminal()` — otherwise CI tests hang.
- **[Windows $EDITOR]** Mock with `"cargo --version"`, not `"true"`.
- **[$EDITOR args]** Split on whitespace (`code --wait`).
- **[env tests]** Serialize `$EDITOR`/`$VISUAL` mutations via `crate::diff_tool::TEST_MUTEX`.
