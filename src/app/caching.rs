//! The cache layer wrapped around a provider run.
//!
//! Two modes share this path. Normally the cache short-circuits domains whose
//! results are still fresh. Under `--incremental` every domain is re-fetched
//! and the cache serves as the baseline to diff against, so only URLs that are
//! genuinely new reach the output.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::app::selection::effective_provider_ids;
use crate::cache::{CacheEntry, CacheFilters, CacheKey, CacheManager, CacheType};
use crate::cli::Args;
use crate::filters::HostValidator;
use crate::progress::ProgressManager;
use crate::providers::Provider;
use crate::runner::{process_domains, ProviderRunResult, ProviderStats, UrlEntry};
use crate::utils::verbose_print;

/// Create the cache manager `--cache-type` asks for, or `None` under
/// `--no-cache`.
pub async fn create_cache_manager(args: &Args) -> Result<Option<CacheManager>> {
    if args.no_cache {
        return Ok(None);
    }

    match args.cache_type {
        CacheType::Sqlite => {
            let cache_path = args
                .cache_path
                .clone()
                .unwrap_or_else(crate::cache::default_sqlite_path);

            verbose_print(
                args,
                format!("Using SQLite cache at: {}", cache_path.display()),
            );
            Ok(Some(CacheManager::new_sqlite(cache_path).await?))
        }
        #[cfg(feature = "redis-cache")]
        CacheType::Redis => {
            let Some(redis_url) = &args.redis_url else {
                if !args.silent {
                    eprintln!("Error: Redis cache type selected but no --redis-url provided");
                }
                return Err(anyhow::anyhow!("Redis URL required for Redis cache type"));
            };
            verbose_print(
                args,
                format!(
                    "Using Redis cache at: {}",
                    crate::cache::redact_redis_url(redis_url)
                ),
            );
            Ok(Some(CacheManager::new_redis(redis_url).await?))
        }
        #[cfg(not(feature = "redis-cache"))]
        CacheType::Redis => {
            if !args.silent {
                eprintln!("Error: Redis cache support not compiled in. Use 'sqlite' or compile with --features redis-cache");
            }
            Err(anyhow::anyhow!("Redis cache not supported"))
        }
    }
}

/// Fingerprint a domain's run so a cached answer is only reused for a query
/// that would have asked the same question.
pub fn create_cache_key(domain: &str, args: &Args) -> CacheKey {
    let filters = CacheFilters {
        subs: args.subs,
        extensions: args.extensions.clone(),
        exclude_extensions: args.exclude_extensions.clone(),
        patterns: args.patterns.clone(),
        exclude_patterns: args.exclude_patterns.clone(),
        match_regex: args.match_regex.clone(),
        filter_regex: args.filter_regex.clone(),
        presets: args.preset.clone(),
        min_length: args.min_length,
        max_length: args.max_length,
        strict: args.strict_enabled(),
        normalize_url: args.normalize_url,
        merge_endpoint: args.merge_endpoint,
        dedup_similar: args.dedup_similar,
        // Archive-side scope. These change what the index returns, so leaving
        // them out would serve a `--from 2020` run the answer cached for a
        // `--from 2024` one.
        cc_index: args.cc_index.clone(),
        from: args.from.clone(),
        to: args.to.clone(),
        archive_status: args.archive_status.clone(),
        archive_exclude_status: args.archive_exclude_status.clone(),
        archive_mime: args.archive_mime.clone(),
        archive_exclude_mime: args.archive_exclude_mime.clone(),
        archived_discovery: args.archived_discovery,
        cdx_endpoints: args.cdx_endpoint.clone(),
        // A dialect with no endpoint to read changes nothing, so it must not
        // change the key either. Keyed by its CLI spelling, not a Rust name.
        cdx_dialect: args
            .cdx_dialect
            .filter(|_| !args.cdx_endpoint.is_empty())
            .and_then(|d| clap::ValueEnum::to_possible_value(&d))
            .map(|v| v.get_name().to_string()),
        archived_discovery_limit: (args.archived_discovery
            && args.archived_discovery_limit != crate::cli::DEFAULT_ARCHIVED_DISCOVERY_LIMIT)
            .then_some(args.archived_discovery_limit),
    };

    CacheKey::new(domain, &effective_provider_ids(args), &filters)
}

