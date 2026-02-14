use std::collections::HashSet;
use url::Url;

/// Validates whether URLs have the same host as the provided domains
pub struct HostValidator {
    domains: HashSet<String>,
    include_subdomains: bool,
}

impl HostValidator {
    /// Create a new host validator with the given domains that can include subdomains
    pub fn new(domains: &[String], include_subdomains: bool) -> Self {
        let normalized_domains: HashSet<String> = domains
            .iter()
            .map(|domain| domain.trim().to_lowercase().trim_end_matches('.').to_string())
            .collect();

        HostValidator {
            domains: normalized_domains,
            include_subdomains,
        }
    }

    /// Validate that the URL's host matches one of the provided domains
    pub fn is_valid_host(&self, url_str: &str) -> bool {
        if let Ok(url) = Url::parse(url_str) {
            if let Some(host) = url.host_str() {
                // Normalize the host for comparison (lowercase and strip trailing dot)
                let normalized_host = host.to_lowercase();
                let host_stripped = normalized_host.trim_end_matches('.');

                // Check if the host exactly matches any of our domains
                if self.domains.contains(host_stripped) {
                    return true;
                }

                // If subdomains are allowed, check if the host is a subdomain of any of our domains
                if self.include_subdomains {
                    for domain in &self.domains {
                        if host_stripped.ends_with(&format!(".{domain}")) {
                            return true;
                        }
                    }
                }
            }
        }

        // If we can't parse the URL or it has no host, consider it invalid
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_validation() {
        // Create a validator with specific domains
        let domains = vec!["example.com".to_string(), "test.org".to_string()];
        let validator = HostValidator::new(&domains, false);

        // Test valid URLs
        assert!(validator.is_valid_host("https://example.com/path"));
        assert!(validator.is_valid_host("http://example.com"));
        assert!(validator.is_valid_host("https://test.org/page?query=value"));

        // Test edge cases with unusual characters in the host
        assert!(validator.is_valid_host("https://example.com.")); // Trailing dot should be valid
        assert!(!validator.is_valid_host("https://.example.com")); // Leading dot
        assert!(!validator.is_valid_host("https://-example.com")); // Leading hyphen
        assert!(!validator.is_valid_host("https://example-.com")); // Trailing hyphen

        // Test URLs with no host
        assert!(!validator.is_valid_host("file:///path/to/file"));
        assert!(!validator.is_valid_host("mailto:user@example.com"));
        assert!(!validator.is_valid_host("data:text/plain,Hello%20World"));

        // Test malformed URLs
        assert!(!validator.is_valid_host("https://"));
        assert!(!validator.is_valid_host("http://"));
        assert!(!validator.is_valid_host("not-a-url"));

        // Subdomains should not be valid with default settings
        assert!(!validator.is_valid_host("https://sub.example.com/path"));
    }

    #[test]
    fn test_host_validation_with_subdomains() {
        // Create a validator with specific domains that allows subdomains
        let domains = vec!["example.com".to_string(), "test.org".to_string()];
        let validator = HostValidator::new(&domains, true);

        // Test valid URLs
        assert!(validator.is_valid_host("https://example.com/path"));
        assert!(validator.is_valid_host("http://example.com"));
        assert!(validator.is_valid_host("https://test.org/page?query=value"));

        // Test subdomains
        assert!(validator.is_valid_host("https://sub.example.com/path"));
        assert!(validator.is_valid_host("https://deep.sub.example.com/path"));
        assert!(validator.is_valid_host("https://api.test.org/v1/endpoint"));

        // Test non-matching domains should still be invalid
        assert!(!validator.is_valid_host("https://example.net/path"));
        assert!(!validator.is_valid_host("https://test.com/path"));
    }

    #[test]
    fn test_host_validation_edge_cases() {
        // Create a validator with a domain that has a trailing dot
        let domains = vec!["example.com".to_string(), "test.org.".to_string()];
        let validator = HostValidator::new(&domains, true);

        // Multiple subdomain levels
        assert!(validator.is_valid_host("https://a.b.c.example.com/path"));

        // Similar looking domains (should be invalid)
        assert!(!validator.is_valid_host("https://notexample.com"));
        assert!(!validator.is_valid_host("https://example.com.evil.com"));
        assert!(!validator.is_valid_host("https://example.com-other.org"));

        // Case sensitivity
        assert!(validator.is_valid_host("https://SUB.EXAMPLE.COM"));

        // Trailing dots in URL should be handled
        assert!(validator.is_valid_host("https://example.com."));
        assert!(validator.is_valid_host("https://sub.example.com."));

        // Domains with trailing dots in the initial list should match hosts without them
        assert!(validator.is_valid_host("https://test.org"));
        assert!(validator.is_valid_host("https://sub.test.org"));
        assert!(validator.is_valid_host("https://sub.test.org."));
    }
}
