<div align="center">
  <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/static/images/urx-dark.png" width="500px;">
        <source media="(prefers-color-scheme: light)" srcset="docs/static/images/urx-light.png" width="500px;">
        <img alt="Urx Logo" src="docs/static/images/urx-dark.png" width="500px;">
  </picture>
  <p>Extracts URLs from OSINT Archives for Security Insights.</p>
</div>

<p align="center">
  <a href="https://github.com/hahwul/urx/releases/latest"><img src="https://img.shields.io/github/v/release/hahwul/urx?style=for-the-badge&logoColor=%23000000&label=urx&labelColor=%23000000&color=%23000000"></a>
  <a href="https://app.codecov.io/gh/hahwul/urx"><img src="https://img.shields.io/codecov/c/gh/hahwul/urx?style=for-the-badge&logoColor=%23000000&labelColor=%23000000&color=%23000000"></a>
  <a href="https://github.com/hahwul/urx/blob/main/CONTRIBUTING.md"><img src="https://img.shields.io/badge/CONTRIBUTIONS-WELCOME-000000?style=for-the-badge&labelColor=000000"></a>
  <a href="https://rust-lang.org"><img src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white"></a>
</p>

Urx is a command-line tool designed for collecting URLs from OSINT archives, such as the Wayback Machine and Common Crawl. Built with Rust for efficiency, it leverages asynchronous processing to rapidly query multiple data sources. This tool simplifies the process of gathering URL information for a specified domain, providing a comprehensive dataset that can be used for various purposes, including security testing and analysis.

## Features

* Fetch URLs from multiple sources in parallel (Wayback Machine, Common Crawl, OTX, Arquivo.pt)
* Plug in any other CDX index server — national web archives, a private pywb, OutbackCDX — with `--cdx-endpoint URL`, no code change needed
* Keyless by default: Wayback, Common Crawl, OTX, Arquivo.pt, and URLScan (anonymous) all work without an API key
* API key rotation support for VirusTotal and URLScan providers to mitigate rate limits
* Filter results by file extensions, substring patterns, or full regular expressions (`--match-regex` / `--filter-regex`)
* Predefined presets, both by file family ("no-images", "only-js") and by security interest ("only-secrets", "only-backup", "only-config", "only-api")
* Archive-side filtering: push status code, MIME type, and date range into the CDX query itself, so filtered-out captures never cross the network
* URL normalization and deduplication: Sort query parameters, remove trailing slashes, merge semantically identical URLs, and collapse near-duplicates that differ only in ids, hashes, or dates (`--dedup-similar`)
* Support for multiple output formats: plain text, JSON, JSON Lines, CSV
* Archive capture metadata: `first_seen`, `last_seen`, `mime`, `archive_status`, and `digest` come back with every URL a CDX archive reported, at no extra network cost
* Streaming output (`--stream`): URLs are written as each provider reports them, so a pipeline starts working immediately instead of waiting for the slowest archive
* Direct file input support: Read URLs directly from WARC files, URLTeam compressed files, and text files
* Output results to the console or a file, or stream via stdin for pipeline integration
* URL Testing:
  * Filter and validate URLs based on HTTP status codes and patterns.
  * Extract additional links from collected URLs — anchors, scripts, stylesheets, form actions, iframes, images, media sources, objects, embeds, and meta-refresh targets
* Caching and Incremental Scanning:
  * Local SQLite or remote Redis caching to avoid re-scanning domains
  * Incremental mode to discover only new URLs since last scan
  * Configurable cache TTL and automatic cleanup of expired entries

