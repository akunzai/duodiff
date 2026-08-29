# Product Design

Long-term principles for duodiff's TUI surfaces, user-visible strings, and the
README.

## Product thesis

duodiff is a two-pane instrument for answering one question — *how do these two
directory trees differ, and what do I want to do about it* — without leaving the
terminal.

Users should always be able to tell what the scan actually verified, which side
an entry lives on, and what an action will change before they run it. The
experience reads as structure, not decoration.

## Voice

Write like an opinionated operator with the restraint of a craftsperson.

- Use active voice, sentence case, concrete verbs, and stable product terms.
- Put personality in the README and Help topics, not in errors or confirmations.
- Keep toasts, confirmations, footers, and non-TTY output functional.
- State a recovery or next action only when it helps the user continue.
- Keep an action's verb consistent from Help through palette through toast: a
  change block is **staged**, then **saved**; an entry is **copied**.

Avoid emoji, emotional reactions, stacked punctuation, filler, and claims that
the tool is fast, fluid, or effortless. Report what it did.

## Product language

Use the terms the user sees on screen:

- **Directory Tree**, **File Diff**, **Config**, **Help** are the four screens.
  One name each — not "Config menu" and "Configuration" for the same screen.
- **Left pane** and **right pane**, never "pane 1" outside the `[1]`/`[2]` key
  hints.
- **Scan mode** is Fast or Precise. **Unverified** means the bytes were not
  compared — it is not a synonym for identical.
- **Stage** a change block; **save** the staged sides. **Copy** moves a whole
  entry or file between panes.
- **Exclusion** covers global rules, `.gitignore`, and `.duodiffignore` alike.

## Screens

Every screen carries the same three regions: a top bar naming the screen, the
content, and a footer. Hints live on borders and in the footer, never as a
separate legend block. A screen's contextual title states the keys that apply to
the current row, and drops units from the right as the terminal narrows.

Modals capture all input while open. A confirm dialog leads with one emphasized
sentence naming the consequence, then the paths it applies to, then the ways
out. Every choice is a chip carrying its own key, the default choice is drawn
filled rather than left for the user to guess, and the chips wrap onto another
line before they clip — a dialog the user cannot leave is worse than an ugly
one. Text never touches the frame: the popup pads its body on all four sides,
and a wrapped path continues in its own column rather than restarting under its
label.

## Marks and terminal capability

The tree uses single-width, text-presentation Unicode marks so both panes stay
column-aligned at any width:

```text
=  identical    ≈  unverified   ≠  differing
<  left only    >  right only   !  type conflict
▸  collapsed    ▾  expanded     Aa case-only mismatch
```

Directories carry a trailing `/`, which survives truncation and breadcrumbs when
the disclosure marker does not apply. Files carry no icon; two columns of
indentation keep them aligned with their siblings.

Emoji are not marks. They are double-width, render inconsistently across
terminals and fonts, and break the alignment the two-pane layout depends on. Use
a widely supported Unicode mark when its meaning is clear, and a short text
label otherwise.

This rule covers what duodiff renders and what its docs describe. GitHub release
notes are outside it: the section emoji in the release-drafter categories
(🚀 Features, 🐛 Bug Fixes, 📚 Documentation, ⬆️ Dependencies) are a platform
convention and stay.

Colour reinforces status or hierarchy but never carries meaning alone — every
row state is readable in monochrome. Honor `NO_COLOR`; emit no ANSI styling for
non-TTY output.

## Output

A completed action leaves one durable result. Toasts state what happened, not
what is happening:

```text
Saved staged changes
Copied 'notes.txt'
```

Errors use the smallest useful subset:

```text
Error: what could not be completed
Cause: why it failed, when known
Next: the concrete recovery action, when available
```

The Command Palette lists every command the active screen has, including the
ones that cannot run right now, each carrying the reason. The inventory does not
change shape with the selection.

## README

The README's first job is to get a new user from install to one successful
comparison within 60 seconds. Its second job is to explain why the tool exists.

Use a plain heading, a short value proposition, one install path, one quick-start
path, and one demo. Keep the feature tour, the key reference, and the settings
table in focused docs. Document released behavior, not plans.

The demo uses fixed fixture data, dimensions, theme, timing, and output paths,
and must not expose local usernames or home paths. Re-record it once per release
— see [RELEASING.md](../RELEASING.md), not per issue or PR.
