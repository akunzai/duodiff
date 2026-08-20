# Keyboard & Mouse Shortcuts Reference

This document provides a comprehensive list of all keyboard shortcuts and mouse interactions available in `duodiff`.

---

## Global

| Key | Description |
| --- | --- |
| `T` | **Toggle Theme**: Switch between the dark (default) and light colour theme, and persist the choice. Works from the Directory Tree, File Diff, Config, and Help screens (not while typing in the filter bar). |

---

## 1. Directory Tree View

This is the main view when launching `duodiff` to compare two directories.

### Navigation

| Key | Description |
| --- | --- |
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `Ctrl+f` | Page selection down (about one screen) |
| `Ctrl+b` | Page selection up (about one screen) |
| `h` / `Left` | Collapse selected directory |
| `l` / `Right` | Expand selected directory |
| `Space` | Toggle directory expansion (collapse if expanded, expand if collapsed) |
| `Tab` | Switch focus between the Left and Right panes |
| `1` | Focus the Left pane directly |
| `2` | Focus the Right pane directly |

### Actions

| Key | Description |
| --- | --- |
| `Enter` | **Enter Diff View**: Open the built-in side-by-side diff viewer for the selected file pair (non-directory). If it is a directory, it toggles folder expansion. |
| `D` | **External Diff**: Compare the selected file pair using the configured external diff tool (if both left and right files exist). |
| `E` | **External Editor**: Open the selected file in your external editor (defined via `$VISUAL` or `$EDITOR`). |
| `L` | **Copy Right to Left**: Copy the selected item (file or folder) from the right pane into the left pane (prompts for `y/n` confirmation). |
| `R` | **Copy Left to Right**: Copy the selected item (file or folder) from the left pane into the right pane (prompts for `y/n` confirmation). |

Symlink policy: directory scans **do not follow** symlinks (they appear as leaf entries, which prevents cycles). Copy refuses destinations outside the target root and **recreates** symlinks instead of walking through them.
| `;` | **Menu**: Open the unified action menu (Menu mode). |
| `Ctrl+p` | **Palette**: Open the command palette with search filtering (Command mode). |
| `C` | **Settings**: Open the configuration screen to select the active external diff tool. |
| `c` | **Toggle Scan Mode**: Switch between **Fast mode** (size and modification time) and **Precise mode** (content SHA-256 streaming hash) and trigger a re-scan. |
| `r` | **Manual Re-scan**: Force a manual re-scan of the comparison directories. |
| `s` | **Swap Directories**: Swap the left and right comparison directories and trigger a re-scan. |
| `q` | **Quit**: Exit the application. |
| `Esc` | **Clear Filter / Quit**: With a filter applied (pattern or diffs-only), clear it and restore the full tree. Only when there is nothing left to dismiss does `Esc` quit. |
| `?` | **Help**: Open the Help screen (opens on the Directory Tree topic). |

---

## 2. File Diff View

This view displays a line-by-line comparison of two text files.

The built-in viewer only accepts UTF-8 text files up to **10 MiB** per side. Binary, non-UTF-8, or oversized files show an error toast — use the external diff tool (`D`) instead.

| Key | Description |
| --- | --- |
| `j` / `Down` | Scroll diff content down by one line |
| `k` / `Up` | Scroll diff content up by one line |
| `Ctrl+f` | Page scroll diff content down (about one screen) |
| `Ctrl+b` | Page scroll diff content up (about one screen) |
| `Left` / `Right` | Scroll diff content horizontally (only when line wrap is disabled) |
| `N` / `Alt+Down` | Jump to the next change block (skips unchanged lines) |
| `P` / `Alt+Up` | Jump to the previous change block (skips unchanged lines) |
| `[` | Copy the change block under the cursor to the left side |
| `]` | Copy the change block under the cursor to the right side |
| `L` | Copy the whole right file to the left side (with `y/n` confirmation). Lowercase `l` is deliberately unbound here so Directory Tree muscle memory cannot overwrite a file. |
| `R` | Copy the whole left file to the right side (with `y/n` confirmation). Lowercase `r` is deliberately unbound here for the same reason. |
| `w` | **Toggle Line Wrap**: Toggle wrapping of long lines. |
| `f` | **Toggle Context**: Toggle showing full file content vs only changed blocks (collapsed view shows a configurable number of context lines — see Config). |
| `C` | **Settings**: Open the Config screen (returns here on `Esc`/`q`). |
| `q` / `Esc` | Return to the Directory Tree view |
| `?` | **Help**: Open the Help screen (opens on the File Diff topic). |

