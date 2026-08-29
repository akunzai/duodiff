# Features

What each part of `duodiff` does. Keys are listed in
[SHORTCUTS.md](SHORTCUTS.md); settings in [CONFIGURATION.md](CONFIGURATION.md).

## Directory Tree

Both trees are aligned row by row, so a matched pair, a difference, and a
missing entry all sit on the same line. The narrow column between the panes
marks each pair:

| Mark | Meaning |
| --- | --- |
| `=` | No difference found by the active scan mode. |
| `≈` | Content unverified — the bytes were not compared. |
| `≠` | A difference the scan established. |
| `<` / `>` | Present on the left / right side only. |
| `!` | One side is a file, the other a directory. |
| `Aa` | Case-only path mismatch or collision. |

Directories carry a `▸` / `▾` disclosure marker and a trailing `/`.

Scanning and checksums run on background threads, so the interface stays
responsive on large trees. Each pane's bottom border shows the selected row as
`n/N`; the footer shows a whole-tree inventory of leaf pairs.

## Scan modes

- **Fast** (default) compares size and modification time.
- **Precise** compares a streaming SHA-256 of the contents.

The choice persists. `--scan-mode <fast|precise>` overrides it for one session
without writing the config file.

`≈` is the honest answer when the bytes were not compared: in Fast mode the
sizes match but the timestamps differ; in Precise mode a side could not be read
or hashed. Switching to Precise mode resolves it.

## File Diff

A side-by-side, colour-coded diff of one file pair, with intraline
highlighting, next/previous change jumps, and full-file or collapsed context.
Change blocks can be staged to either side (`[` / `]`), undone, and saved.
Limits: UTF-8 text only, 10 MiB per side.

## Sync

Copy a file or a whole directory between panes with `L` / `R`, or copy one
staged change block from the File Diff view. Every copy asks for confirmation
and re-scans afterwards. A directory copy follows the scan's own entry list, so
excluded paths and files created after the scan stay out of it.

## Filter

`/` opens an inline filter bar over the tree, with an optional diffs-only mode.
Filtered rows are shown flat, as `parent › name` breadcrumbs.

## External tools

`D` opens the selected file pair in an external diff tool (`vim`, `nvim`,
`code`, `meld`, `bcomp`, `smerge`, `ksdiff`, `difft`); `E` opens the selected
file in `$VISUAL` or `$EDITOR`. `duodiff` leaves the terminal before spawning
either and restores it on return.

## Exclusions

Each root's nested `.gitignore` and `.duodiffignore` rules are read by default,
after a global exclusion list of common VCS and build artifacts. Both are
editable from the Config screen; `--exclude` adds session-only rules.

## Screens and chrome

- **Config** (`C`) — a flat settings screen that detects installed diff tools
  and persists every change immediately.
- **Command Palette** (`;`, `Ctrl+p`, right-click) — one searchable command
  surface for every screen. Commands that cannot run right now stay listed,
  carrying the reason.
- **Help** (`?`) — topic-based, per screen.
- **Theme** (`T`) — dark (default) or light; the choice persists.
- **Directory swap** (`s`) — swap the two roots and re-scan.

## Updates

An optional daily background check reports a newer GitHub release. Standalone
installs can update in place with `duodiff --upgrade`.
