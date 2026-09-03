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
      --completions <SHELL>       Print a shell completion script (bash, zsh, fish, powershell, elvish) to stdout and exit
      --manpage                   Print the roff man page to stdout and exit
  -h, --help             Print help
  -V, --version          Print version

Input Options:
      --files <FILES>...     Read URLs directly from files (supports WARC, URLTeam compressed, and text files)
      --domain-list <PATH>   File of newline-separated domains to scan (repeatable; merged with positional DOMAINS and stdin; `#` comments allowed)

Output Options:
  -o, --output <OUTPUT>          Output file to write results
      --output-dir <PATH>        Write one file per domain into this directory; extension matches --format. Coexists with --output / stdout.
  -f, --format <FORMAT>          Output format: "plain", "json", "jsonl", "csv" [default: plain]
      --stream                   Write URLs as providers report them (unsorted, bypasses cache)
      --merge-endpoint           Merge endpoints with the same path and merge URL parameters
      --normalize-url            Normalize URLs for better deduplication
      --dedup-similar            Collapse URLs differing only in ids, hashes, dates, or query values

Provider Options:
  --providers <PROVIDERS>                Providers to use (comma-separated) [default: wayback,cc,otx]
  --exclude-providers <PROVIDERS>        Providers to exclude (wins on conflict)
  --all-providers                        Enable every supported provider (API-keyed ones only if a key is available)
  --list-providers                       List every supported provider then exit
  --subs                                 Include subdomains when searching
  --cc-index <CC_INDEX>                  Common Crawl index(es), comma-separated for parallel queries; `latest` auto-resolves [default: latest]
  --from <DATE>                          Restrict CDX providers to captures >= DATE (YYYY/YYYYMM/YYYYMMDD/YYYYMMDDhhmmss)
  --to <DATE>                            Restrict CDX providers to captures <= DATE (same format as --from)
  --archive-status <CODES>               Keep only captures the archive recorded with these status codes
  --archive-exclude-status <CODES>       Drop captures the archive recorded with these status codes
  --archive-mime <TYPES>                 Keep only captures with these recorded MIME types
  --archive-exclude-mime <TYPES>         Drop captures with these recorded MIME types
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
      --no-color      Disable ANSI color (NO_COLOR is also honored)
      --show-sources  Annotate output URLs with the providers that returned them
      --show-meta     Annotate plain-text URLs with the archive capture metadata
      --stats         Print a per-provider summary to stderr at end of run

Filter Options:
  -p, --preset <PRESET>                     Filter Presets (e.g., "no-resources,no-images,only-js,only-secrets,only-api")
  -e, --extensions <EXTENSIONS>              Filter by extensions (e.g., "js,php,aspx")
      --exclude-extensions <EXTENSIONS>      Exclude extensions (e.g., "html,txt")
      --patterns <PATTERNS>                  Include URLs containing patterns
      --exclude-patterns <PATTERNS>          Exclude URLs containing patterns
      --match-regex <RE>                     Keep only URLs matching this regex (repeatable, ORed, case-sensitive)
      --filter-regex <RE>                    Drop URLs matching this regex (repeatable; one match is enough)
      --show-only-host                       Only show the host part
      --show-only-path                       Only show the path part
      --show-only-param                      Only show the parameters part
      --min-length <MIN_LENGTH>              Minimum URL length
      --max-length <MAX_LENGTH>              Maximum URL length
      --strict                               Enforce exact host validation (default)
      --no-strict                            Disable host validation entirely (wins over --strict)

Network Options:
  --network-scope <SCOPE>        Apply settings to: all, providers, testers, providers,testers [default: all]
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
  --extract-links                    Extract additional links from collected URLs (see "Link Extraction" below)

Cache Options:
  --incremental              Only return new URLs compared to previous scans
  --cache-type <CACHE_TYPE>  Cache backend: sqlite or redis [default: sqlite]
  --cache-path <CACHE_PATH>  Path for SQLite cache database
  --redis-url <REDIS_URL>    Redis connection URL
  --cache-ttl <CACHE_TTL>    Cache TTL in seconds [default: 86400]
  --no-cache                 Disable caching entirely

Notification Options:
  --notify <URL>                   POST a run summary to this webhook when the run ends (repeatable; also URX_NOTIFY_URL, provider-config `notify_url`, or `[notify].url`)
  --notify-on <NOTIFY_ON>          When to send: new (only if URLs were emitted), always, or never [default: new]
  --notify-format <NOTIFY_FORMAT>  Payload shape: json (urx summary), slack ({"text"}), or discord ({"content"}) [default: json]
