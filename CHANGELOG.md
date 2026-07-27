# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Fixed the file diff view keeping a scroll position past the end of the content after the terminal is resized, wrap mode is toggled, or a shorter file is opened — the next page-down or arrow key appeared to jump backwards. Viewport geometry is now recomputed once per frame before input is handled, instead of relying on a render pass having already run (Issue #111).
- Fixed the confirm modal (`y/n` copy-overwrite prompt) not trapping mouse input: scrolling or clicking anywhere other than its `[x]` close button while it was open could still scroll the screen underneath it or trigger the top bar's Config/Help buttons (Issue #122).
- Fixed opening the filter bar from the command palette clearing any previously committed filter pattern, unlike the `/` keyboard shortcut which preserves it (Issue #133).

## [0.5.1] — 2026-07-09

- Reworked the Help screen's `About` topic: the repo URL is now read from `Cargo.toml` instead of being hard-coded, the update status only shows a line when a newer version is actually available (with an install-method-aware upgrade command), and the repo link stays clickable while the Help body is scrolled.
- Pressing `?` now jumps straight to the current screen's Help topic body instead of opening the topic index list first; the index is still one `Tab` away.
- Fixed the top bar's `(?)Help` mouse click while already on the Help screen overwriting the remembered return screen, which trapped `Esc`/`q`/`?` in Help with no keyboard way out.
- Added operation hints (`Tab topics`, scroll, `Esc back`) directly to the Help screen's title bar instead of requiring a trip to the General topic to discover them.
- Fixed the `C` Config hotkey not working from the File Diff or Help screens (previously Directory Tree only), even though the top bar's `(C)onfig` hint and mouse click were always available from every screen. Config now also remembers which screen it was opened from and returns there on `Esc`/`q` or a click on the `[x]` close button, instead of always dropping back to the Directory Tree.

## [0.5.0] — 2026-07-09

- Fixed the external editor resuming on stale content by injecting `--wait`/`-w` for known GUI editors (VS Code, Cursor, Zed, Sublime Text, …) that fork and return immediately (Issue #84).
- Added a `mouse` config toggle and `--no-mouse` CLI flag to actually disable mouse support, closing the gap left by Issue #56 (Issue #83).
- Added a light/dark colour theme (`T` to toggle from any screen, persists), replacing hard-coded ANSI colours with a semantic `Theme` palette for readable contrast on light-background terminals (Issue #82).
- Fixed the `/` filter bar's cursor/backspace/insert behaviour for multi-byte text (CJK, emoji) with a char-indexed `TextInput`, and made the edit cursor visible in the filter bar (Issue #85).
- Added a configurable diff context line count (`diff_context`, default 3): the collapsed file diff view now shows a persisted, adjustable number of unchanged lines around each hunk instead of a fixed 3, adjustable from the Config screen (`h`/`l` or `Left`/`Right`) (Issue #86).
- Added an annotated `config.example.toml` and a README Configuration settings table covering every field (Issue #87).
- Fixed the light theme not actually turning the canvas white: `Theme::base_style()` was defined but never applied, so most of the screen kept the terminal's native background colour (Issue #99).
- Added mouse wheel scroll support to the Config screen, Help topic body/index, and the unified menu/palette list (previously only the directory tree and file diff scrolled); scrolling over the Config screen's Diff context row now adjusts its value (Issue #98).
- Refreshed the GitHub Pages landing copy to match shipped workflows (merge, page scroll, palette, help, pane copy).

## [0.4.0] — 2026-07-09

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
- Removed unused scan-progress event/fields that were never driven by the scanner (Issue #64).
- Config screen can toggle daily update checks; first-run tool auto-detect no longer writes `config.toml` until you change a setting (Issue #66).
- Multi-OS CI: `cargo test` on Windows and macOS (Issue #61).
- Removed unused `walkdir` and `crc32fast` direct dependencies (Issue #62).
- After `L`/`R` copy, only the affected directory subtree is re-scanned (falls back to a full scan for root-level items) (Issue #67).
- Extracted keyboard/mouse handling and shared actions out of `main.rs` into `input` / `actions` modules (Issue #65).


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
