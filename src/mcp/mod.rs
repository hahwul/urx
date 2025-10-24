//! MCP (Model Context Protocol) server implementation for URX
//!
//! This module implements an MCP server that exposes URX functionality as tools
//! that can be used by AI assistants and other MCP clients.

#[cfg(feature = "mcp")]
pub mod server;

#[cfg(feature = "mcp")]
pub use server::UrxMcpServer;
