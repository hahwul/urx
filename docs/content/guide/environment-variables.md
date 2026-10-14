+++
title = "Environment Variables"
description = "The environment variables urx reads, including provider API keys and their comma-separated rotation form."
toc = true
weight = 4
+++

## Environment Variables

urx reads environment variables for provider API keys, the notification webhook, color and the system proxy. No variable sets a general option default; use a [config file](/guide/configuration/) for that.

### API Keys

Each key variable takes one key or a comma-separated list, which rotates.

| Variable | Provider | Notes |
|----------|----------|-------|
| `URX_VT_API_KEY` | `vt` (VirusTotal) | |
| `URX_URLSCAN_API_KEY` | `urlscan` | Optional. Without a key the provider runs anonymously (about 30 requests/min per IP); a key raises the limits and allows rotation. |
| `URX_ZOOMEYE_API_KEY` | `zoomeye` | |
| `URX_GITHUB_API_KEY` | `github` (GitHub Code Search) | Personal access token. Required. |
| `URX_BEVIGIL_API_KEY` | `bevigil` (URLs from unpacked Android apps) | Required. Get one from the [BeVigil OSINT API](https://bevigil.com/osint-api). |

```bash
export URX_VT_API_KEY=your_api_key_here
urx example.com --providers vt

# several keys rotate
export URX_VT_API_KEY=key1,key2,key3
```

How keys combine:

- Keys from `--*-api-key` flags and the matching `URX_*_API_KEY` variable are combined: CLI keys first, duplicates dropped, all rotate.
- The command line and the environment both win over the [provider-config file](/guide/configuration/#provider-config-file), which wins over the main config.
- Setting a key enables its provider even when `--providers` doesn't name it. Use `--exclude-providers` to keep it off. This also changes the [cache key](/guide/caching/).

### Notifications

#### URX_NOTIFY_URL

Webhook URL(s) for `--notify`. Comma-separate several to fan out. urx treats the URL as a secret and never prints more than its scheme, host and port, so the environment is the recommended place for it.

```bash
export URX_NOTIFY_URL=https://hooks.slack.com/services/T000/B000/XXXX
urx example.com --incremental --notify-format slack
```

`--notify` on the command line replaces the variable outright (it is not combined like API keys). Both win over `notify_url` in the provider-config file and `[notify].url` in the main config.

### Display

#### NO_COLOR

Follows the [`NO_COLOR`](https://no-color.org/) convention. Any value, even an empty one, drops ANSI color from the progress UI and output, the same as `--no-color`.

```bash
NO_COLOR=1 urx example.com --check-status
```

### Proxy

#### HTTP_PROXY / HTTPS_PROXY / ALL_PROXY / NO_PROXY

The HTTP client honours the standard proxy variables (upper- or lower-case) whenever no `--proxy` (or `[network].proxy`) applies to a request. That includes components `--network-scope` leaves out: with `--network-scope testers`, archive queries still go through `HTTPS_PROXY` if it is set. On macOS and Windows the system proxy settings are honoured the same way.

```bash
HTTPS_PROXY=http://127.0.0.1:8080 urx example.com
```

### Config Locations

urx does not read `XDG_CONFIG_HOME`. The config and provider-config files live under `$HOME/.config/urx/` (`%APPDATA%\urx\` on Windows). The default SQLite cache is `$HOME/.urx/cache.db` on every platform (`./.urx/cache.db` when `HOME` is unset). See [Configuration](/guide/configuration/#config-file-location).

### Summary

| Variable | Description |
|----------|-------------|
| `URX_VT_API_KEY` | VirusTotal API key |
| `URX_URLSCAN_API_KEY` | URLScan API key (optional) |
| `URX_ZOOMEYE_API_KEY` | ZoomEye API key |
| `URX_GITHUB_API_KEY` | GitHub Code Search personal access token |
| `URX_BEVIGIL_API_KEY` | BeVigil OSINT API key |
| `URX_NOTIFY_URL` | Webhook URL(s) for `--notify`, comma-separated |
| `NO_COLOR` | Any value disables ANSI color |
| `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY` | System proxy, used when no `--proxy` applies |
| `HOME` | Base of the default cache path on every platform, and of the config paths on Linux/macOS |
| `APPDATA` | Base of the config paths on Windows |

### Passing Keys

Load a `.env` file with `set -a` so the variables are exported (a plain `source .env` sets shell variables that `urx` never sees):

```bash
set -a; source .env; set +a
urx example.com
```

Docker (the image has no ENTRYPOINT, so name `./urx`):

```bash
docker run --rm -e URX_VT_API_KEY=your_key ghcr.io/hahwul/urx:latest ./urx example.com
```

GitHub Actions:

```yaml
- name: Run urx
  env:
    URX_VT_API_KEY: ${{ secrets.VT_API_KEY }}
  run: urx example.com
```

Keep keys out of the main config if you commit it: use these variables or the [provider-config file](/guide/configuration/#provider-config-file).
