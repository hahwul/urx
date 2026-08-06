//! The registry of every provider urx knows about.
//!
//! One table drives `--list-providers`, the meaning of `--all-providers`, the
//! set of ids the CLI will accept, and the display names shown in stats — so a
//! new provider is added in exactly one place.

use anyhow::Result;

use crate::app::keys::{api_key_env_var, api_key_flag};
use crate::cli::Args;

/// Static metadata for one of urx's URL providers.
pub struct ProviderInfo {
    /// Short identifier accepted on the command line (e.g. "wayback").
    pub id: &'static str,
    /// Human-readable display name shown in stats and `--list-providers`.
    pub display_name: &'static str,
    /// True when the provider can only be enabled with an API key.
    pub requires_key: bool,
    /// One-line description shown by `--list-providers`.
    pub summary: &'static str,
}

/// Catalog of every provider urx knows about. The order here drives the
/// `--list-providers` output and the meaning of `--all-providers`.
pub fn provider_catalog() -> &'static [ProviderInfo] {
    &[
        ProviderInfo {
            id: "wayback",
            display_name: "Wayback Machine",
            requires_key: false,
            summary: "Internet Archive CDX index",
        },
        ProviderInfo {
            id: "cc",
            display_name: "Common Crawl",
            requires_key: false,
            summary: "Common Crawl monthly URL index",
        },
        ProviderInfo {
            id: "otx",
            display_name: "OTX",
            requires_key: false,
            summary: "AlienVault Open Threat Exchange passive DNS / URLs",
        },
        ProviderInfo {
            id: "arquivo",
            display_name: "Arquivo.pt",
            requires_key: false,
            summary: "Arquivo.pt Portuguese web archive CDX index",
        },
        ProviderInfo {
            id: "vt",
            display_name: "VirusTotal",
            requires_key: true,
            summary: "VirusTotal observed URLs (URX_VT_API_KEY)",
        },
        ProviderInfo {
            id: "urlscan",
            display_name: "Urlscan",
            requires_key: false,
            summary: "Urlscan.io search (anonymous; URX_URLSCAN_API_KEY raises rate limits)",
        },
        ProviderInfo {
            id: "zoomeye",
            display_name: "ZoomEye",
            requires_key: true,
            summary: "ZoomEye search (URX_ZOOMEYE_API_KEY)",
        },
        ProviderInfo {
            id: "github",
            display_name: "GitHub",
            requires_key: true,
            summary: "GitHub Code Search (URX_GITHUB_API_KEY)",
        },
        ProviderInfo {
            id: "robots",
            display_name: "robots.txt",
            requires_key: false,
            summary: "Discovery from the target's robots.txt",
        },
        ProviderInfo {
            id: "sitemap",
            display_name: "sitemap.xml",
            requires_key: false,
            summary: "Discovery from the target's sitemap.xml",
        },
    ]
}

/// Look up a provider's entry by its command-line id.
pub fn provider_info(id: &str) -> Option<&'static ProviderInfo> {
    provider_catalog().iter().find(|p| p.id == id)
}

/// The provider's human-readable name, falling back to the id for anything not
/// in the catalog (only reachable from tests, which use synthetic ids).
pub fn provider_display_name(id: &str) -> &str {
    provider_info(id).map_or(id, |p| p.display_name)
}

/// The error shown when a key-gated provider was requested without a key.
///
/// Derived from the catalog rather than written out per provider, so the flag
/// and environment-variable names can never drift from what the CLI accepts.
pub fn missing_api_key_message(id: &str) -> String {
    format!(
        "Error: The {} provider ({id}) requires an API key. Please use {} or set the {} environment variable.",
        provider_display_name(id),
        api_key_flag(id),
        api_key_env_var(id),
    )
}

