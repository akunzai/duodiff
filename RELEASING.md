# Releasing

The maintainer/owner runbook for cutting a duodiff release. Contributors don't need any of this — see [CONTRIBUTING.md](CONTRIBUTING.md).

## How a release works

A release is a `vX.Y.Z` git tag that matches `Cargo.toml`'s `version`. Pushing the tag triggers the full pipeline:

- `.github/workflows/release.yml` — builds and attaches the platform binaries to the GitHub Release.
- `.github/workflows/publish.yml` — publishes the crate to [crates.io](https://crates.io/crates/duodiff).

The crate is published once the `CARGO_REGISTRY_TOKEN` secret is configured, so the publish step runs automatically on tag.

Packaging stays lean via `Cargo.toml` `exclude` (the CI config, install scripts, and docs are kept out of the published tarball); `cargo publish --dry-run` validates the tarball.

## Cutting a release

1. Bump `version` in `Cargo.toml` (and refresh `Cargo.lock` by running a build); confirm `cargo publish --dry-run` is clean.
2. In `CHANGELOG.md`, retitle the `## [Unreleased]` entries under a dated `## [X.Y.Z] — YYYY-MM-DD` heading, and add back an empty `## [Unreleased]` heading above it as a placeholder for the next cycle.
3. Refresh website visuals only if a change since the last recording is both user-visible and appears in the recorded flow (`docs/demo.md`'s storyboard or `website/tree-view.png`) — e.g. a renamed screen, a changed row mark, or altered on-screen text the storyboard actually triggers. Skip it for changes that are real but invisible in what's recorded (an internal behavior fix, a toast the storyboard never hits, docs-only changes). When warranted: `mise run demo` (rewrites `website/demo.gif` and `website/*.png`). Per-issue PRs must not land these assets regardless.
4. Merge to `main` (CI gate green).
5. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
6. Verify: the GitHub release has the binaries, [crates.io](https://crates.io/crates/duodiff) shows the new version (and docs.rs built).
7. Close the `X.Y.Z` milestone, and open the one for the next version so incoming issues and PRs have a milestone to land on — see [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md).
   Move any still-open issue off the milestone being closed onto the next one first: `gh issue list --milestone X.Y.Z --state open`.
