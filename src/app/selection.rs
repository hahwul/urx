//! Deciding which providers run, and building them.
//!
//! Selection is deliberately split from construction: [`effective_provider_ids`]
//! is pure and cheap, so the cache key can ask "which providers would this run
//! use?" without instantiating a single HTTP client.

use std::collections::HashSet;

use anyhow::Result;

use crate::app::catalog::{
    missing_api_key_message, provider_catalog, valid_provider_ids, validate_provider_ids,
    validate_rate_limit_override_ids,
};
use crate::app::keys::{auto_enable_provider, ApiKeys, KEYED_PROVIDER_IDS};
use crate::cli::Args;
use crate::filters::validate_presets;
use crate::network::NetworkSettings;
use crate::providers::{
    self, ArquivoProvider, CommonCrawlProvider, GitHubProvider, OTXProvider, Provider,
    RobotsProvider, SitemapProvider, UrlscanProvider, VirusTotalProvider, WaybackMachineProvider,
    ZoomEyeProvider,
};
use crate::runner::add_provider;

/// The built providers alongside their display names, positionally aligned.
pub type ProviderList = (Vec<Box<dyn Provider>>, Vec<String>);

/// Providers that speak a CDX API and can therefore honour `--from/--to` and
/// the `--archive-*` predicates. Everything else silently ignores them, so we
/// warn when the user asked for filters no selected provider can apply.
const CDX_PROVIDERS: [&str; 3] = ["wayback", "cc", "arquivo"];

/// Providers whose index server is pywb-derived, and therefore cannot express
/// a multi-value positive filter (see [`providers::ArchiveFilters`]).
const PYWB_PROVIDERS: [&str; 2] = ["cc", "arquivo"];

/// Trim the ids named by a provider-selecting flag.
///
/// clap splits `--providers` on commas but keeps the surrounding spaces, so the
/// entirely natural `--providers "wayback, cc"` used to fail with
/// `Unknown provider id(s) in --providers:  cc. Allowed values: ..., cc, ...`
/// — an error that names the value it is simultaneously rejecting. An entry
/// that is empty *after* trimming is left in place so it is still reported,
/// rather than turning `--providers ""` into a silently different selection.
fn trimmed_ids(raw: &[String]) -> Vec<String> {
    raw.iter().map(|id| id.trim().to_string()).collect()
}

/// Which providers this run would use, given the flags and available API keys.
///
/// Pure and side-effect free — [`crate::app::caching::create_cache_key`] relies
/// on that to fingerprint a run without building anything.
pub fn effective_provider_ids(args: &Args) -> Vec<String> {
    let keys = ApiKeys::resolve(args);

    let mut providers_list: Vec<String> = if args.all_providers {
        provider_catalog()
            .iter()
            // A keyed provider only joins `--all-providers` once its key exists;
            // robots/sitemap are per-target probes, opted into separately.
            .filter(|p| !p.requires_key || !keys.for_provider(p.id).is_empty())
            .filter(|p| p.id != "robots" && p.id != "sitemap")
            .map(|p| p.id.to_string())
            .collect()
    } else {
        trimmed_ids(&args.providers)
    };

    if !args.all_providers {
        // Supplying a key is taken as intent to use that provider. Silent here:
        // this function also runs for cache-key construction, where announcing
        // the same thing a second time would be noise.
        for id in KEYED_PROVIDER_IDS {
            auto_enable_provider(&mut providers_list, keys.for_provider(id), id, false, true);
        }
    }

    let excluded_ids = trimmed_ids(&args.exclude_providers);
    let excluded: HashSet<&str> = excluded_ids.iter().map(String::as_str).collect();
    providers_list.retain(|p| !excluded.contains(p.as_str()));

    for (id, requested) in [
        ("robots", args.should_use_robots()),
        ("sitemap", args.should_use_sitemap()),
    ] {
        if requested && !excluded.contains(id) && !providers_list.iter().any(|p| p == id) {
            providers_list.push(id.to_string());
        }
    }

    providers_list
}

/// Display label for one Common Crawl index instance. A raw `CC-MAIN-…` id is
/// already self-identifying, but aliases like `latest` are not — prefix those
/// so the progress line and verbose log say which provider they belong to.
pub fn cc_provider_label(index: &str) -> String {
    if index.to_ascii_uppercase().starts_with("CC-") {
        index.to_string()
    } else {
        format!("CC ({index})")
    }
}

