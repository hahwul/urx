---
title: "Performance Tips"
weight: 2
---

## Optimizing Urx Performance

### Parallel Processing

#### Adjust Parallelism
```bash
# Default parallelism (5 concurrent requests)
urx example.com

# Increase for faster processing (but more resource usage)
urx example.com --parallel 20

# Decrease for rate-limit sensitive targets
urx example.com --parallel 2
```

The `--parallel` flag controls both:
- Maximum concurrent requests per provider
- Maximum concurrent domain processing

**Recommendations:**
- Fast connections: `--parallel 15-20`
- Normal connections: `--parallel 5-10`
- Slow/rate-limited: `--parallel 2-3`

### Caching Strategies

#### Enable Incremental Scanning
```bash
# First scan (builds cache)
urx example.com --incremental -o initial.txt

# Subsequent scans (only new URLs)
urx example.com --incremental -o new-urls.txt
```

**Benefits:**
- Dramatically faster subsequent scans
- Only fetches and processes new data
- Perfect for continuous monitoring

#### SQLite Cache (Local)
```bash
# Default location
urx example.com --incremental

# Custom location
urx example.com --incremental --cache-path /path/to/cache.db
```

**Best for:**
- Single-machine scanning
- Local development
- Personal projects

#### Redis Cache (Distributed)
```bash
urx example.com --cache-type redis --redis-url redis://localhost:6379
```

**Best for:**
- Team environments
- Distributed scanning across multiple machines
- High-performance scenarios
- Kubernetes/container deployments

#### Cache TTL Optimization
```bash
# Short TTL for frequently changing targets (5 minutes)
urx example.com --cache-ttl 300

# Medium TTL for daily scans (12 hours)
urx example.com --cache-ttl 43200

# Long TTL for stable targets (7 days)
urx example.com --cache-ttl 604800
```

### Network Optimization

#### Timeout Configuration
```bash
# Fast timeout for quick scans (may miss some results)
urx example.com --timeout 15

# Extended timeout for slow providers
urx example.com --timeout 120
```

#### Retry Settings
```bash
# Fewer retries for speed
urx example.com --retries 1

# More retries for reliability
urx example.com --retries 5
```

#### Network Scope
Control which components use network settings:
```bash
# Apply to all components (default)
urx example.com --network-scope all --parallel 20

# Only providers (faster URL collection)
urx example.com --network-scope providers --parallel 20

# Only testers (for status checking)
urx example.com --network-scope testers --check-status --parallel 10
```

### Provider Selection

#### Choose Fast Providers
```bash
# Only fast providers (Wayback Machine is generally fastest)
urx example.com --providers wayback

# Exclude slow providers when speed is critical
urx example.com --providers wayback,cc
```

#### API Key Rotation
Distribute load across multiple API keys:
```bash
# Multiple VirusTotal keys
urx example.com \
  --vt-api-key=key1 \
  --vt-api-key=key2 \
  --vt-api-key=key3 \
  --providers vt
```

**Benefits:**
- Bypass rate limits
- Faster scanning with multiple keys
- Better reliability

### Filtering Early

#### Filter at Collection Time
```bash
# Filter during collection (more efficient)
urx example.com -e js,php --patterns api

# Instead of filtering after
urx example.com | grep "api" | grep "\.js$"
```

**Why it's faster:**
- Less data to process
- Reduced memory usage
- Faster output generation

#### Use Presets
```bash
# Preset filters are optimized
urx example.com -p no-images,no-resources

# More efficient than manual exclusion
urx example.com --exclude-extensions jpg,png,gif,css,woff,woff2,ttf
```

### Output Optimization

#### Disable Progress Bar
```bash
# For scripts and pipelines
urx example.com --no-progress
```

#### Silent Mode
```bash
# Minimal output overhead
urx example.com --silent -o results.txt
```

#### Direct Output
```bash
# Avoid large files in memory
urx example.com -o results.txt
# Instead of: urx example.com > results.txt
```

### Resource Management

#### Memory Optimization
```bash
# Process domains one at a time with lower parallelism
cat large-list.txt | urx --parallel 3 --no-progress
```

#### Disk Space
```bash
# Clear old cache periodically
rm ~/.urx/cache.db

# Or use shorter TTL
urx example.com --cache-ttl 3600
```

### Batch Processing

#### Process Multiple Domains Efficiently
```bash
# Stream processing (lower memory)
cat domains.txt | urx --incremental --no-progress -o results.txt

# Parallel domain processing
cat domains.txt | xargs -P 3 -I {} urx {} --incremental -o {}.txt
```

### Performance Monitoring

#### Verbose Mode
```bash
# See timing and debug information
urx example.com -v
```

#### Measure Performance
```bash
# Time the execution
time urx example.com -o results.txt

# With different configurations
time urx example.com --parallel 5 -o results-5.txt
time urx example.com --parallel 20 -o results-20.txt
```

## Best Practices by Use Case

### Rapid Testing
```bash
urx example.com \
  --providers wayback \
  --parallel 20 \
  --timeout 15 \
  --retries 1 \
  --no-progress
```

### Production Monitoring
```bash
urx example.com \
  --incremental \
  --cache-type redis \
  --redis-url redis://cache:6379 \
  --parallel 10 \
  --timeout 60 \
  --retries 3 \
  --silent
```

### Comprehensive Discovery
```bash
urx example.com \
  --providers wayback,cc,otx,vt,urlscan \
  --subs \
  --parallel 15 \
  --timeout 120 \
  --retries 5 \
  --cache-type sqlite \
  --incremental
```

### Resource-Constrained Environment
```bash
urx example.com \
  --providers wayback \
  --parallel 2 \
  --timeout 30 \
  --no-cache \
  --no-progress
```

## Benchmarking

### Compare Configurations
```bash
#!/bin/bash
DOMAIN="example.com"

echo "Testing different parallel settings..."
for parallel in 5 10 15 20; do
  echo "Parallel: $parallel"
  time urx $DOMAIN --parallel $parallel --no-cache -o /dev/null
done
```

### Monitor System Resources
```bash
# CPU and memory usage
top -p $(pgrep urx)

# Network usage
iftop
```

## Troubleshooting Performance Issues

### Slow Scans
1. Increase `--parallel` value
2. Reduce `--timeout` if appropriate
3. Use `--incremental` for subsequent scans
4. Select faster providers only
5. Filter early to reduce processing

### High Memory Usage
1. Decrease `--parallel` value
2. Process fewer domains simultaneously
3. Use streaming output (`-o file`)
4. Clear cache if very large

### Rate Limiting
1. Reduce `--parallel` value
2. Increase `--timeout` value
3. Use `--rate-limit` flag if available
4. Implement API key rotation

### Network Timeouts
1. Increase `--timeout` value
2. Increase `--retries` value
3. Check network connectivity
4. Try different providers
