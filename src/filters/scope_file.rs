//! Bug-bounty scope files: a list of hosts to keep, and hosts to drop.
//!
//! A program's scope is a list of hosts, and every bug bounty platform writes
//! it the same way — `*.example.com` for a wildcard, a bare host for a single
//! target, and a note about the handful of subdomains that are explicitly out
//! of scope. Expressing that with `--filter-regex` means hand-translating it
//! into anchored alternations on every run, and getting the anchoring wrong
//! silently widens the scope rather than failing.
//!
//! ```text
//! # in scope
//! *.example.com
//! api.example.org
//!
//! # out of scope, even though the wildcard above covers them
//! !admin.example.com
//! !*.internal.example.com
//! ```
//!
//! # The rules
//!
//! * A line is a host pattern, optionally prefixed with `!` to exclude.
//! * `*.example.com` matches `example.com` and every host under it. Matching
//!   the apex is the bug-bounty reading rather than the DNS one, and it is the
//!   one every platform's scope table means; a program that really excludes its
//!   apex says so with a `!example.com` line, which wins.
//! * A bare `example.com` matches that host and nothing else — not `www.`, not
//!   any subdomain. A scope file is an explicit list, so the `www`-counts-as-
//!   apex leniency [`super::HostValidator`] applies to a *queried domain* would
//!   silently widen it here.
//! * A lone `*` matches every host, for a file that is purely a deny-list.
//! * Everything from a `#` to the end of the line is a comment, so an entry can
//!   be annotated in place — which is how a scope table copied off a platform
//!   usually reads. Blank lines are skipped.
//! * **Exclusion always wins**, mirroring `--filter-regex` beating
//!   `--match-regex`.
//! * A file with no include lines at all is a pure deny-list: everything is in
//!   scope except what it excludes.
//!
//! Anything else — a port, a path, a wildcard in the middle — is a startup
//! error. That is the deliberate choice: this filter decides which hosts a user
//! is willing to touch, so a line urx cannot honour has to stop the run rather
//! than be dropped into a wider scope than the file describes.
//!
//! # Relationship to `--strict`
//!
//! [`super::HostValidator`] (`--strict`, on by default) and this are separate
//! gates and a URL must pass both. Host validation answers "does this URL
//! belong to a domain I queried?"; a scope file answers "is this host one I am
//! allowed to touch?". They usually agree, but not always: `--scope-file` with
//! `*.example.com` while querying the bare apex still needs `--subs`, because
//! strict mode drops the subdomains before the scope file ever sees them.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use url::Url;

use super::host_validation::normalize_domain;

/// One host pattern from a scope file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum HostPattern {
    /// A lone `*`: every host.
    Any,
    /// A bare host, matched exactly.
    Exact(String),
    /// `*.example.com`, stored as `example.com`: that host and anything under it.
    Suffix(String),
}

impl HostPattern {
    fn matches(&self, host: &str) -> bool {
        match self {
            HostPattern::Any => true,
            HostPattern::Exact(want) => host == want,
            HostPattern::Suffix(base) => host == base || host.ends_with(&format!(".{base}")),
        }
    }
}

/// Parse one already-trimmed, non-comment scope line into a pattern plus
/// whether it excludes.
///
/// The host half goes through the same normalization [`super::HostValidator`]
/// applies to a target domain, so an IDN scope line matches the punycode host
/// `Url::parse` reports and case/trailing-dot differences fold away.
fn parse_line(line: &str) -> Result<(bool, HostPattern)> {
    let (negated, rest) = match line.strip_prefix('!') {
        Some(rest) => (true, rest.trim()),
        None => (false, line),
    };

    if rest.is_empty() {
        bail!("expected a host after '!'");
    }
    if rest == "*" {
        return Ok((negated, HostPattern::Any));
    }

    // A pasted scope entry often arrives as a URL. The scheme carries no host
    // information and is dropped, but a *path* does carry scope information urx
    // cannot honour — silently widening `example.com/api/*` to the whole host
    // would put out-of-scope endpoints back in the result set.
    let rest = rest
        .split_once("://")
        .map_or(rest, |(_, after)| after)
        .trim_end_matches('/');
    if rest.contains(['/', '?', '#']) {
        bail!(
            "path-scoped entries are not supported; list the host here and use \
             --match-regex / --filter-regex for the path"
        );
    }
    if rest.contains('@') {
        bail!("userinfo is not part of a host");
    }

    let (wildcard, host) = match rest.strip_prefix("*.") {
        Some(host) => (true, host),
        None => (false, rest),
    };
    if host.contains('*') {
        bail!("'*' is only supported as a leading '*.' wildcard, or on its own");
    }
    if host.contains(':') {
        bail!("a port is not part of a host; drop the ':port' suffix");
    }

    let host = normalize_domain(host).with_context(|| format!("{host:?} is not a host"))?;

    Ok((
        negated,
        if wildcard {
            HostPattern::Suffix(host)
        } else {
            HostPattern::Exact(host)
        },
    ))
}