/// Collect URLs that truly belong to `domain`, using host validation instead of
/// substring matching so cache entries don't bleed across similar domains or
/// query strings.
pub fn collect_domain_urls(
    urls: &HashMap<String, UrlEntry>,
    domain: &str,
    include_subdomains: bool,
) -> HashSet<String> {
    let validator = HostValidator::new(&[domain.to_string()], include_subdomains);
    urls.keys()
        .filter(|url| validator.is_valid_host(url))
        .cloned()
        .collect()
}

/// URLs on none of `domains`' hosts. `--no-strict` outputs them, so the
/// cache has to keep them too, or a hit returns less than the miss did.
// ponytail: which target's fetch returned one isn't tracked, so a multi-domain
// run cannot attribute them; tag records with their target to cache that case.
fn off_host_urls(
    urls: &HashMap<String, UrlEntry>,
    domains: &[String],
    include_subdomains: bool,
) -> HashSet<String> {
    let validator = HostValidator::new(domains, include_subdomains);
    urls.keys()
        .filter(|url| !validator.is_valid_host(url))
        .cloned()
        .collect()
}

/// Merge one URL's providers and capture metadata into `target`, creating the
/// entry when the URL is new.
///
/// Cached URLs carry neither attribution nor metadata (the cache stores URLs
/// only), so they land with an empty entry rather than a wrong one.
pub(crate) fn merge_entry(target: &mut HashMap<String, UrlEntry>, url: String, entry: &UrlEntry) {
    let slot = target.entry(url).or_default();
    slot.sources.extend(entry.sources.iter().cloned());
    slot.meta.merge(&entry.meta);
}

