+++
title = "Configuration"
description = "Set urx defaults in a TOML config file, and how those settings combine with command-line flags."
toc = true
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

`-c` replaces the default file rather than layering on top of it: when it is
given, `~/.config/urx/config.toml` is not read at all.

### How Settings Combine

Command-line flags take precedence over config file values, with three caveats:

- **Boolean keys can only switch a feature on.** A `true` in the config (say
  `subs`, `check_status`, `random_agent`, `incremental` or `no_cache`) cannot be
  turned off from the command line, because there is no `--no-…` counterpart.
  Keep such keys out of a shared default config, or switch profiles with `-c`.
  (`exclude_robots` / `exclude_sitemap` are the exception: `--include-robots` /
  `--include-sitemap` override them.)
- **Lists given on the command line replace the configured list** rather than
  extending it. Any `-H` replaces the whole configured `header` set, for example,
  and `--providers` replaces `providers`.
- **A configured `include_status` beats `--exclude-status` on the command
  line.** The two are separate lists, and whenever an include list is present it
  alone decides; pass `--include-status` to replace it. Either list in the
  config also runs the live status check on every run and makes `--stream`
  refuse to run.

This also applies to output views: a view selected on the command line replaces
any configured `show_only_*` view. The three `show_only_*` config keys are
mutually exclusive, and the config is rejected if more than one is `true`.

Keys urx does not recognise, including a misspelled section such as `[filters]`,
are ignored with a `Warning: ignoring unrecognised key(s) …` message. An invalid
value for `format`, `network_scope`, `cache_type`, `[notify].on` or
`[notify].format`, and a `timeout` or `parallel` of `0`, are likewise ignored
with an `Ignoring [section].key=… in config` message and the built-in default is
used. Nothing is printed under `--silent`.

### Full Configuration Reference

Below is a complete annotated configuration file. All sections and keys are
optional, and a key left out takes the default shown in the comment. Keys that
are commented out have no useful default value to show; uncomment and fill them in
as needed.

```toml
# Targets are given on the command line, not here:
#   urx -c config.toml example.com

# ─── Output ──────────────────────────────────────────────
[output]
# output = "results.txt"
format = "plain"           # plain (default), json, jsonl, csv, wordlist
merge_endpoint = false
dedup_similar = false      # Collapse URLs differing only in ids, hashes, dates, or query values
stream = false             # Write URLs as providers report them (unsorted, bypasses cache)

# ─── Providers ───────────────────────────────────────────
[provider]
providers = ["wayback", "cc", "otx"] # default; also keyless: "arquivo", "urlscan" (anonymous)
subs = false                          # Include subdomains
cc_index = "latest"                   # default: newest index, resolved at runtime via collinfo.json.
                                      # Or pin one ("CC-MAIN-2026-17"); several go comma-separated
                                      # in ONE string ("CC-MAIN-2026-17,CC-MAIN-2025-51"), not an array
cdx_endpoint = []                     # Extra CDX index servers, e.g. ["https://vefsafn.is/cdx"] (ids: cdx:<host>)
cdx_dialect = ""                      # "pywb" or "classic" for those servers; empty = probe once, pywb fallback
# from = "2023"                       # Restrict CDX providers to captures >= this date (YYYY/YYYYMM/YYYYMMDD)
# to = "2024"                         # Restrict CDX providers to captures <= this date
archive_status = []                   # Keep only captures the archive recorded with these status codes
archive_exclude_status = []           # Drop captures with these recorded status codes
archive_mime = []                     # Keep only captures with these recorded MIME types
archive_exclude_mime = []             # Drop captures with these recorded MIME types
# API keys are strings, not arrays: comma-separate several keys to rotate them
# ("key1,key2"). Prefer the provider-config file or URX_*_API_KEY for these.
# vt_api_key = "YOUR_KEY"             # VirusTotal API key
# urlscan_api_key = "YOUR_KEY"        # URLScan API key (optional; urlscan also works anonymously)
# zoomeye_api_key = "YOUR_KEY"        # ZoomEye API key
# github_api_key = "YOUR_TOKEN"       # GitHub Code Search personal access token
# bevigil_api_key = "YOUR_KEY"        # BeVigil API key (URLs from unpacked Android apps)
exclude_robots = false                # Skip robots.txt discovery
exclude_sitemap = false               # Skip sitemap.xml discovery
archived_discovery = false            # Also read archived robots.txt / sitemap versions
archived_discovery_limit = 50         # default; documents fetched per domain by each archived provider

# ─── Filters ─────────────────────────────────────────────
[filter]
preset = ["no-images"]            # "no-resources" would also drop js/css, emptying extensions below
extensions = ["js", "php", "aspx"]
exclude_extensions = ["html", "txt"]
patterns = ["admin", "api"]
exclude_patterns = ["logout", "static"]
match_regex = ["/api/v[0-9]+/"]   # Repeatable regexes, ORed, case-sensitive
filter_regex = ["/(assets|static)/"]
show_only_host = false
show_only_path = false
show_only_param = false
min_length = 10
max_length = 500
scope_file = ["scope.txt"]        # Bug-bounty scope files; repeatable, unioned, `!` excludes (`~` is not expanded)
meta_first_seen_after = ""        # Keep URLs first archived on or after this date (YYYY/YYYYMM/YYYYMMDD)
meta_first_seen_before = ""       # ...on or before
meta_last_seen_after = ""         # Keep URLs last archived on or after this date ("still alive as of")
meta_last_seen_before = ""        # ...on or before ("dead since")
meta_mime = []                    # Keep only these archived MIME types ("image/*" matches any subtype)
meta_exclude_mime = []            # Drop these archived MIME types
meta_status = []                  # Keep only these archived status codes ("20x" / "5xx" patterns)
meta_exclude_status = []          # Drop these archived status codes

# ─── Network ─────────────────────────────────────────────
[network]
network_scope = "all"                  # default; all, providers, testers, or providers,testers
# proxy = "http://proxy.example.com:8080"
# proxy_auth = "username:password"
insecure = false
random_agent = false
header = ["X-Env: staging"]            # A string or a list; sent to the target only, never to an archive
cookie = "session=abc"
user_agent = "acme-security-scan/1.0"  # Overrides random_agent for target requests
timeout = 120                          # default, in seconds
retries = 2                            # default
parallel = 5                           # default; domains fetched concurrently per provider
# rate_limit = 10                      # requests per second; unset = no limit

# ─── Testing ─────────────────────────────────────────────
[testing]
check_status = false
# include_status = ["200", "30x"]  # setting this (or exclude_status) runs the live check even with check_status = false
# exclude_status = ["404", "50x"]   # ignored when include_status is set
extract_links = false
extract_js_endpoints = false   # Mine collected JavaScript for endpoints
max_js_files = 500             # default; cap on files --extract-js-endpoints fetches (0 = unlimited)
archive_body = false                   # Mine the archived bodies of collected URLs
archive_body_limit = 500               # default; distinct bodies fetched per run (duplicates never count)
# archive_body_dir = "./corpus"        # Keep each replayed body, with an index.jsonl beside them.
                                       # Requires archive_body = true, or the run is rejected
expand_specs = false                   # Expand collected OpenAPI/Swagger/GraphQL documents into routes
max_spec_files = 50                    # default; cap on specification documents fetched (0 = unlimited)

# ─── Cache ────────────────────────────────────────────────
[cache]
incremental = false
cache_type = "sqlite"                  # default; sqlite or redis (redis needs a --features redis-cache build)
# cache_path = "/home/me/.urx/cache.db" # default: $HOME/.urx/cache.db. `~` is NOT expanded; use an absolute path
# redis_url = "redis://localhost:6379"
cache_ttl = 86400                      # default: 24 hours
no_cache = false

# ─── Notify ──────────────────────────────────────────────
[notify]
# url = "https://hooks.slack.com/services/..."   # or a list; the URL is a secret —
#                                                 # prefer URX_NOTIFY_URL or the
#                                                 # provider-config file's notify_url
on = "new"                             # default; new, always, or never
format = "json"                        # default; json, slack, or discord
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
scope_file = ["/home/me/programs/example/scope.txt"]  # the program's own scope, verbatim

[cache]
incremental = true
```

