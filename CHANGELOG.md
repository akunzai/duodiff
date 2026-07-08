# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Added a topic-based Help screen (`?` to open, 5 topics, number-key quick jump) (Issue #28).
- Dropped the redundant `Left:`/`Right:` prefix from the directory tree and diff view pane titles, freeing up space for longer paths (Issue #29).
- Redesigned UI/UX: implemented unified Top Bar, minimal Footer, and unified Palette (Ctrl+p / ;) with Menu & Command modes. Folded About info into Help screen.
- Fixed Help Topic Index list items now selectable by mouse click.
- Removed background colour from the top-bar product name/mode label for a cleaner look.
- Added next/previous change navigation in the file diff view (`N`/`P` or `Alt+Down`/`Alt+Up`) (Issue #30).

## [0.2.0] — 2026-07-08

- Added self-upgrade support and a daily background update check (Issue #12).
- Added automated demo recording via `asciinema`/`agg` (Issue #13).
- Extracted keyboard/mouse shortcuts into `docs/SHORTCUTS.md` and added a GitHub Pages landing page.
- Aligned side-by-side diff replacement rows and normalized CRLF/LF line endings (Issue #26).
- Added `install.sh`/`install.ps1` installer scripts.
- Updated `Cargo.toml` metadata for crates.io and `cargo-binstall` support.

## [0.1.0] — 2026-07-07

### Added
- **Side-by-Side Tree View**: Balanced, clean double-pane tree view aligning matched, differing, and missing folders/files.
- **Asynchronous Scanner**: Directory comparison and checksum calculations run in non-blocking background threads.
- **Vim Keys & Mouse Support**: Navigable via arrow keys, `hjkl`, tab-focus switching, mouse clicks, and synchronized scrolling.
- **Comparison Modes**: Fast mode (size and modification time) and Precise mode (MD5 checksums).
- **Built-in Diff View**: In-app read-only, color-coded side-by-side file contents diff viewer.
- **External Diff Tool & Editor**: Configure and launch external diff tools (`vim`, `nvim`, `code`, `meld`, `bcomp`, `smerge`, `ksdiff`, `difft`) or editors.
- **Interactive File Copying**: Copy files/folders directly between panes (`L` / `R`) with safety confirmation modals.
- **Interactive Filter & Search**: Search and filter tree items inline using `/`.
- **Directory Swapping**: Quick swap of left and right target directories with the `s` key.
- **Right-Click Context Menu**: Mouse context actions for quick compares, editing, and configuration.
- **Settings Screen**: Persistent configurations saved to `~/.config/duodiff/config.toml`.
