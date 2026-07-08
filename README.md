# duodiff

[![CI](https://github.com/akunzai/duodiff/actions/workflows/ci.yml/badge.svg)](https://github.com/akunzai/duodiff/actions/workflows/ci.yml)
[![crates.io](https://badgen.net/crates/v/duodiff)](https://crates.io/crates/duodiff)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`duodiff` is a fast, cross-platform terminal user interface (TUI) directory comparison tool written in Rust.

![duodiff demo](https://raw.githubusercontent.com/akunzai/duodiff/main/website/demo.gif)

## Features

- **Side-by-Side Tree View**: Balanced, clean double-pane tree view aligning matched, differing, and missing folders/files.
- **Asynchronous Scanner**: Directory comparison and checksum calculations run in non-blocking background threads, keeping the TUI completely fluid and responsive.
- **Vim Keys & Mouse Support**: Navigable via arrow keys, `hjkl`, tab-focus switching, mouse clicks (double-click to open/expand), and synchronized mouse scrolling.
- **Flexible Comparison Modes**:
  - *Fast mode*: Compares file size and modification time.
  - *Precise mode*: Compares file contents using streaming MD5 checksums.
- **Built-in Diff View**: In-app read-only, color-coded side-by-side file contents diff viewer with synchronous scrolling.
- **External Diff Tool & Editor**: Configure and launch external diff tools (`vim`, `nvim`, `code`, `meld`, `bcomp`, `smerge`, `ksdiff`, `difft`) to compare differing file pairs, or open the selected file in your external editor (using `$VISUAL` or `$EDITOR`).
- **Configuration Screen & Persistence**: A built-in configuration UI to detect system-installed diff tools and save your selection to `~/.config/duodiff/config.toml`.
- **Right-Click Context Menu**: Mouse context actions (Compare via Ext Diff Tool, Edit via Ext Editor, Edit Configuration, Cancel) by right-clicking on any tree item.

## Installation

**Recommended** — download a checksummed prebuilt binary (no Rust toolchain):

```bash
curl -fsSL https://raw.githubusercontent.com/akunzai/duodiff/main/install.sh | bash
```

On Windows, use the [PowerShell installer](docs/INSTALL.md#windows-powershell) instead of piping `install.sh` into `bash`.

Homebrew, Scoop, crates.io, manual download, build-from-source, and cargo binstall are documented in **[docs/INSTALL.md](docs/INSTALL.md)**.


## Keyboard & Mouse Shortcuts

A comprehensive list of keyboard shortcuts and mouse interactions for navigating the directory tree, viewing file diffs, and managing files/folders is documented in **[docs/SHORTCUTS.md](docs/SHORTCUTS.md)**.
