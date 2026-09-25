# Releasing

The maintainer/owner runbook for cutting a duodiff release. Contributors don't need any of this — see [CONTRIBUTING.md](CONTRIBUTING.md).

## How a release works

A release is a `vX.Y.Z` git tag that matches `Cargo.toml`'s `version`. Pushing the tag triggers the full pipeline:

- `.github/workflows/release.yml` — builds and attaches the platform binaries to the GitHub Release.
- `.github/workflows/publish.yml` — publishes the crate to [crates.io](https://crates.io/crates/duodiff).

The crate is published once the `CARGO_REGISTRY_TOKEN` secret is configured, so the publish step runs automatically on tag.

Packaging stays lean via `Cargo.toml` `exclude` (the CI config, install scripts, and docs are kept out of the published tarball); `cargo publish --dry-run` validates the tarball.

## Cutting a release

1. On a `release/X.Y.Z` branch, bump `version` in `Cargo.toml` and refresh `Cargo.lock` with a plain `cargo build` — `--locked` refuses the version change. Confirm the tarball with `cargo publish --dry-run --allow-dirty` before committing (or without `--allow-dirty` after).
2. In `CHANGELOG.md`, retitle the `## [Unreleased]` entries under a dated `## [X.Y.Z] — YYYY-MM-DD` heading, and add back an empty `## [Unreleased]` heading above it as a placeholder for the next cycle.
3. Refresh website visuals only if a change since the last recording is both user-visible and appears in the recorded flow (`docs/demo.md`'s storyboard or `website/tree-view.png`) — e.g. a renamed screen, a changed row mark, or altered on-screen text the storyboard actually triggers. Skip it for changes that are real but invisible in what's recorded (an internal behavior fix, a toast the storyboard never hits, docs-only changes). When warranted: `mise run demo`. It rewrites `website/demo.gif` and `website/*.png` every time, so keep only the files whose picture changed and restore the rest (`git checkout -- website/tree-view.png`): a re-recorded still differs by a few timestamp and anti-aliasing pixels even when nothing on screen changed. To check a frame, compare it with the committed one, e.g. the GIF's last frame: `git show HEAD:website/demo.gif > old.gif && magick old.gif -coalesce -delete 0--2 old.png` (same for the new GIF), then look at the two, or `magick compare -metric AE old.png new.png null:` for the stills. Per-issue PRs must not land these assets regardless.
4. Open a pull request from `release/X.Y.Z` and merge it to `main` once the CI gate is green; the tag is cut from the merged `main`.
5. On the merged `main`, tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
6. Verify: the GitHub release has the binaries, and [crates.io](https://crates.io/crates/duodiff) shows the new version. docs.rs lists the version too, but duodiff is binary-only, so there is no API documentation to build (`https://docs.rs/crate/duodiff/X.Y.Z/status.json` reports `doc_status: false`), and the page can take a quarter of an hour to appear. Neither is a release failure.
7. Update the `X.Y.Z` milestone's description with a concise summary of shipped highlights (derived from `CHANGELOG.md`), close it, and open the one for the next version so incoming issues and PRs have a milestone to land on — see [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md).
   Move any still-open issue off the milestone being closed onto the next one first: `gh issue list --milestone X.Y.Z --state open`.

