# Configuration

Most settings are easiest to change from the in-app Config screen (`C`), which
persists each change immediately.

To configure by hand instead, copy
[config.example.toml](../config.example.toml) to
`~/.config/duodiff/config.toml` (or `$XDG_CONFIG_HOME/duodiff/config.toml` when
set) and edit it. All fields are optional.

If the file cannot be parsed, duodiff starts with the defaults, names the file
and line in a toast, and does not save any settings change over it until you
fix it. `duodiff --check` reports the same problem and exits non-zero.

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

## Key bindings

The `[keys]` section remaps the keys that run a command. Each entry names a
command and gives one key or a list of keys:

```toml
[keys]
copy_to_left = "R"
copy_to_right = "L"
next_change = ["n", "alt+down"]
save_staged = []
```

- Your keys **replace** that command's default keys on every screen that has
  the command. `[]` leaves it with no key; every command stays reachable from
  the Command Palette (`;` or `Ctrl+p`).
- Help, the footers, the top bar, and the Command Palette show your keys.
- Changes take effect the next time duodiff starts. The Config screen shows how
  many commands have custom keys.

### Writing a key

- A single character is taken literally and is case-sensitive: `"L"` and `"l"`
  are different keys. Write an uppercase letter or a shifted symbol (`"+"`,
  `"?"`) directly; `shift+` is not accepted.
- Named keys, in any case: `enter`, `tab`, `esc`, `backspace`, `delete`,
  `insert`, `space`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`,
  `pagedown`, `f1` … `f12`.
- Modifiers go in front: `ctrl+x`, `alt+down`, `ctrl+alt+enter`.

### Keys that cannot be bound

duodiff handles these keys itself before looking at any binding, so a command
bound to one could never run:

| Screen | Keys |
| --- | --- |
| Every screen | `;`, `ctrl+p` |
| Directory Tree | `j`, `k`, `up`, `down`, `space`, `ctrl+f`, `ctrl+b` |
| File Diff | `j`, `k`, `up`, `down`, `left`, `right`, `ctrl+f`, `ctrl+b` |
| Config | `j`, `k`, `h`, `l`, `up`, `down`, `left`, `right`, `space`, `enter` |
| Help | `j`, `k`, `up`, `down`, `1` … `6` |

`alt+up` and `alt+down` stay bindable where only the plain arrows are taken.
Some keys are taken only in certain states and stay bindable: in the Directory
Tree, `enter` on a directory expands or collapses it, and `esc` / `backspace`
clear an applied filter; in Help, `enter` and `tab` pick topics.

### When an entry is ignored

An entry is ignored as a whole, and its command keeps its default keys, when:

- the name is not a command below, or the value is not a key or a list of keys;
- a key cannot be read, or is one duodiff handles itself (above);
- a key already runs another command on the same screen. If that other
  command's key is a default, only your entry is ignored; if both are your
  entries, both are. Rebind the other command too to free its key.

duodiff names the first ignored entry in a toast when it starts, and
`duodiff --check` lists every one and exits non-zero. A save from the Config
screen writes `[keys]` back as you wrote it, ignored entries included.

### Command names

| Name | Default keys | Command |
| --- | --- | --- |
| `open_diff` | `Enter` | Open the diff view (Directory Tree) |
| `external_diff` | `D` | Compare with the external diff tool |
| `external_edit` | `E` | Edit in the external editor |
| `copy_to_right` | `R` | Copy the selection, or the whole file, to the right |
| `copy_to_left` | `L` | Copy the selection, or the whole file, to the left |
| `expand` | `l`, `Right` | Expand the selected directory |
| `collapse` | `h`, `Left` | Collapse the selected directory |
| `expand_all` | `+`, `=` | Expand every directory |
| `collapse_all` | `-` | Collapse every directory |
| `next_difference` | `N`, `Alt+Down` | Jump to the next difference (Directory Tree) |
| `prev_difference` | `P`, `Alt+Up` | Jump to the previous difference (Directory Tree) |
| `switch_pane` | `Tab` | Switch the focused pane |
| `focus_left` | `1` | Focus the left pane |
| `focus_right` | `2` | Focus the right pane |
| `filter` | `/` | Filter the tree |
| `swap_sides` | `s` | Swap the left and right directories |
| `switch_scan_mode` | `c` | Switch scan mode (Fast / Precise) |
| `rescan` | `r` | Re-scan both directories |
| `next_change` | `N`, `Alt+Down` | Jump to the next change block (File Diff) |
| `prev_change` | `P`, `Alt+Up` | Jump to the previous change block (File Diff) |
| `stage_to_right` | `]` | Stage the change block to the right |
| `stage_to_left` | `[` | Stage the change block to the left |
| `save_staged` | `s` | Save staged changes (File Diff) |
| `undo_staged` | `u` | Undo the last staged change block |
| `toggle_wrap` | `w` | Toggle line wrapping |
| `toggle_full_context` | `f` | Toggle full-file context |
| `switch_theme` | `T` | Switch the light and dark theme (every screen) |
| `config` | `C` | Open the Config screen |
| `help` | `?` | Open Help |
| `back` | `Esc`, `q` (and `?` in Help) | Go back |
| `quit` | `q`, `Esc` | Quit (Directory Tree) |
