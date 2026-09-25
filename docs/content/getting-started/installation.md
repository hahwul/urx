+++
title = "Installation"
description = "Install urx with Cargo, Homebrew, the AUR, a release binary, Docker or from source, then add shell completions and the man page."
toc = true
weight = 1
+++

## Installation Methods

Urx can be installed through multiple methods depending on your preference and environment.

### From Cargo

The easiest way to install Urx if you have Rust installed:

```bash
cargo install urx
```

### From Homebrew

For macOS and Linux users with Homebrew:

```bash
brew install urx

# or from the project's own tap
brew install hahwul/urx/urx
```

### From the AUR

For Arch Linux, with an AUR helper:

```bash
yay -S urx
# or
paru -S urx
```

### From GitHub Releases

Every [release](https://github.com/hahwul/urx/releases/latest) ships prebuilt
binaries, each with a `.sha256` checksum beside it:

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `urx-X.Y.Z-linux-x86_64.tar.gz` |
| Linux aarch64 | `urx-X.Y.Z-linux-aarch64.tar.gz` |
| macOS Intel | `urx-X.Y.Z-macos-x86_64.tar.gz` |
| macOS Apple Silicon | `urx-X.Y.Z-macos-aarch64.tar.gz` |
| Windows x86_64 | `urx-X.Y.Z-windows-x86_64.zip` |

Unpack the archive and put the `urx` binary somewhere on your `$PATH`.

### From Source

Build from source for the latest development version:

```bash
git clone https://github.com/hahwul/urx.git
cd urx
cargo build --release
```

Binary location: `target/release/urx`

### Optional: Redis Cache Support

The Redis cache backend (`--cache-type redis`) is an optional Cargo feature.
None of the packaged builds above (crates.io, Homebrew, AUR, release binaries,
Docker) include it. Build it in yourself:

```bash
cargo install urx --features redis-cache
# or, from a source checkout
cargo build --release --features redis-cache
```

### From Docker

Pull the pre-built Docker image:

```bash
docker pull ghcr.io/hahwul/urx:latest
```

Run with Docker. The image has no entrypoint, so name the binary (`./urx`)
before its arguments:

```bash
docker run --rm ghcr.io/hahwul/urx:latest ./urx example.com

# piping domains in needs -i
cat domains.txt | docker run --rm -i ghcr.io/hahwul/urx:latest ./urx
```

Besides `latest`, each release is tagged with its version (e.g. `0.11.0`), and
`main` tracks the development branch.

## Verifying Installation

After installation, verify that Urx is working correctly:

```bash
urx --version
```

You should see the version number displayed.

## Shell Completions and Man Page

Urx generates both from the binary, so they always match the version you have
installed:

```bash
# zsh (any directory on your $fpath)
urx --completions zsh > ~/.zfunc/_urx

# bash
urx --completions bash > ~/.local/share/bash-completion/completions/urx

# fish
urx --completions fish > ~/.config/fish/completions/urx.fish

# man page
urx --manpage > ~/.local/share/man/man1/urx.1
man urx
```

`powershell` and `elvish` are supported too. See
[CLI Options](/guide/cli-options/) for details.

## Next Steps

Once installed, proceed to the [Quick Start](/getting-started/quick-start/) guide to learn basic usage.
