+++
title = "Examples"
description = "Worked commands for filtering, provider selection, API keys, link extraction and status checking."
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

### CSV Format
```bash
urx example.com -f csv -o results.csv
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
# Only Wayback Machine and OTX
urx example.com --providers wayback,otx

# Keyless archives only (no API keys needed) — incl. Arquivo.pt and anonymous URLScan
urx example.com --providers wayback,cc,otx,arquivo,urlscan

# All available providers (with API keys)
urx example.com --providers wayback,cc,otx,arquivo,vt,urlscan,zoomeye,github,bevigil

# Or enable everything at once (keyed providers activate only when a key is present)
urx example.com --all-providers

# Add any other CDX index server (here the Icelandic web archive) — id cdx:vefsafn.is
urx example.is --cdx-endpoint https://vefsafn.is/cdx --rate-limit-by cdx:vefsafn.is=1
```

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
urx example.com --providers=vt,urlscan,zoomeye
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
host validation, and output transforms as the rest of the run:

```bash
# Only the JavaScript the pages reference
urx example.com --extract-links -e js
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
# Collect the site's bundles with --extract-links, then mine them
urx example.com --extract-links --extract-js-endpoints --max-js-files 100

# Keep the API-looking paths and probe them
urx example.com --extract-js-endpoints --patterns api,graphql --check-status --include-status 200,401,403
```

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
urx example.com --archive-body -e js --no-cache
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
# --check-title implies --check-status; --show-meta shows the extra fields in plain text
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

## Caching & Incremental Scanning

### SQLite Cache (Default)
```bash
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db
```

### Redis Cache
```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

### Incremental Mode
```bash
# Only return new URLs not seen before
urx example.com --incremental
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
  --extract-links \
  | tee js-files.txt \
  | nuclei -t exposures/
```
