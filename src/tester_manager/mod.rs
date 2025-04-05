use futures::future::join_all;
use tokio::task;

use crate::cli::Args;
use crate::network::NetworkSettings;
use crate::output;
use crate::progress::ProgressManager;
use crate::testers::Tester;
use crate::utils::verbose_print;

/// Helper function to apply network settings to a tester
pub fn apply_network_settings_to_tester(tester: &mut dyn Tester, settings: &NetworkSettings) {
    // Skip applying settings if network scope doesn't include testers
    if settings.scope == NetworkScope::Providers {
        return;
    }

    tester.with_timeout(settings.timeout);
    tester.with_retries(settings.retries);
    tester.with_random_agent(settings.random_agent);
    tester.with_insecure(settings.insecure);

    if let Some(proxy) = &settings.proxy {
        tester.with_proxy(Some(proxy.clone()));

        if let Some(auth) = &settings.proxy_auth {
            tester.with_proxy_auth(Some(auth.clone()));
        }
    }
}

/// Process URLs with tester components (status checker, link extractor, etc.)
pub async fn process_urls_with_testers(
    transformed_urls: Vec<String>,
    args: &Args,
    _network_settings: &NetworkSettings,
    progress_manager: &ProgressManager,
    testers: Vec<Box<dyn Tester>>,
    should_check_status: bool,
) -> Vec<output::UrlData> {
    verbose_print(args, "Applying testing options...");

    // Create progress bar for testing
    let test_bar = progress_manager.create_test_bar(transformed_urls.len());
    test_bar.set_message("Preparing URL testing...");

    // Process URLs with testers
    let mut new_urls = Vec::new();

    // Create tasks for parallel processing
    let mut tasks = Vec::new();
    let url_chunks: Vec<_> = transformed_urls.chunks(10).collect();
    let chunk_count = url_chunks.len();

    for (chunk_idx, url_chunk) in url_chunks.into_iter().enumerate() {
        let url_vec = url_chunk.to_vec();
        let testers_clone: Vec<_> = testers.iter().map(|t| t.clone_box()).collect();
        let verbose = args.verbose;
        let check_status = should_check_status;
        let extract_links = args.extract_links;
        let silent = args.silent;

        let task = task::spawn(async move {
            let mut result_urls = Vec::new();

            for url in url_vec {
                let mut status_result = None;
                let mut links_result = None;

                // Process URL with each tester
                for (i, tester) in testers_clone.iter().enumerate() {
                    match tester.test_url(&url).await {
                        Ok(results) => {
                            if i == 0 && check_status {
                                // Status checker results (first tester if check_status is enabled)
                                status_result = Some(results);
                            } else if extract_links {
                                // Link extractor results
                                links_result = Some(results);
                            }
                        }
                        Err(e) => {
                            if verbose && !silent {
                                eprintln!("Error testing URL {}: {}", url, e);
                            }
                        }
                    }
                }

                // Create UrlData for this URL
                if let Some(status_urls) = status_result {
                    for status_url in status_urls {
                        // Parse the status URL (format: "{url} - {status}")
                        result_urls.push(output::UrlData::from_string(status_url));
                    }
                } else {
                    // If no status but URL should be included anyway
                    if check_status {
                        let url_data = output::UrlData::with_status(
                            url.clone(),
                            "Status check failed".to_string(),
                        );
                        result_urls.push(url_data);
                    } else {
                        let url_data = output::UrlData::new(url.clone());
                        result_urls.push(url_data);
                    }
                }

                // If we have extracted links, add them to the result
                if let Some(link_urls) = links_result {
                    for link_url in link_urls {
                        result_urls.push(output::UrlData::new(link_url));
                    }
                }
            }

            (result_urls, chunk_idx)
        });

        tasks.push(task);

        // Update progress bar
        test_bar.set_position((chunk_idx as u64 * 10).min(transformed_urls.len() as u64));
        test_bar.set_message(format!(
            "Processing chunk {}/{}",
            chunk_idx + 1,
            chunk_count
        ));
    }

    // Collect results
    let results = join_all(tasks).await;

    for result in results {
        match result {
            Ok((urls, _)) => {
                for url in urls {
                    new_urls.push(url);
                }
            }
            Err(e) => {
                if !args.silent {
                    eprintln!("Task error: {}", e);
                }
            }
        }
    }

    // Sort URLs by their URL field
    new_urls.sort_by(|a, b| a.url.cmp(&b.url));

    test_bar.finish_with_message(format!("Testing complete, found {} URLs", new_urls.len()));

    if args.verbose && !args.silent {
        println!("Testing complete, final URL count: {}", new_urls.len());
    }

    new_urls
}

use crate::network::NetworkScope;
