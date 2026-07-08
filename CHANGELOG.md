# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-07-08

- **Update Checker & Self-Upgrade (Issue #12)**:
  - Added the `--upgrade` and `--upgrade-version` CLI options to easily self-upgrade prebuilt release binaries.
  - Implemented automatic background check for updates (once per day) against the GitHub Releases API, displaying a polite upgrade hint in the TUI footer border.
  - Supported dynamically upgrading via Homebrew or Scoop if managed by package managers, or direct in-place replacement for standalone installs.
  - Added the `check_updates` settings configuration parameter.
- **Automated Demo Recording (Issue #13)**:
  - Added a scripted recording harness under `scripts/demo/` using `asciinema` and `agg` to generate a high-quality, reproducible `website/demo.gif`.
  - Added a `demo` task to `mise.toml` to automate the recording and rendering workflow.
- **Documentation & Landing Page**:
  - Extracted the TUI keyboard and mouse shortcuts table from `README.md` into an independent reference document `docs/SHORTCUTS.md`.
  - Designed and created a modern, responsive landing page in `website/index.html` for GitHub Pages.
- **Aligned Diff View & Line Endings Normalization (Issue #26)**:
  - Redesigned the in-app side-by-side diff view to align replacement modifications (deletions and insertions) side-by-side on the same rows rather than stacking them vertically.
  - Implemented CRLF (`\r\n`) to LF (`\n`) normalization in file comparison to ignore line ending differences.
  - Added line ending format detection and style indicators (`[LF]` or `[CRLF]`) to each file pane in the diff view's Info bar.
- Added automated installer scripts: `install.sh` for Unix-like systems and `install.ps1` for Windows (PowerShell).
- Updated package metadata in `Cargo.toml` for crates.io publishing and `cargo-binstall` support.

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
