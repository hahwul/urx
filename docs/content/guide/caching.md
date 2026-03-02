+++
title = "Caching"
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

### Clearing the Cache

For SQLite, delete the database file:

```bash
rm ~/.urx/cache.db
```

For Redis, use the Redis CLI:

```bash
redis-cli FLUSHDB
```

### Combined Examples

```bash
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
