<div align="center">
  <picture>
    <img alt="URX Logo" src="https://github.com/user-attachments/assets/e147b5fb-84c1-428b-ab04-ceb70dd49339" width="300px;">
  </picture>
  <p>Extracts URLs from OSINT Archives for Security Insights.</p>
</div>

<p align="center">
<a href="https://github.com/owasp-noir/noir/blob/main/CONTRIBUTING.md">
<img src="https://img.shields.io/badge/CONTRIBUTIONS-WELCOME-000000?style=for-the-badge&labelColor=black"></a>
<a href="https://github.com/owasp-noir/noir/releases">
<!-- <img src="https://img.shields.io/github/v/release/hahwul/urx?style=for-the-badge&color=black&labelColor=black&logo=web"></a> -->
<a href="https://rust-lang.org">
<img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white"></a>
</p>

Urx is a command-line tool designed for collecting security-related URLs from OSINT archives, such as the Wayback Machine and Common Crawl. Built with Rust for efficiency, it uses asynchronous processing to quickly query multiple data sources. This tool streamlines the process of gathering and analyzing URL information, which is crucial for effective security testing.

## Features

- Fetch URLs from multiple sources (Wayback Machine, Common Crawl, OTX)
- Process multiple domains concurrently
- Filter results by file extensions or patterns
- Multiple output formats (plain, JSON, CSV)
- Output to console or file
- Support for reading domains from stdin (pipeline integration)
- URL testing capabilities (status checking, link extraction)

![Preview](https://github.com/user-attachments/assets/292fee95-5a8a-4316-95f2-22ae22d5c070)

## Installation

### From Cargo (Recommended)

```bash
cargo install urx
```

### From Homebrew (Tap)

```bash
brew tap hahwul/urx
brew install urx
```

### From Source

```bash
git clone https://github.com/hahwul/urx.git
cd urx
cargo build --release
```

The compiled binary will be available at `target/release/urx`.

### From Docker

[ghcr.io/hahwul/urx](https://github.com/hahwul/urx/pkgs/container/urx)

## Usage

### Basic Usage

```bash
# Scan a single domain
urx example.com

# Scan multiple domains
urx example.com example.org

# Scan domains from a file
cat domains.txt | urx
```

### Options

```
Usage: urx [OPTIONS] [DOMAINS]...

Arguments:
  [DOMAINS]...  Domains to fetch URLs for

Options:
  -h, --help     Print help
  -V, --version  Print version

Output Options:
  -o, --output <OUTPUT>  Output file to write results
  -f, --format <FORMAT>  Output format (e.g., "plain", "json", "csv") [default: plain]
      --merge-endpoint   Merge endpoints with the same path and merge URL parameters

Provider Options:
      --cc-index <CC_INDEX>    Common Crawl index to use (e.g., CC-MAIN-2025-08) [default: CC-MAIN-2025-08]
      --providers <PROVIDERS>  Providers to use (comma-separated, e.g., "wayback,cc,otx") [default: wayback,cc]
      --subs                   Include subdomains when searching

Display Options:
  -v, --verbose  Show verbose output

Filter Options:
  -e, --extensions <EXTENSIONS>
          Filter URLs to only include those with specific extensions (comma-separated, e.g., "js,php,aspx")
      --exclude-extensions <EXCLUDE_EXTENSIONS>
          Filter URLs to exclude those with specific extensions (comma-separated, e.g., "html,txt")
      --patterns <PATTERNS>
          Filter URLs to only include those containing specific patterns (comma-separated)
      --exclude-patterns <EXCLUDE_PATTERNS>
          Filter URLs to exclude those containing specific patterns (comma-separated)
      --show-only-host
          Only show the host part of the URLs
      --show-only-path
          Only show the path part of the URLs
      --show-only-param
          Only show the parameters part of the URLs
      --min-length <MIN_LENGTH>
          Minimum URL length to include
      --max-length <MAX_LENGTH>
          Maximum URL length to include

Network Options:
      --proxy <PROXY>            Use proxy for HTTP requests (format: http://proxy.example.com:8080)
      --proxy-auth <PROXY_AUTH>  Proxy authentication credentials (format: username:password)
      --random-agent             Use a random User-Agent for HTTP requests
      --timeout <TIMEOUT>        Request timeout in seconds [default: 30]
      --retries <RETRIES>        Number of retries for failed requests [default: 3]
      --parallel <PARALLEL>      Maximum number of parallel requests [default: 5]
      --rate-limit <RATE_LIMIT>  Rate limit (requests per second)

Testing Options:
      --check-status   Check HTTP status code of collected URLs
      --extract-links  Extract additional links from collected URLs (requires HTTP requests)
```

### Examples

```bash
# Save results to a file
urx example.com -o results.txt

# Output in JSON format
urx example.com -f json -o results.json

# Filter for JavaScript files only
urx example.com -e js

# Exclude HTML and text files
urx example.com --exclude-extensions html,txt

# Filter for API endpoints
urx example.com --patterns api,v1,graphql

# Exclude specific patterns
urx example.com --exclude-patterns static,images

# Use specific providers
urx example.com --providers wayback,otx

# Include subdomains
urx example.com --subs

# Check status of collected URLs
urx example.com --check-status

# Extract additional links from collected URLs
urx example.com --extract-links

# Network configuration
urx example.com --proxy http://localhost:8080 --timeout 60 --parallel 10

# Advanced filtering
urx example.com -e js,php --patterns admin,login --exclude-patterns logout,static --min-length 20
```

## Integration with Other Tools

Urx works well in pipelines with other security and reconnaissance tools:

```bash
# Find domains, then discover URLs
echo "example.com" | urx | grep "login" > potential_targets.txt

# Combine with other tools
cat domains.txt | urx --patterns api | other-tool
```

## Inspiration

Urx was inspired by [gau (GetAllUrls)](https://github.com/lc/gau), a tool that fetches known URLs from AlienVault's Open Threat Exchange, the Wayback Machine, and Common Crawl. While sharing similar core functionality, Urx was built from the ground up in Rust with a focus on performance, concurrency, and expanded filtering capabilities.

## Contribute

Urx is open-source project and made it with ❤️ 
if you want contribute this project, please see [CONTRIBUTING.md](./CONTRIBUTING.md) and Pull-Request with cool your contents.

[![](./CONTRIBUTORS.svg)](https://github.com/hahwul/urx/graphs/contributors)