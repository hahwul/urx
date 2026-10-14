+++
title = "Caching"
description = "Reuse recent scans from the SQLite or Redis cache, return only new URLs with incremental mode, and inspect or prune the cache."
toc = true
weight = 5
+++

## Caching & Incremental Scanning

urx caches the URLs each scan collected. A repeat scan can be answered from the cache without querying providers, and an incremental scan reports only new URLs.

```bash
# SQLite is the default backend; name it and its path explicitly
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db

# Redis (needs: cargo install urx --features redis-cache)
urx example.com --cache-type redis --redis-url redis://localhost:6379

# only URLs not seen in the last scan
urx example.com --incremental

# cache TTL of 12 hours
urx example.com --cache-ttl 43200

# no cache
urx example.com --no-cache

# incremental with filters
urx example.com --incremental -e js,php --patterns api

# every option from a config file (the example writes to results.txt, not stdout; review it first)
urx -c example/config.toml example.com
```

### How It Works

Caching is on by default; `--no-cache` turns it off. After a scan, urx stores the URLs collected for each domain in SQLite (local) or Redis (remote).

- Normal run: if the domain has an entry younger than `--cache-ttl`, urx returns the cached URLs and skips the providers for that domain. Otherwise it fetches fresh results and replaces the entry.
- `--incremental` run: urx always fetches fresh results, prints only URLs no earlier run had seen, then adds the fresh set to the baseline. The baseline only grows, so a run cut short never makes the next one re-report URLs.

A cache entry belongs to one domain and one configuration. The key covers the domain (or path-scoped target), the effective provider list, and the options that change what is collected: `--subs`, `-e` / `--exclude-extensions`, `--patterns` / `--exclude-patterns`, `--match-regex` / `--filter-regex`, `-p`, `--min-length` / `--max-length`, `--strict`, `--normalize-url`, `--merge-endpoint`, `--dedup-similar`, `--cc-index`, `--from` / `--to`, the `--archive-*` filters, `--archived-discovery` (and a non-default `--archived-discovery-limit`), and `--cdx-endpoint` / `--cdx-dialect`. Change any of these and the run starts a new baseline, so an incremental scan re-reports everything once. Setting a `URX_*_API_KEY` variable adds that provider to the list, which also changes the key. `--incremental` is not part of the key: a normal run refreshes the same baseline an incremental run compares against.

### Incremental Scanning

```bash
# first scan: builds the cache, outputs all URLs
urx example.com --incremental -o initial.txt

# later scans: only URLs new since the last run
urx example.com --incremental -o new-urls.txt
```

Output holds only URLs the previous run with the same configuration had not seen. Pair it with `--notify --notify-on new` to alert only when something new turns up. Every provider is still queried on each incremental run; it saves reading, not fetching.

### SQLite Cache (Default)

The default database is `$HOME/.urx/cache.db` (`./.urx/cache.db` when `HOME` is not set). Use it for single-machine scanning.

```bash
# default location
urx example.com --incremental

# custom location
urx example.com --incremental --cache-path /path/to/cache.db
```

### Redis Cache

Redis gives a cache shared across machines (teams, distributed or container scanning).

> Redis support is an optional Cargo feature. Packaged builds (crates.io, Homebrew, AUR, release binaries, Docker image) leave it out, and `--cache-type redis` fails with `Redis cache support not compiled in`. Build with `cargo install urx --features redis-cache`.

Machines share entries only when they run with the same flags and the same keyed providers enabled. A keyed provider joins the cache key, but the key's value does not, so different keys for the same provider share entries.

```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

### Cache TTL

The TTL sets how long an entry is served instead of a fresh fetch. Default: 86400 seconds (24 hours).

It also drives cleanup. At the end of every scan, urx deletes every entry older than twice the TTL, for any domain, except entries an `--incremental` run wrote: those baselines are compared against with no TTL, so the sweep keeps them. `urx cache prune` deletes every entry older than the TTL, baselines included, and `urx cache drop` removes one domain's.

The sweep uses the TTL of the run doing it. One run with `--cache-ttl 300` deletes every non-baseline entry older than 10 minutes in that database. Give short-TTL experiments their own `--cache-path`.

```bash
# 5 minutes, in its own cache
urx example.com --cache-ttl 300 --cache-path ~/.urx/short-ttl.db

# 12 hours, for daily scans
urx example.com --cache-ttl 43200