/// Print the provider catalog to stdout in a `--list-providers` format.
pub fn print_provider_list() {
    println!("Available providers:");
    println!("  {:<9}  {:<16}  {:<8}  description", "id", "name", "key");
    println!(
        "  {:<9}  {:<16}  {:<8}  -----------",
        "---------", "----------------", "--------"
    );
    for p in provider_catalog() {
        println!(
            "  {:<9}  {:<16}  {:<8}  {}",
            p.id,
            p.display_name,
            if p.requires_key { "required" } else { "—" },
            p.summary
        );
    }
    println!();
    println!("Use --providers id1,id2 to select. --all-providers enables every entry");
    println!("(API-keyed providers only activate when a key is available).");
    println!("--exclude-providers wins on conflict.");
}

/// Every provider id the CLI accepts, sorted for stable error messages.
pub fn valid_provider_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = provider_catalog().iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids
}

/// Reject unknown ids passed to a provider-selecting flag, naming both the
/// offenders and the full set of allowed values.
pub fn validate_provider_ids(ids: &[String], flag_name: &str) -> Result<()> {
    let allowed = valid_provider_ids();

    let unknown: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|id| !allowed.contains(id))
        .collect();

    if unknown.is_empty() {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Unknown provider id(s) in {flag_name}: {}. Allowed values: {}",
        unknown.join(", "),
        allowed.join(", ")
    ))
}

/// Validate `--rate-limit-by`, which carries provider ids inside `id=rate`
/// pairs and so needs its syntax checked before its ids.
pub fn validate_rate_limit_override_ids(args: &Args) -> Result<()> {
    let (overrides, malformed) = args.parse_rate_limit_overrides();
    if !malformed.is_empty() {
        return Err(anyhow::anyhow!(
            "Malformed entr{} in --rate-limit-by: {}. Expected id=requests-per-second with a positive rate, e.g. wayback=5",
            if malformed.len() == 1 { "y" } else { "ies" },
            malformed.join(", ")
        ));
    }
    let override_ids: Vec<String> = overrides.into_keys().collect();
    validate_provider_ids(&override_ids, "--rate-limit-by")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keys::KEYED_PROVIDER_IDS;

    #[test]
    fn test_catalog_ids_are_unique_and_sorted_for_errors() {
        let ids = valid_provider_ids();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate provider id in catalog");
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn test_every_key_gated_catalog_entry_is_a_known_keyed_provider() {
        // The catalog's `requires_key` flag and the keyed-provider list in
        // `keys` have to agree, or a provider gets a key it never reads.
        for p in provider_catalog() {
            if p.requires_key {
                assert!(
                    KEYED_PROVIDER_IDS.contains(&p.id),
                    "{} requires a key but has no key source",
                    p.id
                );
            }
        }
    }

    #[test]
    fn test_missing_api_key_message_matches_the_real_flag_and_env_var() {
        let msg = missing_api_key_message("vt");
        assert!(msg.contains("The VirusTotal provider (vt)"), "{msg}");
        assert!(msg.contains("--vt-api-key"), "{msg}");
        assert!(msg.contains("URX_VT_API_KEY"), "{msg}");

        let msg = missing_api_key_message("github");
        assert!(msg.contains("The GitHub provider (github)"), "{msg}");
        assert!(msg.contains("--github-api-key"), "{msg}");
        assert!(msg.contains("URX_GITHUB_API_KEY"), "{msg}");
    }

    #[test]
    fn test_validate_provider_ids_lists_offenders_and_allowed_values() {
        let err = validate_provider_ids(&["wayback".into(), "bogus".into()], "--providers")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Unknown provider id(s) in --providers"),
            "{err}"
        );
        assert!(err.contains("bogus"), "{err}");
        assert!(err.contains("wayback"), "allowed list should appear: {err}");

        assert!(validate_provider_ids(&["wayback".into()], "--providers").is_ok());
        assert!(validate_provider_ids(&[], "--providers").is_ok());
    }

    #[test]
    fn test_provider_display_name_falls_back_to_the_id() {
        assert_eq!(provider_display_name("wayback"), "Wayback Machine");
        assert_eq!(provider_display_name("mock"), "mock");
    }
}
