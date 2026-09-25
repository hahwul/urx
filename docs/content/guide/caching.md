+++
title = "Caching"
description = "Reuse recent scans from the SQLite or Redis cache, return only new URLs with incremental mode, and inspect or prune the cache."
toc = true
weight = 5
+++

## Caching & Incremental Scanning

Urx includes a built-in caching system that stores the URLs each scan
collected, so a repeated scan can be answered without querying the providers
again, and an incremental scan can report only what is new.

### How It Works

Caching is **on by default** (turn it off with `--no-cache`). After a scan, urx
stores the URLs collected for each domain in a local (SQLite) or remote (Redis)
cache. What happens on the next run depends on the mode:

- **Normal run.** If the domain has an entry younger than `--cache-ttl`, urx
  returns the cached URLs and skips the providers for that domain entirely.
  Otherwise it fetches fresh results and replaces the entry.
- **`--incremental` run.** urx always fetches fresh results, compares them
  against the stored set, prints only the URLs the previous run had not seen,
  and then stores the full fresh set as the baseline for next time.

A cache entry belongs to one domain *and* one configuration. The key covers the
domain (or path-scoped target), the effective provider list, and the options that
change what is collected: `--subs`, `-e` / `--exclude-extensions`, `--patterns` /
`--exclude-patterns`, `--match-regex` / `--filter-regex`, `-p`, `--min-length` /
`--max-length`, `--strict`, `--normalize-url`, `--merge-endpoint`,
`--dedup-similar`, `--cc-index`, `--from` / `--to`, the `--archive-*` filters and
`--archived-discovery`. Change any of these and the run starts a new baseline,
so an incremental scan re-reports everything once. Setting a `URX_*_API_KEY`
variable also changes the provider list (the provider joins automatically), and
with it the key. `--incremental` itself is not part of the key: a normal run
refreshes the same baseline an incremental run compares against.

### Incremental Scanning

```bash
# First scan — builds the cache and outputs all URLs
urx example.com --incremental -o initial.txt

# Subsequent scans — only new URLs since the last run
urx example.com --incremental -o new-urls.txt
```

**Benefits:**
- Output contains only URLs the previous run for the same configuration had not
  seen, so a monitoring job reports changes rather than the whole attack surface
- Pairs with `--notify --notify-on new` to alert only when something turned up

Every provider is still queried on each incremental run; it saves reading, not
fetching.

### SQLite Cache (Default)

SQLite is the default backend, storing the cache in a local database file.

The default database is `$HOME/.urx/cache.db` (`./.urx/cache.db` when `HOME`
is not set).

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

> Redis support is an optional Cargo feature that the packaged builds (crates.io,
> Homebrew, AUR, release binaries and the Docker image) leave out. Without it,
> `--cache-type redis` fails with `Redis cache support not compiled in`. Build
> with `cargo install urx --features redis-cache` to enable it.

Machines sharing one Redis cache only share entries when they run with the same
flags and the same provider API keys, since both are part of the cache key.

```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

**Best for:**
- Team environments
- Distributed scanning across multiple machines
- Kubernetes/container deployments
- High-performance scenarios

### Cache TTL

The time-to-live (TTL) controls how long an entry is served in place of a fresh
fetch. It also drives cleanup: at the end of every scan, urx deletes every entry
older than **twice** the TTL, for any domain, and `urx cache prune` deletes
entries older than the TTL. An incremental baseline that ages out this way is
gone, and the next incremental run reports everything again, so set the TTL
comfortably above your scan interval.

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

With `--no-cache`, `--incremental` has no baseline to compare against and is
ignored: every URL is printed.

### What the Cache Does and Does Not Hold

- The cache stores the host-validated URLs the providers returned, before
  filters run. Filters and output views are applied afterwards, on hits and
  misses alike.
- It stores URLs only. A cache hit carries no provider attribution and no archive
  capture metadata, so on a hit `--show-sources` has nothing to show, JSON/CSV
  lack `first_seen` / `last_seen` / `mime` / `archive_status` / `digest`, the
  `--meta-*` filters drop everything, `--archive-body` has nothing to replay and
  `--stats` stays empty. Add `--no-cache` when you need those.
- `--stream` and `--files` bypass the cache.
- A run cut short by `--max-time`, Ctrl-C or provider errors is cached like any
  other, so its partial result is what the next run within the TTL gets.
- A cache backend that cannot be opened (an unreachable Redis server, say) is a
  fatal error rather than a silent fallback.

### Inspecting and Maintaining the Cache

`urx cache` is the operator-facing view of the cache. Every subcommand honours
the same `--cache-type`, `--cache-path`, `--redis-url` and `--cache-ttl` a scan
uses, so what it reports is what a scan would actually see, and all five work
against both backends.

| Command | What it does |
|---------|--------------|
| `urx cache stats` | Entries, domains, URLs, age span, size (the database file for SQLite, stored bytes for Redis), expired count |
| `urx cache list [--domain PAT]` | Per-domain entry and URL counts, last scan time, TTL remaining |
| `urx cache prune` | Delete only what `--cache-ttl` has expired |
| `urx cache drop <DOMAIN>...` | Delete every entry for the given domains |
| `urx cache clear [-y\|--yes]` | Delete everything, confirming first |

```console
$ urx cache stats
Cache:    sqlite
Location: /home/you/.urx/cache.db
Size:     4.2 MiB (database file)

Entries:  312
Domains:  27
URLs:     184,905
Expired:  41  (--cache-ttl 86400s = 1d 0h)

Oldest:   2026-09-08T19:02:44Z  (1d 20h ago)
Newest:   2026-09-09T22:41:02Z  (16h 12m ago)

$ urx cache list --domain '*.example.com'
DOMAIN            ENTRIES  EXPIRED       URLS  LAST SCAN             TTL LEFT
----------------  -------  -------  ---------  --------------------  --------
api.example.com         2        0     12,884  2026-09-09T22:41:02Z  7h 47m
shop.example.com        1        1      3,201  2026-09-08T19:02:44Z  expired
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

**Machine-readable output.** `-f json` switches every subcommand to JSON (`-f
jsonl` prints the same document), so cache state can be monitored the same way a
scan is:

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
- The `urx cache` subcommands sweep Redis with `SCAN` rather than `KEYS`, which
  would block a shared server for the whole sweep, and redact any password in
  `--redis-url` before printing the location. Keys are `urx:cache:<sha256>` and
  `urx:meta:<sha256>`; urx manages expiry itself and sets no Redis `EXPIRE`.
- `urx cache` reads `-c` / `--config` and the `[cache]` section like a scan does,
  and ignores `--no-cache`.

### Combined Examples

```bash
# Daily monitoring with incremental updates, alerting a webhook only when
# something new turned up (see --notify in CLI Options)
urx example.com --incremental --silent --notify "$URX_HOOK" --notify-format slack

# Daily monitoring with incremental updates (--silent would suppress stdout,
# so hide only the progress bar)
urx target.com --incremental --no-progress | notify-tool

# Distributed scanning with shared Redis cache (needs a redis-cache build)
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
cache_type = "sqlite"                     # or "redis"
cache_path = "/home/me/.urx/cache.db"     # `~` is not expanded; omit for the default
# redis_url = "redis://localhost:6379"
cache_ttl = 86400
no_cache = false
```
