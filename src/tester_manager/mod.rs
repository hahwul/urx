use futures::stream::{self, StreamExt};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::cli::Args;
use crate::filters::{HostValidator, UrlFilter};
use crate::network::client::HttpClientConfig;
use crate::network::{NetConfig, NetworkScope, NetworkSettings, RateLimiter};
use crate::output;
use crate::progress::ProgressManager;
use crate::testers::Tester;
use crate::utils::{verbose_print, UrlTransformer};

/// The filtering a URL discovered by `--extract-links`, `--extract-js-endpoints`
/// or `--archive-body` has to pass.
///
/// The primary URL list goes through the filters, host validation, and the
/// `show_only_*`/`--normalize-url` views *before* testing starts. Links found
/// inside those pages only exist afterwards, so without applying the same rules
/// here `--extract-links` silently bypasses every filter the user set: `-e js`
/// would emit non-JS links, and strict mode (on by default) would emit every
/// off-site link a page happens to point at — ads, CDNs, social buttons.
pub struct ExtractedLinkFilter {
    filter: UrlFilter,
    transformer: UrlTransformer,
    host_validator: Option<HostValidator>,
}

impl ExtractedLinkFilter {
    pub fn new(
        filter: UrlFilter,
        transformer: UrlTransformer,
        host_validator: Option<HostValidator>,
    ) -> Self {
        ExtractedLinkFilter {
            filter,
            transformer,
            host_validator,
        }
    }

    /// The surviving, transformed form of `url`, or `None` if it is filtered out.
    pub fn accept(&self, url: &str) -> Option<String> {
        if !self.filter.matches(url) {
            return None;
        }
        if let Some(v) = &self.host_validator {
            if !v.is_valid_host(url) {
                return None;
            }
        }
        self.transformer.transform_one(url)
    }
}

/// Helper function to apply network settings to a tester
pub fn apply_network_settings_to_tester(tester: &mut dyn Tester, settings: &NetworkSettings) {
    // Skip applying settings if network scope doesn't include testers
    if settings.scope == NetworkScope::Providers {
        return;
    }

    tester.with_network(NetConfig {
        http: HttpClientConfig {
            timeout: settings.timeout,
            insecure: settings.insecure,
            random_agent: settings.random_agent,
            proxy: settings.proxy.clone(),
            proxy_auth: settings.proxy_auth.clone(),
            // Testers request the target, so the user's headers go along;
            // the archive replayer strips them again.
            headers: settings.headers.clone(),
        },
        retries: settings.retries,
        rate_limit: settings.rate_limit.and_then(RateLimiter::new),
    });
}

