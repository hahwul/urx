//! The registry of every provider urx knows about.
//!
//! One table drives `--list-providers`, the meaning of `--all-providers`, the
//! set of ids the CLI will accept, and the display names shown in stats — so a
//! new provider is added in exactly one place.
//!
//! The one thing the table cannot hold is a provider that only exists once the
//! user names it: `--cdx-endpoint URL` turns any CDX server into a provider
//! with id `cdx:<host>`. Those ids are derived from the arguments in exactly
//! one place too — [`cdx_endpoints`] — and every consumer of the static table
//! (validation, `--list-providers`, selection) folds them in from there.

use anyhow::Result;

use crate::app::keys::{api_key_env_var, api_key_flag};
use crate::cli::Args;
use crate::providers::cdx::normalize_endpoint;

/// Prefix of the provider ids `--cdx-endpoint` servers get: `cdx:<host>`.
pub const CDX_ENDPOINT_ID_PREFIX: &str = "cdx:";

/// The provider id for one `--cdx-endpoint` URL: `cdx:<host>`, plus `:<port>`
/// when the URL names one, so two servers on one host stay distinguishable.
/// The host is what the operator reads in `--stats` and `--show-sources`, and
/// what they type in `--exclude-providers` / `--rate-limit-by`.
pub fn cdx_endpoint_id(endpoint: &str) -> Result<String> {
    let normalized = normalize_endpoint(endpoint)?;
    // `normalize_endpoint` already proved this parses and has a host.
    let url = url::Url::parse(&normalized)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid --cdx-endpoint {endpoint:?}: no host"))?
        .to_ascii_lowercase();
    Ok(match url.port() {
        Some(port) => format!("{CDX_ENDPOINT_ID_PREFIX}{host}:{port}"),
        None => format!("{CDX_ENDPOINT_ID_PREFIX}{host}"),
    })
}

/// True for an id minted by [`cdx_endpoint_id`].
pub fn is_cdx_endpoint_id(id: &str) -> bool {
    id.starts_with(CDX_ENDPOINT_ID_PREFIX)
}

/// Every `--cdx-endpoint` this run configured, as `(id, normalized URL)` in
/// flag order. A URL given twice (or two URLs mapping to one id) is kept once,
/// so the same server is never queried twice per domain. Fails on the first
/// malformed URL so a typo is reported at startup, not once per domain.
pub fn cdx_endpoints(args: &Args) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for raw in &args.cdx_endpoint {
        let id = cdx_endpoint_id(raw)?;
        if out.iter().any(|(existing, _)| *existing == id) {
            continue;
        }
        out.push((id, normalize_endpoint(raw)?));
    }
    Ok(out)
}

/// Just the ids of [`cdx_endpoints`], for callers that only need to know which
/// dynamic ids exist.
pub fn cdx_endpoint_ids(args: &Args) -> Result<Vec<String>> {
    Ok(cdx_endpoints(args)?.into_iter().map(|(id, _)| id).collect())
}

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
            id: "bevigil",
            display_name: "BeVigil",
            requires_key: true,
            summary: "URLs extracted from unpacked Android apps (URX_BEVIGIL_API_KEY)",
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
/// in the catalog — a `cdx:<host>` endpoint, whose id already names the
/// server, or a synthetic test id.
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
///
/// The static table is followed by the one family it cannot enumerate: the
/// `cdx:<host>` providers `--cdx-endpoint` creates. Their row is always shown
/// (so the option is discoverable), and any endpoint named on this very
/// command line is listed underneath with the id it will run as.
pub fn print_provider_list(args: &Args) {
    print!("{}", render_provider_list(args));
}

/// The `--list-providers` text. Separate from the printing so tests can read
/// it.
fn render_provider_list(args: &Args) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "Available providers:");
    let _ = writeln!(
        out,
        "  {:<11}  {:<16}  {:<8}  description",
        "id", "name", "key"
    );
    let _ = writeln!(
        out,
        "  {:<11}  {:<16}  {:<8}  -----------",
        "-----------", "----------------", "--------"
    );
    for p in provider_catalog() {
        let _ = writeln!(
            out,
            "  {:<11}  {:<16}  {:<8}  {}",
            p.id,
            p.display_name,
            if p.requires_key { "required" } else { "—" },
            p.summary
        );
    }
    let _ = writeln!(
        out,
        "  {:<11}  {:<16}  {:<8}  Any pywb / OutbackCDX / classic CDX server, via --cdx-endpoint URL (see --cdx-dialect)",
        "cdx:<host>", "Custom CDX", "—",
    );
    match cdx_endpoints(args) {
        Ok(endpoints) if !endpoints.is_empty() => {
            let _ = writeln!(out);
            let _ = writeln!(out, "Configured --cdx-endpoint providers:");
            for (id, endpoint) in endpoints {
                let _ = writeln!(out, "  {id:<24}  {endpoint}");
            }
        }
        Ok(_) => {}
        Err(e) => {
            let _ = writeln!(out);
            let _ = writeln!(out, "{e}");
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Use --providers id1,id2 to select. --all-providers enables every entry"
    );
    let _ = writeln!(
        out,
        "(API-keyed providers only activate when a key is available)."
    );
    let _ = writeln!(
        out,
        "--cdx-endpoint providers are enabled by naming them. --exclude-providers wins on conflict."
    );
    out
}

/// Every provider id the CLI accepts, sorted for stable error messages.
pub fn valid_provider_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = provider_catalog().iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids
}

