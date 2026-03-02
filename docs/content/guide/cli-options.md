+++
title = "CLI Options"
weight = 1
+++

## Command Line Options

Urx provides a comprehensive set of command-line options for customizing behavior.

```
Usage: urx [OPTIONS] [DOMAINS]...

Arguments:
  [DOMAINS]...  Domains to fetch URLs for

Options:
  -c, --config <CONFIG>  Config file to load
  -h, --help             Print help
  -V, --version          Print version

Input Options:
      --files <FILES>...  Read URLs directly from files (supports WARC, URLTeam compressed, and text files)

Output Options:
  -o, --output <OUTPUT>  Output file to write results
  -f, --format <FORMAT>  Output format (e.g., "plain", "json", "csv") [default: plain]
      --merge-endpoint   Merge endpoints with the same path and merge URL parameters
      --normalize-url    Normalize URLs for better deduplication

Provider Options:
  --providers <PROVIDERS>                Providers to use (comma-separated) [default: wayback,cc,otx]
  --subs                                 Include subdomains when searching
  --cc-index <CC_INDEX>                  Common Crawl index to use [default: CC-MAIN-2025-13]
  --vt-api-key <VT_API_KEY>             API key for VirusTotal
  --urlscan-api-key <URLSCAN_API_KEY>   API key for Urlscan
  --zoomeye-api-key <ZOOMEYE_API_KEY>   API key for ZoomEye

Discovery Options:
  --exclude-robots   Exclude robots.txt discovery
  --exclude-sitemap  Exclude sitemap.xml discovery

Display Options:
  -v, --verbose      Show verbose output
      --silent       Silent mode (no output)
      --no-progress  No progress bar

Filter Options:
  -p, --preset <PRESET>                     Filter Presets (e.g., "no-resources,no-images,only-js,only-style")
  -e, --extensions <EXTENSIONS>              Filter by extensions (e.g., "js,php,aspx")
      --exclude-extensions <EXTENSIONS>      Exclude extensions (e.g., "html,txt")
      --patterns <PATTERNS>                  Include URLs containing patterns
      --exclude-patterns <PATTERNS>          Exclude URLs containing patterns
      --show-only-host                       Only show the host part
      --show-only-path                       Only show the path part
      --show-only-param                      Only show the parameters part
      --min-length <MIN_LENGTH>              Minimum URL length
      --max-length <MAX_LENGTH>              Maximum URL length
      --strict                               Enforce exact host validation (default)

Network Options:
  --network-scope <SCOPE>        Apply settings to: all, providers, testers [default: all]
  --proxy <PROXY>                HTTP proxy (e.g., http://proxy:8080)
  --proxy-auth <PROXY_AUTH>      Proxy credentials (username:password)
  --insecure                     Skip SSL certificate verification
  --random-agent                 Use a random User-Agent
  --timeout <TIMEOUT>            Request timeout in seconds [default: 120]
  --retries <RETRIES>            Retries for failed requests [default: 2]
  --parallel <PARALLEL>          Max parallel requests per provider [default: 5]
  --rate-limit <RATE_LIMIT>      Requests per second

Testing Options:
  --check-status                     Check HTTP status code of collected URLs
  --include-status <INCLUDE_STATUS>  Include specific status codes (e.g., 200,30x)
  --exclude-status <EXCLUDE_STATUS>  Exclude specific status codes (e.g., 404,50x)
  --extract-links                    Extract additional links from collected URLs

Cache Options:
  --incremental              Only return new URLs compared to previous scans
  --cache-type <CACHE_TYPE>  Cache backend: sqlite or redis [default: sqlite]
  --cache-path <CACHE_PATH>  Path for SQLite cache database
  --redis-url <REDIS_URL>    Redis connection URL
  --cache-ttl <CACHE_TTL>    Cache TTL in seconds [default: 86400]
  --no-cache                 Disable caching entirely
```

## Available Providers

| Provider | Flag | API Key Required | Environment Variable |
|----------|------|-----------------|---------------------|
| Wayback Machine | `wayback` | No | - |
| Common Crawl | `cc` | No | - |
| OTX (AlienVault) | `otx` | No | - |
| VirusTotal | `vt` | Yes | `URX_VT_API_KEY` |
| URLScan | `urlscan` | Yes | `URX_URLSCAN_API_KEY` |
| ZoomEye | `zoomeye` | Yes | `URX_ZOOMEYE_API_KEY` |

Default providers: `wayback,cc,otx`. Providers requiring API keys are automatically enabled when their keys are provided.

## Filter Presets

| Preset | Description |
|--------|-------------|
| `no-resources` | Exclude resource files (images, CSS, fonts, etc.) |
| `no-images` | Exclude image files |
| `only-js` | Only JavaScript files |
| `only-style` | Only stylesheet files |
