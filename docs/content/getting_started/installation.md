---
title: "Installation"
weight: 2
---

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

## Next Steps

Once installed, proceed to the [Quick Start](../quick-start) guide to learn basic usage.
