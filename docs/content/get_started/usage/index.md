---
title: "Usage"
weight: 2
---

This section covers the basic usage of Urx and provides examples for common use cases.

### Basic Usage

To scan a single domain:

```bash
urx example.com
```

To scan multiple domains:

```bash
urx example.com example.org
```

You can also pipe a list of domains from a file:

```bash
cat domains.txt | urx
```

Unified File Input:

Read URLs from files with automatic format detection (supports WARC files, URLTeam compressed files (gzip/bzip2), and plain text files):

```bash
urx --files urls.txt
```

### Examples

Here are some examples of how to use Urx with different options:

**Saving Output**

Save results to a file:

```bash
urx example.com -o results.txt
```

Output in JSON format:

```bash
urx example.com -f json -o results.json
```

**Filtering**

Filter for JavaScript files only:

```bash
urx example.com -e js
```

Exclude HTML and text files:

```bash
urx example.com --exclude-extensions html,txt
```

Filter for API endpoints:

```bash
urx example.com --patterns api,v1,graphql
```

Exclude specific patterns:

```bash
urx example.com --exclude-patterns static,images
```

Use a filter preset to exclude common image types:

```bash
urx example.com -p no-images
```

**Providers**

Use specific providers:

```bash
urx example.com --providers wayback,otx
```

Using VirusTotal and URLScan providers requires API keys. You can provide them in multiple ways:

1.  **Command Line:**

    ```bash
    urx example.com --providers=vt,urlscan --vt-api-key=YOUR_VT_KEY --urlscan-api-key=YOUR_URLSCAN_KEY
    ```

2.  **Environment Variables:**

    ```bash
    export URX_VT_API_KEY=YOUR_VT_KEY
    export URX_URLSCAN_API_KEY=YOUR_URLSCAN_KEY
    urx example.com --providers=vt,urlscan
    ```

3.  **Auto-Enabling:** If API keys are provided, the providers are automatically enabled.

    ```bash
    urx example.com --vt-api-key=YOUR_VT_KEY --urlscan-api-key=YOUR_URLSCAN_KEY
    ```

4.  **Multiple API key rotation (to mitigate rate limits)**

    Using repeated flags for multiple keys:

    ```bash
    urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3
    ```

    Using environment variables with comma-separated keys:

    ```bash
    URX_VT_API_KEY=key1,key2,key3 URX_URLSCAN_API_KEY=ukey1,ukey2 urx example.com
    ```

    Combining CLI flags and environment variables (CLI keys are used first):

    ```bash
    URX_VT_API_KEY=env_key1,env_key2 urx example.com --vt-api-key=cli_key1 --vt-api-key=cli_key2
    ```

**Discovery**

By default, Urx includes URLs from `robots.txt` and `sitemap.xml`.

Exclude `robots.txt`:

```bash
urx example.com --exclude-robots
```

Exclude `sitemap.xml`:

```bash
urx example.com --exclude-sitemap
```

**Other**

Include subdomains in the scan:

```bash
urx example.com --subs
```

Check the HTTP status of collected URLs:

```bash
urx example.com --check-status
```

Extract additional links from the HTML of collected URLs:

```bash
urx example.com --extract-links
```

**Network Configuration**

Use a proxy, set a timeout, and increase parallel requests:

```bash
urx example.com --proxy http://localhost:8080 --timeout 60 --parallel 10 --insecure
```

**Advanced Filtering**

Combine multiple filters:

```bash
urx example.com -e js,php --patterns admin,login --exclude-patterns logout,static --min-length 20
```

Filter by HTTP status codes:

```bash
urx example.com --include-status 200,30x,405 --exclude-status 20x
```

**Unified File Input**

Read URLs from a single file (auto-detects format):

```bash
urx --files urls.txt
```

Read from multiple files with space separation:

```bash
urx --files urls.txt archive.warc data.gz
```

Read from multiple files with repeated flags:

```bash
urx --files urls.txt --files archive.warc
```

Combine with filtering and formatting options:

```bash
urx --files data.txt --patterns api,admin -f json
```

URL normalization and deduplication:

Normalize URLs by sorting query parameters and removing trailing slashes:

```bash
urx example.com --normalize-url
```

Combine normalization with endpoint merging for comprehensive deduplication:

```bash
urx example.com --normalize-url --merge-endpoint
```

URL normalization with file input:

```bash
urx --files urls.txt --normalize-url
```

### Caching and Incremental Scanning

Urx supports caching to improve performance for repeated scans and incremental scanning to discover only new URLs.

```bash
# Enable caching with SQLite (default)
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db

# Use Redis for distributed caching
urx example.com --cache-type redis --redis-url redis://localhost:6379

# Incremental scanning - only show new URLs since last scan
urx example.com --incremental

# Set cache TTL (time-to-live) to 12 hours
urx example.com --cache-ttl 43200

# Disable caching entirely
urx example.com --no-cache

# Combine incremental scanning with filters
urx example.com --incremental -e js,php --patterns api

# Configuration file with caching settings
urx -c example/config.toml example.com
```

#### Caching Use Cases

```bash
# Daily monitoring - only alert on new URLs
urx target.com --incremental --silent | notify-tool

# Efficient domain lists processing
cat domains.txt | urx --incremental --cache-ttl 3600 > new_urls.txt

# Distributed team scanning with Redis
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# Fast re-scans during development
urx test-domain.com --cache-ttl 300  # 5-minute cache for rapid iterations
```
