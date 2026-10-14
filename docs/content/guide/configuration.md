+++
title = "Configuration"
description = "Set urx defaults in a TOML config file, and how those settings combine with command-line flags."
toc = true
weight = 2
+++

## Configuration File

urx reads settings from a TOML file, so you don't repeat options on every run. Keep one file per scan profile.

### Loading a Config File

```bash
# explicit path
urx -c /path/to/config.toml example.com

# default location, read automatically: ~/.config/urx/config.toml
urx example.com
```

`-c` replaces the default file. When it is given, `~/.config/urx/config.toml` is not read.

### How Settings Combine

Command-line flags override config values, with these exceptions:

- A boolean key set to `true` (`subs`, `check_status`, `random_agent`, `incremental`, `no_cache`, ...) can't be turned off from the command line, since there is no `--no-...` flag. Keep such keys out of a shared default config, or switch profiles with `-c`. `exclude_robots` / `exclude_sitemap` are the exception: `--include-robots` / `--include-sitemap` override them.
- A list given on the command line replaces the configured list. Any `-H` replaces the whole `header` set, and a `-H` naming `Cookie` or `User-Agent` also overrides the `cookie` / `user_agent` key; `--providers` replaces `providers`.
- A configured `include_status` beats `--exclude-status` on the command line. Whenever an include list is present, it alone decides; pass `--include-status` to replace it. Either list in the config also runs the live status check on every run and makes `--stream` refuse to run.
- An output view chosen on the command line replaces any configured `show_only_*` view. The three `show_only_*` keys are mutually exclusive; the config is rejected if more than one is `true`.

Unknown keys, including a misspelled section such as `[filters]`, are ignored with `Warning: ignoring unrecognised key(s) ...`. An invalid value for `format`, `network_scope`, `cache_type`, `[notify].on` or `[notify].format`, or a `timeout` / `parallel` of `0`, is ignored with `Ignoring [section].key=... in config` and the built-in default is used. Nothing is printed under `--silent`.

### Full Configuration Reference

All sections and keys are optional. A key left out takes the default shown. Commented-out keys have no useful default; uncomment and fill them in as needed.

