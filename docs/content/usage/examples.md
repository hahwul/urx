---
title: "Examples"
weight: 3
---

## Usage Examples

### Basic Usage

#### Single Domain
```bash
urx example.com
```

#### Multiple Domains
```bash
urx example.com example.org
```

#### From Standard Input
```bash
cat domains.txt | urx
```

#### File Input
```bash
# Single file
urx --files urls.txt

# Multiple files
urx --files urls.txt archive.warc data.gz
urx --files urls.txt --files archive.warc

# With filters
urx --files data.txt --patterns api,admin -f json
```

## Output Formats

### Save to File
```bash
urx example.com -o results.txt
```

### JSON Format
```bash
urx example.com -f json -o results.json
```

### CSV Format
```bash
urx example.com -f csv -o results.csv
```

## Filtering Examples

### Include Specific Extensions
```bash
# JavaScript files only
urx example.com -e js

# Multiple extensions
urx example.com -e js,php,aspx
```

### Exclude Extensions
```bash
urx example.com --exclude-extensions html,txt
```

### Pattern Matching
```bash
# Include patterns
urx example.com --patterns api,v1,graphql

# Exclude patterns
urx example.com --exclude-patterns static,images

# Combined
urx example.com --patterns api --exclude-patterns test,dev
```

### Using Presets
```bash
# Exclude images
urx example.com -p no-images

# Exclude all resources (images, CSS, fonts, etc.)
urx example.com -p no-resources

# JavaScript files only
urx example.com -p only-js
```

### Advanced Filtering
```bash
# Multiple filters
urx example.com -e js,php --patterns admin,login --min-length 20

# URL length constraints
urx example.com --min-length 50 --max-length 200
```

## Provider Selection

### Specific Providers
```bash
# Only Wayback Machine and OTX
urx example.com --providers wayback,otx

# All available providers
urx example.com --providers wayback,cc,otx,vt,urlscan
```

### With API Keys

#### Command Line
```bash
urx example.com --vt-api-key=YOUR_KEY --urlscan-api-key=YOUR_KEY
```

#### Environment Variables
```bash
export URX_VT_API_KEY=YOUR_KEY
export URX_URLSCAN_API_KEY=YOUR_KEY
urx example.com --providers=vt,urlscan
```

#### API Key Rotation
```bash
# Multiple keys for rate limit distribution
urx example.com --vt-api-key=key1 --vt-api-key=key2 --vt-api-key=key3

# Or with environment variable
URX_VT_API_KEY=key1,key2,key3 urx example.com
```

## Discovery Options

### Exclude Discovery Features
```bash
# Exclude robots.txt
urx example.com --exclude-robots

# Exclude sitemap.xml
urx example.com --exclude-sitemap

# Exclude both
urx example.com --exclude-robots --exclude-sitemap
```

## Testing & Validation

### Include Subdomains
```bash
urx example.com --subs
```

### Check HTTP Status
```bash
urx example.com --check-status
```

### Extract Links
```bash
urx example.com --extract-links
```

### Status Filtering
```bash
# Include only successful responses
urx example.com --check-status --include-status 200

# Include redirects and success
urx example.com --check-status --include-status 200,30x

# Exclude errors
urx example.com --check-status --exclude-status 404,50x
```

## Network Configuration

### Proxy Usage
```bash
urx example.com --proxy http://localhost:8080
```

### Proxy with Authentication
```bash
urx example.com --proxy http://localhost:8080 --proxy-auth username:password
```

### Custom Timeouts and Parallelism
```bash
urx example.com --timeout 60 --parallel 10
```

### Skip SSL Verification
```bash
urx example.com --insecure
```

### Random User-Agent
```bash
urx example.com --random-agent
```

### Complete Network Configuration
```bash
urx example.com \
  --proxy http://localhost:8080 \
  --timeout 60 \
  --parallel 10 \
  --retries 5 \
  --insecure \
  --random-agent
```

## URL Normalization

### Basic Normalization
```bash
urx example.com --normalize-url
```

### With Endpoint Merging
```bash
urx example.com --normalize-url --merge-endpoint
```

### From File
```bash
urx --files urls.txt --normalize-url
```

## Caching & Incremental Scanning

### SQLite Cache (Default)
```bash
urx example.com --cache-type sqlite --cache-path ~/.urx/cache.db
```

### Redis Cache
```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

### Incremental Mode
```bash
# Only return new URLs not seen before
urx example.com --incremental
```

### Custom TTL
```bash
# Set cache TTL to 12 hours (43200 seconds)
urx example.com --cache-ttl 43200
```

### Disable Cache
```bash
urx example.com --no-cache
```

### Combined Caching Examples
```bash
# Daily monitoring with incremental updates
urx target.com --incremental --silent | notify-tool

# Distributed scanning with shared Redis cache
urx example.com --cache-type redis --redis-url redis://shared-cache:6379

# Rapid iterations with short cache TTL
urx test-domain.com --cache-ttl 300

# Incremental scan with filtering
urx example.com --incremental -e js,php --patterns api
```

## Pipeline Integration

### Filter with grep
```bash
echo "example.com" | urx | grep "login" > targets.txt
```

### Chain with Other Tools
```bash
cat domains.txt | urx --patterns api | other-tool
```

### Security Tool Integration
```bash
# With Nuclei for XSS scanning
urx example.com -e js | nuclei -t xss

# With httpx for HTTP probing
urx example.com | httpx -silent

# With gf patterns
urx example.com | gf xss
```

## Display Options

### Verbose Output
```bash
urx example.com -v
```

### Silent Mode
```bash
urx example.com --silent
```

### Disable Progress Bar
```bash
urx example.com --no-progress
```

## Complex Scenarios

### Complete Bug Bounty Workflow
```bash
urx target.com \
  --subs \
  -e js,json,xml \
  --patterns api,v1,v2,admin,panel \
  --exclude-patterns cdn,static \
  --check-status \
  --include-status 200,30x \
  --incremental \
  --parallel 15 \
  -o results.txt
```

### API Endpoint Discovery
```bash
urx example.com \
  --patterns api,graphql,rest,v1,v2,v3 \
  -e json,xml \
  --exclude-patterns test,staging \
  -f json \
  -o api-endpoints.json
```

### JavaScript Analysis Pipeline
```bash
urx target.com \
  -p only-js \
  --check-status \
  --include-status 200 \
  --extract-links \
  | tee js-files.txt \
  | nuclei -t exposures/
```
