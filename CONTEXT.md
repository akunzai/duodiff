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
