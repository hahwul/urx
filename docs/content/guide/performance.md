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

`--retries` (default 2) is the number of extra attempts after the first:

- The archive indexes (wayback, cc, arquivo, `--cdx-endpoint`, and the
  archived-discovery listings) retry network errors and HTTP `408`, `429` and
  `5xx`, backing off linearly (0.5 s, 1 s, …) unless the server sends
  `Retry-After` (seconds, capped at 60 s), which replaces the back-off.
- bevigil retries network errors, `429` and `5xx` the same way; a `404` means no
  data, and any other error status fails at once.
- vt, urlscan, zoomeye and github retry any non-2xx answer (except vt's 404 and
  github's 422, which mean no data) with the same back-off. They first wait out
  the `Retry-After` of a 429 (and, for github, of a 403 secondary rate limit).
- otx retries any failure a flat 1 s apart.
- The testers (`--check-status`, the extractors, `--expand-specs`,
  `--archive-body`) retry only network errors, 0.5 s apart, because the HTTP
  status is their answer.
- The live robots.txt / sitemap.xml probes make a single attempt.

#### Network Scope
`--network-scope` decides which components get the network settings: timeout,
retries, proxy, `--insecure`, `--random-agent`, the rate limit, and the `-H` /
`--cookie` / `--user-agent` headers (sent only to the target: the testers and the
robots/sitemap probes). It does not change `--parallel`. Components outside the
scope use their built-in defaults (3 retries, a 30 s timeout — 60 s for wayback,
arquivo and `--cdx-endpoint`, 10 s for cc — the default User-Agent, and any
`HTTP(S)_PROXY` / system proxy). The `--notify` webhook takes only `--proxy`
(with `--proxy-auth`), `--timeout` and `--insecure`, whatever the scope, and makes a single attempt.

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

The rate applies per provider, and may be fractional: `--rate-limit-by vt=0.066`
is about 4 requests a minute, VirusTotal's public-API quota. It also paces
`--extract-js-endpoints`, `--expand-specs` and `--archive-body`.

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

# A path-scoped target: the CDX archives only ask for that path
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
output, including results on stdout (except under `--stream`), so use it only
together with `-o` or `--notify`.

Batch output is held in memory until the run ends, whether it goes to `-o` or to
stdout. `--stream` writes each provider's URLs as soon as it finishes a domain,
and keeps only the set of URLs already written (for de-duplication) instead of
the full result set with sources and metadata. It is unsorted, bypasses the cache, supports only `plain`, `jsonl` and
`csv`, and rejects options that need the complete result set (see
[CLI Options](/guide/cli-options/)).

### Batch Processing

```bash
# Many domains in one run; urx fetches them concurrently itself
urx --domain-list domains.txt --no-progress -o results.txt

# One output file per host (www. and each subdomain get their own)
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

vt and zoomeye need `URX_VT_API_KEY` / `URX_ZOOMEYE_API_KEY` (or the matching
flags); without a key they report an error and are skipped.

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
1. Use `--stream`, which keeps only the written URLs rather than the full result set
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