/// The include/exclude host lists loaded from one or more `--scope-file`s.
///
/// Several files union: each contributes its includes and its excludes to the
/// same two sets, so `--scope-file core.txt --scope-file extra.txt` behaves as
/// if the two files were concatenated. Exclusion still wins across files, which
/// means a shared "never touch these" file can be layered onto any program's
/// scope.
#[derive(Debug, Clone, Default)]
pub struct ScopeMatcher {
    include: Vec<HostPattern>,
    exclude: Vec<HostPattern>,
}

impl ScopeMatcher {
    /// Load every `--scope-file` path, or `None` when the flag wasn't used.
    ///
    /// A file that cannot be read, or that holds a line urx cannot honour, is
    /// an error naming the file and line number — see the module docs for why
    /// this is fatal rather than a warning.
    pub fn from_files(paths: &[PathBuf]) -> Result<Option<ScopeMatcher>> {
        if paths.is_empty() {
            return Ok(None);
        }
        let mut matcher = ScopeMatcher::default();
        for path in paths {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read scope file: {}", path.display()))?;
            matcher.extend_from_str(&text, path)?;
        }
        Ok(Some(matcher))
    }

    /// Parse the contents of one scope file into this matcher. `path` only
    /// names the file in error messages.
    fn extend_from_str(&mut self, text: &str, path: &Path) -> Result<()> {
        for (index, raw) in text.lines().enumerate() {
            // U+FEFF is not `White_Space`, so `trim` leaves it on the first
            // line of a file saved by Notepad or PowerShell's `>` — where it
            // would turn the first scope entry into a host that matches nothing.
            // A `#` cannot appear in a host, so taking everything before the
            // first one supports both a whole-line comment and the trailing
            // annotation a scope table copied off a platform usually carries.
            let line = raw
                .trim()
                .trim_start_matches('\u{feff}')
                .split('#')
                .next()
                .unwrap_or("")
                .trim();
            if line.is_empty() {
                continue;
            }
            let (negated, pattern) = parse_line(line).with_context(|| {
                format!(
                    "Invalid scope entry at {}:{}: {:?}",
                    path.display(),
                    index + 1,
                    line
                )
            })?;
            let bucket = if negated {
                &mut self.exclude
            } else {
                &mut self.include
            };
            if !bucket.contains(&pattern) {
                bucket.push(pattern);
            }
        }
        Ok(())
    }

