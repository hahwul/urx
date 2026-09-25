+++
title = "CLI Options"
description = "Every command-line flag urx accepts, plus the webhook notification payloads and their failure handling."
toc = true
weight = 1
+++

## Command Line Options

Urx provides a comprehensive set of command-line options for customizing behavior.

```
Usage: urx [OPTIONS] [DOMAINS]... [COMMAND]

Commands:
  cache  Inspect and maintain the URL cache (stats, list, prune, drop, clear)
  help   Print this message or the help of the given subcommand(s)

Arguments:
  [DOMAINS]...  Domains to fetch URLs for

Options:
  -c, --config <CONFIG>           Config file to load
      --provider-config <PATH>    Separate provider config holding only API keys (default: ~/.config/urx/provider-config.toml; %APPDATA%\urx\provider-config.toml on Windows)
      --completions <SHELL>       Print a shell completion script (bash, zsh, fish, powershell, elvish) to stdout and exit
      --manpage                   Print the roff man page to stdout and exit
  -h, --help             Print help
  -V, --version          Print version

Input Options:
      --files <FILES>...     Read URLs directly from files (supports WARC, URLTeam compressed, and text files)
      --domain-list <PATH>   File of newline-separated domains to scan (repeatable; merged with positional DOMAINS; stdin is read only when they name no domains; `#` comments allowed) [alias: --dL]

Output Options:
  -o, --output <OUTPUT>          Output file to write results
      --output-dir <PATH>        Write one file per URL host into this directory; extension matches --format. Coexists with --output / stdout. [alias: --oD]
  -f, --format <FORMAT>
          Output format: plain text, a JSON array, JSON Lines, CSV, or a URL wordlist

          [default: plain]
          [possible values: plain, json, jsonl, csv, wordlist]
      --merge-endpoint           Merge endpoints with the same path and merge URL parameters
      --stream                   Write URLs as providers report them (plain/jsonl/csv only; unsorted; bypasses cache)
      --normalize-url            Normalize URLs for better deduplication
      --dedup-similar            Collapse URLs differing only in ids, hashes, dates, or query values
      --params                   Replace the URL list with every query parameter name the run saw, once each
      --params-by-endpoint       One line per endpoint: the endpoint and the union of the parameter names seen on it
      --fuzz-placeholder <VALUE> Replace every query parameter value with VALUE, one URL per parameter signature

Provider Options:
  --providers <PROVIDERS>                Providers to use (comma-separated) [default: wayback,cc,otx]
  --exclude-providers <PROVIDERS>        Providers to exclude (wins on conflict)
  --all-providers                        Enable every supported provider (API-keyed ones only if a key is available)
  --list-providers                       List every supported provider then exit
  --subs                                 Include subdomains when searching
  --cc-index <CC_INDEX>                  Common Crawl index(es), comma-separated for parallel queries; `latest` auto-resolves [default: latest]
  --cdx-endpoint <URL>                   Query an additional CDX index server (pywb / OutbackCDX / classic) by its API URL; repeatable; id `cdx:<host>` (`cdx:<host>:<port>` with a port)
  --cdx-dialect <DIALECT>                Dialect of the --cdx-endpoint servers: `pywb` or `classic` (unset: probed once, pywb fallback)
  --from <DATE>                          Restrict CDX providers to captures >= DATE (YYYY/YYYYMM/YYYYMMDD/YYYYMMDDhhmmss); legacy alias --wayback-from
  --to <DATE>                            Restrict CDX providers to captures <= DATE (same format as --from); legacy alias --wayback-to
  --archive-status <CODE>                Keep only captures the archive recorded with this status code (a list works on wayback/classic endpoints only)
  --archive-exclude-status <CODES>       Drop captures the archive recorded with these status codes
  --archive-mime <TYPE>                  Keep only captures with this recorded MIME type (a list works on wayback/classic endpoints only)
  --archive-exclude-mime <TYPES>         Drop captures with these recorded MIME types
  --vt-api-key <VT_API_KEY>             API key for VirusTotal
  --urlscan-api-key <URLSCAN_API_KEY>   Optional API key for Urlscan (also works anonymously)
  --zoomeye-api-key <ZOOMEYE_API_KEY>   API key for ZoomEye
  --github-api-key <GITHUB_API_KEY>     Personal access token for GitHub Code Search (URX_GITHUB_API_KEY)
  --bevigil-api-key <BEVIGIL_API_KEY>   API key for BeVigil, URLs from unpacked Android apps (URX_BEVIGIL_API_KEY)

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
      --no-strict                            Disable host validation; a target's path scope still applies (wins over --strict)
      --scope-file <FILE>                    Bug-bounty host scope file (`!` excludes, `*.host` wildcard, `[2001:db8::1]` exact IPv6); repeatable
      --meta-first-seen-after <DATE>         Keep URLs whose oldest archived capture is on or after DATE
      --meta-first-seen-before <DATE>        Keep URLs whose oldest archived capture is on or before DATE
      --meta-last-seen-after <DATE>          Keep URLs whose newest archived capture is on or after DATE ("still alive as of")
      --meta-last-seen-before <DATE>         Keep URLs whose newest archived capture is on or before DATE ("dead since")
      --meta-mime <TYPE>                     Keep only URLs with these archived MIME types (`image/*` matches any subtype)
      --meta-exclude-mime <TYPE>             Drop URLs with these archived MIME types
      --meta-status <CODE>                   Keep only URLs with these archived status codes (20x / 5xx patterns)
      --meta-exclude-status <CODE>           Drop URLs with these archived status codes