/// Process URLs with tester components (status checker, link extractor, etc.)
pub async fn process_urls_with_testers(
    transformed_urls: Vec<String>,
    args: &Args,
    progress_manager: &ProgressManager,
    testers: Vec<Box<dyn Tester>>,
    should_check_status: bool,
    link_filter: Option<Arc<ExtractedLinkFilter>>,
) -> Vec<output::UrlData> {
    verbose_print(args, "Applying testing options...");

    // Create progress bar for testing
    let test_bar = progress_manager.create_test_bar(transformed_urls.len());
    test_bar.set_message("Preparing URL testing...");

    // Process URLs with testers.
    //
    // Concurrency is bounded by --parallel. The previous implementation spawned
    // one task per 10-URL chunk and launched them all at once, so a run over
    // tens of thousands of URLs could open thousands of simultaneous
    // connections — exhausting file descriptors and hammering the target. We
    // instead stream URL chunks through `buffer_unordered`, keeping at most
    // `parallel` chunks in flight at a time, and advance the progress bar as
    // each URL actually completes (not when its task is merely scheduled).
    let parallel = args.parallel.max(1) as usize;
    let total = transformed_urls.len() as u64;
    let completed = Arc::new(AtomicU64::new(0));

    let verbose = args.verbose;
    let check_status = should_check_status;
    // All of these discover URLs inside fetched bodies; any subset may be in
    // the tester list after the status checker.
    // --- spec-expansion --- (`|| args.expand_specs`)
    let extract_links =
        args.extract_links || args.extract_js_endpoints || args.archive_body || args.expand_specs;
    let silent = args.silent;
    // With an --include-status allowlist, a URL whose status we could never
    // resolve has not been shown to match it. Emitting it with a placeholder
    // status would smuggle it past the very filter the user asked for — so an
    // allowlist drops unresolvable URLs. An --exclude-status denylist is the
    // other way round: a failed check matched nothing on the list, so the URL
    // is kept (flagged) rather than silently discarded.
    let drop_unresolved = !args.include_status.is_empty();

    let url_chunks: Vec<Vec<String>> = transformed_urls
        .chunks(10)
        .map(|chunk| chunk.to_vec())
        .collect();

    // Per-URL diagnostics belong on stderr, above the live region — stdout is
    // the URL list a caller pipes onward.
    let notifier = progress_manager.notifier();

    let chunk_results: Vec<Vec<output::UrlData>> =
        stream::iter(url_chunks.into_iter().map(|url_vec| {
            let testers_clone: Vec<_> = testers.iter().map(|t| t.clone_box()).collect();
            let test_bar = test_bar.clone();
            let completed = Arc::clone(&completed);
            let link_filter = link_filter.clone();
            let notifier = notifier.clone();

            async move {
                let mut result_urls = Vec::new();

                for url in url_vec {
                    let mut status_result = None;
                    let mut links_result: Option<Vec<output::UrlData>> = None;

                    // Process URL with each tester
                    for (i, tester) in testers_clone.iter().enumerate() {
                        match tester.test_url(&url).await {
                            Ok(results) => {
                                if i == 0 && check_status {
                                    // Status checker results (first tester if check_status is enabled)
                                    status_result = Some(results);
                                } else if extract_links {
                                    // Link / JS-endpoint / archive-body
                                    // extractor results. Any subset of these
                                    // may run in the same pass, so accumulate
                                    // rather than replace.
                                    links_result.get_or_insert_with(Vec::new).extend(results);
                                }
                            }
                            Err(e) => {
                                if verbose && !silent {
                                    notifier.note(format!("Error testing URL {url}: {e}"));
                                }
                            }
                        }
                    }

                    // Create UrlData for this URL
                    if let Some(status_urls) = status_result {
                        result_urls.extend(status_urls);
                    } else {
                        // If no status but URL should be included anyway
                        if check_status {
                            if !drop_unresolved {
                                let url_data = output::UrlData::with_status(
                                    url.clone(),
                                    "Status check failed".to_string(),
                                );
                                result_urls.push(url_data);
                            }
                        } else {
                            let url_data = output::UrlData::new(url.clone());
                            result_urls.push(url_data);
                        }
                    }

                    // If we have extracted links, add them to the result — after
                    // putting them through the same filters, host validation and
                    // views the primary URLs already passed.
                    if let Some(link_urls) = links_result {
                        for link in link_urls {
                            match &link_filter {
                                Some(f) => {
                                    if let Some(kept) = f.accept(&link.url) {
                                        result_urls.push(output::UrlData::new(kept));
                                    }
                                }
                                None => result_urls.push(link),
                            }
                        }
                    }

                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    test_bar.set_position(done.min(total));
                }

                result_urls
            }
        }))
        .buffer_unordered(parallel)
        .collect()
        .await;

    let mut new_urls = Vec::new();
    for urls in chunk_results {
        new_urls.extend(urls);
    }

    // Sort URLs by their URL field, then collapse repeats.
    //
    // Every other path into the output is deduplicated — the batch path builds
    // from a HashSet, the streaming sink keeps a `seen` set — but this one was
    // not, and it is the one that manufactures duplicates. `--extract-links`
    // emits every link found on every page, so a nav entry linked from a hundred
    // pages was printed a hundred times; a discovered link that is also a
    // primary URL was printed twice, once with a status and once without.
    new_urls.sort_by(|a, b| a.url.cmp(&b.url));
    new_urls.dedup_by(|dropped, kept| {
        if dropped.url != kept.url {
            return false;
        }
        // Keep whichever copy carries the status — the whole record, so the
        // title, location and content type the check found come with it; a
        // checked URL rediscovered by the extractor must not lose them to the
        // bare copy.
        if kept.status.is_none() && dropped.status.is_some() {
            std::mem::swap(kept, dropped);
        }
        true
    });

    test_bar.finish_with_message(format!("Testing complete, found {} URLs", new_urls.len()));

    if args.verbose && !args.silent {
        progress_manager.note(format!(
            "Testing complete, final URL count: {}",
            new_urls.len()
        ));
    }

    new_urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::UrlData;
    use crate::testers::urls;
    use anyhow::Result;
    use std::future::Future;
    use std::pin::Pin;

    /// Records the network settings it was handed.
    #[derive(Clone, Default)]
    struct MockTester {
        net: Option<NetConfig>,
    }

    impl Tester for MockTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            _url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn with_network(&mut self, net: NetConfig) {
            self.net = Some(net);
        }
    }

    /// A tester whose every request fails, standing in for an unreachable host.
    #[derive(Clone, Default)]
    struct FailingTester;

    impl Tester for FailingTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>> {
            let url = url.to_string();
            Box::pin(async move { Err(anyhow::anyhow!("connection refused for {url}")) })
        }
    }

    async fn run_failing_status_check(argv: &[&str]) -> Vec<output::UrlData> {
        use clap::Parser;
        let args = Args::parse_from(argv);
        let progress = ProgressManager::new(true);
        process_urls_with_testers(
            vec!["https://example.com/a".to_string()],
            &args,
            &progress,
            vec![Box::new(FailingTester)],
            true,
            None,
        )
        .await
    }

    #[tokio::test]
    async fn test_include_status_drops_urls_whose_status_never_resolved() {
        // Regression: an unreachable URL was emitted with a placeholder
        // "Status check failed" status even under --include-status 200, so the
        // allowlist leaked URLs that were never shown to return 200.
        let out =
            run_failing_status_check(&["urx", "--is", "200", "--silent", "example.com"]).await;
        assert!(out.is_empty(), "{out:?}");
    }

    #[tokio::test]
    async fn test_plain_check_status_still_reports_the_failure() {
        // Without an allowlist there is nothing to leak past, and the failure is
        // itself information worth surfacing.
        let out =
            run_failing_status_check(&["urx", "--check-status", "--silent", "example.com"]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status.as_deref(), Some("Status check failed"));
    }

    #[tokio::test]
    async fn test_exclude_status_keeps_urls_whose_status_never_resolved() {
        // A denylist is the inverse: a failed check matched nothing on the list.
        let out =
            run_failing_status_check(&["urx", "--es", "404", "--silent", "example.com"]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status.as_deref(), Some("Status check failed"));
    }

    /// A tester that returns a fixed set of "extracted links" for any URL.
    #[derive(Clone)]
    struct FixedLinkTester(Vec<String>);

    impl Tester for FixedLinkTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }

        fn test_url<'a>(
            &'a self,
            _url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>> {
            let links = self.0.iter().cloned().map(UrlData::new).collect();
            Box::pin(async move { Ok(links) })
        }
    }

    async fn run_extract_links(
        argv: &[&str],
        discovered: &[&str],
        link_filter: Option<Arc<ExtractedLinkFilter>>,
    ) -> Vec<String> {
        use clap::Parser;
        let args = Args::parse_from(argv);
        let progress = ProgressManager::new(true);
        let tester = FixedLinkTester(discovered.iter().map(|s| s.to_string()).collect());
        process_urls_with_testers(
            vec!["https://example.com/seed".to_string()],
            &args,
            &progress,
            vec![Box::new(tester)],
            false,
            link_filter,
        )
        .await
        .into_iter()
        .map(|d| d.url)
        .collect()
    }

    #[tokio::test]
    async fn test_extracted_links_obey_the_extension_filter() {
        // Regression: extracted links were appended raw, after the filters had
        // already run over the primary list — so `-e js` still emitted non-JS
        // links that the extractor happened to find.
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string()]);
        let link_filter = Some(Arc::new(ExtractedLinkFilter::new(
            filter,
            UrlTransformer::default(),
            None,
        )));

        let out = run_extract_links(
            &[
                "urx",
                "--extract-links",
                "-e",
                "js",
                "--silent",
                "example.com",
            ],
            &[
                "https://example.com/app.js",
                "https://example.com/index.html",
            ],
            link_filter,
        )
        .await;

        assert!(
            out.contains(&"https://example.com/app.js".to_string()),
            "{out:?}"
        );
        assert!(
            !out.contains(&"https://example.com/index.html".to_string()),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn test_extracted_links_obey_host_validation() {
        // Strict host validation is on by default, but extracted links skipped
        // it entirely — so every off-site link a page pointed at (ads, CDNs,
        // social buttons) landed in the results.
        let link_filter = Some(Arc::new(ExtractedLinkFilter::new(
            UrlFilter::new(),
            UrlTransformer::default(),
            Some(HostValidator::new(&["example.com".to_string()], false)),
        )));

        let out = run_extract_links(
            &["urx", "--extract-links", "--silent", "example.com"],
            &[
                "https://example.com/kept",
                "https://ads.tracker.net/beacon",
                "https://cdn.other.org/lib.js",
            ],
            link_filter,
        )
        .await;

        assert!(
            out.contains(&"https://example.com/kept".to_string()),
            "{out:?}"
        );
        assert!(
            !out.iter()
                .any(|u| u.contains("tracker.net") || u.contains("other.org")),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn test_extracted_links_are_transformed_like_the_rest() {
        // The show-only / normalize views applied to the primary list must apply
        // to extracted links too, or the output mixes two different shapes.
        let transformer = UrlTransformer {
            show_only_path: true,
            ..Default::default()
        };
        let link_filter = Some(Arc::new(ExtractedLinkFilter::new(
            UrlFilter::new(),
            transformer,
            None,
        )));

        let out = run_extract_links(
            &[
                "urx",
                "--extract-links",
                "--show-only-path",
                "--silent",
                "example.com",
            ],
            &["https://example.com/a/b?q=1"],
            link_filter,
        )
        .await;

        assert!(out.contains(&"/a/b".to_string()), "{out:?}");
    }

    #[tokio::test]
    async fn test_link_and_js_extractor_results_are_both_kept() {
        // Both extractors can run in the same pass. Results used to be
        // *replaced* per tester, so whichever ran last silently discarded the
        // other's discoveries.
        use clap::Parser;
        let args = Args::parse_from([
            "urx",
            "--extract-links",
            "--extract-js-endpoints",
            "--silent",
            "example.com",
        ]);
        let progress = ProgressManager::new(true);
        let links = FixedLinkTester(vec!["https://example.com/from-html".to_string()]);
        let endpoints = FixedLinkTester(vec!["https://example.com/api/from-js".to_string()]);
        let out: Vec<String> = process_urls_with_testers(
            vec!["https://example.com/seed".to_string()],
            &args,
            &progress,
            vec![Box::new(links), Box::new(endpoints)],
            false,
            None,
        )
        .await
        .into_iter()
        .map(|d| d.url)
        .collect();

        assert!(
            out.contains(&"https://example.com/from-html".to_string()),
            "{out:?}"
        );
        assert!(
            out.contains(&"https://example.com/api/from-js".to_string()),
            "{out:?}"
        );
    }

    #[tokio::test]
    async fn test_no_link_filter_keeps_links_verbatim() {
        // Without --extract-links no filter is built, so nothing changes for
        // callers that pass None.
        let out = run_extract_links(
            &["urx", "--extract-links", "--silent", "example.com"],
            &["https://elsewhere.test/x"],
            None,
        )
        .await;
        assert!(
            out.contains(&"https://elsewhere.test/x".to_string()),
            "{out:?}"
        );
    }

    #[test]
    fn test_apply_network_settings_to_tester_honours_scope() {
        for (scope, applied) in [
            (NetworkScope::All, true),
            (NetworkScope::Testers, true),
            (NetworkScope::Providers, false),
        ] {
            let mut tester = MockTester::default();
            let settings = NetworkSettings {
                timeout: 60,
                retries: 5,
                insecure: true,
                proxy: Some("http://proxy:8080".to_string()),
                proxy_auth: Some("user:pass".to_string()),
                rate_limit: Some(2.0),
                scope,
                ..Default::default()
            };

            apply_network_settings_to_tester(&mut tester, &settings);

            let Some(net) = tester.net else {
                assert!(!applied, "{scope:?}");
                continue;
            };
            assert!(applied, "{scope:?}");
            assert_eq!(net.http.timeout, 60);
            assert_eq!(net.retries, 5);
            assert!(net.http.insecure);
            assert_eq!(net.http.proxy.as_deref(), Some("http://proxy:8080"));
            assert_eq!(net.http.proxy_auth.as_deref(), Some("user:pass"));
            assert!(net.rate_limit.is_some());
        }
    }

    #[tokio::test]
    async fn test_extracted_links_are_deduplicated() {
        // Regression: every link found on every page was appended verbatim, so a
        // nav entry present on many pages was printed once per page.
        use clap::Parser;
        let args = Args::parse_from(["urx", "--extract-links", "--silent", "example.com"]);
        let progress = ProgressManager::new(true);
        let tester = FixedLinkTester(vec![
            "https://example.com/contact".to_string(),
            "https://example.com/about".to_string(),
        ]);

        // Three seed pages, each yielding the same two links.
        let out = process_urls_with_testers(
            vec![
                "https://example.com/p1".to_string(),
                "https://example.com/p2".to_string(),
                "https://example.com/p3".to_string(),
            ],
            &args,
            &progress,
            vec![Box::new(tester)],
            false,
            None,
        )
        .await;

        let urls: Vec<String> = out.into_iter().map(|d| d.url).collect();
        assert_eq!(
            urls,
            vec![
                "https://example.com/about".to_string(),
                "https://example.com/contact".to_string(),
                "https://example.com/p1".to_string(),
                "https://example.com/p2".to_string(),
                "https://example.com/p3".to_string(),
            ],
            "{urls:?}"
        );
    }

    /// A status checker that always reports 200 for whatever it is given.
    #[derive(Clone)]
    struct OkStatusTester;

    impl Tester for OkStatusTester {
        fn clone_box(&self) -> Box<dyn Tester> {
            Box::new(self.clone())
        }
        fn test_url<'a>(
            &'a self,
            url: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Vec<UrlData>>> + Send + 'a>> {
            let url = url.to_string();
            Box::pin(async move {
                Ok(vec![UrlData {
                    title: Some("Title".to_string()),
                    ..UrlData::with_status(url, "200 OK".to_string())
                }])
            })
        }
    }

    #[tokio::test]
    async fn test_dedup_keeps_the_entry_carrying_the_status() {
        // A URL that was status-checked and then rediscovered by the extractor
        // appeared twice — once with its status, once bare. Collapsing the two
        // must not throw the status away.
        use clap::Parser;
        let args = Args::parse_from([
            "urx",
            "--check-status",
            "--extract-links",
            "--silent",
            "example.com",
        ]);
        let progress = ProgressManager::new(true);

        let out = process_urls_with_testers(
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
            ],
            &args,
            &progress,
            vec![
                Box::new(OkStatusTester),
                // Every page links to /b, so a bare /b sorts beside the
                // checked one — on either side of it.
                Box::new(FixedLinkTester(vec!["https://example.com/b".to_string()])),
            ],
            true,
            None,
        )
        .await;

        assert_eq!(out.len(), 2, "{out:?}");
        for entry in &out {
            assert_eq!(entry.status.as_deref(), Some("200 OK"), "{entry:?}");
            // The whole checked record survives, not just its status.
            assert_eq!(entry.title.as_deref(), Some("Title"), "{entry:?}");
        }
    }

    #[tokio::test]
    async fn test_input_urls_pass_through_when_statuses_are_not_consumed() {
        // With should_check_status false, a tester still runs (it may be the
        // link extractor) but its output must not replace the input list.
        use crate::test_support::{build_test_args, MockStatusChecker};

        let testers: Vec<Box<dyn Tester>> = vec![Box::new(MockStatusChecker::new(vec![
            "https://example.com/result1".to_string(),
            "https://example.com/result2".to_string(),
        ]))];

        let input_urls = vec![
            "https://example.com/page1".to_string(),
            "https://example.com/page2".to_string(),
        ];

        let result = process_urls_with_testers(
            input_urls,
            &build_test_args(),
            &ProgressManager::new(true),
            testers,
            false,
            None,
        )
        .await;

        let urls: Vec<String> = result.iter().map(|d| d.url.clone()).collect();
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"https://example.com/page2".to_string()));
    }

    #[tokio::test]
    async fn custom_headers_reach_the_target_but_never_the_archive() {
        use crate::network::CustomHeaders;
        use crate::testers::{ArchiveBodyExtractor, ArchiveCapture, LinkExtractor};

        let settings = NetworkSettings {
            headers: CustomHeaders::parse(
                &["X-Trace: urx".to_string()],
                Some("session=secret"),
                Some("urx-test/1"),
            )
            .unwrap(),
            ..Default::default()
        };

        // The link extractor requests URLs from the target, so it must send
        // them...
        let mut server = mockito::Server::new_async().await;
        let target = server
            .mock("GET", "/page")
            .match_header("x-trace", "urx")
            .match_header("cookie", "session=secret")
            .match_header("user-agent", "urx-test/1")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/found">x</a>"#)
            .expect(1)
            .create_async()
            .await;

        let mut extractor = LinkExtractor::new();
        apply_network_settings_to_tester(&mut extractor, &settings);
        let links = extractor
            .test_url(&format!("{}/page", server.url()))
            .await
            .map(urls)
            .unwrap();
        assert_eq!(links, vec![format!("{}/found", server.url())]);
        // Mockito answers 501 when no mock matches, so a missed header would
        // have produced no links at all. `assert` pins the reason.
        target.assert();

        // ...and the archive replayer must not: its requests go to the Wayback
        // Machine, and `session=secret` is the *target's* credential.
        let mut archive = mockito::Server::new_async().await;
        let replay = archive
            .mock("GET", mockito::Matcher::Any)
            .match_header("cookie", mockito::Matcher::Missing)
            .match_header("x-trace", mockito::Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body(r#"<a href="/archived">x</a>"#)
            .expect(1)
            .create_async()
            .await;

        let mut replayer = ArchiveBodyExtractor::new(
            [(
                "https://example.com/gone".to_string(),
                ArchiveCapture {
                    timestamp: "20200101000000".to_string(),
                    digest: Some("D1".to_string()),
                },
            )]
            .into_iter()
            .collect(),
            10,
        );
        apply_network_settings_to_tester(&mut replayer, &settings);
        replayer.with_origin(archive.url());
        let found = replayer
            .test_url("https://example.com/gone")
            .await
            .map(urls)
            .unwrap();
        assert_eq!(found, vec!["https://example.com/archived".to_string()]);
        replay.assert();
    }
}