```toml
# targets go on the command line: urx -c config.toml example.com

# ─── Output ──────────────────────────────────────────────
[output]
# output = "results.txt"
format = "plain"           # plain (default), json, jsonl, csv, wordlist
merge_endpoint = false
dedup_similar = false      # collapse URLs differing only in ids, hashes, dates or query values
stream = false             # write URLs as providers report them (unsorted, bypasses cache)

# ─── Providers ───────────────────────────────────────────
[provider]
providers = ["wayback", "cc", "otx"] # default; also keyless: "arquivo", "urlscan" (anonymous)
subs = false                          # include subdomains
cc_index = "latest"                   # default: newest index, resolved at runtime via collinfo.json.
                                      # or pin one ("CC-MAIN-2026-17"); several go comma-separated
                                      # in ONE string ("CC-MAIN-2026-17,CC-MAIN-2025-51"), not an array
cdx_endpoint = []                     # extra CDX servers, e.g. ["https://vefsafn.is/cdx"] (ids: cdx:<host>)
cdx_dialect = ""                      # "pywb" or "classic"; empty = probe once, pywb fallback
# from = "2023"                       # CDX providers: captures >= this date (YYYY/YYYYMM/YYYYMMDD)
# to = "2024"                         # CDX providers: captures <= this date
archive_status = []                   # keep only captures recorded with these status codes
archive_exclude_status = []           # drop captures with these recorded status codes
archive_mime = []                     # keep only captures with these recorded MIME types
archive_exclude_mime = []             # drop captures with these recorded MIME types
# API keys are strings: comma-separate several to rotate ("key1,key2").
# Prefer the provider-config file or URX_*_API_KEY for these.
# vt_api_key = "YOUR_KEY"             # VirusTotal
# urlscan_api_key = "YOUR_KEY"        # URLScan (optional; works anonymously)
# zoomeye_api_key = "YOUR_KEY"        # ZoomEye
# github_api_key = "YOUR_TOKEN"       # GitHub Code Search personal access token
# bevigil_api_key = "YOUR_KEY"        # BeVigil (URLs from unpacked Android apps)
include_robots = true                 # default; false skips robots.txt discovery
include_sitemap = true                # default; false skips sitemap.xml discovery
exclude_robots = false                # skip robots.txt discovery (wins over include_robots)
exclude_sitemap = false               # skip sitemap.xml discovery (wins over include_sitemap)
archived_discovery = false            # also read archived robots.txt / sitemap versions
archived_discovery_limit = 50         # default; documents fetched per domain by each archived provider

# ─── Filters ─────────────────────────────────────────────
[filter]
preset = ["no-images"]            # "no-resources" would also drop js/css, emptying extensions below
extensions = ["js", "php", "aspx"]
exclude_extensions = ["html", "txt"]
patterns = ["admin", "api"]
exclude_patterns = ["logout", "static"]
match_regex = ["/api/v[0-9]+/"]   # repeatable, ORed, case-sensitive
filter_regex = ["/(assets|static)/"]
show_only_host = false
show_only_path = false
show_only_param = false
min_length = 10
max_length = 500
scope_file = ["scope.txt"]        # bug-bounty scope files; unioned, `!` excludes, `~` not expanded
meta_first_seen_after = ""        # keep URLs first archived on or after this date (YYYY/YYYYMM/YYYYMMDD)
meta_first_seen_before = ""       # ...on or before
meta_last_seen_after = ""         # keep URLs last archived on or after this date ("still alive as of")
meta_last_seen_before = ""        # ...on or before ("dead since")
meta_mime = []                    # keep only these archived MIME types ("image/*" matches any subtype)
meta_exclude_mime = []            # drop these archived MIME types
meta_status = []                  # keep only these archived status codes ("20x" / "5xx" patterns)
meta_exclude_status = []          # drop these archived status codes

# ─── Network ─────────────────────────────────────────────
[network]
network_scope = "all"                  # default; all, providers, testers, or providers,testers
# proxy = "http://proxy.example.com:8080"
# proxy_auth = "username:password"
insecure = false
random_agent = false
header = ["X-Env: staging"]            # string or list; sent to the target only, never to an archive
cookie = "session=abc"
user_agent = "acme-security-scan/1.0"  # overrides random_agent for target requests
timeout = 120                          # default, seconds
retries = 2                            # default
parallel = 5                           # default; domains fetched concurrently per provider
# rate_limit = 10                      # requests per second; unset = no limit

# ─── Testing ─────────────────────────────────────────────
[testing]
check_status = false
# include_status = ["200", "30x"]  # this (or exclude_status) runs the live check even with check_status = false
# exclude_status = ["404", "50x"]   # ignored when include_status is set
extract_links = false
extract_js_endpoints = false   # mine collected JavaScript for endpoints
max_js_files = 500             # default; JS files fetched (0 = unlimited)
archive_body = false           # mine the archived bodies of collected URLs
archive_body_limit = 500       # default; distinct bodies fetched per run (duplicates don't count)
# archive_body_dir = "./corpus" # keep each replayed body plus an index.jsonl.
                                # requires archive_body = true, or the run is rejected
expand_specs = false           # expand collected OpenAPI/Swagger/GraphQL documents into routes
max_spec_files = 50            # default; spec documents fetched (0 = unlimited)

# ─── Cache ───────────────────────────────────────────────
[cache]
incremental = false
cache_type = "sqlite"                   # default; sqlite or redis (redis needs a --features redis-cache build)
# cache_path = "/home/me/.urx/cache.db" # default: $HOME/.urx/cache.db. `~` is NOT expanded
# redis_url = "redis://localhost:6379"
cache_ttl = 86400                       # default: 24 hours
no_cache = false

# ─── Notify ──────────────────────────────────────────────
[notify]
# url = "https://hooks.slack.com/services/..."  # or a list. The URL is a secret: prefer
#                                                # URX_NOTIFY_URL or provider-config's notify_url
on = "new"                             # default; new, always, or never
format = "json"                        # default; json, slack, or discord
```

