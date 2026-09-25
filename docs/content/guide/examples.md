+++
title = "Examples"
description = "Worked commands for filtering, provider selection, API keys, link extraction, status checking, streaming and caching."
toc = true
weight = 3
+++

## Usage Examples

### Basic Usage

#### Single Domain
```bash
urx example.com
```

#### Multiple Domains
```bash
urx example.com example.org
```

#### From Standard Input
```bash
cat domains.txt | urx
```

#### From a Domain List
```bash
# Repeatable; merged with any domains on the command line
urx --domain-list domains.txt
urx --dL program-a.txt --dL program-b.txt
```

#### Path-Scoped Target
```bash
# Only URLs under /shop on example.com
urx example.com/shop
```

#### File Input
```bash
# Single file
urx --files urls.txt

# Multiple files (WARC, compressed, text)
urx --files urls.txt archive.warc data.gz
urx --files urls.txt --files archive.warc

# With filters
urx --files data.txt --patterns api,admin -f json
```

## Output Formats

### Save to File
```bash
urx example.com -o results.txt
```

### JSON Format
```bash
urx example.com -f json -o results.json
```

### JSON Lines Format
```bash
# One JSON object per line, easy to process with jq
urx example.com -f jsonl | jq -r '.url'
```

### CSV Format
```bash
urx example.com -f csv -o results.csv
```

### One File per Domain
```bash
# out/example.com.json, out/example.org.json, ... plus the combined file
urx example.com example.org -f json --output-dir out/ -o all.json
```

### Streaming Output
```bash
# Write URLs as each provider reports them, so the next tool starts at once
urx example.com --stream | httpx -silent
```

`--stream` output is unsorted, bypasses the cache, and supports `plain`, `jsonl`
and `csv`. Options that need the complete result set (`--incremental`,
`--check-status` / `--include-status` / `--exclude-status` / `--check-title`,
the extractors, `--archive-body`, `--expand-specs`, `--merge-endpoint`, `--dedup-similar`,
`--show-sources`, `--show-meta`, `--output-dir`, the parameter views, the
`--meta-*` filters, `--files`) are rejected with it.

### Provenance and Run Statistics
```bash
# Which providers returned each URL
urx example.com --show-sources

# Archive capture metadata in plain text
urx example.com --providers wayback --show-meta

# Per-provider URL counts, errors and timings on stderr
urx example.com --stats
```

### Wordlist
Every path segment and parameter name the run saw, deduplicated and sorted —
segments that look like ids, hashes or dates are left out:
```bash
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ
```

### Parameter Inventory
```bash
# Every query parameter name the target uses, once each
urx example.com --params

# ...grouped by the endpoint that takes them
urx example.com --params-by-endpoint

# One URL per parameter signature, values replaced — feed it straight to a fuzzer
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ
```

## Filtering Examples

### Include Specific Extensions
```bash
# JavaScript files only
urx example.com -e js

# Multiple extensions
urx example.com -e js,php,aspx
```

### Exclude Extensions
```bash
urx example.com --exclude-extensions html,txt
```

### Pattern Matching
```bash
# Include patterns
urx example.com --patterns api,v1,graphql

# Exclude patterns
urx example.com --exclude-patterns static,images

# Combined
urx example.com --patterns api --exclude-patterns test,dev
```

### Using Presets
```bash
# Exclude images
urx example.com -p no-images

# Exclude all resources
urx example.com -p no-resources

# JavaScript files only
urx example.com -p only-js
```

### Scope Files
Keep a bug bounty program's own scope list as the filter, `!` lines and all:
```bash
# scope.txt
#   *.example.com
#   !admin.example.com
urx example.com --subs --scope-file scope.txt

# Several programs at once; the files are unioned, exclusions always win
urx --domain-list targets.txt --subs --scope-file scope-a.txt --scope-file scope-b.txt
```

