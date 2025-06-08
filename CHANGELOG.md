# Changelog

## Unreleased

- Dependencies Update
- Fixed a bug in the HostValidator when using the `--subs` flag (#78)

## 0.6.0

- Enhanced URL discovery features
  - Added robots.txt and sitemap.xml discovery by default
  - Added `--exclude-robots` and `--exclude-sitemap` flags to disable discovery when needed
- Added HTTP response status highlighting with `--check-status` flag (#59)
- Improved API key handling for providers
  - Auto-enables VirusTotal and Urlscan when API keys are provided (#60)
- Enhanced network reliability
  - Increased default timeout from 30s to 120s and optimized retry settings (#68)
- Fixed provider issues
  - Resolved OTX provider parsing bug for null values (#70)
  - Fixed connectivity issues with Wayback Machine, Common Crawl, and OTX (#62)

## 0.5.0

- Added robots.txt discovery functionality by [@Adesoji1](https://github.com/Adesoji1)
- Added sitemap.xml discovery functionality
- Added `--strict` flag - Enforce exact host validation (default is true)

## 0.4.0

- Added `--config` flag - Load configuration from a specified file
- Support to vt provider(Virustotal) - Search URLs from Virustotal API
  - Added `--vt-api-key` flag and `URX_VT_API_KEY` - Specify API key for Virustotal
- Support to urlscan provider - Search URLs from Urlscan API
  - Added `--urlscan-api-key` flag and `URX_URLSCAN_API_KEY` - Specify API key for Urlscan
- Improve performance

## 0.3.0

- Added `--insecure` - Skip SSL certificate verification
- Added `--network-scope` - Control which components network settings apply to
- Added status filtering options:
  - `--include-status` - Filter URLs by specific HTTP status codes (aliases: `--is`)
  - `--exclude-status` - Exclude URLs with specific HTTP status codes (aliases: `--es`)

## 0.2.0

- Added display control options:
  - `--silent` - Run in silent mode with no output
  - `--no-progress` - Disable progress bar display
- Added `--preset` - Apply predefined URL filters (e.g., no-resources, only-js)

## 0.1.0

- Initial release
- Project foundation established
