+++
title = "Performance"
description = "Tune throughput with parallelism, network settings, provider choice, archive-side filtering and streaming."
toc = true
weight = 7
+++

## Optimizing Urx Performance

Selected providers always run concurrently. Defaults: `--timeout 120`,
`--retries 2`, `--parallel 5`, no `--rate-limit`, no `--max-time`.

### Parallel Processing

```bash
# more domains in flight per provider (default 5)
urx --domain-list domains.txt --parallel 20

# gentler on rate-limited providers and targets
urx --domain-list domains.txt --parallel 2
```

`--parallel` caps how many domains each provider fetches at once, and how many
URLs `--check-status` and the extractors request at once. It has no effect on a
single target's collection. A provider's `--rate-limit` is shared across its
concurrent domains, so raising `--parallel` never exceeds the rate.

### Network Optimization

#### Timeout Configuration
```bash
# per-request timeout
urx example.com --timeout 15

# stop collecting after 5 minutes, keep what arrived
urx example.com --max-time 300
```

When `--max-time` elapses, in-flight fetches stop and urx continues with the
URLs collected so far. `--stats` counts cut-short fetches in its `partial`
column. A cut-short result is cached like any other.

#### Retry Settings
```bash
urx example.com --retries 1
urx example.com --retries 5
```

`--retries` is the number of extra attempts after the first:

| Component | Retries on | Back-off |
|-----------|-----------|----------|
| wayback, cc, arquivo, `--cdx-endpoint`, archived-discovery listings | network errors, `408`, `429`, `5xx` | linear (0.5 s, 1 s, ...); a `Retry-After` (capped at 60 s) replaces it |
| bevigil | network errors, `429`, `5xx` (`404` = no data; other errors fail at once) | same |
| vt, urlscan, zoomeye, github | any non-2xx except vt `404` and github `422` (no data) | same; first waits out the `Retry-After` of a `429` (and github's `403` secondary rate limit) |
| otx | any failure | flat 1 s |
| testers (`--check-status`, extractors, `--expand-specs`, `--archive-body`) | network errors only (the status is the answer) | flat 0.5 s |
| live robots.txt / sitemap.xml | no retry | |

#### Network Scope
`--network-scope` picks which components get the network settings: timeout,
retries, proxy, `--insecure`, `--random-agent`, the rate limit, and the `-H` /
`--cookie` / `--user-agent` headers (target only: testers and robots/sitemap).
It does not change `--parallel`. Components outside the scope use built-in
defaults: 3 retries, a 30 s timeout (60 s for wayback, arquivo and
`--cdx-endpoint`; 10 s for cc), the default User-Agent, and any
`HTTP(S)_PROXY` / system proxy. The `--notify` webhook always uses only
`--proxy` (with `--proxy-auth`), `--timeout` and `--insecure`, and makes one
attempt.

```bash
# all components (default)
urx example.com --network-scope all --proxy http://127.0.0.1:8080

# providers only
urx example.com --network-scope providers --proxy http://127.0.0.1:8080

# testers only (status checks, extractors)
urx example.com --network-scope testers --check-status --proxy http://127.0.0.1:8080
```

### Provider Selection

#### Choose Fast Providers
```bash
# Wayback only (plus the robots.txt / sitemap.xml probes)
urx example.com --providers wayback

# drop one provider from the default set
urx example.com --exclude-providers otx

# archive only, no live requests to the target
urx example.com --providers wayback --exclude-robots --exclude-sitemap
```

A keyed provider also joins whenever its `URX_*_API_KEY` is set; keep it out
with `--exclude-providers`.

#### Rate Limits
```bash
# at most 10 req/s per provider
urx example.com --rate-limit 10

# per-provider overrides; others fall back to --rate-limit
urx example.com --rate-limit-by vt=1,wayback=10
```

Rates may be fractional: `--rate-limit-by vt=0.066` is about 4 requests a
minute, VirusTotal's public-API quota. The rate also paces
`--extract-js-endpoints`, `--expand-specs` and `--archive-body`.

#### API Key Rotation
```bash
# spread requests across several keys
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3 --providers vt
```

### Filtering Early

Most filters (`-e`, `--patterns`, `-p`, the regex filters) run after
collection: they cut output and tester work, not the amount fetched. Only the
archive-side filters shrink the fetch:

```bash
# date range (wayback, cc, arquivo, --cdx-endpoint)
urx example.com --from 2023 --to 2024

# only what the archive recorded as 200 / as JSON
urx example.com --archive-status 200 --archive-mime application/json

# path-scoped target: the CDX archives only ask for that path
urx example.com/api
```

Filtering in urx (instead of `grep`) still helps when testers run, since they
request only the URLs that survive:

```bash
# status checks only for JS files that mention "api"
urx example.com -e js --patterns api --check-status
```

Presets and `--exclude-extensions` use the same filter; a preset is shorthand,
not a faster path:

```bash
urx example.com -p no-images,no-resources
urx example.com --exclude-extensions jpg,png,gif,css,woff,woff2,ttf
```

### Output Optimization

```bash
# file only, nothing on the terminal
urx example.com --silent -o results.txt

# results reach the next tool as each provider reports them
urx example.com --stream | httpx -silent
```

The progress bar is on stderr and hides itself when stderr is not a terminal.
`--silent` hides results on stdout too (except under `--stream`), so pair it
with `-o` or `--notify`.

Batch output is held in memory until the run ends, for `-o` and stdout alike.
`--stream` writes each provider's URLs as it finishes a domain, and keeps only
the set of written URLs (for dedup) instead of the full result set. It is
unsorted, skips the cache, supports only `plain`, `jsonl` and `csv`, and rejects
options that need the full result set (see [CLI Options](/guide/cli-options/)).

### Batch Processing

```bash
# many domains in one run, fetched concurrently
urx --domain-list domains.txt --no-progress -o results.txt

# one output file per host (www. and each subdomain get their own)
urx --domain-list domains.txt --incremental --output-dir out/

# lowest memory
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

For a cache shared across machines, add `--cache-type redis --redis-url ...`
(needs a `redis-cache` build; see [Caching](/guide/caching/)).

### Comprehensive Discovery
```bash
# vt and zoomeye need URX_VT_API_KEY / URX_ZOOMEYE_API_KEY (or the flags);
# without a key they report an error and are skipped
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

| Problem | Try |
|---------|-----|
| Slow scans | Re-run within `--cache-ttl` (served from cache unless `--no-cache`); raise `--cache-ttl`; cap with `--max-time`; fewer providers; `--from` / `--to`, `--archive-*` or a path-scoped target; more `--parallel` for many domains |
| High memory | `--stream`; lower `--parallel`; fewer domains per run; `--from` / `--to` or `--archive-*` |
| Rate limiting | `--rate-limit` or `--rate-limit-by`; lower `--parallel`; key rotation |
| Timeouts | raise `--timeout` and `--retries`; try other providers |
