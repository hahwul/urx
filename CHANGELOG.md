# Changelog

## Unreleased

- Added `--config` - Load configuration from a specified file
- Support to vt provider(Virustotal) - Search URLs from Virustotal API
  - Added `--vt-api-key` flag and `URX_VT_API_KEY` - Specify API key for Virustotal integration

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