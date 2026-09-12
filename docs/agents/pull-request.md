# Pull requests

Write pull request titles, descriptions, and comments in **English**.
**Git commit messages are English**, imperative, subject under 72
characters — they live in history and get searched by tooling. This file
itself stays English throughout, sample blocks included.

`.github/PULL_REQUEST_TEMPLATE.md` is authoritative on the section
structure. What follows only adds what it does not say.

## Preparing

- Work on a feature branch (`feat/...`, `fix/issue-123`, `docs/...`).
  Never prepare a request from `main`.
- **Use a concise descriptive title with no Conventional Commit
  prefix.** One request may carry commits of more than one kind, so a
  single prefix on the title would misdescribe it. The prefix rule in
  CONTRIBUTING.md governs commit subjects, not the request title.
- Apply **exactly one release label**: `enhancement`, `bug`,
  `documentation`, `dependencies`, or `skip-changelog`. Set the
  milestone to the release the change targets. See
  `docs/agents/change-gates.md`.
- **Do not open a pull request, draft included, without the developer
  asking.**

## Description shape

1. A plain-language opening: what changed and why, as a reviewer who did
   not write it would need it.
2. A visual GitHub renders inline, chosen by what changed:

   | Change | Visual |
   | --- | --- |
   | Flow or state transition | Mermaid `flowchart` / `stateDiagram` |
   | Cross-module interaction | Mermaid `sequenceDiagram` |
   | Data model | Mermaid `erDiagram` |
   | TUI appearance | Before/after screenshots |
   | Multi-step interaction | Short recording |
   | Internal or library only | None; test output instead |

   Pair before and after. At most one diagram unless it is such a pair.
   No personally identifiable information in any attachment.
   `docs/agents/verification.md` holds the capture rules.
   When capture is impossible, leave a named placeholder comment:
   `<!-- screenshot pending: after -->`.

   Attach the file with `gh`'s repeatable `--attach` flag, which uploads
   it and embeds it in the body. It works on `gh pr create`,
   `gh pr edit`, `gh pr comment`, and the three `gh issue` equivalents,
   takes up to 50 files per command, and accepts PNG, JPEG, GIF, WebP,
   SVG, MP4, MOV, and WebM, with images capped at 10 MB.

   ```sh
   gh pr create --attach './before.png#Tree before' \
                --attach './after.png#Tree after'
   ```

   Alt text follows the path after `#`; without it the filename is used.
   A path the body already references as `![alt](./after.png)` is
   rewritten in place to point at the uploaded asset, so write the body
   around the images and let `--attach` resolve them.

   Produce the file first. A tcut script renders the TUI to a PNG or GIF
   at a path you choose — never at the committed `website/` paths, see
   `docs/demo.md`. For a before-and-after that needs no image at all,
   paste the two pane renders from `herdr pane read --format text` as
   fenced code blocks: every duodiff row state is legible in monochrome
   by design, so the text carries the same evidence.

   An attached screenshot is not a repo asset, so attach freely. The
   committed `website/` assets are a separate thing with their own rule
   in `docs/agents/change-gates.md`.
3. A collapsed `<details>` technical trailer holding affected paths,
   implementation notes, verification commands, and log excerpts.

## Tests land with the behaviour

- **Product logic**: `src/`. A change here lands with its tests in the
  same request. Tests live in `#[cfg(test)]` modules beside the code
  they cover; shared fixtures live in `src/test_support.rs`. There is no
  top-level `tests/` directory.
- **Exempt**: `docs/`, `website/`, `scripts/`, `.github/`, `*.md`, and
  dependency bumps with no behaviour change.
- **Structurally untestable** code — a terminal-handoff path, a raw-mode
  guard — is declared in the description, naming what covers it instead.

No coverage threshold. The reviewer judges whether the new behaviour is
actually exercised.

## Review readiness

Nothing unverified enters review. Verify locally per
`docs/agents/verification.md`, then open the request with the evidence.
State in the description which paths were verified and which were not,
with the reason.

duodiff has no deployed environment, so the draft-then-deploy order does
not apply here. A change whose only proof is a real terminal — colour,
mouse capture, a specific emulator — is verified by the developer and
recorded as such in the description.
