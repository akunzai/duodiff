# Centralize Command semantics behind one interface

**Status**: accepted

duodiff defines a **Command** as a named, discrete user intent, independent of
the keyboard, mouse, Command Palette, or Help-link adapter that triggers it. One
deep Commands module owns inventory membership, availability, execution-time
revalidation, effect orchestration, confirmations, and canonical outcomes behind
`inventory` and `execute`; this concentrates change and verification instead of
repeating Command semantics across adapters.

## Considered options

- A runtime registry was rejected because duodiff has no runtime Command
  extension requirement; a closed vocabulary preserves exhaustive checking.
- Adapter-owned execution was rejected because it recreates the message and
  behavior drift this decision resolves.
- Low-level input was rejected from the Command vocabulary: text editing,
  cursor movement, continuous scrolling, modal capture, and confirmation choices
  remain adapter concerns.

## Consequences

- Keyboard bindings have one source of truth in the keyboard adapter; Palette
  and Help presentation reuse its display hints.
- The Commands module and App state are owned separately by the event loop.
- Filesystem and scan seams remain private and local-substitutable. Terminal
  handoff has production and test adapters and is borrowed during execution.
- Tests exercise inventory and execution through the Commands interface;
  adapter tests cover input mapping and outcome presentation rather than repeat
  Command semantics.
- Back and Quit remain distinct Commands, and Expand and Collapse describe
  explicit target states.
