---
title: "Changelog"
weight: 3
---

## Version History

Track the evolution of Urx with release notes for each version.

## Version 0.8.0

**New Features:**
- **Multiple API Key Rotation**: Support for rotating multiple API keys (e.g., `--vt-api-key key1 --vt-api-key key2`)
- **URL Normalization**: Added `--normalize-url` flag for URL normalization and deduplication
- **Endpoint Merging**: Added `--merge-endpoint` flag to merge endpoints with same path
- **Caching System**: Built-in caching support for incremental scanning
  - SQLite cache support (default)
  - Redis cache support for distributed environments
  - `--incremental` mode for scanning only new URLs
  - Configurable cache TTL with `--cache-ttl`
  - Cache management with `--no-cache` option

## Version 0.7.0

**New Features:**
- **Direct File Reading**: Unified `--files` flag for reading from files with auto-detection
  - Support for WARC files
  - Support for gzip/bzip2 compressed files
  - Support for plain text files

**Improvements:**
- Centralized, randomized modern User-Agent across providers and testers
- robots.txt and sitemap.xml now respect `--random-agent` flag
- Stabilized environment variable tests
- Fixed clippy warnings for better code quality

## Version 0.6.1

**Improvements:**
- Updated dependencies for better performance and security

**Bug Fixes:**
- Fixed bug in HostValidator when using `--subs` flag (#78)

## Version 0.6.0

**New Features:**
- **Enhanced URL Discovery**:
  - robots.txt discovery enabled by default
  - sitemap.xml discovery enabled by default
  - `--exclude-robots` flag to disable robots.txt discovery
  - `--exclude-sitemap` flag to disable sitemap.xml discovery
- **HTTP Status Highlighting**: Added `--check-status` flag for response status checking (#59)

**Improvements:**
- Enhanced API key handling - auto-enables VirusTotal and URLScan when keys are provided (#60)
- Improved network reliability with increased default timeout (30s → 120s) and optimized retry settings (#68)

**Bug Fixes:**
- Resolved OTX provider parsing bug for null values (#70)
- Fixed connectivity issues with Wayback Machine, Common Crawl, and OTX (#62)

## Version 0.5.0

**New Features:**
- robots.txt discovery functionality (by [@Adesoji1](https://github.com/Adesoji1))
- sitemap.xml discovery functionality
- `--strict` flag for enforcing exact host validation (enabled by default)

## Version 0.4.0

**New Features:**
- **Configuration File Support**: `--config` flag to load settings from file
- **VirusTotal Provider**: Search URLs via VirusTotal API
  - `--vt-api-key` flag and `URX_VT_API_KEY` environment variable
- **URLScan Provider**: Search URLs via URLScan API
  - `--urlscan-api-key` flag and `URX_URLSCAN_API_KEY` environment variable

**Improvements:**
- Significant performance improvements

## Version 0.3.0

**New Features:**
- `--insecure` flag to skip SSL certificate verification
- `--network-scope` flag to control which components use network settings
- **Status Filtering**:
  - `--include-status` (alias: `--is`) to filter by HTTP status codes
  - `--exclude-status` (alias: `--es`) to exclude specific status codes

## Version 0.2.0

**New Features:**
- **Display Control**:
  - `--silent` mode for no output
  - `--no-progress` to disable progress bar
- `--preset` flag for predefined URL filters (e.g., no-resources, only-js)

## Version 0.1.0

**Initial Release:**
- Core functionality established
- Multi-provider support (Wayback Machine, Common Crawl, OTX)
- Basic filtering capabilities
- Async processing architecture
- Multiple output formats (plain, JSON, CSV)

---

For detailed release information and downloads, visit the [GitHub Releases page](https://github.com/hahwul/urx/releases).
