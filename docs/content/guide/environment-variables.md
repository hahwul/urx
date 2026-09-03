+++
title = "Environment Variables"
weight = 4
+++

## Environment Variables

Urx supports configuration through environment variables for sensitive data and default settings.

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

### Notifications

#### URX_NOTIFY_URL
Webhook URL(s) for `--notify`. Comma-separate several to fan out. The URL is
treated as a secret: urx never prints more than its host, so the environment
is the recommended place for it.

```bash
export URX_NOTIFY_URL=https://hooks.slack.com/services/T000/B000/XXXX
urx example.com --incremental --notify-format slack
```

`--notify` on the command line takes precedence over the variable; both take
precedence over `notify_url` in the provider-config file and `[notify].url`
in the main config.

### Summary

| Variable | Provider | Description |
|----------|----------|-------------|
| `URX_VT_API_KEY` | VirusTotal | VirusTotal API key |
| `URX_URLSCAN_API_KEY` | URLScan | Optional URLScan API key (the provider also works anonymously) |
| `URX_ZOOMEYE_API_KEY` | ZoomEye | ZoomEye API key |
| `URX_GITHUB_API_KEY` | GitHub | GitHub Code Search personal access token |
| `URX_NOTIFY_URL` | — | Webhook URL(s) for `--notify`, comma-separated |

### Usage Notes

- Environment variables are automatically detected when running Urx
- Command-line flags take precedence over environment variables
- Multiple API keys can be comma-separated for rotation
- API keys enable automatic activation of the respective providers

### Best Practices

#### Store in Profile
Add to your `~/.bashrc`, `~/.zshrc`, or `~/.profile`:

```bash
# Urx Configuration
export URX_VT_API_KEY=your_vt_key
export URX_URLSCAN_API_KEY=your_urlscan_key
export URX_ZOOMEYE_API_KEY=your_zoomeye_key
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

Load with:
```bash
source .env
urx example.com
```

#### Docker Environment
```bash
docker run --rm \
  -e URX_VT_API_KEY=your_key \
  -e URX_URLSCAN_API_KEY=your_key \
  -e URX_ZOOMEYE_API_KEY=your_key \
  ghcr.io/hahwul/urx:latest \
  example.com
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
