use std::collections::HashSet;
use std::path::Path;

use anyhow::Context;
use regex::Regex;
use url::Url;

use super::preset::{FilterPreset, PathRule};

/// Normalise one user-supplied extension.
///
/// The extension we compare against comes from `Path::extension()`, which never
/// includes the leading dot — so `-e .js`, the spelling half of the world types,
/// matched nothing at all and produced a silently empty result set. Surrounding
/// whitespace (`-e "js, php"`) had the same effect. Both are stripped here, and
/// an entry that names no extension after stripping (an empty item from a
/// trailing comma) is dropped rather than kept as a token that can never match.
fn normalize_extension(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('.').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

fn normalize_extensions<I>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    raw.into_iter()
        .filter_map(|s| normalize_extension(&s))
        .collect()
}

/// Normalise one user-supplied pattern.
///
/// An empty pattern is a substring of every URL, so a single empty item — from
/// `--exclude-patterns ""` or, far more easily, the trailing comma in
/// `--exclude-patterns "admin,"` — silently discarded the *entire* result set,
/// and an empty `--patterns` item silently disabled the filter. Whitespace is
/// trimmed for the same reason it is on extensions: URLs are whitespace-free by
/// construction, so ` admin` from `--patterns "api, admin"` could only ever
/// match nothing.
fn normalize_pattern(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

fn normalize_patterns<I>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    raw.into_iter()
        .filter_map(|s| normalize_pattern(&s))
        .collect()
}

/// The lower-cased path of `url`, for the suffix-anchored preset path rules.
///
/// Anchoring on the path rather than the whole URL is what makes a rule like
/// "ends with `~`" survive a query string: `/index.php~?v=1` is still a backup.
/// Unparseable input falls back to everything before the query, matching how
/// the rest of the filter degrades on malformed URLs.
fn lower_path(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) => parsed.path().to_lowercase(),
        Err(_) => url.split(['?', '#']).next().unwrap_or("").to_lowercase(),
    }
}

/// Compile the regular expressions given to `--match-regex` / `--filter-regex`.
///
/// Done once, up front, so a typo in the pattern is a single clear startup
/// error instead of a per-URL failure (or, worse, a filter that silently
/// matches nothing). `flag` names the option in the message so a run with both
/// says which one is broken. Blank entries are dropped rather than compiled
/// into the empty regex, which matches every URL.
pub fn compile_url_regexes(raw: &[String], flag: &str) -> anyhow::Result<Vec<Regex>> {
    raw.iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            Regex::new(p).with_context(|| format!("Invalid regular expression in {flag}: {p}"))
        })
        .collect()
}

/// URL Filter for filtering URLs based on extensions, patterns, length, etc.
#[derive(Default)]
pub struct UrlFilter {
    extensions: Vec<String>,
    exclude_extensions: Vec<String>,
    patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    /// Preset-supplied path shapes, ORed with `extensions` rather than ANDed
    /// with them. See [`FilterPreset::get_path_rules`].
    path_rules: Vec<PathRule>,
    match_regex: Vec<Regex>,
    filter_regex: Vec<Regex>,
    min_length: Option<usize>,
    max_length: Option<usize>,
}

impl UrlFilter {
    /// Create a new URL filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply filter presets to this URL filter
    pub fn apply_presets(&mut self, presets: &[String]) -> &mut Self {
        for preset_str in presets {
            if let Some(preset) = FilterPreset::from_str(preset_str) {
                // Merge preset extensions/patterns with existing ones
                self.extensions
                    .extend(normalize_extensions(preset.get_extensions()));
                self.exclude_extensions
                    .extend(normalize_extensions(preset.get_exclude_extensions()));
                self.patterns
                    .extend(normalize_patterns(preset.get_patterns()));
                self.exclude_patterns
                    .extend(normalize_patterns(preset.get_exclude_patterns()));
                self.path_rules.extend(preset.get_path_rules());
            }
        }
        self
    }

    /// Set extensions to include
    pub fn with_extensions(&mut self, extensions: Vec<String>) -> &mut Self {
        // Merge with existing extensions instead of replacing
        self.extensions.extend(normalize_extensions(extensions));
        self
    }

    /// Set extensions to exclude
    pub fn with_exclude_extensions(&mut self, exclude_extensions: Vec<String>) -> &mut Self {
        self.exclude_extensions
            .extend(normalize_extensions(exclude_extensions));
        self
    }

