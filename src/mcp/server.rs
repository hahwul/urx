//! URX MCP Server implementation
//!
//! Provides MCP tools for URL extraction from OSINT archives.

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cli::Args;
use crate::filters::UrlFilter;
use crate::progress::ProgressManager;
use crate::providers::{
    CommonCrawlProvider, OTXProvider, Provider, UrlscanProvider, VirusTotalProvider,
    WaybackMachineProvider,
};
use crate::runner::process_domains;

/// Arguments for the fetch_urls tool
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct FetchUrlsArgs {
    /// Domain to fetch URLs for
    pub domain: String,

    /// Providers to use (comma-separated: wayback, cc, otx, vt, urlscan)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers: Option<String>,

    /// Include subdomains when searching
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_subdomains: Option<bool>,

    /// Filter URLs by file extensions (comma-separated, e.g., "js,php")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<String>,

    /// Exclude URLs by file extensions (comma-separated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_extensions: Option<String>,

    /// Filter URLs by patterns (comma-separated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patterns: Option<String>,

    /// Exclude URLs by patterns (comma-separated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_patterns: Option<String>,

    /// Maximum number of URLs to return
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Arguments for the list_providers tool
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListProvidersArgs {}

/// URX MCP Server
#[derive(Clone)]
pub struct UrxMcpServer {
    tool_router: ToolRouter<UrxMcpServer>,
    // Store API keys for providers that need them
    vt_api_keys: Arc<Mutex<Vec<String>>>,
    urlscan_api_keys: Arc<Mutex<Vec<String>>>,
}

#[tool_router]
impl UrxMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            vt_api_keys: Arc::new(Mutex::new(Vec::new())),
            urlscan_api_keys: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set VirusTotal API keys for the server
    pub async fn set_vt_api_keys(&self, keys: Vec<String>) {
        let mut vt_keys = self.vt_api_keys.lock().await;
        *vt_keys = keys;
    }

    /// Set URLScan API keys for the server
    pub async fn set_urlscan_api_keys(&self, keys: Vec<String>) {
        let mut urlscan_keys = self.urlscan_api_keys.lock().await;
        *urlscan_keys = keys;
    }

    /// Fetch URLs from OSINT archives for a domain
    #[tool(
        description = "Extract URLs from OSINT archives (Wayback Machine, Common Crawl, OTX, etc.) for security analysis"
    )]
    async fn fetch_urls(
        &self,
        Parameters(args): Parameters<FetchUrlsArgs>,
    ) -> Result<CallToolResult, McpError> {
        // Parse providers from args or use defaults
        let providers_str = args
            .providers
            .unwrap_or_else(|| "wayback,cc,otx".to_string());
        let provider_names: Vec<String> = providers_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        // Initialize providers
        let mut providers: Vec<Box<dyn Provider>> = Vec::new();
        let mut active_provider_names = Vec::new();

        for provider_name in &provider_names {
            match provider_name.as_str() {
                "wayback" => {
                    let mut provider = WaybackMachineProvider::new();
                    if let Some(subs) = args.include_subdomains {
                        provider.with_subdomains(subs);
                    }
                    providers.push(Box::new(provider));
                    active_provider_names.push("Wayback Machine".to_string());
                }
                "cc" => {
                    let mut provider =
                        CommonCrawlProvider::with_index("CC-MAIN-2025-13".to_string());
                    if let Some(subs) = args.include_subdomains {
                        provider.with_subdomains(subs);
                    }
                    providers.push(Box::new(provider));
                    active_provider_names.push("Common Crawl".to_string());
                }
                "otx" => {
                    let mut provider = OTXProvider::new();
                    if let Some(subs) = args.include_subdomains {
                        provider.with_subdomains(subs);
                    }
                    providers.push(Box::new(provider));
                    active_provider_names.push("OTX".to_string());
                }
                "vt" => {
                    let vt_keys = self.vt_api_keys.lock().await;
                    if !vt_keys.is_empty() {
                        let mut provider = VirusTotalProvider::new_with_keys(vt_keys.clone());
                        if let Some(subs) = args.include_subdomains {
                            provider.with_subdomains(subs);
                        }
                        providers.push(Box::new(provider));
                        active_provider_names.push("VirusTotal".to_string());
                    }
                }
                "urlscan" => {
                    let urlscan_keys = self.urlscan_api_keys.lock().await;
                    if !urlscan_keys.is_empty() {
                        let mut provider = UrlscanProvider::new_with_keys(urlscan_keys.clone());
                        if let Some(subs) = args.include_subdomains {
                            provider.with_subdomains(subs);
                        }
                        providers.push(Box::new(provider));
                        active_provider_names.push("Urlscan".to_string());
                    }
                }
                _ => {}
            }
        }

        if providers.is_empty() {
            return Err(McpError::invalid_params(
                "No valid providers specified or API keys missing for selected providers",
                None,
            ));
        }

        // Create minimal args for processing
        let process_args = Args {
            domains: vec![args.domain.clone()],
            config: None,
            files: vec![],
            output: None,
            format: "plain".to_string(),
            merge_endpoint: false,
            normalize_url: false,
            providers: provider_names.clone(),
            subs: args.include_subdomains.unwrap_or(false),
            cc_index: "CC-MAIN-2025-13".to_string(),
            vt_api_key: vec![],
            urlscan_api_key: vec![],
            verbose: false,
            silent: true,
            no_progress: true,
            preset: vec![],
            extensions: vec![],
            exclude_extensions: vec![],
            patterns: vec![],
            exclude_patterns: vec![],
            show_only_host: false,
            show_only_path: false,
            show_only_param: false,
            min_length: None,
            max_length: None,
            strict: true,
            network_scope: "all".to_string(),
            proxy: None,
            proxy_auth: None,
            insecure: false,
            random_agent: false,
            timeout: 30,
            retries: 2,
            parallel: Some(5),
            rate_limit: None,
            check_status: false,
            include_status: vec![],
            exclude_status: vec![],
            extract_links: false,
            include_robots: true,
            include_sitemap: true,
            exclude_robots: false,
            exclude_sitemap: false,
            incremental: false,
            cache_type: "sqlite".to_string(),
            cache_path: None,
            redis_url: None,
            cache_ttl: 86400,
            no_cache: true, // Disable caching in MCP mode
            #[cfg(feature = "mcp")]
            mcp: false,
        };

        // Create progress manager (silent mode)
        let progress_manager = ProgressManager::new(true);

        // Process domains
        let urls = process_domains(
            vec![args.domain.clone()],
            &process_args,
            &progress_manager,
            &providers,
            &active_provider_names,
        )
        .await;

        // Apply filters if provided
        let mut url_filter = UrlFilter::new();

        if let Some(exts) = args.extensions {
            url_filter.with_extensions(exts.split(',').map(|s| s.trim().to_string()).collect());
        }

        if let Some(exclude_exts) = args.exclude_extensions {
            url_filter.with_exclude_extensions(
                exclude_exts
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }

        if let Some(pats) = args.patterns {
            url_filter.with_patterns(pats.split(',').map(|s| s.trim().to_string()).collect());
        }

        if let Some(exclude_pats) = args.exclude_patterns {
            url_filter.with_exclude_patterns(
                exclude_pats
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            );
        }

        let mut filtered_urls = url_filter.apply_filters(&urls);

        // Apply limit if specified
        if let Some(limit) = args.limit {
            filtered_urls.truncate(limit);
        }

        // Format response
        let url_count = filtered_urls.len();
        let url_list = filtered_urls.join("\n");

        let response_text = format!(
            "Found {} URLs for domain '{}' using providers: {}\n\nURLs:\n{}",
            url_count,
            args.domain,
            active_provider_names.join(", "),
            url_list
        );

        Ok(CallToolResult::success(vec![Content::text(response_text)]))
    }

    /// List available OSINT providers
    #[tool(description = "List all available OSINT URL providers and their status")]
    async fn list_providers(
        &self,
        Parameters(_args): Parameters<ListProvidersArgs>,
    ) -> Result<CallToolResult, McpError> {
        let vt_keys = self.vt_api_keys.lock().await;
        let urlscan_keys = self.urlscan_api_keys.lock().await;

        let providers = vec![
            ("wayback", "Wayback Machine", "Always available", true),
            ("cc", "Common Crawl", "Always available", true),
            ("otx", "AlienVault OTX", "Always available", true),
            (
                "vt",
                "VirusTotal",
                if vt_keys.is_empty() {
                    "Requires API key"
                } else {
                    "API key configured"
                },
                !vt_keys.is_empty(),
            ),
            (
                "urlscan",
                "URLScan.io",
                if urlscan_keys.is_empty() {
                    "Requires API key"
                } else {
                    "API key configured"
                },
                !urlscan_keys.is_empty(),
            ),
        ];

        let mut response = String::from("Available OSINT URL Providers:\n\n");
        for (name, full_name, status, available) in providers {
            let status_icon = if available { "✓" } else { "⚠" };
            response.push_str(&format!(
                "{} {} ({}): {}\n",
                status_icon, full_name, name, status
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(response)]))
    }
}

