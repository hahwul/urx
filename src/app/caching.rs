//! The cache layer wrapped around a provider run.
//!
//! Two modes share this path. Normally the cache short-circuits domains whose
//! results are still fresh. Under `--incremental` every domain is re-fetched
//! and the cache serves as the baseline to diff against, so only URLs that are
//! genuinely new reach the output.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::app::selection::effective_provider_ids;
use crate::cache::{CacheEntry, CacheFilters, CacheKey, CacheManager};
use crate::cli::Args;
use crate::filters::HostValidator;
use crate::progress::ProgressManager;
use crate::providers::Provider;
use crate::runner::{process_domains, ProviderRunResult};
use crate::utils::verbose_print;

/// Create the cache manager `--cache-type` asks for, or `None` under
/// `--no-cache`.
pub async fn create_cache_manager(args: &Args) -> Result<Option<CacheManager>> {
    if args.no_cache {
        return Ok(None);
    }

    match args.cache_type.as_str() {
        "sqlite" => {
            let cache_path = args.cache_path.clone().unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                std::path::PathBuf::from(home).join(".urx").join("cache.db")
            });

            verbose_print(
                args,
                format!("Using SQLite cache at: {}", cache_path.display()),
            );
            Ok(Some(CacheManager::new_sqlite(cache_path).await?))
        }
        #[cfg(feature = "redis-cache")]
        "redis" => {
            let Some(redis_url) = &args.redis_url else {
                if !args.silent {
                    eprintln!("Error: Redis cache type selected but no --redis-url provided");
                }
                return Err(anyhow::anyhow!("Redis URL required for Redis cache type"));
            };
            verbose_print(args, format!("Using Redis cache at: {redis_url}"));
            Ok(Some(CacheManager::new_redis(redis_url).await?))
        }
        #[cfg(not(feature = "redis-cache"))]
        "redis" => {
            if !args.silent {
                eprintln!("Error: Redis cache support not compiled in. Use 'sqlite' or compile with --features redis-cache");
            }
            Err(anyhow::anyhow!("Redis cache not supported"))
        }
        other => {
            if !args.silent {
                eprintln!("Error: Unknown cache type '{other}'. Use 'sqlite' or 'redis'");
            }
            Err(anyhow::anyhow!("Invalid cache type"))
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
        presets: args.preset.clone(),
        min_length: args.min_length,
        max_length: args.max_length,
        strict: args.strict_enabled(),
        normalize_url: args.normalize_url,
        merge_endpoint: args.merge_endpoint,
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
    };

    CacheKey::new(domain, &effective_provider_ids(args), &filters)
}

/// Collect URLs that truly belong to `domain`, using host validation instead of
/// substring matching so cache entries don't bleed across similar domains or
/// query strings.
pub fn collect_domain_urls(
    urls: &HashMap<String, HashSet<String>>,
    domain: &str,
    include_subdomains: bool,
) -> HashSet<String> {
    let validator = HostValidator::new(&[domain.to_string()], include_subdomains);
    urls.keys()
        .filter(|url| validator.is_valid_host(url))
        .cloned()
        .collect()
}

