use std::collections::HashSet;
use url::Url;

/// Validates whether URLs have the same host as the provided domains
pub struct HostValidator {
    domains: HashSet<String>,
}

impl HostValidator {
    /// Create a new host validator with the given domains
    pub fn new(domains: &[String]) -> Self {
        // Create a normalized set of domains for comparison
        let normalized_domains: HashSet<String> = domains
            .iter()
            .map(|domain| {
                domain
                    .trim()
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_end_matches('/')
                    .to_lowercase()
            })
            .collect();

        HostValidator {
            domains: normalized_domains,
        }
    }

    /// Validate that the URL's host matches one of the provided domains
    pub fn is_valid_host(&self, url_str: &str) -> bool {
        if let Ok(url) = Url::parse(url_str) {
            if let Some(host) = url.host_str() {
                // Normalize the host for comparison
                let normalized_host = host.to_lowercase();

                // Check if the host exactly matches any of our domains
                return self.domains.contains(&normalized_host);
            }
        }

        // If we can't parse the URL or it has no host, consider it invalid
        false
    }

    /// Filter URLs to only include those with valid hosts
    pub fn filter_urls(&self, urls: &HashSet<String>) -> HashSet<String> {
        urls.iter()
            .filter(|url| self.is_valid_host(url))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_validation() {
        // Create a validator with specific domains
        let domains = vec!["example.com".to_string(), "test.org".to_string()];
        let validator = HostValidator::new(&domains);

        // Test valid URLs
        assert!(validator.is_valid_host("https://example.com/path"));
        assert!(validator.is_valid_host("http://example.com"));
        assert!(validator.is_valid_host("https://test.org/page?query=value"));

        // Test invalid URLs
        assert!(!validator.is_valid_host("https://subdomain.example.com/path"));
        assert!(!validator.is_valid_host("https://other-domain.com"));
        assert!(!validator.is_valid_host("https://test.com"));

        // Test URL filtering
        let urls = HashSet::from([
            "https://example.com/page1".to_string(),
            "https://subdomain.example.com/page2".to_string(),
            "https://test.org/page3".to_string(),
            "https://invalid.com/page4".to_string(),
        ]);

        let filtered = validator.filter_urls(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains("https://example.com/page1"));
        assert!(filtered.contains("https://test.org/page3"));
        assert!(!filtered.contains("https://subdomain.example.com/page2"));
        assert!(!filtered.contains("https://invalid.com/page4"));
    }
}