/// Run every domain, consulting and updating the cache.
pub async fn process_domains_with_cache(
    domains: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    providers: &[Box<dyn Provider>],
    provider_names: &[String],
    cache_manager: Option<&CacheManager>,
) -> Result<ProviderRunResult> {
    let Some(cache) = cache_manager else {
        return Ok(process_domains(
            domains,
            args,
            progress_manager,
            providers,
            provider_names,
            None,
        )
        .await);
    };

    let mut final_result = ProviderRunResult::default();
    let mut domains_to_process = Vec::new();
    // `--stats` gets a row for what the cache served, or a full hit prints
    // no table at all.
    let lookup_started = std::time::Instant::now();
    let mut cached_urls = 0;

    for domain in &domains {
        let cache_key = create_cache_key(domain, args);

        // Incremental runs always re-fetch: the cached set is the baseline they
        // diff against, not a substitute for fetching.
        if !args.incremental {
            if let Some(cached_entry) = cache.get_fresh(&cache_key, args.cache_ttl).await? {
                verbose_print(args, format!("Using cached results for domain: {domain}"));
                // Cached URLs carry no provider or metadata: an empty entry.
                cached_urls += cached_entry.urls.len();
                for url in cached_entry.urls {
                    final_result.urls.entry(url).or_default();
                }
                continue;
            }
        }

        domains_to_process.push(domain.clone());
    }
    let cache_stats = (cached_urls > 0).then(|| ProviderStats {
        name: "cache".to_string(),
        url_count: cached_urls,
        error_count: 0,
        partial_count: 0,
        elapsed: lookup_started.elapsed(),
        aborted: false,
    });

    if domains_to_process.is_empty() {
        final_result.stats.extend(cache_stats);
        // Housekeeping: a failed sweep must not discard results already in hand.
        if let Err(e) = cache.delete_expired(args.cache_ttl.saturating_mul(2)).await {
            if !args.silent {
                crate::utils::stderr_line(&format!("Cache sweep failed: {e}"));
            }
        }
        return Ok(final_result);
    }

    verbose_print(
        args,
        format!(
            "Processing {} domains (cache miss/expired)",
            domains_to_process.len()
        ),
    );

    let fresh_run = process_domains(
        domains_to_process.clone(),
        args,
        progress_manager,
        providers,
        provider_names,
        None,
    )
    .await;

    // Carry the provider stats from the fresh run through to the caller.
    final_result.stats = fresh_run.stats;
    final_result.stats.extend(cache_stats);

    let off_host = if args.strict_enabled() {
        HashSet::new()
    } else {
        off_host_urls(&fresh_run.urls, &domains_to_process, args.subs)
    };
    // Off-host URLs from several fetched targets cannot be filed under the one
    // that returned them; filing them under every target would leak one
    // target's URLs into another's hit. An `--incremental` run files them
    // under every target anyway: dropping them from the baselines would
    // either drop them from the output or report them as new on every run.
    let unattributable = !args.incremental && !off_host.is_empty() && domains_to_process.len() > 1;
    let off_host = if unattributable {
        HashSet::new()
    } else {
        off_host
    };
    let domain_urls = |domain: &str| {
        let mut urls = collect_domain_urls(&fresh_run.urls, domain, args.subs);
        urls.extend(off_host.iter().cloned());
        urls
    };

    if args.incremental {
        for domain in &domains_to_process {
            let cache_key = create_cache_key(domain, args);

            // Emit only what the baseline hadn't seen; it absorbs the rest.
            let new_urls = cache
                .update_baseline(&cache_key, domain_urls(domain))
                .await?;
            if !new_urls.is_empty() {
                verbose_print(
                    args,
                    format!("Found {} new URLs for domain: {domain}", new_urls.len()),
                );
                for url in new_urls {
                    // A URL the diff reported as new always came from the fresh
                    // run, so the lookup hits; defaulting keeps the entry
                    // present rather than dropping the URL if it ever misses.
                    let entry = fresh_run.urls.get(&url).cloned().unwrap_or_default();
                    merge_entry(&mut final_result.urls, url, &entry);
                }
            }
        }
    } else {
        for (url, entry) in &fresh_run.urls {
            merge_entry(&mut final_result.urls, url.clone(), entry);
        }

        // Storing without the off-host URLs would make the hit return less
        // than this run did. Skip the store.
        if unattributable {
            verbose_print(
                args,
                "Not caching: --no-strict URLs from several targets cannot be attributed",
            );
        }
        for domain in domains_to_process.iter().filter(|_| !unattributable) {
            // A partial answer stored here would be served as the whole one
            // for the entire TTL.
            if fresh_run.incomplete_domains.contains(domain) {
                verbose_print(
                    args,
                    format!("Not caching incomplete results for domain: {domain}"),
                );
                continue;
            }
            let domain_urls: Vec<String> = domain_urls(domain).into_iter().collect();

            if !domain_urls.is_empty() {
                let cache_key = create_cache_key(domain, args);
                let entry = CacheEntry::new(domain_urls);
                cache.store_urls(&cache_key, &entry).await?;
            }
        }
    }

    // Saturating: `--cache-ttl` is an unvalidated u64, and `* 2` on a large one
    // overflows (a debug-build panic, a wrap in release).
    if let Err(e) = cache.delete_expired(args.cache_ttl.saturating_mul(2)).await {
        if !args.silent {
            crate::utils::stderr_line(&format!("Cache sweep failed: {e}"));
        }
    }

    Ok(final_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keys::KEYED_PROVIDER_IDS;
    use crate::cache;
    use crate::test_support::{build_test_args, EnvGuard, MockProvider, ENV};

    struct FailingCacheBackend;

    #[async_trait::async_trait]
    impl cache::CacheBackend for FailingCacheBackend {
        async fn get(&self, _key: &CacheKey) -> Result<Option<CacheEntry>> {
            Err(anyhow::anyhow!("cache get failed"))
        }

        async fn set(&self, _key: &CacheKey, _entry: &CacheEntry) -> Result<()> {
            Err(anyhow::anyhow!("cache set failed"))
        }

        async fn delete(&self, _key: &CacheKey) -> Result<()> {
            Err(anyhow::anyhow!("cache delete failed"))
        }
    }

    #[tokio::test]
    async fn test_create_cache_manager_is_none_under_no_cache() {
        let mut args = build_test_args();
        args.no_cache = true;

        // `CacheManager` isn't `Debug`, so match rather than unwrap.
        match create_cache_manager(&args).await {
            Ok(manager) => assert!(manager.is_none()),
            Err(e) => panic!("--no-cache should short-circuit, got {e}"),
        }
    }

    #[tokio::test]
    async fn test_process_domains_with_cache_surfaces_write_errors_but_not_read_errors() {
        // A failed read is a miss, so the run still fetches; a failed write
        // still fails the run.
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(MockProvider::new(
            vec!["https://example.com/page1".to_string()],
            false,
        ))];
        let provider_names = vec!["MockProvider".to_string()];
        let cache = CacheManager::new_for_test(Box::new(FailingCacheBackend));
        let args = build_test_args();

        let err = process_domains_with_cache(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &provider_names,
            Some(&cache),
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("cache set failed"), "{err}");
    }

    #[tokio::test]
    async fn test_process_domains_without_cache_runs_providers_directly() {
        let provider = MockProvider::new(vec!["https://example.com/a".to_string()], false);
        let calls = provider.calls.clone();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];

        let result = process_domains_with_cache(
            vec!["example.com".to_string()],
            &build_test_args(),
            &ProgressManager::new(true),
            &providers,
            &["MockProvider".to_string()],
            None,
        )
        .await
        .unwrap();

        assert_eq!(calls.lock().unwrap().len(), 1);
        assert!(result.urls.contains_key("https://example.com/a"));
    }

    #[test]
    fn test_cache_key_uses_effective_provider_ids() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env-vt")]);
        let _unset = EnvGuard::unset(&[
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
            "URX_BEVIGIL_API_KEY",
        ]);

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.include_robots = true;
        args.exclude_robots = false;
        args.include_sitemap = false;
        args.exclude_sitemap = true;

        // vt joins via its key and robots via --include-robots, so the key
        // covers providers the user never named on --providers.
        let key = create_cache_key("example.com", &args);
        assert_eq!(key.providers, vec!["robots", "vt", "wayback"]);
    }

    #[test]
    fn test_cache_key_changes_with_archive_scope() {
        let _env_lock = ENV.lock().unwrap();
        let _guard = EnvGuard::unset(&[
            "URX_VT_API_KEY",
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
            "URX_BEVIGIL_API_KEY",
        ]);

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];

        let baseline = create_cache_key("example.com", &args);

        args.from = Some("2024".to_string());
        let narrowed = create_cache_key("example.com", &args);

        assert_ne!(
            baseline.filters_hash, narrowed.filters_hash,
            "--from must not reuse an unscoped run's cached answer"
        );
    }

    #[test]
    fn test_cache_key_tells_cdx_collections_and_discovery_limits_apart() {
        // Regression: the provider id names only the endpoint's host, so two
        // collections on one server shared a cache entry.
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        let hash = |args: &Args| create_cache_key("example.com", args).filters_hash;
        let plain = hash(&args);

        args.cdx_endpoint = vec!["https://archive.example/colA/cdx".to_string()];
        let col_a = hash(&args);
        args.cdx_endpoint = vec!["https://archive.example/colB/cdx".to_string()];
        assert_ne!(col_a, hash(&args));

        args.cdx_dialect = Some(crate::providers::CdxDialect::Classic);
        let classic = hash(&args);
        args.cdx_dialect = Some(crate::providers::CdxDialect::Pywb);
        assert_ne!(classic, hash(&args));
        // ...and a dialect with no endpoint to apply to keys like none.
        args.cdx_endpoint.clear();
        assert_eq!(plain, hash(&args));

        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.archived_discovery = true;
        let default_limit = hash(&args);
        args.archived_discovery_limit = 5;
        assert_ne!(default_limit, hash(&args));
        // The limit means nothing without archived discovery.
        args.archived_discovery = false;
        assert_eq!(plain, hash(&args));
    }

    /// A cache that actually stores, so cache-hit paths can be exercised.
    #[derive(Default)]
    struct MemoryCacheBackend {
        entries: std::sync::Mutex<HashMap<String, CacheEntry>>,
    }

    impl MemoryCacheBackend {
        fn id(key: &CacheKey) -> String {
            format!(
                "{}|{}|{}",
                key.domain,
                key.providers.join(","),
                key.filters_hash
            )
        }
    }

    #[async_trait::async_trait]
    impl cache::CacheBackend for MemoryCacheBackend {
        async fn get(&self, key: &CacheKey) -> Result<Option<CacheEntry>> {
            Ok(self.entries.lock().unwrap().get(&Self::id(key)).cloned())
        }

        async fn set(&self, key: &CacheKey, entry: &CacheEntry) -> Result<()> {
            self.entries
                .lock()
                .unwrap()
                .insert(Self::id(key), entry.clone());
            Ok(())
        }

        async fn delete(&self, key: &CacheKey) -> Result<()> {
            self.entries.lock().unwrap().remove(&Self::id(key));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_incremental_fetches_each_domain_exactly_once_on_a_cache_hit() {
        // Regression: a cache hit under --incremental used to push the domain
        // onto the work list twice — once in the "still need fresh data" branch
        // and again in the fall-through — doubling every provider request for
        // an identical result.
        let cache = CacheManager::new_for_test(Box::<MemoryCacheBackend>::default());
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        // Pin the cache key against a developer's ambient API keys, which would
        // otherwise auto-enable providers and change it. Excluding beats an env
        // guard here: this test awaits, and the env lock isn't async-aware.
        args.exclude_providers = KEYED_PROVIDER_IDS.iter().map(|s| s.to_string()).collect();
        args.incremental = true;

        // Seed the cache so the domain is a hit on the run below.
        let key = create_cache_key("example.com", &args);
        cache
            .store_urls(
                &key,
                &CacheEntry::new(vec!["https://example.com/old".to_string()]),
            )
            .await
            .unwrap();

        let provider = MockProvider::new(
            vec![
                "https://example.com/old".to_string(),
                "https://example.com/new".to_string(),
            ],
            false,
        );
        let calls = provider.calls.clone();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(provider)];

        let result = process_domains_with_cache(
            vec!["example.com".to_string()],
            &args,
            &ProgressManager::new(true),
            &providers,
            &["MockProvider".to_string()],
            Some(&cache),
        )
        .await
        .unwrap();

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            ["example.com"],
            "a cached domain must be fetched once, not once per code path"
        );
        // Only the URL the baseline hadn't seen is reported.
        assert_eq!(
            result.urls.keys().collect::<Vec<_>>(),
            vec!["https://example.com/new"]
        );
    }

    /// Run `providers` over example.com through the cache.
    async fn run_cached(
        cache: &CacheManager,
        args: &Args,
        providers: &[Box<dyn Provider>],
    ) -> ProviderRunResult {
        let names: Vec<String> = (0..providers.len()).map(|i| format!("p{i}")).collect();
        process_domains_with_cache(
            vec!["example.com".to_string()],
            args,
            &ProgressManager::new(true),
            providers,
            &names,
            Some(cache),
        )
        .await
        .unwrap()
    }

    /// Args whose cache key ignores a developer's ambient API keys; see
    /// `test_incremental_fetches_each_domain_exactly_once_on_a_cache_hit`.
    fn pinned_args() -> Args {
        let mut args = build_test_args();
        args.providers = vec!["wayback".to_string()];
        args.exclude_providers = KEYED_PROVIDER_IDS.iter().map(|s| s.to_string()).collect();
        args
    }

    #[tokio::test]
    async fn test_a_domain_a_provider_failed_on_is_not_cached() {
        // Regression: a run with provider errors (or cut short by --max-time)
        // was cached like any other, and its subset served as the whole answer
        // for the entire TTL.
        let cache = CacheManager::new_for_test(Box::<MemoryCacheBackend>::default());
        let args = pinned_args();
        let providers: Vec<Box<dyn Provider>> = vec![
            Box::new(MockProvider::new(
                vec!["https://example.com/a".to_string()],
                false,
            )),
            Box::new(MockProvider::new(vec![], true)),
        ];

        let result = run_cached(&cache, &args, &providers).await;

        assert!(result.urls.contains_key("https://example.com/a"));
        let key = create_cache_key("example.com", &args);
        assert!(cache.get_fresh(&key, 3600).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_no_strict_cache_hit_returns_what_the_miss_did() {
        // Regression: the cache stored only host-validated URLs even under
        // --no-strict, so a hit dropped the off-host URLs the miss had output.
        let cache = CacheManager::new_for_test(Box::<MemoryCacheBackend>::default());
        let mut args = pinned_args();
        args.no_strict = true;
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(MockProvider::new(
            vec![
                "https://example.com/a".to_string(),
                "https://cdn.other.test/x".to_string(),
            ],
            false,
        ))];

        let sorted = |r: &ProviderRunResult| {
            let mut urls: Vec<String> = r.urls.keys().cloned().collect();
            urls.sort();
            urls
        };
        let miss = sorted(&run_cached(&cache, &args, &providers).await);
        let hit_run = run_cached(&cache, &args, &providers).await;
        let hit = sorted(&hit_run);
        assert_eq!(miss.len(), 2);
        assert_eq!(hit, miss);
        // `--stats` on a full hit reports what the cache served.
        let rows: Vec<_> = hit_run
            .stats
            .iter()
            .map(|r| (r.name.as_str(), r.url_count))
            .collect();
        assert_eq!(rows, [("cache", 2)]);
    }

    #[tokio::test]
    async fn test_incremental_no_strict_multi_target_keeps_off_host_urls() {
        // Regression: off-host URLs from several targets were withheld from
        // the cache, and with it from the incremental output.
        let cache = CacheManager::new_for_test(Box::<MemoryCacheBackend>::default());
        let mut args = pinned_args();
        args.no_strict = true;
        args.incremental = true;
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(MockProvider::new(
            vec![
                "https://example.com/a".to_string(),
                "https://cdn.other.test/x".to_string(),
            ],
            false,
        ))];

        let progress = ProgressManager::new(true);
        let names = ["p0".to_string()];
        let run = || {
            process_domains_with_cache(
                vec!["example.com".to_string(), "example.org".to_string()],
                &args,
                &progress,
                &providers,
                &names,
                Some(&cache),
            )
        };
        let first = run().await.unwrap();
        assert!(first.urls.contains_key("https://example.com/a"));
        assert!(first.urls.contains_key("https://cdn.other.test/x"));
        // ...and, once seen, they are not new again.
        assert!(run().await.unwrap().urls.is_empty());
    }

    #[test]
    fn test_collect_domain_urls_matches_host_only() {
        let urls = HashMap::from([
            ("https://example.com/path".to_string(), UrlEntry::default()),
            (
                "https://notexample.com/redirect?next=example.com".to_string(),
                UrlEntry::default(),
            ),
            (
                "https://example.com.evil.test/path".to_string(),
                UrlEntry::default(),
            ),
            (
                "https://api.example.com/path".to_string(),
                UrlEntry::default(),
            ),
        ]);

        let exact = collect_domain_urls(&urls, "example.com", false);
        assert_eq!(
            exact,
            HashSet::from(["https://example.com/path".to_string()])
        );

        let with_subdomains = collect_domain_urls(&urls, "example.com", true);
        assert_eq!(
            with_subdomains,
            HashSet::from([
                "https://example.com/path".to_string(),
                "https://api.example.com/path".to_string(),
            ])
        );
    }
}
