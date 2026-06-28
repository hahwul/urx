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
  -c, --config <CONFIG>           Config file to load
      --provider-config <PATH>    Separate provider config holding only API keys (default: $XDG_CONFIG_HOME/urx/provider-config.toml)
  -h, --help             Print help
  -V, --version          Print version

Input Options:
      --files <FILES>...     Read URLs directly from files (supports WARC, URLTeam compressed, and text files)
      --domain-list <PATH>   File of newline-separated domains to scan (repeatable; merged with positional DOMAINS and stdin; `#` comments allowed)

Output Options:
  -o, --output <OUTPUT>          Output file to write results
      --output-dir <PATH>        Write one file per domain into this directory; extension matches --format. Coexists with --output / stdout.
  -f, --format <FORMAT>          Output format (e.g., "plain", "json", "csv") [default: plain]
      --merge-endpoint   Merge endpoints with the same path and merge URL parameters
      --normalize-url    Normalize URLs for better deduplication

Provider Options:
  --providers <PROVIDERS>                Providers to use (comma-separated) [default: wayback,cc,otx]
  --exclude-providers <PROVIDERS>        Providers to exclude (wins on conflict)
  --all-providers                        Enable every supported provider (API-keyed ones only if a key is available)
  --list-providers                       List every supported provider then exit
  --subs                                 Include subdomains when searching
  --cc-index <CC_INDEX>                  Common Crawl index(es), comma-separated for parallel queries; `latest` auto-resolves [default: latest]
  --wayback-from <DATE>                  Restrict Wayback results to >= DATE (YYYY/YYYYMM/YYYYMMDD/YYYYMMDDhhmmss)
  --wayback-to <DATE>                    Restrict Wayback results to <= DATE (same format as --wayback-from)
  --vt-api-key <VT_API_KEY>             API key for VirusTotal
  --urlscan-api-key <URLSCAN_API_KEY>   Optional API key for Urlscan (also works anonymously)
  --zoomeye-api-key <ZOOMEYE_API_KEY>   API key for ZoomEye
  --github-api-key <GITHUB_API_KEY>     Personal access token for GitHub Code Search (URX_GITHUB_API_KEY)

Discovery Options:
  --exclude-robots   Exclude robots.txt discovery
  --exclude-sitemap  Exclude sitemap.xml discovery

Display Options:
  -v, --verbose       Show verbose output
      --silent        Silent mode (no output)
      --no-progress   No progress bar
      --show-sources  Annotate output URLs with the providers that returned them
      --stats         Print a per-provider summary to stderr at end of run

Filter Options:
  -p, --preset <PRESET>                     Filter Presets (e.g., "no-resources,no-images,no-audio,only-js,only-style")
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
  --parallel <PARALLEL>          Max domains fetched concurrently per provider (rate-limit shared) [default: 5]
  --rate-limit <RATE_LIMIT>      Requests per second
  --rate-limit-by <PAIRS>        Per-provider rate overrides (e.g. `vt=1,wayback=10`); falls back to --rate-limit for unlisted providers
  --max-time <SECONDS>           Global ceiling on provider enumeration time in seconds; in-flight fetches are aborted at deadline (0 = unlimited) [default: 0]

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
| Arquivo.pt | `arquivo` | No | - |
| VirusTotal | `vt` | Yes | `URX_VT_API_KEY` |
| URLScan | `urlscan` | No (optional) | `URX_URLSCAN_API_KEY` |
| ZoomEye | `zoomeye` | Yes | `URX_ZOOMEYE_API_KEY` |

Default providers: `wayback,cc,otx`. Providers requiring API keys are automatically enabled when their keys are provided. `arquivo` (the Portuguese web archive) is keyless but opt-in — add it with `--providers` or enable everything with `--all-providers`. URLScan works anonymously without a key (rate-limited to ~30 requests/min per IP); a key only raises those limits and enables rotation.

## Filter Presets

| Preset | Description |
|--------|-------------|
| `no-resources` | Exclude resource files (images, CSS, fonts, etc.) |
| `no-images` | Exclude image files |
| `no-audio` | Exclude audio files |
| `only-js` | Only JavaScript files |
| `only-style` | Only stylesheet files |
| `only-audio` | Only audio files |
