---
title: "Contributing"
weight: 4
---

## Contributing to Urx

Urx is an open-source project made with ❤️, and we welcome contributions from the community!

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
   git checkout -b feature/your-feature-name
   ```

#### Making Changes

1. **Write clean code** following Rust best practices
2. **Add tests** for new functionality
3. **Update documentation** if needed
4. **Run formatting**:
   ```bash
   cargo fmt
   ```
5. **Run linting**:
   ```bash
   cargo clippy -- --deny warnings
   ```

#### Submitting Changes

1. **Commit your changes**:
   ```bash
   git add .
   git commit -m "Add feature: description"
   ```

2. **Push to your fork**:
   ```bash
   git push origin feature/your-feature-name
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
- Translate documentation (future)

Documentation is in `docs/content/` directory using Markdown format.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](https://github.com/hahwul/urx/blob/main/CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

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
- Be responsive to feedback

## Project Structure

```
urx/
├── src/              # Source code
│   ├── cli/         # CLI argument parsing
│   ├── providers/   # URL data providers
│   ├── filters/     # URL filtering logic
│   ├── testers/     # HTTP testing
│   └── network/     # Network configuration
├── docs/            # Documentation (Zola site)
├── example/         # Example configurations
└── tests/           # Integration tests
```

## Need Help?

- Read the full [CONTRIBUTING.md](https://github.com/hahwul/urx/blob/main/CONTRIBUTING.md) guide
- Join [GitHub Discussions](https://github.com/hahwul/urx/discussions)
- Check the [documentation](../../getting_started/introduction)
- Ask questions in issues

## Recognition

All contributors are recognized in the project!

[![Contributors](https://raw.githubusercontent.com/hahwul/urx/refs/heads/main/CONTRIBUTORS.svg)](https://github.com/hahwul/urx/graphs/contributors)

## License

By contributing to Urx, you agree that your contributions will be licensed under the project's [MIT License](https://github.com/hahwul/urx/blob/main/LICENSE).

---

Thank you for contributing to Urx! Your support helps make the project better for everyone. 🚀
