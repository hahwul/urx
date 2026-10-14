+++
title = "Examples"
description = "Worked commands for filtering, provider selection, API keys, link extraction, status checking, streaming and caching."
toc = true
weight = 3
+++

## Usage Examples

### Basic Usage
```bash
urx example.com
urx example.com example.org
cat domains.txt | urx

# domain list files; repeatable, merged with domains on the command line
urx --domain-list domains.txt
urx --dL program-a.txt --dL program-b.txt

# only URLs under /shop
urx example.com/shop
```

#### File Input
```bash
# text, WARC or compressed files; space-separated or repeated
urx --files urls.txt archive.warc data.gz
urx --files urls.txt --files archive.warc

# with filters
urx --files data.txt --patterns api,admin -f json
```

## Output Formats

```bash
urx example.com -o results.txt
urx example.com -f json -o results.json
urx example.com -f csv -o results.csv

# one JSON object per line
urx example.com -f jsonl | jq -r '.url'
```

### One File per Host
```bash
# out/example.com.json, out/example.org.json, ... plus the combined all.json
urx example.com example.org -f json --output-dir out/ -o all.json
```

Each host gets its own file, so `www.example.com` and every subdomain (under
`--subs`) are split out. Output that is not URLs (`--show-only-host`,
`--show-only-path`, `--show-only-param`, `--params`) goes to `_unknown.<ext>`.

### Streaming Output
```bash
# write URLs as each provider reports them
urx example.com --stream | httpx -silent
```

`--stream` output is unsorted, skips the cache, and supports `plain`, `jsonl`
and `csv` only. It rejects options that need the full result set:
`--incremental`, `--check-status` / `--include-status` / `--exclude-status` /
`--check-title`, the extractors, `--archive-body`, `--expand-specs`,
`--merge-endpoint`, `--dedup-similar`, `--show-sources`, `--show-meta`,
`--output-dir`, the parameter views, the `--meta-*` filters and `--files`.

### Provenance and Run Statistics
```bash
# providers that returned each URL
urx example.com --show-sources

# archive capture metadata in plain text
urx example.com --providers wayback --show-meta

# per-provider counts, errors and timings on stderr
urx example.com --stats
```

A repeat run served from the cache has no sources or archive metadata, and
`--stats` prints nothing. If the target was scanned within `--cache-ttl`, add
`--no-cache` here and to the `--meta-*` and `--archive-body` examples below.

### Wordlist
```bash
# path segments and parameter names, deduplicated and sorted
# (segments that look like ids, hashes or dates are left out)
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ
```

### Parameter Inventory
```bash
# every query parameter name, once each
urx example.com --params

# grouped by endpoint
urx example.com --params-by-endpoint

# one URL per parameter signature, values replaced
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ
```

## Filtering Examples

### Extensions and Patterns
```bash
urx example.com -e js,php,aspx
urx example.com --exclude-extensions html,txt

urx example.com --patterns api,v1,graphql
urx example.com --exclude-patterns static,images
urx example.com --patterns api --exclude-patterns test,dev

# URL length
urx example.com --min-length 50 --max-length 200
```

### Using Presets
```bash
urx example.com -p no-images
urx example.com -p no-resources
urx example.com -p only-js
```

### Scope Files
```bash
# scope.txt
#   *.example.com
#   !admin.example.com
urx example.com --subs --scope-file scope.txt

# several files are unioned; exclusions always win
urx --domain-list targets.txt --subs --scope-file scope-a.txt --scope-file scope-b.txt
```

### Archive Metadata Filtering
Runs after collection, on the metadata the archives recorded:
```bash
# still captured since 2024, no HTML or images
urx example.com --providers wayback --meta-last-seen-after 2024 --meta-exclude-mime 'text/html,image/*'

# nothing captured since 2019
urx example.com --providers wayback --meta-last-seen-before 2019

# JSON the archive got with a 200
urx example.com --providers wayback --meta-mime application/json --meta-status 200

# first archived during 2020
urx example.com --providers wayback --meta-first-seen-after 2020 --meta-first-seen-before 2020
```

### Regular Expressions
```bash
# keep versioned API paths (case-insensitive); repeat the flag to OR patterns
urx example.com --match-regex '(?i)/api/v[0-9]+/'

# drop static asset directories
urx example.com --filter-regex '/(assets|static)/'
```

### Collapsing Near-Duplicates
```bash
# /post/1, /post/2 and /post/99999 become one line
urx example.com --dedup-similar
```

### Archive-Side Filters
Applied by the CDX index itself (wayback, cc, arquivo, `--cdx-endpoint`), so
less is fetched:
```bash
# captures from 2023 on, recorded as 200
urx example.com --from 2023 --archive-status 200

# served as JSON, even without a .json extension
urx example.com --archive-mime application/json

# skip error pages and HTML
urx example.com --archive-exclude-status 404,500 --archive-exclude-mime text/html
```

