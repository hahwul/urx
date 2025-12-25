---
title: "Troubleshooting"
weight: 1
---

## Common Issues and Solutions

This section will contain troubleshooting guides for common issues encountered when using Urx.

### Coming Soon

Troubleshooting documentation is being compiled based on user feedback and common issues. This will include:

- **Installation Issues**: Solutions for build and installation problems
- **Provider Errors**: Handling API rate limits and connectivity issues
- **Performance Problems**: Diagnosing slow scans and timeouts
- **Output Issues**: Troubleshooting incorrect or missing results
- **Network Configuration**: Proxy and firewall issues
- **Cache Problems**: SQLite and Redis cache troubleshooting

### Getting Help

While this documentation is being developed, you can:

1. **Check GitHub Issues**: Search [existing issues](https://github.com/hahwul/urx/issues) for similar problems
2. **Create a New Issue**: [Report bugs](https://github.com/hahwul/urx/issues/new) with detailed information
3. **Review FAQ**: See the [FAQ page](../faq) for frequently asked questions
4. **Check Discussions**: Join [GitHub Discussions](https://github.com/hahwul/urx/discussions) for community help

### Quick Diagnostics

#### Verbose Mode
Enable verbose output to see detailed execution information:
```bash
urx example.com -v
```

#### Test Installation
Verify Urx is installed correctly:
```bash
urx --version
urx --help
```

#### Network Connectivity
Test with a single provider:
```bash
urx example.com --providers wayback
```

### Reporting Issues

When reporting issues, please include:
- Urx version (`urx --version`)
- Operating system and version
- Command used
- Complete error message
- Verbose output if applicable (`-v` flag)

---

**Note**: This page will be updated with comprehensive troubleshooting guides as issues are identified and solutions are documented.
