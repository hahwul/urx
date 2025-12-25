---
title: "Environment Variables"
weight: 1
---

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
URLScan API key for accessing the URLScan provider.

```bash
export URX_URLSCAN_API_KEY=your_api_key_here
urx example.com --providers urlscan
```

**Multiple Keys (Rotation):**
```bash
export URX_URLSCAN_API_KEY=key1,key2,key3
urx example.com --providers urlscan
```

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
```

#### Use .env Files
For project-specific configuration:

```bash
# .env
URX_VT_API_KEY=your_vt_key
URX_URLSCAN_API_KEY=your_urlscan_key
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
  run: urx example.com
```

### Security Considerations

- Never commit API keys to version control
- Use secrets management for production environments
- Rotate keys regularly
- Use different keys for different environments (dev/staging/prod)

### Future Environment Variables

Additional environment variables may be added in future releases for:
- Default configuration paths
- Cache directory locations
- Network settings
- Output preferences

Check the [Changelog](../../support/changelog) for updates on new environment variables.