/// Translate the `--from/--to/--archive-*` flags into [`providers::ArchiveFilters`],
/// warning once about any date we could not parse rather than failing the run.
pub fn build_archive_filters(args: &Args) -> providers::ArchiveFilters {
    let parse_date = |raw: &str, flag: &str, end_of_range: bool| {
        let parsed = providers::normalize_cdx_timestamp(raw, end_of_range);
        if parsed.is_none() && !args.silent {
            eprintln!(
                "Ignoring {flag}={raw:?}: expected YYYY, YYYYMM, YYYYMMDD, or YYYYMMDDhhmmss"
            );
        }
        parsed
    };

    providers::ArchiveFilters::from_cli_lists(
        args.from
            .as_deref()
            .and_then(|s| parse_date(s, "--from", false)),
        args.to.as_deref().and_then(|s| parse_date(s, "--to", true)),
        &args.archive_status,
        &args.archive_exclude_status,
        &args.archive_mime,
        &args.archive_exclude_mime,
    )
}

/// Warn about `--from/--to/--archive-*` flags that will not take effect, rather
/// than letting them be silently inert.
fn warn_about_inert_archive_filters(
    args: &Args,
    providers_list: &[String],
    filters: &providers::ArchiveFilters,
) {
    if filters.is_empty() || args.silent {
        return;
    }

    if !providers_list
        .iter()
        .any(|p| CDX_PROVIDERS.contains(&p.as_str()))
    {
        eprintln!(
            "Warning: --from/--to/--archive-* apply only to CDX-backed providers ({}); none are enabled, so they will have no effect.",
            CDX_PROVIDERS.join(", ")
        );
        return;
    }

    // Common Crawl and Arquivo.pt match filter values exactly and AND repeated
    // filters together, so "200 or 301" is unsatisfiable there. urx drops such
    // a filter for those providers instead of sending a query that would come
    // back empty and read as "the archive has nothing".
    let affected: Vec<&str> = providers_list
        .iter()
        .map(String::as_str)
        .filter(|p| PYWB_PROVIDERS.contains(p))
        .collect();
    if affected.is_empty() {
        return;
    }
    let unsupported = filters.unsupported_positives(providers::CdxDialect::Pywb);
    if unsupported.is_empty() {
        return;
    }
    eprintln!(
        "Warning: {} accept only a single value for {} (their index matches exactly, with no OR); \
         that filter is skipped for them and applied on wayback only.",
        affected.join(" and "),
        unsupported.join(" / ")
    );
}

/// Validate every provider-selecting flag before any network client is built,
/// so a typo fails immediately instead of after minutes of fetching.
fn validate_selection_flags(args: &Args) -> Result<()> {
    validate_provider_ids(&trimmed_ids(&args.providers), "--providers")?;
    // A misspelled preset used to be dropped in silence, producing an
    // unfiltered run that looked like the filter had matched everything.
    validate_presets(&args.preset)?;
    validate_provider_ids(&trimmed_ids(&args.exclude_providers), "--exclude-providers")?;
    validate_rate_limit_override_ids(args)
}

