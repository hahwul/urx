+++
title = "Environment Variables"
description = "The environment variables urx reads, including provider API keys and their comma-separated rotation form."
toc = true
weight = 4
+++

## Environment Variables

Urx reads environment variables for provider API keys, the notification
webhook, color, and the system proxy. No environment variable sets a general
option default; use a [config file](/guide/configuration/) for that.

### API Keys

#### URX_VT_API_KEY
VirusTotal API key for accessing the VirusTotal provider.

```bash
export URX_VT_API_KEY=your_api_key_here
urx example.com --providers vt
```

**Multiple Keys (Rotation):**
```bash
export URX_VT_API_KEY=key1,key2,key3
urx example.com --providers vt
```

#### URX_URLSCAN_API_KEY
Optional URLScan API key. The `urlscan` provider works anonymously without a
key (rate-limited to ~30 requests/min per IP); set a key only to raise those
limits and enable key rotation.

```bash
export URX_URLSCAN_API_KEY=your_api_key_here
urx example.com --providers urlscan
```

**Multiple Keys (Rotation):**
```bash
export URX_URLSCAN_API_KEY=key1,key2,key3
urx example.com --providers urlscan
```

#### URX_ZOOMEYE_API_KEY
ZoomEye API key for accessing the ZoomEye provider.

```bash
export URX_ZOOMEYE_API_KEY=your_api_key_here
urx example.com --providers zoomeye
```

**Multiple Keys (Rotation):**
```bash
export URX_ZOOMEYE_API_KEY=key1,key2,key3
urx example.com --providers zoomeye
```

#### URX_GITHUB_API_KEY
GitHub personal access token for the `github` provider (GitHub Code Search),
which requires a token to run.

```bash
export URX_GITHUB_API_KEY=your_token_here
urx example.com --providers github
```

**Multiple Keys (Rotation):**
```bash
export URX_GITHUB_API_KEY=token1,token2,token3
urx example.com --providers github
```