![Preview](https://raw.githubusercontent.com/hahwul/urx/refs/heads/main/docs/static/images/preview.jpg)

## Installation

### From Cargo

```bash
# https://crates.io/crates/urx
cargo install urx
```

### From Homebrew

```bash
# https://formulae.brew.sh/formula/urx
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

### Shell Completions

`urx` generates its own completion script, so it always matches the flags of
the binary you actually have installed.

```bash
# zsh — any directory on your $fpath works
urx --completions zsh > ~/.zfunc/_urx
# (make sure ~/.zfunc is on the fpath, then `compinit`)

# bash
urx --completions bash > ~/.local/share/bash-completion/completions/urx

# fish
urx --completions fish > ~/.config/fish/completions/urx.fish
```

`powershell` and `elvish` are supported too. The flag needs no target domain.

### Man Page

```bash
urx --manpage > ~/.local/share/man/man1/urx.1
man urx
```

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
  -c, --config <CONFIG>           Config file to load
      --provider-config <PATH>    Separate provider config file holding only API keys (default: $XDG_CONFIG_HOME/urx/provider-config.toml). CLI/env > provider-config > main config.
      --completions <SHELL>       Print a shell completion script (bash, zsh, fish, powershell, elvish) to stdout and exit
      --manpage                   Print the roff man page to stdout and exit
  -h, --help             Print help
  -V, --version          Print version

Input Options:
      --files <FILES>...        Read URLs directly from files (supports WARC, URLTeam compressed, and text files)
      --domain-list <PATH>      File of newline-separated domains to scan (repeatable; merged with positional DOMAINS and stdin; `#` comments allowed)

Output Options:
  -o, --output <OUTPUT>          Output file to write results
      --output-dir <PATH>        Write one file per domain into this directory (extension matches --format). Coexists with --output / stdout.
  -f, --format <FORMAT>          Output format: "plain", "json" (one array), "jsonl" (one JSON object per line), "csv" [default: plain]
      --stream           Write URLs as each provider reports them instead of once at the end (unsorted; bypasses cache; rejects options needing the full result set)
      --merge-endpoint   Merge endpoints with the same path and merge URL parameters
      --normalize-url    Normalize URLs for better deduplication (sorts query parameters, removes trailing slashes)
      --dedup-similar    Collapse URLs that differ only in variable data (numeric ids, UUIDs, hashes, dates, query values)

Provider Options:
      --providers <PROVIDERS>
          Providers to use (comma-separated, e.g., "wayback,cc,otx,arquivo,vt,urlscan") [default: wayback,cc,otx]
      --exclude-providers <EXCLUDE_PROVIDERS>
          Providers to exclude (comma-separated). Wins on conflict with --providers / --all-providers.
      --all-providers
          Enable every supported provider. API-keyed providers only activate when a key is available.
      --list-providers
          List every supported provider then exit.
      --subs
          Include subdomains when searching
      --cc-index <CC_INDEX>
          Common Crawl index to use; accepts comma-separated list to query multiple indexes in parallel (e.g. `CC-MAIN-2026-17,CC-MAIN-2025-51`). `latest` (the default) resolves the newest via collinfo.json. [default: latest]
      --cdx-endpoint <URL>
          Query an additional CDX index server (any pywb, OutbackCDX, or classic Internet-Archive-style CDX API) by its full API URL, e.g. https://vefsafn.is/cdx. Repeatable. Each endpoint becomes a provider with id `cdx:<host>` and honours --subs, --from/--to and the --archive-* filters. See "Custom CDX Endpoints" below
      --cdx-dialect <DIALECT>
          Which CDX dialect the --cdx-endpoint servers speak: `pywb` or `classic`. Unset: urx probes each endpoint once and falls back to pywb when the answer is ambiguous
      --from <DATE>
          Restrict every CDX-backed provider (wayback, cc, arquivo, --cdx-endpoint) to captures at or after DATE (YYYY/YYYYMM/YYYYMMDD/YYYYMMDDhhmmss). Alias: --wayback-from
      --to <DATE>
          Restrict every CDX-backed provider to captures at or before DATE (same format as --from). Alias: --wayback-to
      --archive-status <CODE>
          Keep only captures the archive recorded with this HTTP status code (e.g. "200"). Applied by the CDX index itself, so unlike --include-status it costs no extra requests. A multi-value list works on wayback only — see "Archive-side Filtering" below
      --archive-exclude-status <CODES>
          Drop captures the archive recorded with these HTTP status codes (comma-separated, e.g. "404,500"). Multi-value works on every CDX provider
      --archive-mime <TYPE>
          Keep only captures with this recorded MIME type (e.g. "application/json"). Catches endpoints with no file extension, which -e/--extensions cannot
      --archive-exclude-mime <TYPES>
          Drop captures with these recorded MIME types (comma-separated, e.g. "text/html,image/png")
      --vt-api-key <VT_API_KEY>
          API key for VirusTotal (can be used multiple times for rotation, can also use URX_VT_API_KEY environment variable with comma-separated keys)
      --urlscan-api-key <URLSCAN_API_KEY>
          Optional API key for Urlscan; the provider also works anonymously (rate-limited ~30 req/min per IP). Can be used multiple times for rotation, or via URX_URLSCAN_API_KEY (comma-separated keys)
      --github-api-key <GITHUB_API_KEY>
          Personal access token for the GitHub Code Search provider (also reads URX_GITHUB_API_KEY, comma-separated for rotation)

Discovery Options:
      --exclude-robots   Exclude robots.txt discovery
      --exclude-sitemap  Exclude sitemap.xml discovery

Display Options:
  -v, --verbose       Show verbose output
      --silent        Silent mode (no output)
      --no-progress   No progress bar
      --show-sources  Annotate output URLs with the providers that returned them
      --show-meta     Annotate plain-text URLs with the archive capture metadata
      --stats         Print a per-provider summary to stderr at end of run

Filter Options:
  -p, --preset <PRESET>
          Filter Presets (e.g., "no-resources,no-images,no-audio,only-js,only-style,only-secrets,only-backup,only-config,only-api")
  -e, --extensions <EXTENSIONS>
          Filter URLs to only include those with specific extensions (comma-separated, e.g., "js,php,aspx")
      --exclude-extensions <EXCLUDE_EXTENSIONS>
          Filter URLs to exclude those with specific extensions (comma-separated, e.g., "html,txt")
      --patterns <PATTERNS>
          Filter URLs to only include those containing specific patterns (comma-separated)
      --exclude-patterns <EXCLUDE_PATTERNS>
          Filter URLs to exclude those containing specific patterns (comma-separated)
      --match-regex <RE>
          Keep only URLs matching this regular expression (repeatable, ORed; case-sensitive; never comma-split)
      --filter-regex <RE>
          Drop URLs matching this regular expression (repeatable; one match is enough)
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
      --proxy <PROXY>                  Use proxy for HTTP requests (format: <http://proxy.example.com:8080>)
      --proxy-auth <PROXY_AUTH>        Proxy authentication credentials (format: username:password)
      --insecure                       Skip SSL certificate verification (accept self-signed certs)
      --random-agent                   Use a random User-Agent for HTTP requests
      --timeout <TIMEOUT>              Request timeout in seconds [default: 120]
      --retries <RETRIES>              Number of retries for failed requests [default: 2]
      --parallel <PARALLEL>            Maximum domains fetched concurrently per provider (and concurrent URL tests); a provider's --rate-limit is shared across them [default: 5]
      --rate-limit <RATE_LIMIT>        Rate limit (requests per second)
      --rate-limit-by <PAIRS>          Per-provider rate overrides (e.g. `vt=1,wayback=10`); falls back to --rate-limit for unlisted providers
      --max-time <MAX_TIME>            Global ceiling on provider enumeration time in seconds (0 = unlimited) [default: 0]

Testing Options:
      --check-status
          Check HTTP status code of collected URLs [aliases: ----cs]
      --include-status <INCLUDE_STATUS>
          Include URLs with specific HTTP status codes or patterns (e.g., --is=200,30x) [aliases: ----is]
      --exclude-status <EXCLUDE_STATUS>
          Exclude URLs with specific HTTP status codes or patterns (e.g., --es=404,50x,5xx) [aliases: ----es]
      --extract-links
          Extract additional links from collected URLs (requires HTTP requests)
```

`--extract-links` reads every URL-bearing tag, not just anchors: `<a href>`,
`<script src>`, `<link href>`, `<form action>`, `<iframe src>`, `<img src>`,
`<source src>`, `<object data>`, `<embed src>`, and `<meta http-equiv="refresh">`
targets. Relative URLs resolve against the page (honouring `<base href>`),
duplicates are collapsed, and discovered links pass through the same filters
and host validation as the rest of the run. See
[docs/content/guide/cli-options.md](docs/content/guide/cli-options.md) for the
full table.

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

# Use Fileter Preset (similar to --exclude-extensions=png,jpg,.....)
urx example.com -p no-images

# Use specific providers
urx example.com --providers wayback,otx

# Add the keyless Arquivo.pt (Portuguese web archive) provider
urx example.com --providers wayback,cc,otx,arquivo

# Query another CDX index server alongside the defaults (id: cdx:vefsafn.is)
urx example.is --cdx-endpoint https://vefsafn.is/cdx

# ...or on its own, rate-limited, with the archive-side filters it shares with wayback/cc
urx example.is --cdx-endpoint https://vefsafn.is/cdx --providers cdx:vefsafn.is \
  --rate-limit-by cdx:vefsafn.is=1 --from 2020 --archive-status 200

# URLScan works without a key (anonymous, rate-limited); a key just raises limits
urx example.com --providers urlscan

# Using VirusTotal and URLScan providers
# 1. Explicitly add to providers (with API keys via command line)
urx example.com --providers=vt,urlscan --vt-api-key=*** --urlscan-api-key=***

# 2. Using environment variables for API keys
URX_VT_API_KEY=*** URX_URLSCAN_API_KEY=*** urx example.com --providers=vt,urlscan

# 3. Auto-enabling: providers are automatically added when API keys are provided
urx example.com --vt-api-key=*** --urlscan-api-key=*** # No need to specify in --providers

# 4. Multiple API key rotation (to mitigate rate limits)
# Using repeated flags for multiple keys
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3

# Using environment variables with comma-separated keys
URX_VT_API_KEY=key1,key2,key3 URX_URLSCAN_API_KEY=ukey1,ukey2 urx example.com

# Combining CLI flags and environment variables (CLI keys are used first)
URX_VT_API_KEY=env_key1,env_key2 urx example.com --vt-api-key=cli_key1 --vt-api-key=cli_key2

# URLs from robots.txt and sitemap.xml are included by default

# Exclude URLs from robots.txt files
urx example.com --exclude-robots

# Exclude URLs from sitemap
urx example.com --exclude-sitemap

# Include subdomains
urx example.com --subs

# Check status of collected URLs
urx example.com --check-status

# Read URLs directly from a text file
urx --files urls.txt

# Combine file input with filtering
urx --files urls.txt --patterns api,admin -f json

# Extract additional links from collected URLs
# (anchors, scripts, stylesheets, form actions, iframes, images, media
#  sources, objects, embeds, and meta-refresh targets)
urx example.com --extract-links

# Discovered links go through the same filters as everything else, so this
# keeps only the JavaScript the pages reference
urx example.com --extract-links -e js

# Network configuration
urx example.com --proxy http://localhost:8080 --timeout 60 --parallel 10 --insecure

# Advanced filtering
urx example.com -e js,php --patterns admin,login --exclude-patterns logout,static --min-length 20

# HTTP Status code based filtering (live requests: urx re-fetches each URL)
urx example.com --include-status 200,30x,405 --exclude-status 20x

# Archive-side filtering (free: the CDX index already knows these)
# Skip everything the archive recorded as a 404 — no extra requests
urx example.com --archive-exclude-status 404

# Only captures the archive served as JSON — finds extensionless API endpoints
urx example.com --archive-mime application/json

# Drop HTML to leave assets and endpoints behind
urx example.com --archive-exclude-mime text/html

# Restrict the crawl window across wayback, cc, arquivo, and any --cdx-endpoint alike
urx example.com --from 2023 --to 2024

# Disable host validation
urx example.com --strict false

# URL normalization and deduplication
# Normalize URLs by sorting query parameters and removing trailing slashes
urx example.com --normalize-url

# Combine normalization with endpoint merging for comprehensive deduplication
urx example.com --normalize-url --merge-endpoint

# URL normalization with file input
urx --files urls.txt --normalize-url

# Collapse /post/1, /post/2, /post/99999 ... into a single representative line
urx example.com --dedup-similar

# Regular-expression filtering (repeat either flag; they are never comma-split)
urx example.com --match-regex '/api/v[0-9]+/'
urx example.com --match-regex '\.php$' --match-regex '\.aspx$'
urx example.com --filter-regex '/(assets|static)/'

# Regexes are case-sensitive; ask for insensitivity explicitly
urx example.com --match-regex '(?i)admin'

# Security presets: match by path shape as well as by extension
urx example.com -p only-secrets   # /.env, /.git/config, id_rsa, *.pem
urx example.com -p only-backup    # *.bak, *.sql, /backup/, index.php~
urx example.com -p only-config    # *.yaml, web.config, .htaccess, Dockerfile
urx example.com -p only-api       # /api/, /v1/, /graphql, /swagger, *.wsdl
```

### Regular-expression Filtering

`--patterns` / `--exclude-patterns` are plain substring tests: both sides are
lower-cased, and every metacharacter is a literal. `--match-regex` /
`--filter-regex` are the regex counterparts, and they differ in three ways worth
remembering:

| | `--patterns` | `--match-regex` |
|---|---|---|
| Matching | substring | full [regex syntax](https://docs.rs/regex/latest/regex/#syntax) |
| Case | insensitive (both sides lower-cased) | **sensitive** — use `(?i)` to opt out |
| Multiple values | one comma-separated flag | repeat the flag; commas are never split |

Both regex flags are evaluated against the **whole URL string** as collected
(scheme, host, path, and query), so `^https://` and `\.js$` both work.
Exclusion wins: a URL matching `--filter-regex` is dropped even if
`--match-regex` also matched it. A malformed expression fails the run at
startup, before any archive is queried.

### Collapsing Near-duplicates

An archive will happily hand back `/post/1` through `/post/99999`. They are one
endpoint, and `--dedup-similar` prints one line for them. A path segment is
treated as data — not as part of the route — when it is entirely one of:

* a run of digits (`/post/1`, `/page/42`)
* a UUID (`/u/550e8400-e29b-41d4-a716-446655440000`)
* a 32/40/64-character hex digest (md5, sha1, sha256)
* a separated date (`/blog/2024-01-02/`)
* a long mixed-case token with digits in it (session ids, signed blobs)

Segments that merely *contain* digits stay put, so `/api/v1/` and `/api/v2/` are
still two endpoints, and a lower-case slug is prose rather than a token. Query
strings are grouped by parameter *names* only: `?q=cats&page=1` and
`?q=dogs&page=7` collapse, while `?q=cats` alone does not — dropping a
parameter changes the request.

The survivor of each group is its lexicographically smallest URL, so two runs
over the same data print the same thing. `--verbose` reports how many URLs were
collapsed. The option is independent of `--normalize-url` and
`--merge-endpoint` and combines with either; all three need the complete result
set, so none of them works with `--stream`.

### Streaming Output

By default urx collects everything, then filters, sorts, and prints once. On a
large target that means no output at all until the slowest archive finishes.
`--stream` writes each URL the moment the provider reporting it comes back:

```bash
# Matches start appearing immediately instead of after the slowest provider
urx big-target.com --stream | grep admin

# Line-delimited JSON stays valid while it is still being written
urx big-target.com --stream -f jsonl | jq -r 'select(.url | test("/api/")) | .url'
```

Streamed URLs pass exactly the same filters as a batch run and are still
deduplicated. Two things differ:

* **Order.** Results arrive in provider-completion order, so the output is
  unsorted. Pipe through `sort` if you need ordering.
* **Scope.** Options that need the complete result set are rejected up front
  (with a message naming each one): `--merge-endpoint`, `--dedup-similar`,
  `--check-status` /
  `--include-status` / `--exclude-status`, `--extract-links`, `--incremental`,
  `--show-sources`, `--show-meta`, `--output-dir`, and `--files`. Caching is
  bypassed, and
  `--format json` is refused in favour of `jsonl` because a JSON array has to
  know which entry is last.

Because the batch result map is never populated in this mode, a streamed run
also holds far less in memory — only the dedup set of URLs already written.

### Archive Capture Metadata

A CDX index records more than the URL: every capture carries a timestamp, the
MIME type and HTTP status the archive saw, and a digest of the body. urx keeps
all of it, so the CDX-backed providers — `wayback`, `cc`, `arquivo`, and any
`--cdx-endpoint` — report each URL together with:

| Field | Meaning |
|---|---|
| `first_seen` | Oldest capture timestamp, 14-digit CDX form (`YYYYMMDDhhmmss`) |
| `last_seen` | Newest capture timestamp |
| `mime` | MIME type of the most recent capture that recorded one |
| `archive_status` | HTTP status the *archive* recorded at capture time |
| `digest` | A representative content digest across the captures |

`archive_status` is not `status`: `status` only appears under `--check-status`,
which re-requests the URL live now, whereas `archive_status` is what the crawler
got when it captured the page. A URL can perfectly well be `archive_status`
`200` and dead today.

Where the same URL comes from several captures or several archives, the values
are merged: `first_seen` is the oldest timestamp anyone reported, `last_seen`
the newest, and `mime`/`archive_status` come from the most recent capture that
had them. Providers with no capture index (`otx`, `vt`, `urlscan`, `zoomeye`,
`github`, `robots`, `sitemap`, and `--files` input) report the URL alone — no
values are invented for them.

How the metadata surfaces depends on the format:

* **`json` / `jsonl`** — each field appears as a key when it has a value and is
  omitted entirely when it does not, exactly like `sources`.
* **`csv`** — a column is added only when at least one row has a value for it,
  so a run with no metadata still produces a single `url` column.
* **plain text** — unchanged by default, one bare URL per line, so existing
  pipelines keep working. Pass `--show-meta` to append the fields.

```bash
# Rich records: when the URL was alive, and what it served
urx example.com --providers wayback -f jsonl
# {"url":"https://example.com/old.php","first_seen":"20040112093000",
#  "last_seen":"20180722140311","mime":"text/html","archive_status":"200",
#  "digest":"HT2DYGA5UKZCPBSFVCV3JOBXGW2G5UUA"}

# Triage by age: everything last captured before 2010
urx example.com -f jsonl | jq -r 'select(.last_seen < "20100101000000") | .url'

# Opt plain output into the metadata
urx example.com --providers wayback --show-meta
```

Streaming (`--stream`) reports URLs only. A URL is printed on first sighting,
before the captures that would widen its `first_seen`/`last_seen` range have
arrived, so `--show-meta` is rejected there for the same reason
`--show-sources` is.

A cache hit also carries no metadata: the cache stores URLs, so a domain served
from cache reports its URLs without capture fields. Use `--no-cache` (or wait
for the TTL) for a run that repopulates them.

### Archive-side Filtering

`--archive-status`, `--archive-mime`, `--from`, and `--to` are evaluated by the
archive's CDX index rather than by urx. Two consequences are worth knowing:

* They apply only to CDX-backed providers — `wayback`, `cc`, `arquivo`, and
  any `--cdx-endpoint`. Other providers ignore them; urx warns when none is
  enabled.
* The archives do not share one filter dialect. Wayback Machine (and any
  `--cdx-dialect classic` endpoint) treats values as **regular expressions**, so
  `--archive-status "30."` matches any 3xx. Common Crawl, Arquivo.pt and pywb
  endpoints match **exactly**, and their index ANDs repeated filters together —
  so a multi-value positive list like `--archive-status 200,301` is
  unsatisfiable there. urx skips that filter for those providers (with a
  warning) instead of sending a query that would come back empty. Multi-value
  *exclusions* mean "not this and not that" and work everywhere.

Use `--archive-status` when you want what the archive recorded at crawl time and
`--check-status` / `--include-status` when you want the target's status *now*;
the latter re-requests every URL.

### Custom CDX Endpoints

Every web archive built on pywb, OutbackCDX, or the Internet Archive's CDX
server exposes the same query API. Rather than hardcoding a provider per
archive, `--cdx-endpoint URL` turns any such server into a provider on the
spot:

```bash
# The Icelandic web archive, alongside the default providers
urx example.is --cdx-endpoint https://vefsafn.is/cdx

# Several at once; each gets its own progress line, stats row and rate limit
urx example.com --cdx-endpoint https://vefsafn.is/cdx --cdx-endpoint http://localhost:8080/cdx \
  --rate-limit-by cdx:vefsafn.is=1
```

* The provider id is `cdx:<host>` (`cdx:vefsafn.is`), which is what
  `--exclude-providers`, `--rate-limit-by`, `--stats` and `--show-sources` use.
  Naming an endpoint enables it; no `--providers` entry is needed, and
  `--providers cdx:vefsafn.is` runs it alone. `--list-providers` shows the
  endpoints named on the same command line with the ids they will run as.
* Everything the built-in CDX providers honour applies here too: `--subs`,
  `--from`/`--to`, the `--archive-*` filters, pagination, `--rate-limit`, and
  the capture metadata described above.
* `--cdx-dialect classic|pywb` names the server's dialect (field names, filter
  semantics, row format and pagination scheme all follow from it — see
  "Archive-side Filtering"). Left unset, urx probes the endpoint once per run
  and falls back to `pywb`, the more common dialect; set it explicitly when the
  probe cannot tell (an empty answer for an unknown domain, for instance).
* Can also be set in the config file (`cdx_endpoint = [...]`, `cdx_dialect`).

**Verified endpoints.** As of this writing, the only public endpoint confirmed
to work end to end is `https://vefsafn.is/cdx` (Landsbókasafn's Icelandic web
archive, pywb dialect). Two things to know about it: it ignores `limit`, `page`
and `showNumPages` and returns the complete result set for every query, which
urx handles; and after a handful of requests it may start answering with an
Anubis-style bot-protection page ("Session Verification"). urx detects an HTML
answer in place of CDX rows and reports it as a provider error naming the
endpoint — it is never counted as "no URLs". If you hit it, slow down with
`--rate-limit-by cdx:vefsafn.is=1` or retry later.

**Known not to work.** The UK Web Archive (`webarchive.org.uk`), the Library of
Congress web archive (`webarchive.loc.gov`), Bibliotheca Alexandrina, and the
National Library of Australia (`web.archive.org.au`) all sit behind bot
protection or redirects that block their CDX APIs from a command-line client.
urx does not attempt to work around that, so pointing `--cdx-endpoint` at them
yields the HTML-answer error above.

### Caching and Incremental Scanning

Urx supports caching to improve performance for repeated scans and incremental scanning to discover only new URLs.

```bash
# Enable caching with SQLite (default)
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db

# Use Redis for distributed caching
urx example.com --cache-type redis --redis-url redis://localhost:6379

# Incremental scanning - only show new URLs since last scan
urx example.com --incremental

# Set cache TTL (time-to-live) to 12 hours
urx example.com --cache-ttl 43200

# Disable caching entirely
urx example.com --no-cache

# Combine incremental scanning with filters
urx example.com --incremental -e js,php --patterns api

# Configuration file with caching settings
urx -c example/config.toml example.com
```

#### Caching Use Cases

```bash
# Daily monitoring - only alert on new URLs
urx target.com --incremental --silent | notify-tool

# Efficient domain lists processing
cat domains.txt | urx --incremental --cache-ttl 3600 > new_urls.txt

# Distributed team scanning with Redis
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# Fast re-scans during development
urx test-domain.com --cache-ttl 300  # 5-minute cache for rapid iterations
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

[![](https://raw.githubusercontent.com/hahwul/urx/refs/heads/main/CONTRIBUTORS.svg)](https://github.com/hahwul/urx/graphs/contributors)
