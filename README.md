# duodiff

`duodiff` is a fast, cross-platform terminal user interface (TUI) directory comparison tool written in Rust.

## Features

- **Side-by-Side Tree View**: Balanced, clean double-pane tree view aligning matched, differing, and missing folders/files.
- **Asynchronous Scanner**: Directory comparison and checksum calculations run in non-blocking background threads, keeping the TUI completely fluid and responsive.
- **Vim Keys & Mouse Support**: Navigable via arrow keys, `hjkl`, tab-focus switching, mouse clicks (double-click to open/expand), and synchronized mouse scrolling.
- **Flexible Comparison Modes**:
  - *Fast mode*: Compares file size and modification time.
  - *Precise mode*: Compares file contents using streaming MD5 checksums.
- **Built-in Diff View**: In-app read-only, color-coded side-by-side file contents diff viewer with synchronous scrolling.
- **External Editor Diffing**: Configure and launch external diff tools (`vim`, `nvim`, `code`, `zed`) to compare differing file pairs directly, suspending TUI raw mode and restoring it cleanly on exit.
- **Settings Screen & Persistence**: A built-in configuration UI to detect system-installed editors and save your selection to `~/.config/duodiff/settings.toml`.
- **Right-Click Context Menu**: Mouse context actions (Compare via Ext Editor, Configure Settings, Cancel) by right-clicking on any tree item.

## Installation

### Prerequisites
Make sure you have Rust and Cargo installed.

### Quick Start
Build and run `duodiff` directly using Cargo:
```bash
cargo run -- <left_directory_path> <right_directory_path>
```

## Keyboard & Mouse Shortcuts

| Key / Action | Action |
| --- | --- |
| `q` / `Esc` | Quit application (or return to directory tree from diff view) |
| `j` / `k` / `Down` / `Up` | Move selection down / up |
| `h` / `l` / `Left` / `Right` | Collapse / expand selected directory |
| `Space` | Toggle folder expansion |
| `Tab` | Switch focus between left and right panes |
| `Enter` | Enter built-in side-by-side diff view for files |
| `e` | Compare the selected file pair via the configured external editor (when both exist) |
| `S` | Open settings configuration menu (e.g. select external editor) |
| `c` | Toggle between Fast mode and Precise (MD5) mode |
| `r` | Trigger manual directory re-scan |
| **Mouse Click** | Select a row (active side borders highlight in Green) |
| **Mouse Right-Click** | Select a row and open floating actions context menu |
| **Mouse Double-Click** | Open diff view for files / expand directory for folders |
| **Mouse Scroll** | Synchronously scroll directory trees or diff lines |
