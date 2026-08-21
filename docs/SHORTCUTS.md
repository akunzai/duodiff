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

### Row states

The narrow column between the two panes marks each aligned pair:

| Symbol | Meaning |
| --- | --- |
| `=` | No difference found by the active scan mode. |
| `≈` | **Content unverified** — the bytes were not compared. In Fast mode the sizes match but the timestamps differ; in Precise mode a side could not be read or hashed. Switch to Precise mode (`c`) to resolve it. |
| `≠` | A difference the scan established (a size mismatch, or content that hashed differently). |
| `⬅` / `➡` | Present on the right / left side only. |
| `💥` | One side is a file, the other a directory. |

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
| `L` | **Copy Right to Left**: Copy the selected item (file or folder) from the right pane into the left pane. A preview names the operation (`Create` / `Overwrite` / `Merge`) and both absolute paths before anything is written; `Y`/`Enter` executes, `N`/`Esc` cancels. An already-identical row is a no-op. |
| `R` | **Copy Left to Right**: The same, in the other direction. Directory copies process only the entries this scan listed, so excluded entries (`.git`, …) are never copied implicitly. |

Symlink policy: directory scans **do not follow** symlinks (they appear as leaf entries, which prevents cycles). Copy refuses destinations outside the target root and **recreates** symlinks instead of walking through them.
| `;` / `Ctrl+p` | **Command Palette**: Open the searchable command surface for this screen. |
| `C` | **Settings**: Open the configuration screen to select the active external diff tool. |
| `c` | **Toggle Scan Mode**: Switch between **Fast mode** (size and modification time) and **Precise mode** (content SHA-256 streaming hash). The new mode is persisted to `config.toml` first, then adopted, then one background re-scan starts; if saving fails the previous mode is kept and no re-scan runs. |
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
| `[` | **Stage** the change block under the cursor to the left side. Nothing is written until you save. |
| `]` | **Stage** the change block under the cursor to the right side. Nothing is written until you save. |
| `s` | **Save staged changes**: lists every destination path and asks to confirm, then writes all dirty sides all-or-nothing. The diff stays open. |
| `u` | **Undo** the most recent staged change block. |
| `L` | Copy the whole right file to the left side (shows a preview, then confirms). Blocked while staged changes are unsaved. Lowercase `l` is deliberately unbound here so Directory Tree muscle memory cannot overwrite a file. |
| `R` | Copy the whole left file to the right side (shows a preview, then confirms). Blocked while staged changes are unsaved. Lowercase `r` is deliberately unbound here for the same reason. |
| `w` | **Toggle Line Wrap**: Toggle wrapping of long lines. |
| `f` | **Toggle Context**: Toggle showing full file content vs only changed blocks (collapsed view shows a configurable number of context lines — see Config). |
| `D` | **External Diff**: Compare the same file pair using the configured external diff tool. |
| `E` | **External Editor**: Open the focused side's file in your external editor. |
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
| `Enter` / `Space` | Select the highlighted external diff tool, toggle **Check for updates** / **Mouse support** / **Light theme** / **Scan mode** / **Respect .gitignore**, or open **Global exclusions**. |
| `h` / `Left`, `l` / `Right` *(on the Diff context row)* | Decrease / increase the collapsed-view context radius by 1 line (0–50). |
| `q` / `Esc` | Close Config and return to the screen you opened it from |
| `?` | **Help**: Open the Help screen (opens on the Config topic). |

Settings are saved to `~/.config/duodiff/config.toml` only when you change a value here (first-run auto-detect of a diff tool does not write the file).

**Scan mode** stays editable when Config is opened from the File Diff view; changing it re-scans the tree in the background without closing the diff. When `--scan-mode` overrode the saved value for this session, the row is annotated `session override; saved default: Fast/Precise` until an in-app change brings the two back in sync.

In **Global exclusions**, `a` adds, `Enter` edits, `d` deletes, `r` restores the built-in defaults into the draft (still requires `Ctrl+s`), `J`/`K` reorder, `Ctrl+s` validates, saves, and starts one re-scan, while `Esc` cancels the entire editing session without saving or scanning. The list grows with the terminal and scrolls so the highlighted rule stays visible. The Config screen shows the per-root `.gitignore`/`.duodiffignore` sources and CLI rule count as read-only provenance.

---

## 4. Search & Filtering

| Key | Description |
| --- | --- |
| `/` | **Open Filter Input**: Activates the filter bar at the bottom of the screen. |
| `Ctrl+f` *(In Filter Input)* | Toggle showing **only differing items** (excludes identical files). The toggle is drafted alongside the query: the `[diffs only]` badge updates immediately, but `Enter` commits both together and `Esc` restores both. |
| *Any printable character* *(In Filter Input)* | Typed into the query. While the filter bar is open no unmodified character is a shortcut, so `f`, `T`, and `;` are all searchable. |
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
| **Right Click** | Select the pointed row (Directory Tree) and open the Command Palette. |
| **Double Click** | Open diff view for files, or expand/collapse directory folders. |
| **Mouse Scroll** | Synchronously scroll directory trees or diff lines. Also scrolls the Config screen, Help topic body/index, and the unified menu/palette list; scrolling over the Config screen's **Diff context** row adjusts its value instead of moving the selection. |

---

## 7. Command Palette

`;`, `Ctrl+p`, and right-click all open the same Command Palette. It lists every discrete command available on the current screen for the current selection — continuous scrolling (cursor, page, horizontal) is deliberately left out. Commands that cannot run right now stay listed, greyed out, with the reason they are unavailable, so the inventory does not change shape as you move around.

Each open clears the search box and selects the first available command.

| Key | Description |
| --- | --- |
| `;` / `Ctrl+p` | Open the Command Palette. `Ctrl+p` also closes it again. |
| *Any printable character* | Typed into the search box — `j`, `k`, and `;` included. Matching is a case-insensitive **substring** search over each command's key and label, not fuzzy matching. |
| `Backspace` | Erase one search character. |
| `Up` / `Down` | Move the selection. Long inventories scroll to keep the selection visible. |
| `Enter` | Run the selected command (no-op when it is unavailable). |
| `Esc` | Close the palette. |
| **Mouse** | Wheel scrolls the selection; clicking a visible, available row runs it; clicking `[x]` or outside the popup closes it. |

When the search matches nothing, the palette shows a non-selectable `No matching commands` row.

There are no single-character accelerators inside the palette — the key column is a reminder of each command's direct binding, not a shortcut within the popup.