Network Options:
  --network-scope <SCOPE>        Apply settings to: all, providers, testers, providers,testers [default: all]
  --proxy <PROXY>                HTTP proxy (e.g., http://proxy:8080)
  --proxy-auth <PROXY_AUTH>      Proxy credentials (username:password)
  --insecure                     Skip SSL certificate verification
  --random-agent                 Use a random User-Agent
  -H, --header <NAME: VALUE>     Extra request header, repeatable; sent to the target only, never to an archive
  --cookie <COOKIES>             Cookie header for requests to the target
  --user-agent <STRING>          User-Agent for requests to the target, overriding --random-agent
  --timeout <TIMEOUT>            Request timeout in seconds [default: 120]
  --retries <RETRIES>            Retries for failed requests [default: 2]
  --parallel <PARALLEL>          Max domains fetched concurrently per provider, and concurrent URL tests (rate-limit shared) [default: 5]
  --rate-limit <RATE_LIMIT>      Requests per second
  --rate-limit-by <PAIRS>        Per-provider rate overrides (e.g. `vt=1,wayback=10`); falls back to --rate-limit for unlisted providers
  --max-time <SECONDS>           Global ceiling on provider enumeration time in seconds; in-flight fetches are aborted at deadline (0 = unlimited) [default: 0]

Testing Options:
  --check-status                     Check HTTP status code of collected URLs [alias: --cs]
  --include-status <INCLUDE_STATUS>  Include specific status codes (e.g., 200,30x); implies the status check [alias: --is]
  --exclude-status <EXCLUDE_STATUS>  Exclude specific status codes (e.g., 404,50x); implies the status check [alias: --es]
  --extract-links                    Extract additional links from collected URLs (see "Link Extraction" below)
  --extract-js-endpoints             Fetch collected JavaScript and extract the endpoints in its string literals; with --archive-body also mines archived scripts (see "JavaScript Endpoint Extraction" below)
  --max-js-files <N>                 Maximum number of files --extract-js-endpoints will fetch (0 = unlimited) [default: 500]
  --archive-body                     Extract links from the *archived* body of each collected URL (see "Archived Response Bodies" below)
  --archive-body-limit <N>           Maximum archived bodies fetched per run; bounds distinct bodies, not URLs [default: 500]
  --archive-body-dir <DIR>           Keep every replayed body in DIR with an index.jsonl mapping it back to its URL
  --expand-specs                     Fetch collected OpenAPI/Swagger/GraphQL documents and expand every route they describe (see "API Specification Expansion" below)
  --max-spec-files <N>               Maximum number of specification documents --expand-specs will fetch (0 = unlimited) [default: 50]
  --check-title                      Also record each response's HTML <title>; implies --check-status

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

The one subcommand, `urx cache`, inspects and maintains the URL cache — see
[Inspecting and Maintaining the Cache](/guide/caching/#inspecting-and-maintaining-the-cache).
Everything else on this page belongs to the scan invocation.

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
  "version": "0.11.0",
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
payload carries the count and an empty sample (`sample_truncated` is then `true`
whenever any URL was emitted).

**`slack`** sends `{"text": "..."}`, **`discord`** sends `{"content": "..."}`.
Both carry a short message: a header line with the count, the targets and the
elapsed time, one line of provider totals, then the URL sample, followed by
`… N more not shown` when the run found more URLs than the sample holds. Discord caps a
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

A Slack or Discord webhook URL *is* the credential. urx prints only its scheme,
host and any non-default port (`https://hooks.slack.com`) anywhere it mentions the destination —
verbose output, warnings, error text — and the payload never contains it. To
keep it out of a config you check in, use the `URX_NOTIFY_URL` environment
variable or `notify_url` in the provider-config file; `[notify].url` in the
main config works as well. Precedence is CLI/env > provider-config > main
config, the same order the API keys follow. The `[notify]` section also takes
`on` and `format`, the config-file equivalents of `--notify-on` and
`--notify-format`.

### Network settings

The request honours `--proxy`, `--proxy-auth`, `--timeout` and `--insecure`.
`--network-scope` is not consulted: that flag partitions the traffic urx sends
at the archives (providers) and at the target (testers), and the webhook is
neither — it is your own endpoint, reached with whatever egress settings the
run was given.

## Archive Capture Metadata

The CDX-backed providers (`wayback`, `cc`, `arquivo`, and any `--cdx-endpoint`) index *captures*, not just
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
`github`, `bevigil`, `robots`, `sitemap`) and `--files` input report the URL alone; no
values are invented for them, and a domain served from cache has none either
(the cache stores URLs only).

Per format:

* `json` / `jsonl` — a key per field, present only when it has a value.
* `csv` — a column per field, added only when at least one row has a value.
* `plain` — unchanged by default (one bare URL per line, for piping); pass
  `--show-meta` to append ` [first_seen=… last_seen=… mime=…]` after the URL.

A URL rewritten by `--normalize-url` or `--merge-endpoint` no longer matches
what the providers reported, so it carries no `sources` or capture metadata.

### Source Attribution (`--show-sources`)

`--show-sources` names each source by its run label (the name `--stats` prints),
not by its `--providers` id: `Wayback Machine`, `CC (latest)` or the pinned index
name (e.g. `CC-MAIN-2026-17`), `OTX`, `Arquivo.pt`, `VirusTotal`, `Urlscan`,
`ZoomEye`, `GitHub`, `BeVigil`, `Robots.txt`, `Sitemap`, `Robots.txt (archived)`,
`Sitemap (archived)`, `cdx:<host>`, and `file` for `--files` input. JSON gives a
`sources` array, plain text a ` [a,b]` suffix, and CSV one `sources` cell joined
with `|` (e.g. `CC (latest)|Wayback Machine`). A URL served from the cache, or
found by `--extract-links`, `--extract-js-endpoints`, `--archive-body` or
`--expand-specs`, has no sources.

`--show-sources` and `--show-meta` are incompatible with `--stream`, which
prints a URL on first sighting — before later providers could report it too, and
before the captures that would widen its `first_seen`/`last_seen`
range have arrived.

```bash
urx example.com --providers wayback -f jsonl
urx example.com -f jsonl | jq -r 'select(.last_seen and .last_seen < "20100101000000") | .url'
urx example.com --providers wayback --show-meta
```

## Live Response Metadata

`--check-status` already sends a request and waits for the response head, so the
fields that head carries come for free. urx keeps them alongside the status
code.

| Field | Meaning |
|-------|---------|
| `status` | The status the live request returned, as code and reason (`200 OK`), or `Status check failed` when the request itself failed (connection refused, timeout, TLS). Such URLs are kept unless `--include-status` is set |
| `location` | The `Location` header of a 3xx — recorded, never followed |
| `content_length` | The `Content-Length` header, verbatim |
| `content_type` | The `Content-Type` header, verbatim |
| `title` | The HTML `<title>`, only under `--check-title` |

When both `--include-status` and `--exclude-status` are given,
`--include-status` alone decides and `--exclude-status` is ignored, so use one
or the other.

`--check-status` deliberately does not follow redirects, so a reported status
always belongs to the URL that was asked for. `location` is where the 3xx
pointed, without urx ever going there.

`--check-title` is the one field that is not free: a title needs the response
*body*, so it sits behind its own flag. The read is bounded twice — at most
64 KiB (checked between chunks), and it stops at the closing tag — and skipped entirely for a body the
server declared as non-HTML, so a JSON API or an image costs nothing. The title
itself is whitespace-collapsed, entity-decoded and cut to 200 characters (with a
trailing `…` when it was longer).
`--check-title` implies `--check-status`; without that request there is nothing
to read a title from.

Exposure follows the same rule the archive metadata does:

* `json` / `jsonl` — a key per field, present only when it has a value.
* `csv` — a column per field, appended after the existing columns so an
  established consumer sees its columns unmoved. A title is chosen by the host
  being checked, so it goes through the same spreadsheet-formula escaping the
  URL does.
* `plain` — the URL and its ` [status]` per line. `--check-title` appends
  ` [title="…"]`; `--show-meta` adds `location`, `content_length` and
  `content_type` to that same bracket. The title is quoted, since it is the one
  value that routinely contains spaces.

```bash
# Status plus the response head, as JSON Lines
urx example.com --check-status -f jsonl

# Titles too, in plain text (add --show-meta for the response-head fields)
urx example.com --check-title

# Where did the redirects point?
urx example.com --check-status --is 30x -f jsonl | jq -r '.url + " -> " + .location'
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
| BeVigil | `bevigil` | Yes | `URX_BEVIGIL_API_KEY` |
| robots.txt discovery | `robots` | No | - |
| sitemap.xml discovery | `sitemap` | No | - |
| Custom CDX server | `cdx:<host>` (via `--cdx-endpoint URL`) | No | - |

Default providers: `wayback,cc,otx`, plus the `robots` and `sitemap` probes of
the target, which request its live `robots.txt` and `sitemap.xml` unless you pass
`--exclude-robots`, `--exclude-sitemap` or `--exclude-providers robots,sitemap`
(`--all-providers` leaves them to those same switches). Providers requiring API keys are automatically enabled when their keys are provided. `arquivo` (the Portuguese web archive) is keyless but opt-in — add it with `--providers` or enable everything with `--all-providers`. URLScan works anonymously without a key (rate-limited to ~30 requests/min per IP); a key only raises those limits and enables rotation. `github` searches GitHub Code Search and requires a personal access token (`--github-api-key` or `URX_GITHUB_API_KEY`). `bevigil` returns URLs that [BeVigil](https://bevigil.com/osint-api) extracted from unpacked Android apps — a source no web archive covers — and requires an API key (`--bevigil-api-key` or `URX_BEVIGIL_API_KEY`).

Run `urx --list-providers` to print the full catalog (id, API-key requirement, and a one-line summary) directly from the binary.

### Custom CDX Endpoints

Any archive built on pywb, OutbackCDX, or the Internet Archive's CDX server can
be queried without a dedicated provider:

```bash
urx example.is --cdx-endpoint https://vefsafn.is/cdx
urx example.is --cdx-endpoint https://vefsafn.is/cdx --providers cdx:vefsafn.is --rate-limit-by cdx:vefsafn.is=1
```

Each endpoint becomes a provider with id `cdx:<host>` (`cdx:<host>:<port>` when
the URL names a port), enabled by being named.
It honours `--subs`, `--from`/`--to`, the `--archive-*` filters, pagination and
rate limiting exactly like `wayback`/`cc`/`arquivo`, and reports capture
metadata. `--cdx-dialect classic|pywb` fixes the server's dialect; unset, urx
probes once and falls back to `pywb`.

The only public endpoint verified to work is `https://vefsafn.is/cdx`
(Iceland; pywb). It ignores pagination parameters and returns the whole result
set, and may answer with an Anubis-style "Session Verification" page after a
few requests — urx reports that as an error naming the endpoint, never as an
empty result. The UK Web Archive, Library of Congress, Bibliotheca Alexandrina
and the National Library of Australia CDX APIs are blocked by bot protection or
redirects and do not work from urx.

### Provider Notes

| Provider | What it queries, and its limits |
|----------|---------------------------------|
| `otx` | `/api/v1/indicators/{domain\|hostname}/…/url_list`, 200 URLs per page, at most 1,000 pages. The `hostname` endpoint is used for a target with three or more labels (`sub.example.com`, but also `example.co.uk`) without `--subs`; it excludes `www.` and other subdomains |
| `vt` | v3 `domains/{domain}/urls`, 40 per page with cursor pagination. A 404 means no data; a 429 waits for the server's `Retry-After` (at most 60 s) before retrying |
| `urlscan` | `search/?q=domain:<domain>` with 100 results per page and `search_after` paging. Works without a key |
| `github` | Code Search for `"<domain>"`; URLs come from the matched text fragments, not whole files. GitHub caps code search at 1,000 results (10 pages × 100). Any personal access token works |
| `zoomeye` | `api.zoomeye.ai/v2/search` with the dork `site:<domain>` (`site:*.<domain>` under `--subs`), 100 per page |
| `bevigil` | One request per domain to `osint.bevigil.com/api/<domain>/urls/`, no pagination |

`--subs` changes the query itself only for the CDX providers, ZoomEye
(`site:*.<domain>`), and OTX when the target has three or more labels (it then
switches from the `hostname` to the `domain` endpoint). GitHub sends the same
search either way but keeps subdomain URLs from the matched fragments only under
`--subs`; without it GitHub keeps URLs on exactly the target host, not even
`www.`. VirusTotal, urlscan, BeVigil and the `robots` / `sitemap` fetchers send
the same query either way, and host validation then keeps or drops the
subdomains they return.

Naming a keyed provider without a key prints `Error: The … provider (…) requires
an API key…` (not under `--silent` or `--all-providers`) and skips that
provider. The run carries on with exit code 0 as long as another provider
remains, including the default `robots` / `sitemap` probes. If the keyed
provider was the only one, the run fails with `No valid providers specified`
and exits 1.

## Reading URLs from Files

`--files` reads URLs from local files instead of querying providers. Positional
domains and `--domain-list` are ignored, no provider is queried, and no host
validation applies, since there is no target to validate against; use
`--scope-file` or `--match-regex` to restrict hosts. Every URL is attributed to
`file` in `--show-sources` and `--stats`. The filters, testers and output
options all work as usual; `--stream` is rejected, and the cache and
`--incremental` are not used. `--files` takes several paths, so a positional
domain placed after it is read as another file.

**How the format is chosen**, by file name:

- `.warc` → WARC.
- `.gz` or `.bz2` → WARC if the name contains `warc`, otherwise URLTeam.
- `.txt` or `.list` → text.
- Otherwise, a name containing `warc` → WARC, and `urlteam` / `url_team` →
  URLTeam. Everything else is read as text.

Files read as WARC or URLTeam are checked for gzip by their magic bytes, and
multi-member streams are read in full. A file read as text (`.txt`, `.list`, or
any other name) is never decompressed: a gzip or bzip2 file with such a name
yields no URLs and no error, so give it a `.gz` extension or decompress it
first. bzip2 is not supported in the WARC or URLTeam readers: the run stops with
`bzip2 input is not supported. Decompress it first` (`bzip2 WARC input…` for a
WARC) and exits 1.

**What is extracted.** Text files: each non-blank, non-`#` line that starts with
`http://` or `https://`. URLTeam: the first `http(s)://` token on each line.
WARC: the `WARC-Target-URI` headers and bare URL lines in the payloads.

**Limits.** Each file is capped at 1,000,000 URLs and 1 GiB of decompressed
input, and lines over 1 MiB are skipped; hitting a cap prints a warning.

```bash
urx --files urls.txt --check-status --include-status 200
urx --files crawl.warc.gz --scope-file scope.txt -f jsonl
```

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
| `no-resources` | Exclude resource files (JavaScript and its relatives such as `.json` / `.map`, stylesheets, images, fonts, documents, videos, audio) |
| `no-images` | Exclude image files |
| `no-fonts` | Exclude font files |
| `no-documents` | Exclude document files |
| `no-videos` | Exclude video files |
| `no-audio` | Exclude audio files |
| `only-js` | Only JavaScript and related sources (`js`, `mjs`, `cjs`, `jsx`, `ts`, `tsx`, `vue`, `svelte`, `json`, `map`, …) |
| `only-style` | Only stylesheet files |
| `only-fonts` | Only font files |
| `only-documents` | Only document files |
| `only-videos` | Only video files |
| `only-audio` | Only audio files |
| `only-images` | Only image files |

### Security Presets

These four go beyond file extensions: a URL qualifies when it carries a listed
extension **or** when it has a listed shape (shape rules that look for a
substring are checked against the whole lower-cased URL, query included). That is what lets
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

## Scoping a Run to a Path

A target may name a path, and it means what it says: `urx example.com/shop`
collects the part of the site under `/shop`.

```bash
urx example.com/shop
urx https://example.com/api/v2      # a pasted URL works too
```

This is not a filter applied after the fact. A CDX index answers prefix queries
natively, so urx sends `url=example.com/shop*` and the archive never ships the
rest of the site across the network — on a large target that is the difference
between a few hundred rows and a few hundred thousand.

Details worth knowing:

- Providers whose query cannot express a path (OTX, VirusTotal, urlscan,
  GitHub, BeVigil, ZoomEye, and the `robots`/`sitemap` fetchers, which read the
  site root whatever the scope is) are asked about the host, and their results
  are narrowed afterwards by host validation.
- `--subs` cannot push the scope into the query either: a leading `*.` selects
  `matchType=domain`, which no CDX server combines with a path. The prefix is
  applied to the results instead.
- Scope means *at or under* the path: `/shop` and `/shop/cart` are in,
  `/shopping` is not.
- Case is ignored. A CDX server lower-cases the whole URL when it builds its
  index key, so `example.com/Shop*` and `example.com/shop*` return the same
  rows — all spelled in lower case. Matching case-sensitively here would
  discard every one of them.
- A query string or fragment in the target is dropped. Those narrow a request,
  not a scope.
- `--no-strict` waives the *host* check, not the path scope: it was asked for
  explicitly, and it is part of what the target is.

> Note: urx used to discard the path from a target, so
> `urx https://example.com/shop` scanned the whole of `example.com`. It now
> scans `/shop`. Pass just the host for the old behaviour; a run whose target
> carries a path says so on stderr.

## Host Validation

With `--strict` (the default) a URL is kept only when its host is one of the
targets. `www.<target>` counts as the target itself, so a site served entirely
on `www.` is not lost; with `--subs`, any subdomain of a target is kept too.
`--no-strict` waives the host check but keeps a target's path scope. `--strict`
takes no value: `--no-strict` is the way to turn it off (`--strict false` would
read `false` as a domain).

Host validation needs the targets on the command line: domains piped through
stdin are currently not validated at all. Every host a provider returns is kept,
and a path in a stdin target narrows only the CDX providers' own queries —
nothing filters the other providers' results to it. Pass targets positionally or
with `--domain-list` when you want `--strict` and the path scope to apply.

When validation removes more than half of the URLs that survived the other
filters and `--subs` is off, urx prints a one-line hint on stderr, even without
`-v`:

```
[urx] strict host validation removed 812/1400 URLs; pass --subs to keep subdomains or --no-strict to keep all hosts
```

When a target carries a path, the advice is to drop the path instead of
`--no-strict`, and under `--no-strict` the line reads `[urx] the target's path
scope removed …`. The hint is printed for batch output only (not under
`--stream`), and counts the URLs that survived the other filters. `--silent`
hides it.

## Regular-expression Filtering

`--patterns` and `--exclude-patterns` are substring tests. `--match-regex` and
`--filter-regex` are the [regex](https://docs.rs/regex/latest/regex/#syntax)
equivalents and behave differently in three ways:

| | `--patterns` | `--match-regex` |
|---|---|---|
| Matching | substring | full regex syntax |
| Case | insensitive (both sides lower-cased) | **sensitive** — prefix `(?i)` to opt out |
| Multiple values | comma-separated (the flag may also be repeated) | repeat the flag; commas are never split |

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

## Scope Files

A bug bounty program's scope is a list of hosts, and every platform writes it
the same way: `*.example.com` for a wildcard, a bare host for a single target,
and a handful of subdomains that are explicitly out of scope. `--scope-file`
takes that list as-is, so it never has to be hand-translated into anchored
regex alternations — where getting the anchoring wrong silently *widens* the
scope instead of failing.

```text
# scope.txt — in scope
*.example.com
api.example.org

# out of scope, even though the wildcard above covers them
!admin.example.com
!*.internal.example.com
```

```bash
urx example.com --subs --scope-file scope.txt

# Several programs at once; the files are unioned
urx --domain-list targets.txt --subs --scope-file scope-a.txt --scope-file scope-b.txt
```

The rules:

* A line is a host pattern, optionally prefixed with `!` to exclude it.
* `*.example.com` matches `example.com` **and** every host under it. That is
  the bug-bounty reading rather than the DNS one, and it is what every
  platform's scope table means; a program that really excludes its apex says so
  with a `!example.com` line, which wins.
* A bare `example.com` matches that host and nothing else — not `www.`, not any
  subdomain. A scope file is an explicit list, so no leniency is applied.
* A lone `*` matches every host, for a file that is purely a deny-list.
* Everything from a `#` to the end of the line is a comment, so an entry can be
  annotated in place. Blank lines are skipped.
* **Exclusion always wins**, mirroring `--filter-regex` beating `--match-regex`.
* A file with no include lines at all is a pure deny-list: everything is in
  scope except what it excludes.

IPv6 literals must be bracketed, for example `[2001:db8::1]`; they are exact
hosts and cannot be wildcarded. Anything else — a port, a path, a wildcard in
the middle — is a startup error naming the file and the line. This filter
decides which hosts you are willing to touch, so a line urx cannot honour has
to stop the run rather than quietly leave the scope wider than the file
describes.

`--scope-file` and `--strict` are separate gates and a URL must pass both. Host
validation answers "does this URL belong to a domain I queried?"; a scope file
answers "is this host one I am allowed to touch?". They usually agree, but not
always: a `*.example.com` scope line while querying the bare apex still needs
`--subs`, because strict mode drops the subdomains before the scope file ever
sees them. The filter lives inside urx's URL filter, so the batch result, the
`--stream` sink and the links `--extract-links` discovers are all held to it.

## Archive Metadata Filters

`--from`/`--to` and the `--archive-*` predicates are pushed down into the
archive's own query, which makes them free — and also limits them: they reach
CDX-backed providers only, and the two CDX dialects disagree badly enough that a
positive multi-value list (`--archive-status 200,301`) is unsatisfiable on pywb
servers and gets dropped with a warning.

The eight `--meta-*` filters run **after** collection instead. They see one
merged set of [capture metadata](#archive-capture-metadata) per URL regardless of
which provider produced it, so "any of these" always works:

| Flag | Keeps |
|------|-------|
| `--meta-first-seen-after <DATE>` | URLs whose oldest capture is on or after `DATE` |
| `--meta-first-seen-before <DATE>` | URLs whose oldest capture is on or before `DATE` |
| `--meta-last-seen-after <DATE>` | URLs whose newest capture is on or after `DATE` — "still alive as of" |
| `--meta-last-seen-before <DATE>` | URLs whose newest capture is on or before `DATE` — "dead since" |
| `--meta-mime <TYPE>` | URLs with one of these archived MIME types (`image/*` matches any subtype) |
| `--meta-exclude-mime <TYPE>` | everything except those |
| `--meta-status <CODE>` | URLs with one of these archived status codes (`20x` / `5xx` patterns, as in `--include-status`) |
| `--meta-exclude-status <CODE>` | everything except those |

Dates accept `YYYY`, `YYYYMM`, `YYYYMMDD` or `YYYYMMDDhhmmss` and are padded the
way `--from`/`--to` are: an `after` bound pads to the start of the period, a
`before` bound to the end. So `--meta-first-seen-after 2020
--meta-first-seen-before 2020` means "first archived during 2020".

```bash
# Endpoints that were alive recently, HTML and images out of the way
urx example.com --providers wayback --meta-last-seen-after 2024 --meta-exclude-mime 'text/html,image/*'

# Pages that died: last captured before 2019, and nothing since
urx example.com --providers wayback --meta-last-seen-before 2019

# JSON the archive served successfully
urx example.com --providers wayback --meta-mime application/json --meta-status 200
```

The two kinds of filter are complementary, not alternatives. Archive-side
predicates reduce what comes over the wire; `--meta-*` predicates apply
uniformly to the merged result set. Using both is normal.

**URLs with no metadata.** Most URLs in a mixed run carry none: the non-CDX
providers (`otx`, `vt`, `urlscan`, `zoomeye`, `github`, `bevigil`, `robots`,
`sitemap`) have no capture index, `--files` input is a list of strings, and a
cache hit stores URLs only. The direction of the predicate decides what happens
to them:

* A **positive** predicate (`--meta-mime`, `--meta-status`, any date bound) asks
  "is this value one of these?", which an absent value cannot answer — the URL
  is dropped.
* An **exclusion** (`--meta-exclude-mime`, `--meta-exclude-status`) drops only
  what positively matches, so a URL with no metadata survives. This is the rule
  `--filter-regex` and `--exclude-status` already follow.

`--verbose` reports the split ("… failed a predicate, … carried no archive
metadata to test"). When missing metadata accounts for the *whole* result set,
urx says so even without `-v`, because a cache hit otherwise makes an empty run
look like a target with nothing to find. Pass `--no-cache` and a CDX provider to
get metadata back.

The `--meta-*` flags are rejected under `--stream` for the same reason
`--show-meta` is: the sink emits a URL on first sighting, before the captures
that complete its metadata have arrived.

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
can be combined; they run in the order `--normalize-url` → `--merge-endpoint` →
`--dedup-similar`. `--merge-endpoint` and `--dedup-similar` need the complete
result set and cannot be used with `--stream`; `--normalize-url` works on one URL
at a time and can.

```bash
urx example.com --dedup-similar --verbose
urx --files urls.txt --normalize-url --merge-endpoint --dedup-similar
```

## Parameter and Fuzz Views

`--show-only-param` only ever cuts the query string off each URL, which cannot
answer the first question a tester asks: what parameters does this target take?
Three views replace the URL list with an answer instead, built on the same
grouping `--dedup-similar` uses.

**`--params`** — every query parameter name in the result set, once each,
sorted. The parameter inventory of the whole target rather than of one URL at a
time.

```console
$ urx example.com --params
page
q
ref
sort
utm_source
```

**`--params-by-endpoint`** — one line per endpoint: the endpoint, a space, and
the comma-separated union of the parameter names seen on it. Identifier-looking
path segments collapse to `{id}` exactly as under `--dedup-similar`, so
`/post/1?a=1` and `/post/2?b=2` report as one endpoint taking `a,b`. The
endpoint is spelled out in full rather than as a bare path, because urx
routinely scans several hosts in one run and a bare path would merge
`a.example.com/search` with `b.example.com/search` into a line true of neither.

```console
$ urx example.com --params-by-endpoint
https://example.com/post/{id} ref,utm_source
https://example.com/search page,q,sort
```

**`--fuzz-placeholder VALUE`** — every query parameter *value* rewritten to
`VALUE`, keeping one URL per parameter signature. URLs without parameters drop
out. The representative keeps its real path — a `{id}` would not route — and is
the lexicographically smallest URL of its group, so runs are reproducible.

```console
$ urx example.com --fuzz-placeholder FUZZ
https://example.com/post/1?ref=FUZZ
https://example.com/post/2?utm_source=FUZZ
https://example.com/search?q=FUZZ&page=FUZZ
https://example.com/search?q=FUZZ&sort=FUZZ
```

```bash
# Straight into ffuf
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ

# ...or dalfox
urx example.com --fuzz-placeholder FUZZ | dalfox pipe
```

Parameter names are split out of the raw query rather than decoded first: the
name is precisely what has to survive verbatim to be worth fuzzing.

All three views need the complete result set, so they cannot be combined with
`--stream`. They are mutually exclusive with each other and with the
`--show-only-*` views. The three `--show-only-*` views are also mutually
exclusive with each other. A command-line view replaces any configured
`show_only_*` view; a config file that sets more than one of those keys to
`true` is rejected.

## Wordlist Output

`-f wordlist` turns a collected result set into a wordlist: every path segment
and query parameter name the run saw, deduplicated across the whole run and
sorted, one term per line.

```console
$ urx example.com -f wordlist
admin
api
page
post
q
ref
search
sort
users
users.json
utm_source
v1
```

Segments that look like data rather than route names are left out, reusing the
same test `--dedup-similar` groups on — a wordlist full of `4711`, UUIDs, dates
and session tokens is worse than no wordlist, since every one of those words
exists on exactly one target. A segment whose *stem* is an identifier goes too:
`1234.html` is not a word either. A name with a number attached, such as
`article-1234.html`, is kept.

Case is preserved rather than normalised. Path segments are case-sensitive on
most origins, so lower-casing `WebResource.axd` would produce a word that 404s
everywhere it is tried, and a target that really serves both `/Admin` and
`/admin` is telling you something worth keeping.

The union has to be taken over the full set, so the format is batch-only and
`--stream` rejects it. Per-URL fields — a status code, `--show-sources`
attribution, capture metadata — have nowhere to go in a wordlist and are simply
not emitted. `--output-dir` writes wordlists as `.txt`, and `[output].format`
in the config file accepts `wordlist` alongside the rest.

```bash
# Build a target-specific wordlist and fuzz with it
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ
```

## Authenticated and Custom Requests

`--check-status`, `--extract-links`, `--extract-js-endpoints` and
`--expand-specs` all re-request collected URLs from the target itself. `-H`
gives those requests whatever headers they need:

```bash
urx example.com --check-status -H "Authorization: Bearer $TOKEN"
urx example.com --extract-links --cookie "session=abc; role=admin"
urx example.com --check-status --user-agent "acme-security-scan/1.0"
```

`-H` is repeatable and takes `Name: value`. A malformed one stops the run
rather than going out unnoticed: an argument that is silently dropped leaves an
anonymous scan reading as an authenticated one. `--cookie` and `--user-agent`
are shorthands for the corresponding headers, and a later value for a name
replaces an earlier one.

**These headers never reach an archive.** They are sent only by the components
that talk to the target: the four testers above, plus the `robots` and
`sitemap` providers, which fetch from the target too. Every other provider
queries web.archive.org, index.commoncrawl.org or a third-party API, and so
does `--archive-body` when it replays a capture; handing them the target's
session cookie would mail a credential to a service that keeps what it
receives, for no gain. Archive queries keep urx's own User-Agent, which
`--random-agent` still rotates.

The headers also follow `--network-scope`: under `--network-scope providers`
the testers get none (an authenticated `--check-status` would then go out
anonymous), and under `--network-scope testers` the `robots` and `sitemap`
providers get none.

They can also be set in the config file:

```toml
[network]
header = ["X-Env: staging", "X-Team: appsec"]
cookie = "session=abc"
user_agent = "acme-security-scan/1.0"
```

Any `-H` on the command line replaces the configured set wholesale, so a run
can always be made anonymous again without editing the file.

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
  transforms as URLs that came from a provider. Those filters also run on the
  collected URLs *before* any page is fetched, so `--extract-links -e js` drops
  the HTML pages first and finds nothing; filter the output instead.
- Discovered links are not status-checked: `--check-status` covers the
  collected URLs only.
- Only responses that succeeded and look like markup are parsed, and each body
  is capped at 10 MiB.

```bash
# Crawl one hop deeper and keep only JavaScript
urx example.com --extract-links | grep -E '\.js(\?|$)'

# Extraction obeys the network settings too
urx example.com --extract-links --proxy http://localhost:8080 --timeout 20
```

## JavaScript Endpoint Extraction

`--extract-js-endpoints` is the companion to `--extract-links` for the URLs
that never appear in HTML. A modern web app's API surface lives inside its
JavaScript bundles as string literals — `fetch("/api/v2/users")`,
`axios.post("/graphql")`, `` `/api/orders/${id}` `` — and `-e js` collects
those bundles without ever reading them. This option re-fetches every
collected URL that looks like JavaScript and mines the body for those
literals.

**What is fetched.** URLs are skipped up front when their extension is
certainly not script (images, fonts, CSS, archives, `.json`, `.map`, ...).
Everything else is requested and classified by `Content-Type`: JavaScript
types are scanned whole; HTML is scanned for its inline `<script>` blocks
only; a `.js`/`.mjs`/`.cjs`/`.jsx`/`.ts`/`.tsx` URL served as `text/plain` or
`application/octet-stream`, or with no `Content-Type` at all is still treated as
script. Any other response with no `Content-Type` is scanned for inline
`<script>` blocks like HTML. Any other type is discarded unread.

**What is extracted.**

- Quoted absolute and origin-relative paths: `"/api/v2/users"`, `'/graphql'`.
- Full and protocol-relative URLs: `"https://api.example.com/v1"`, `"//cdn.example.com/x"`.
- The URL argument of `fetch(...)`, `axios.get/post/...(...)`, `axios({url: ...})`,
  `$.ajax(...)`, and `XMLHttpRequest.open(method, ...)`, including the
  `base + "/path"` spelling.
- The static prefix of a template literal: `` `/api/users/${id}` `` → `/api/users/`.
- ES-module chunk imports and asset references: `import("./chunk-ab12.js")`.

**How relative paths resolve.** `/api/x` resolves against the script's
origin, which is what the browser does at runtime. `./chunk.js` resolves
against the script's own URL, as an ES-module import does. Bare relative
paths (`api/v1/x`) and `./x` request arguments resolve at runtime against the
page that loaded the bundle, which urx does not know; they are resolved
against the origin root, since resolving them under the bundle's asset
directory would certainly be wrong.

**Noise suppression.** Regex-mining a minified bundle produces mostly
garbage, so the policy errs towards dropping. The following are discarded,
and the code comments in `src/testers/js_endpoint_extractor.rs` name the
real-bundle shape behind each rule:

| Dropped | Why |
|---------|-----|
| MIME types (`image/png`, `application/json`, `*/*`) | Header values with the shape of a two-segment path — the single most common false positive |
| `"/"`, `"/x"`, `"//"`, `"./"`, `"../"` | Path-join fragments and comment delimiters (`fetch("/")` is still kept) |
| Sourcemap directives, `//#`, `//@` | Not URLs |
| Base64 and `data:` payloads | Inline images and fonts; hex content hashes and hashed filenames survive |
| CSS shorthand and ratios (`12px/1.5`, `16/9`) | Style values |
| Date formats (`MM/DD/YYYY`, `HH/mm`) | Formatting tokens |
| Bare extensions (`.js`, `/.png`) | File-type checks |
| Regex sources and tag fragments (`/^\/api/(\d+)`, `</div>`) | Excluded by the character class — they never match at all |
| Strings closed by a different quote (`'/g,"`), trailing `,` `;` `:` | Artifacts of regex literals next to strings |
| Bare relative paths with no leading `/` or `.` (`react/jsx-runtime`, `en/US`) | Package paths and locale tags; kept only when a segment has three or more characters and there are three or more segments, a query string, or a dot in the last segment (`app/main.js`). `fetch` / `axios` / XHR arguments are exempt |
| Package and source-tree paths (`@scope/pkg/...`, `node_modules/...`, `./src/...`, `lib/esm/...`) | Import specifiers and webpack module keys |
| `./x` without a fetchable extension (`require("./utils")`, `{"./zlib/deflate":46}`) | CommonJS module specifiers — a real bundle contributed ~50 of them from jszip and pako alone |
| Path suffixes after `+` (`"/users/" + id + "/avatar"` → `/avatar`) | Only the prefix is a route; `base + "/api/x"` is still kept |
| XML namespaces and schema hosts (`www.w3.org`, `schema.org`) | Boilerplate in every SVG-bearing bundle |

A string that is the argument of a request call is known to be a URL and
bypasses the length and bare-relative rules.

**Safety.** Each body is capped at 10 MiB, fetches respect `--timeout`,
`--retries`, `--proxy`, `--insecure`, `--random-agent`, and `--rate-limit`,
and the number of files fetched per run is capped by `--max-js-files`
(default 500, `0` for unlimited). Discovered endpoints go through the same
filters and host validation as everything else, so strict mode (the default)
drops off-site URLs, and `--no-strict` keeps them. Note that because
discovered endpoints pass through your filters, combining this option with
`-e js` keeps only the `.js` endpoints it found — the extractor already
selects JavaScript by itself, so leave `-e js` off when you want the API paths.
`--check-status` checks the collected URLs only; discovered endpoints are
emitted without a status. `--extract-js-endpoints` needs the complete result set
and cannot be combined with `--stream`.

```bash
# Mine every collected script for API paths
urx example.com --extract-js-endpoints

# Keep the discovered paths that look like an API (filter the output:
# --patterns would drop the bundles before they are read)
urx example.com --extract-js-endpoints | grep -E 'api|graphql'

# Bound the run: at most 50 bundles, one request per second
urx example.com --extract-js-endpoints --max-js-files 50 --rate-limit 1
```


## API Specification Expansion

A `--preset only-api` sweep finds `/swagger.json`, `/openapi.yaml` and
`/v3/api-docs` and then never opens them: `--extract-links` parses HTML,
`--extract-js-endpoints` deliberately drops `application/json` bodies, and
`--archive-body` runs the HTML parser over whatever the archive returns. So the
single most information-dense file on the target is collected as one URL and
left unread.

`--expand-specs` fetches those documents and expands every route they describe
into the result set. One request buys the whole documented surface — exact and
already parameterised — which is a far better exchange rate than mining
minified bundles.

```bash
# Find the specs and expand them in the same run (only-api also filters the
# expanded routes; drop it to keep every documented path)
urx example.com --preset only-api --expand-specs

# Bound and pace it
urx example.com --expand-specs --max-spec-files 10 --rate-limit 2

# Recover an API that no longer exists: read the archived copy of the document
urx example.com --archive-body --expand-specs
```

**What is expanded.**

- **OpenAPI 3.x** — `servers[].url` (absolute, relative to the document, and
  templated: `{var}` resolves from `variables[var].default`, else the first
  `enum` value) crossed with every `paths` key. A path item's own `servers`
  override the document's, which is what gateways fronting several backends
  use.
- **Swagger 2.0** — `schemes` × `host` + `basePath`, each part falling back to
  the corresponding part of the document's own URL when omitted, as the
  specification says. `ws`/`wss` schemes are dropped; they are not URLs a
  scanner can request.
- **GraphQL introspection** (`{"data":{"__schema":…}}` or the unwrapped form) —
  one URL per query, mutation and subscription field, written as the endpoint
  plus `?query=…`. That is both a request a server may genuinely answer and a
  legible name for the operation in a result list. A schema saved as a file
  resolves to its endpoint (`/graphql/schema.json` → `/graphql`).

JSON and YAML are both read; a YAML document is converted to the same shape as
a JSON one before expansion, so one reader covers both.

**Which URLs are requested.** The name is checked first and for free: the path
must contain a specification marker (`swagger`, `openapi`, `api-docs`,
`apidocs`, `api_docs`, `graphql`, `introspection`) and, when the URL has an
extension at all, it must be `json`, `yaml` or `yml`. So `swagger-ui.html` and
`swagger-ui-bundle.js` cost no request. The response's `Content-Type` then
decides: a definite unrelated type (HTML, image, JavaScript) skips the body even
under a specification name, while the vague types static hosts hand out
(`text/plain`, `application/octet-stream`) yield to the extension and then to
the first byte of the body, so an untyped `/v3/api-docs` still parses.

### Details

- Path templates are emitted as the document writes them (`/users/{id}`, not
  `/users/%7Bid%7D`) — reading the route is the point. `--normalize-url`, if
  you ask for it, re-parses and encodes them downstream.
- `--max-spec-files` (default 50) caps the documents fetched per run; `0` means
  unlimited.
- Each body is capped at 10 MiB, the same guard the other body-reading testers
  use. A YAML document with more than 32 alias references is refused before
  parsing starts: YAML aliases expand by copying, so a few hundred bytes can
  expand to gigabytes of nodes ("billion laughs"), which a byte cap cannot
  catch. Published specifications use `$ref`, a plain string, and the rare
  document that uses YAML anchors uses a handful.
- Only the first document of a multi-document YAML stream is read.
- Discovered URLs go through the same filters, host validation, and output
  transforms as URLs that came from a provider.
- Incompatible with `--stream`, like every option that runs after collection.
- With `--archive-body` also on, an *archived* specification is read as one
  rather than being handed to the HTML link extractor and discarded. This costs
  no extra requests — the body was already being fetched and already counted
  against `--archive-body-limit` — and it is where the feature earns the most:
  the live host may have retired the API, moved it behind auth, or removed the
  document, while the archive still holds the file that described every route
  it had.

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
urx example.com --archive-body | grep -E '\.js(\?|$)'
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

### Mining archived JavaScript

A modern app's API surface lives in its bundles as string literals, and
`--extract-js-endpoints` fetches those from the live site — where they are
frequently gone. Bundles are named by build hash, so `app.a3f9c2.js` 404s the
moment the site redeploys, and the endpoints it named go with it. Run the two
flags together and urx mines the *archived* copy instead, and an archived
page's inline `<script>` blocks alongside its links:

```bash
urx example.com --archive-body --extract-js-endpoints
```

An archived body is classified as script before it is classified as markup: a
capture the archive replayed without a `Content-Type` satisfies the "might be
markup" test, so asking that question first would hand every typeless bundle to
the HTML parser and mine nothing.

### Keeping the bodies

The requests are already being made, so writing the bodies to disk costs
nothing extra and answers the questions no link extractor asks: the
`<!-- staging.internal -->` comment, the token a 2019 build inlined, the stack
trace naming a framework version.

```bash
urx example.com --archive-body --archive-body-dir ./corpus
grep -ri "api[_-]key" ./corpus
```

Each file is named after its URL plus a hash of it — the slug is lossy, so the
hash is what keeps two URLs apart — and `corpus/index.jsonl` maps every file
back to its URL, capture timestamp, digest and content type. Only text-like
bodies are stored (HTML, script, JSON, XML, CSS, plain text), so the directory
does not fill up with the site's images and fonts. Because the fetch is
deduplicated by digest, the corpus covers far more of the target per request
than one response per URL would.

An unwritable directory stops the run at start-up rather than after an hour of
replaying, and a failure on an individual file is reported without discarding
the links the run is collecting. `--archive-body-dir` without `--archive-body`
(including an `archive_body_dir` set only in the config file) is rejected at
start-up too.

### Details

- Only URLs with a capture timestamp qualify. The CDX providers (`wayback`,
  `cc`, `arquivo`, and any `--cdx-endpoint`) supply one; `--files` input, non-CDX providers, and cached
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