/// Build every provider this run selected, in catalog order.
pub fn initialize_providers(
    args: &Args,
    network_settings: &NetworkSettings,
) -> Result<ProviderList> {
    validate_selection_flags(args)?;

    let keys = ApiKeys::resolve(args);
    let selected = effective_provider_ids(args);
    let enabled: HashSet<&str> = selected.iter().map(String::as_str).collect();

    // Predicates the archives evaluate for us. Built once so a malformed date
    // warns a single time rather than once per provider per domain.
    let archive_filters = build_archive_filters(args);
    warn_about_inert_archive_filters(args, &selected, &archive_filters);

    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let mut names: Vec<String> = Vec::new();

    // Every provider is registered the same way; the macro keeps the shared
    // `args`/`network_settings`/accumulator arguments out of ten call sites.
    macro_rules! register {
        ($id:literal, $label:expr, $builder:expr) => {
            add_provider(
                args,
                network_settings,
                &mut providers,
                &mut names,
                $id,
                $label,
                $builder,
            )
        };
    }

    if enabled.contains("wayback") {
        let filters = archive_filters.clone();
        register!("wayback", "Wayback Machine".to_string(), move || {
            let mut p = WaybackMachineProvider::new();
            p.with_filters(filters);
            p
        });
    }

    if enabled.contains("cc") {
        // Each --cc-index entry becomes its own provider instance so they
        // run in parallel and the per-provider stats stay distinct.
        for index in &args.cc_index {
            let index = index.clone();
            let label = cc_provider_label(&index);
            let filters = archive_filters.clone();
            register!("cc", label, move || {
                let mut p = CommonCrawlProvider::with_index(index);
                p.with_filters(filters);
                p
            });
        }
    }

    if enabled.contains("robots") {
        register!("robots", "Robots.txt".to_string(), RobotsProvider::new);
    }

    if enabled.contains("sitemap") {
        register!("sitemap", "Sitemap".to_string(), SitemapProvider::new);
    }

    if enabled.contains("otx") {
        register!("otx", "OTX".to_string(), OTXProvider::new);
    }

    if enabled.contains("arquivo") {
        let filters = archive_filters.clone();
        register!("arquivo", "Arquivo.pt".to_string(), move || {
            let mut p = ArquivoProvider::new();
            p.with_filters(filters);
            p
        });
    }

    // From here on the order matches the catalog's keyed section, which is also
    // the order provider rows appear in `--stats` and the progress region.
    //
    // These three cannot run at all without a key. `--all-providers` users
    // don't want a noisy error for every key they happen not to have, so the
    // complaint is suppressed in that mode.
    if enabled.contains("vt") {
        let vt_keys = keys.vt.clone();
        if !vt_keys.is_empty() {
            register!("vt", "VirusTotal".to_string(), || {
                VirusTotalProvider::new_with_keys(vt_keys)
            });
        } else if !args.silent && !args.all_providers {
            eprintln!("{}", missing_api_key_message("vt"));
        }
    }

    if enabled.contains("urlscan") {
        // urlscan.io's public search works without a key (rate-limited to
        // ~30 req/min per IP); a key only raises those limits and enables
        // rotation. So always instantiate — keys are passed through when
        // present, but their absence no longer disables the provider.
        let keys = keys.urlscan.clone();
        register!("urlscan", "Urlscan".to_string(), || {
            UrlscanProvider::new_with_keys(keys)
        });
    }

    if enabled.contains("zoomeye") {
        let zoomeye_keys = keys.zoomeye.clone();
        if !zoomeye_keys.is_empty() {
            register!("zoomeye", "ZoomEye".to_string(), || {
                ZoomEyeProvider::new_with_keys(zoomeye_keys)
            });
        } else if !args.silent && !args.all_providers {
            eprintln!("{}", missing_api_key_message("zoomeye"));
        }
    }

    if enabled.contains("github") {
        let github_keys = keys.github.clone();
        if !github_keys.is_empty() {
            register!("github", "GitHub".to_string(), || {
                GitHubProvider::new_with_keys(github_keys)
            });
        } else if !args.silent && !args.all_providers {
            eprintln!("{}", missing_api_key_message("github"));
        }
    }

    if providers.is_empty() {
        // Returned, not printed-and-returned: `main` reports the error too, so
        // the old eprintln made every no-provider run emit the message twice —
        // and the copy `--silent` suppressed was the one carrying the guidance.
        return Err(anyhow::anyhow!(
            "No valid providers specified. Please use --providers with valid provider names ({})",
            valid_provider_ids().join(", ")
        ));
    }

    Ok((providers, names))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{build_test_args, env_mutex, EnvGuard};
    use clap::Parser;

    /// The four keyed providers' environment variables, cleared so a developer's
    /// real keys can't change what a selection test observes.
    const KEYED_ENV: [&str; 4] = [
        "URX_VT_API_KEY",
        "URX_URLSCAN_API_KEY",
        "URX_ZOOMEYE_API_KEY",
        "URX_GITHUB_API_KEY",
    ];

    /// Run `initialize_providers` expecting rejection, and return the message.
    /// `Result::unwrap_err` is unavailable here — `dyn Provider` isn't `Debug`.
    fn selection_error(args: &Args) -> String {
        match initialize_providers(args, &NetworkSettings::default()) {
            Ok(_) => panic!("expected provider initialization to fail"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn test_cc_provider_label() {
        // Aliases carry no provider hint on their own, so they get one.
        assert_eq!(cc_provider_label("latest"), "CC (latest)");
        assert_eq!(cc_provider_label("LATEST"), "CC (LATEST)");
        // A real index id already reads as Common Crawl; don't double up.
        assert_eq!(cc_provider_label("CC-MAIN-2026-17"), "CC-MAIN-2026-17");
        assert_eq!(cc_provider_label("cc-main-2023-06"), "cc-main-2023-06");
    }

    #[test]
    fn test_build_archive_filters_maps_every_flag() {
        let args = Args::parse_from([
            "urx",
            "--from",
            "2020",
            "--to",
            "2021",
            "--archive-status",
            "200",
            "--archive-exclude-status",
            "404,500",
            "--archive-mime",
            "application/json",
            "--archive-exclude-mime",
            "text/html",
            "example.com",
        ]);
        let f = build_archive_filters(&args);

        // Partial dates pad toward opposite ends of the range.
        assert_eq!(f.from.as_deref(), Some("20200101000000"));
        assert_eq!(f.to.as_deref(), Some("20211231235959"));
        assert_eq!(f.status, vec!["200"]);
        assert_eq!(f.exclude_status, vec!["404", "500"]);
        assert_eq!(f.mime, vec!["application/json"]);
        assert_eq!(f.exclude_mime, vec!["text/html"]);
    }

    #[test]
    fn test_build_archive_filters_ignores_unparseable_date() {
        // A bad date warns and is dropped rather than aborting the whole run.
        let args = Args::parse_from(["urx", "--silent", "--from", "not-a-date", "example.com"]);
        let f = build_archive_filters(&args);
        assert!(f.from.is_none());
        assert!(f.is_empty());
    }

    #[test]
    fn test_archive_filters_reach_both_cdx_dialects() {
        // Ties the CLI flags to the wire format each archive actually accepts,
        // which was verified against the live servers: web.archive.org wants
        // `statuscode`/`mimetype`, Common Crawl and Arquivo.pt want
        // `status`/`mime`.
        let args = Args::parse_from([
            "urx",
            "--archive-status",
            "200",
            "--archive-exclude-mime",
            "text/html",
            "example.com",
        ]);
        let f = build_archive_filters(&args);

        let classic = f.query_params(providers::CdxDialect::Classic);
        assert!(classic.contains("&filter=statuscode:200"), "{classic}");
        assert!(
            classic.contains("&filter=!mimetype:text%2Fhtml"),
            "{classic}"
        );

        let pywb = f.query_params(providers::CdxDialect::Pywb);
        assert!(pywb.contains("&filter=status:200"), "{pywb}");
        assert!(pywb.contains("&filter=!mime:text%2Fhtml"), "{pywb}");
    }

    #[test]
    fn test_legacy_wayback_date_flags_still_feed_archive_filters() {
        let args = Args::parse_from(["urx", "--wayback-from", "2020", "example.com"]);
        assert_eq!(
            build_archive_filters(&args).from.as_deref(),
            Some("20200101000000")
        );
    }

    #[test]
    fn test_initialize_providers_rejects_unknown_provider_ids() {
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string(), "bogus".to_string()];

        let err = selection_error(&args);
        assert!(
            err.contains("Unknown provider id(s) in --providers"),
            "{err}"
        );
    }

    #[test]
    fn test_initialize_providers_rejects_unknown_excluded_provider_ids() {
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.exclude_providers = vec!["bogus".to_string()];

        let err = selection_error(&args);
        assert!(
            err.contains("Unknown provider id(s) in --exclude-providers"),
            "{err}"
        );
    }

    #[test]
    fn test_initialize_providers_rejects_unknown_rate_limit_override_ids() {
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.rate_limit_by = vec!["bogus=1".to_string()];

        let err = selection_error(&args);
        assert!(
            err.contains("Unknown provider id(s) in --rate-limit-by"),
            "{err}"
        );
    }

    #[test]
    fn test_initialize_providers_errors_when_nothing_was_selected() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.providers = vec![];

        let err = selection_error(&args);
        assert!(err.contains("No valid providers specified"), "{err}");
    }

    #[test]
    fn test_initialize_providers_enables_urlscan_without_api_key() {
        // urlscan is keyless: requesting it with no API key must still
        // instantiate the provider (regression guard for the removed key gate).
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&["URX_URLSCAN_API_KEY"]);

        let mut args = build_test_args();
        args.providers = vec!["urlscan".to_string()];

        let (providers, names) = initialize_providers(&args, &NetworkSettings::default())
            .expect("urlscan should initialize without an API key");
        assert!(
            !providers.is_empty(),
            "urlscan must be instantiated even without a key"
        );
        assert!(names.iter().any(|n| n == "Urlscan"));
    }

    #[test]
    fn test_initialize_providers_skips_keyed_provider_without_a_key() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string(), "vt".to_string()];

        let (providers, names) = initialize_providers(&args, &NetworkSettings::default()).unwrap();
        assert_eq!(providers.len(), 1, "vt has no key and must be skipped");
        assert_eq!(names, vec!["Wayback Machine"]);
    }

    #[test]
    fn test_initialize_providers_builds_one_instance_per_cc_index() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.providers = vec!["cc".to_string()];
        args.cc_index = vec!["CC-MAIN-2026-17".to_string(), "latest".to_string()];

        let (providers, names) = initialize_providers(&args, &NetworkSettings::default()).unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(names, vec!["CC-MAIN-2026-17", "CC (latest)"]);
    }

    #[test]
    fn test_effective_provider_ids_all_providers_keyless() {
        // --all-providers with no keys must enable every keyless provider
        // (including arquivo and the now-keyless urlscan) while keeping the
        // keyed providers disabled.
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.all_providers = true;
        args.providers = vec![]; // ignored when --all-providers is set

        let ids = effective_provider_ids(&args);

        for id in ["wayback", "cc", "otx", "arquivo", "urlscan"] {
            assert!(
                ids.iter().any(|p| p == id),
                "--all-providers (keyless) must enable {id}; got {ids:?}"
            );
        }
        for id in ["vt", "zoomeye", "github"] {
            assert!(
                !ids.iter().any(|p| p == id),
                "keyed provider {id} must not activate without a key; got {ids:?}"
            );
        }
    }

    #[test]
    fn test_exclude_providers_wins_over_auto_enable() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "some-key")]);

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.exclude_providers = vec!["vt".to_string()];

        let ids = effective_provider_ids(&args);
        assert!(
            !ids.iter().any(|p| p == "vt"),
            "--exclude-providers must beat key-driven auto-enable; got {ids:?}"
        );
    }

    #[test]
    fn test_provider_flags_tolerate_whitespace_around_commas() {
        // Regression: clap splits on ',' but keeps the spaces, so
        // `--providers "wayback, cc"` failed with
        // "Unknown provider id(s) in --providers:  cc. Allowed values: ..., cc, ..."
        // — an error naming the very value it rejected.
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let args = Args::parse_from([
            "urx",
            "--providers",
            " wayback , cc ",
            "--exclude-providers",
            " cc ",
            "--exclude-robots",
            "--exclude-sitemap",
            "example.com",
        ]);

        assert_eq!(effective_provider_ids(&args), vec!["wayback"]);
        let (providers, names) = initialize_providers(&args, &NetworkSettings::default())
            .expect("whitespace around ids must not be an error");
        assert_eq!(providers.len(), 1);
        assert_eq!(names, vec!["Wayback Machine"]);
    }

    #[test]
    fn test_empty_provider_id_is_still_rejected() {
        // Trimming must not quietly reinterpret an explicitly empty selection.
        let mut args = build_test_args();
        args.providers = vec![String::new()];
        assert!(
            selection_error(&args).contains("Unknown provider id(s) in --providers"),
            "an empty id must still be reported"
        );
    }

    #[test]
    fn test_no_provider_error_carries_the_guidance_exactly_once() {
        // The message used to be printed by this function *and* reported again
        // by `main`, so a failing run showed it twice — and `--silent`
        // suppressed the copy that carried the list of valid ids, leaving only
        // the bare "No valid providers specified".
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.silent = true;
        args.providers = vec![];

        let err = selection_error(&args);
        assert!(err.contains("No valid providers specified"), "{err}");
        assert!(
            err.contains("wayback"),
            "the allowed ids must travel with the error: {err}"
        );
    }

    #[test]
    fn test_robots_and_sitemap_join_only_when_requested() {
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&KEYED_ENV);

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        assert_eq!(effective_provider_ids(&args), vec!["wayback"]);

        args.include_robots = true;
        args.exclude_robots = false;
        assert_eq!(effective_provider_ids(&args), vec!["wayback", "robots"]);

        // ...and an explicit exclusion still wins.
        args.exclude_robots = true;
        assert_eq!(effective_provider_ids(&args), vec!["wayback"]);
    }
}