impl Default for UrxMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler]
impl ServerHandler for UrxMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "urx-mcp-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "URX MCP Server - Extract URLs from OSINT archives for security analysis.\n\
                 \n\
                 Available tools:\n\
                 - fetch_urls: Extract URLs from multiple OSINT sources (Wayback Machine, Common Crawl, OTX, etc.)\n\
                 - list_providers: Show available OSINT providers and their status\n\
                 \n\
                 Providers that require API keys:\n\
                 - VirusTotal: Set URX_VT_API_KEY environment variable\n\
                 - URLScan: Set URX_URLSCAN_API_KEY environment variable"
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        let server = UrxMcpServer::new();
        let info = server.get_info();

        assert_eq!(info.server_info.name, "urx-mcp-server");
        assert_eq!(info.protocol_version, ProtocolVersion::V_2024_11_05);
        assert!(info.capabilities.tools.is_some());
    }

    #[tokio::test]
    async fn test_list_providers_no_keys() {
        let server = UrxMcpServer::new();
        let args = ListProvidersArgs {};
        let result = server.list_providers(Parameters(args)).await;

        assert!(result.is_ok());
        let tool_result = result.unwrap();
        assert!(!tool_result.content.is_empty());
    }

    #[tokio::test]
    async fn test_api_key_management() {
        let server = UrxMcpServer::new();

        // Initially should be empty
        assert!(server.vt_api_keys.lock().await.is_empty());
        assert!(server.urlscan_api_keys.lock().await.is_empty());

        // Set keys
        server
            .set_vt_api_keys(vec!["test_vt_key".to_string()])
            .await;
        server
            .set_urlscan_api_keys(vec!["test_urlscan_key".to_string()])
            .await;

        // Verify keys are set
        assert_eq!(server.vt_api_keys.lock().await.len(), 1);
        assert_eq!(server.urlscan_api_keys.lock().await.len(), 1);
    }
}
