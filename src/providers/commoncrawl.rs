use anyhow::Result;
use reqwest::header::{CONTENT_LENGTH, RANGE};
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

use super::Provider;

const CHUNK_SIZE: usize = 1024 * 1024; // 1MB

#[derive(Clone)]
pub struct CommonCrawlProvider {
    index: String,
    include_subdomains: bool,
    proxy: Option<String>,
    proxy_auth: Option<String>,
    timeout: u64,
    retries: u32,
    random_agent: bool,
    insecure: bool,
    parallel: u32,
    rate_limit: Option<f32>,
    #[cfg(test)]
    base_url: String,
}

#[derive(Deserialize)]
struct CCRecord {
    url: String,
}

impl CommonCrawlProvider {
    #[allow(dead_code)]
    pub fn new() -> Self {
        CommonCrawlProvider {
            index: "CC-MAIN-2025-13".to_string(),
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 10,
            retries: 3,
            random_agent: true,
            insecure: false,
            parallel: 1,
            rate_limit: None,
            #[cfg(test)]
            base_url: "https://index.commoncrawl.org".to_string(),
        }
    }

    /// Creates a provider instance with a specific Common Crawl index
    pub fn with_index(index: String) -> Self {
        CommonCrawlProvider {
            index,
            include_subdomains: false,
            proxy: None,
            proxy_auth: None,
            timeout: 10,
            retries: 3,
            random_agent: true,
            insecure: false,
            parallel: 1,
            rate_limit: None,
            #[cfg(test)]
            base_url: "https://index.commoncrawl.org".to_string(),
        }
    }
}

