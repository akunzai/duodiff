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

### Actions

| Key | Description |
| --- | --- |
| `Enter` | **Enter Diff View**: Open the built-in side-by-side diff viewer for the selected file pair (non-directory). If it is a directory, it toggles folder expansion. |
| `D` | **External Diff**: Compare the selected file pair using the configured external diff tool (if both left and right files exist). |
| `E` | **External Editor**: Open the selected file in your external editor (defined via `$VISUAL` or `$EDITOR`). |
| `L` | **Copy Right to Left**: Copy the selected item (file or folder) from the right pane into the left pane (prompts for `y/n` confirmation). |
| `R` | **Copy Left to Right**: Copy the selected item (file or folder) from the left pane into the right pane (prompts for `y/n` confirmation). |
| `C` | **Settings Menu**: Open the configuration menu to select the active external diff tool. |
| `c` | **Toggle Scan Mode**: Switch between **Fast mode** (size and modification time) and **Precise mode** (content MD5 streaming hash) and trigger a re-scan. |
| `r` | **Manual Re-scan**: Force a manual re-scan of the comparison directories. |
| `q` / `Esc` | **Quit**: Exit the application. |

---

## 2. File Diff View

This view displays a line-by-line comparison of two files.

| Key | Description |
| --- | --- |
| `j` / `Down` | Scroll diff content down by one line |
| `k` / `Up` | Scroll diff content up by one line |
| `l` / `L` | Copy the right file to the left side (with `y/n` confirmation) |
| `r` / `R` | Copy the left file to the right side (with `y/n` confirmation) |
| `q` / `Esc` | Return to the Directory Tree view |

---

## 3. Search & Filtering

| Key | Description |
| --- | --- |
| `/` | **Open Filter Input**: Activates the filter bar at the bottom of the screen. |
| `f` *(In Filter Input)* | Toggle showing **only differing items** (excludes identical files). |
| `Esc` *(In Filter Input)* | Cancel filter editing and revert to previous pattern. |
| `Enter` *(In Filter Input)* | Commit and apply the filter pattern to the tree view. |
| `Backspace` *(With committed filter)* | Pressing `Backspace` when the filter is not active clears the committed filter pattern. |

---

## 4. Mouse Interactions

`duodiff` has full mouse support enabled by default (can be disabled via settings or using `--no-mouse`).

| Action | Description |
| --- | --- |
| **Left Click** | Select a row (switches pane focus and highlights the active side border in Green). |
| **Right Click** | Select a row and open the floating actions context menu. |
| **Double Click** | Open diff view for files, or expand/collapse directory folders. |
| **Mouse Scroll** | Synchronously scroll directory trees or diff lines. |