## Provider Selection

```bash
urx example.com --providers wayback,otx

# all keyless sources (urlscan works without a key)
urx example.com --providers wayback,cc,otx,arquivo,urlscan

# every provider; vt, zoomeye, github and bevigil error without a key
urx example.com --providers wayback,cc,otx,arquivo,vt,urlscan,zoomeye,github,bevigil

# same, but keyed providers run only when a key is set
urx example.com --all-providers

# everything except OTX
urx example.com --all-providers --exclude-providers otx

# any other CDX server (here the Icelandic web archive), id cdx:vefsafn.is
urx example.is --cdx-endpoint https://vefsafn.is/cdx --rate-limit-by cdx:vefsafn.is=1

# two Common Crawl indexes in parallel
urx example.com --providers cc --cc-index CC-MAIN-2026-17,CC-MAIN-2025-51

# list providers and which need a key
urx --list-providers
```

`--providers` also runs the live robots.txt / sitemap.xml probes, and any keyed
provider whose `URX_*_API_KEY` is set. Drop them with `--exclude-robots`,
`--exclude-sitemap` or `--exclude-providers`.

### Using a Config File
```bash
# profile plus a separate file holding only the API keys
urx -c ~/.config/urx/bugbounty.toml --provider-config ~/.config/urx/keys.toml example.com
```

See [Configuration](/guide/configuration/) for every key.

### With API Keys
```bash
urx example.com --vt-api-key=YOUR_KEY --urlscan-api-key=YOUR_KEY
urx example.com --zoomeye-api-key=YOUR_KEY --providers zoomeye

# or via environment; a set key also enables its provider
export URX_VT_API_KEY=YOUR_KEY
export URX_URLSCAN_API_KEY=YOUR_KEY
export URX_ZOOMEYE_API_KEY=YOUR_KEY
export URX_GITHUB_API_KEY=YOUR_TOKEN
export URX_BEVIGIL_API_KEY=YOUR_KEY
urx example.com --providers=vt,urlscan,zoomeye,github,bevigil

# key rotation: repeat the flag, or comma-separate the env var
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3
URX_VT_API_KEY=key1,key2,key3 urx example.com
```

## Discovery Options

```bash
# skip robots.txt and/or sitemap.xml
urx example.com --exclude-robots --exclude-sitemap

# also read every archived version of robots.txt and sitemap.xml
urx example.com --archived-discovery

# see which URLs came from an old version
urx example.com --archived-discovery --show-sources

# robots.txt only, versions captured 2014 to 2016
urx example.com --archived-discovery --from 2014 --to 2016 --exclude-sitemap

# at most 10 documents per domain, newest first (default 50)
urx example.com --archived-discovery --archived-discovery-limit 10
```

## Testing & Validation

```bash
# include subdomains
urx example.com --subs

# check HTTP status
urx example.com --check-status
```

### Extract Links
```bash
urx example.com --extract-links
```

Re-fetches each surviving URL and reads every URL-bearing tag: `<a href>`,
`<script src>`, `<link href>`, `<form action>`, `<iframe src>`, `<img src>`,
`<source src>`, `<object data>`, `<embed src>` and `<meta http-equiv="refresh">`.
Relative URLs resolve against the page (and `<base href>`), and duplicates are
collapsed.

Filters and host validation run twice: on the collected URLs before any page is
fetched, and on the discovered links. So `-e js` would drop the HTML pages before they are read.
Filter the output instead:

```bash
# JS URLs, collected or referenced by the pages
urx example.com --extract-links | grep -E '\.js(\?|$)'
```

### Extract Endpoints from JavaScript
```bash
urx example.com --extract-js-endpoints

# cap the scripts fetched (default 500)
urx example.com --extract-js-endpoints --max-js-files 100

# scripts found by --extract-links are not mined in the same run; chain two runs
urx example.com --extract-links | grep -E '\.js(\?|$)' > bundles.txt
urx --files bundles.txt --extract-js-endpoints --max-js-files 100

# API-looking paths (--patterns would drop the bundles before they are read)
urx example.com --extract-js-endpoints | grep -E 'api|graphql'
```

