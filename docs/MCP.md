# URX MCP Server

URX can run as a Model Context Protocol (MCP) server, allowing AI assistants and other MCP clients to extract URLs from OSINT archives.

## What is MCP?

The Model Context Protocol (MCP) is an open protocol that enables seamless integration between AI applications and data sources. URX implements an MCP server that exposes URL extraction functionality as tools that can be called by AI assistants.

## Installation

Build URX with MCP support:

```bash
cargo build --release --features mcp
```

## Usage

### Starting the MCP Server

Run URX in MCP mode:

```bash
urx --mcp
```

The server will communicate via stdin/stdout using the MCP protocol.

### Configuration

API keys can be configured via environment variables:

```bash
# VirusTotal API key
export URX_VT_API_KEY=your_vt_api_key_here

# URLScan API key
export URX_URLSCAN_API_KEY=your_urlscan_api_key_here

# Start the MCP server
urx --mcp
```

Multiple API keys (for rotation) can be provided as comma-separated values:

```bash
export URX_VT_API_KEY=key1,key2,key3
```

### Using with MCP Clients

#### Claude Desktop

Add to your Claude Desktop configuration (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "urx": {
      "command": "/path/to/urx",
      "args": ["--mcp"],
      "env": {
        "URX_VT_API_KEY": "your_vt_api_key_here",
        "URX_URLSCAN_API_KEY": "your_urlscan_api_key_here"
      }
    }
  }
}
```

#### MCP Inspector

Test the server with the MCP Inspector:

```bash
npx @modelcontextprotocol/inspector /path/to/urx --mcp
```

## Available Tools

### fetch_urls

Extract URLs from OSINT archives for a domain.

**Parameters:**
- `domain` (required): Domain to fetch URLs for
- `providers` (optional): Comma-separated list of providers (wayback, cc, otx, vt, urlscan)
- `include_subdomains` (optional): Include subdomains when searching
- `extensions` (optional): Filter URLs by file extensions (comma-separated)
- `exclude_extensions` (optional): Exclude URLs by file extensions
- `patterns` (optional): Filter URLs by patterns (comma-separated)
- `exclude_patterns` (optional): Exclude URLs by patterns
- `limit` (optional): Maximum number of URLs to return

**Example:**
```json
{
  "domain": "example.com",
  "providers": "wayback,cc,otx",
  "extensions": "js,php",
  "limit": 100
}
```

### list_providers

List all available OSINT URL providers and their status.

**Parameters:** None

**Example:**
```json
{}
```

## Default Providers

The following providers are available by default (no API key required):
- **Wayback Machine**: Internet Archive's historical web snapshots
- **Common Crawl**: Open repository of web crawl data
- **OTX (AlienVault Open Threat Exchange)**: Community-driven threat intelligence

The following providers require API keys:
- **VirusTotal**: Requires `URX_VT_API_KEY` environment variable
- **URLScan**: Requires `URX_URLSCAN_API_KEY` environment variable

## Architecture

The MCP server implementation:
- Uses the official Rust MCP SDK ([rmcp](https://github.com/modelcontextprotocol/rust-sdk))
- Communicates via stdio transport
- Exposes URX functionality as MCP tools
- Supports async operations with tokio runtime
- Maintains compatibility with existing URX CLI functionality

## Security Notes

- API keys are only stored in memory during server runtime
- The server runs in silent mode with caching disabled by default
- No data is persisted between tool calls unless explicitly configured

## Troubleshooting

### Server Won't Start

Make sure you built with the `mcp` feature:
```bash
cargo build --release --features mcp
```

### Provider Not Working

Check that API keys are properly set:
```bash
echo $URX_VT_API_KEY
echo $URX_URLSCAN_API_KEY
```

Use the `list_providers` tool to check provider status.

### No URLs Returned

- Verify the domain is correct
- Try different providers
- Check network connectivity
- Some domains may have no historical data in OSINT archives

## Examples

### Basic URL Extraction

Use the `fetch_urls` tool with just a domain:
```json
{
  "domain": "example.com"
}
```

### Advanced Filtering

Extract only JavaScript files from a specific domain:
```json
{
  "domain": "example.com",
  "providers": "wayback,cc",
  "extensions": "js",
  "include_subdomains": true,
  "limit": 50
}
```

### Using Multiple Providers

Combine multiple OSINT sources:
```json
{
  "domain": "example.com",
  "providers": "wayback,cc,otx,vt,urlscan"
}
```

## Related Resources

- [MCP Specification](https://spec.modelcontextprotocol.io/)
- [MCP Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
- [URX Documentation](https://github.com/hahwul/urx)
