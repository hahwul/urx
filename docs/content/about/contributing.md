+++
title = "Contributing"
description = "Report issues, set up a development environment and open a pull request against urx."
toc = true
weight = 2
+++

## Contributing to Urx

Urx is an open-source project, and we welcome contributions from the community!

## How to Contribute

### Reporting Issues

Found a bug or have a feature request?

1. **Search existing issues** to avoid duplicates
2. **Create a new issue** with a clear title and description
3. **Provide details**:
   - Urx version (`urx --version`)
   - Operating system
   - Command used
   - Expected vs actual behavior
   - Steps to reproduce

[Report an issue on GitHub](https://github.com/hahwul/urx/issues/new)

### Contributing Code

#### Prerequisites

- Rust (latest stable version)
- Git
- Familiarity with the Rust ecosystem

#### Development Setup

1. **Fork the repository** on GitHub

2. **Clone your fork**:
   ```bash
   git clone https://github.com/YOUR_USERNAME/urx.git
   cd urx
   ```

3. **Build the project**:
   ```bash
   cargo build
   ```

4. **Run tests**:
   ```bash
   cargo test
   ```

5. **Create a feature branch**:
   ```bash
   git checkout -b features/your-feature-name
   # or
   git checkout -b bugfix/issue-description
   ```

#### Making Changes

1. **Write clean code** following Rust best practices
2. **Add tests** for new functionality
3. **Update documentation** if needed
4. **Run formatting**:
   ```bash
   cargo fmt
   ```
5. **Run the same lints CI runs**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   ```
   `--all-targets --all-features` also lints the tests and the optional
   `redis-cache` code, which a plain `cargo clippy` skips.

If you have [just](https://github.com/casey/just) installed, `just test` runs the
tests, clippy (on code and tests), the format check and `cargo doc` in one go,
and `just fix` formats and applies clippy's automatic fixes.

#### Submitting Changes

1. **Commit your changes**:
   ```bash
   git add .
   git commit -m "Add feature: description"
   ```

2. **Push to your fork**:
   ```bash
   git push origin features/your-feature-name
   ```

3. **Create a Pull Request** on GitHub:
   - Clear title and description
   - Reference related issues
   - Describe changes made
   - Include any breaking changes

### Contributing Documentation

Documentation improvements are always welcome!

- Fix typos or unclear explanations
- Add examples and use cases
- Improve existing guides

Documentation is in the `docs/content/` directory using Markdown format with TOML front matter.
The site is built with [Hwaro](https://github.com/hahwul/hwaro):

```bash
just docs-dependencies   # installs Hwaro (macOS, via Homebrew)
just docs-serve          # live preview at http://localhost:3000
```

When you add or change a flag or config key, update both `README.md` and the
pages under `docs/content/guide/`. The option blocks on
[CLI Options](/guide/cli-options/) and in the README are copied by hand from
`urx --help`, so diff them against the built binary's output. New config keys
also belong in `example/config.toml` and [Configuration](/guide/configuration/).

## Development Guidelines

### Code Style

- Follow Rust standard formatting (use `cargo fmt`)
- Write clear, self-documenting code
- Add comments for complex logic
- Keep functions focused and small

### Testing

- Add unit tests for new functions
- Add integration tests for features
- Ensure all tests pass before submitting
- Aim for high test coverage

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb (Add, Fix, Update, Remove)
- Reference issue numbers when applicable
- Keep the first line under 72 characters

### Pull Request Guidelines

- One feature/fix per pull request
- Keep changes focused and atomic
- Update CHANGELOG.md for significant changes
- Ensure CI passes before requesting review

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
│   ├── tester_manager/ # Runs the testers over collected URLs
│   ├── readers/        # --files readers (WARC, URLTeam, text)
│   ├── cache/          # SQLite / Redis cache and the `urx cache` subcommand
│   ├── output/         # Output formats and --stream
│   ├── notify/         # Webhook notifications
│   └── network/        # HTTP client, proxy, headers, rate limiting
├── tests/              # Integration tests
├── docs/               # Documentation (Hwaro site)
├── example/            # Example configurations
├── aur/                # Arch Linux package
└── Dockerfile          # Container image
```

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](https://github.com/hahwul/urx/blob/main/CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## Need Help?

- Read the full [CONTRIBUTING.md](https://github.com/hahwul/urx/blob/main/CONTRIBUTING.md) guide
- Ask a question in [GitHub Issues](https://github.com/hahwul/urx/issues)

## Recognition

All contributors are recognized in the project!

[![Contributors](https://raw.githubusercontent.com/hahwul/urx/refs/heads/main/CONTRIBUTORS.svg)](https://github.com/hahwul/urx/graphs/contributors)

## License

By contributing to Urx, you agree that your contributions will be licensed under the project's [MIT License](https://github.com/hahwul/urx/blob/main/LICENSE).
