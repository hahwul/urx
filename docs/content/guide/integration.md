+++
title = "Integration"
description = "Pipe urx into other recon tooling, notify on results, and run it as continuous monitoring."
toc = true
weight = 6
+++

## Pipeline Integration

Urx is designed to work seamlessly in command-line pipelines and with other security tools.

### Standard Input/Output

Urx reads domains from standard input and outputs URLs to standard output, making it perfect for piping:

```bash
cat domains.txt | urx | grep "api"

# Or name the file directly (alias --dL); repeatable
urx --domain-list domains.txt | grep "api"
```

Standard input is read only when the command line and any `--domain-list` files
name no domains at all.

Results go to stdout; the progress bar, warnings and errors go to stderr, and the
progress bar hides itself when stderr is not a terminal. Two things to keep in
mind in a pipe:

- `--silent` suppresses **all** output, results included (except under
  `--stream`, which still writes results and only loses its diagnostics). Use it
  only with `-o` or `--notify`; to quiet a pipe, use `--no-progress` instead.
- `-v` / `--verbose` writes its setup and stage messages to stdout, mixed into
  the URL list, where the next tool would read them as URLs (its per-provider
  progress and errors go to stderr). Leave it off in pipelines.

For JSON consumers, `-f jsonl` writes one object per line, while `-f json` writes
a single array. `--stream` starts writing before the run ends, so the next tool
starts working at once (plain, `jsonl` and `csv` only; unsorted; bypasses the
cache):

```bash
urx example.com --stream | httpx -silent
```

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | The run completed, including runs where some providers failed, results were partial, or the webhook could not be delivered (an unwritable `-o` under `--stream` is caught earlier as an exit-1 startup error) |
| `1` | A runtime error: no domains given, a rejected option combination (e.g. `--stream` with `--incremental`), a cache backend that cannot be opened, `urx cache clear` without a terminal or `--yes`, or a failed `-o` / `--output-dir` write |
| `2` | Invalid command-line usage, such as an unknown flag or `--parallel 0` |
| `130` | A Ctrl-C after collection has ended exits immediately. During provider collection, the first Ctrl-C stops fetching gracefully and returns collected URLs; a second Ctrl-C during the remaining pipeline force-quits |

In batch mode, urx attempts both `-o` and `--output-dir` when both are set. It
then attempts configured `--notify` webhooks before returning any output error,
which exits 1 and is reported even with `--silent`. Stdout-only runs keep their
existing behavior: a closed pipe is treated as successful output completion.

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

`--fuzz-placeholder` hands ffuf the parameter templates directly — every query
value replaced, one URL per parameter signature:
```bash
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ
```

`-f wordlist` turns the run into a target-specific wordlist instead — the path
segments and parameter names the target is built from, with ids, hashes and
dates left out:
```bash
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ
```

#### dalfox
Parameter templates feed a scanner just as well as a fuzzer:
```bash
urx example.com --fuzz-placeholder FUZZ | dalfox pipe
```

#### waybackurls / gau
Combine with other URL collection tools:
```bash
(urx example.com && gau example.com) | sort -u
```

### Notification Integration

#### Built-in Webhook (`--notify`)
urx can POST a run summary itself — no extra tool in the pipe. Paired with
`--incremental` the webhook fires only when the run finds new URLs:
```bash
# Slack (--silent keeps stdout quiet; the webhook still fires)
urx target.com --incremental --silent \
  --notify https://hooks.slack.com/services/T000/B000/XXXX --notify-format slack

# Discord
urx target.com --incremental --silent --notify "$DISCORD_HOOK" --notify-format discord

# Anything that takes JSON (n8n, ntfy, a Lambda, your own receiver)
export URX_NOTIFY_URL=https://n8n.example/webhook/urx
urx target.com --incremental --silent
```
See [CLI Options → Webhook Notifications](/guide/cli-options/#webhook-notifications)
for the payload schema, `--notify-on`, and the length limits.

#### Notify
Send the new URLs themselves, one per message, through an external notifier:
```bash
urx target.com --incremental --no-progress | notify -silent
```

#### Discord Webhook (per URL)
```bash
urx example.com | while read url; do
  curl -X POST "webhook_url" -d "{\"content\":\"$url\"}"
done
```

### Database Integration

#### PostgreSQL
Store results in a database:
```bash
urx example.com -f jsonl | jq -r '.url' | while read url; do
  psql -c "INSERT INTO urls (url) VALUES ('$url')"
done
```

#### MongoDB
```bash
urx example.com -f jsonl | mongoimport --db security --collection urls
```

### Continuous Monitoring

#### Daily Cron Job
Monitor targets daily for new URLs:
```bash
# Add to crontab
0 0 * * * /usr/local/bin/urx target.com --incremental --no-progress >> /var/log/urx.log 2>/dev/null
```

#### With Redis for Distributed Scanning
```bash
urx example.com --cache-type redis --redis-url redis://central-cache:6379 --incremental
```

Redis needs a build with `--features redis-cache`, and the machines only share
cache entries when they run with the same flags and the same keyed providers
enabled. See
[Caching](/guide/caching/).

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
      # Without a persisted cache, every run on a fresh runner is a first run
      # and --incremental reports everything.
      - name: Restore URL cache
        uses: actions/cache@v4
        with:
          path: ~/.urx
          key: urx-cache-${{ github.run_id }}
          restore-keys: urx-cache-
      - name: Run Discovery
        run: urx example.com --incremental -o results.txt
      - name: Upload Results
        uses: actions/upload-artifact@v4
        with:
          name: urls
          path: results.txt
```

### Docker Integration

The image has no entrypoint: its default command is `./urx`, so arguments
passed to the container must start with `./urx` (or use `--entrypoint ./urx`).

#### Run in Container
```bash
docker run --rm \
  -v "$(pwd)":/data \
  ghcr.io/hahwul/urx:latest \
  ./urx example.com -o /data/results.txt
```

The container runs as uid 100 (`app`). On Linux, make the mounted directory
writable by it, or run with `--user "$(id -u):$(id -g)" -e HOME=/tmp` (the
`HOME` override keeps the cache writable). A failed `-o` or `--output-dir`
write is reported on stderr and exits 1, including under `--silent`.

#### Docker Compose for Monitoring Stack

The published image is built without Redis support, so keep the SQLite cache on a
volume instead (the container runs as the `app` user, whose home is `/home/app`):

```yaml
services:
  urx:
    image: ghcr.io/hahwul/urx:latest
    command: ["./urx", "example.com", "--incremental", "-o", "/data/new-urls.txt"]
    volumes:
      - urx-cache:/home/app   # a new volume inherits the image's app:app ownership here
      - ./data:/data
volumes:
  urx-cache:
```

For a shared Redis cache, build your own image with
`cargo build --release --features redis-cache` and add
`--cache-type redis --redis-url redis://redis:6379`.

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
            command: ["./urx"]
            args: ["example.com", "--incremental", "--silent",
                   "--notify-format", "slack"]
            env:
            - name: URX_NOTIFY_URL
              valueFrom:
                secretKeyRef: {name: urx-secrets, key: notify-url}
            volumeMounts:
            - name: cache
              mountPath: /home/app/.urx   # keeps the --incremental baseline between runs
          volumes:
          - name: cache
            persistentVolumeClaim:
              claimName: urx-cache
          securityContext:
            fsGroup: 101   # the image's `app` group, so the volume is writable
          restartPolicy: OnFailure
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