/// Merge `sources` into `target`, creating the entry when the URL is new.
///
/// Cached URLs carry no attribution (the cache stores URLs only), so they land
/// with an empty provider set rather than a wrong one.
fn merge_urls<I>(target: &mut HashMap<String, HashSet<String>>, url: String, sources: I)
where
    I: IntoIterator<Item = String>,
{
    target.entry(url).or_default().extend(sources);
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

    for domain in &domains {
        let cache_key = create_cache_key(domain, args);

        // Incremental runs always re-fetch: the cached set is the baseline they
        // diff against, not a substitute for fetching.
        if !args.incremental && cache.is_valid(&cache_key, args.cache_ttl).await? {
            if let Some(cached_entry) = cache.get_cached_urls(&cache_key).await? {
                verbose_print(args, format!("Using cached results for domain: {domain}"));
                for url in cached_entry.urls {
                    merge_urls(&mut final_result.urls, url, []);
                }
                continue;
            }
        }

        domains_to_process.push(domain.clone());
    }

    if domains_to_process.is_empty() {
        cache
            .cleanup_expired(args.cache_ttl.saturating_mul(2))
            .await?;
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

    if args.incremental {
        for domain in &domains_to_process {
            let cache_key = create_cache_key(domain, args);
            let domain_fresh_urls = collect_domain_urls(&fresh_run.urls, domain, args.subs);

            // Emit only what the previous run hadn't seen...
            let new_urls = cache.get_new_urls(&cache_key, &domain_fresh_urls).await?;
            if !new_urls.is_empty() {
                verbose_print(
                    args,
                    format!("Found {} new URLs for domain: {domain}", new_urls.len()),
                );
                for url in new_urls {
                    let sources = fresh_run.urls.get(&url).cloned().unwrap_or_default();
                    merge_urls(&mut final_result.urls, url, sources);
                }
            }

            // ...but store the full set, so it is the baseline next time.
            let entry = CacheEntry::new(domain_fresh_urls.into_iter().collect());
            cache.store_urls(&cache_key, &entry).await?;
        }
    } else {
        for (url, sources) in &fresh_run.urls {
            merge_urls(&mut final_result.urls, url.clone(), sources.iter().cloned());
        }

        for domain in &domains_to_process {
            let domain_urls: Vec<String> = collect_domain_urls(&fresh_run.urls, domain, args.subs)
                .into_iter()
                .collect();

            if !domain_urls.is_empty() {
                let cache_key = create_cache_key(domain, args);
                let entry = CacheEntry::new(domain_urls);
                cache.store_urls(&cache_key, &entry).await?;
            }
        }
    }

    // Saturating: `--cache-ttl` is an unvalidated u64, and `* 2` on a large one
    // overflows (a debug-build panic, a wrap in release).
    cache
        .cleanup_expired(args.cache_ttl.saturating_mul(2))
        .await?;

    Ok(final_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keys::KEYED_PROVIDER_IDS;
    use crate::cache;
    use crate::test_support::{build_test_args, env_mutex, EnvGuard, MockProvider};

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

        async fn cleanup_expired(&self, _ttl_seconds: u64) -> Result<()> {
            Err(anyhow::anyhow!("cache cleanup failed"))
        }

        async fn exists(&self, _key: &CacheKey) -> Result<bool> {
            Err(anyhow::anyhow!("cache exists failed"))
        }
    }

    #[tokio::test]
    async fn test_create_cache_manager_invalid_type_errors() {
        let mut args = build_test_args();
        args.cache_type = "bogus".to_string();

        match create_cache_manager(&args).await {
            Ok(_) => panic!("expected an unknown cache type to error"),
            Err(e) => assert!(e.to_string().contains("Invalid cache type"), "{e}"),
        }
    }

    #[tokio::test]
    async fn test_create_cache_manager_is_none_under_no_cache() {
        let mut args = build_test_args();
        args.no_cache = true;
        // --no-cache wins even over a cache type that would otherwise error.
        args.cache_type = "bogus".to_string();

        // `CacheManager` isn't `Debug`, so match rather than unwrap.
        match create_cache_manager(&args).await {
            Ok(manager) => assert!(manager.is_none()),
            Err(e) => panic!("--no-cache should short-circuit, got {e}"),
        }
    }

    #[tokio::test]
    async fn test_process_domains_with_cache_surfaces_backend_errors() {
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

        assert!(err.to_string().contains("cache get failed"));
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
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::set(&[("URX_VT_API_KEY", "env-vt")]);
        let _unset = EnvGuard::unset(&[
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
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
        let _env_lock = env_mutex().lock().unwrap();
        let _guard = EnvGuard::unset(&[
            "URX_VT_API_KEY",
            "URX_URLSCAN_API_KEY",
            "URX_ZOOMEYE_API_KEY",
            "URX_GITHUB_API_KEY",
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

        async fn cleanup_expired(&self, _ttl_seconds: u64) -> Result<()> {
            Ok(())
        }

        async fn exists(&self, key: &CacheKey) -> Result<bool> {
            Ok(self.entries.lock().unwrap().contains_key(&Self::id(key)))
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

    #[test]
    fn test_collect_domain_urls_matches_host_only() {
        let urls = HashMap::from([
            ("https://example.com/path".to_string(), HashSet::new()),
            (
                "https://notexample.com/redirect?next=example.com".to_string(),
                HashSet::new(),
            ),
            (
                "https://example.com.evil.test/path".to_string(),
                HashSet::new(),
            ),
            ("https://api.example.com/path".to_string(), HashSet::new()),
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
