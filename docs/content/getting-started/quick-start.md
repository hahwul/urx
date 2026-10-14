+++
title = "Quick Start"
description = "Run a first scan, pick providers, filter what comes back, and write the results to a file."
toc = true
weight = 2
+++

## Basic Usage

### Single Domain

```bash
urx example.com
```

This queries the default providers (Wayback Machine, Common Crawl, OTX) plus the
target's live `robots.txt` and `sitemap.xml`, and prints the URLs. Turn the last
two off with `--exclude-robots` / `--exclude-sitemap`.

A provider that takes an API key (vt, urlscan, zoomeye, github, bevigil) is added
when its key is set, even with an explicit `--providers` list. See
[Environment Variables](/guide/environment-variables/).

### Multiple Domains

```bash
urx example.com example.org
```

### From Standard Input

```bash
cat domains.txt | urx

# or name the file directly (alias --dL)
urx --domain-list domains.txt
```

### Save Output to File

```bash
urx example.com -o results.txt
```

### JSON Output

```bash
urx example.com -f json -o results.json
```

## Common Use Cases

### Security Scanning

JavaScript files only:

```bash
urx example.com -e js -o js-files.txt
```

### API Endpoint Discovery

URLs containing any of these patterns:

```bash
urx example.com --patterns api,v1,v2,graphql
```

### Exclude Common Resources

Drop images and static resources with presets:

```bash
urx example.com -p no-images,no-resources
```

### With HTTP Status Checking

Keep only URLs that return 200:

```bash
urx example.com --check-status --include-status 200
```

## Getting Help

```bash
urx --help
```

## Next Steps

- [CLI Options](/guide/cli-options/)
- [Examples](/guide/examples/)
- [Configuration](/guide/configuration/) file