---

## 3. Configuration

Flat settings screen opened with `C` from the Directory Tree, File Diff, or Help screens (or via the top-bar Config link, reachable from anywhere).

| Key | Description |
| --- | --- |
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `Enter` / `Space` | Select the highlighted external diff tool, or toggle **Check for updates** / **Mouse support** / **Light theme** |
| `h` / `Left`, `l` / `Right` *(on the Diff context row)* | Decrease / increase the collapsed-view context radius by 1 line (0–50). |
| `q` / `Esc` | Close Config and return to the screen you opened it from |
| `?` | **Help**: Open the Help screen (opens on the Config topic). |

Settings are saved to `~/.config/duodiff/config.toml` only when you change a value here (first-run auto-detect of a diff tool does not write the file).

---

## 4. Search & Filtering

| Key | Description |
| --- | --- |
| `/` | **Open Filter Input**: Activates the filter bar at the bottom of the screen. |
| `f` *(In Filter Input)* | Toggle showing **only differing items** (excludes identical files). |
| `Left` / `Right`, `Home` / `End` *(In Filter Input)* | Move the edit cursor within the filter text (char-indexed, so multi-byte/CJK text is handled correctly). |
| `Backspace` / `Delete` *(In Filter Input)* | Delete the character before / at the cursor. |
| `Esc` *(In Filter Input)* | Cancel filter editing and revert to previous pattern. |
| `Enter` *(In Filter Input)* | Commit and apply the filter pattern to the tree view. |
| `Backspace` *(With committed filter)* | Pressing `Backspace` when the filter is not active clears the committed filter pattern. |
| `Esc` *(With committed filter)* | Pressing `Esc` when the filter is not active clears the committed filter instead of quitting. |

---

## 5. Help Screen

Press `?` from the Directory Tree, File Diff, or Config views to open a
topic-based help overlay.

| Key | Description |
| --- | --- |
| `1`-`6` | Jump directly to a topic (works in both the topic view and the index list). |
| `Tab` | Open the topic index list. |
| `j` / `k`, `Down` / `Up` | Scroll the current topic's text, or move the selection in the index list. |
| `Enter` *(index list)* | Open the highlighted topic. |
| `C` | **Settings**: Open the Config screen (returns here on `Esc`/`q`). |
| `q` / `Esc` / `?` | Close Help and return to the screen you opened it from. |

---

## 6. Mouse Interactions

`duodiff` has full mouse support enabled by default. Disable it in the Config
screen, set `mouse = false` in `config.toml`, or pass `--no-mouse` to disable
it for one session (there is no `--mouse` flag to force it on).

| Action | Description |
| --- | --- |
| **Left Click** | Select the clicked row. |
| **Right Click** | Select a row and open the unified action menu (Menu mode). |
| **Double Click** | Open diff view for files, or expand/collapse directory folders. |
| **Mouse Scroll** | Synchronously scroll directory trees or diff lines. Also scrolls the Config screen, Help topic body/index, and the unified menu/palette list; scrolling over the Config screen's **Diff context** row adjusts its value instead of moving the selection. |

---

## 7. Unified Menu & Command Palette

You can open the Unified Action Menu or Command Palette from any screen to search and run available commands.

| Key | Description |
| --- | --- |
| `;` | **Menu**: Open the unified action menu (Menu mode) listing all valid context commands. |
| `Ctrl+p` | **Palette**: Open the command palette with interactive fuzzy search filtering (Command mode). |
| `j` / `k` or `Down` / `Up` | Move selection down or up within the menu/palette. |
| `Enter` | Execute the selected command or action. |
| `Esc` | Close the menu/palette and return to the active view. |
| `q` | Close the menu (Menu mode only). |
| `Backspace` | Erase search characters (Command mode only). |
| *Alphanumeric* | **Quick Action** *(Menu mode)*: Pressing any listed action's hotkey executes it immediately.<br>**Search Query** *(Command mode)*: Type to filter the list of commands. |
