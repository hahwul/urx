+++
title = "Performance"
weight = 7
+++

## Optimizing Urx Performance

### Parallel Processing

#### Adjust Parallelism
```bash
# Default parallelism (5 concurrent requests)
urx example.com

# Increase for faster processing
urx example.com --parallel 20

# Decrease for rate-limit sensitive targets
urx example.com --parallel 2
```

The `--parallel` flag controls both maximum concurrent requests per provider and maximum concurrent domain processing.

**Recommendations:**
- Fast connections: `--parallel 15-20`
- Normal connections: `--parallel 5-10`
- Slow/rate-limited: `--parallel 2-3`

### Network Optimization

#### Timeout Configuration
```bash
# Fast timeout for quick scans
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
# Only fast providers
urx example.com --providers wayback

# Exclude slow providers when speed is critical
urx example.com --providers wayback,cc
```

#### API Key Rotation
Distribute load across multiple API keys to bypass rate limits:
```bash
urx example.com \
  --vt-api-key=key1 \
  --vt-api-key=key2 \
  --vt-api-key=key3 \
  --providers vt
```

### Filtering Early

Filter at collection time rather than post-processing:
```bash
# More efficient — filter during collection
urx example.com -e js,php --patterns api

# Less efficient — filter after
urx example.com | grep "api" | grep "\.js$"
```

**Why it's faster:**
- Less data to process and deduplicate
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

```bash
# Disable progress bar for scripts
urx example.com --no-progress

# Silent mode — minimal output overhead
urx example.com --silent -o results.txt

# Direct file output — avoids buffering
urx example.com -o results.txt
```

### Batch Processing

```bash
# Stream processing (lower memory)
cat domains.txt | urx --incremental --no-progress -o results.txt

# Parallel domain processing
cat domains.txt | xargs -P 3 -I {} urx {} --incremental -o {}.txt
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
  --providers wayback,cc,otx,vt,urlscan,zoomeye \
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
2. Use `--rate-limit` flag
3. Implement API key rotation
4. Increase `--timeout` value

### Network Timeouts
1. Increase `--timeout` value
2. Increase `--retries` value
3. Check network connectivity
4. Try different providers
