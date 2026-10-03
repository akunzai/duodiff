# Releasing

The maintainer/owner runbook for cutting a duodiff release. Contributors don't need any of this — see [CONTRIBUTING.md](CONTRIBUTING.md).

## How a release works

A release is a `vX.Y.Z` git tag that matches `Cargo.toml`'s `version`. Pushing the tag triggers `.github/workflows/release.yml`, which runs these steps in order, each only if the one before succeeded:

1. Checks that the tag matches `Cargo.toml`'s version and runs the `mise run check` gate on the tagged commit.
2. Builds the platform binaries and attests their build provenance.
3. Waits for approval of the `release` environment, then creates the GitHub Release with the binaries attached and updates the Scoop manifest and Homebrew formula.
4. Publishes the crate to [crates.io](https://crates.io/crates/duodiff) from the `crates-io` environment, with no second approval: it starts only after the approved release job has succeeded. If only this step fails, re-run the failed job from the tag's workflow run; the release already exists.

The crate is published only while the `CARGO_REGISTRY_TOKEN` secret is configured; without it the publish step skips itself and succeeds.

Packaging stays lean via `Cargo.toml` `exclude` (the CI config, install scripts, and docs are kept out of the published tarball); `cargo publish --dry-run` validates the tarball.

## Release notes

[GitHub Releases](https://github.com/akunzai/duodiff/releases) hold the published change history. Unreleased work is tracked in PRs and commits. The former `CHANGELOG.md` remains available in Git history; existing Releases are not backfilled from it.

The release workflow generates notes from merged PR titles, grouped by the labels in
`.github/release.yml`. Review titles, labels, and direct commits using the procedure below.

After the release PR is merged, preview notes for the merged `main` before tagging.
Replace `vX.Y.Z` and `vPREVIOUS` with the proposed and previous release tags:

```sh
gh api --method POST repos/{owner}/{repo}/releases/generate-notes \
  -f tag_name=vX.Y.Z \
  -f target_commitish=main \
  -f previous_tag_name=vPREVIOUS \
  --jq .body
```

The [generate-notes API](https://docs.github.com/en/rest/releases/releases#generate-release-notes-content-for-a-release)
returns a preview without saving a release or draft; the workflow generates the final
notes at the tag. Check that the comparison starts at the intended previous release,
PR titles describe the user-visible changes, and labels place them in the right sections.
`skip-changelog` excludes a PR from the notes. Correct misleading titles or labels and
regenerate the preview before tagging.

After fetching `origin`, compare the preview with
`git log --oneline vPREVIOUS..origin/main`. Changes committed directly to `main` have no
merged PR entry: identify missing user-visible changes before tagging, then add their
entries to the published release notes during verification. Add a highlights summary
when it helps readers. [Immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
lock the tag and assets, but still allow editing the title and release notes.

## Cutting a release

1. On a `release/X.Y.Z` branch, bump `version` in `Cargo.toml` and refresh `Cargo.lock` with a plain `cargo build` — `--locked` refuses the version change. Confirm the tarball with `cargo publish --dry-run --allow-dirty` before committing (or without `--allow-dirty` after).
2. Review the release's merged PR titles and labels, and check direct commits for user-visible changes that need a manual release-note entry — see Release notes above.
3. Refresh website visuals only if a change since the last recording is both user-visible and appears in the recorded flow (`docs/demo.md`'s storyboard or `website/tree-view.png`) — e.g. a renamed screen, a changed row mark, or altered on-screen text the storyboard actually triggers. Skip it for changes that are real but invisible in what's recorded (an internal behavior fix, a toast the storyboard never hits, docs-only changes). When warranted: `mise run demo`. It rewrites `website/demo.gif` and `website/*.png` every time, so keep only the files whose picture changed and restore the rest (`git checkout -- website/tree-view.png`): a re-recorded still differs by a few timestamp and anti-aliasing pixels even when nothing on screen changed. To check a frame, compare it with the committed one, e.g. the GIF's last frame: `git show HEAD:website/demo.gif > old.gif && magick old.gif -coalesce -delete 0--2 old.png` (same for the new GIF), then look at the two, or `magick compare -metric AE old.png new.png null:` for the stills. Per-issue asset updates follow the exception in `docs/agents/change-gates.md` when a change makes the committed assets wrong.
4. Open a pull request from `release/X.Y.Z` and merge it to `main` once the CI gate is green; the tag is cut from the merged `main`.
5. On the merged `main`, preview the release notes as described above, then tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
   Then open the tag's Release run in the Actions tab. Once the gate, builds, and attestation are green, approve the `release` deployment under **Review deployments**. That is the only approval; nothing leaves the repository before it. `release` (with a required reviewer) holds `HOMEBREW_BUMP_TOKEN`; `crates-io` (no reviewer) holds `CARGO_REGISTRY_TOKEN`. Both admit only tags matching `v*`.
6. Verify: the GitHub release has the binaries and the expected notes; add the missing entries
   identified during review or a highlights summary when useful. Confirm [crates.io](https://crates.io/crates/duodiff) shows the new version. docs.rs lists the version too, but duodiff is binary-only, so there is no API documentation to build (`https://docs.rs/crate/duodiff/X.Y.Z/status.json` reports `doc_status: false`), and the page can take a quarter of an hour to appear. Neither is a release failure.
   Check that `Formula/duodiff.rb` and `bucket/duodiff.json` have the new version and a
   `chore: bump duodiff to vX.Y.Z` commit on the tap's and bucket's `main`. If either is
   missing, inspect the run's "Update Homebrew formula" or "Update Scoop manifest" step
   and confirm `HOMEBREW_BUMP_TOKEN` is configured and valid.
7. Create the next version's milestone if it does not exist, with a one-line description —
   see [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md). Move still-open issues
   from the released milestone to it (`gh issue list --milestone X.Y.Z --state open`).
   Update the released milestone's description with a concise summary of shipped
   highlights derived from GitHub release notes, then close it. A PR merged after the tag
   belongs to the next milestone.
