use std::collections::{HashMap, HashSet};
use url::Url;

/// Validates that a URL is inside the run's targets: on one of their hosts,
/// and — when a target named a path prefix — under that prefix.
pub struct HostValidator {
    /// Host to the path prefixes that host was scoped to. An empty list means
    /// the whole host is in scope, which is the ordinary case.
    domains: HashMap<String, Vec<String>>,
    include_subdomains: bool,
    /// Whether the host itself is checked. False only under `--no-strict` with
    /// a path-scoped target, where the user waived the host check but the path
    /// scope still stands. A URL on an unrecognised host then passes.
    enforce_host: bool,
}

/// Split a target into its host and its optional path prefix, normalising the
/// host exactly as [`normalize_domain`] does.
fn split(target: &str) -> Option<(String, Option<String>)> {
    let (host, path) = crate::cli::split_target(target);
    Some((normalize_domain(host)?, path.map(str::to_string)))
}

/// Whether `path` is at or under `prefix`.
///
/// `/shop` is in scope for the prefix `/shop`, and so is `/shop/x`; `/shopping`
/// is not. A prefix match on the raw string alone would accept it, which is
/// the classic way a path scope leaks — so the boundary has to be a separator
/// or the end of the path.
///
/// The comparison ignores ASCII case, which is not a guess about the target's
/// filesystem: a CDX server canonicalises the *whole* URL to lower case when
/// it builds a urlkey, so `url=example.com/Shop*` and `url=example.com/shop*`
/// return exactly the same rows — all of them spelled in lower case
/// (verified against web.archive.org). A case-sensitive check here would
/// therefore discard every row the archive just returned for a mixed-case
/// target, and `urx example.com/Shop` would silently produce nothing at all.
/// Matching the index's own semantics keeps the two ends of the query
/// agreeing; on a site that really does serve `/Shop` and `/shop` separately,
/// the cost is that scoping to one also collects the other.
fn path_in_scope(path: &str, prefix: &str) -> bool {
    let path = path.trim_end_matches('/').as_bytes();
    let prefix = prefix.as_bytes();
    if path.len() == prefix.len() {
        return path.eq_ignore_ascii_case(prefix);
    }
    path.len() > prefix.len()
        && path[prefix.len()] == b'/'
        && path[..prefix.len()].eq_ignore_ascii_case(prefix)
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
pub(super) fn normalize_domain(domain: &str) -> Option<String> {
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
    /// Create a new host validator with the given targets, which may be bare
    /// hosts or `host/path` scopes, and which can include subdomains.
    pub fn new(domains: &[String], include_subdomains: bool) -> Self {
        HostValidator {
            domains: Self::index(domains),
            include_subdomains,
            enforce_host: true,
        }
    }

    /// A validator that enforces only the targets' path scopes, for
    /// `--no-strict`. `None` when no target named a path, because there is
    /// then nothing left to check and the caller should skip validation
    /// entirely.
    pub fn paths_only(domains: &[String]) -> Option<Self> {
        let indexed = Self::index(domains);
        if indexed.values().all(Vec::is_empty) {
            return None;
        }
        Some(HostValidator {
            domains: indexed,
            // A path-scoped target implies its subdomains are out of scope
            // only if the host check runs at all, which here it does not.
            include_subdomains: false,
            enforce_host: false,
        })
    }

    /// Group the targets by host, collecting each host's path prefixes. A host
    /// named both bare and with a path is in scope entirely: the broader of
    /// two overlapping targets wins, exactly as two `--scope-file` includes do.
    fn index(domains: &[String]) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        let mut unscoped: HashSet<String> = HashSet::new();
        for target in domains {
            let Some((host, path)) = split(target) else {
                continue;
            };
            match path {
                None => {
                    unscoped.insert(host.clone());
                    out.insert(host, Vec::new());
                }
                Some(path) => {
                    if unscoped.contains(&host) {
                        continue;
                    }
                    out.entry(host).or_default().push(path);
                }
            }
        }
        for host in unscoped {
            out.insert(host, Vec::new());
        }
        out
    }

    /// Whether any target narrowed its host to a path.
    ///
    /// Read by the advisory that fires when validation discards most of a
    /// result: with a path scope in play, "pass --no-strict to keep all hosts"
    /// is wrong advice, because --no-strict does not widen a path scope.
    pub fn has_path_scopes(&self) -> bool {
        self.domains.values().any(|paths| !paths.is_empty())
    }

    /// Whether `url`'s path is inside the scopes recorded for `host`.
    fn path_allowed(&self, host: &str, url: &Url) -> bool {
        match self.domains.get(host) {
            // An unrecognised host only reaches here with the host check off.
            None => true,
            Some(prefixes) if prefixes.is_empty() => true,
            Some(prefixes) => prefixes
                .iter()
                .any(|prefix| path_in_scope(url.path(), prefix)),
        }
    }

    /// Validate that the URL is inside one of the run's targets: on a target
    /// host (unless `--no-strict` waived that) and under that target's path
    /// prefix, when it named one.
    pub fn is_valid_host(&self, url_str: &str) -> bool {
        let Ok(url) = Url::parse(url_str) else {
            // If we can't parse the URL, consider it invalid
            return false;
        };
        let Some(host) = url.host_str() else {
            // ...and likewise when it has no host at all
            return false;
        };
        // Normalize the host for comparison (lowercase and strip trailing dot)
        let normalized_host = host.to_lowercase();
        let host_stripped = normalized_host.trim_end_matches('.');

        // Check if the host exactly matches any of our domains
        if self.domains.contains_key(host_stripped) {
            return self.path_allowed(host_stripped, &url);
        }

        if self.include_subdomains {
            // If subdomains are allowed, accept any subdomain of a target.
            for domain in self.domains.keys() {
                if host_stripped.ends_with(&format!(".{domain}")) {
                    return self.path_allowed(domain, &url);
                }
            }
        } else {
            // Even in strict (apex-only) mode, treat the conventional
            // `www.` host as the apex itself: a site served entirely on
            // www.<domain> must not return zero results for a bare
            // <domain> query. Other subdomains still require --subs.
            for domain in self.domains.keys() {
                if host_stripped == format!("www.{domain}") {
                    return self.path_allowed(domain, &url);
                }
            }
        }

        // An unrecognised host: out of scope in strict mode, and in
        // paths-only mode nothing was claimed about it either way.
        !self.enforce_host
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

    #[test]
    fn a_path_scoped_target_admits_only_urls_under_that_path() {
        let validator = HostValidator::new(&["example.com/shop".to_string()], false);

        // At the prefix, and under it.
        assert!(validator.is_valid_host("https://example.com/shop"));
        assert!(validator.is_valid_host("https://example.com/shop/"));
        assert!(validator.is_valid_host("https://example.com/shop/item?id=1"));

        // The classic way a path scope leaks: a sibling sharing the prefix.
        // The CDX query returns these (it asks for `example.com/shop*`), so
        // this check is what actually enforces the scope.
        assert!(!validator.is_valid_host("https://example.com/shopping"));
        assert!(!validator.is_valid_host("https://example.com/shop-admin"));

        // Elsewhere on the same host, and on other hosts.
        assert!(!validator.is_valid_host("https://example.com/about"));
        assert!(!validator.is_valid_host("https://example.com/"));
        assert!(!validator.is_valid_host("https://other.test/shop"));
    }

    #[test]
    fn a_path_scope_ignores_case_because_the_cdx_index_does() {
        // A CDX server lower-cases the whole URL when it builds a urlkey, so
        // `url=example.com/Shop*` returns rows spelled `/shop...`. Rejecting
        // those here would make a mixed-case target return nothing at all.
        let validator = HostValidator::new(&["example.com/Shop".to_string()], false);
        assert!(validator.is_valid_host("https://EXAMPLE.com/Shop/x"));
        assert!(validator.is_valid_host("https://example.com/shop/x"));
        // The boundary still holds, whatever the case.
        assert!(!validator.is_valid_host("https://example.com/shopping"));
    }

    #[test]
    fn the_broader_of_two_overlapping_targets_wins() {
        // Naming the host both ways means the whole host was asked for; the
        // narrower target must not silently cancel the broader one.
        let validator = HostValidator::new(
            &["example.com/shop".to_string(), "example.com".to_string()],
            false,
        );
        assert!(validator.is_valid_host("https://example.com/about"));

        // Order must not matter.
        let validator = HostValidator::new(
            &["example.com".to_string(), "example.com/shop".to_string()],
            false,
        );
        assert!(validator.is_valid_host("https://example.com/about"));
    }

    #[test]
    fn two_scopes_on_one_host_are_a_union() {
        let validator = HostValidator::new(
            &[
                "example.com/shop".to_string(),
                "example.com/api".to_string(),
            ],
            false,
        );
        assert!(validator.is_valid_host("https://example.com/shop/x"));
        assert!(validator.is_valid_host("https://example.com/api/x"));
        assert!(!validator.is_valid_host("https://example.com/blog"));
    }

    #[test]
    fn a_path_scope_applies_to_subdomains_too() {
        // --subs cannot push the prefix into the CDX query, so everything
        // under every subdomain comes back and this is the only thing
        // enforcing the scope.
        let validator = HostValidator::new(&["example.com/shop".to_string()], true);
        assert!(validator.is_valid_host("https://cdn.example.com/shop/x"));
        assert!(!validator.is_valid_host("https://cdn.example.com/about"));
    }

    #[test]
    fn paths_only_waives_the_host_check_but_not_the_scope() {
        // `--no-strict` says "don't drop off-host URLs". It does not say
        // "ignore the /shop I asked for".
        let validator =
            HostValidator::paths_only(&["example.com/shop".to_string()]).expect("a scope exists");
        assert!(validator.is_valid_host("https://example.com/shop/x"));
        assert!(!validator.is_valid_host("https://example.com/about"));
        // An unrecognised host was never claimed either way, so it passes.
        assert!(validator.is_valid_host("https://other.test/anything"));
    }

    #[test]
    fn paths_only_is_nothing_at_all_when_no_target_named_a_path() {
        // With no scope to enforce there is nothing left to check, and the
        // caller should skip validation entirely rather than run an
        // always-true predicate over every URL.
        assert!(HostValidator::paths_only(&["example.com".to_string()]).is_none());
    }
}
