# Urx - URL Discovery Tool

Urx is a command-line tool for discovering URLs associated with specific domains from the Wayback Machine and Common Crawl archives. It's built in Rust and leverages asynchronous processing to efficiently query multiple data sources concurrently.

## Features

- Fetch URLs from multiple sources (Wayback Machine, Common Crawl, OTX)
- Process multiple domains concurrently
- Filter results by file extensions or patterns
- Multiple output formats (plain, JSON, CSV)
- Output to console or file
- Configurable verbosity levels
- Support for reading domains from stdin (pipeline integration)
- URL testing capabilities (status checking, link extraction)

## Installation

### From Source

```bash
git clone https://github.com/hahwul/urx.git
cd urx
cargo build --release
```

The compiled binary will be available at `target/release/urx`.

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

# Output JSON for further processing
cat domains.txt | urx -f json | jq '.[] | select(.status == 200)'
```

## Inspiration

Urx was inspired by [gau (GetAllUrls)](https://github.com/lc/gau), a tool that fetches known URLs from AlienVault's Open Threat Exchange, the Wayback Machine, and Common Crawl. While sharing similar core functionality, Urx was built from the ground up in Rust with a focus on performance, concurrency, and expanded filtering capabilities.

## License

MIT License