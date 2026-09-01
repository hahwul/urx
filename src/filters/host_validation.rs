use std::collections::HashSet;
use url::Url;

/// Validates whether URLs have the same host as the provided domains
pub struct HostValidator {
    domains: HashSet<String>,
    include_subdomains: bool,
}

/// Put a target domain into the exact form [`Url::host_str`] would report for
/// it, so the two sides of the comparison speak the same dialect.
///
/// The important case is IDN. `Url::parse` runs IDNA on the host, so
/// `https://café.com/x` reports its host as `xn--caf-dma.com`; comparing that
/// against the raw `café.com` the user typed never matched, and strict mode
/// (the default) therefore discarded *every* URL of an internationalised
/// target. Feeding the domain through the same parser also folds in the case,
/// trailing-dot and percent-encoding normalisation the host side already gets.
///
/// Input the parser cannot make a host of (a leading dot, say) falls back to
/// the trimmed, lowercased original so it keeps behaving as before.
fn normalize_domain(domain: &str) -> Option<String> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return None;
    }
    let parsed = Url::parse(&format!("https://{trimmed}/"))
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_lowercase()));
    Some(match parsed {
        Some(host) => host.trim_end_matches('.').to_string(),
        None => trimmed.to_lowercase(),
    })
}

impl HostValidator {
    /// Create a new host validator with the given domains that can include subdomains
    pub fn new(domains: &[String], include_subdomains: bool) -> Self {
        let normalized_domains: HashSet<String> = domains
            .iter()
            .filter_map(|domain| normalize_domain(domain))
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

                if self.include_subdomains {
                    // If subdomains are allowed, accept any subdomain of a target.
                    for domain in &self.domains {
                        if host_stripped.ends_with(&format!(".{domain}")) {
                            return true;
                        }
                    }
                } else {
                    // Even in strict (apex-only) mode, treat the conventional
                    // `www.` host as the apex itself: a site served entirely on
                    // www.<domain> must not return zero results for a bare
                    // `<domain>` query. Other subdomains still require --subs.
                    for domain in &self.domains {
                        if host_stripped == format!("www.{domain}") {
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
    fn test_www_treated_as_apex_in_strict_mode() {
        let domains = vec!["example.com".to_string()];
        let validator = HostValidator::new(&domains, false); // strict, no --subs

        // www counts as the apex...
        assert!(validator.is_valid_host("https://www.example.com/path"));
        assert!(validator.is_valid_host("https://example.com/path"));
        // ...but other subdomains still require --subs.
        assert!(!validator.is_valid_host("https://blog.example.com/path"));
        assert!(!validator.is_valid_host("https://api.example.com/path"));
        // a www-of-www is a sub-subdomain, not the apex.
        assert!(!validator.is_valid_host("https://www.www.example.com/path"));
    }

    #[test]
    fn test_idn_target_matches_the_punycode_host() {
        // Regression: `Url::parse` reports an IDN host in its punycode form, so
        // comparing it against the Unicode domain the user typed never matched
        // and strict mode (the default) dropped every URL of the target.
        let domains = vec!["café.com".to_string()];
        let validator = HostValidator::new(&domains, false);

        assert!(validator.is_valid_host("https://café.com/path"));
        assert!(validator.is_valid_host("https://xn--caf-dma.com/path"));
        assert!(validator.is_valid_host("https://www.café.com/path"));
        // ...and an unrelated host is still rejected.
        assert!(!validator.is_valid_host("https://evil.com/path"));
        assert!(!validator.is_valid_host("https://café.com.evil.com/path"));
    }

    #[test]
    fn test_idn_target_matches_subdomains_with_subs() {
        let domains = vec!["例え.jp".to_string()];
        let validator = HostValidator::new(&domains, true);

        assert!(validator.is_valid_host("https://例え.jp/a"));
        assert!(validator.is_valid_host("https://api.例え.jp/a"));
        assert!(validator.is_valid_host("https://api.xn--r8jz45g.jp/a"));
        assert!(!validator.is_valid_host("https://xn--r8jz45g.jp.evil.tld/a"));
    }

    #[test]
    fn test_punycode_target_matches_the_unicode_url() {
        // The mirror image: the target is already punycode (what
        // `urx https://café.com` normalizes to) and the archive returned the
        // Unicode spelling.
        let domains = vec!["xn--caf-dma.com".to_string()];
        let validator = HostValidator::new(&domains, false);

        assert!(validator.is_valid_host("https://café.com/path"));
        assert!(validator.is_valid_host("https://xn--caf-dma.com/path"));
    }

    #[test]
    fn test_domain_normalization_does_not_widen_matching() {
        // The domain now goes through the URL parser, so make sure that did not
        // turn any near-miss host into a match.
        let domains = vec!["example.com".to_string()];
        for validator in [
            HostValidator::new(&domains, false),
            HostValidator::new(&domains, true),
        ] {
            assert!(!validator.is_valid_host("https://evil-example.com/x"));
            assert!(!validator.is_valid_host("https://example.com.evil.tld/x"));
            assert!(!validator.is_valid_host("http://example.com@evil.tld/x"));
            assert!(!validator.is_valid_host("https://example%2ecom.evil.tld/x"));
            assert!(!validator.is_valid_host("https://notexample.com/x"));
            // userinfo before the real host must not confuse it either way
            assert!(validator.is_valid_host("http://evil.tld@example.com/x"));
        }
    }

    #[test]
    fn test_empty_domains_are_dropped() {
        // A blank line in --domain-list must not become a domain that matches.
        let validator = HostValidator::new(&[String::new(), "  ".to_string()], true);
        assert!(!validator.is_valid_host("https://example.com/x"));
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