    /// Whether `url`'s host is in scope.
    ///
    /// A URL with no parseable host is out of scope, matching how
    /// [`super::HostValidator`] treats one: there is no host to check against
    /// the list, and a scope file exists to be conservative.
    pub fn is_in_scope(&self, url: &str) -> bool {
        let Some(host) = Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        else {
            return false;
        };
        let host = host.trim_end_matches('.');

        // Exclusion wins, and is checked first so a `!` line is authoritative
        // no matter which include covered the host.
        if self.exclude.iter().any(|p| p.matches(host)) {
            return false;
        }
        // A file of nothing but exclusions is a deny-list, not a scope of zero
        // hosts — otherwise layering a shared "never touch these" file onto a
        // run would discard the entire result set.
        self.include.is_empty() || self.include.iter().any(|p| p.matches(host))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn matcher(text: &str) -> ScopeMatcher {
        let mut m = ScopeMatcher::default();
        m.extend_from_str(text, Path::new("scope.txt")).unwrap();
        m
    }

    fn scope_error(text: &str) -> String {
        let mut m = ScopeMatcher::default();
        format!(
            "{:#}",
            m.extend_from_str(text, Path::new("scope.txt"))
                .expect_err("expected this scope file to be rejected")
        )
    }

    #[test]
    fn a_wildcard_covers_the_apex_and_every_subdomain() {
        let m = matcher("*.example.com\n");
        assert!(m.is_in_scope("https://example.com/a"));
        assert!(m.is_in_scope("https://api.example.com/a"));
        assert!(m.is_in_scope("https://deep.api.example.com/a"));
        // ...and nothing that merely looks like it.
        assert!(!m.is_in_scope("https://example.com.evil.tld/a"));
        assert!(!m.is_in_scope("https://notexample.com/a"));
        assert!(!m.is_in_scope("https://evil-example.com/a"));
    }

    #[test]
    fn a_bare_host_matches_only_itself() {
        let m = matcher("api.example.org\n");
        assert!(m.is_in_scope("https://api.example.org/v1"));
        assert!(!m.is_in_scope("https://example.org/v1"));
        assert!(!m.is_in_scope("https://v2.api.example.org/v1"));
        // Explicitly *not* the www leniency HostValidator grants a queried
        // domain: a scope file is a list, not a target.
        assert!(!m.is_in_scope("https://www.api.example.org/v1"));
    }

    #[test]
    fn exclusion_beats_the_wildcard_that_covers_it() {
        let m = matcher(
            "*.example.com\n\
             !admin.example.com\n\
             !*.internal.example.com\n",
        );
        assert!(m.is_in_scope("https://www.example.com/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
        // The negated wildcard takes the apex of its own subtree with it...
        assert!(!m.is_in_scope("https://internal.example.com/a"));
        assert!(!m.is_in_scope("https://db.internal.example.com/a"));
        // ...but a sibling is untouched.
        assert!(m.is_in_scope("https://external.example.com/a"));
    }

    #[test]
    fn an_excluded_apex_overrides_the_wildcards_apex_reading() {
        // The escape hatch for a program whose wildcard really does exclude the
        // bare domain.
        let m = matcher("*.example.com\n!example.com\n");
        assert!(!m.is_in_scope("https://example.com/a"));
        assert!(m.is_in_scope("https://api.example.com/a"));
    }

    #[test]
    fn a_file_of_only_exclusions_is_a_deny_list() {
        let m = matcher("!admin.example.com\n");
        assert!(m.is_in_scope("https://example.com/a"));
        assert!(m.is_in_scope("https://anything.else.tld/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
    }

    #[test]
    fn a_lone_star_includes_everything() {
        let m = matcher("*\n!admin.example.com\n");
        assert!(m.is_in_scope("https://example.com/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
    }

    #[test]
    fn comments_blank_lines_and_a_bom_are_skipped() {
        let m = matcher(
            "\u{feff}# program scope\n\
             \n\
             *.example.com\n\
             \t\n\
             # out of scope\n\
             !admin.example.com\n",
        );
        assert!(m.is_in_scope("https://api.example.com/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
        assert_eq!(m.include.len(), 1);
        assert_eq!(m.exclude.len(), 1);
    }

    #[test]
    fn an_entry_can_carry_a_trailing_comment() {
        // How a scope table pasted off a platform actually reads.
        let m = matcher(
            "*.example.com      # everything under the apex\n\
             !admin.example.com # staff only -- out of scope\n",
        );
        assert!(m.is_in_scope("https://api.example.com/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
        assert_eq!(m.include.len(), 1);
        assert_eq!(m.exclude.len(), 1);
    }

    #[test]
    fn a_bom_on_the_first_scope_entry_does_not_corrupt_it() {
        let m = matcher("\u{feff}*.example.com\n");
        assert!(m.is_in_scope("https://api.example.com/a"));
    }

    #[test]
    fn entries_are_normalized_the_way_target_domains_are() {
        let m = matcher(
            "*.EXAMPLE.com.\n\
             https://api.example.org/\n\
             café.com\n",
        );
        assert!(m.is_in_scope("https://API.Example.Com/a"));
        assert!(m.is_in_scope("https://api.example.org/a"));
        // IDN: the scope line is Unicode, the URL's host is punycode.
        assert!(m.is_in_scope("https://xn--caf-dma.com/a"));
        assert!(m.is_in_scope("https://café.com/a"));
    }

    #[test]
    fn a_url_with_no_host_is_out_of_scope() {
        let m = matcher("*\n");
        assert!(!m.is_in_scope("not-a-url"));
        assert!(!m.is_in_scope("mailto:user@example.com"));
        assert!(!m.is_in_scope("file:///etc/passwd"));
    }

    #[test]
    fn a_trailing_dot_on_the_url_host_still_matches() {
        let m = matcher("*.example.com\n");
        assert!(m.is_in_scope("https://api.example.com./a"));
    }

    #[test]
    fn several_files_union_and_exclusions_carry_across() {
        let mut m = ScopeMatcher::default();
        m.extend_from_str("*.example.com\n", Path::new("a.txt"))
            .unwrap();
        m.extend_from_str("api.example.org\n!admin.example.com\n", Path::new("b.txt"))
            .unwrap();

        assert!(m.is_in_scope("https://www.example.com/a"));
        assert!(m.is_in_scope("https://api.example.org/a"));
        // A shared deny-list file layered onto another file's scope.
        assert!(!m.is_in_scope("https://admin.example.com/a"));
    }

    #[test]
    fn duplicate_entries_across_files_are_stored_once() {
        let mut m = ScopeMatcher::default();
        m.extend_from_str("*.example.com\n", Path::new("a.txt"))
            .unwrap();
        m.extend_from_str("*.EXAMPLE.COM\n", Path::new("b.txt"))
            .unwrap();
        assert_eq!(m.include.len(), 1);
    }

    #[test]
    fn a_path_scoped_entry_is_rejected_rather_than_widened() {
        // Silently reading this as "all of example.com" would put endpoints the
        // program excluded back into the result set.
        let err = scope_error("https://example.com/api/*\n");
        assert!(err.contains("path-scoped"), "{err}");
        assert!(err.contains("scope.txt:1"), "{err}");
    }

    #[test]
    fn an_unsupported_wildcard_shape_is_rejected() {
        let err = scope_error("api.*.example.com\n");
        assert!(err.contains("leading '*.'"), "{err}");
        assert!(scope_error("exa*mple.com\n").contains("leading '*.'"));
    }

    #[test]
    fn a_port_or_userinfo_is_rejected() {
        assert!(scope_error("example.com:8080\n").contains("port"));
        assert!(scope_error("user@example.com\n").contains("userinfo"));
    }

    #[test]
    fn a_bare_bang_and_an_unusable_host_are_rejected() {
        assert!(scope_error("!\n").contains("expected a host"));
        let err = scope_error("...\n");
        assert!(err.contains("is not a host"), "{err}");
    }

    #[test]
    fn the_error_names_the_file_and_the_offending_line() {
        let err = scope_error("*.example.com\n\n# ok so far\nexample.com:443\n");
        assert!(err.contains("scope.txt:4"), "{err}");
        assert!(err.contains("example.com:443"), "{err}");
    }

    #[test]
    fn from_files_returns_none_without_the_flag() {
        assert!(ScopeMatcher::from_files(&[]).unwrap().is_none());
    }

    #[test]
    fn from_files_reads_and_unions_real_files() {
        let mut a = tempfile::NamedTempFile::new().unwrap();
        writeln!(a, "*.example.com\n!admin.example.com").unwrap();
        let mut b = tempfile::NamedTempFile::new().unwrap();
        writeln!(b, "# shared deny-list\n!*.internal.example.com").unwrap();

        let m = ScopeMatcher::from_files(&[a.path().to_path_buf(), b.path().to_path_buf()])
            .unwrap()
            .expect("a scope file was given");

        assert!(m.is_in_scope("https://api.example.com/a"));
        assert!(!m.is_in_scope("https://admin.example.com/a"));
        assert!(!m.is_in_scope("https://db.internal.example.com/a"));
    }

    #[test]
    fn a_missing_scope_file_names_itself_in_the_error() {
        let err = ScopeMatcher::from_files(&[PathBuf::from("/nonexistent/scope.txt")])
            .expect_err("a scope file that cannot be read must stop the run");
        assert!(
            format!("{err:#}").contains("/nonexistent/scope.txt"),
            "{err:#}"
        );
    }
}
