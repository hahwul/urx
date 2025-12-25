---
title: "Integration"
weight: 1
---

## Pipeline Integration

Urx is designed to work seamlessly in command-line pipelines and with other security tools.

### Standard Input/Output

Urx reads domains from standard input and outputs URLs to standard output, making it perfect for piping:

```bash
cat domains.txt | urx | grep "api"
```

### With Security Tools

#### Nuclei
Scan for vulnerabilities in discovered JavaScript files:
```bash
urx example.com -e js | nuclei -t xss
```

#### httpx
Probe discovered URLs for HTTP information:
```bash
urx example.com | httpx -silent -status-code
```

#### gf (Go Filters)
Filter URLs for specific patterns:
```bash
urx example.com | gf xss
urx example.com | gf redirect
urx example.com | gf ssrf
```

#### ffuf
Fuzz discovered endpoints:
```bash
urx example.com --patterns api | ffuf -w - -u FUZZ
```

#### waybackurls / gau
Combine with other URL collection tools:
```bash
(urx example.com && gau example.com) | sort -u
```

### Notification Integration

#### Notify
Send new URLs to various notification channels:
```bash
urx target.com --incremental --silent | notify -silent
```

#### Discord Webhook
```bash
urx example.com | while read url; do
  curl -X POST "webhook_url" -d "{\"content\":\"$url\"}"
done
```

### Database Integration

#### PostgreSQL
Store results in a database:
```bash
urx example.com -f json | jq -r '.url' | while read url; do
  psql -c "INSERT INTO urls (url) VALUES ('$url')"
done
```

#### MongoDB
```bash
urx example.com -f json | mongoimport --db security --collection urls
```

### Continuous Monitoring

#### Daily Cron Job
Monitor targets daily for new URLs:
```bash
# Add to crontab
0 0 * * * /usr/local/bin/urx target.com --incremental --silent >> /var/log/urx.log
```

#### With Redis for Distributed Scanning
```bash
urx example.com --cache-type redis --redis-url redis://central-cache:6379 --incremental
```

### CI/CD Integration

#### GitHub Actions
```yaml
name: URL Discovery
on:
  schedule:
    - cron: '0 0 * * *'
jobs:
  discover:
    runs-on: ubuntu-latest
    steps:
      - name: Install Urx
        run: cargo install urx
      - name: Run Discovery
        run: urx example.com --incremental -o results.txt
      - name: Upload Results
        uses: actions/upload-artifact@v3
        with:
          name: urls
          path: results.txt
```

### API Integration

#### Custom Processing
```bash
urx example.com -f json | python3 process_urls.py
```

Example Python script:
```python
import json
import sys

for line in sys.stdin:
    data = json.loads(line)
    # Process URL data
    print(f"Processing: {data['url']}")
```

### Docker Integration

#### Run in Container
```bash
docker run --rm \
  -v $(pwd):/data \
  ghcr.io/hahwul/urx:latest \
  example.com -o /data/results.txt
```

#### Docker Compose for Monitoring Stack
```yaml
version: '3'
services:
  urx:
    image: ghcr.io/hahwul/urx:latest
    command: example.com --cache-type redis --redis-url redis://redis:6379 --incremental
    depends_on:
      - redis
  redis:
    image: redis:alpine
    volumes:
      - redis-data:/data
volumes:
  redis-data:
```

### Kubernetes CronJob

```yaml
apiVersion: batch/v1
kind: CronJob
metadata:
  name: urx-scanner
spec:
  schedule: "0 0 * * *"
  jobTemplate:
    spec:
      template:
        spec:
          containers:
          - name: urx
            image: ghcr.io/hahwul/urx:latest
            args: ["example.com", "--incremental", "--silent"]
          restartPolicy: OnFailure
```

### Output Format Integration

#### JSON for jq Processing
```bash
urx example.com -f json | jq '.[] | select(.status == 200)'
```

#### CSV for Spreadsheet Import
```bash
urx example.com -f csv -o results.csv
# Import into Excel, Google Sheets, etc.
```

### Multi-Tool Workflows

#### Complete Reconnaissance Pipeline
```bash
#!/bin/bash
TARGET=$1

# Discover URLs
urx $TARGET --subs -e js,json,xml -o urls.txt

# Probe for live URLs
cat urls.txt | httpx -silent -o live.txt

# Scan for vulnerabilities
cat live.txt | nuclei -t cves/ -o vulnerabilities.txt

# Check for secrets in JS files
cat urls.txt | grep "\.js$" | while read url; do
  curl -s $url | grep -i "api.*key"
done
```

#### Bug Bounty Automation
```bash
#!/bin/bash
TARGET=$1

# Initial discovery
urx $TARGET --subs --incremental -o new-urls.txt

# Filter interesting endpoints
cat new-urls.txt | gf redirect > potential-redirects.txt
cat new-urls.txt | gf xss > potential-xss.txt
cat new-urls.txt | gf sqli > potential-sqli.txt

# Notify on Slack
if [ -s new-urls.txt ]; then
  COUNT=$(wc -l < new-urls.txt)
  curl -X POST $SLACK_WEBHOOK -d "{\"text\":\"Found $COUNT new URLs for $TARGET\"}"
fi
```
