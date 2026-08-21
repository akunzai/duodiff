# duodiff

[![CI](https://github.com/akunzai/duodiff/actions/workflows/ci.yml/badge.svg)](https://github.com/akunzai/duodiff/actions/workflows/ci.yml)
[![crates.io](https://badgen.net/crates/v/duodiff)](https://crates.io/crates/duodiff)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`duodiff` is a fast, cross-platform terminal user interface (TUI) directory comparison tool written in Rust.

![duodiff demo](https://raw.githubusercontent.com/akunzai/duodiff/main/website/demo.gif)

## Features

- **Side-by-Side Tree View**: Balanced, clean double-pane tree view aligning matched, differing, and missing folders/files.
- **Asynchronous Scanner**: Directory comparison and checksum calculations run in non-blocking background threads, keeping the TUI completely fluid and responsive.
- **Vim Keys & Mouse Support**: Navigable via arrow keys, `hjkl`, `Ctrl+f`/`Ctrl+b` page scroll, tab-focus switching (`1`/`2`), mouse clicks (double-click to open/expand), and synchronized mouse scrolling.
- **Flexible Comparison Modes**: *Fast* (default; size and modification time) or *Precise* (streaming SHA-256 of the contents). The choice persists, and `--scan-mode <fast|precise>` overrides it for one session.
- **Honest Row States**: `=` no difference found, `≈` **content unverified** (the bytes were not compared — same size but a different timestamp in Fast mode, or an unreadable side in Precise mode), `≠` an established difference, `⬅` / `➡` present on one side only, `💥` file/directory conflict.
- **Built-in Diff View**: In-app color-coded side-by-side file contents diff with intraline highlighting, next/previous change jumps, and per-hunk copy (`[` / `]`).
- **Interactive File Sync**: Copy files or folders between panes with `L` / `R` (confirmation required).
- **Filter & Search**: Inline `/` filter bar, with optional diffs-only mode.
- **Directory Swap**: Swap left and right comparison roots with `s` and re-scan.
- **External Diff Tool & Editor**: Configure and launch external diff tools (`vim`, `nvim`, `code`, `meld`, `bcomp`, `smerge`, `ksdiff`, `difft`) to compare differing file pairs, or open the selected file in your external editor (using `$VISUAL` or `$EDITOR`).
- **Configuration Screen & Persistence**: A built-in configuration UI to detect system-installed diff tools and save your selection to `~/.config/duodiff/config.toml` (honors `XDG_CONFIG_HOME` when set).
- **Light/Dark Theme**: Press `T` from anywhere to toggle between the dark (default) and light colour theme; the choice persists across restarts.
- **Command Palette**: One searchable command surface for every screen, opened with `;`, `Ctrl+p`, or a right-click. Type to search, `Up`/`Down` to select, `Enter` to run; commands that cannot run right now stay listed with the reason.
- **Help**: Topic-based in-app Help (`?`).
- **Self-Upgrade**: Optional background update check and `duodiff --upgrade` for standalone installs.

## Installation

**Recommended** — download a checksummed prebuilt binary (no Rust toolchain):

```bash
curl -fsSL https://raw.githubusercontent.com/akunzai/duodiff/main/install.sh | bash
```

On Windows, use the [PowerShell installer](docs/INSTALL.md#windows-powershell) instead of piping `install.sh` into `bash`.

Homebrew, Scoop, crates.io, manual download, build-from-source, and cargo binstall are documented in **[docs/INSTALL.md](docs/INSTALL.md)**.


## Keyboard & Mouse Shortcuts

A comprehensive list of keyboard shortcuts and mouse interactions for navigating the directory tree, viewing file diffs, and managing files/folders is documented in **[docs/SHORTCUTS.md](docs/SHORTCUTS.md)**.

## Configuration

Most settings are easiest to change from the in-app Config screen (`C`), which persists your choice immediately. To configure by hand instead, copy **[config.example.toml](config.example.toml)** to `~/.config/duodiff/config.toml` (or `$XDG_CONFIG_HOME/duodiff/config.toml` if set) and edit it — all fields are optional.

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `external_diff_tool` | string | `"auto"` | External diff tool for the `D` key: `"auto"` (default; resolves first available tool), `"disabled"`, or a pinned tool (`"vim"`, `"nvim"`, `"code"`, `"meld"`, `"bcomp"`, `"smerge"`, `"ksdiff"`, `"difft"`). |
| `check_updates` | bool | `true` | Daily background check for a newer GitHub release. |
| `mouse` | bool | `true` | Mouse support (click, scroll, double-click). `--no-mouse` also disables it for one session. |
| `theme` | string | `"dark"` | Colour theme: `"dark"` or `"light"`. Press `T` to toggle and persist. |
| `diff_context` | integer | `3` | Unchanged context lines shown around each hunk in the collapsed File Diff view (`f` toggles full vs. collapsed). |
| `scan_mode` | string | `"fast"` | Scan mode: `"fast"` (size + mtime) or `"precise"` (streaming SHA-256). Change it with `c`, the Config screen, or the palette — all persist. `--scan-mode <fast\|precise>` overrides it for one session without writing the file. |
| `respect_gitignore` | bool | `true` | Read each root's nested `.gitignore` rules. `--gitignore` / `--no-gitignore` override it for one session. |
| `global_exclusions` | string list | built-in VCS/junk list | Rules for both roots, before their `.gitignore` and `.duodiffignore` rules. Set `[]` to disable defaults; repeated `--exclude` patterns are session-only and take precedence. |