```

## Webhook Notifications

`--notify <URL>` POSTs a summary of the run to a webhook once the run ends.
Combined with `--incremental` it turns urx into a monitor: run it from cron
and the webhook fires only when the archives have something new.

```bash
# Slack, only when new URLs turned up (the default --notify-on new)
urx target.com --incremental --silent \
  --notify https://hooks.slack.com/services/T000/B000/XXXX --notify-format slack

# Discord, every run
urx target.com --incremental --notify "$DISCORD_HOOK" --notify-format discord --notify-on always

# Fan out to several receivers with urx's JSON schema
urx target.com --incremental --notify https://n8n.example/hook --notify https://ntfy.example/urx
```

### When it sends

| `--notify-on` | Behaviour |
|---------------|-----------|
| `new` (default) | Only when the run emitted at least one URL. Under `--incremental` that means "at least one URL the previous run had not seen". |
| `always` | After every run, including one with zero URLs. |
| `never` | Keeps the configuration in place but sends nothing. |

### Payload formats

**`json`** (default) — urx's own schema:

```json
{
  "tool": "urx",
  "version": "0.10.0",
  "domains": ["example.com"],
  "incremental": true,
  "url_count": 12,
  "new_url_count": 12,
  "elapsed_ms": 3210,
  "providers": [
    {"name": "Wayback Machine", "urls": 1200, "errors": 0, "partial": 0, "elapsed_ms": 2500, "aborted": false}
  ],
  "sample": ["https://example.com/api/v2/users", "..."],
  "sample_truncated": false
}
```

`providers` carries the same numbers `--stats` prints. `sample` holds at most
20 emitted URLs, in output order; `sample_truncated` is `true` when the run
found more. Under `--stream` the URLs were written as they arrived, so the
payload carries the count and an empty sample.

**`slack`** sends `{"text": "..."}`, **`discord`** sends `{"content": "..."}`.
Both carry a short message: a header line with the count, the targets and the
elapsed time, one line of provider totals, then the URL sample. Discord caps a
message at 2000 characters and Slack messages become unreadable past 4000, so
the text is cut at that limit on a line boundary and ends with
`[truncated: N lines cut to fit the message limit]`. A URL is never sliced in
the middle.

### Failure handling

Delivery never changes the exit code. By the time the webhook is called the
URLs are already on stdout or in `--output`, so a webhook that is down, slow,
or answering 4xx/5xx produces a warning on stderr and the run still exits 0.
`--verbose` prints the HTTP status of each delivery. `--silent` hides those
lines but still sends. Each URL is tried exactly once — chat webhooks are not
idempotent, and a retry after a slow-but-delivered request posts twice.

### The URL is a secret

A Slack or Discord webhook URL *is* the credential. urx prints only its scheme
and host (`https://hooks.slack.com`) anywhere it mentions the destination —
verbose output, warnings, error text — and the payload never contains it. To
keep it out of a config you check in, use the `URX_NOTIFY_URL` environment
variable or `notify_url` in the provider-config file; `[notify].url` in the
main config works as well. Precedence is CLI/env > provider-config > main
config, the same order the API keys follow.

### Network settings

The request honours `--proxy`, `--proxy-auth`, `--timeout` and `--insecure`.
`--network-scope` is not consulted: that flag partitions the traffic urx sends
at the archives (providers) and at the target (testers), and the webhook is
neither — it is your own endpoint, reached with whatever egress settings the
run was given.

## Archive Capture Metadata

The CDX-backed providers (`wayback`, `cc`, `arquivo`) index *captures*, not just
URLs, so every row they return already carries when the capture was taken, what
it served, and a digest of the body. urx keeps those fields and reports them
alongside each URL.

| Field | Meaning |
|-------|---------|
| `first_seen` | Oldest capture timestamp, 14-digit CDX form (`YYYYMMDDhhmmss`) |
| `last_seen` | Newest capture timestamp |
| `mime` | MIME type of the most recent capture that recorded one |
| `archive_status` | HTTP status the *archive* recorded at capture time |
| `digest` | A representative content digest across the captures |

`archive_status` is what the crawler saw when it captured the page. It is not
the same as `status`, which only appears under `--check-status` and comes from
re-requesting the URL live.

