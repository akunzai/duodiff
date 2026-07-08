# duodiff - Agent Guidelines

## Quick Commands
- Build: `cargo build`
- Test: `cargo test`
- Run: `cargo run -- <left_dir> <right_dir>`
- Lint/Format: `cargo clippy && cargo fmt`
- Demo GIF: `mise run demo` (outputs to `website/demo.gif`)

## Architecture Overview
- `src/main.rs`: Entry point, CLI parsing, terminal configuration, and async event loop.
- `src/app.rs`: Application state (`App`, `FlatRow`, `ViewMode`, double-click state) and scrolling logic.
- `src/diff.rs`: Side-by-side directory diff scanner and alignment algorithm.
- `src/diff_view.rs`: Text line-by-line diff computation using the `similar` crate.
- `src/ui.rs`: Layout rendering, widget drawing, path title truncation, and focus pane highlight.
- `src/event.rs`: Tokio-based input listener (keys, mouse, ticks).
- `src/diff_tool.rs`: External diff tool detection (vim, nvim, code, meld, bcomp, smerge, ksdiff, difft) and subprocess diff/editor launcher.
- `src/settings.rs`: Application configuration persistence (`config.toml`).
- `website/`: Landing page (`index.html`) and demo animation (`demo.gif`) — deployed to GitHub Pages via `.github/workflows/deploy-pages.yml`.
- `docs/`: Reference documentation (`INSTALL.md`, `SHORTCUTS.md`) — linked from `README.md` and the landing page.


## Code Style & Conventions
- **Clean Terminal Recovery**: Ensure that raw mode is disabled and the alternate screen is exited unconditionally upon app termination, errors, or startup failures. Wrap the event loop inside the safe `run_app` helper.
- **External Diff Tool / Editor Diffing**: When launching external diff tools or editors for file comparisons/editing, temporarily disable raw mode and exit the alternate screen before spawning the process, and restore terminal states immediately afterwards to prevent character corruption or TTY hangs.
- **O(N) Render Optimization**: Render layouts from the flat cache `app.flat_rows` to prevent $O(N^2)$ recursive searches during draw ticks.
- **Diff View Caching**: Calculate and cache file differences in `app.diff_rows` once when entering `ViewMode::FileDiff`. Do not read files or run diffs inside the draw loop.
- **Focus Highlighting**: Highlight the active panel's borders dynamically in green based on `app.active_side_left`.
- **File/Directory Sync (Copying)**: Copying files/directories between panes uses recursive filesystem helpers (`copy_dir_recursive`) and triggers a full background scanner re-scan immediately upon confirmation. While the confirmation modal is active (`app.show_confirm_modal`), all key and mouse events must be intercepted and routed to the modal handler.

## Lessons Learned
- **TTY Test Hangs**: Crossterm raw mode transitions and alternate screen actions can hang or crash standard cargo tests in non-TTY environments (e.g., CI). Always wrap TUI setup and cleanup calls in `std::io::stdout().is_terminal()` guards.
- **Cross-Platform Mocking**: On Windows, mocking `$EDITOR` using `"true"` fails since it is not a standard executable. Use `"cargo --version"` instead, which exits immediately and exists cross-platform.
- **Space-Containing Paths**: `$EDITOR` variables can contain space-delimited arguments (e.g., `code --wait`). Split the environment variable by whitespace to extract arguments correctly before launching the command.
- **Environment Mutating Tests**: Modifying process-wide environment variables (e.g., `$EDITOR` or `$VISUAL`) concurrently in tests causes race conditions. Acquire the process-wide `crate::diff_tool::TEST_MUTEX` lock to serialize any tests mutating the environment.

## Conventions
- Commit messages: Conventional Commits, in English (e.g. `feat:`, `docs:`, `fix:`).
- Fold same-scope follow-up fixes into the original commit (amend) rather than adding `fix typo` / `review fix` commits.
- Every PR MUST carry a release-note category label (`enhancement`, `bug`, `documentation`, `dependencies`, or `skip-changelog`) — GitHub groups auto-generated release notes by these via `.github/release.yml`.
- When a change adds or alters a user-facing key, screen, or feature, update `docs/SHORTCUTS.md` and the in-app `?` Help screen content (`help_topic_body` in `src/ui.rs`) **in the same PR** — keep docs and behavior in lockstep.
- Any user-facing feature or bug fix MUST add a concise bullet under the `## [Unreleased]` section of `CHANGELOG.md` **in the same PR**. Keep it one line, summarising the user-visible effect. Changes labelled `skip-changelog` or purely internal (dependency bumps, refactors, test-only, typo fixes) do not need an entry.
- Versioning (SemVer): stay on `0.x` while the keymap/feature surface is still evolving; only cut `1.0.0` once it has gone several releases without a breaking UX change. A release is a `vX.Y.Z` tag matching `Cargo.toml`, which triggers `.github/workflows/release.yml` to build and attach the platform binaries the install scripts expect.
- Release flow: bump `Cargo.toml` to the next version **when starting** the first new feature after a release — this keeps the in-development build distinct from the published one. Land changelog entries under `## [Unreleased]` during development (do **not** stamp a version or date on them yet). Only at the actual release does the `## [Unreleased]` heading get renamed to `## [X.Y.Z] — YYYY-MM-DD` (see `RELEASING.md`); that is also when the `vX.Y.Z` tag is cut. So a version bump alone (no changelog version/date) is the normal mid-cycle state, not an oversight.

## Claude Code Compatibility

> [!NOTE]
> This repository maintains compatibility with Claude Code. The file `CLAUDE.md` is a symbolic link pointing to `AGENTS.md`. 
> All commands, style guides, and workflows defined in `AGENTS.md` apply to both Antigravity (and other agentic assistants) and Claude Code.
> **DO NOT** delete the `CLAUDE.md` symbolic link or edit it independently; all guidelines must be updated directly in `AGENTS.md`.
