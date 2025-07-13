---
title: "Installation"
weight: 1
---

Urx can be installed in several ways, depending on your preference and environment.

### From Cargo

If you have Rust and Cargo installed, you can install Urx directly from [crates.io](https://crates.io/crates/urx):

```bash
cargo install urx
```

### From Homebrew

For macOS users, Urx is available via Homebrew:

```bash
brew install urx
```

### From Source

To build Urx from source, you'll need to have Rust and Cargo installed.

```bash
git clone https://github.com/hahwul/urx.git
cd urx
cargo build --release
```

The compiled binary will be located at `target/release/urx`.

### From Docker

Urx is also available as a Docker image on [GitHub Container Registry](https://github.com/hahwul/urx/pkgs/container/urx).

You can pull the image using the following command:

```bash
docker pull ghcr.io/hahwul/urx:latest
```