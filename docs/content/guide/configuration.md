+++
title = "Configuration"
weight = 2
+++

## Configuration File

Urx supports loading settings from a TOML configuration file. This avoids repeating options on every command and lets you manage profiles for different scanning scenarios.

### Loading a Config File

```bash
# Explicit path
urx -c /path/to/config.toml example.com

# Default location (auto-detected)
# ~/.config/urx/config.toml
urx example.com
```

Command-line flags always take precedence over config file values.

### Full Configuration Reference

Below is a complete annotated configuration file. All sections and keys are optional.

```toml
# Domain configuration
domains = ["example.com", "example.org"]

# ─── Output ──────────────────────────────────────────────
[output]
output = "results.txt"
format = "plain"           # plain, json, csv
merge_endpoint = false
normalize_url = false

# ─── Providers ───────────────────────────────────────────
[provider]
providers = ["wayback", "cc", "otx"]
subs = false                          # Include subdomains
cc_index = "CC-MAIN-2025-13"         # Common Crawl index
vt_api_key = ""                       # VirusTotal API key
urlscan_api_key = ""                  # URLScan API key
zoomeye_api_key = ""                  # ZoomEye API key
exclude_robots = false                # Skip robots.txt discovery
exclude_sitemap = false               # Skip sitemap.xml discovery

# ─── Display ─────────────────────────────────────────────
verbose = false
silent = false
no_progress = false

# ─── Filters ─────────────────────────────────────────────
[filter]
preset = ["no-resources", "no-images"]
extensions = ["js", "php", "aspx"]
exclude_extensions = ["html", "txt"]
patterns = ["admin", "api"]
exclude_patterns = ["logout", "static"]
show_only_host = false
show_only_path = false
show_only_param = false
min_length = 10
max_length = 500
strict = true

# ─── Network ─────────────────────────────────────────────
[network]
network_scope = "all"                  # all, providers, testers
proxy = "http://proxy.example.com:8080"
proxy_auth = "username:password"
insecure = false
random_agent = true
timeout = 30
retries = 3
parallel = 5
rate_limit = 10

# ─── Testing ─────────────────────────────────────────────
[testing]
check_status = false
include_status = ["200", "30x"]
exclude_status = ["404", "50x"]
extract_links = false

# ─── Cache ────────────────────────────────────────────────
[cache]
incremental = false
cache_type = "sqlite"                  # sqlite or redis
cache_path = "~/.urx/cache.db"
redis_url = "redis://localhost:6379"
cache_ttl = 86400                      # 24 hours
no_cache = false
```

### Minimal Config Examples

**Bug bounty profile:**

```toml
[provider]
providers = ["wayback", "cc", "otx", "vt"]
vt_api_key = "YOUR_KEY"
subs = true

[filter]
preset = ["no-resources"]
patterns = ["api", "admin", "login"]

[cache]
incremental = true
```

**API-focused discovery:**

```toml
[provider]
providers = ["wayback", "cc", "otx", "vt", "urlscan", "zoomeye"]
vt_api_key = "YOUR_VT_KEY"
urlscan_api_key = "YOUR_URLSCAN_KEY"
zoomeye_api_key = "YOUR_ZOOMEYE_KEY"
subs = true

[filter]
patterns = ["api", "graphql", "rest", "v1", "v2"]
extensions = ["json", "xml"]

[network]
parallel = 10
timeout = 60
```

**Monitoring with Redis:**

```toml
[provider]
providers = ["wayback", "cc", "otx"]

silent = true

[cache]
incremental = true
cache_type = "redis"
redis_url = "redis://cache-server:6379"
cache_ttl = 43200
```

### Config File Location

The default config file location is `~/.config/urx/config.toml`. You can override this with the `-c` / `--config` flag.

### Tips

- Start with a minimal config and add sections as needed.
- Use environment variables for sensitive API keys instead of storing them in the config file. See [Environment Variables](/guide/environment-variables/).
- Create multiple config files for different scanning profiles and switch with `-c`.