### Archive Metadata Filtering
Filter on when a URL was archived and what the archive recorded, after
collection, so it applies to every provider uniformly:
```bash
# Endpoints still being captured recently, with images and HTML out of the way
urx example.com --providers wayback --meta-last-seen-after 2024 --meta-exclude-mime 'text/html,image/*'

# Pages that died: nothing captured since 2019
urx example.com --providers wayback --meta-last-seen-before 2019

# JSON the archive served successfully
urx example.com --providers wayback --meta-mime application/json --meta-status 200

# First archived during 2020
urx example.com --providers wayback --meta-first-seen-after 2020 --meta-first-seen-before 2020
```

### Regular Expressions
```bash
# Keep versioned API paths (case-insensitive); repeat the flag to OR patterns
urx example.com --match-regex '(?i)/api/v[0-9]+/'

# Drop static asset directories
urx example.com --filter-regex '/(assets|static)/'
```

### Collapsing Near-Duplicates
```bash
# /post/1, /post/2 and /post/99999 become one line
urx example.com --dedup-similar
```

### Archive-Side Filters
Applied by the CDX index itself (wayback, cc, arquivo, `--cdx-endpoint`), so they
cost no extra requests and shrink what is fetched:
```bash
# Captures from 2023 onwards that the archive recorded as 200
urx example.com --from 2023 --archive-status 200

# Endpoints served as JSON, even without a .json extension
urx example.com --archive-mime application/json

# Skip error pages and HTML
urx example.com --archive-exclude-status 404,500 --archive-exclude-mime text/html
```

### Advanced Filtering
```bash
# Multiple filters
urx example.com -e js,php --patterns admin,login --min-length 20

# URL length constraints
urx example.com --min-length 50 --max-length 200
```

## Provider Selection

### Specific Providers
```bash
# Wayback Machine and OTX (plus the live robots.txt / sitemap.xml probes, and
# any keyed provider whose URX_*_API_KEY is set; --exclude-providers keeps one out)
urx example.com --providers wayback,otx

# Keyless sources (no API keys needed) — incl. Arquivo.pt and anonymous URLScan.
# Same caveat: a set URX_*_API_KEY still adds its provider
urx example.com --providers wayback,cc,otx,arquivo,urlscan

# All available providers (vt, zoomeye, github and bevigil need their
# URX_*_API_KEY or --*-api-key set, or they report an error)
urx example.com --providers wayback,cc,otx,arquivo,vt,urlscan,zoomeye,github,bevigil

# Or enable everything at once (keyed providers activate only when a key is present)
urx example.com --all-providers

# Everything except OTX
urx example.com --all-providers --exclude-providers otx

# Add any other CDX index server (here the Icelandic web archive) — id cdx:vefsafn.is
urx example.is --cdx-endpoint https://vefsafn.is/cdx --rate-limit-by cdx:vefsafn.is=1

# What is available, and which providers need a key
urx --list-providers

# Query two specific Common Crawl indexes in parallel
urx example.com --providers cc --cc-index CC-MAIN-2026-17,CC-MAIN-2025-51
```

### Using a Config File
```bash
# Load a profile; the provider-config file holds only the API keys
urx -c ~/.config/urx/bugbounty.toml --provider-config ~/.config/urx/keys.toml example.com
```

See [Configuration](/guide/configuration/) for every key.

### With API Keys

#### Command Line
```bash
urx example.com --vt-api-key=YOUR_KEY --urlscan-api-key=YOUR_KEY
urx example.com --zoomeye-api-key=YOUR_KEY --providers zoomeye
```

#### Environment Variables
```bash
export URX_VT_API_KEY=YOUR_KEY
export URX_URLSCAN_API_KEY=YOUR_KEY
export URX_ZOOMEYE_API_KEY=YOUR_KEY
export URX_GITHUB_API_KEY=YOUR_TOKEN
export URX_BEVIGIL_API_KEY=YOUR_KEY
urx example.com --providers=vt,urlscan,zoomeye,github,bevigil
```

#### API Key Rotation
```bash
# Multiple keys for rate limit distribution
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3

# Or with environment variable (comma-separated)
URX_VT_API_KEY=key1,key2,key3 urx example.com
```

### ZoomEye Provider
```bash
# Basic ZoomEye usage
urx example.com --zoomeye-api-key YOUR_KEY --providers zoomeye

# With subdomains
urx example.com --zoomeye-api-key YOUR_KEY --providers zoomeye --subs

# Auto-enabled when key is provided
export URX_ZOOMEYE_API_KEY=YOUR_KEY
urx example.com
```

