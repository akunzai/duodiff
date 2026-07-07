# Contributing to duodiff

## Prerequisites

- Rust (stable toolchain)
- [mise](https://mise.jdx.dev/) (optional, but highly recommended for task running)

## Development Setup

```bash
git clone https://github.com/akunzai/duodiff.git
cd duodiff
cargo build
```

Run TUI on two directories to try it out:

```bash
cargo run -- <left_dir_path> <right_dir_path>
```

## Verification Gate

We use `mise` to enforce project formatting, lints, and test suites. Before submitting commits or PRs, run the following verification checks:

Using `mise` (Recommended):
```bash
mise run check
```

Or using native Cargo commands:
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo check
```

## Building a Release Binary

```bash
cargo build --release
```

The optimized binary is located at `target/release/duodiff`.

## Architecture

See [AGENTS.md](AGENTS.md) for the full architecture guide, conventions, and rules.

## Submitting a PR

1. Fork and create a branch (`feat/my-feature` or `fix/issue-123`).
2. Keep commits focused; follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `chore:`).
3. Open a PR against `main`; the CI gate must be green.
4. **Label the PR** so it lands in the right release-note section: `enhancement` (🚀 Features), `bug` (🐛 Bug Fixes), `documentation` (📚 Documentation), `dependencies` (⬆️ Dependencies), or `skip-changelog`.

## Reporting Issues

Use GitHub Issues to report bugs or request new features.
