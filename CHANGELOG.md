# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Diff load errors for binary, oversize, and non-UTF-8 files now truncate paths from the left to retain filenames and append actionable hints to press D for the external diff tool, and status toast messages on narrow screens now truncate cleanly with `…` instead of hard-clipping (Issue #246).
- The directory-tree footer metadata strip now remains visible for identical file pairs so size and timestamp details are always available, and enforces a minimum separating gutter with width-proportional truncation so left and right metadata never collide at narrow terminal widths (Issue #245).
- Overlay panels (command palette, confirm dialogs, and exclusion editor) now pad straddling double-width characters with a space at the boundary so underlying wide characters no longer eat panel borders or leave orphaned continuation cells (Issue #244).
- Diff view pane titles now prefix `[1]` and `[2]` markers to distinguish left and right roots even when long paths truncate to identical segments, and the right pane title reserves space so the `[x]` close button no longer clips the timestamp or path (Issue #243).

- Directory-tree file names that overflow a pane now truncate in the middle with `…`, keeping the prefix and extension instead of clipping the tail with no marker (Issue #242).
- The built-in side-by-side diff now shows 1-based source line numbers and colour-independent change markers (`-`, `+`, blank context, `…` for omitted collapsed ranges). Gutters stay fixed while text wraps or scrolls, drop the numbers on very narrow panes, and hunk copy uses the same source indices so later hunks in collapsed view splice the correct lines (Issue #241).
- File-diff changed lines no longer render with the dim attribute: insert/delete colour stays full intensity, with bold+underline reserved for the characters that actually differ. Context stays muted, so changes read as more prominent than surrounding lines (Issue #240).
- Exclusions are now visible and configurable: a persisted `global_exclusions` list defaults to VCS and desktop-junk paths, while an explicit empty list disables those defaults. Each root now evaluates its own nested `.gitignore` (toggleable with `respect_gitignore` or session-only `--gitignore` / `--no-gitignore`) and `.duodiffignore` rules using standard Git semantics; repeated `--exclude` rules remain session-only and take precedence. Config includes a provenance row and a dedicated global-exclusion editor with validation, reorder, restore-defaults (`r` fills the draft only; `Ctrl+s` still applies), apply, and whole-session cancel behavior; the popup grows with the terminal, a compact key legend replaces the long prose hint, and a long list scrolls to keep the highlighted rule visible; while a pattern is being edited the row uses the selection background and a reverse-video cursor so the insertion point is visible; applying performs one background re-scan. The provenance row abbreviates the current user's home directory as `~` and splits Left / Right / CLI onto wrapped lines so long paths stay readable (Issue #237).
- Copy previews and staged hunk editing now protect unsaved work: hunk changes remain in memory until saved and can be undone; saves detect external changes and offer Reload or Cancel; leaving a dirty diff offers Save, Discard, or Cancel; directory copies use the scanned subtree so excluded entries such as `.git` are never copied. Confirmation dialogs also remain usable in narrow terminals (Issue #235).
- Replaced the split Menu / Command Palette with a single searchable Command Palette. `;`, `Ctrl+p`, and right-click now open the same surface; every open clears the search box and selects the first available command. Plain characters — `j`, `k`, and `;` included — always edit the search, matching is a case-insensitive substring search, and the Menu's single-character immediate execution (with its `c`/`C` ambiguity) is gone. The inventory is complete per screen: theme, pane focus (`Tab`, `1`, `2`), expand/collapse, and the File Diff external diff/editor commands joined it, and `D` / `E` gained matching direct bindings in File Diff. Unavailable commands stay listed with the reason they cannot run instead of disappearing, an empty search shows a `No matching commands` row, the popup clamps to the terminal and scrolls so the selection stays visible past the eighth item, and long labels truncate by display width (Issue #239).
- Scan mode is now a persisted preference (`scan_mode = "fast" | "precise"`, defaulting to `fast` when absent). Switching it with `c`, the palette, or the new Config screen row runs one atomic flow — persist, adopt, then exactly one background re-scan — and a failed save keeps the previous mode with a visible error instead of re-scanning. The new `--scan-mode <fast|precise>` flag overrides the saved value for a single session without writing the config file; Config annotates the row `session override; saved default: …` until an in-app change re-syncs them. Scan mode stays editable when Config is opened from File Diff, and changing it no longer discards the open diff session (Issue #238).
- The filter bar now accepts every printable character, including `f`, `T`, and `;` — filtering for `config`, `footer`, or `Fast` was previously impossible because those keys stayed bound to global shortcuts while typing. The diffs-only toggle moved to `Ctrl+f`, and is drafted alongside the query: the `[diffs only]` badge updates immediately, `Enter` commits the query and the toggle together, and `Esc` restores both to their pre-editing values (Issue #236).
- Added a third directory-tree row state, `≈` (**content unverified**), for pairs whose bytes the active scan mode never compared. In Fast mode a matching size with a different modification time is now `≈` instead of `≠` — comparing a cloud backup against a working copy no longer shows a wall of `≠` for byte-identical files. A Precise-mode read or hash failure is `≈` too, with the reason shown in the selected-row details. Size mismatches and hash-confirmed content mismatches remain `≠`, directories aggregate to `≈` only when their descendants are merely unverified, and diffs-only filtering keeps `≈` rows. `≈` renders in the warning colour with `≠` now bold, so an established difference visually outweighs an unverified one (Issue #232).
- `Esc` in the Directory Tree now clears an applied filter (pattern or diffs-only) instead of quitting the app; it only quits once there is nothing left to dismiss. `q` still quits directly (Issue #233).
- Unbound the lowercase `l` / `r` keys in the File Diff view; whole-file overwrite now requires the uppercase `L` / `R`. In the Directory Tree the lowercase keys expand a directory and force a re-scan, so carrying that muscle memory into the diff view could overwrite an entire file behind a single `y` (Issue #234).

## [0.6.0] — 2026-08-04

- Precise mode and the file-diff info bar now use streaming **SHA-256** instead of MD5 (same hash family as release checksums); the info bar label is `SHA256:`.
- File modification timestamps in the tree detail line and related UI are shown in **UTC** (`YYYY-MM-DD HH:MM:SS UTC`) instead of local time, dropping the `libc` dependency.
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