## Discovery Options

### Exclude Discovery Features
```bash
# Exclude robots.txt
urx example.com --exclude-robots

# Exclude sitemap.xml
urx example.com --exclude-sitemap

# Exclude both
urx example.com --exclude-robots --exclude-sitemap
```

### Archived robots.txt and sitemap.xml
```bash
# Every distinct archived version, alongside the live files
urx example.com --archived-discovery

# See which URLs came from an old version
urx example.com --archived-discovery --show-sources

# Only the versions captured in a given era, robots.txt only
urx example.com --archived-discovery --from 2014 --to 2016 --exclude-sitemap

# Cap the documents fetched per domain (newest versions first)
urx example.com --archived-discovery --archived-discovery-limit 10
```

## Testing & Validation

### Include Subdomains
```bash
urx example.com --subs
```

### Check HTTP Status
```bash
urx example.com --check-status
```

### Extract Links
```bash
urx example.com --extract-links
```

`--extract-links` re-fetches every surviving URL and mines the HTML for more.
It reads every URL-bearing tag, not just anchors: `<a href>`, `<script src>`,
`<link href>`, `<form action>`, `<iframe src>`, `<img src>`, `<source src>`,
`<object data>`, `<embed src>`, and `<meta http-equiv="refresh">` targets.
Relative URLs resolve against the page (honouring `<base href>`), duplicates
are collapsed, and the discovered links go through exactly the same filters,
host validation, and output transforms as the rest of the run. Those filters
also run on the collected URLs *before* any page is fetched, so a filter such as
`-e js` would remove the HTML pages there is nothing to extract from. Filter the
output instead:

```bash
# Only the JavaScript the pages reference
urx example.com --extract-links | grep -E '\.js(\?|$)'
```

### Extract Endpoints from JavaScript
```bash
urx example.com --extract-js-endpoints
```

