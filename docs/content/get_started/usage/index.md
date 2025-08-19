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

Using VirusTotal and URLScan providers requires API keys. You can provide them in three ways:

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