### Minimal Config Examples

Bug bounty profile:

```toml
[provider]
providers = ["wayback", "cc", "otx", "vt"]
vt_api_key = "YOUR_KEY"
subs = true

[filter]
preset = ["no-resources"]
patterns = ["api", "admin", "login"]
scope_file = ["/home/me/programs/example/scope.txt"]  # the program's scope, verbatim

[cache]
incremental = true
```

API-focused discovery:

```toml
[provider]
providers = ["wayback", "cc", "otx", "vt", "urlscan", "zoomeye", "bevigil"]
vt_api_key = "YOUR_VT_KEY"
urlscan_api_key = "YOUR_URLSCAN_KEY"
zoomeye_api_key = "YOUR_ZOOMEYE_KEY"
bevigil_api_key = "YOUR_BEVIGIL_KEY"
subs = true

[filter]
patterns = ["api", "graphql", "rest", "v1", "v2", "swagger"]  # "swagger" keeps /swagger.json

[testing]
expand_specs = true          # [filter] runs first, so a spec URL must match patterns to be
                             # opened, and filters run again on the routes it expands into
max_spec_files = 25

[network]
parallel = 10
timeout = 60
```

Monitoring with Redis. Redis is an optional feature: prebuilt binaries, the Docker image and a plain `cargo install urx` leave it out, and `cache_type = "redis"` fails with `Redis cache support not compiled in`. Build with `cargo install urx --features redis-cache`.

```toml
[provider]
providers = ["wayback", "cc", "otx"]

[cache]
incremental = true
cache_type = "redis"
redis_url = "redis://cache-server:6379"
cache_ttl = 43200
```

### CLI-only Flags

These flags have no config key. Setting them in the file only produces the "unrecognised key" warning.

- display: `--silent`, `--verbose`, `--no-progress`, `--no-color`, `--show-sources`, `--show-meta`, `--stats`
- host validation: `--strict` / `--no-strict`
- input: `--files`, `--domain-list`
- output: `--output-dir`, `--normalize-url`, and the views `--params`, `--params-by-endpoint`, `--fuzz-placeholder`
- providers: `--all-providers`, `--exclude-providers`
- network: `--rate-limit-by`, `--max-time`
- testing: `--check-title`
- `--provider-config` itself

`--silent` also hides the URL list on stdout, so pair it with `-o` or `--notify`. To hide only the progress bar, use `--no-progress`.

### Config File Location

| Platform | Default path |
|----------|--------------|
| Linux / macOS | `$HOME/.config/urx/config.toml` |
| Windows | `%APPDATA%\urx\config.toml` |

`$XDG_CONFIG_HOME` is not read. If the file is missing, urx creates the directory and an empty `config.toml` on first run. Override with `-c` / `--config`.

### Provider Config File

API keys can live in a separate file, so the main config holds no secrets and can be committed or shared. urx reads it from:

| Platform | Default path |
|----------|--------------|
| Linux / macOS | `$HOME/.config/urx/provider-config.toml` |
| Windows | `%APPDATA%\urx\provider-config.toml` |

or from `--provider-config <PATH>`. It is never created automatically. Keys sit at the top level (no section header) and all take strings:

```toml
# ~/.config/urx/provider-config.toml
vt_api_key      = "key1,key2"      # comma-separated keys rotate
urlscan_api_key = "YOUR_KEY"
zoomeye_api_key = "YOUR_KEY"
github_api_key  = "ghp_..."
bevigil_api_key = "YOUR_KEY"
notify_url      = "https://hooks.slack.com/services/..."  # comma-separated to fan out
```

Precedence per key: command-line flag / environment variable > provider-config file > main config. A `[provider]` table in this file is ignored with a warning.

For keys in the environment, see [Environment Variables](/guide/environment-variables/).