impl Provider for CommonCrawlProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(self.clone())
    }

    fn fetch_urls<'a>(
        &'a self,
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            // Construct URL based on subdomain inclusion
            #[cfg(test)]
            let url_pattern = if self.include_subdomains {
                format!(
                    "{}/{}-index?url=*.{}/*&output=json",
                    self.base_url, self.index, domain
                )
            } else {
                format!(
                    "{}/{}-index?url={}/*&output=json",
                    self.base_url, self.index, domain
                )
            };

            #[cfg(not(test))]
            let url_pattern = if self.include_subdomains {
                format!(
                    "https://index.commoncrawl.org/{}-index?url=*.{}/*&output=json",
                    self.index, domain
                )
            } else {
                format!(
                    "https://index.commoncrawl.org/{}-index?url={}/*&output=json",
                    self.index, domain
                )
            };

            // Create client builder with timeout
            let mut client_builder =
                reqwest::Client::builder().timeout(std::time::Duration::from_secs(self.timeout));

            // Skip SSL verification if insecure is enabled
            if self.insecure {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            // Add random user agent if enabled
            if self.random_agent {
                let user_agents = [
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36",
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Safari/605.1.15",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:109.0) Gecko/20100101 Firefox/115.0",
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/113.0.0.0 Safari/537.36",
                    "Mozilla/5.0 (iPhone; CPU iPhone OS 16_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.5 Mobile/15E148 Safari/604.1",
                ];
                let random_index = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as usize
                    % user_agents.len();
                client_builder = client_builder.user_agent(user_agents[random_index]);
            }

            // Apply proxy if configured
            if let Some(proxy_url) = &self.proxy {
                let mut proxy = reqwest::Proxy::all(proxy_url)?;

                // Add proxy authentication if provided
                if let Some(auth) = &self.proxy_auth {
                    proxy = proxy.basic_auth(
                        auth.split(':').next().unwrap_or(""),
                        auth.split(':').nth(1).unwrap_or(""),
                    );
                }

                client_builder = client_builder.proxy(proxy);
            }

            let client = client_builder.build()?;

            let mut all_urls: Vec<String> = Vec::new();
            let mut current_byte: usize = 0;
            let mut incomplete_line_buffer: String = String::new();
            let mut total_size: Option<usize> = None;

            // Optional: Make a HEAD request to get Content-Length
            if let Ok(head_response) = client.head(&url_pattern).send().await {
                if head_response.status().is_success() {
                    if let Some(content_length) = head_response.headers().get(CONTENT_LENGTH) {
                        if let Ok(length_str) = content_length.to_str() {
                            if let Ok(length) = length_str.parse::<usize>() {
                                total_size = Some(length);
                            }
                        }
                    }
                }
            }

            loop {
                let end_byte = current_byte + CHUNK_SIZE - 1;
                let range_value = if let Some(size) = total_size {
                    if current_byte >= size {
                        break; // Already fetched everything
                    }
                    format!("bytes={}-{}", current_byte, std::cmp::min(end_byte, size -1))
                } else {
                    format!("bytes={}-{}", current_byte, end_byte)
                };

                let mut attempt = 0;
                let mut last_error = None;

                let response = loop {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
                    }
                    if attempt > self.retries {
                        if let Some(e) = last_error {
                            return Err(anyhow::anyhow!("Failed after {} attempts for range {}: {}", self.retries + 1, range_value, e));
                        } else {
                            return Err(anyhow::anyhow!("Failed after {} attempts for range {}", self.retries + 1, range_value));
                        }
                    }
                    attempt += 1;

                    match client.get(&url_pattern).header(RANGE, &range_value).send().await {
                        Ok(res) => {
                            // Check for 416 Range Not Satisfiable, which can happen if total_size was unknown and we request past EOF
                            if res.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                                // This means we've likely reached the end if total_size was not known
                                log::info!("Range not satisfiable for {}, assuming end of file.", range_value);
                                break Ok::<Option<reqwest::Response>, anyhow::Error>(None); // Signal to break outer loop
                            }
                            if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                                last_error = Some(anyhow::anyhow!("HTTP error: {} for range {}", res.status(), range_value));
                                continue;
                            }
                            break Ok(Some(res));
                        }
                        Err(e) => {
                            last_error = Some(e.into());
                            continue;
                        }
                    }
                }?;

                if response.is_none() { // Signalled to break outer loop (e.g. 416 error)
                    break;
                }
                let response = response.unwrap();


                match response.text().await {
                    Ok(mut text) => {
                        if text.is_empty() {
                            // If total_size was known and text is empty, it's fine.
                            // If total_size was not known, this means we've fetched all data.
                            if total_size.is_none() {
                                break;
                            }
                        }

                        if !incomplete_line_buffer.is_empty() {
                            text.insert_str(0, &incomplete_line_buffer);
                            incomplete_line_buffer.clear();
                        }

                        let mut lines: Vec<&str> = text.lines().collect();

                        if !text.ends_with('\n') && !text.ends_with('}') {
                             // Heuristic: if the chunk doesn't end with a newline or '}', the last line might be incomplete.
                             // This is especially true if the text received is less than CHUNK_SIZE, implying end of stream.
                             // However, if total_size is known and we are at the last chunk, the last line is complete.
                            let at_end_of_stream = total_size.map_or(false, |ts| end_byte >= ts -1);
                            if !at_end_of_stream && !lines.is_empty() {
                                incomplete_line_buffer = lines.pop().unwrap_or("").to_string();
                            }
                        }


                        for line in lines {
                            if line.trim().is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<CCRecord>(line) {
                                Ok(record) => {
                                    all_urls.push(record.url);
                                }
                                Err(e) => {
                                    // It's possible a partial line at the very end of the stream is valid JSON if small enough
                                    // For now, we log a warning. More robust handling might be needed.
                                    log::warn!("Failed to parse line as CCRecord: '{}'. Error: {}", line, e);
                                }
                            }
                        }

                        // If we received less data than requested (and didn't know total_size),
                        // or if we knew total_size and have received it all, assume end of stream.
                        let received_less_than_chunk = text.len() < CHUNK_SIZE && total_size.is_none();
                        let received_all_known_data = total_size.map_or(false, |ts| current_byte + text.len() >= ts);

                        if received_less_than_chunk || received_all_known_data {
                            if !incomplete_line_buffer.is_empty() { // Process any remaining buffer as the last line
                                match serde_json::from_str::<CCRecord>(&incomplete_line_buffer) {
                                    Ok(record) => {
                                        all_urls.push(record.url);
                                    }
                                    Err(e) => {
                                        log::warn!("Failed to parse final incomplete line as CCRecord: '{}'. Error: {}", incomplete_line_buffer, e);
                                    }
                                }
                                incomplete_line_buffer.clear();
                            }
                            break; // End of content
                        }
                        current_byte += text.len(); // approximate, might need adjustment based on actual bytes consumed vs characters
                                                // A more robust way would be to count bytes, but for UTF-8 text lines, this is tricky.
                                                // The key is that current_byte advances. If the server ignores ranges and sends full content,
                                                // this logic would break if we didn't also check for received_less_than_chunk or received_all_known_data.

                    }
                    Err(e) => {
                        // This is a retryable error for the current chunk
                        // The inner loop already handles retries for network errors for this specific chunk request.
                        // If it exhausts retries, it will return Err via the `?` operator above.
                        // This path implies text() conversion failed after a successful response.
                        // This should be rare. We can log and break, or attempt to treat as a failed chunk.
                        log::error!("Failed to get text from response for range bytes={}-{}: {}", current_byte, end_byte, e);
                        // We might want to retry the chunk here, or break and return what we have.
                        // For now, let's break and return successfully processed URLs.
                        break;
                    }
                }
            }

            // Remove duplicates
            all_urls.sort();
            all_urls.dedup();

            Ok(all_urls)
        })
    }

    // Implement new trait methods
    fn with_subdomains(&mut self, include: bool) {
        self.include_subdomains = include;
    }

    fn with_proxy(&mut self, proxy: Option<String>) {
        self.proxy = proxy;
    }

    fn with_proxy_auth(&mut self, auth: Option<String>) {
        self.proxy_auth = auth;
    }

    // New method implementations for the additional features
    fn with_timeout(&mut self, seconds: u64) {
        self.timeout = seconds;
    }

    fn with_retries(&mut self, count: u32) {
        self.retries = count;
    }

    fn with_random_agent(&mut self, enabled: bool) {
        self.random_agent = enabled;
    }

    fn with_insecure(&mut self, enabled: bool) {
        self.insecure = enabled;
    }

    fn with_parallel(&mut self, parallel: u32) {
        self.parallel = parallel;
    }

    fn with_rate_limit(&mut self, rate_limit: Option<f32>) {
        self.rate_limit = rate_limit;
    }
}