When the same URL arrives from several captures or several archives, the fields
merge: `first_seen` is the oldest timestamp anyone reported, `last_seen` the
newest, and `mime`/`archive_status` come from the most recent capture that had
them. Providers with no capture index (`otx`, `vt`, `urlscan`, `zoomeye`,
`github`, `robots`, `sitemap`) and `--files` input report the URL alone; no
values are invented for them, and a domain served from cache has none either
(the cache stores URLs only).

Per format:

* `json` / `jsonl` — a key per field, present only when it has a value.
* `csv` — a column per field, added only when at least one row has a value.
* `plain` — unchanged by default (one bare URL per line, for piping); pass
  `--show-meta` to append `first_seen=… last_seen=… mime=…` after the URL.

`--show-meta` is incompatible with `--stream`, which prints a URL on first
sighting — before the captures that would widen its `first_seen`/`last_seen`
range have arrived.

```bash
urx example.com --providers wayback -f jsonl
urx example.com -f jsonl | jq -r 'select(.last_seen < "20100101000000") | .url'
urx example.com --providers wayback --show-meta
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
| GitHub Code Search | `github` | Yes | `URX_GITHUB_API_KEY` |

Default providers: `wayback,cc,otx`. Providers requiring API keys are automatically enabled when their keys are provided. `arquivo` (the Portuguese web archive) is keyless but opt-in — add it with `--providers` or enable everything with `--all-providers`. URLScan works anonymously without a key (rate-limited to ~30 requests/min per IP); a key only raises those limits and enables rotation. `github` searches GitHub Code Search and requires a personal access token (`--github-api-key` or `URX_GITHUB_API_KEY`).

Run `urx --list-providers` to print the full catalog (id, API-key requirement, and a one-line summary) directly from the binary.

## Shell Completions and the Man Page

Both are generated by the binary itself, so they always describe the flags of
the version you have installed — there is nothing to keep in sync by hand, and
neither flag needs a target domain.

```bash
# zsh — write into any directory on your $fpath, then re-run compinit
urx --completions zsh > ~/.zfunc/_urx

# bash
urx --completions bash > ~/.local/share/bash-completion/completions/urx

# fish
urx --completions fish > ~/.config/fish/completions/urx.fish

# powershell
urx --completions powershell | Out-String | Invoke-Expression

# elvish
urx --completions elvish > ~/.config/elvish/lib/urx.elv
```

```bash
# man page
urx --manpage > ~/.local/share/man/man1/urx.1
man urx
```

Regenerate after upgrading urx to pick up new flags.

## Filter Presets

Exclude a family with a `no-*` preset, or keep only a family with an `only-*`
preset. Singular spellings (e.g. `no-image`, `only-font`) are accepted too.

| Preset | Description |
|--------|-------------|
| `no-resources` | Exclude resource files (images, CSS, fonts, documents, videos, audio) |
| `no-images` | Exclude image files |
| `no-fonts` | Exclude font files |
| `no-documents` | Exclude document files |
| `no-videos` | Exclude video files |
| `no-audio` | Exclude audio files |
| `only-js` | Only JavaScript files |
| `only-style` | Only stylesheet files |
| `only-fonts` | Only font files |
| `only-documents` | Only document files |
| `only-videos` | Only video files |
| `only-audio` | Only audio files |
| `only-images` | Only image files |

### Security Presets

These four go beyond file extensions: a URL qualifies when it carries a listed
extension **or** when its path has a listed shape. That is what lets
`only-secrets` catch `/.env` (a dotfile with no extension at all) and
`only-backup` catch `/index.php~` (an ordinary name with an editor suffix).

| Preset | Description |
|--------|-------------|
| `only-secrets` | Leaked credentials and VCS metadata: `/.env`, `/.git/`, `/.svn/`, `/.aws/`, `/.ssh/`, `id_rsa`, `.htpasswd`, `credentials`, `*.pem`, `*.key`, `*.p12` |
| `only-backup` | Backups and archived copies: `*.bak`, `*.old`, `*.orig`, `*.swp`, `*.sql`, `*.dump`, `*.zip`, `*.tar.gz`, `/backup/`, and paths ending in `~` |
| `only-config` | Configuration files: `*.conf`, `*.config`, `*.ini`, `*.yaml`, `*.yml`, `*.toml`, `*.properties`, `web.config`, `.htaccess`, `.npmrc`, `Dockerfile` |
| `only-api` | API surfaces: `/api/`, `/v1/`–`/v4/`, `/rest/`, `/graphql`, `/swagger`, `/openapi`, `/wp-json`, `*.wsdl` |

Singular and plural spellings both work here too (`only-secret`, `only-backups`,
`only-configs`, `only-apis`). Presets combine by OR, so
`-p only-secrets,only-backup` keeps everything either one would.

```bash
urx example.com -p only-secrets
urx example.com -p only-backup,only-config
```

## Regular-expression Filtering

`--patterns` and `--exclude-patterns` are substring tests. `--match-regex` and
`--filter-regex` are the [regex](https://docs.rs/regex/latest/regex/#syntax)
equivalents and behave differently in three ways:

| | `--patterns` | `--match-regex` |
|---|---|---|
| Matching | substring | full regex syntax |
| Case | insensitive (both sides lower-cased) | **sensitive** — prefix `(?i)` to opt out |
| Multiple values | one comma-separated flag | repeat the flag; commas are never split |

The expression is applied to the whole URL string as collected — scheme, host,
path, and query — so `^https://` and `\.js$` both work. Several
`--match-regex` values are ORed; a single `--filter-regex` hit is enough to drop
a URL, and exclusion beats inclusion. A malformed expression aborts the run at
startup, before any archive is queried, rather than failing silently per URL.

