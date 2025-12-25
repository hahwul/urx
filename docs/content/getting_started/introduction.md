---
title: "Introduction"
weight: 1
---

## What is Urx?

Urx is a powerful Rust-based command-line tool designed for extracting URLs from various OSINT (Open Source Intelligence) archives. It helps security researchers, penetration testers, and bug bounty hunters discover attack surfaces by collecting historical URLs from multiple sources.

## Key Features

- **Multi-Source URL Collection**: Fetch URLs from multiple OSINT archives in parallel, including Wayback Machine, Common Crawl, OTX, VirusTotal, and URLScan
- **Advanced Filtering**: Filter results by file extensions, patterns, or presets (e.g., exclude images or resources)
- **Flexible Output**: Supports plain text, JSON, and CSV formats for seamless integration with other tools
- **URL Testing & Validation**: Filter and validate URLs based on HTTP status codes, extract additional links, and perform live checks
- **Performance & Concurrency**: Built with Rust for speed and efficiency, leveraging asynchronous processing and parallel requests
- **Customizable Network Options**: Configure proxies, timeouts, retries, parallelism, and more for robust network operations
- **Caching & Incremental Scanning**: Built-in SQLite/Redis caching support for efficient incremental scans

## Background

Urx was inspired by [gau (GetAllUrls)](https://github.com/lc/gau) and built from the ground up in Rust to provide superior performance, concurrency, and advanced filtering capabilities for security professionals.

## Use Cases

- **Security Research**: Discover historical endpoints and forgotten assets
- **Bug Bounty Hunting**: Find potential vulnerabilities in historical versions of web applications
- **Attack Surface Mapping**: Comprehensive enumeration of all accessible URLs for a target domain
- **Penetration Testing**: Identify potential entry points and interesting endpoints
- **Monitoring**: Track new URLs and changes over time with incremental scanning
