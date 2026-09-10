+++
title = "Caching"
description = "Skip domains you already scanned with the SQLite or Redis cache, and return only new URLs with incremental mode."
toc = true
weight = 5
+++

## Caching & Incremental Scanning

Urx includes a built-in caching system that stores previously seen URLs, enabling incremental scanning and faster subsequent runs.

### How It Works

When caching is enabled, Urx stores each discovered URL in a local (SQLite) or remote (Redis) cache. On subsequent runs with `--incremental`, only URLs not already in the cache are returned.

### Incremental Scanning

```bash
# First scan — builds the cache and outputs all URLs
urx example.com --incremental -o initial.txt

# Subsequent scans — only new URLs since the last run
urx example.com --incremental -o new-urls.txt
```

**Benefits:**
- Dramatically faster subsequent scans
- Only fetches and processes new data
- Perfect for continuous monitoring

### SQLite Cache (Default)

SQLite is the default backend, storing the cache in a local database file.

```bash
# Default location
urx example.com --incremental

# Custom location
urx example.com --incremental --cache-path /path/to/cache.db
```

**Best for:**
- Single-machine scanning
- Local development
- Personal projects

### Redis Cache

Redis provides a shared cache accessible from multiple machines.

```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

**Best for:**
- Team environments
- Distributed scanning across multiple machines
- Kubernetes/container deployments
- High-performance scenarios

### Cache TTL

The time-to-live (TTL) controls how long entries stay in the cache before expiring.

```bash
# Short TTL for frequently changing targets (5 minutes)
urx example.com --cache-ttl 300

# Medium TTL for daily scans (12 hours)
urx example.com --cache-ttl 43200

# Long TTL for stable targets (7 days)
urx example.com --cache-ttl 604800
```

Default TTL is 86400 seconds (24 hours).

### Disabling the Cache

```bash
urx example.com --no-cache
```

### Inspecting and Maintaining the Cache

`urx cache` is the operator-facing view of the cache. Every subcommand honours
the same `--cache-type`, `--cache-path`, `--redis-url` and `--cache-ttl` a scan
uses, so what it reports is what a scan would actually see, and all five work
against both backends.

| Command | What it does |
|---------|--------------|
| `urx cache stats` | Entries, domains, URLs, age span, size on disk, expired count |
| `urx cache list [--domain PAT]` | Per-domain entry and URL counts, last scan time, TTL remaining |
| `urx cache prune` | Delete only what `--cache-ttl` has expired |
| `urx cache drop <DOMAIN>...` | Delete every entry for the given domains |
| `urx cache clear [--yes]` | Delete everything, confirming first |

```console
$ urx cache stats
Cache:    sqlite
Location: /home/you/.urx/cache.db
Size:     4.2 MiB (database file)

Entries:  312
Domains:  27
URLs:     184,905
Expired:  41  (--cache-ttl 86400s = 1d 0h)

Oldest:   2026-09-02T11:04:18Z  (8d 3h ago)
Newest:   2026-09-09T22:41:02Z  (16h 12m ago)

$ urx cache list --domain '*.example.com'
DOMAIN            ENTRIES  EXPIRED       URLS  LAST SCAN             TTL LEFT
----------------  -------  -------  ---------  --------------------  --------
api.example.com         2        0     12,884  2026-09-09T22:41:02Z  7h 47m
shop.example.com        1        1      3,201  2026-09-02T11:04:18Z  expired
```

```bash
# Which domains are cached, and how much life is left in each
urx cache list

# Just one program's hosts
urx cache list --domain '*.example.com'

# Rescan one target from scratch without touching the rest of the cache
urx cache drop example.com

# Housekeeping: drop only what has expired
urx cache prune

# A shared Redis cache is addressed the same way
urx cache stats --cache-type redis --redis-url redis://cache-server:6379
```

**Domain patterns.** `list --domain` and `drop` take the same pattern language.
Matching is case-insensitive and **exact** unless the pattern contains `*`,
which stands for any run of characters: `*.example.com` matches subdomains only,
`example.*` matches any TLD, `*example*` matches any domain containing the text.
The exact-by-default rule is deliberate — a substring default would have let
`drop example.com` take out `notexample.com` too.

**Machine-readable output.** `-f json` / `-f jsonl` switch every subcommand to
JSON, so cache state can be monitored the same way a scan is:

```bash
urx cache stats -f json | jq '.expired_entries'
urx cache list -f json | jq -r '.[] | select(.ttl_remaining == null) | .domain'
```

Details worth knowing:

- Looking at the cache does not create one. A first-ever `urx cache stats`
  reports the location as "does not exist yet" rather than leaving behind the
  database it had just called empty.
- `clear` asks before deleting and refuses a non-interactive stdin instead of
  assuming an answer — use `--yes` in a script.
- `drop` names any pattern that matched nothing, so a typo'd domain does not
  look like a successful no-op.
- Redis sweeps with `SCAN` rather than `KEYS`, which would block a shared
  server for the whole sweep, and any password in `--redis-url` is redacted
  before the location is printed.

### Combined Examples

```bash
# Daily monitoring with incremental updates, alerting a webhook only when
# something new turned up (see --notify in CLI Options)
urx example.com --incremental --silent --notify "$URX_HOOK" --notify-format slack

# Daily monitoring with incremental updates
urx target.com --incremental --silent | notify-tool

# Distributed scanning with shared Redis cache
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# Rapid iterations with short cache TTL
urx test-domain.com --cache-ttl 300

# Incremental scan with filtering
urx example.com --incremental -e js,php --patterns api
```

### Configuration File

Caching can also be configured in a [config file](/guide/configuration/):

```toml
[cache]
incremental = true
cache_type = "sqlite"
cache_path = "~/.urx/cache.db"
cache_ttl = 86400
```