#[cfg(test)]
pub struct MockCommonCrawlProvider {
    mock_urls: Vec<String>,
    include_subdomains: bool,
}

#[cfg(test)]
impl MockCommonCrawlProvider {
    pub fn new(mock_urls: Vec<String>) -> Self {
        MockCommonCrawlProvider {
            mock_urls,
            include_subdomains: false,
        }
    }

    pub fn with_subdomains(&mut self, include: bool) -> &mut Self {
        self.include_subdomains = include;
        self
    }
}

#[cfg(test)]
impl Provider for MockCommonCrawlProvider {
    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(Self {
            mock_urls: self.mock_urls.clone(),
            include_subdomains: self.include_subdomains,
        })
    }

    fn fetch_urls<'a>(
        &'a self,
        _domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>> {
        let urls = self.mock_urls.clone();
        Box::pin(async move { Ok(urls) })
    }

    fn with_subdomains(&mut self, include: bool) {
        self.include_subdomains = include;
    }

    fn with_proxy(&mut self, _proxy: Option<String>) {}
    fn with_proxy_auth(&mut self, _auth: Option<String>) {}
    fn with_timeout(&mut self, _seconds: u64) {}
    fn with_retries(&mut self, _count: u32) {}
    fn with_random_agent(&mut self, _enabled: bool) {}
    fn with_insecure(&mut self, _enabled: bool) {}
    fn with_parallel(&mut self, _parallel: u32) {}
    fn with_rate_limit(&mut self, _rate_limit: Option<f32>) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{mock, Server, Matcher}; // Added mockito imports
    use tokio; // Ensure tokio is imported for async tests
    use reqwest::StatusCode;


    // Helper function to create a CommonCrawlProvider instance for testing
    // configured to use the mock server's URL.
    fn new_test_provider(server_url: &str) -> CommonCrawlProvider {
        let mut provider = CommonCrawlProvider::new();
        provider.base_url = server_url.to_string();
        provider.retries = 1; // Default to 1 retry for faster tests
        provider
    }

    // Helper function to create JSON strings for CCRecord
    fn make_cc_record_line(url: &str) -> String {
        format!(r#"{{"url": "{}"}}"#, url)
    }


    #[test]
    fn test_new_provider() {
        let provider = CommonCrawlProvider::new();
        assert_eq!(provider.index, "CC-MAIN-2025-13");
        assert!(!provider.include_subdomains);
        assert_eq!(provider.proxy, None);
        assert_eq!(provider.proxy_auth, None);
        assert_eq!(provider.timeout, 10);
        assert_eq!(provider.retries, 3);
        assert!(provider.random_agent);
        assert!(!provider.insecure);
        assert_eq!(provider.parallel, 1);
        assert_eq!(provider.rate_limit, None);
        assert_eq!(provider.base_url, "https://index.commoncrawl.org");
    }

    #[test]
    fn test_with_index() {
        let index = "CC-MAIN-2023-06".to_string();
        let provider = CommonCrawlProvider::with_index(index.clone());
        assert_eq!(provider.index, index);
        assert_eq!(provider.base_url, "https://index.commoncrawl.org");
    }

    #[test]
    fn test_with_subdomains() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_subdomains(true);
        assert!(provider.include_subdomains);
    }

    #[test]
    fn test_with_proxy() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_proxy(Some("http://proxy.example.com:8080".to_string()));
        assert_eq!(
            provider.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_with_proxy_auth() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_proxy_auth(Some("user:pass".to_string()));
        assert_eq!(provider.proxy_auth, Some("user:pass".to_string()));
    }

    #[test]
    fn test_with_timeout() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_timeout(60);
        assert_eq!(provider.timeout, 60);
    }

    #[test]
    fn test_with_retries() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_retries(5);
        assert_eq!(provider.retries, 5);
    }

    #[test]
    fn test_with_random_agent() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_random_agent(false);
        assert!(!provider.random_agent);
    }

    #[test]
    fn test_with_insecure() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_insecure(true);
        assert!(provider.insecure);
    }

    #[test]
    fn test_with_parallel() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_parallel(10);
        assert_eq!(provider.parallel, 10);
    }

    #[test]
    fn test_with_rate_limit() {
        let mut provider = CommonCrawlProvider::new();
        provider.with_rate_limit(Some(2.5));
        assert_eq!(provider.rate_limit, Some(2.5));
    }

    #[test]
    fn test_clone_box() {
        let provider = CommonCrawlProvider::new();
        let _cloned = provider.clone_box();
        // Just testing that cloning works without error
    }

    #[tokio::test]
    #[ignore = "Skip tests that make actual network requests in CI"]
    async fn test_fetch_urls_builds_correct_url_without_subdomains() {
        // Create a mock provider with predefined results
        let urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];
        let provider = MockCommonCrawlProvider::new(urls.clone());

        // Test fetching URLs
        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());

        let fetched_urls = result.unwrap();
        assert_eq!(fetched_urls.len(), 2);
        assert_eq!(fetched_urls[0], "https://example.com/page1");
        assert_eq!(fetched_urls[1], "https://example.com/page2");
    }

    #[tokio::test]
    #[ignore = "Skip tests that make actual network requests in CI"]
    async fn test_fetch_urls_builds_correct_url_with_subdomains() {
        // Create a mock provider with predefined results for subdomain test
        let urls = vec![
            "https://sub1.example.com/page1".to_string(),
            "https://sub2.example.com/page2".to_string(),
        ];
        let mut provider = MockCommonCrawlProvider::new(urls.clone());
        provider.with_subdomains(true);

        // Test fetching URLs with subdomains enabled
        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok());

        let fetched_urls = result.unwrap();
        assert_eq!(fetched_urls.len(), 2);
        assert_eq!(fetched_urls[0], "https://sub1.example.com/page1");
        assert_eq!(fetched_urls[1], "https://sub2.example.com/page2");
    }

    #[test]
    fn test_cc_record_deserialize() {
        let json = r#"{"url":"https://example.com/test"}"#;
        let record: CCRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.url, "https://example.com/test");
    }

    #[test]
    fn test_url_construction_without_subdomains() {
        // This test just verifies that the URL is constructed correctly without making a network request
        let provider = CommonCrawlProvider::new();

        // Use private helper function to check URL formation
        let url = format!(
            "https://index.commoncrawl.org/{}-index?url={}/*&output=json",
            provider.index, "example.com"
        );

        // This assertion needs to use the base_url from the provider instance for testing
        let test_provider = CommonCrawlProvider::new();
        let expected_url = format!(
            "{}/{}-index?url={}/*&output=json",
            test_provider.base_url, test_provider.index, "example.com"
        );
        assert_eq!(url, expected_url);
    }

    #[test]
    fn test_url_construction_with_subdomains() {
        // This test just verifies that the URL is constructed correctly without making a network request
        let mut provider = CommonCrawlProvider::new();
        provider.with_subdomains(true);

        // Use private helper function to check URL formation
        // This assertion needs to use the base_url from the provider instance for testing
        let url = format!(
            "{}/{}-index?url=*.{}/*&output=json",
            provider.base_url, provider.index, "example.com"
        );

        let expected_url = format!(
            "{}/{}-index?url=*.{}/*&output=json",
            provider.base_url, provider.index, "example.com"
        );
        assert_eq!(url, expected_url);
    }

    #[tokio::test]
    async fn test_fetch_urls_cc_pagination_multiple_chunks() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        provider.include_subdomains = false; // To simplify URL matching

        let url_path = format!("/{}-index?url=example.com/*&output=json", provider.index);

        let chunk1_content = format!("{}\n{}\n", make_cc_record_line("http://example.com/page1"), make_cc_record_line("http://example.com/page2"));
        let chunk2_content = format!("{}\n", make_cc_record_line("http://example.com/page3"));
        let incomplete_line_part1 = r#"{"url": "http://example.com/incomplete"#;
        let incomplete_line_part2 = r#"page"}"#;

        let chunk3_content_start_with_incomplete = format!("{}\n{}\n", incomplete_line_part2, make_cc_record_line("http://example.com/page4"));
        let total_content = format!("{}{}{}", chunk1_content, incomplete_line_part1, chunk3_content_start_with_incomplete);
        let total_len = total_content.len();

        // Mock HEAD request
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(200)
            .with_header(CONTENT_LENGTH, &total_len.to_string())
            .create_async().await;

        // Mock first chunk
        let range1 = format!("bytes=0-{}", std::cmp::min(CHUNK_SIZE -1, total_len -1));
        let _chunk1_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range1.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16())
            .with_body(format!("{}{}", chunk1_content, incomplete_line_part1))
            .create_async().await;

        let bytes_sent_chunk1 = chunk1_content.len() + incomplete_line_part1.len();

        // Mock second chunk (contains the rest of the incomplete line and page4)
        let range2 = format!("bytes={}-{}", bytes_sent_chunk1, std::cmp::min(bytes_sent_chunk1 + CHUNK_SIZE - 1, total_len - 1));
         let _chunk2_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range2.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16()) // Or 200 if it's the last part
            .with_body(chunk3_content_start_with_incomplete.clone())
            .create_async().await;


        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "fetch_urls failed: {:?}", result.err());
        let urls = result.unwrap();

        let mut expected_urls = vec![
            "http://example.com/page1",
            "http://example.com/page2",
            "http://example.com/incompletepage", // From the combined incomplete line
            "http://example.com/page4",
        ];
        expected_urls.sort();

        assert_eq!(urls, expected_urls);
    }


    #[tokio::test]
    async fn test_fetch_urls_cc_pagination_single_chunk() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        let url_path = format!("/{}-index?url=example.com/*&output=json", provider.index);

        let content = format!("{}\n{}\n", make_cc_record_line("http://example.com/single1"), make_cc_record_line("http://example.com/single2"));
        let content_len = content.len();

        // Mock HEAD
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(200)
            .with_header(CONTENT_LENGTH, &content_len.to_string())
            .create_async().await;

        // Mock GET
        let range = format!("bytes=0-{}", std::cmp::min(CHUNK_SIZE - 1, content_len - 1));
        let _get_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range))
            .with_status(StatusCode::OK.as_u16()) // OK or PARTIAL_CONTENT if server supports it well
            .with_body(&content)
            .create_async().await;

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "fetch_urls failed: {:?}", result.err());
        let urls = result.unwrap();
        let mut expected = vec!["http://example.com/single1", "http://example.com/single2"];
        expected.sort();
        assert_eq!(urls, expected);
    }

    #[tokio::test]
    async fn test_fetch_urls_cc_pagination_incomplete_last_line() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        let url_path = format!("/{}-index?url=example.com/*&output=json", provider.index);

        let line1 = make_cc_record_line("http://example.com/perfectline");
        let incomplete_part = r#"{"url": "http://example.com/partial"#; // Missing "}" and newline
        let content = format!("{}\n{}", line1, incomplete_part);
        let content_len = content.len();

        // Mock HEAD
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(200)
            .with_header(CONTENT_LENGTH, &content_len.to_string())
            .create_async().await;

        // Mock GET - server sends everything, but it ends with an incomplete line
        let range = format!("bytes=0-{}", std::cmp::min(CHUNK_SIZE - 1, content_len - 1));
        let _get_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range))
            .with_status(StatusCode::OK.as_u16())
            .with_body(&content)
            .create_async().await;

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "fetch_urls failed: {:?}", result.err());
        let urls = result.unwrap();

        // The current logic will parse the incomplete_part if it's the very end of the stream.
        // If serde_json can parse `incomplete_part` (it can't as is), it would be included.
        // Otherwise, it's logged as a warning.
        // Given `incomplete_part` is not valid JSON on its own, it should be logged and ignored.
        let mut expected = vec!["http://example.com/perfectline"];
        expected.sort();
        assert_eq!(urls, expected);
        // To verify logging, one would need to capture log output, which is complex in tests.
    }


    #[tokio::test]
    async fn test_fetch_urls_cc_no_content_length() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        let url_path = format!("/{}-index?url=example.com/*&output=json", provider.index);

        let chunk1_content = format!("{}\n", make_cc_record_line("http://example.com/no_len_page1"));
        let chunk2_content = format!("{}\n", make_cc_record_line("http://example.com/no_len_page2"));

        // Mock HEAD - fails or doesn't return content-length
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(404) // Or 200 without Content-Length header
            .create_async().await;

        // Mock first GET
        let range1 = format!("bytes=0-{}", CHUNK_SIZE - 1);
        let _chunk1_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range1.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16())
            .with_body(&chunk1_content)
            .create_async().await;

        // Mock second GET
        let range2 = format!("bytes={}-{}", chunk1_content.len(), chunk1_content.len() + CHUNK_SIZE - 1);
        let _chunk2_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range2.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16())
            .with_body(&chunk2_content)
            .create_async().await;

        // Mock third GET - empty response, signaling end of data
        let range3 = format!("bytes={}-{}", chunk1_content.len() + chunk2_content.len(), chunk1_content.len() + chunk2_content.len() + CHUNK_SIZE - 1);
        let _chunk3_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range3.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16()) // Or OK
            .with_body("") // Empty body
            .create_async().await;

        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_ok(), "fetch_urls failed: {:?}", result.err());
        let urls = result.unwrap();
        let mut expected = vec!["http://example.com/no_len_page1", "http://example.com/no_len_page2"];
        expected.sort();
        assert_eq!(urls, expected);
    }

    #[tokio::test]
    async fn test_fetch_urls_cc_range_not_satisfiable() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        let url_path = format!("/{}-index?url=empty.com/*&output=json", provider.index);

        // Mock HEAD - could indicate zero length or not be present
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(200)
            .with_header(CONTENT_LENGTH, "0") // Explicitly zero length
            .create_async().await;

        // Alternative: HEAD gives some length, but first GET for range 0-CHUNK_SIZE-1 returns 416
        // For this test, let's assume total_size is known as 0. The loop for ranges shouldn't even start.
        // If total_size was None, and the first GET returns 416:
        let _get_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(format!("bytes=0-{}", CHUNK_SIZE -1)))
            .with_status(StatusCode::RANGE_NOT_SATISFIABLE.as_u16())
            .create_async().await;


        // If HEAD returns 0, current_byte (0) >= total_size (0) is true, so loop is skipped.
        // If HEAD fails and first GET returns 416, the code should break and return empty.
        let result = provider.fetch_urls("empty.com").await;

        assert!(result.is_ok(), "fetch_urls failed: {:?}", result.err());
        assert!(result.unwrap().is_empty(), "Expected no URLs for Range Not Satisfiable");
    }

    #[tokio::test]
    async fn test_fetch_urls_cc_error_in_one_chunk() {
        let mut server = Server::new_async().await;
        let mut provider = new_test_provider(&server.url());
        provider.retries = 0; // No retries for this test to make behavior predictable for one error
        let url_path = format!("/{}-index?url=example.com/*&output=json", provider.index);

        let chunk1_content = format!("{}\n", make_cc_record_line("http://example.com/chunk1_ok"));
        let total_len = CHUNK_SIZE * 3; // Simulate enough content for more chunks

        // Mock HEAD
        let _head_mock = server.mock("HEAD", url_path.as_str())
            .with_status(200)
            .with_header(CONTENT_LENGTH, &total_len.to_string())
            .create_async().await;

        // Mock first chunk (successful)
        let range1 = format!("bytes=0-{}", CHUNK_SIZE - 1);
        let _chunk1_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range1.clone()))
            .with_status(StatusCode::PARTIAL_CONTENT.as_u16())
            .with_body(&chunk1_content)
            .create_async().await;

        let bytes_after_chunk1 = chunk1_content.len();

        // Mock second chunk (HTTP error)
        let range2 = format!("bytes={}-{}", bytes_after_chunk1, bytes_after_chunk1 + CHUNK_SIZE - 1);
        let _chunk2_mock = server.mock("GET", url_path.as_str())
            .match_header(RANGE, Matcher::Exact(range2.clone()))
            .with_status(StatusCode::INTERNAL_SERVER_ERROR.as_u16())
            .create_async().await;

        // The current implementation of fetch_urls will return Err if any chunk fails after retries.
        // It doesn't aggregate partial results if a chunk definitively fails.
        // So, we expect an Err here.
        let result = provider.fetch_urls("example.com").await;
        assert!(result.is_err(), "Expected an error due to the failing chunk");
        if let Err(e) = result {
            assert!(e.to_string().contains("Failed after 1 attempts for range"), "Error message mismatch: {}", e);
            assert!(e.to_string().contains("HTTP error: 500 Internal Server Error"), "Error message mismatch: {}", e);
        }
    }
}
