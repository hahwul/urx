---
title: "Usage"
weight: 2
---

### Basic Usage

```bash
# Single domain
urx example.com

# Multiple domains
urx example.com example.org

# From file
cat domains.txt | urx

# File input (WARC, gzip/bzip2, plain text)
urx --files urls.txt
```

### Output

```bash
# Save to file
urx example.com -o results.txt

# JSON format
urx example.com -f json -o results.json
```

### Filtering

```bash
# Include extensions
urx example.com -e js

# Exclude extensions
urx example.com --exclude-extensions html,txt

# Pattern matching
urx example.com --patterns api,v1,graphql
urx example.com --exclude-patterns static,images

# Presets
urx example.com -p no-images

# Advanced filters
urx example.com -e js,php --patterns admin,login --min-length 20
```

### Providers

```bash
# Specific providers
urx example.com --providers wayback,otx

# API keys (CLI)
urx example.com --vt-api-key=KEY --urlscan-api-key=KEY

# API keys (Environment)
export URX_VT_API_KEY=KEY
export URX_URLSCAN_API_KEY=KEY
urx example.com --providers=vt,urlscan

# API key rotation
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3
URX_VT_API_KEY=key1,key2,key3 urx example.com
```

### Discovery

```bash
# Exclude robots.txt
urx example.com --exclude-robots

# Exclude sitemap.xml
urx example.com --exclude-sitemap
```

### Testing & Validation

```bash
# Include subdomains
urx example.com --subs

# Check HTTP status
urx example.com --check-status

# Extract links
urx example.com --extract-links

# Status filtering
urx example.com --include-status 200,30x --exclude-status 404,50x
```

### Network

```bash
urx example.com --proxy http://localhost:8080 --timeout 60 --parallel 10 --insecure
```

### File Input

```bash
# Single file
urx --files urls.txt

# Multiple files
urx --files urls.txt archive.warc data.gz
urx --files urls.txt --files archive.warc

# With filters
urx --files data.txt --patterns api,admin -f json
```

### URL Normalization

```bash
# Basic normalization
urx example.com --normalize-url

# With endpoint merging
urx example.com --normalize-url --merge-endpoint

# From file
urx --files urls.txt --normalize-url
```

### Caching

```bash
# SQLite (default)
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db

# Redis
urx example.com --cache-type redis --redis-url redis://localhost:6379

# Incremental (new URLs only)
urx example.com --incremental

# Set TTL (seconds)
urx example.com --cache-ttl 43200

# Disable cache
urx example.com --no-cache

# With filters
urx example.com --incremental -e js,php --patterns api
```

**Use Cases:**

```bash
# Daily monitoring
urx target.com --incremental --silent | notify-tool

# Distributed scanning
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# Rapid iterations
urx test-domain.com --cache-ttl 300
```