# 7 days, for stable targets
urx example.com --cache-ttl 604800
```

### Disabling the Cache

```bash
urx example.com --no-cache
```

With `--no-cache`, `--incremental` has no baseline and is ignored: every URL is printed.

### What the Cache Does and Does Not Hold

- It stores the host-validated URLs the providers returned, before filters run. Filters and output views apply afterwards, on hits and misses alike. Under `--no-strict` it also keeps the off-host URLs, so a hit returns what the miss did; in a multi-domain run each off-host URL is stored under every domain of that run.
- It stores URLs only: no provider attribution and no archive capture metadata. On a hit, `--show-sources` has nothing to show, JSON/CSV lack `first_seen` / `last_seen` / `mime` / `archive_status` / `digest`, positive `--meta-*` filters drop everything (`--meta-exclude-*` drop nothing), `--archive-body` has nothing to replay, and `--stats` stays empty. Add `--no-cache` when you need those.
- `--stream` and `--files` bypass the cache.
- A normal run does not cache a domain that `--max-time`, Ctrl-C or a provider error left incomplete, nor one that returned no URLs; the next run fetches it again. An `--incremental` run adds whatever it got to the baseline, even a partial set, which is safe because the baseline never shrinks.
- A cache backend that can't be opened (an unreachable Redis server, say) is a fatal error. There is no silent fallback.

### Inspecting and Maintaining the Cache

`urx cache` inspects and maintains the cache. Every subcommand honours the same `--cache-type`, `--cache-path`, `--redis-url` and `--cache-ttl` a scan uses, so it reports what a scan would see. All five work on both backends.

| Command | What it does |
|---------|--------------|
| `urx cache stats` | Entries, domains, URLs, age span, size (database file for SQLite, stored bytes for Redis), expired count |
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
# cached domains and their remaining TTL
urx cache list

# one program's hosts
urx cache list --domain '*.example.com'

# rescan one target from scratch, keep the rest
urx cache drop example.com

# drop only what has expired
urx cache prune

# delete everything without the prompt
urx cache clear --yes

# a shared Redis cache
urx cache stats --cache-type redis --redis-url redis://cache-server:6379
```

Domain patterns: `list --domain` and `drop` match case-insensitively and exactly, unless the pattern contains `*` (any run of characters). `*.example.com` matches subdomains only, `example.*` any TLD, `*example*` any domain containing the text. So `drop example.com` does not touch `notexample.com`.

JSON output: `-f json` switches every subcommand to JSON (`-f jsonl` prints the same document; other `-f` values print plain text).

```bash
urx cache stats -f json | jq '.expired_entries'
urx cache list -f json | jq -r '.[] | select(.ttl_remaining == null) | .domain'
```

Other behaviour:

- Looking at the cache never creates one. A first-ever `urx cache stats` reports the location as "does not exist yet".
- `clear` asks before deleting and refuses a non-interactive stdin. Use `--yes` in scripts.
- `drop` names any pattern that matched nothing, so a typo doesn't look like success.
- On Redis, `urx cache` sweeps with `SCAN` (never the blocking `KEYS`) and redacts any password in `--redis-url` before printing it. Keys are `urx:cache:<sha256>` and `urx:meta:<sha256>`. urx manages expiry itself and sets no Redis `EXPIRE`.
- `urx cache` reads `-c` / `--config` and the `[cache]` section like a scan does, and ignores `no_cache` / `--no-cache`. `--no-cache` is only accepted before `cache`; `--cache-type`, `--cache-path`, `--redis-url`, `--cache-ttl` and `-f` work in either position.

### Combined Examples

```bash
# daily monitoring: webhook fires only when something new turns up
# (see --notify in CLI Options)
urx example.com --incremental --silent --notify "$URX_HOOK" --notify-format slack

# hand new URLs to an external notifier
# (--silent would hide stdout too; --no-progress only hides the progress bar)
urx target.com --incremental --no-progress | notify-tool

# domain lists (keep --cache-ttl above the scan interval: entries older than 2x TTL are swept)
cat domains.txt | urx --incremental --no-progress > new_urls.txt

# shared Redis cache (needs a redis-cache build)
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# fast re-scans: 5-minute TTL in a separate cache, since a short TTL sweeps
# every older entry in the database it runs against
urx test-domain.com --cache-ttl 300 --cache-path /tmp/urx-scratch.db
```

### Configuration File

Caching can also be set in a [config file](/guide/configuration/):

```toml
[cache]
incremental = true
cache_type = "sqlite"                     # or "redis"
cache_path = "/home/me/.urx/cache.db"     # `~` is not expanded; omit for the default
# redis_url = "redis://localhost:6379"
cache_ttl = 86400
no_cache = false
```
