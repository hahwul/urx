# URX - URL Extraction Tool for OSINT Archives

**ALWAYS follow these instructions first and fallback to additional search and context gathering only if the information here is incomplete or found to be in error.**

URX is a Rust-based command-line tool for extracting URLs from OSINT archives like Wayback Machine, Common Crawl, and OTX. It features async processing, extensive filtering capabilities, and multiple output formats.

## Working Effectively

### Bootstrap and Build the Repository
- **Install Rust toolchain**: URX requires Rust and Cargo (latest stable version)
- **Build debug version**: `cargo build` -- takes 2-3 minutes first time. NEVER CANCEL. Set timeout to 10+ minutes.
- **Build release version**: `cargo build --release` -- takes 1-2 minutes. NEVER CANCEL. Set timeout to 10+ minutes.
- **Alternative task runner**: Project includes `justfile` with shortcuts, but use cargo commands for maximum compatibility

### Testing
- **Run full test suite**: `cargo test` -- takes 20-30 seconds. NEVER CANCEL. Set timeout to 5+ minutes.
  - **Expected**: 185+ tests pass, 1 network test may fail (OTX provider - normal in CI environments)
  - **Some tests ignored**: 2 network tests are skipped in CI environments (normal behavior)
- **Test specific modules**: `cargo test <module_name>` for targeted testing

### Code Quality and Linting
- **Format code**: `cargo fmt`
- **Check formatting**: `cargo fmt --check` 
- **Run clippy**: `cargo clippy -- --deny warnings` -- takes 15-20 seconds
- **Run clippy on tests**: `cargo clippy --tests -- --deny warnings` -- takes 5-10 seconds  
- **Generate docs**: `cargo doc --workspace --all-features --no-deps --document-private-items`
- **ALWAYS run these before committing** or CI will fail

### Running the Application
- **Debug binary**: `./target/debug/urx`
- **Release binary**: `./target/release/urx` (faster, use for actual URL fetching)
- **Basic usage**: `urx example.com`
- **Help**: `urx --help` (comprehensive option list)
- **Version**: `urx --version`
- **Configuration**: Use `example/config.toml` as template for complex setups

## Build Timing and Timeouts

**CRITICAL - NEVER CANCEL builds or tests:**
- **First build**: 2-3 minutes (downloading dependencies)
- **Incremental builds**: 30-60 seconds
- **Release builds**: 1-2 minutes  
- **Test suite**: 20-30 seconds
- **Clippy checks**: 15-20 seconds
- **ALWAYS set timeouts to 10+ minutes for builds, 5+ minutes for tests**

## Validation Scenarios

**ALWAYS test these scenarios after making changes:**

### Core Development Workflow
1. **Build validation**: `cargo build && cargo build --release`
2. **Test validation**: `cargo test` (expect 185+ passes, 1 network failure OK, 2 tests ignored)
3. **Code quality**: `cargo fmt --check && cargo clippy -- --deny warnings`
4. **CLI functionality**: `./target/release/urx --help` (verify help displays)
5. **Version check**: `./target/release/urx --version` (verify version displays)

### Complete Validation Workflow
**Run this complete check before any commit:**
```bash
# Build and validate
cargo build && \
cargo test && \
cargo fmt --check && \
cargo clippy -- --deny warnings && \
cargo clippy --tests -- --deny warnings && \
./target/release/urx --version
```

### Functional Testing
Since URX fetches from external APIs, **manual validation should include**:
- **Basic run**: `./target/release/urx example.com --silent --timeout 10` (may timeout, that's OK)
- **Configuration test**: `./target/release/urx -c example/config.toml --help` (verify config loads)
- **Format options**: Test `--format json`, `--format csv` output options
- **Provider options**: Test `--providers wayback` or specific provider selection

### CI Validation 
**Before committing, ALWAYS run**:
- `cargo fmt --check` - formatting must be correct
- `cargo clippy -- --deny warnings` - no clippy warnings allowed
- `cargo clippy --tests -- --deny warnings` - test clippy must pass
- `cargo test` - test suite must mostly pass

## Common Tasks

### Repository Structure
```
.
├── src/                 # Main source code
│   ├── main.rs         # Application entry point
│   ├── cli/            # Command-line interface
│   ├── providers/      # URL data sources (wayback, cc, otx, etc)
│   ├── filters/        # URL filtering logic
│   ├── testers/        # HTTP testing and link extraction
│   └── network/        # Network configuration
├── example/            # Example configuration files
├── docs/               # Documentation (Zola static site)
├── justfile           # Alternative task runner
├── Cargo.toml         # Rust project configuration
└── .github/workflows/ # CI/CD pipelines
```

### Key Files to Check After Changes
- **CLI changes**: Always check `src/cli/mod.rs` and run help command
- **Provider changes**: Test with `--providers wayback` or specific provider
- **Filter changes**: Test with various filter options (`-e js`, `-p no-images`)
- **Network changes**: Check `src/network/settings.rs` and proxy-related code
- **Output changes**: Test `--format json` and `--format csv`

### Environment Variables
URX supports these environment variables:
- `URX_VT_API_KEY`: VirusTotal API key
- `URX_URLSCAN_API_KEY`: URLScan API key

### Development Tips
- **Modular architecture**: Each provider/filter/tester is a separate module
- **Async processing**: Uses tokio for concurrent operations
- **Extensive testing**: Most modules have comprehensive test coverage
- **Configuration-driven**: Supports TOML config files (see `example/config.toml`)

## Troubleshooting

### Build Issues
- **Dependency issues**: Run `cargo clean && cargo build`
- **Network timeouts**: Some tests may fail in restricted environments (normal)
- **Missing Rust**: Install via `https://rustup.rs/`

### Test Failures
- **Network tests failing**: Normal in CI, URX needs internet for real functionality
- **Single test failures**: Usually OTX provider test fails due to network restrictions
- **Multiple failures**: Likely indicates real issues, investigate carefully

### Runtime Issues
- **No output**: URX needs internet access to fetch URLs from archives
- **API errors**: Check if API keys are needed for VirusTotal/URLScan providers
- **Timeout errors**: Increase `--timeout` value or use `--parallel` to adjust concurrency

## CI/CD Integration

**The CI pipeline runs**:
1. **Multi-platform builds**: Ubuntu, macOS, Windows
2. **Test suite**: `cargo test --verbose`
3. **Linting**: `cargo fmt --check` and `cargo clippy`
4. **Code coverage**: Using cargo-llvm-cov

**Your changes must**:
- Build successfully on all platforms
- Pass the test suite (185+ tests)
- Have no formatting issues
- Generate no clippy warnings
- Maintain or improve code coverage