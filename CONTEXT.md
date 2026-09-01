# duodiff

duodiff compares and synchronizes two directory trees through a terminal user
interface.

## Language

**Command**:
A named, discrete user intent that can be triggered through one or more input
paths. Availability, execution semantics, and the user-visible outcome are the
same regardless of how it is triggered.
_Avoid_: shortcut, action

A raw key or mouse gesture is input, not a Command. Text editing, cursor movement,
continuous scrolling, and confirmation choices are also not Commands.

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
