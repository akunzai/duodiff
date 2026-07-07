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
2. Merge to `main` (CI gate green).
3. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Verify: the GitHub release has the binaries, [crates.io](https://crates.io/crates/duodiff) shows the new version (and docs.rs built).
