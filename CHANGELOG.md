# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Restored directory-tree selection and scroll position after `L`/`R` copy (and other rescans), including expanded folders (Issue #47).
- Added vim-style page scrolling with `Ctrl+f` / `Ctrl+b` in the directory tree and file diff views (Issue #49).
- Fixed `Esc` quitting the directory tree (matching Help / SHORTCUTS) (Issue #51).
- Fixed file-diff mouse wheel scrolling to respect line-wrap physical rows (Issue #52).
- Ignored stale overlapping scan results via a scan generation token (Issue #53).
- Surface scan failures as a status toast instead of exiting the TUI (Issue #54).
- Fixed anchored ignore patterns to match via path components (Windows-safe) (Issue #55).
- Removed the false claim that mouse support can be disabled via settings or `--no-mouse` (Issue #56).
- Updated the README feature list to match shipped capabilities (Issue #57).
- Precise mode no longer treats MD5 read failures as Identical (Issue #60).
- Built-in file diff refuses binary, non-UTF-8, and oversized files (>10 MiB) with a status toast instead of a false empty view (Issue #59).
- Scan no longer follows directory symlinks (cycle-safe); copy keeps destinations under the target root and recreates symlinks instead of traversing them (Issue #58).
- Settings stay under `$HOME/.config/duodiff` (or `%USERPROFILE%\.config\duodiff`), and honor `XDG_CONFIG_HOME` when set (Issue #63).

## [0.3.0] — 2026-07-08

- Added a topic-based Help screen (`?` to open, 5 topics, number-key quick jump) (Issue #28).
- Dropped the redundant `Left:`/`Right:` prefix from the directory tree and diff view pane titles, freeing up space for longer paths (Issue #29).
- Redesigned UI/UX: implemented unified Top Bar, minimal Footer, and unified Palette (Ctrl+p / ;) with Menu & Command modes. Folded About info into Help screen.
- Fixed Help Topic Index list items now selectable by mouse click.
- Removed background colour from the top-bar product name/mode label for a cleaner look.
- Added next/previous change navigation in the file diff view (`N`/`P` or `Alt+Down`/`Alt+Up`) (Issue #30).
- Added `1`/`2` shortcuts in the directory tree to jump focus directly to the left or right pane (Issue #32).
- Added character-level intraline diff highlighting within changed lines in the file diff view (Issue #33).
- Flattened the configuration screen into a single field list, removing the intermediate category menu (Issue #35).
- Added per-hunk copy in the file diff view (`[` copies the change block to the left, `]` to the right) while keeping whole-file `L`/`R` copy (Issue #34).
- Highlight the current diff line and mergeable change blocks in the file diff view (active hunk emphasized for `[` / `]`).

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