```bash
# Versioned API paths only
urx example.com --match-regex '/api/v[0-9]+/'

# Two alternatives, one per flag (a comma inside a regex stays intact)
urx example.com --match-regex '\.php$' --match-regex '/admin/[a-z]{3,8}$'

# Drop build output, keep everything else
urx example.com --filter-regex '/(assets|static|dist)/'
```

## Collapsing Near-duplicates

`--dedup-similar` prints one line for a group of URLs that are the same endpoint
carrying different data — the `/post/1` … `/post/99999` problem that turns a real
run into an unreadable wall of output.

A path segment counts as data, rather than as part of the route, when the whole
segment is one of:

* a run of digits — `/post/1`, `/page/42`
* a UUID — `/u/550e8400-e29b-41d4-a716-446655440000`
* a 32/40/64-character hex digest (md5, sha1, sha256)
* a separated date — `/blog/2024-01-02/`
* a long mixed-case token containing digits (session ids, signed blobs)

A segment that merely contains digits is left alone, so `/api/v1/` and `/api/v2/`
stay distinct, and a lower-case slug reads as prose rather than as a token.
Query strings are grouped by parameter *names* only: `?q=cats&page=1` and
`?q=dogs&page=7` collapse together, while `?q=cats` on its own does not.

The URL kept from each group is the lexicographically smallest one, so repeated
runs over the same data produce identical output. `--verbose` reports how many
URLs were collapsed.

`--dedup-similar`, `--normalize-url`, and `--merge-endpoint` are independent and
can be combined; they run in that order of increasing aggressiveness. All three
need the complete result set, so none of them can be used with `--stream`.

```bash
urx example.com --dedup-similar --verbose
urx --files urls.txt --normalize-url --merge-endpoint --dedup-similar
```

## Link Extraction

`--extract-links` re-fetches every URL that survived filtering and mines the
response HTML for more. It reads every URL-bearing tag, not only anchors:

| Tag | Attribute | Typically finds |
|-----|-----------|-----------------|
| `<a>` | `href` | Navigation |
| `<script>` | `src` | JavaScript bundles |
| `<link>` | `href` | Stylesheets, icons, preloads, canonical/alternate URLs |
| `<form>` | `action` | Endpoints that are never linked |
| `<iframe>` | `src` | Embedded apps and widgets |
| `<img>` | `src` | Images, including CDN hosts |
| `<source>` | `src` | Media alternatives inside `<video>` / `<audio>` |
| `<object>` | `data` | Legacy embedded objects |
| `<embed>` | `src` | Legacy plugin content |
| `<meta http-equiv="refresh">` | `content` | Markup redirects (`0; url=...`) |

Details worth knowing:

- Relative URLs resolve against the page, honouring a `<base href>` when the
  document declares one.
- Non-fetchable targets are skipped: `javascript:`, `mailto:`, `tel:`, `data:`,
  `about:`, `blob:`, and bare `#fragment` references.
- Duplicates are collapsed, so a logo referenced from a dozen places is
  reported once.
- Discovered links go through the same filters, host validation, and output
  transforms as URLs that came from a provider — `--extract-links -e js`
  returns only JavaScript.
- Only responses that succeeded and look like markup are parsed, and each body
  is capped at 10 MiB.

```bash
# Crawl one hop deeper and keep only JavaScript
urx example.com --extract-links -e js

# Extraction obeys the network settings too
urx example.com --extract-links --proxy http://localhost:8080 --timeout 20
```
