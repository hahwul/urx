---
title: "Integration"
weight: 4
---

Urx is designed to work seamlessly with other command-line tools, allowing you to create powerful and customized security testing workflows.

### Pipelining

You can pipe the output of Urx to other tools for further processing. For example, you can use `grep` to filter for specific keywords:

```bash
echo "example.com" | urx | grep "login" > potential_targets.txt
```

### Chaining with Other Tools

You can also chain Urx with other security and reconnaissance tools. For instance, you can use the output of a domain discovery tool as input for Urx:

```bash
cat domains.txt | urx --patterns api | other-tool
```

This allows you to create a flexible and modular approach to your security assessments, where each tool performs a specific task in the pipeline.