`--extract-js-endpoints` fetches every collected URL that looks like a script
and mines its string literals for the paths and URLs the app calls:
`fetch("/api/v2/users")`, `axios.post("/graphql")`, the static prefix of
`` `/api/orders/${id}` ``, ES-module chunk imports. These are the endpoints
that never appear in HTML. Output is heavily de-noised (MIME types, module
specifiers, base64 payloads, CSS values, regex fragments and the like are
dropped — see the [CLI options guide](/guide/cli-options/#javascript-endpoint-extraction)
for the full policy), bodies are capped at 10 MiB, the number of files fetched
is bounded by `--max-js-files`, and the discovered endpoints go through the
same filters and host validation as the rest of the run.

```bash
# Bound the number of scripts fetched
urx example.com --extract-js-endpoints --max-js-files 100

# The extractors all run over the collected URLs in one pass, so scripts that
# --extract-links discovers are not mined in the same run. Chain two runs:
urx example.com --extract-links | grep -E '\.js(\?|$)' > bundles.txt
urx --files bundles.txt --extract-js-endpoints --max-js-files 100

# Keep the API-looking paths (filter the output: --patterns would drop the
# bundles before they are read)
urx example.com --extract-js-endpoints | grep -E 'api|graphql'
```

Discovered endpoints are not status-checked; `--check-status` covers the
collected URLs only.

Leave `-e js` off when using this option: discovered endpoints pass through
your filters too, so `-e js` would keep only the `.js` files it found rather
than the API paths.

### Mine Archived Bodies
```bash
urx example.com --archive-body
```

`--archive-body` runs the same extraction over the bodies the Wayback Machine
*stored*, so pages that no longer exist still give up the links they contained.
URLs whose captures share a content digest are fetched once, so the run costs
one request per distinct body rather than one per URL; `--archive-body-limit`
(default 500) bounds those distinct bodies:

```bash
# Bounded and paced
urx example.com --archive-body --archive-body-limit 200 --rate-limit 5

# Only what the archived pages referenced as JavaScript
urx example.com --archive-body --no-cache | grep -E '\.js(\?|$)'
```

### Expand API Specifications
```bash
urx example.com --preset only-api --expand-specs
```

`--expand-specs` opens the OpenAPI, Swagger and GraphQL documents the run
collected and expands every route they describe into the result set — one
request buys the whole documented surface. JSON and YAML are both read:

```bash
# Bounded and paced
urx example.com --expand-specs --max-spec-files 10 --rate-limit 2

# Read the archived copy instead, for an API the live host no longer serves
urx example.com --archive-body --expand-specs
```

### Record Response Titles
```bash
# --check-title implies --check-status; --show-meta adds the response-head fields
urx example.com --check-title --show-meta

# Everything the response head carried, as JSON Lines
urx example.com --check-status -f jsonl | jq -r '[.url, .status, .content_type] | @tsv'
```

### Status Filtering
```bash
# Include only successful responses
urx example.com --check-status --include-status 200

# Include redirects and success
urx example.com --check-status --include-status 200,30x

# Exclude errors
urx example.com --check-status --exclude-status 404,50x
```

## Network Configuration

### Proxy Usage
```bash
urx example.com --proxy http://localhost:8080
```

### Proxy with Authentication
```bash
urx example.com --proxy http://localhost:8080 --proxy-auth username:password
```

### Custom Timeouts and Parallelism
```bash
urx example.com --timeout 60 --parallel 10
```

### Skip SSL Verification
```bash
urx example.com --insecure
```

### Random User-Agent
```bash
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

### Complete Network Configuration
```bash
urx example.com \
  --proxy http://localhost:8080 \
  --timeout 60 \
  --parallel 10 \
  --retries 5 \
  --insecure \
  --random-agent
```

## URL Normalization

### Basic Normalization
```bash
urx example.com --normalize-url
```

### With Endpoint Merging
```bash
urx example.com --normalize-url --merge-endpoint
```

### Show Only One Part of Each URL
```bash
# Hosts (handy with --subs), paths, or query strings
urx example.com --subs --show-only-host
urx example.com --show-only-path
urx example.com --show-only-param
```

### Off-Host URLs
```bash
# Keep URLs on any host a provider returned (a path-scoped target still applies)
urx example.com --no-strict
```

## Caching & Incremental Scanning

### SQLite Cache (Default)
```bash
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db
```

### Redis Cache
```bash
# Requires a build with: cargo install urx --features redis-cache
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

### Incremental Mode
```bash
# Only return new URLs not seen before
urx example.com --incremental

# ...and post a summary to Slack only when something new turned up
urx example.com --incremental --silent --notify "$SLACK_HOOK" --notify-format slack
```

### Custom TTL
```bash
# Set cache TTL to 12 hours
urx example.com --cache-ttl 43200
```

### Disable Cache
```bash
urx example.com --no-cache
```

### Inspect and Prune the Cache
```bash
# What is in there
urx cache stats
urx cache list --domain '*.example.com'

# Rescan one target from scratch, leaving the rest of the cache alone
urx cache drop example.com

# Housekeeping
urx cache prune
urx cache clear --yes
```

## Pipeline Integration

### Filter with grep
```bash
echo "example.com" | urx | grep "login" > targets.txt
```

### Chain with Other Tools
```bash
cat domains.txt | urx --patterns api | other-tool
```

### Security Tool Integration
```bash
# With Nuclei for vulnerability scanning
urx example.com -e js | nuclei -t xss

# With httpx for HTTP probing
urx example.com | httpx -silent

# With gf patterns
urx example.com | gf xss
```

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
  --preset only-api \
  --expand-specs \
  --max-spec-files 25 \
  --check-status \
  --include-status 200 \
  -f jsonl \
  -o api-surface.jsonl
```

### Parameter Discovery for Fuzzing
```bash
# What does this target take, and where?
urx target.com --subs --params-by-endpoint -o params-by-endpoint.txt

# Turn the same run into ffuf input
urx target.com --subs --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ
```

### JavaScript Analysis Pipeline
```bash
urx target.com \
  -p only-js \
  --check-status \
  --include-status 200 \
  | tee js-files.txt \
  | nuclei -t exposures/
```
