# Changelog

## Unreleased

- Add `arquivo` provider for [Arquivo.pt](https://arquivo.pt), the Portuguese web archive. Keyless; queries its Wayback-compatible CDX index (`output=json`) and walks `page=` cursors, stopping once a page adds no new URLs. Opt in with `--providers arquivo` or `--all-providers`
- URLScan now works without an API key: the `urlscan` provider queries the public search endpoint anonymously (rate-limited to ~30 req/min per IP) and is no longer disabled when no key is configured. A key remains optional and only raises limits / enables rotation
- Fix Wayback Machine timeouts on large domains: switch CDX to plain-text response with `collapse=urlkey` server-side dedup, raise default timeout to 60s, and filter non-URL response bodies
- Bump default Common Crawl index to `CC-MAIN-2026-17`, and add `--cc-index latest` to auto-resolve the newest index via `collinfo.json` (cached per run, validated against `CC-MAIN-YYYY-WW` shape)
- Track provider attribution per URL and surface it via `--show-sources` (JSON adds a `sources` field, CSV adds a `sources` column, plain text appends `[provider1,provider2]`)
- Add `--list-providers` to enumerate every supported provider, `--exclude-providers` for negative selection, and `--all-providers` to enable every catalog entry (API-keyed providers only activate when a key is set)
- Add `--stats` to print a per-provider summary (URLs found, errors, elapsed) to stderr at end of run
- Add `--domain-list FILE` (alias `--dL`) to read newline-separated domains from a file (repeatable; merged with positional DOMAINS and stdin; `#` comments allowed)
- Add `--max-time SECONDS` global ceiling on provider enumeration; on deadline urx aborts in-flight fetches and returns whatever URLs have been collected so far (0 = unlimited; default)
- Add `--rate-limit-by id=req_per_sec,...` for per-provider rate limits; providers not listed fall back to global `--rate-limit`
- Add `--provider-config FILE` for a separate API-keys-only TOML (default `$XDG_CONFIG_HOME/urx/provider-config.toml`); precedence is CLI/env > provider-config > main config, so the main config can be safely committed to source control
- Add `--output-dir PATH` (alias `--oD`) to split results into one file per domain (`<host>.<ext>`), with `<ext>` matching `--format`. Coexists with `--output` and stdout; the directory is created if missing; unparseable URLs land in `_unknown.<ext>`
- Add `--wayback-from` / `--wayback-to` to restrict Wayback Machine results to a date window. Accepts YYYY / YYYYMM / YYYYMMDD / YYYYMMDDhhmmss; partial dates pad toward the appropriate end of the range; malformed values are dropped with a warning
- `--cc-index` now accepts a comma-separated list (e.g. `CC-MAIN-2026-17,CC-MAIN-2025-51`); each entry becomes its own provider instance running in parallel, with separate stats lines
- Add `github` provider for GitHub Code Search. Enabled via `--github-api-key` (or `URX_GITHUB_API_KEY`, comma-separated for rotation). Pulls URLs out of `text_matches` fragments and requires an exact host or subdomain match before keeping a URL

## 0.9.0

- Add ZoomEye Provider
- Code Refactoring and Enhanced Testing

## 0.8.0

- Add Multiple API Key Rotation (e.g., `--vt-api-key` key1 `--vt-api-key` key2)
- Add URL Normalization and Deduplication (Added `--normalize-url` and `--merge-endpoint`)
- Add Caching and Incremental Scanning (Added `--incremental`, `--cache-type`, `--cache-path`, `--cache-ttl`, `--redis-url`, `--no-cache`)

## 0.7.0

- Add Support for Direct Reading from Files with Unified `--files` Flag and Auto-Detection
- Centralized, randomized modern User-Agent; applied across providers/testers
- robots.txt/sitemap use `--random-agent`; disabling resets UA
- Stabilized env-var tests; fixed clippy warnings

## 0.6.1

- Dependencies Update
- Fixed a bug in the HostValidator when using the `--subs` flag (#78)

## 0.6.0

- Enhanced URL discovery features
  - Added robots.txt and sitemap.xml discovery by default
  - Added `--exclude-robots` and `--exclude-sitemap` flags to disable discovery when needed
- Added HTTP response status highlighting with `--check-status` flag (#59)
- Improved API key handling for providers
  - Auto-enables VirusTotal and Urlscan when API keys are provided (#60)
- Enhanced network reliability
  - Increased default timeout from 30s to 120s and optimized retry settings (#68)
- Fixed provider issues
  - Resolved OTX provider parsing bug for null values (#70)
  - Fixed connectivity issues with Wayback Machine, Common Crawl, and OTX (#62)

## 0.5.0

- Added robots.txt discovery functionality by [@Adesoji1](https://github.com/Adesoji1)
- Added sitemap.xml discovery functionality
- Added `--strict` flag - Enforce exact host validation (default is true)

## 0.4.0

- Added `--config` flag - Load configuration from a specified file
- Support to vt provider(Virustotal) - Search URLs from Virustotal API
  - Added `--vt-api-key` flag and `URX_VT_API_KEY` - Specify API key for Virustotal
- Support to urlscan provider - Search URLs from Urlscan API
  - Added `--urlscan-api-key` flag and `URX_URLSCAN_API_KEY` - Specify API key for Urlscan
- Improve performance

## 0.3.0

- Added `--insecure` - Skip SSL certificate verification
- Added `--network-scope` - Control which components network settings apply to
- Added status filtering options:
  - `--include-status` - Filter URLs by specific HTTP status codes (aliases: `--is`)
  - `--exclude-status` - Exclude URLs with specific HTTP status codes (aliases: `--es`)

## 0.2.0

- Added display control options:
  - `--silent` - Run in silent mode with no output
  - `--no-progress` - Disable progress bar display
- Added `--preset` - Apply predefined URL filters (e.g., no-resources, only-js)

## 0.1.0

- Initial release
- Project foundation established
