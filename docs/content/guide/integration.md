+++
title = "Integration"
description = "Pipe urx into other recon tooling, notify on results, and run it as continuous monitoring."
toc = true
weight = 6
+++

## Pipeline Integration

### Standard Input/Output

```bash
cat domains.txt | urx | grep "api"

# or name the file (alias --dL); repeatable
urx --domain-list domains.txt | grep "api"
```

Stdin is read only when the command line and `--domain-list` files name no
domains.

Results go to stdout. The progress bar, warnings and errors go to stderr, and
the progress bar hides itself when stderr is not a terminal. In a pipe:

- `--silent` hides all output, results included (except under `--stream`, which
  still writes results). Use it only with `-o` or `--notify`. To quiet a pipe,
  use `--no-progress`.
- `-v` / `--verbose` writes setup and stage messages to stdout, mixed into the
  URL list (per-provider progress and errors go to stderr). Leave it off in
  pipelines.

`-f jsonl` writes one object per line; `-f json` writes a single array.
`--stream` writes before the run ends (plain, `jsonl` and `csv` only; unsorted;
skips the cache):

```bash
urx example.com --stream | httpx -silent
```

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Run completed, even if some providers failed, results were partial, or the webhook was not delivered (an unwritable `-o` under `--stream` is caught at startup, and a write that fails mid-stream also exits 1) |
| `1` | Runtime error: no domains, a rejected option combination (e.g. `--stream` with `--incremental`), a cache backend that cannot be opened, `urx cache clear` without a terminal or `--yes`, or a failed `-o` / `--output-dir` write |
| `2` | Invalid usage, such as an unknown flag or `--parallel 0` |
| `130` | Ctrl-C after collection exits at once. During collection, the first Ctrl-C stops fetching and keeps the URLs collected so far; a second Ctrl-C force-quits |

In batch mode, urx tries both `-o` and `--output-dir` when both are set, then
sends any `--notify` webhooks, then returns the output error (exit 1, reported
even with `--silent`). For stdout-only runs, a closed pipe counts as success.

### With Security Tools

```bash
# nuclei on JS files
urx example.com -e js | nuclei -t xss

# httpx probing
urx example.com | httpx -silent -status-code

# gf patterns
urx example.com | gf xss
urx example.com | gf redirect
urx example.com | gf ssrf

# ffuf: each URL as the target
urx example.com --patterns api | ffuf -w - -u FUZZ

# ffuf / dalfox: one URL per parameter signature, values replaced
urx example.com --fuzz-placeholder FUZZ | ffuf -w - -u FUZZ
urx example.com --fuzz-placeholder FUZZ | dalfox pipe

# target-specific wordlist (path segments and parameter names)
urx example.com --subs -f wordlist -o words.txt
ffuf -w words.txt -u https://example.com/FUZZ

# merge with gau
(urx example.com && gau example.com) | sort -u
```

### Notification Integration

#### Built-in Webhook (`--notify`)
urx POSTs a run summary itself. With `--incremental`, it fires only when the run
finds new URLs:
```bash
# Slack (--silent keeps stdout quiet; the webhook still fires)
urx target.com --incremental --silent \
  --notify https://hooks.slack.com/services/T000/B000/XXXX --notify-format slack

# Discord
urx target.com --incremental --silent --notify "$DISCORD_HOOK" --notify-format discord

# anything that takes JSON (n8n, ntfy, a Lambda, your own receiver)
export URX_NOTIFY_URL=https://n8n.example/webhook/urx
urx target.com --incremental --silent
```
See [CLI Options → Webhook Notifications](/guide/cli-options/#webhook-notifications)
for the payload schema, `--notify-on`, and the length limits.

#### Notify
```bash
# one message per new URL
urx target.com --incremental --no-progress | notify -silent
```

### Database Integration

```bash
# PostgreSQL
urx example.com | psql -c "\copy urls(url) FROM pstdin"

# MongoDB
urx example.com -f jsonl | mongoimport --db security --collection urls
```

### Continuous Monitoring

#### Daily Cron Job
```bash
0 0 * * * /usr/local/bin/urx target.com --incremental --no-progress >> /var/log/urx.log 2>/dev/null
```

#### With Redis for Distributed Scanning
```bash
urx example.com --cache-type redis --redis-url redis://central-cache:6379 --incremental
```

Needs a build with `--features redis-cache`. Machines share cache entries only
when they run with the same flags and the same keyed providers. See
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
      # without a persisted cache every run is a first run,
      # and --incremental reports everything
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

The image has no entrypoint (its default command is `./urx`), so container
arguments must start with `./urx`, or use `--entrypoint ./urx`.

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
write goes to stderr and exits 1, even under `--silent`.

#### Docker Compose for Monitoring Stack

The published image has no Redis support, so keep the SQLite cache on a volume
(the `app` user's home is `/home/app`):

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

urx "$TARGET" --subs -e js,json,xml -o urls.txt
httpx -silent -l urls.txt -o live.txt
nuclei -l live.txt -t cves/ -o vulnerabilities.txt

# grep JS files for key-looking strings
grep '\.js$' urls.txt | while read -r url; do
  curl -s "$url" | grep -i "api.*key"
done
```

#### Bug Bounty Automation
```bash
#!/bin/bash
TARGET=$1

# new URLs only; Slack summary when there are any
urx "$TARGET" --subs --incremental -o new-urls.txt \
  --notify "$SLACK_WEBHOOK" --notify-format slack

gf redirect < new-urls.txt > potential-redirects.txt
gf xss < new-urls.txt > potential-xss.txt
gf sqli < new-urls.txt > potential-sqli.txt
```