    /// Set patterns to include
    pub fn with_patterns(&mut self, patterns: Vec<String>) -> &mut Self {
        // Merge with existing patterns instead of replacing
        self.patterns.extend(normalize_patterns(patterns));
        self
    }

    /// Set patterns to exclude
    pub fn with_exclude_patterns(&mut self, exclude_patterns: Vec<String>) -> &mut Self {
        // Merge with existing exclude_patterns instead of replacing
        self.exclude_patterns
            .extend(normalize_patterns(exclude_patterns));
        self
    }

    /// Keep only URLs matching at least one of these regular expressions.
    ///
    /// Evaluated against the **whole URL string** exactly as collected, so the
    /// match is case-sensitive unless the pattern says otherwise (`(?i)`).
    /// That is deliberately unlike `--patterns`, which lower-cases both sides.
    /// Several expressions OR together.
    pub fn with_match_regex(&mut self, regexes: Vec<Regex>) -> &mut Self {
        self.match_regex.extend(regexes);
        self
    }

    /// Drop URLs matching any of these regular expressions. Same evaluation
    /// rules as [`UrlFilter::with_match_regex`]; one hit is enough to exclude.
    pub fn with_filter_regex(&mut self, regexes: Vec<Regex>) -> &mut Self {
        self.filter_regex.extend(regexes);
        self
    }

    /// Set minimum URL length
    pub fn with_min_length(&mut self, min_length: Option<usize>) -> &mut Self {
        self.min_length = min_length;
        self
    }

    /// Set maximum URL length
    pub fn with_max_length(&mut self, max_length: Option<usize>) -> &mut Self {
        self.max_length = max_length;
        self
    }

