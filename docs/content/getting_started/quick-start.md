---
title: "Quick Start"
weight: 3
---

## Basic Usage

Get started with Urx in minutes using these simple examples.

### Single Domain

Fetch URLs for a single domain:

```bash
urx example.com
```

This will retrieve URLs from default providers (Wayback Machine, Common Crawl, and OTX) and output them to the console.

### Multiple Domains

Process multiple domains at once:

```bash
urx example.com example.org
```

### From Standard Input

Read domains from a file or pipeline:

```bash
cat domains.txt | urx
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

- Learn about all available [Options & Flags](../../usage/options-flags)
- Explore more [Examples](../../usage/examples)
- Set up [Configuration](../../usage/configuration) for advanced use
