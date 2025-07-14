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
            .map(|domain| domain.trim().to_lowercase())
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
                // Normalize the host for comparison
                let normalized_host = host.to_lowercase();

                // Check if the host exactly matches any of our domains
                if self.domains.contains(&normalized_host) {
                    return true;
                }

                // If subdomains are allowed, check if the host is a subdomain of any of our domains
                if self.include_subdomains {
                    for domain in &self.domains {
                        if normalized_host.ends_with(&format!(".{domain}")) {
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
        assert!(!validator.is_valid_host("https://example.com.")); // Trailing dot
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
}