    /// Whether a single URL survives every configured filter.
    ///
    /// This is the whole of the filtering decision, kept per-URL so the batch
    /// path ([`UrlFilter::apply_filters`]) and the streaming path apply exactly
    /// the same rules instead of drifting apart.
    pub fn matches(&self, url: &str) -> bool {
        // Skip if URL doesn't match the length criteria
        if let Some(min) = self.min_length {
            if url.len() < min {
                return false;
            }
        }

        if let Some(max) = self.max_length {
            if url.len() > max {
                return false;
            }
        }

        // Parse the URL to extract the path for better extension handling
        let extension = match Url::parse(url) {
            Ok(parsed_url) => {
                // Get the path from the URL
                if let Some(path) = parsed_url
                    .path_segments()
                    .and_then(|mut segments| segments.next_back())
                {
                    // Extract extension from the last path segment
                    Path::new(path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|s| s.to_lowercase())
                } else {
                    None
                }
            }
            Err(_) => {
                // Fallback for invalid URLs - try to extract extension from the whole string
                let parts: Vec<&str> = url.split('/').collect();
                if let Some(last) = parts.last() {
                    let filename_parts: Vec<&str> = last.split('.').collect();
                    if filename_parts.len() > 1 {
                        Some(
                            filename_parts
                                .last()
                                .unwrap()
                                .split('?')
                                .next()
                                .unwrap_or("")
                                .to_lowercase(),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        // Compute url_lower once per URL if needed
        let mut url_lower = None;

        // Check exclusions first
        if !self.exclude_extensions.is_empty() {
            if let Some(ext) = &extension {
                if self
                    .exclude_extensions
                    .iter()
                    .any(|excluded_ext| excluded_ext == ext)
                {
                    return false;
                }
            }
        }

        if !self.exclude_patterns.is_empty() {
            let url_lower_str = url_lower.get_or_insert_with(|| url.to_lowercase());
            if self
                .exclude_patterns
                .iter()
                .any(|pattern| url_lower_str.contains(pattern))
            {
                return false;
            }
        }

        // Regular expressions see the URL exactly as collected: no lower-casing,
        // no reparsing. `(?i)` is how a user asks for a case-insensitive match.
        if self.filter_regex.iter().any(|re| re.is_match(url)) {
            return false;
        }

        // Then check inclusions. The extension list and the preset path rules
        // are *alternatives*: a URL qualifies by carrying a listed extension or
        // by having a listed path shape, because a backup is `db.sql` or
        // `index.php~` and neither carries the other's marker. `--patterns` and
        // `--match-regex` then narrow whatever survived.
        let mut include = true;

        if !self.extensions.is_empty() || !self.path_rules.is_empty() {
            let by_extension = match &extension {
                Some(ext) => self
                    .extensions
                    .iter()
                    .any(|included_ext| included_ext == ext),
                // No extension found but an extensions filter is set.
                None => false,
            };
            include = by_extension || {
                let url_lower_str = url_lower.get_or_insert_with(|| url.to_lowercase());
                self.matches_path_rule(url, url_lower_str)
            };
        }

        if include && !self.patterns.is_empty() {
            let url_lower_str = url_lower.get_or_insert_with(|| url.to_lowercase());
            include = self
                .patterns
                .iter()
                .any(|pattern| url_lower_str.contains(pattern));
        }

        if include && !self.match_regex.is_empty() {
            include = self.match_regex.iter().any(|re| re.is_match(url));
        }

        include
    }

    /// Whether any preset path rule accepts this URL.
    ///
    /// `url_lower` is passed in because the caller has usually built it
    /// already; the path is only derived when a suffix rule actually needs it.
    fn matches_path_rule(&self, url: &str, url_lower: &str) -> bool {
        if self.path_rules.is_empty() {
            return false;
        }

        let mut path_lower: Option<String> = None;
        self.path_rules.iter().any(|rule| match rule {
            PathRule::Contains(needle) => url_lower.contains(needle.as_str()),
            PathRule::PathEndsWith(suffix) => path_lower
                .get_or_insert_with(|| lower_path(url))
                .ends_with(suffix.as_str()),
        })
    }

    /// Apply filters to a set of URLs, returning the survivors sorted.
    pub fn apply_filters(&self, urls: &HashSet<String>) -> Vec<String> {
        let mut result: Vec<String> = urls
            .iter()
            .filter(|url| self.matches(url))
            .cloned()
            .collect();

        // Sort the results for consistent output
        result.sort();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn create_test_urls() -> HashSet<String> {
        let urls = vec![
            "https://example.com/index.html",
            "https://example.com/script.js",
            "https://example.com/style.css",
            "https://example.com/image.png",
            "https://example.com/document.pdf",
            "https://example.com/font.woff2",
            "https://example.com/video.mp4",
            "https://example.com/admin/login.php",
            "https://example.com/api/v1/users?id=123",
            "https://example.com/very/long/path/to/resource/file.html",
            "https://example.com/.git/config",
        ];
        urls.into_iter().map(String::from).collect()
    }

    #[test]
    fn test_new_filter() {
        let filter = UrlFilter::new();
        assert!(filter.extensions.is_empty());
        assert!(filter.exclude_extensions.is_empty());
        assert!(filter.patterns.is_empty());
        assert!(filter.exclude_patterns.is_empty());
        assert!(filter.path_rules.is_empty());
        assert!(filter.match_regex.is_empty());
        assert!(filter.filter_regex.is_empty());
        assert_eq!(filter.min_length, None);
        assert_eq!(filter.max_length, None);
    }

    #[test]
    fn test_with_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string(), "php".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_with_exclude_extensions() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_extensions(vec![
            "js".to_string(),
            "css".to_string(),
            "png".to_string(),
        ]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 8);
        assert!(!filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/style.css".to_string()));
        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
    }

    #[test]
    fn test_with_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["admin".to_string(), "api".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
        assert!(filtered.contains(&"https://example.com/api/v1/users?id=123".to_string()));
    }

    #[test]
    fn test_with_exclude_patterns() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_patterns(vec!["admin".to_string(), ".git".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 9);
        assert!(!filtered.contains(&"https://example.com/admin/login.php".to_string()));
        assert!(!filtered.contains(&"https://example.com/.git/config".to_string()));
    }

    #[test]
    fn test_with_length_filters() {
        let mut filter = UrlFilter::new();
        filter.with_min_length(Some(40));
        filter.with_max_length(Some(60));

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        for url in &filtered {
            assert!(url.len() >= 40);
            assert!(url.len() <= 60);
        }
    }

    #[test]
    fn test_apply_presets() {
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["no-images".to_string(), "only-js".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
    }

    #[test]
    fn test_extensions_accept_a_leading_dot() {
        // Regression: `-e .js` is the spelling most people reach for, and it
        // matched nothing at all — a silently empty result set rather than an
        // error. The extension we compare against never carries the dot.
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec![".js".to_string(), "..php".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2, "{filtered:?}");
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_exclude_extensions_accept_a_leading_dot() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_extensions(vec![".js".to_string(), ".css".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert!(!filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(!filtered.contains(&"https://example.com/style.css".to_string()));
        assert!(filtered.contains(&"https://example.com/index.html".to_string()));
    }

    #[test]
    fn test_extension_entries_that_name_nothing_are_dropped() {
        // A trailing comma (`-e "js,"`) yields an empty item. Kept as-is it
        // would be compared against the extension of `/file.`, which
        // `Path::extension()` reports as "" — a match nobody asked for.
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string(), String::new(), ".".to_string()]);
        assert_eq!(filter.extensions, vec!["js".to_string()]);

        let urls: HashSet<String> = ["https://example.com/file.", "https://example.com/app.js"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            filter.apply_filters(&urls),
            vec!["https://example.com/app.js".to_string()]
        );
    }

    #[test]
    fn test_extensions_tolerate_surrounding_whitespace() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string(), " php".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_empty_exclude_pattern_does_not_wipe_the_result_set() {
        // Regression: an empty pattern is a substring of every URL, so
        // `--exclude-patterns "admin,"` (one stray comma) excluded *everything*
        // and produced an empty run with no diagnostic whatsoever.
        let mut filter = UrlFilter::new();
        filter.with_exclude_patterns(vec!["admin".to_string(), String::new()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), urls.len() - 1, "{filtered:?}");
        assert!(!filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_only_empty_exclude_patterns_disable_the_filter() {
        let mut filter = UrlFilter::new();
        filter.with_exclude_patterns(vec![String::new(), "   ".to_string()]);
        assert!(filter.exclude_patterns.is_empty());

        let urls = create_test_urls();
        assert_eq!(filter.apply_filters(&urls).len(), urls.len());
    }

    #[test]
    fn test_empty_include_pattern_does_not_disable_the_filter() {
        // The inverse failure: an empty item matched every URL, so
        // `--patterns "api,"` quietly stopped filtering at all.
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["api".to_string(), String::new()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(
            filtered,
            vec!["https://example.com/api/v1/users?id=123".to_string()]
        );
    }

    #[test]
    fn test_patterns_tolerate_surrounding_whitespace() {
        // `--patterns "api, admin"` used to look for " admin", which no URL can
        // contain: URLs are whitespace-free by construction.
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["api".to_string(), " admin".to_string()]);

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2, "{filtered:?}");
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_extension_filter_ignores_query_and_fragment() {
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string()]);

        let urls: HashSet<String> = [
            "https://example.com/app.js?v=1",
            "https://example.com/app.js#top",
            "https://example.com/App.JS",
            "https://example.com/dir.js/index",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let filtered = filter.apply_filters(&urls);
        assert_eq!(filtered.len(), 3, "{filtered:?}");
        assert!(!filtered.contains(&"https://example.com/dir.js/index".to_string()));
    }

    #[test]
    fn test_match_regex_keeps_only_matching_urls() {
        let mut filter = UrlFilter::new();
        filter.with_match_regex(
            compile_url_regexes(&[r"/api/v\d+/".to_string()], "--match-regex").unwrap(),
        );

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(
            filtered,
            vec!["https://example.com/api/v1/users?id=123".to_string()]
        );
    }

    #[test]
    fn test_several_match_regexes_are_ored() {
        let mut filter = UrlFilter::new();
        filter.with_match_regex(
            compile_url_regexes(
                &[r"\.js$".to_string(), r"login\.php".to_string()],
                "--match-regex",
            )
            .unwrap(),
        );

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 2, "{filtered:?}");
        assert!(filtered.contains(&"https://example.com/script.js".to_string()));
        assert!(filtered.contains(&"https://example.com/admin/login.php".to_string()));
    }

    #[test]
    fn test_filter_regex_drops_on_any_match() {
        let mut filter = UrlFilter::new();
        filter.with_filter_regex(
            compile_url_regexes(
                &[
                    r"\.(png|css)$".to_string(),
                    r"^https://example\.com/\.git/".to_string(),
                ],
                "--filter-regex",
            )
            .unwrap(),
        );

        let urls = create_test_urls();
        let filtered = filter.apply_filters(&urls);

        assert!(!filtered.contains(&"https://example.com/image.png".to_string()));
        assert!(!filtered.contains(&"https://example.com/style.css".to_string()));
        assert!(!filtered.contains(&"https://example.com/.git/config".to_string()));
        assert_eq!(filtered.len(), urls.len() - 3, "{filtered:?}");
    }

    #[test]
    fn test_filter_regex_wins_over_match_regex() {
        // Exclusion is evaluated first and is final: a URL that satisfies both
        // is dropped, mirroring how --exclude-patterns beats --patterns.
        let mut filter = UrlFilter::new();
        filter
            .with_match_regex(
                compile_url_regexes(&["example".to_string()], "--match-regex").unwrap(),
            )
            .with_filter_regex(
                compile_url_regexes(&["admin".to_string()], "--filter-regex").unwrap(),
            );

        assert!(filter.matches("https://example.com/index.html"));
        assert!(!filter.matches("https://example.com/admin/login.php"));
    }

    #[test]
    fn test_regexes_are_case_sensitive_unlike_patterns() {
        // The documented difference between the two flags. --patterns
        // lower-cases both sides; a regex sees the URL exactly as collected,
        // and asks for case-insensitivity with (?i).
        let mut sensitive = UrlFilter::new();
        sensitive.with_match_regex(
            compile_url_regexes(&["ADMIN".to_string()], "--match-regex").unwrap(),
        );
        assert!(!sensitive.matches("https://example.com/admin/login.php"));

        let mut insensitive = UrlFilter::new();
        insensitive.with_match_regex(
            compile_url_regexes(&["(?i)ADMIN".to_string()], "--match-regex").unwrap(),
        );
        assert!(insensitive.matches("https://example.com/admin/login.php"));

        let mut patterns = UrlFilter::new();
        patterns.with_patterns(vec!["ADMIN".to_string()]);
        assert!(patterns.matches("https://example.com/admin/login.php"));
    }

    #[test]
    fn test_match_regex_narrows_the_other_include_filters() {
        // Inclusion filters AND together, exactly as --extensions and
        // --patterns already do.
        let mut filter = UrlFilter::new();
        filter
            .with_extensions(vec!["php".to_string(), "js".to_string()])
            .with_match_regex(
                compile_url_regexes(&["/admin/".to_string()], "--match-regex").unwrap(),
            );

        assert!(filter.matches("https://example.com/admin/login.php"));
        assert!(!filter.matches("https://example.com/script.js"));
        assert!(!filter.matches("https://example.com/admin/index.html"));
    }

    #[test]
    fn test_compile_url_regexes_reports_the_flag_and_the_pattern() {
        let err = compile_url_regexes(&["[unclosed".to_string()], "--match-regex")
            .unwrap_err()
            .to_string();
        assert!(err.contains("--match-regex"), "{err}");
        assert!(err.contains("[unclosed"), "{err}");
    }

    #[test]
    fn test_compile_url_regexes_drops_blank_entries() {
        // An empty pattern matches every URL, so keeping it would silently
        // disable --match-regex and wipe the result set through --filter-regex.
        let compiled = compile_url_regexes(
            &[String::new(), "  ".to_string(), r"\.js$".to_string()],
            "--match-regex",
        )
        .unwrap();
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].as_str(), r"\.js$");
    }

    #[test]
    fn test_regex_flags_leave_substring_patterns_untouched() {
        // Backwards compatibility: --patterns stays a plain lower-cased
        // substring test, so a regex metacharacter in it is a literal.
        let mut filter = UrlFilter::new();
        filter.with_patterns(vec!["v1/users?id=".to_string()]);

        assert!(filter.matches("https://example.com/api/v1/users?id=123"));
        assert!(!filter.matches("https://example.com/api/v1/user?id=123"));
    }

    #[test]
    fn test_only_secrets_matches_by_path_or_extension() {
        // The whole point of the path rules: a secret is `/.env` (no extension
        // at all) *or* `deploy.pem` (extension only), and an AND would keep
        // neither.
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-secrets".to_string()]);

        assert!(filter.matches("https://example.com/.env"));
        assert!(filter.matches("https://example.com/app/.env.production"));
        assert!(filter.matches("https://example.com/.git/config"));
        assert!(filter.matches("https://example.com/keys/deploy.pem"));
        assert!(filter.matches("https://example.com/home/.ssh/id_rsa"));

        assert!(!filter.matches("https://example.com/index.html"));
        assert!(!filter.matches("https://example.com/script.js"));
        // "environment" must not be mistaken for a dotfile named ".env".
        assert!(!filter.matches("https://example.com/environment/setup"));
    }

    #[test]
    fn test_only_backup_anchors_the_tilde_to_the_path_end() {
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-backup".to_string()]);

        assert!(filter.matches("https://example.com/index.php~"));
        // A query string must not defeat the suffix rule.
        assert!(filter.matches("https://example.com/index.php~?v=1"));
        assert!(filter.matches("https://example.com/db.sql"));
        assert!(filter.matches("https://example.com/config.php.bak"));

        // A home directory is not a backup, which is why `~` is a suffix rule
        // rather than a substring one.
        assert!(!filter.matches("https://example.com/~john/index.html"));
        assert!(!filter.matches("https://example.com/index.php"));
    }

    #[test]
    fn test_only_api_matches_bare_and_versioned_endpoints() {
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-api".to_string()]);

        assert!(filter.matches("https://example.com/api/v1/users"));
        assert!(filter.matches("https://example.com/graphql"));
        assert!(filter.matches("https://example.com/swagger-ui/index.html"));
        assert!(filter.matches("https://example.com/openapi.json"));
        // No trailing slash: the suffix rule covers what "/api/" cannot.
        assert!(filter.matches("https://example.com/api"));

        assert!(!filter.matches("https://example.com/about.html"));
        assert!(!filter.matches("https://example.com/rapid/deploy"));
    }

    #[test]
    fn test_only_config_matches_named_files_and_extensions() {
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-config".to_string()]);

        assert!(filter.matches("https://example.com/app.yaml"));
        assert!(filter.matches("https://example.com/web.config"));
        assert!(filter.matches("https://example.com/.htaccess"));
        assert!(filter.matches("https://example.com/Dockerfile"));

        assert!(!filter.matches("https://example.com/index.html"));
    }

    #[test]
    fn test_security_presets_still_honour_the_exclusion_flags() {
        // Path rules are an inclusion mechanism; they must not smuggle a URL
        // past --exclude-patterns.
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-secrets".to_string()]);
        filter.with_exclude_patterns(vec!["/vendor/".to_string()]);

        assert!(filter.matches("https://example.com/.env"));
        assert!(!filter.matches("https://example.com/vendor/.env"));
    }

    #[test]
    fn test_path_rules_combine_with_an_extension_preset() {
        // Two presets OR together, as they always have.
        let mut filter = UrlFilter::new();
        filter.apply_presets(&["only-secrets".to_string(), "only-js".to_string()]);

        assert!(filter.matches("https://example.com/.env"));
        assert!(filter.matches("https://example.com/app.js"));
        assert!(!filter.matches("https://example.com/index.html"));
    }

    #[test]
    fn test_extension_only_presets_are_unchanged_by_path_rules() {
        // Regression guard for the 13 original presets: none of them defines a
        // path rule, so the include decision must stay purely extension-based.
        for preset in ["only-js", "only-images", "only-fonts", "only-documents"] {
            let mut filter = UrlFilter::new();
            filter.apply_presets(&[preset.to_string()]);
            assert!(
                !filter.matches("https://example.com/.env"),
                "{preset} must not start matching path shapes"
            );
            assert!(
                !filter.matches("https://example.com/no-extension-here"),
                "{preset} must still reject an extensionless URL"
            );
        }
    }

    #[test]
    fn test_fallback_invalid_urls() {
        let mut filter = UrlFilter::new();
        // Allow js and png
        filter.with_extensions(vec!["js".to_string(), "png".to_string()]);

        let urls: HashSet<String> = vec![
            "script.js",           // Invalid URL, has allowed extension
            "/path/to/image.png",  // Invalid URL, has allowed extension
            "style.css",           // Invalid URL, disallowed extension
            "readme.txt",          // Invalid URL, disallowed extension
            "no_extension",        // Invalid URL, no extension
            "image.png?version=1", // Invalid URL with query param
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let filtered = filter.apply_filters(&urls);

        assert_eq!(filtered.len(), 3);
        assert!(filtered.contains(&"script.js".to_string()));
        assert!(filtered.contains(&"/path/to/image.png".to_string()));
        assert!(filtered.contains(&"image.png?version=1".to_string()));
    }
}
