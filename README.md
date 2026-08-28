# duodiff

[![CI](https://github.com/akunzai/duodiff/actions/workflows/ci.yml/badge.svg)](https://github.com/akunzai/duodiff/actions/workflows/ci.yml)
[![crates.io](https://badgen.net/crates/v/duodiff)](https://crates.io/crates/duodiff)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Compare and synchronize two directory trees in the terminal.

`duodiff` aligns both trees side by side, marks every pair as identical,
differing, unverified, or present on one side only, and lets you resolve the
difference in place — open a file diff, merge a change block, or copy an entry
across.

![duodiff demo](https://raw.githubusercontent.com/akunzai/duodiff/main/website/demo.gif)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/akunzai/duodiff/main/install.sh | bash
```

Homebrew, Scoop, crates.io, cargo binstall, manual download, and
build-from-source are in [docs/INSTALL.md](docs/INSTALL.md). On Windows use the
[PowerShell installer](docs/INSTALL.md#windows-powershell).

## Quick start

```bash
duodiff <left-dir> <right-dir>
```

Move with `j`/`k` or the arrow keys, `Enter` opens the diff view, `?` opens
Help, and `q` quits.

## Docs

- [Features](docs/FEATURES.md) — what each screen does.
- [Shortcuts](docs/SHORTCUTS.md) — every key and mouse interaction.
- [Configuration](docs/CONFIGURATION.md) — the Config screen and `config.toml`.
- [Contributing](CONTRIBUTING.md) — setup and the verification gate.
