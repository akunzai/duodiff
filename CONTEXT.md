# duodiff

duodiff compares and synchronizes two directory trees, or two files, through a
terminal user interface.

## Language

**Command**:
A named, discrete user intent that can be triggered through one or more input
paths. Availability, execution semantics, and the user-visible outcome are the
same regardless of how it is triggered.
_Avoid_: shortcut, action

A raw key or mouse gesture is input, not a Command. Text editing, cursor movement,
continuous scrolling, and confirmation choices are also not Commands.

**Gesture**:
A fixed navigation key a screen answers before any binding: moving or paging the
selection, scrolling, expanding a directory with Space, adjusting a Config value,
jumping to a Help topic, and opening the Command Palette. A Gesture cannot be
remapped, and a Command cannot be bound to its key.
_Avoid_: shortcut, navigation command

Keys a screen takes only in some states — Enter on a directory, Esc or Backspace
while a filter is applied — are not Gestures; they stay bindable.

**Compared pair**:
The two sides File Diff shows and the copy, external diff, and editor Commands
act on: the two files of a session started on a file pair, or the selected
Directory Tree row under each root. It says what each side is — a file,
nothing, read-only, read from a pipe — so every Command asks one place.
_Avoid_: current file, selected pair, file pair (the command-line pair only)

The comparison target is the session's: two directories or one file pair
(ADR-0004). The Compared pair is what that target puts in front of the user now.

**Directory Tree**:
The two roots aligned into one tree, together with what the user did to it:
which directories are expanded, the filter over its rows, and the cursor into
what is listed. A scan produces the tree; the Directory Tree adopts it and keeps
the user's expand choices across every rescan.
_Avoid_: scan (the background work that produces the tree), tree list

**Settings**:
The preferences a session runs with — the external diff tool, the update
check, mouse support, theme, diff context, scan mode, and what a scan leaves
out. Each is saved in the config file, and some start from a command-line
flag instead. A change takes effect at once and is saved; one that cannot be
saved still lasts until duodiff exits.

A flag only sets where the session starts. Changing that setting in the app
replaces the flag for the rest of the session, as a change to any other
setting would.
_Avoid_: config (the Config screen, or the file), options, preferences

**Display width**:
The column count a string occupies in the terminal: each character's Unicode
display width, with a tab counted as four columns. Line breaking measures in it,
so a wrapped row is the same height to the code that clamps scrolling and to the
code that paints it.
_Avoid_: column width, cell width, character count

Byte and character counts are not display width; a double-width character
occupies two columns. Truncation of paths, breadcrumbs, and chips still measures
a tab as zero columns — those strings do not contain tabs, and the two
conventions have not been merged.
