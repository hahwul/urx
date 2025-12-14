---
title: "Options"
weight: 3
---

```
Usage: urx [OPTIONS] [DOMAINS]...

Arguments:
  [DOMAINS]...  Domains to fetch URLs for

Options:
  -c, --config <CONFIG>  Config file to load
  -h, --help             Print help
  -V, --version          Print version

Output Options:
  -o, --output <OUTPUT>  Output file to write results
  -f, --format <FORMAT>  Output format (e.g., "plain", "json", "csv") [default: plain]
      --merge-endpoint   Merge endpoints with the same path and merge URL parameters

Provider Options:
  --providers <PROVIDERS>              Providers to use (comma-separated, e.g., "wayback,cc,otx,vt,urlscan") [default: wayback,cc,otx]
  --subs                               Include subdomains when searching
  --cc-index <CC_INDEX>                Common Crawl index to use (e.g., CC-MAIN-2025-13) [default: CC-MAIN-2025-13]
  --vt-api-key <VT_API_KEY>            API key for VirusTotal (can also use URX_VT_API_KEY environment variable)
  --urlscan-api-key <URLSCAN_API_KEY>  API key for Urlscan (can also use URX_URLSCAN_API_KEY environment variable)

Discovery Options:
  --exclude-robots   Exclude robots.txt discovery
  --exclude-sitemap  Exclude sitemap.xml discovery

Display Options:
  -v, --verbose      Show verbose output
      --silent       Silent mode (no output)
      --no-progress  No progress bar

Filter Options:
  -p, --preset <PRESET>
          Filter Presets (e.g., "no-resources,no-images,only-js,only-style")
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
      --strict
          Enforce exact host validation (default)

Network Options:
  --network-scope <NETWORK_SCOPE>  Control which components network settings apply to (all, providers, testers, or providers,testers) [default: all]
  --proxy <PROXY>                  Use proxy for HTTP requests (format: http://proxy.example.com:8080)
  --proxy-auth <PROXY_AUTH>        Proxy authentication credentials (format: username:password)
  --insecure                       Skip SSL certificate verification (accept self-signed certs)
  --random-agent                   Use a random User-Agent for HTTP requests
  --timeout <TIMEOUT>              Request timeout in seconds [default: 30]
  --retries <RETRIES>              Number of retries for failed requests [default: 3]
  --parallel <PARALLEL>            Maximum number of parallel requests per provider and maximum concurrent domain processing [default: 5]
  --rate-limit <RATE_LIMIT>        Rate limit (requests per second)

Testing Options:
  --check-status                     Check HTTP status code of collected URLs [aliases: --cs]
  --include-status <INCLUDE_STATUS>  Include URLs with specific HTTP status codes or patterns (e.g., --is=200,30x) [aliases: --is]
  --exclude-status <EXCLUDE_STATUS>  Exclude URLs with specific HTTP status codes or patterns (e.g., --es=404,50x,5xx) [aliases: --es]
  --extract-links                    Extract additional links from collected URLs (requires HTTP requests)
```
