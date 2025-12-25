---
title: "Configuration"
weight: 2
---

## Configuration File Support

Urx supports loading configuration from TOML files for complex setups. This feature is planned for future releases.

### Coming Soon

The configuration file feature will allow you to:

- Define default settings in a configuration file
- Store API keys securely
- Set up multiple profiles for different scanning scenarios
- Share configuration across team members

### Current Workaround

For now, you can use shell aliases or wrapper scripts to set default options:

```bash
# Add to your .bashrc or .zshrc
alias urx-api='urx --patterns api,v1,v2,graphql'
alias urx-js='urx -e js,jsx --check-status'
```

### Example Configuration File

See the `example/config.toml` file in the repository for a reference configuration structure:

```bash
cat example/config.toml
```

This documentation will be updated once the configuration feature is fully implemented.