/// Reject unknown ids passed to a provider-selecting flag, naming both the
/// offenders and the full set of allowed values.
///
/// `dynamic_ids` are the `cdx:<host>` ids this run configured (see
/// [`cdx_endpoint_ids`]); they are accepted alongside the catalog and listed
/// with it, so an operator who mistypes one sees the spelling urx expects.
pub fn validate_provider_ids(
    ids: &[String],
    flag_name: &str,
    dynamic_ids: &[String],
) -> Result<()> {
    let mut allowed: Vec<&str> = valid_provider_ids();
    allowed.extend(dynamic_ids.iter().map(String::as_str));

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
pub fn validate_rate_limit_override_ids(args: &Args, dynamic_ids: &[String]) -> Result<()> {
    let (overrides, malformed) = args.parse_rate_limit_overrides();
    if !malformed.is_empty() {
        return Err(anyhow::anyhow!(
            "Malformed entr{} in --rate-limit-by: {}. Expected id=requests-per-second with a positive rate, e.g. wayback=5",
            if malformed.len() == 1 { "y" } else { "ies" },
            malformed.join(", ")
        ));
    }
    let override_ids: Vec<String> = overrides.into_keys().collect();
    validate_provider_ids(&override_ids, "--rate-limit-by", dynamic_ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keys::KEYED_PROVIDER_IDS;
    use crate::test_support::build_test_args;

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

        let msg = missing_api_key_message("bevigil");
        assert!(msg.contains("The BeVigil provider (bevigil)"), "{msg}");
        assert!(msg.contains("--bevigil-api-key"), "{msg}");
        assert!(msg.contains("URX_BEVIGIL_API_KEY"), "{msg}");
    }

    #[test]
    fn test_validate_provider_ids_lists_offenders_and_allowed_values() {
        let err = validate_provider_ids(&["wayback".into(), "bogus".into()], "--providers", &[])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Unknown provider id(s) in --providers"),
            "{err}"
        );
        assert!(err.contains("bogus"), "{err}");
        assert!(err.contains("wayback"), "allowed list should appear: {err}");

        assert!(validate_provider_ids(&["wayback".into()], "--providers", &[]).is_ok());
        assert!(validate_provider_ids(&[], "--providers", &[]).is_ok());
    }

    #[test]
    fn test_validate_provider_ids_accepts_configured_cdx_endpoints() {
        let dynamic = vec!["cdx:vefsafn.is".to_string()];
        assert!(validate_provider_ids(&["cdx:vefsafn.is".into()], "--providers", &dynamic).is_ok());

        // ...but only the ones this run configured: an endpoint id out of thin
        // air is as unknown as any other typo, and the allowed list names the
        // real one.
        let err = validate_provider_ids(&["cdx:other.org".into()], "--providers", &dynamic)
            .unwrap_err()
            .to_string();
        assert!(err.contains("cdx:other.org"), "{err}");
        assert!(err.contains("cdx:vefsafn.is"), "{err}");
        assert!(validate_provider_ids(&["cdx:vefsafn.is".into()], "--providers", &[]).is_err());
    }

    #[test]
    fn test_cdx_endpoint_ids_are_derived_from_the_host() {
        assert_eq!(
            cdx_endpoint_id("https://vefsafn.is/cdx").unwrap(),
            "cdx:vefsafn.is"
        );
        assert_eq!(
            cdx_endpoint_id("HTTPS://Vefsafn.IS/cdx?").unwrap(),
            "cdx:vefsafn.is"
        );
        assert_eq!(
            cdx_endpoint_id("http://localhost:8080/cdx").unwrap(),
            "cdx:localhost:8080"
        );
        assert!(cdx_endpoint_id("vefsafn.is/cdx").is_err());
        assert!(is_cdx_endpoint_id("cdx:vefsafn.is"));
        assert!(!is_cdx_endpoint_id("wayback"));
    }

    #[test]
    fn test_cdx_endpoints_dedupe_and_report_the_first_bad_url() {
        let mut args = build_test_args();
        args.cdx_endpoint = vec![
            "https://vefsafn.is/cdx".to_string(),
            "https://vefsafn.is/cdx?".to_string(),
            "http://localhost:8080/cdx".to_string(),
        ];
        assert_eq!(
            cdx_endpoints(&args).unwrap(),
            vec![
                (
                    "cdx:vefsafn.is".to_string(),
                    "https://vefsafn.is/cdx".to_string()
                ),
                (
                    "cdx:localhost:8080".to_string(),
                    "http://localhost:8080/cdx".to_string()
                ),
            ]
        );

        args.cdx_endpoint.push("not a url".to_string());
        let err = cdx_endpoints(&args).unwrap_err().to_string();
        assert!(err.contains("Invalid --cdx-endpoint"), "{err}");
        assert!(err.contains("not a url"), "{err}");
    }

    #[test]
    fn test_list_providers_shows_the_cdx_family_and_configured_endpoints() {
        let mut args = build_test_args();
        let listing = render_provider_list(&args);
        assert!(listing.contains("cdx:<host>"), "{listing}");
        assert!(listing.contains("--cdx-endpoint"), "{listing}");
        assert!(
            !listing.contains("Configured --cdx-endpoint"),
            "nothing configured: {listing}"
        );
        // Every static entry is still there.
        for p in provider_catalog() {
            assert!(listing.contains(p.id), "{}: {listing}", p.id);
        }

        args.cdx_endpoint = vec!["https://vefsafn.is/cdx".to_string()];
        let listing = render_provider_list(&args);
        assert!(listing.contains("Configured --cdx-endpoint"), "{listing}");
        assert!(
            listing.contains("cdx:vefsafn.is") && listing.contains("https://vefsafn.is/cdx"),
            "{listing}"
        );
    }

    #[test]
    fn test_provider_display_name_falls_back_to_the_id() {
        assert_eq!(provider_display_name("wayback"), "Wayback Machine");
        assert_eq!(provider_display_name("mock"), "mock");
    }
}
