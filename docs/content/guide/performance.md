+++
title = "Performance"
description = "Tune throughput with parallelism, network settings, provider choice, archive-side filtering and streaming."
toc = true
weight = 7
+++

## Optimizing Urx Performance

All selected providers always run concurrently. The settings below control how
much work each one does, how patiently it waits, and how fast results reach you.

Defaults worth knowing: `--timeout 120`, `--retries 2`, `--parallel 5`, no
`--rate-limit`, no `--max-time`.

### Parallel Processing

#### Adjust Parallelism
```bash
# Default: 5 domains per provider at once
urx --domain-list domains.txt

# More domains in flight per provider
urx --domain-list domains.txt --parallel 20

# Gentler on rate-limited providers and targets
urx --domain-list domains.txt --parallel 2
```

`--parallel` (default 5) bounds how many domains each provider fetches
concurrently, and how many URLs `--check-status` and the extractors request at
once. It matters for multi-domain runs and for the testers; for a single target's
collection it changes nothing, because each provider has only one domain to
fetch. A provider's `--rate-limit` is shared across its concurrent domains, so
raising `--parallel` never exceeds the configured rate.

**Recommendations:**
- Fast connections: `--parallel 15-20`
- Normal connections: `--parallel 5-10`
- Slow/rate-limited: `--parallel 2-3`

### Network Optimization

#### Timeout Configuration
```bash
# Fast per-request timeout for quick scans
urx example.com --timeout 15

# Cap the whole collection phase at 5 minutes, keeping what arrived in time
urx example.com --max-time 300
```

`--timeout` (default 120 seconds) applies to each request. `--max-time` is the
ceiling on the whole provider phase: when it elapses, in-flight fetches are
stopped and urx continues with the URLs collected so far. `--stats` counts the
fetches that were cut short in its `partial` column. A cut-short result is
cached like any other.

#### Retry Settings
```bash
# Fewer retries for speed
urx example.com --retries 1

# More retries for reliability
urx example.com --retries 5
```

`--retries` (default 2) re-sends a request only after a network error or an HTTP
`408`, `429` or `5xx`, with a growing back-off between attempts. A `Retry-After`
header from the server is honoured (up to 60 seconds).

#### Network Scope
`--network-scope` decides which components get the network settings: timeout,
retries, proxy, `--insecure`, `--random-agent`, the rate limit, and (for the
testers) the `-H` / `--cookie` / `--user-agent` headers. It does not change
`--parallel`.

```bash
# Apply to all components (default)
urx example.com --network-scope all --proxy http://127.0.0.1:8080

# Only the providers go through the proxy
urx example.com --network-scope providers --proxy http://127.0.0.1:8080

# Only the testers (status checks, extractors) go through the proxy
urx example.com --network-scope testers --check-status --proxy http://127.0.0.1:8080
```

### Provider Selection

#### Choose Fast Providers
```bash
# Only the Wayback Machine (plus the robots.txt / sitemap.xml probes)
urx example.com --providers wayback

# Drop one provider from the default set
urx example.com --exclude-providers otx

# Nothing but the archive: no live requests to the target
urx example.com --providers wayback --exclude-robots --exclude-sitemap
```

`--providers wayback` is not quite "one provider": the `robots` and `sitemap`
probes of the target run by default, and a keyed provider joins automatically
whenever its `URX_*_API_KEY` variable is set. Use `--exclude-robots`,
`--exclude-sitemap` or `--exclude-providers` to keep them out.

#### Rate Limits
```bash
# At most 10 requests per second per provider
urx example.com --rate-limit 10

# Per-provider overrides; others fall back to --rate-limit
urx example.com --rate-limit-by vt=1,wayback=10
```

The rate applies per provider. It also paces `--extract-js-endpoints`,
`--expand-specs` and `--archive-body`.

#### API Key Rotation
Distribute load across multiple API keys to bypass rate limits:
```bash
urx example.com \
  --vt-api-key=key1 \
  --vt-api-key=key2 \
  --vt-api-key=key3 \
  --providers vt
```

### Filtering Early

