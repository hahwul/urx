+++
title = "Quick Start"
description = "Run a first scan, pick providers, filter what comes back, and write the results to a file."
toc = true
weight = 2
+++

## Basic Usage

Get started with Urx in minutes using these simple examples.

### Single Domain

Fetch URLs for a single domain:

```bash
urx example.com
```

This retrieves URLs from the default providers (Wayback Machine, Common Crawl and
OTX), plus the target's live `robots.txt` and `sitemap.xml` (turn those off with
`--exclude-robots` / `--exclude-sitemap`), and prints them to the console.
Any provider that takes an API key (vt, urlscan, zoomeye, github, bevigil) joins
automatically when its key is set, even alongside an explicit `--providers`
list; see [Environment Variables](/guide/environment-variables/).

### Multiple Domains

Process multiple domains at once:

```bash
urx example.com example.org
```

### From Standard Input

Read domains from a file or pipeline:

```bash
cat domains.txt | urx

# or name the file directly (alias --dL)
urx --domain-list domains.txt
```

### Save Output to File

Save results to a file instead of displaying in console:

```bash
urx example.com -o results.txt
```

### JSON Output

Output in JSON format for parsing with other tools:

```bash
urx example.com -f json -o results.json
```

## Common Use Cases

### Security Scanning

Filter for JavaScript files that may contain sensitive information:

```bash
urx example.com -e js -o js-files.txt
```

### API Endpoint Discovery

Find API endpoints using pattern matching:

```bash
urx example.com --patterns api,v1,v2,graphql
```

### Exclude Common Resources

Use presets to exclude images and other non-interesting files:

```bash
urx example.com -p no-images,no-resources
```

### With HTTP Status Checking

Validate which URLs are still active:

```bash
urx example.com --check-status --include-status 200
```

## Getting Help

For a complete list of options and flags:

```bash
urx --help
```

## Next Steps

- Learn about all available [CLI Options](/guide/cli-options/)
- Explore more [Examples](/guide/examples/)
- Set up a [Configuration](/guide/configuration/) file for advanced use