Fetches each collected URL that looks like a script and pulls paths and URLs
from its string literals: `fetch("/api/v2/users")`, `axios.post("/graphql")`,
the static prefix of `` `/api/orders/${id}` ``, chunk imports. Noise (MIME
types, module specifiers, base64, CSS values, regex fragments) is dropped; see
the [CLI options guide](/guide/cli-options/#javascript-endpoint-extraction) for
the full policy. Bodies are capped at 10 MiB.

- Discovered endpoints pass through your filters and host validation, so leave
  `-e js` off or only the `.js` files survive.
- Discovered endpoints are not status-checked; `--check-status` covers the
  collected URLs only.

### Mine Archived Bodies
```bash
# extract links from the bodies the Wayback Machine stored
urx example.com --archive-body

# fewer bodies (default 500), paced
urx example.com --archive-body --archive-body-limit 200 --rate-limit 5

# JS URLs, collected or referenced by the archived pages
urx example.com --archive-body --no-cache | grep -E '\.js(\?|$)'
```

Works for pages that no longer exist. URLs whose captures share a content
digest are fetched once, and `--archive-body-limit` counts distinct bodies.

### Expand API Specifications
```bash
# expand every route in the collected OpenAPI, Swagger and GraphQL documents
urx example.com --expand-specs

# fewer documents (default 50), paced
urx example.com --expand-specs --max-spec-files 10 --rate-limit 2

# use the archived copy when the live host no longer serves it
urx example.com --archive-body --expand-specs
```

JSON and YAML are both read. Filters and presets also apply to the expanded
routes, so `--preset only-api` would drop `/users` or `/orders/{id}`. No preset
is needed to find the documents.

### Record Response Titles
```bash
# --check-title implies --check-status; --show-meta prints the response-head fields
urx example.com --check-title --show-meta

# response-head fields as JSON Lines
urx example.com --check-status -f jsonl | jq -r '[.url, .status, .content_type] | @tsv'
```

### Status Filtering
```bash
urx example.com --check-status --include-status 200
urx example.com --check-status --include-status 200,30x
urx example.com --check-status --exclude-status 404,50x
```

With both set, `--include-status` decides and `--exclude-status` is ignored. A
URL whose check failed outright (refused, timed out) prints as
`[Status check failed]` unless `--include-status` is set.

## Network Configuration

```bash
urx example.com --proxy http://localhost:8080
urx example.com --proxy http://localhost:8080 --proxy-auth username:password
urx example.com --timeout 60 --parallel 10 --retries 5
urx example.com --insecure
urx example.com --random-agent
```

### Authenticated and Custom Requests
Headers go only to the target (status checks, extractors, robots/sitemap), never
to the archives:
```bash
urx example.com --check-status \
  -H "Authorization: Bearer $TOKEN" \
  --cookie "session=abc" \
  --user-agent "acme-security-scan/1.0"
```

### Rate Limits and Time Budget
```bash
# 10 req/s per provider, 1 req/s for VirusTotal, stop collecting after 10 minutes
urx example.com --rate-limit 10 --rate-limit-by vt=1 --max-time 600
```

## URL Normalization

```bash
urx example.com --normalize-url
urx example.com --normalize-url --merge-endpoint

# only hosts, paths or query strings
urx example.com --subs --show-only-host
urx example.com --show-only-path
urx example.com --show-only-param

# keep URLs on any host a provider returned (a path-scoped target still applies)
urx example.com --no-strict
```

## Caching & Incremental Scanning

```bash
# SQLite (default)
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db

# Redis; needs a build with: cargo install urx --features redis-cache
urx example.com --cache-type redis --redis-url redis://localhost:6379

# TTL of 12 hours (default 24)
urx example.com --cache-ttl 43200

# no cache
urx example.com --no-cache
```

### Incremental Mode
```bash
# only URLs not seen before
urx example.com --incremental

# post to Slack only when something new turned up
urx example.com --incremental --silent --notify "$SLACK_HOOK" --notify-format slack
```

### Inspect and Prune the Cache
```bash
urx cache stats
urx cache list --domain '*.example.com'

# rescan one target from scratch
urx cache drop example.com

urx cache prune
urx cache clear --yes
```

## Pipeline Integration

```bash
echo "example.com" | urx | grep "login" > targets.txt
urx example.com | httpx -silent
urx example.com | gf xss
urx example.com -e js | nuclei -t xss
```

More in [Integration](/guide/integration/).

## Complex Scenarios

### Complete Bug Bounty Workflow
```bash
urx target.com \
  --subs \
  -e js,json,xml \
  --patterns api,v1,v2,admin,panel \
  --exclude-patterns cdn,static \
  --check-status \
  --include-status 200,30x \
  --incremental \
  --parallel 15 \
  -o results.txt
```

### API Endpoint Discovery
```bash
urx example.com \
  --patterns api,graphql,rest,v1,v2,v3 \
  -e json,xml \
  --exclude-patterns test,staging \
  -f json \
  -o api-endpoints.json
```

### API Surface from Specifications
```bash
urx target.com \
  --subs \
  --expand-specs \
  --max-spec-files 25 \
  --check-status \
  --include-status 200 \
  -f jsonl \
  -o api-surface.jsonl
```

Expanded routes are not status-checked. `--include-status 200` keeps only the
collected URLs (including the spec documents) that returned 200; every expanded
route is added unchecked, path templates like `/orders/{id}` included.

### JavaScript Analysis Pipeline
```bash
urx target.com \
  -p only-js \
  --check-status \
  --include-status 200 \
  -f jsonl \
  | jq -r .url \
  | tee js-files.txt \
  | nuclei -t exposures/
```
