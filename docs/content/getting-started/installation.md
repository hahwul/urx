+++
title = "Installation"
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
```

### From Source

Build from source for the latest development version:

```bash
git clone https://github.com/hahwul/urx.git
cd urx
cargo build --release
```

Binary location: `target/release/urx`

### From Docker

Pull the pre-built Docker image:

```bash
docker pull ghcr.io/hahwul/urx:latest
```

Run with Docker:

```bash
docker run --rm ghcr.io/hahwul/urx:latest example.com
```

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
```

`powershell` and `elvish` are supported too. See
[CLI Options](/guide/cli-options/) for details.

## Next Steps

Once installed, proceed to the [Quick Start](/getting-started/quick-start/) guide to learn basic usage.
