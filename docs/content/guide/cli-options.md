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
  --exclude-robots                   Exclude robots.txt discovery
  --exclude-sitemap                  Exclude sitemap.xml discovery
  --archived-discovery               Also read every distinct archived version of robots.txt and sitemap.xml (see "Archived robots.txt and sitemap.xml" below)
  --archived-discovery-limit <N>     Maximum archived documents fetched per domain by each archived provider; nested sitemaps count [default: 50]

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
  --archive-body                     Extract links from the *archived* body of each collected URL (see "Archived Response Bodies" below)
  --archive-body-limit <N>           Maximum archived bodies fetched per run; bounds distinct bodies, not URLs [default: 500]

Cache Options:
  --incremental              Only return new URLs compared to previous scans
  --cache-type <CACHE_TYPE>  Cache backend: sqlite or redis [default: sqlite]
  --cache-path <CACHE_PATH>  Path for SQLite cache database
  --redis-url <REDIS_URL>    Redis connection URL
  --cache-ttl <CACHE_TTL>    Cache TTL in seconds [default: 86400]
  --no-cache                 Disable caching entirely
```

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

## Archived Response Bodies

`--extract-links` fetches every collected URL from the live site, which is the
wrong place to look for the pages an OSINT sweep cares about most: the ones
that no longer exist. `--archive-body` fetches the bodies the Wayback Machine
*stored* instead, and runs exactly the link extraction described above over
them.

For every collected URL that carries a capture timestamp, urx replays that
capture in its raw form:

```text
https://web.archive.org/web/<timestamp>id_/<url>
```

The `id_` flag after the timestamp switches off the Wayback toolbar and link
rewriting, so the response is the original bytes with the original
`Content-Type`. Relative links inside the body resolve against the captured
URL, not the replay URL.

```bash
# Links from the archived bodies of everything the CDX providers found
urx example.com --archive-body

# Bound the run and pace it; the archive is one host no matter how many URLs
urx example.com --archive-body --archive-body-limit 200 --rate-limit 5

# Only the JavaScript those pages referenced back then
urx example.com --archive-body -e js
```

### Why this needs far fewer requests than waymore

Every CDX row carries a content digest, and two captures with the same digest
are byte-for-byte the same body. Archives are full of such duplicates: every
`?utm_source=` variant of a page, every `/index.html` next to its `/`, every
tracking-parameter permutation serves identical bytes, so a list of tens of
thousands of URLs routinely collapses to a few thousand distinct bodies.

waymore has no notion of this. It downloads one response per URL and copes with
the volume through a blunt `-l 5000` cap, which both hammers the archive and
truncates coverage. urx claims each digest the first time it is seen and skips
every later URL that would replay the same bytes, so the same coverage costs
one request per *distinct body* rather than one per URL. `--archive-body-limit`
(default 500) bounds distinct bodies, not URLs: duplicates never count against
it, and `--verbose` reports how many URLs were skipped as duplicates, how many
fell past the limit, and how many had no capture to replay.

### Details

- Only URLs with a capture timestamp qualify. The CDX providers (`wayback`,
  `cc`, `arquivo`) supply one; `--files` input, non-CDX providers, and cached
  results (the cache stores URLs only) have none. urx says so when there is
  nothing to replay; pass `--no-cache` to get fresh captures.
- The newest capture of each URL is replayed, and the digest of *that* capture
  is what deduplication keys on. A timestamp reported by another archive lands
  on the nearest Wayback capture; a URL the Wayback Machine never saw answers
  404 and is skipped quietly.
- Captures the archive recorded as errors are not mined, exactly as
  `--extract-links` ignores live error pages, and non-markup bodies are skipped
  without being parsed.
- Discovered links go through the same filters, host validation, and output
  transforms as URLs that came from a provider.
- Each body is capped at 10 MiB, the same guard `--extract-links` uses.
- `--rate-limit`, `--rate-limit-by wayback=N`, `--parallel`, `--proxy`,
  `--timeout`, and `--retries` apply to the replay requests. Under
  `--network-scope providers` the replay requests, being part of the testing
  stage, are left unconfigured like the other testers.
- Incompatible with `--stream`, like every option that runs after collection.

## Archived robots.txt and sitemap.xml

The `robots` and `sitemap` providers read the *live* files, which only say
what a site hides or lists today. `--archived-discovery` also reads every
distinct version of those files the Wayback Machine has stored. A `Disallow:`
from 2015 names paths the site has since stopped mentioning — often because
they were meant to be forgotten, not because they are gone — and an old
sitemap lists everything the site once wanted crawled.

```bash
# Every archived version of robots.txt and sitemap.xml, alongside the live ones
urx example.com --archived-discovery

# Bound it and pace it; both archived providers answer to --rate-limit-by
urx example.com --archived-discovery --archived-discovery-limit 20 --rate-limit-by robots=2,sitemap=2

# Only the versions captured in a given era
urx example.com --archived-discovery --from 2014 --to 2016

# Just the robots.txt history
urx example.com --archived-discovery --exclude-sitemap --show-sources
```

### How it works

1. The versions of each document are listed with one CDX query per file name
   (`robots.txt`, `sitemap.xml`, `sitemap_index.xml`, `sitemap.txt`):

   ```text
   /cdx/search/cdx?url=<domain>/robots.txt&fl=original,timestamp,statuscode,digest
       &collapse=digest&filter=statuscode:2..
   ```

   `collapse=digest` folds consecutive captures that served the same bytes into
   one row, so a file crawled daily but edited yearly comes back as one row per
   *change*. The status filter is what keeps that cheap: the CDX urlkey folds
   `www.` and the apex into one listing, and their interleaved `301`/`200` rows
   otherwise defeat the collapse. Measured on github.com/robots.txt: 325,036
   rows without the filter, 13,909 with it, for the same 107 distinct versions.
   Any duplicate digest that survives is dropped client-side.
2. Each distinct version is replayed in raw form
   (`/web/<timestamp>id_/<original url>`) and handed to the **same parser as
   the live file**. There is no second parser: a 2015 robots.txt is read by
   exactly the rules the current one is, including the absolute-path and
   pattern-skipping guards, and its paths land on the host that actually
   served it. An archived `<sitemapindex>` is followed into its children at
   that same timestamp, with the same same-host rule as the live walk.
3. Captures the archive recorded as anything but a success (github.com's
   robots.txt was a 401 for part of 2007) are never requested. They are
   counted and reported under `--verbose` only, as is any version the replay
   endpoint refuses.

### Details

- `--archived-discovery-limit` (default 50) caps the documents fetched per
  domain by each archived provider. The newest versions are read first — the
  live provider already covers the present, and recently-removed paths are the
  ones most likely to still exist — and nested sitemaps count against the
  cap. `--verbose` says when the cap cut the list short.
- The archived reads run as their own provider instances, labelled
  "Robots.txt (archived)" and "Sitemap (archived)" in `--stats` and
  `--show-sources`, but they are registered under the existing `robots` and
  `sitemap` ids rather than as new providers. `--exclude-robots`,
  `--exclude-sitemap`, and `--rate-limit-by robots=N` / `sitemap=N` therefore
  govern the live and archived reads together.
- `--from` / `--to` narrow which versions are considered; the other
  `--archive-*` predicates do not apply to a version history.
- Bodies are capped exactly as the live files are (1 MiB for robots.txt,
  50 MiB per sitemap document).
- Because it is a provider, it works with `--stream` and its results are
  cached like any other provider's (the cache key includes the flag).
