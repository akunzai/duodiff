# Installation

Each [release](https://github.com/akunzai/duodiff/releases/latest) attaches prebuilt,
checksummed binaries — no Rust toolchain required.

## Download a prebuilt binary (recommended)

The install script detects your platform, downloads the matching release asset, verifies
its SHA-256 checksum, and installs it into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/akunzai/duodiff/main/install.sh | bash
```

It supports Linux (x86-64/ARM64), macOS (Intel/Apple Silicon), and Windows (x86-64) under
[Git Bash](https://gitforwindows.org)/MSYS2 — on Windows, prefer the native
[PowerShell installer](#windows-powershell) below. Pass `--version <tag>` to pin a release or
`--bin-dir <dir>` to change the install location.

### Manual download

Grab the archive for your platform from the releases page:

| Platform | Asset |
|----------|-------|
| macOS (Apple Silicon) | `duodiff-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `duodiff-<version>-x86_64-apple-darwin.tar.gz` |
| Linux (x86-64) | `duodiff-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (ARM64) | `duodiff-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| Windows (x86-64) | `duodiff-<version>-x86_64-pc-windows-msvc.zip` |

Then extract it and put `duodiff` somewhere on your `PATH`, e.g. on macOS/Linux:

```bash
tar -xzf duodiff-<version>-<target>.tar.gz
install -m 755 duodiff-<version>-<target>/duodiff ~/.local/bin/duodiff
```

## Homebrew (macOS / Linux)

```bash
brew install akunzai/tap/duodiff
```

This installs `duodiff` from the [`akunzai/homebrew-tap`](https://github.com/akunzai/homebrew-tap) tap. 

Homebrew 6.0.0+ requires non-official taps to be trusted before their code runs. The fully-qualified name above trusts only this formula, so no extra step is needed. If you want to use the shorthand flow instead:

```bash
brew tap akunzai/tap
brew trust --formula akunzai/tap/duodiff
brew install duodiff
```

## Windows (PowerShell)

Install natively from PowerShell — this downloads the `x86_64-pc-windows-msvc` build,
verifies its SHA-256 checksum, installs into `~\.local\bin`, and adds that directory to your
user `PATH`:

```powershell
irm https://raw.githubusercontent.com/akunzai/duodiff/main/install.ps1 | iex
```

To pin a release or change the install directory, invoke it as a script block:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/akunzai/duodiff/main/install.ps1))) -Version v0.1.0 -BinDir 'C:\tools\bin'
```

> Don't pipe `install.sh` into `bash` from PowerShell: if WSL is installed, `bash` resolves to
> the WSL launcher and installs the **Linux** binary inside WSL. Use `install.ps1` for a native
> Windows install, or run `install.sh` from [Git Bash](https://gitforwindows.org).

## Scoop (Windows)

With [Scoop](https://scoop.sh), install from the
[`akunzai/scoop-bucket`](https://github.com/akunzai/scoop-bucket) bucket — it handles `PATH` and updates for you:

```powershell
scoop bucket add akunzai https://github.com/akunzai/scoop-bucket
scoop install duodiff
```

## crates.io

With a Rust toolchain, install the published crate from [crates.io](https://crates.io/crates/duodiff):

```bash
cargo install duodiff
```

Or grab the same checksummed release binaries without compiling, via [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall duodiff
```

## Build from source

With a Rust toolchain, install into `~/.cargo/bin` (make sure that directory is on your `PATH`):

```bash
cargo install --path .
```

Or build a release binary and place it yourself:

```bash
cargo build --release
# binary is at target/release/duodiff — copy or symlink it onto your PATH, e.g.
ln -sf "$PWD/target/release/duodiff" ~/.local/bin/duodiff
```
