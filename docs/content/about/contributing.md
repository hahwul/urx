+++
title = "Contributing"
description = "Report issues, set up a development environment and open a pull request against urx."
toc = true
weight = 2
+++

## Reporting Issues

Search [existing issues](https://github.com/hahwul/urx/issues) first, then
[open a new one](https://github.com/hahwul/urx/issues/new) with:

- `urx --version` output and your OS
- the command you ran
- expected vs actual output

## Development Setup

You need Rust (latest stable) and Git. Fork the repo, then:

```bash
git clone https://github.com/YOUR_USERNAME/urx.git
cd urx
cargo build
cargo test
```

urx is a binary crate: use `cargo test` (or `cargo test --bin urx`).
`cargo test --lib` fails.

## Before Opening a PR

Run what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

`--all-targets --all-features` also lints the tests and the optional
`redis-cache` code, which a plain `cargo clippy` skips. CI uses the latest
stable toolchain, so new lints can show up there first.

With [just](https://github.com/casey/just), `just test` runs tests, clippy (code
and tests), the format check and `cargo doc`. `just fix` formats and applies
clippy's automatic fixes.

Keep each PR to one change, add tests for new behaviour, reference related
issues, and note breaking changes. Update `CHANGELOG.md` for significant changes.

## Documentation

Docs live in `docs/content/` (Markdown with TOML front matter) and are built
with [Hwaro](https://github.com/hahwul/hwaro):

```bash
just docs-dependencies   # install Hwaro (macOS, via Homebrew)
just docs-serve          # preview at http://localhost:3000
```

When you add or change a flag or config key, update both `README.md` and the
pages under `docs/content/guide/`. The option blocks on
[CLI Options](/guide/cli-options/) and in the README are copied by hand from
`urx --help`, so diff them against the built binary's output. New config keys
also go in `example/config.toml` and [Configuration](/guide/configuration/).

## Project Structure

```
urx/
├── src/                # Source code
│   ├── main.rs         # Entry point
│   ├── app/            # Run orchestration; catalog.rs is the provider registry
│   ├── cli/            # CLI argument parsing
│   ├── config/         # TOML config and provider-config loading
│   ├── providers/      # URL data providers
│   ├── filters/        # URL filtering, presets, scope files
│   ├── testers/        # Status checks, link / JS / spec extraction
│   ├── runner/         # Concurrent provider runs, --max-time, Ctrl-C
│   ├── tester_manager/ # Runs the testers over collected URLs
│   ├── readers/        # --files readers (WARC, URLTeam, text)
│   ├── cache/          # SQLite / Redis cache and the `urx cache` subcommand
│   ├── output/         # Output formats and --stream
│   ├── notify/         # Webhook notifications
│   ├── progress/       # Progress bars
│   ├── utils/          # URL transforms and helpers
│   └── network/        # HTTP client, proxy, headers, rate limiting
├── tests/              # Integration tests
├── docs/               # Documentation (Hwaro site)
├── example/            # Example configurations
├── aur/                # Arch Linux package
└── Dockerfile          # Container image
```

## More

- Full guide: [CONTRIBUTING.md](https://github.com/hahwul/urx/blob/main/CONTRIBUTING.md)
- [Code of Conduct](https://github.com/hahwul/urx/blob/main/CODE_OF_CONDUCT.md) (Contributor Covenant)
- Contributions are licensed under the project's [MIT License](https://github.com/hahwul/urx/blob/main/LICENSE).

[![Contributors](https://raw.githubusercontent.com/hahwul/urx/refs/heads/main/CONTRIBUTORS.svg)](https://github.com/hahwul/urx/graphs/contributors)
