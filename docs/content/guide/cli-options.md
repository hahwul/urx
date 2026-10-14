+++
title = "CLI Options"
description = "Every command-line flag urx accepts, plus the webhook notification payloads and their failure handling."
toc = true
weight = 1
+++

## Command Line Options

Output of `urx --help`:

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
  -f, --format <FORMAT>          Output format: "plain", "json", "jsonl", "csv", "wordlist" (case-insensitive; anything else is a usage error) [default: plain]
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

`urx cache` is the only subcommand. See
[Inspecting and Maintaining the Cache](/guide/caching/#inspecting-and-maintaining-the-cache).
Everything else on this page applies to a scan.

## Webhook Notifications

`--notify <URL>` POSTs a run summary to a webhook when the run ends. Run it
from cron with `--incremental` and it fires only when the archives have
something new.

```bash
# slack, only when new URLs turned up (default --notify-on new)
urx target.com --incremental --silent \
  --notify https://hooks.slack.com/services/T000/B000/XXXX --notify-format slack

# discord, every run
urx target.com --incremental --notify "$DISCORD_HOOK" --notify-format discord --notify-on always

# several receivers, urx JSON schema
urx target.com --incremental --notify https://n8n.example/hook --notify https://ntfy.example/urx

# keep the webhook out of shell history
export URX_NOTIFY_URL=https://hooks.slack.com/services/...
urx target.com --incremental --notify-format slack
```

`--silent` still sends, but it also hides the URL list on stdout. Add `-o` to
keep the results.

### When it sends

| `--notify-on` | Sends |
|---------------|-------|
| `new` (default) | Only when the run emitted at least one URL. Under `--incremental`, at least one URL the previous run had not seen. |
| `always` | After every run, including one with zero URLs. |
| `never` | Nothing. The configuration stays in place. |

### Payload formats

`json` (default) uses urx's own schema:

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

`incremental` is `true` only when the run diffed against the cache, so it is
`false` under `--no-cache` or `--files`. `providers` has the same numbers
`--stats` prints. `sample` holds up to 20
emitted URLs in output order, and `sample_truncated` is `true` when there were
more. Under `--stream` the payload has the count and an empty sample
(`sample_truncated` is `true` if any URL was emitted).

`slack` sends `{"text": "..."}` and `discord` sends `{"content": "..."}`. The
message is a header line (count, targets, elapsed time), one line of provider
totals, the URL sample, and `… N more not shown` when the sample is short of
the total. The text is cut on a line boundary at 2000 characters for Discord
and 4000 for Slack, and ends with
`[truncated: N lines cut to fit the message limit]`. A URL is never split.
Archived URLs cannot ping anyone: Slack text escapes `&`, `<` and `>`, and the
Discord payload sets `"allowed_mentions": {"parse": []}`.

### Failure handling

Delivery never changes the exit code. The URLs are already on stdout or in
`--output`, so a webhook that is down, slow or returns 4xx/5xx prints a warning
on stderr and the run exits 0. `--verbose` prints the HTTP status of each
delivery; `--silent` hides those lines but still sends. Each URL is tried once
with no retry: chat webhooks are not idempotent, and a retry after a slow but
delivered request would post twice.

### The URL is a secret

A Slack or Discord webhook URL is the credential. urx prints only its scheme,
host and non-default port (`https://hooks.slack.com`) in verbose output,
warnings, `--stats` and error text, and the payload never contains it.

To keep it out of a checked-in config, use the `URX_NOTIFY_URL` environment
variable or `notify_url` in the provider-config file. `[notify].url` in the main
config also works. Precedence is CLI/env > provider-config > main config, the
same as API keys. The `[notify]` section also takes `on` and `format` (the
config equivalents of `--notify-on` and `--notify-format`).

### Network settings

The request uses `--proxy`, `--proxy-auth`, `--timeout` and `--insecure`.
`--network-scope` does not apply: it splits traffic between the archives
(providers) and the target (testers), and the webhook is your own endpoint.

## Archive Capture Metadata

The CDX-backed providers (`wayback`, `cc`, `arquivo`, and any `--cdx-endpoint`)
return captures, so each URL comes with these fields:

| Field | Meaning |
|-------|---------|
| `first_seen` | Oldest capture timestamp, 14-digit CDX form (`YYYYMMDDhhmmss`) |
| `last_seen` | Newest capture timestamp |
| `mime` | MIME type of the most recent capture that recorded one |
| `archive_status` | HTTP status the *archive* recorded at capture time |
| `digest` | A representative content digest across the captures |

`archive_status` is what the crawler got at capture time. `status` (only under
`--check-status`) comes from requesting the URL live now. A URL can have
`archive_status` `200` and be dead today.

When the same URL comes from several captures or archives, the fields merge:
`first_seen` is the oldest timestamp, `last_seen` the newest, and
`mime`/`archive_status` come from the most recent capture that had them.
Providers with no capture index (`otx`, `vt`, `urlscan`, `zoomeye`, `github`,
`bevigil`, `robots`, `sitemap`) and `--files` input report the URL only. A
domain served from cache has no metadata either (the cache stores URLs only);
use `--no-cache` to get it back.

| Format | Output |
|--------|--------|
| `json` / `jsonl` | A key per field, only when it has a value |
| `csv` | A column per field, only when at least one row has a value |
| `plain` | Bare URL by default. `--show-meta` appends ` [first_seen=… last_seen=… mime=…]` |

A URL rewritten by `--normalize-url` or `--merge-endpoint` no longer matches
what the providers reported, so it has no `sources` or capture metadata.

### Source Attribution (`--show-sources`)

`--show-sources` names each source by its run label (the name `--stats`
prints) rather than its `--providers` id: `Wayback Machine`, `CC (latest)` or
the pinned index name (e.g. `CC-MAIN-2026-17`), `OTX`, `Arquivo.pt`,
`VirusTotal`, `Urlscan`, `ZoomEye`, `GitHub`, `BeVigil`, `Robots.txt`,
`Sitemap`, `Robots.txt (archived)`, `Sitemap (archived)`, `cdx:<host>`, and
`file` for `--files` input.

JSON gives a `sources` array, plain text a ` [a,b]` suffix, and CSV one
`sources` cell joined with `|` (e.g. `CC (latest)|Wayback Machine`). URLs
served from cache, or found by `--extract-links`, `--extract-js-endpoints`,
`--archive-body` or `--expand-specs`, have no sources.

`--show-sources` and `--show-meta` are rejected under `--stream`. A streamed URL
is printed on first sighting, before other providers or later captures could
add to it.

```bash
urx example.com --providers wayback -f jsonl
# {"url":"https://example.com/old.php","first_seen":"20040112093000",
#  "last_seen":"20180722140311","mime":"text/html","archive_status":"200",
#  "digest":"HT2DYGA5UKZCPBSFVCV3JOBXGW2G5UUA"}

# everything last captured before 2010
urx example.com -f jsonl | jq -r 'select(.last_seen and .last_seen < "20100101000000") | .url'

# metadata in plain output
urx example.com --providers wayback --show-meta
```

## Live Response Metadata

`--check-status` records these fields from the response head along with the
status:

| Field | Meaning |
|-------|---------|
| `status` | Code and reason (`200 OK`), or `Status check failed` when the request failed (connection refused, timeout, TLS). Failed URLs are kept unless `--include-status` is set |
| `location` | The `Location` header of a 3xx, recorded but never followed |
| `content_length` | The `Content-Length` header, verbatim |
| `content_type` | The `Content-Type` header, verbatim |
| `title` | The HTML `<title>`, only under `--check-title` |

- Redirects are never followed, so `status` always belongs to the requested URL
  and `location` is where the 3xx pointed.
- With both `--include-status` and `--exclude-status`, only `--include-status`
  applies. Use one or the other.
- `--check-title` implies `--check-status` and has to read the body. It reads at
  most 64 KiB (checked between chunks), stops at the closing tag, and skips
  bodies the server declared as non-HTML. The title is whitespace-collapsed,
  entity-decoded and cut to 200 characters (with a trailing `…` when longer).

| Format | Output |
|--------|--------|
| `json` / `jsonl` | A key per field, only when it has a value |
| `csv` | Columns appended after the existing ones. The title gets the same spreadsheet-formula escaping as the URL |
| `plain` | The URL and ` [status]`. `--check-title` appends ` [title="…"]` (quoted, titles contain spaces); `--show-meta` adds `location`, `content_length` and `content_type` to the same bracket |

```bash
# status plus response head, as JSON Lines
urx example.com --check-status -f jsonl

# titles in plain text (add --show-meta for the head fields)
urx example.com --check-title

# where did the redirects point?
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

Defaults are `wayback,cc,otx`, plus `robots` and `sitemap`, which fetch the
target's live `robots.txt` and `sitemap.xml`. Turn those two off with
`--exclude-robots`, `--exclude-sitemap` or `--exclude-providers robots,sitemap`
(`--all-providers` still honours those switches).

- Keyed providers are enabled automatically when their key is set.
- `arquivo` (the Portuguese web archive) is keyless but opt-in. Add it with
  `--providers` or `--all-providers`.
- `urlscan` works without a key (about 30 requests/min per IP). A key raises the
  limits and enables rotation.
- `github` searches GitHub Code Search and needs a personal access token
  (`--github-api-key` or `URX_GITHUB_API_KEY`).
- `bevigil` returns URLs [BeVigil](https://bevigil.com/osint-api) extracted from
  unpacked Android apps, and needs an API key (`--bevigil-api-key` or
  `URX_BEVIGIL_API_KEY`).

`urx --list-providers` prints the catalog (id, key requirement, one-line
summary) from the binary. It also lists any `--cdx-endpoint` named on the same
command line, with the id it will run as.

### Custom CDX Endpoints

`--cdx-endpoint URL` queries any pywb, OutbackCDX or Internet Archive-style CDX
server:

```bash
# Icelandic web archive, alongside the default providers
urx example.is --cdx-endpoint https://vefsafn.is/cdx

# only that endpoint, one request per second
urx example.is --cdx-endpoint https://vefsafn.is/cdx --providers cdx:vefsafn.is --rate-limit-by cdx:vefsafn.is=1

# several at once; each gets its own progress line, stats row and rate limit
urx example.com --cdx-endpoint https://vefsafn.is/cdx --cdx-endpoint http://localhost:8080/cdx
```

- The provider id is `cdx:<host>`, or `cdx:<host>:<port>` when the URL has a
  port. Use it with `--providers`, `--exclude-providers` and `--rate-limit-by`;
  `--stats` and `--show-sources` show it. Naming an endpoint enables it.
- It honours `--subs`, `--from`/`--to`, the `--archive-*` filters, pagination and
  rate limiting like `wayback`/`cc`/`arquivo`, and reports capture metadata.
- `--cdx-dialect classic|pywb` sets the server's dialect (field names, filter
  semantics, row format, pagination; see
  [Archive-side Filtering](#archive-side-filtering)). Unset, urx probes once per
  run and falls back to `pywb`. Set it when the probe can't tell, e.g. the
  server gives an empty answer for an unknown domain.
- Config file: `cdx_endpoint = [...]` and `cdx_dialect` under `[provider]`.

The only public endpoint verified to work is `https://vefsafn.is/cdx`
(Iceland, pywb). It ignores `limit`, `page` and `showNumPages` and returns the
whole result set, which urx handles. After a few requests it may answer with an
Anubis-style "Session Verification" page. urx reports that as an error naming
the endpoint, never as an empty result. Slow down with
`--rate-limit-by cdx:vefsafn.is=1` or retry later.

These do not work: the UK Web Archive (`webarchive.org.uk`), the Library of
Congress (`webarchive.loc.gov`), Bibliotheca Alexandrina and the National
Library of Australia (`web.archive.org.au`). Bot protection or redirects block
their CDX APIs, and urx reports the same HTML-answer error.

### Provider Notes

| Provider | What it queries, and its limits |
|----------|---------------------------------|
| `otx` | `/api/v1/indicators/{domain\|hostname}/…/url_list`, 200 URLs per page, at most 1,000 pages. The `hostname` endpoint is used for a target with three or more labels (`sub.example.com`, but also `example.co.uk`) without `--subs`; it excludes `www.` and other subdomains |
| `vt` | v3 `domains/{domain}/urls`, 40 per page with cursor pagination. A 404 means no data; a 429 waits for the server's `Retry-After` (at most 60 s) before retrying |
| `urlscan` | `search/?q=domain:<domain>` with 100 results per page and `search_after` paging. Works without a key |
| `github` | Code Search for `"<domain>"`; URLs come from the matched text fragments, not whole files. GitHub caps code search at 1,000 results (10 pages × 100). Any personal access token works |
| `zoomeye` | `api.zoomeye.ai/v2/search` with the dork `site:<domain>` (`site:*.<domain>` under `--subs`), 100 per page |
| `bevigil` | One request per domain to `osint.bevigil.com/api/<domain>/urls/`, no pagination |

`--subs` changes the query only for the CDX providers, ZoomEye
(`site:*.<domain>`), and OTX on a target with three or more labels (it switches
from the `hostname` to the `domain` endpoint). GitHub sends the same search
either way but keeps subdomain URLs only under `--subs`; without it, GitHub
keeps URLs on exactly the target host (not even `www.`). VirusTotal, urlscan,
BeVigil and the `robots`/`sitemap` fetchers send the same query either way, and
host validation keeps or drops the subdomains they return.

Naming a keyed provider without a key prints `Error: The … provider (…) requires
an API key…` (not under `--silent` or `--all-providers`) and skips it. The run
continues with exit code 0 if any other provider remains, including the default
`robots`/`sitemap` probes. If it was the only provider, the run fails with
`No valid providers specified` and exits 1.

## Reading URLs from Files

`--files` reads URLs from local files. No provider is queried.

- Positional domains and `--domain-list` are ignored. `--files` takes several
  paths, so a domain placed after it is read as another file.
- No host validation, since there is no target. Use `--scope-file` or
  `--match-regex` to restrict hosts.
- Every URL is attributed to `file` in `--show-sources` and `--stats`.
- Filters, testers and output options work as usual. `--stream` is rejected;
  the cache and `--incremental` are not used.

The format is picked by file name:

| File name | Read as |
|-----------|---------|
| `.warc` | WARC |
| `.gz` or `.bz2` | WARC if the name contains `warc`, otherwise URLTeam |
| `.txt` or `.list` | text |
| other, containing `warc` | WARC |
| other, containing `urlteam` / `url_team` | URLTeam |
| anything else | text |

WARC and URLTeam files are checked for gzip by magic bytes, and multi-member
streams are read in full. Text files are never decompressed: a gzip or bzip2
file with a text name yields no URLs and no error, so give it a `.gz` extension
or decompress it first. The WARC and URLTeam readers do not support bzip2: the
run stops with `bzip2 input is not supported. Decompress it first`
(`bzip2 WARC input…` for a WARC) and exits 1.

| Format | Extracted |
|--------|-----------|
| text | Each non-blank, non-`#` line starting with `http://` or `https://` |
| URLTeam | The first `http(s)://` token on each line |
| WARC | `WARC-Target-URI` headers and bare URL lines in the payloads |

Each file is capped at 1,000,000 URLs and 1 GiB of decompressed input. Lines
over 1 MiB are skipped. Hitting a cap prints a warning.

```bash
urx --files urls.txt --check-status --include-status 200
urx --files crawl.warc.gz --scope-file scope.txt -f jsonl
```

## Shell Completions and the Man Page

Both are generated by the binary, so they match the installed version. Neither
needs a target domain. Regenerate after upgrading urx.

```bash
# zsh: write into a directory on $fpath, then re-run compinit
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

## Filter Presets

`no-*` presets exclude a family, `only-*` presets keep only that family.
Singular spellings (e.g. `no-image`, `only-font`) work too.

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

These match a listed extension **or** a listed shape. Substring shape rules are
checked against the lower-cased path, query and fragment (never the host). So `only-secrets`
catches `/.env` (no extension) and `only-backup` catches `/index.php~`.

| Preset | Description |
|--------|-------------|
| `only-secrets` | Leaked credentials and VCS metadata: `/.env`, `/.git/`, `/.svn/`, `/.aws/`, `/.ssh/`, `id_rsa`, `.htpasswd`, `credentials`, `*.pem`, `*.key`, `*.p12` |
| `only-backup` | Backups and archived copies: `*.bak`, `*.old`, `*.orig`, `*.swp`, `*.sql`, `*.dump`, `*.zip`, `*.tar.gz`, `/backup/`, and paths ending in `~` |
| `only-config` | Configuration files: `*.conf`, `*.config`, `*.ini`, `*.yaml`, `*.yml`, `*.toml`, `*.properties`, `web.config`, `.htaccess`, `.npmrc`, `Dockerfile` |
| `only-api` | API surfaces: `/api/`, `/v1/` to `/v4/`, `/rest/`, `/graphql`, `/swagger`, `/openapi`, `/wp-json`, `*.wsdl` |

Singular and plural spellings both work (`only-secret`, `only-backups`,
`only-configs`, `only-apis`). Presets combine with OR, so
`-p only-secrets,only-backup` keeps what either one keeps.

```bash
urx example.com -p only-secrets
urx example.com -p only-backup,only-config
```

## Scoping a Run to a Path

A target can include a path. `urx example.com/shop` collects only the part of
the site under `/shop`.

```bash
urx example.com/shop
urx https://example.com/api/v2      # a pasted URL works too
```

CDX providers get the path in the query (`url=example.com/shop*`), so the
archive never sends the rest of the site. On a large target that is a few
hundred rows instead of a few hundred thousand.

- Providers that can't put a path in their query (OTX, VirusTotal, urlscan,
  GitHub, BeVigil, ZoomEye, and the `robots`/`sitemap` fetchers, which always
  read the site root) are queried by host. Host validation then narrows their
  results.
- With `--subs` the path can't go in the query either: the leading `*.` selects
  `matchType=domain`, which no CDX server combines with a path. The prefix is
  applied to the results instead.
- Scope means at or under the path: `/shop` and `/shop/cart` are in,
  `/shopping` is out.
- Case is ignored. CDX servers lower-case the URL in their index key, so
  `example.com/Shop*` and `example.com/shop*` return the same rows, all in lower
  case.
- A query string or fragment in the target is dropped.
- `--no-strict` turns off the host check but keeps the path scope.

> Note: older urx versions dropped the path, so `urx https://example.com/shop`
> scanned all of `example.com`. Pass only the host for that. A run whose target
> has a path says so on stderr.

## Host Validation

With `--strict` (the default) a URL is kept only if its host is one of the
targets. `www.<target>` counts as the target, so a site served only on `www.`
is kept. With `--subs`, any subdomain of a target is kept too. `--no-strict`
turns off the host check but keeps a target's path scope. `--strict` takes no
value; use `--no-strict` to turn it off (`--strict false` reads `false` as a
domain).

This applies to targets from positional arguments, `--domain-list` and stdin. A
target path narrows CDX queries and also filters every provider's results
client-side.

When validation removes more than half of the URLs left after the other filters
and `--subs` is off, urx prints a hint on stderr, even without `-v`:

```
[urx] strict host validation removed 812/1400 URLs; pass --subs to keep subdomains or --no-strict to keep all hosts
```

When the target has a path, the hint suggests dropping the path instead of
`--no-strict`. Under `--no-strict` it reads `[urx] the target's path scope
removed …`. The hint is printed for batch output only (not under `--stream`).
`--silent` hides it.

## Regular-expression Filtering

`--patterns` and `--exclude-patterns` are substring tests (every metacharacter
is literal). `--match-regex` and `--filter-regex` use
[regex syntax](https://docs.rs/regex/latest/regex/#syntax):

| | `--patterns` | `--match-regex` |
|---|---|---|
| Matching | substring | full regex syntax |
| Case | insensitive (both sides lower-cased) | **sensitive**; prefix `(?i)` to opt out |
| Multiple values | comma-separated (the flag may also be repeated) | repeat the flag; commas are never split |

The regex runs against the whole URL as collected (scheme, host, path, query),
so `^https://` and `\.js$` both work. Several `--match-regex` values are ORed.
One `--filter-regex` hit drops a URL, and exclusion beats inclusion. A malformed
expression aborts the run at startup, before any archive is queried.

```bash
# versioned API paths only
urx example.com --match-regex '/api/v[0-9]+/'

# two alternatives, one per flag (a comma inside a regex stays intact)
urx example.com --match-regex '\.php$' --match-regex '/admin/[a-z]{3,8}$'

# drop build output
urx example.com --filter-regex '/(assets|static|dist)/'
```

## Scope Files

`--scope-file` reads a bug bounty scope list as platforms write it, so you don't
have to turn it into anchored regexes (where a wrong anchor quietly widens the
scope).

```text
# scope.txt: in scope
*.example.com
api.example.org

# out of scope, even though the wildcard above covers them
!admin.example.com
!*.internal.example.com
```

```bash
urx example.com --subs --scope-file scope.txt

# several programs at once; the files are unioned
urx --domain-list targets.txt --subs --scope-file scope-a.txt --scope-file scope-b.txt
```

Rules:

* One host pattern per line. A `!` prefix excludes it.
* `*.example.com` matches `example.com` **and** every host under it (the
  bug-bounty reading). To exclude the apex, add `!example.com`.
* A bare `example.com` matches only that host: not `www.`, not subdomains.
* A lone `*` matches every host, for a file that is only a deny-list.
* `#` starts a comment that runs to the end of the line. Blank lines are
  skipped.
* **Exclusion always wins**, as `--filter-regex` beats `--match-regex`.
* A file with no include lines is a deny-list: everything is in scope except
  what it excludes.
* IPv6 literals must be bracketed (`[2001:db8::1]`). They are exact hosts and
  can't be wildcarded.
* Anything else (a port, a path, a wildcard in the middle) is a startup error
  naming the file and line.

`--scope-file` and `--strict` are separate checks and a URL must pass both. A
`*.example.com` line while querying the bare apex still needs `--subs`, because
strict mode drops the subdomains first. The scope applies to every provider, the
batch result, the `--stream` output and links found by `--extract-links`.

## Archive-side Filtering

`--from`, `--to`, `--archive-status`, `--archive-exclude-status`,
`--archive-mime` and `--archive-exclude-mime` are sent to the archive's CDX
index, so filtered-out rows never come over the network.

```bash
# captures from 2018 to 2020 that the archive saw as 200
urx example.com --from 2018 --to 2020 --archive-status 200

# skip redirects, 404s and HTML
urx example.com --archive-exclude-status 301,302,404 --archive-exclude-mime text/html

# JSON endpoints, even ones with no file extension
urx example.com --providers wayback --archive-mime application/json
```

- They apply only to CDX-backed providers: `wayback`, `cc`, `arquivo` and any
  `--cdx-endpoint`. Other providers ignore them, and urx warns when no CDX
  provider is enabled.
- `--from`/`--to` take `YYYY`, `YYYYMM`, `YYYYMMDD` or `YYYYMMDDhhmmss`. Partial
  dates pad to the start (`--from`) or end (`--to`) of the period. The old names
  `--wayback-from`/`--wayback-to` still work.
- The CDX dialects differ:

| | `wayback`, `--cdx-dialect classic` endpoints | `cc`, `arquivo`, pywb endpoints |
|---|---|---|
| Value matching | regex (`--archive-status "30."` matches any 3xx) | exact |
| Positive list (`--archive-status 200,301`, `--archive-mime a,b`) | works | skipped with a warning (repeated filters are ANDed, so it could never match) |
| Exclusion list (`--archive-exclude-*`) | works | works |

A `--cdx-endpoint` with no `--cdx-dialect` is treated as pywb for that warning.

`--archive-status` is the status the archive recorded at crawl time and costs
no extra requests. `--check-status`/`--include-status` give the target's status
now and re-request every URL.

## Archive Metadata Filters

The archive-side filters above are free, but they only reach CDX providers and a
positive list is dropped on pywb servers. The eight `--meta-*` filters run
**after** collection on the merged [capture metadata](#archive-capture-metadata)
of each URL, so lists work for every provider:

| Flag | Keeps |
|------|-------|
| `--meta-first-seen-after <DATE>` | URLs whose oldest capture is on or after `DATE` |
| `--meta-first-seen-before <DATE>` | URLs whose oldest capture is on or before `DATE` |
| `--meta-last-seen-after <DATE>` | URLs whose newest capture is on or after `DATE` ("still alive as of") |
| `--meta-last-seen-before <DATE>` | URLs whose newest capture is on or before `DATE` ("dead since") |
| `--meta-mime <TYPE>` | URLs with one of these archived MIME types (`image/*` matches any subtype) |
| `--meta-exclude-mime <TYPE>` | everything except those |
| `--meta-status <CODE>` | URLs with one of these archived status codes (`20x` / `5xx` patterns, as in `--include-status`) |
| `--meta-exclude-status <CODE>` | everything except those |

Dates take the same formats as `--from`/`--to` and pad the same way: an `after`
bound pads to the start of the period, a `before` bound to the end. So
`--meta-first-seen-after 2020 --meta-first-seen-before 2020` means "first
archived during 2020".

```bash
# alive recently, HTML and images out of the way
urx example.com --providers wayback --meta-last-seen-after 2024 --meta-exclude-mime 'text/html,image/*'

# pages that died: nothing captured since 2019
urx example.com --providers wayback --meta-last-seen-before 2019

# JSON the archive served successfully
urx example.com --providers wayback --meta-mime application/json --meta-status 200
```

Using both kinds together is normal: archive-side filters cut what comes over
the network, `--meta-*` filters apply to the merged result.

Many URLs carry no metadata: those from non-CDX providers (`otx`, `vt`,
`urlscan`, `zoomeye`, `github`, `bevigil`, `robots`, `sitemap`), `--files` input
and cache hits. What happens to them depends on the filter:

* A positive filter (`--meta-mime`, `--meta-status`, any date bound) drops them,
  since an absent value can't match.
* An exclusion (`--meta-exclude-mime`, `--meta-exclude-status`) keeps them. It
  drops only positive matches, like `--filter-regex` and `--exclude-status`.

`--verbose` reports the split ("… failed a predicate, … carried no archive
metadata to test"). When missing metadata removes the whole result set, urx says
so even without `-v`, since a cache hit would otherwise look like a target with
nothing to find. Pass `--no-cache` and a CDX provider to get metadata back.

The `--meta-*` flags are rejected under `--stream`, as `--show-meta` is: a
streamed URL is printed before the captures that complete its metadata arrive.

## Collapsing Near-duplicates

`--dedup-similar` prints one line for a group of URLs that are the same endpoint
with different data (`/post/1` … `/post/99999`).

A path segment counts as data when the whole segment is one of:

* a run of digits: `/post/1`, `/page/42`
* a UUID: `/u/550e8400-e29b-41d4-a716-446655440000`
* a 32/40/64-character hex digest (md5, sha1, sha256)
* a separated date: `/blog/2024-01-02/`
* a long mixed-case token containing digits (session ids, signed blobs)

A segment that only contains digits stays, so `/api/v1/` and `/api/v2/` remain
distinct. Lower-case slugs stay too. Query strings are grouped by parameter
names only: `?q=cats&page=1` and `?q=dogs&page=7` collapse together, `?q=cats`
alone does not.

Each group keeps its lexicographically smallest URL, so repeated runs over the
same data give the same output. `--verbose` reports how many URLs were
collapsed.

`--normalize-url`, `--merge-endpoint` and `--dedup-similar` can be combined and
run in that order. `--merge-endpoint` and `--dedup-similar` need the complete
result set and are rejected under `--stream`; `--normalize-url` works per URL
and can stream.

```bash
urx example.com --dedup-similar --verbose
urx --files urls.txt --normalize-url --merge-endpoint --dedup-similar
```

## Parameter and Fuzz Views

`--show-only-param` only cuts each URL down to its query. These three views
replace the URL list with parameter data instead, using the same grouping as
`--dedup-similar`.

`--params` prints every query parameter name in the result set, once each,
sorted.

```console
$ urx example.com --params
page
q
ref
sort
utm_source
```

`--params-by-endpoint` prints one line per endpoint: the endpoint, a space, and
the comma-separated parameter names seen on it. Id-like path segments collapse
to `{id}` as in `--dedup-similar`, so `/post/1?a=1` and `/post/2?b=2` become one
endpoint taking `a,b`. The endpoint includes scheme and host, so
`a.example.com/search` and `b.example.com/search` stay separate.

```console
$ urx example.com --params-by-endpoint
https://example.com/post/{id} ref,utm_source
https://example.com/search page,q,sort
```

`--fuzz-placeholder VALUE` sets every query parameter value to `VALUE` and keeps
one URL per parameter signature. URLs without parameters are dropped. Each kept
URL has its real path (a `{id}` would not route) and is the smallest URL of its
group, so output is reproducible.

```console
$ urx example.com --fuzz-placeholder FUZZ
https://example.com/post/1?ref=FUZZ
https://example.com/post/2?utm_source=FUZZ
https://example.com/search?q=FUZZ&page=FUZZ
https://example.com/search?q=FUZZ&sort=FUZZ
```

```bash
# into ffuf
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ

# or dalfox
urx example.com --fuzz-placeholder FUZZ | dalfox pipe
```

Parameter names are taken from the raw query without decoding.

- All three need the complete result set and are rejected under `--stream`.
- They are mutually exclusive with each other and with the `--show-only-*`
  views. The three `--show-only-*` views are also mutually exclusive with each
  other.
- A command-line view replaces any configured `show_only_*` view. A config file
  that sets more than one of those keys to `true` is rejected.
- `--params`, `--params-by-endpoint` and the `--show-only-*` views are rejected
  with any tester that requests URLs (`--check-status` and the status filters,
  `--check-title`, `--extract-links`, `--extract-js-endpoints`,
  `--archive-body`, `--expand-specs`), because they rewrite URLs before those
  would be requested. `--fuzz-placeholder` output is real URLs and can be
  checked.

## Wordlist Output

`-f wordlist` prints every path segment and query parameter name the run saw,
deduplicated across the run and sorted, one per line.

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

- Segments that look like data are dropped, using the `--dedup-similar` test
  (numbers, UUIDs, dates, session tokens). A segment whose stem is an identifier
  goes too (`1234.html`); a name with a number attached (`article-1234.html`)
  stays.
- Case is kept. Paths are case-sensitive on most servers, so lower-casing
  `WebResource.axd` would give a word that 404s, and `/Admin` next to `/admin`
  is worth keeping.
- Batch only: `--stream` rejects it.
- Per-URL fields (status, `--show-sources`, capture metadata) are not emitted.
- `--output-dir` writes wordlists as `.txt`. `[output].format` in the config
  file accepts `wordlist`.

```bash
# build a target wordlist and fuzz with it
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ
```

## Streaming Output

By default urx collects everything, then filters, sorts and prints once, so
nothing appears until the slowest provider finishes. `--stream` writes each URL
as soon as its provider reports it.

```bash
# matches appear right away
urx big-target.com --stream | grep admin

# jsonl stays valid while it is being written
urx big-target.com --stream -f jsonl | jq -r 'select(.url | test("/api/")) | .url'
```

- Streamed URLs pass the same filters and host validation as a batch run and
  are still deduplicated.
- Output is unsorted (provider-completion order). Pipe through `sort` if you
  need order.
- Formats: `plain`, `jsonl`, `csv`. `json` is refused (use `jsonl`; a JSON array
  has to know its last entry), and so is `wordlist` (no term is known to be new
  until every URL is in).
- The cache is bypassed.
- `--silent` does not hide streamed URLs.
- When the reader goes away (`| head -1`), fetching stops and the run exits 0.
- Memory use is lower: only the set of URLs already written is kept.
- `--normalize-url`, the `--show-only-*` views, `--scope-file` and
  `--archived-discovery` work with it.

Options that need the complete result set are rejected at startup, with a
message naming each one: `--merge-endpoint`, `--dedup-similar`,
`--check-status`, `--include-status`, `--exclude-status`, `--check-title`,
`--extract-links`, `--extract-js-endpoints`, `--archive-body`,
`--expand-specs`, `--incremental`, `--show-sources`, `--show-meta`, the
`--meta-*` filters, `--params`, `--params-by-endpoint`, `--fuzz-placeholder`,
`--output-dir` and `--files`.

## Authenticated and Custom Requests

`--check-status`, `--extract-links`, `--extract-js-endpoints` and
`--expand-specs` re-request collected URLs from the target. `-H` adds headers to
those requests:

```bash
urx example.com --check-status -H "Authorization: Bearer $TOKEN"
urx example.com --extract-links --cookie "session=abc; role=admin"
urx example.com --check-status --user-agent "acme-security-scan/1.0"
```

- `-H` is repeatable and takes `Name: value`. A malformed one stops the run, so
  an anonymous scan can't pass for an authenticated one.
- `--cookie` and `--user-agent` are shorthands for those headers. A later value
  for the same name replaces an earlier one.
- **These headers never reach an archive.** Only components that talk to the
  target send them: the four testers above and the `robots`/`sitemap`
  providers. Other providers, and `--archive-body` replays, query
  web.archive.org, index.commoncrawl.org or third-party APIs, which have no use
  for your session cookie. Archive queries keep urx's own User-Agent, which
  `--random-agent` still rotates.
- They follow `--network-scope`. Under `--network-scope providers` the testers
  get none (an authenticated `--check-status` goes out anonymous); under
  `--network-scope testers` the `robots`/`sitemap` providers get none.

In the config file:

```toml
[network]
header = ["X-Env: staging", "X-Team: appsec"]
cookie = "session=abc"
user_agent = "acme-security-scan/1.0"
```

Any `-H` on the command line replaces the configured `header` list, so a run can
drop the configured headers without editing the file.

## Link Extraction

`--extract-links` re-fetches every URL left after filtering and pulls links out
of the HTML. It reads every URL-bearing tag:

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

- Relative URLs resolve against the page, honouring `<base href>`.
- `javascript:`, `mailto:`, `tel:`, `data:`, `about:`, `blob:` and bare
  `#fragment` references are skipped.
- Duplicates are collapsed.
- Found links go through the same filters, host validation and output
  transforms as provider URLs. The filters also run on the collected URLs
  *before* any page is fetched, so `--extract-links -e js` drops the HTML pages
  first and finds nothing. Filter the output instead.
- Found links are not status-checked. `--check-status` covers the collected URLs
  only.
- Only successful responses that look like markup are parsed. Each body is
  capped at 10 MiB.

```bash
# one hop deeper, keep only JavaScript
urx example.com --extract-links | grep -E '\.js(\?|$)'

# extraction uses the network settings too
urx example.com --extract-links --proxy http://localhost:8080 --timeout 20
```

## JavaScript Endpoint Extraction

`--extract-js-endpoints` fetches collected JavaScript and pulls endpoints out
of its string literals: `fetch("/api/v2/users")`, `axios.post("/graphql")`,
`` `/api/orders/${id}` ``. `-e js` only collects bundles; this reads them.

URLs whose extension is clearly not script (images, fonts, CSS, archives,
`.json`, `.map`, ...) are skipped without a request. The rest are fetched and
sorted by `Content-Type`:

| Response | Scanned |
|----------|---------|
| JavaScript type | Whole body |
| HTML | Inline `<script>` blocks only |
| `.js`/`.mjs`/`.cjs`/`.jsx`/`.ts`/`.tsx` URL served as `text/plain`, `application/octet-stream`, or with no `Content-Type` | Whole body, as script |
| Any other URL with no `Content-Type` | Inline `<script>` blocks, as HTML |
| Any other type | Discarded unread |

Extracted:

- Quoted absolute and origin-relative paths: `"/api/v2/users"`, `'/graphql'`.
- Full and protocol-relative URLs: `"https://api.example.com/v1"`, `"//cdn.example.com/x"`.
- The URL argument of `fetch(...)`, `axios.get/post/...(...)`, `axios({url: ...})`,
  `$.ajax(...)` and `XMLHttpRequest.open(method, ...)`, including the
  `base + "/path"` form.
- The static prefix of a template literal: `` `/api/users/${id}` `` → `/api/users/`.
- ES-module chunk imports and asset references: `import("./chunk-ab12.js")`.

Relative paths:

- `/api/x` resolves against the script's origin, as the browser does.
- `./chunk.js` resolves against the script's own URL, as an ES-module import
  does.
- Bare relative paths (`api/v1/x`) and `./x` request arguments depend on the
  page that loaded the bundle, which urx doesn't know. They resolve against the
  origin root.

Minified bundles produce a lot of noise, so the extractor errs toward dropping.
The code comments in `src/testers/js_endpoint_extractor.rs` show the real-bundle
case behind each rule.

| Dropped | Why |
|---------|-----|
| MIME types (`image/png`, `application/json`, `*/*`) | Header values shaped like a two-segment path; the most common false positive |
| `"/"`, `"/x"`, `"//"`, `"./"`, `"../"` | Path-join fragments and comment delimiters (`fetch("/")` is still kept) |
| Sourcemap directives, `//#`, `//@` | Not URLs |
| Base64 and `data:` payloads | Inline images and fonts; hex content hashes and hashed filenames survive |
| CSS shorthand and ratios (`12px/1.5`, `16/9`) | Style values |
| Date formats (`MM/DD/YYYY`, `HH/mm`) | Formatting tokens |
| Bare extensions (`.js`, `/.png`) | File-type checks |
| Regex sources and tag fragments (`/^\/api/(\d+)`, `</div>`) | Excluded by the character class, so they never match |
| Strings closed by a different quote (`'/g,"`), trailing `,` `;` `:` | Artifacts of regex literals next to strings |
| Bare relative paths with no leading `/` or `.` (`react/jsx-runtime`, `en/US`) | Package paths and locale tags. Kept only when a segment has three or more characters and there are three or more segments, a query string, or a dot in the last segment (`app/main.js`). `fetch` / `axios` / XHR arguments are exempt |
| Package and source-tree paths (`@scope/pkg/...`, `node_modules/...`, `./src/...`, `lib/esm/...`) | Import specifiers and webpack module keys |
| `./x` without a fetchable extension (`require("./utils")`, `{"./zlib/deflate":46}`) | CommonJS module specifiers (one real bundle had ~50 from jszip and pako alone) |
| Path suffixes after `+` (`"/users/" + id + "/avatar"` → `/avatar`) | Only the prefix is a route; `base + "/api/x"` is still kept |
| XML namespaces and schema hosts (`www.w3.org`, `schema.org`) | Boilerplate in every SVG-bearing bundle |

A string passed to a request call is known to be a URL and skips the length and
bare-relative rules.

- Each body is capped at 10 MiB. Fetches use `--timeout`, `--retries`,
  `--proxy`, `--insecure`, `--random-agent` and `--rate-limit`.
- `--max-js-files` caps files fetched per run (default 500, `0` for unlimited).
- Found endpoints go through the same filters and host validation as
  everything else. Strict mode (the default) drops off-site URLs; `--no-strict`
  keeps them.
- With `-e js`, only `.js` endpoints survive the filters. The extractor already
  picks JavaScript itself, so leave `-e js` off to get API paths.
- Found endpoints are not status-checked. `--check-status` covers the collected
  URLs only.
- Rejected under `--stream`.

```bash
# mine every collected script for API paths
urx example.com --extract-js-endpoints

# keep API-looking paths (filter the output; --patterns would drop the
# bundles before they are read)
urx example.com --extract-js-endpoints | grep -E 'api|graphql'

# at most 50 bundles, one request per second
urx example.com --extract-js-endpoints --max-js-files 50 --rate-limit 1
```

## API Specification Expansion

`--expand-specs` fetches collected OpenAPI, Swagger and GraphQL documents and
adds every route they describe to the results. The other testers skip these
files: `--extract-links` parses HTML, `--extract-js-endpoints` drops
`application/json` bodies, and `--archive-body` runs the HTML parser.

```bash
# find the specs and expand them in one run (only-api also filters the
# expanded routes; drop it to keep every documented path)
urx example.com --preset only-api --expand-specs

# bound and pace it
urx example.com --expand-specs --max-spec-files 10 --rate-limit 2

# an API that no longer exists: read the archived document
urx example.com --archive-body --expand-specs
```

What is expanded (JSON or YAML):

- OpenAPI 3.x: `servers[].url` (absolute, relative to the document, or
  templated, with `{var}` taken from `variables[var].default`, else the first
  `enum` value) crossed with every `paths` key. A path item's own `servers`
  override the document's.
- Swagger 2.0: `schemes` × `host` + `basePath`, each part falling back to the
  matching part of the document's own URL when omitted. `ws`/`wss` schemes are
  dropped.
- GraphQL introspection (`{"data":{"__schema":…}}` or unwrapped): one URL per
  query, mutation and subscription field, written as the endpoint plus
  `?query=…`. A schema saved as a file resolves to its endpoint
  (`/graphql/schema.json` → `/graphql`).

Which URLs are fetched: the path must contain a spec marker (`swagger`,
`openapi`, `api-docs`, `apidocs`, `api_docs`, `graphql`, `introspection`), and
if the URL has an extension it must be `json`, `yaml` or `yml`. So
`swagger-ui.html` and `swagger-ui-bundle.js` cost no request. Then
`Content-Type` decides: a clearly unrelated type (HTML, image, JavaScript) skips
the body, while vague types (`text/plain`, `application/octet-stream`) fall back
to the extension and then the first byte of the body, so an untyped
`/v3/api-docs` still parses.

### Details

- Path templates are kept as written (`/users/{id}`, not `/users/%7Bid%7D`).
  `--normalize-url` encodes them if you ask for it.
- `--max-spec-files` (default 50) caps documents fetched per run; `0` means
  unlimited.
- Each body is capped at 10 MiB. A YAML document whose aliases would copy more
  than 10 MiB of nodes is refused before parsing ("billion laughs" guard).
- Only the first document of a multi-document YAML stream is read.
- Found URLs go through the same filters, host validation and output
  transforms as provider URLs.
- Rejected under `--stream`.
- With `--archive-body` on, an archived spec is read as a spec instead of being
  handed to the HTML link extractor. No extra requests: the body was already
  fetched and counted against `--archive-body-limit`. This recovers routes after
  the live API was retired, moved behind auth, or its document removed.

## Archived Response Bodies

`--archive-body` fetches the bodies the Wayback Machine stored for the
collected URLs and runs the `--extract-links` extraction on them. This finds
links on pages that no longer exist.

Each collected URL with a capture timestamp is replayed raw:

```text
https://web.archive.org/web/<timestamp>id_/<url>
```

`id_` turns off the Wayback toolbar and link rewriting, so the response is the
original bytes with the original `Content-Type`. Relative links resolve against
the captured URL.

```bash
# links from the archived bodies of everything the CDX providers found
urx example.com --archive-body

# bound and pace it; the archive is one host however many URLs there are
urx example.com --archive-body --archive-body-limit 200 --rate-limit 5

# only the JavaScript those pages referenced back then
urx example.com --archive-body | grep -E '\.js(\?|$)'
```

### Why this needs far fewer requests than waymore

Captures with the same CDX digest have identical bodies, and archives are full
of them: `?utm_source=` variants, `/index.html` next to `/`, tracking-parameter
permutations. Tens of thousands of URLs often collapse to a few thousand
distinct bodies. urx fetches each digest once and skips later URLs with the same
digest. waymore downloads one response per URL and caps the volume with
`-l 5000`.

`--archive-body-limit` (default 500) counts distinct bodies, so duplicates
don't count against it. `--verbose` reports how many URLs were skipped as
duplicates, fell past the limit, or had no capture to replay.

### Mining archived JavaScript

Bundles are named by build hash, so `app.a3f9c2.js` 404s after a redeploy and
live `--extract-js-endpoints` can't read it. With both flags, urx mines the
archived bundles and the inline `<script>` blocks of archived pages:

```bash
urx example.com --archive-body --extract-js-endpoints
```

An archived body is checked for script before markup, because a capture with
no `Content-Type` would otherwise go to the HTML parser and yield nothing.

### Keeping the bodies

`--archive-body-dir` saves the replayed bodies at no extra request cost, for
grepping comments, inlined tokens or stack traces:

```bash
urx example.com --archive-body --archive-body-dir ./corpus
grep -ri "api[_-]key" ./corpus
```

- Files are named from the URL plus a hash of it (the slug is lossy).
  `corpus/index.jsonl` maps each file to its URL, capture timestamp, digest and
  content type.
- Only text-like bodies are stored (HTML, script, JSON, XML, CSS, plain text).
- An unwritable directory stops the run at startup. A failure on one file is
  reported and the run keeps its links.
- `--archive-body-dir` without `--archive-body` (including `archive_body_dir`
  set only in the config file) is rejected at startup.

### Details

- Only URLs with a capture timestamp qualify. The CDX providers (`wayback`,
  `cc`, `arquivo`, and any `--cdx-endpoint`) supply one; `--files` input,
  non-CDX providers and cache hits (the cache stores URLs only) have none. urx
  says so when there is nothing to replay. Pass `--no-cache` for fresh captures.
- The newest capture of each URL is replayed, and its digest is the dedup key.
  A timestamp from another archive lands on the nearest Wayback capture. A URL
  the Wayback Machine never saw returns 404 and is skipped quietly.
- Captures the archive recorded as errors are not mined, as `--extract-links`
  ignores live error pages. Non-markup bodies are skipped unparsed.
- Found links go through the same filters, host validation and output
  transforms as provider URLs.
- Each body is capped at 10 MiB.
- `--rate-limit`, `--rate-limit-by wayback=N`, `--parallel`, `--proxy`,
  `--timeout` and `--retries` apply to replays. Under
  `--network-scope providers`, replays count as testers and get none of these
  settings.
- Rejected under `--stream`.

## Archived robots.txt and sitemap.xml

The `robots` and `sitemap` providers read the live files. `--archived-discovery`
also reads every distinct version the Wayback Machine stored. Old `Disallow:`
lines and old sitemaps name paths the live files no longer mention.

```bash
# every archived version of robots.txt and sitemap.xml, plus the live ones
urx example.com --archived-discovery

# bound and pace it; both archived providers follow --rate-limit-by
urx example.com --archived-discovery --archived-discovery-limit 20 --rate-limit-by robots=2,sitemap=2

# only versions captured in a given period
urx example.com --archived-discovery --from 2014 --to 2016

# only the robots.txt history
urx example.com --archived-discovery --exclude-sitemap --show-sources
```

### How it works

1. Versions are listed with one CDX query per file name (`robots.txt`,
   `sitemap.xml`, `sitemap_index.xml`, `sitemap.txt`):

   ```text
   /cdx/search/cdx?url=<domain>/robots.txt&fl=original,timestamp,statuscode,digest
       &collapse=digest&filter=statuscode:2..
   ```

   `collapse=digest` folds consecutive identical captures into one row, so you
   get one row per change. The status filter keeps it cheap: the CDX urlkey
   merges `www.` and the apex, and their interleaved `301`/`200` rows otherwise
   break the collapse. On github.com/robots.txt that is 325,036 rows without the
   filter and 13,909 with it, for the same 107 versions. Remaining duplicate
   digests are dropped client-side.
2. Each version is replayed raw (`/web/<timestamp>id_/<original url>`) and read
   by the **same parser as the live file**, with the same absolute-path and
   pattern-skipping guards. Its paths land on the host that served it. An
   archived `<sitemapindex>` is followed into its children at the same
   timestamp, with the same same-host rule as the live walk.
3. Captures recorded as anything but success (github.com's robots.txt was a 401
   for part of 2007) are never requested. They, and versions the replay endpoint
   refuses, are counted under `--verbose` only.

### Details

- `--archived-discovery-limit` (default 50) caps documents fetched per domain by
  each archived provider. Newest versions are read first, and nested sitemaps
  count. `--verbose` says when the cap cut the list short.
- The archived reads show as "Robots.txt (archived)" and "Sitemap (archived)"
  in `--stats` and `--show-sources`, but run under the `robots` and `sitemap`
  ids. So `--exclude-robots`, `--exclude-sitemap` and
  `--rate-limit-by robots=N` / `sitemap=N` cover live and archived reads
  together.
- `--from`/`--to` narrow which versions are read. The other `--archive-*`
  filters don't apply to a version history.
- Bodies are capped like the live files: 1 MiB for robots.txt, 50 MiB per
  sitemap document.
- It is a provider, so it works with `--stream` and is cached like any other
  provider (the cache key includes the flag).