**API-focused discovery:**

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
expand_specs = true          # [filter] runs first on the collected URLs, so a spec URL must match
                             # patterns to be opened, and again on the routes it expands into
max_spec_files = 25

[network]
parallel = 10
timeout = 60
```

**Monitoring with Redis:**

Redis support is an optional feature: the prebuilt binaries, the Docker image
and a plain `cargo install urx` leave it out, and a `cache_type = "redis"` config
fails with `Redis cache support not compiled in`. Build with
`cargo install urx --features redis-cache` to use it.

```toml
[provider]
providers = ["wayback", "cc", "otx"]

[cache]
incremental = true
cache_type = "redis"
redis_url = "redis://cache-server:6379"
cache_ttl = 43200
```

> Add `--silent` on the command line only together with `-o` or `--notify` (it
> suppresses the results on stdout too); otherwise use `--no-progress`. Several
> flags are CLI-only and
> have no config file equivalent; setting them in the file only produces the
> "unrecognised key" warning:
>
> - display: `--silent`, `--verbose`, `--no-progress`, `--no-color`,
>   `--show-sources`, `--show-meta`, `--stats`
> - host validation: `--strict` / `--no-strict`
> - input: `--files`, `--domain-list`
> - output: `--output-dir`, `--normalize-url`, and the output views
>   `--params`, `--params-by-endpoint` and `--fuzz-placeholder`
> - providers: `--all-providers`, `--exclude-providers`
> - network: `--rate-limit-by`, `--max-time`
> - testing: `--check-title`
> - `--provider-config` itself

### Config File Location

| Platform | Default path |
|----------|--------------|
| Linux / macOS | `$HOME/.config/urx/config.toml` |
| Windows | `%APPDATA%\urx\config.toml` |

`$XDG_CONFIG_HOME` is not consulted. If the file does not exist, urx creates the
directory and an empty `config.toml` on first run. Override it with the `-c` /
`--config` flag.

### Provider Config File

API keys can live in a separate file so the main config holds nothing secret
and can be committed or shared. urx reads it from:

| Platform | Default path |
|----------|--------------|
| Linux / macOS | `$HOME/.config/urx/provider-config.toml` |
| Windows | `%APPDATA%\urx\provider-config.toml` |

or from the path given with `--provider-config <PATH>`. Unlike the main config,
it is never created automatically. Its keys sit at the top level, with no
section header, and all take strings:

```toml
# ~/.config/urx/provider-config.toml
vt_api_key      = "key1,key2"      # comma-separated keys rotate
urlscan_api_key = "YOUR_KEY"
zoomeye_api_key = "YOUR_KEY"
github_api_key  = "ghp_..."
bevigil_api_key = "YOUR_KEY"
notify_url      = "https://hooks.slack.com/services/..."  # comma-separated to fan out
```

Precedence for each key is: command-line flag / environment variable >
provider-config file > main config. A `[provider]` table inside this file is
ignored with a warning, since keys belong at the top level.

### Tips

- Start with a minimal config and add sections as needed.
- Use environment variables for sensitive API keys instead of storing them in the config file. See [Environment Variables](/guide/environment-variables/).
- Create multiple config files for different scanning profiles and switch with `-c`.
