# Configuration

Most settings are easiest to change from the in-app Config screen (`C`), which
persists each change immediately.

To configure by hand instead, copy
[config.example.toml](../config.example.toml) to
`~/.config/duodiff/config.toml` (or `$XDG_CONFIG_HOME/duodiff/config.toml` when
set) and edit it. All fields are optional.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `external_diff_tool` | string | `"auto"` | External diff tool for the `D` key: `"auto"` (resolves the first available tool), `"disabled"`, or a pinned tool (`"vim"`, `"nvim"`, `"code"`, `"meld"`, `"bcomp"`, `"smerge"`, `"ksdiff"`, `"difft"`). |
| `check_updates` | bool | `true` | Daily background check for a newer GitHub release. |
| `mouse` | bool | `true` | Mouse support (click, scroll, double-click). `--no-mouse` also disables it for one session. |
| `theme` | string | `"dark"` | Colour theme: `"dark"` or `"light"`. `T` toggles and persists it. |
| `diff_context` | integer | `3` | Unchanged context lines shown around each hunk in the collapsed File Diff view (`f` toggles full vs. collapsed). |
| `scan_mode` | string | `"fast"` | Scan mode: `"fast"` (size + mtime) or `"precise"` (streaming SHA-256). Change it with `c`, the Config screen, or the palette — all persist. `--scan-mode <fast\|precise>` overrides it for one session without writing the file. |
| `respect_gitignore` | bool | `true` | Read each root's nested `.gitignore` rules. `--gitignore` / `--no-gitignore` override it for one session. |
| `global_exclusions` | string list | built-in VCS/junk list | Rules for both roots, applied before their `.gitignore` and `.duodiffignore` rules. Set `[]` to disable the defaults; repeated `--exclude` patterns are session-only and take precedence. |
