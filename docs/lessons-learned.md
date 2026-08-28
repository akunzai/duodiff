# Lessons Learned

Context-tagged gotchas and non-obvious durable facts for duodiff.

- **[crossterm / non-TTY]** Guard raw mode & alt-screen with `stdout().is_terminal()` — otherwise CI tests hang.
- **[Windows $EDITOR]** Mock with `"cargo --version"`, not `"true"`.
- **[$EDITOR args]** Split on whitespace (`code --wait`).
- **[env tests]** Serialize `$EDITOR`/`$VISUAL` mutations via `crate::diff_tool::TEST_MUTEX`.
- **[settings tests]** Any test that reaches a `settings.save()` needs a `ConfigEnvGuard`. `HOME` is process-global, so an unguarded write does not land in the developer's real config — it lands in whichever concurrent guarded test currently owns `HOME`, silently reverting what that test just saved. `AppSettings::save()` asserts the redirect under `cfg(test)` so the offender fails instead of the victim.