#### URX_BEVIGIL_API_KEY
BeVigil API key for the `bevigil` provider, which returns URLs extracted from
unpacked Android apps. Required; the provider does nothing without it. Get a
key from the [BeVigil OSINT API](https://bevigil.com/osint-api).

```bash
export URX_BEVIGIL_API_KEY=your_key_here
urx example.com --providers bevigil
```

**Multiple Keys (Rotation):**
```bash
export URX_BEVIGIL_API_KEY=key1,key2
urx example.com --providers bevigil
```

### Notifications

#### URX_NOTIFY_URL
Webhook URL(s) for `--notify`. Comma-separate several to fan out. The URL is
treated as a secret: urx never prints more than its scheme, host and port, so the environment
is the recommended place for it.

```bash
export URX_NOTIFY_URL=https://hooks.slack.com/services/T000/B000/XXXX
urx example.com --incremental --notify-format slack
```

`--notify` on the command line takes precedence over the variable; both take
precedence over `notify_url` in the provider-config file and `[notify].url`
in the main config.

### Display

#### NO_COLOR

The standard [`NO_COLOR`](https://no-color.org/) convention is honored: set it
to anything at all — an empty value counts — and urx drops ANSI color from the
progress UI and the output, exactly as `--no-color` does.

```bash
NO_COLOR=1 urx example.com --check-status
```

### Proxy

#### HTTP_PROXY / HTTPS_PROXY / ALL_PROXY / NO_PROXY

urx's HTTP client honours the standard proxy variables (upper- or lower-case)
whenever no `--proxy` (or `[network].proxy`) applies to a request. That includes
the components `--network-scope` leaves out: with `--network-scope testers`, the
archive queries still go through `HTTPS_PROXY` if it is set.

```bash
HTTPS_PROXY=http://127.0.0.1:8080 urx example.com
```

On macOS and Windows the operating system's proxy settings are honoured the same
way.

### Config Locations

urx does not read `XDG_CONFIG_HOME`. The default config and provider-config
files live under `$HOME/.config/urx/` (`%APPDATA%\urx\` on Windows), and the
default SQLite cache is `$HOME/.urx/cache.db` on every platform (`./.urx/cache.db`
when `HOME` is unset). See
[Configuration](/guide/configuration/#config-file-location).

### Summary

| Variable | Provider | Description |
|----------|----------|-------------|
| `URX_VT_API_KEY` | VirusTotal | VirusTotal API key |
| `URX_URLSCAN_API_KEY` | URLScan | Optional URLScan API key (the provider also works anonymously) |
| `URX_ZOOMEYE_API_KEY` | ZoomEye | ZoomEye API key |
| `URX_GITHUB_API_KEY` | GitHub | GitHub Code Search personal access token |
| `URX_BEVIGIL_API_KEY` | BeVigil | BeVigil OSINT API key (URLs from unpacked Android apps) |
| `URX_NOTIFY_URL` | — | Webhook URL(s) for `--notify`, comma-separated |
| `NO_COLOR` | — | Any value disables ANSI color, as `--no-color` does |
| `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY` | — | System proxy, used when no `--proxy` applies |
| `HOME` | — | Base of the default cache path (`$HOME/.urx/cache.db`, else `./.urx/cache.db`) on every platform, and of the config / provider-config paths on Linux/macOS |
| `APPDATA` | — | Base of the config / provider-config paths on Windows |

### Usage Notes

- Environment variables are automatically detected when running Urx
- API keys from `--*-api-key` flags and from the matching `URX_*_API_KEY`
  variable are **combined**, not overridden: CLI keys come first, duplicates are
  dropped, and all of them rotate. `--notify` is the exception; it replaces
  `URX_NOTIFY_URL` outright
- Both the command line and the environment win over the
  [provider-config file](/guide/configuration/#provider-config-file), which wins
  over the main config
- Multiple API keys can be comma-separated for rotation
- Setting a key activates its provider automatically, even when `--providers`
  does not name it; use `--exclude-providers` to keep it off

### Best Practices

#### Store in Profile
Add to your `~/.bashrc`, `~/.zshrc`, or `~/.profile`:

```bash
# Urx Configuration
export URX_VT_API_KEY=your_vt_key
export URX_URLSCAN_API_KEY=your_urlscan_key
export URX_ZOOMEYE_API_KEY=your_zoomeye_key
export URX_GITHUB_API_KEY=your_github_token
export URX_BEVIGIL_API_KEY=your_bevigil_key
export URX_NOTIFY_URL=https://hooks.slack.com/services/...
```

#### Use .env Files
For project-specific configuration:

```bash
# .env
URX_VT_API_KEY=your_vt_key
URX_URLSCAN_API_KEY=your_urlscan_key
URX_ZOOMEYE_API_KEY=your_zoomeye_key
```

Load with `set -a` so the variables are exported to `urx` (a plain
`source .env` creates shell variables that child processes never see):
```bash
set -a; source .env; set +a
urx example.com
```

#### Docker Environment
```bash
docker run --rm \
  -e URX_VT_API_KEY=your_key \
  -e URX_URLSCAN_API_KEY=your_key \
  -e URX_ZOOMEYE_API_KEY=your_key \
  ghcr.io/hahwul/urx:latest \
  ./urx example.com
```

#### CI/CD Secrets
Store API keys as secrets in your CI/CD platform:

**GitHub Actions:**
```yaml
- name: Run Urx
  env:
    URX_VT_API_KEY: ${{ secrets.VT_API_KEY }}
    URX_URLSCAN_API_KEY: ${{ secrets.URLSCAN_API_KEY }}
    URX_ZOOMEYE_API_KEY: ${{ secrets.ZOOMEYE_API_KEY }}
  run: urx example.com
```

### Security Considerations

- Never commit API keys to version control
- Use secrets management for production environments
- Rotate keys regularly
- Use different keys for different environments (dev/staging/prod)
