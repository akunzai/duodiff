# Keyboard & Mouse Shortcuts Reference

This document provides a comprehensive list of all keyboard shortcuts and mouse interactions available in `duodiff`.

---

## 1. Directory Tree View

This is the main view when launching `duodiff` to compare two directories.

### Navigation

| Key | Description |
| --- | --- |
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
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
| `;` | **Menu**: Open the unified action menu (Menu mode). |
| `Ctrl+p` | **Palette**: Open the command palette with search filtering (Command mode). |
| `C` | **Settings**: Open the configuration screen to select the active external diff tool. |
| `c` | **Toggle Scan Mode**: Switch between **Fast mode** (size and modification time) and **Precise mode** (content MD5 streaming hash) and trigger a re-scan. |
| `r` | **Manual Re-scan**: Force a manual re-scan of the comparison directories. |
| `q` / `Esc` | **Quit**: Exit the application. |
| `?` | **Help**: Open the Help screen (opens on the Directory Tree topic). |

---

## 2. File Diff View

This view displays a line-by-line comparison of two files.

| Key | Description |
| --- | --- |
| `j` / `Down` | Scroll diff content down by one line |
| `k` / `Up` | Scroll diff content up by one line |
| `N` / `Alt+Down` | Jump to the next change block (skips unchanged lines) |
| `P` / `Alt+Up` | Jump to the previous change block (skips unchanged lines) |
| `l` / `L` | Copy the right file to the left side (with `y/n` confirmation) |
| `r` / `R` | Copy the left file to the right side (with `y/n` confirmation) |
| `q` / `Esc` | Return to the Directory Tree view |
| `?` | **Help**: Open the Help screen (opens on the File Diff topic). |

---

## 3. Configuration

Flat settings screen opened with `C` from the Directory Tree (or via the top-bar Config link).

| Key | Description |
| --- | --- |
| `j` / `Down` | Move selection down |
| `k` / `Up` | Move selection up |
| `Enter` / `Space` | Select the highlighted external diff tool |
| `q` / `Esc` | Return to the Directory Tree view |
| `?` | **Help**: Open the Help screen (opens on the Config topic). |

---

## 4. Search & Filtering

| Key | Description |
| --- | --- |
| `/` | **Open Filter Input**: Activates the filter bar at the bottom of the screen. |
| `f` *(In Filter Input)* | Toggle showing **only differing items** (excludes identical files). |
| `Esc` *(In Filter Input)* | Cancel filter editing and revert to previous pattern. |
| `Enter` *(In Filter Input)* | Commit and apply the filter pattern to the tree view. |
| `Backspace` *(With committed filter)* | Pressing `Backspace` when the filter is not active clears the committed filter pattern. |

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
| `q` / `Esc` / `?` | Close Help and return to the screen you opened it from. |

---

## 6. Mouse Interactions

`duodiff` has full mouse support enabled by default (can be disabled via settings or using `--no-mouse`).

| Action | Description |
| --- | --- |
| **Left Click** | Select the clicked row. |
| **Right Click** | Select a row and open the unified action menu (Menu mode). |
| **Double Click** | Open diff view for files, or expand/collapse directory folders. |
| **Mouse Scroll** | Synchronously scroll directory trees or diff lines. |
