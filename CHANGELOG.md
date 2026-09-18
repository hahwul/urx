# Changelog

## 0.11.0

### Providers
- New `bevigil` provider (`--bevigil-api-key`) — URLs extracted from unpacked Android apps
- `--cdx-endpoint` / `--cdx-dialect` plug any pywb, OutbackCDX or classic CDX server in as a provider
- `--archived-discovery` reads robots.txt and sitemap.xml out of the archive instead of the live site
- A target may name a path (`urx example.com/shop`); the scope is pushed into the CDX query
- CDX capture metadata (first/last seen, MIME, status) carried through the pipeline

### Testing and extraction
- `--archive-body` replays archived responses and mines them for endpoints; `--archive-body-dir` saves each body with an `index.jsonl`
- `--extract-js-endpoints` mines JavaScript bodies — live and archived — for endpoints
- `--expand-specs` expands OpenAPI, Swagger and GraphQL specs (JSON and YAML), including archived ones
- `--check-title` records each response's HTML `<title>`; `--check-status` now records response metadata
- `--extract-links` also collects script, link, form, iframe and media sources
- `-H` / `--cookie` / `--user-agent` authenticate the requests urx makes to the target; never sent to archives

### Filtering
- `--match-regex` / `--filter-regex` for regex matching, alongside the substring `--patterns`
- `--dedup-similar` folds URLs that share a shape (`/post/1` … `/post/99999`)
- New presets: `only-secrets`, `only-backup`, `only-config`, `only-api`, `no-audio`, `only-audio`; an unknown `--preset` now fails instead of being ignored
- Client-side `--meta-*` filters (first/last seen, MIME, status) that apply to every provider
- `--scope-file` to scope a run from a file
- Archive-side CDX filters: `--archive-status`, `--archive-exclude-status`, `--archive-mime`, `--archive-exclude-mime`
- `--from` / `--to` generalised from Wayback-only to every CDX provider (`--wayback-from` / `--wayback-to` kept as aliases)

### Output
- `--stream` writes URLs as each provider reports them, instead of after the whole scan
- New `jsonl` and `wordlist` formats
- `--params`, `--params-by-endpoint` and `--fuzz-placeholder VALUE` for parameter inventory and fuzz templates
- `--notify` POSTs a run summary to a webhook (`--notify-on`, `--notify-format`)

### CLI
- `urx cache` subcommand: `stats`, `list`, `prune`, `drop`, `clear`
- `--completions SHELL` and `--manpage`
- SOCKS5 proxy support (`--proxy socks5://...`) and gzip/brotli response compression
- Documentation site redesigned around the new logo

### Fixes
- Silent truncation in the Wayback, Arquivo and Common Crawl walks
- Host, extension, pattern and `only-*` preset filters dropping everything
- CLI precedence over config, failing on no targets, and rejecting config typos
- Honest `--max-time` accounting that keeps the URLs a provider already collected
- Broken-pipe panic, CSV injection, multi-member gzip data loss, Redis TTL overflow
- Discovery providers reporting failed scans as clean empty results

## 0.10.0

- New providers: `arquivo` (Arquivo.pt, keyless) and `github` (GitHub Code Search, via `--github-api-key`)
- URLScan now works without an API key (anonymous public search; key remains optional for higher limits)
- Provider selection: `--list-providers`, `--exclude-providers`, and `--all-providers`
- Provider attribution per URL via `--show-sources` (JSON/CSV/plain text)
- Per-provider summary with `--stats`
- Domain input from file with `--domain-list FILE` (alias `--dL`)
- Split output per domain with `--output-dir PATH` (alias `--oD`)
- Separate API-keys-only TOML with `--provider-config FILE` (so the main config can be committed)
- Global enumeration ceiling with `--max-time SECONDS`
- Per-provider rate limits with `--rate-limit-by id=req_per_sec,...`
- Wayback date filtering with `--wayback-from` / `--wayback-to`
- Common Crawl: bump default index to `CC-MAIN-2026-17`, add `--cc-index latest`, and accept a comma-separated list of indexes
- Fix Wayback Machine timeouts on large domains (plain-text CDX, server-side dedup, longer default timeout)

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