Most filters (`-e`, `--patterns`, `-p`, the regex filters) run after collection,
so they cut the output and the work the testers do, but not the amount fetched.
The filters that shrink the fetch itself are the ones the archive applies on its
side:

```bash
# Only captures from a date range (wayback, cc, arquivo, --cdx-endpoint)
urx example.com --from 2023 --to 2024

# Only what the archive recorded as 200 / as JSON
urx example.com --archive-status 200 --archive-mime application/json

# A path-scoped target only asks for that path
urx example.com/api
```

Filtering in urx rather than piping to `grep` still pays off when testers run,
since they then request only the URLs that survive:

```bash
# Status checks only for the JavaScript files that mention "api"
urx example.com -e js --patterns api --check-status
```

#### Use Presets
```bash
# Exclude a whole family of static files by name
urx example.com -p no-images,no-resources

# Or pick exactly the extensions to drop
urx example.com --exclude-extensions jpg,png,gif,css,woff,woff2,ttf
```

Presets and `--exclude-extensions` go through the same filter; presets are a
shorthand, not a faster path.

### Output Optimization

```bash
# Write results to a file with no terminal output
urx example.com --silent -o results.txt

# Start processing results as soon as each provider reports them
urx example.com --stream | httpx -silent
```

The progress bar is drawn on stderr and hidden automatically when stderr is not
a terminal, so scripts need no `--no-progress`. `--silent` suppresses **all**
output, including results on stdout, so use it only together with `-o` or
`--notify`.

Batch output is held in memory until the run ends, whether it goes to `-o` or to
stdout. `--stream` writes each URL as it arrives and skips the in-memory result
set; it is unsorted, bypasses the cache, supports only `plain`, `jsonl` and
`csv`, and rejects options that need the complete result set (see
[CLI Options](/guide/cli-options/)).

### Batch Processing

```bash
# Many domains in one run; urx fetches them concurrently itself
urx --domain-list domains.txt --no-progress -o results.txt

# One output file per domain
urx --domain-list domains.txt --incremental --output-dir out/

# Lowest memory: stream the results
urx --domain-list domains.txt --stream -o results.txt
```

## Best Practices by Use Case

### Rapid Testing
```bash
urx example.com \
  --providers wayback \
  --exclude-robots --exclude-sitemap \
  --timeout 15 \
  --retries 1 \
  --max-time 120
```

### Production Monitoring
```bash
urx example.com \
  --incremental \
  --cache-ttl 172800 \
  --timeout 60 \
  --retries 3 \
  --silent \
  --notify "$URX_HOOK" --notify-format slack
```

For a cache shared across machines, add `--cache-type redis --redis-url …`
(needs a `redis-cache` build; see [Caching](/guide/caching/)).

### Comprehensive Discovery
```bash
urx example.com \
  --providers wayback,cc,otx,arquivo,vt,urlscan,zoomeye \
  --subs \
  --timeout 120 \
  --retries 5 \
  --incremental
```

### Resource-Constrained Environment
```bash
urx example.com \
  --providers wayback \
  --exclude-robots --exclude-sitemap \
  --timeout 30 \
  --no-cache \
  --stream
```

## Troubleshooting Performance Issues

### Slow Scans
1. Re-runs within `--cache-ttl` are served from the cache automatically (don't
   pass `--no-cache`); raise `--cache-ttl` to reuse results longer
2. Cap the run with `--max-time`
3. Reduce `--timeout` if appropriate
4. Select fewer providers, or use `--exclude-providers`
5. Narrow the fetch with `--from` / `--to`, the `--archive-*` filters or a
   path-scoped target
6. For many domains, increase `--parallel`

### High Memory Usage
1. Use `--stream`, which skips the in-memory result set
2. Decrease `--parallel` value
3. Process fewer domains per run
4. Narrow the fetch with `--from` / `--to` or the `--archive-*` filters

### Rate Limiting
1. Use `--rate-limit`, or `--rate-limit-by` for one provider
2. Reduce `--parallel` value
3. Implement API key rotation
4. Increase `--timeout` value

### Network Timeouts
1. Increase `--timeout` value
2. Increase `--retries` value
3. Check network connectivity
4. Try different providers
