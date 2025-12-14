---
title: "Integration"
weight: 4
---

### Pipeline Examples

```bash
# Filter with grep
echo "example.com" | urx | grep "login" > targets.txt

# Chain with other tools
cat domains.txt | urx --patterns api | other-tool

# Combine with security tools
urx example.com -e js | nuclei -t xss
```
