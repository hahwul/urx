//! `--notify`: POST a run summary to one or more webhooks when the run ends.
//!
//! This is the exit that `--incremental` was missing. The cache already
//! answers "which URLs are new since last time"; without this module the only
//! way to get that answer to a person was a pipe into some other tool. A
//! webhook is the lowest common denominator — Slack, Discord, Mattermost,
//! ntfy, n8n, a home-grown receiver — and needs no credential beyond the URL.
//!
//! Three rules shape everything here:
//!
//! - **The webhook URL is a secret.** A Slack or Discord webhook URL *is* the
//!   credential, so no log line, warning, or error may ever print more than
//!   its host. See [`webhook_host`].
//! - **Delivery never fails the run.** By the time a notification goes out the
//!   URLs are collected and already written; a dead webhook must not turn a
//!   successful scan into a non-zero exit. Failures are warnings on stderr.
//! - **Quiet when nothing changed.** The default `--notify-on new` sends only
//!   when the run produced at least one URL, so a cron job that finds nothing
//!   says nothing.

use clap::ValueEnum;
use serde::Serialize;

use crate::cli::Args;
use crate::network::client::HttpClientConfig;
use crate::network::NetworkSettings;
use crate::runner::ProviderStats;

/// When a notification is sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum NotifyOn {
    /// Send after every run, even one that produced no URLs.
    Always,
    /// Send only when the run produced at least one (new) URL.
    #[default]
    New,
    /// Keep the configuration but never send.
    Never,
}

/// The wire shape of the payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum NotifyFormat {
    /// `{"text": "..."}` — Slack incoming webhooks and compatible receivers.
    Slack,
    /// `{"content": "..."}` — Discord webhooks.
    Discord,
    /// urx's own structured summary.
    #[default]
    Json,
}

/// Most URLs the JSON payload and the chat messages carry.
///
/// A monitoring alert should name a few examples, not replay the whole result
/// set — that already went to stdout / `--output`. The cap also bounds the
/// payload size before the per-service character limits are applied.
pub const SAMPLE_LIMIT: usize = 20;

/// Longest `content` Discord accepts on a webhook message.
pub const DISCORD_MAX_CHARS: usize = 2000;

/// Longest `text` sent to Slack. Slack's hard ceiling is higher, but past
/// this length the message collapses behind a "show more" and clients start
/// truncating it themselves; the point of an alert is to be readable.
pub const SLACK_MAX_CHARS: usize = 4000;

/// One provider's row, mirroring what `--stats` prints.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderSummary {
    pub name: String,
    pub urls: usize,
    pub errors: usize,
    pub partial: usize,
    pub elapsed_ms: u64,
    pub aborted: bool,
}

impl From<&ProviderStats> for ProviderSummary {
    fn from(s: &ProviderStats) -> Self {
        Self {
            name: s.name.clone(),
            urls: s.url_count,
            errors: s.error_count,
            partial: s.partial_count,
            elapsed_ms: u64::try_from(s.elapsed.as_millis()).unwrap_or(u64::MAX),
            aborted: s.aborted,
        }
    }
}

/// Everything a notification says about the run. This is the `json` payload
/// verbatim, and the source the chat formats are rendered from.
#[derive(Clone, Debug, Serialize)]
pub struct RunSummary {
    /// Always `"urx"`, so a receiver fed by several tools can route on it.
    pub tool: &'static str,
    pub version: &'static str,
    /// The targets that were scanned, as resolved from the CLI, `--domain-list`
    /// and stdin. Empty for a `--files` run.
    pub domains: Vec<String>,
    /// Whether the run diffed against the cache (`--incremental`).
    pub incremental: bool,
    /// URLs the run emitted.
    pub url_count: usize,
    /// URLs the run considers new. Under `--incremental` the emitted set is
    /// exactly the diff against the previous run, so this equals `url_count`.
    /// Without a baseline every emitted URL is new, so it equals `url_count`
    /// there too; the field exists so a receiver can read it without knowing
    /// which mode produced the payload.
    pub new_url_count: usize,
    pub elapsed_ms: u64,
    pub providers: Vec<ProviderSummary>,
    /// Up to [`SAMPLE_LIMIT`] of the emitted URLs, in output order.
    pub sample: Vec<String>,
    /// True when `url_count` exceeds the sample.
    pub sample_truncated: bool,
}

impl RunSummary {
    /// Build the summary from the run's outputs. `urls` is the final, ordered
    /// result set; only a bounded sample of it is retained.
    pub fn new(
        args: &Args,
        domains: Vec<String>,
        urls: &[String],
        stats: &[ProviderStats],
        elapsed: std::time::Duration,
    ) -> Self {
        Self::from_counts(args, domains, urls.len(), urls, stats, elapsed)
    }

    /// Like [`RunSummary::new`] but with the count supplied separately, for
    /// the `--stream` path where the URLs were written as they arrived and
    /// only the count survives.
    pub fn from_counts(
        args: &Args,
        domains: Vec<String>,
        url_count: usize,
        sample_source: &[String],
        stats: &[ProviderStats],
        elapsed: std::time::Duration,
    ) -> Self {
        let sample: Vec<String> = sample_source.iter().take(SAMPLE_LIMIT).cloned().collect();
        Self {
            tool: "urx",
            version: env!("CARGO_PKG_VERSION"),
            domains,
            incremental: args.incremental,
            url_count,
            new_url_count: url_count,
            elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            providers: stats.iter().map(ProviderSummary::from).collect(),
            sample_truncated: url_count > sample.len(),
            sample,
        }
    }
}

/// Whether `--notify-on` lets this run send anything.
pub fn should_send(on: NotifyOn, summary: &RunSummary) -> bool {
    match on {
        NotifyOn::Always => true,
        NotifyOn::New => summary.new_url_count > 0,
        NotifyOn::Never => false,
    }
}

/// The request body for `format`.
pub fn build_payload(format: NotifyFormat, summary: &RunSummary) -> String {
    match format {
        NotifyFormat::Json => {
            serde_json::to_string(summary).expect("RunSummary serializes infallibly")
        }
        NotifyFormat::Slack => {
            serde_json::json!({ "text": render_text(summary, SLACK_MAX_CHARS) }).to_string()
        }
        NotifyFormat::Discord => {
            serde_json::json!({ "content": render_text(summary, DISCORD_MAX_CHARS) }).to_string()
        }
    }
}

/// The human-readable message for the chat formats, at most `max_chars`
/// characters. When the sample doesn't fit, the message ends with a line
/// saying how many URLs were cut, so a short alert is never mistaken for a
/// short result.
pub fn render_text(summary: &RunSummary, max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();

    let targets = if summary.domains.is_empty() {
        "file input".to_string()
    } else {
        summary.domains.join(", ")
    };
    let noun = if summary.new_url_count == 1 {
        "URL"
    } else {
        "URLs"
    };
    let kind = if summary.incremental { "new " } else { "" };
    lines.push(format!(
        "urx: {} {kind}{noun} for {targets} ({})",
        summary.new_url_count,
        format_elapsed_ms(summary.elapsed_ms)
    ));

    if !summary.providers.is_empty() {
        let rows: Vec<String> = summary
            .providers
            .iter()
            .map(|p| {
                let mut row = format!("{} {} urls", p.name, p.urls);
                if p.errors > 0 {
                    row.push_str(&format!(" / {} errors", p.errors));
                }
                if p.aborted {
                    row.push_str(" (aborted)");
                }
                row
            })
            .collect();
        lines.push(format!("providers: {}", rows.join(", ")));
    }

    lines.extend(summary.sample.iter().cloned());

    // URLs beyond the sample were never in the message; say so before the
    // character budget is even considered.
    let unsampled = summary.url_count.saturating_sub(summary.sample.len());
    if unsampled > 0 {
        lines.push(format!("… {unsampled} more not shown"));
    }

    fit_lines(&lines, max_chars)
}

/// Join `lines` with newlines, dropping whole lines from the end until the
/// result fits `max_chars`, and naming the cut when one was made.
///
/// Whole lines rather than a character cut: a URL sliced mid-way looks like a
/// real, shorter URL. Counted in characters, not bytes — Discord's limit is a
/// character count.
fn fit_lines(lines: &[String], max_chars: usize) -> String {
    let full = lines.join("\n");
    if full.chars().count() <= max_chars {
        return full;
    }

    // Keep the header, then as many further lines as leave room for the
    // marker that names how many were dropped.
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0usize;
    for (i, line) in lines.iter().enumerate() {
        let dropped = lines.len() - i;
        let marker = truncation_marker(dropped);
        let len = line.chars().count();
        // "+1" for the newline that would precede this line, and again for
        // the one before the marker.
        let with_line = used + len + usize::from(i > 0);
        let marker_cost = marker.chars().count() + 1;
        if with_line + marker_cost > max_chars {
            break;
        }
        kept.push(line);
        used = with_line;
    }

    let dropped = lines.len() - kept.len();
    let marker = truncation_marker(dropped);
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&marker);

    // A header alone could exceed the budget (a very long domain list);
    // the marker is the one line that must survive, so cut from the front.
    if out.chars().count() > max_chars {
        let keep = max_chars.saturating_sub(marker.chars().count() + 1);
        let head: String = out.chars().take(keep).collect();
        out = format!("{head}\n{marker}");
    }
    out
}

fn truncation_marker(dropped: usize) -> String {
    let noun = if dropped == 1 { "line" } else { "lines" };
    format!("[truncated: {dropped} {noun} cut to fit the message limit]")
}

fn format_elapsed_ms(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

/// The only part of a webhook URL that may appear in output.
///
/// The path of a Slack/Discord webhook is the credential, and the query of
/// others often is. What's left — scheme, host, port — is enough to tell two
/// destinations apart in a warning. An unparseable string is not echoed
/// either; it might be a malformed but real secret.
pub fn webhook_host(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => match parsed.port() {
                Some(port) => format!("{}://{host}:{port}", parsed.scheme()),
                None => format!("{}://{host}", parsed.scheme()),
            },
            None => "<webhook with no host>".to_string(),
        },
        Err(_) => "<unparseable webhook URL>".to_string(),
    }
}

/// How one delivery ended. The `Display` form is what reaches stderr, and
/// is built only from the redacted host and a status or error text.
#[derive(Debug)]
pub enum Delivery {
    /// The webhook answered 2xx.
    Sent { host: String, status: u16 },
    /// The webhook answered, but not with 2xx.
    Rejected { host: String, status: u16 },
    /// No answer at all: connect failure, timeout, TLS error, bad URL.
    Failed { host: String, error: String },
}

impl Delivery {
    pub fn is_ok(&self) -> bool {
        matches!(self, Delivery::Sent { .. })
    }
}

impl std::fmt::Display for Delivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Delivery::Sent { host, status } => write!(f, "notification sent to {host} ({status})"),
            Delivery::Rejected { host, status } => {
                write!(f, "notification to {host} rejected with HTTP {status}")
            }
            Delivery::Failed { host, error } => {
                write!(f, "notification to {host} failed: {error}")
            }
        }
    }
}

/// POST `body` to `url` once. Nothing here can fail the caller: every outcome
/// is a [`Delivery`].
///
/// A single attempt, deliberately. Chat webhooks are not idempotent — a retry
/// after a slow-but-delivered request posts the alert twice.
pub async fn deliver(client: &reqwest::Client, url: &str, body: &str) -> Delivery {
    let host = webhook_host(url);

    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .await;

    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            if resp.status().is_success() {
                Delivery::Sent { host, status }
            } else {
                Delivery::Rejected { host, status }
            }
        }
        // `without_url`: reqwest's error text otherwise names the full request
        // URL, which is exactly the secret this module exists to keep out of
        // the log.
        Err(e) => Delivery::Failed {
            host,
            error: e.without_url().to_string(),
        },
    }
}

/// The client the webhook request goes through.
///
/// Notifications take the proxy, timeout and `--insecure` settings without
/// consulting `--network-scope`. That flag partitions the traffic urx sends
/// *at the target and the archives* — providers query the archives, testers
/// probe the target — and exists so an operator can, say, route only the
/// target-facing probes through a proxy. A webhook is neither: it is the
/// operator's own endpoint, reached from wherever the cron job runs. Making
/// it a third scope value would force a choice between "the proxy is my only
/// egress" and "don't proxy target traffic" that the flag was never meant to
/// express, so the notification simply uses whatever egress settings were
/// given.
fn build_client(settings: &NetworkSettings) -> anyhow::Result<reqwest::Client> {
    HttpClientConfig {
        timeout: settings.timeout,
        insecure: settings.insecure,
        random_agent: false,
        proxy: settings.proxy.clone(),
        proxy_auth: settings.proxy_auth.clone(),
    }
    .build_client()
}

/// Send the run summary to every `--notify` URL, obeying `--notify-on`.
///
/// Returns the outcome of each delivery in order. Never returns an error and
/// never panics on a bad webhook: URL collection is done and the result is
/// already written, so the exit code belongs to the scan, not to the alert.
pub async fn send_notifications(
    args: &Args,
    settings: &NetworkSettings,
    summary: &RunSummary,
) -> Vec<Delivery> {
    if args.notify.is_empty() {
        return Vec::new();
    }

    if !should_send(args.notify_on, summary) {
        log_verbose(
            args,
            format!(
                "Notification skipped (--notify-on {}, {} new URLs)",
                args.notify_on
                    .to_possible_value()
                    .map(|v| v.get_name().to_string())
                    .unwrap_or_default(),
                summary.new_url_count
            ),
        );
        return Vec::new();
    }

    let client = match build_client(settings) {
        Ok(client) => client,
        Err(e) => {
            // The proxy URL is the usual culprit; it is not a secret and names
            // the fix, so it stays in the message.
            log_warning(
                args,
                format!("Warning: cannot build notification client: {e}"),
            );
            return Vec::new();
        }
    };

    let body = build_payload(args.notify_format, summary);
    let mut outcomes = Vec::with_capacity(args.notify.len());

    for url in &args.notify {
        let outcome = deliver(&client, url, &body).await;
        if outcome.is_ok() {
            log_verbose(args, outcome.to_string());
        } else {
            log_warning(args, format!("Warning: {outcome}"));
        }
        outcomes.push(outcome);
    }

    outcomes
}

/// stderr, not stdout: the URL list has already been written and a warning
/// must not land inside it when stdout is a pipe. Suppressed by `--silent`
/// like every other diagnostic; the delivery still happens.
fn log_warning(args: &Args, message: String) {
    if !args.silent {
        eprintln!("{message}");
    }
}

fn log_verbose(args: &Args, message: String) {
    if args.verbose && !args.silent {
        eprintln!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::build_test_args;
    use std::time::Duration;

    const SECRET: &str = "T000/B000/xoxb-super-secret-token";

    fn stats() -> Vec<ProviderStats> {
        vec![
            ProviderStats {
                name: "Wayback Machine".to_string(),
                url_count: 120,
                error_count: 0,
                partial_count: 0,
                elapsed: Duration::from_millis(2500),
                aborted: false,
            },
            ProviderStats {
                name: "OTX".to_string(),
                url_count: 3,
                error_count: 1,
                partial_count: 0,
                elapsed: Duration::from_millis(80),
                aborted: true,
            },
        ]
    }

    fn urls(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("https://example.com/path/{i}"))
            .collect()
    }

    fn summary(n: usize, incremental: bool) -> RunSummary {
        let mut args = build_test_args();
        args.incremental = incremental;
        RunSummary::new(
            &args,
            vec!["example.com".to_string()],
            &urls(n),
            &stats(),
            Duration::from_millis(3200),
        )
    }

    async fn webhook_server() -> (mockito::ServerGuard, String) {
        let server = mockito::Server::new_async().await;
        let url = format!("{}/services/{SECRET}", server.url());
        (server, url)
    }

    fn notify_args(url: &str, on: NotifyOn, format: NotifyFormat) -> Args {
        let mut args = build_test_args();
        args.silent = false;
        args.notify = vec![url.to_string()];
        args.notify_on = on;
        args.notify_format = format;
        args
    }

    // ---- payloads ---------------------------------------------------------

    #[test]
    fn json_payload_carries_the_run_summary() {
        let s = summary(25, true);
        let body: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Json, &s)).unwrap();

        assert_eq!(body["tool"], "urx");
        assert_eq!(body["domains"], serde_json::json!(["example.com"]));
        assert_eq!(body["incremental"], true);
        assert_eq!(body["url_count"], 25);
        assert_eq!(body["new_url_count"], 25);
        assert_eq!(body["elapsed_ms"], 3200);
        assert_eq!(body["providers"][0]["name"], "Wayback Machine");
        assert_eq!(body["providers"][0]["urls"], 120);
        assert_eq!(body["providers"][1]["errors"], 1);
        assert_eq!(body["providers"][1]["aborted"], true);
        // The sample is capped and says so.
        assert_eq!(body["sample"].as_array().unwrap().len(), SAMPLE_LIMIT);
        assert_eq!(body["sample"][0], "https://example.com/path/0");
        assert_eq!(body["sample_truncated"], true);
    }

    #[test]
    fn json_payload_without_truncation_says_so() {
        let s = summary(3, false);
        let body: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Json, &s)).unwrap();
        assert_eq!(body["sample"].as_array().unwrap().len(), 3);
        assert_eq!(body["sample_truncated"], false);
        assert_eq!(body["incremental"], false);
    }

    #[test]
    fn slack_and_discord_payloads_use_each_services_minimal_shape() {
        let s = summary(2, true);

        let slack: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Slack, &s)).unwrap();
        let text = slack["text"].as_str().expect("slack payload has `text`");
        assert!(
            text.starts_with("urx: 2 new URLs for example.com (3.2s)"),
            "{text}"
        );
        assert!(text.contains("Wayback Machine 120 urls"), "{text}");
        assert!(text.contains("OTX 3 urls / 1 errors (aborted)"), "{text}");
        assert!(text.contains("https://example.com/path/1"), "{text}");
        assert!(slack.get("content").is_none());

        let discord: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Discord, &s)).unwrap();
        assert_eq!(discord["content"].as_str().unwrap(), text);
        assert!(discord.get("text").is_none());
    }

    #[test]
    fn text_names_the_urls_beyond_the_sample() {
        let s = summary(SAMPLE_LIMIT + 7, false);
        let text = render_text(&s, usize::MAX);
        assert!(text.contains("… 7 more not shown"), "{text}");
        // Without --incremental there is no "new" claim to make.
        assert!(text.starts_with("urx: 27 URLs for example.com"), "{text}");
    }

    // ---- length limits ----------------------------------------------------

    #[test]
    fn chat_messages_are_cut_to_the_service_limit_and_say_so() {
        let mut s = summary(SAMPLE_LIMIT, true);
        // Make each sample line long enough that the whole message cannot fit.
        s.sample = (0..SAMPLE_LIMIT)
            .map(|i| format!("https://example.com/{}/{i}", "x".repeat(300)))
            .collect();

        let discord: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Discord, &s)).unwrap();
        let content = discord["content"].as_str().unwrap();
        assert!(
            content.chars().count() <= DISCORD_MAX_CHARS,
            "{}",
            content.len()
        );
        assert!(
            content.starts_with("urx: 20 new URLs"),
            "header must survive"
        );
        assert!(content.contains("[truncated: "), "{content}");
        assert!(
            content.ends_with("cut to fit the message limit]"),
            "{content}"
        );
        // Whole lines only: no URL is sliced.
        for line in content.lines() {
            if line.starts_with("https://") {
                assert!(line.ends_with(|c: char| c.is_ascii_digit()), "{line}");
            }
        }

        let slack: serde_json::Value =
            serde_json::from_str(&build_payload(NotifyFormat::Slack, &s)).unwrap();
        let text = slack["text"].as_str().unwrap();
        assert!(text.chars().count() <= SLACK_MAX_CHARS);
        assert!(text.contains("[truncated: "));
        // Slack's budget is larger, so it keeps more of the sample.
        assert!(text.chars().count() > content.chars().count());
    }

    #[test]
    fn fit_lines_leaves_a_short_message_alone() {
        let lines = vec!["a".to_string(), "b".to_string()];
        assert_eq!(fit_lines(&lines, 3), "a\nb");
        assert!(!fit_lines(&lines, 100).contains("truncated"));
    }

    #[test]
    fn fit_lines_counts_characters_not_bytes() {
        // Six 3-byte characters: fits in 6 chars, would not fit in 6 bytes.
        let lines = vec!["ção—ãé".to_string()];
        assert_eq!(fit_lines(&lines, 6), "ção—ãé");
    }

    #[test]
    fn fit_lines_survives_a_header_longer_than_the_limit() {
        let lines = vec!["h".repeat(100), "second".to_string()];
        let out = fit_lines(&lines, 60);
        assert!(out.chars().count() <= 60, "{}", out.chars().count());
        assert!(out.ends_with("cut to fit the message limit]"), "{out}");
    }

    // ---- --notify-on ------------------------------------------------------

    #[test]
    fn notify_on_decides_from_the_new_url_count() {
        let empty = summary(0, true);
        let some = summary(1, true);

        assert!(should_send(NotifyOn::Always, &empty));
        assert!(should_send(NotifyOn::Always, &some));

        assert!(!should_send(NotifyOn::New, &empty));
        assert!(should_send(NotifyOn::New, &some));

        assert!(!should_send(NotifyOn::Never, &empty));
        assert!(!should_send(NotifyOn::Never, &some));
    }

    #[tokio::test]
    async fn notify_on_new_stays_quiet_for_an_empty_run() {
        let (mut server, url) = webhook_server().await;
        let hook = server
            .mock("POST", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;

        let args = notify_args(&url, NotifyOn::New, NotifyFormat::Json);
        let out = send_notifications(&args, &NetworkSettings::default(), &summary(0, true)).await;

        assert!(out.is_empty());
        hook.assert_async().await;
    }

    #[tokio::test]
    async fn notify_on_always_sends_an_empty_run() {
        let (mut server, url) = webhook_server().await;
        let hook = server
            .mock("POST", format!("/services/{SECRET}").as_str())
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"url_count": 0, "new_url_count": 0}"#.to_string(),
            ))
            .with_status(200)
            .expect(1)
            .create_async()
            .await;

        let args = notify_args(&url, NotifyOn::Always, NotifyFormat::Json);
        let out = send_notifications(&args, &NetworkSettings::default(), &summary(0, true)).await;

        assert_eq!(out.len(), 1);
        assert!(out[0].is_ok(), "{}", out[0]);
        hook.assert_async().await;
    }

    #[tokio::test]
    async fn notify_on_never_sends_nothing_even_with_results() {
        let (mut server, url) = webhook_server().await;
        let hook = server
            .mock("POST", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;

        let args = notify_args(&url, NotifyOn::Never, NotifyFormat::Slack);
        let out = send_notifications(&args, &NetworkSettings::default(), &summary(5, true)).await;

        assert!(out.is_empty());
        hook.assert_async().await;
    }

    // ---- delivery ---------------------------------------------------------

    #[tokio::test]
    async fn every_notify_url_receives_the_same_payload() {
        let (mut a, url_a) = webhook_server().await;
        let (mut b, url_b) = webhook_server().await;
        let hook_a = a
            .mock("POST", mockito::Matcher::Any)
            .match_body(mockito::Matcher::PartialJsonString(
                r#"{"content": "urx: 2 new URLs for example.com (3.2s)\nproviders: Wayback Machine 120 urls, OTX 3 urls / 1 errors (aborted)\nhttps://example.com/path/0\nhttps://example.com/path/1"}"#.to_string(),
            ))
            .expect(1)
            .create_async()
            .await;
        let hook_b = b
            .mock("POST", mockito::Matcher::Any)
            .expect(1)
            .create_async()
            .await;

        let mut args = notify_args(&url_a, NotifyOn::New, NotifyFormat::Discord);
        args.notify.push(url_b);
        let out = send_notifications(&args, &NetworkSettings::default(), &summary(2, true)).await;

        assert_eq!(out.len(), 2);
        assert!(out.iter().all(Delivery::is_ok));
        hook_a.assert_async().await;
        hook_b.assert_async().await;
    }

    #[tokio::test]
    async fn a_rejected_webhook_is_reported_but_not_an_error() {
        let (mut server, url) = webhook_server().await;
        let _hook = server
            .mock("POST", mockito::Matcher::Any)
            .with_status(500)
            .create_async()
            .await;

        let args = notify_args(&url, NotifyOn::New, NotifyFormat::Json);
        // The signature is the contract: there is no `Result` to fail the run.
        let out: Vec<Delivery> =
            send_notifications(&args, &NetworkSettings::default(), &summary(1, true)).await;

        assert_eq!(out.len(), 1);
        match &out[0] {
            Delivery::Rejected { status, .. } => assert_eq!(*status, 500),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_unreachable_webhook_is_reported_but_not_an_error() {
        // Port 1 is never listening; the connect fails immediately.
        let url = format!("http://127.0.0.1:1/hooks/{SECRET}");
        let args = notify_args(&url, NotifyOn::Always, NotifyFormat::Json);
        let settings = NetworkSettings {
            timeout: 5,
            ..Default::default()
        };

        let out = send_notifications(&args, &settings, &summary(0, false)).await;

        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Delivery::Failed { .. }), "{}", out[0]);
    }

    #[tokio::test]
    async fn a_bad_proxy_does_not_fail_the_run() {
        let (_server, url) = webhook_server().await;
        let args = notify_args(&url, NotifyOn::Always, NotifyFormat::Json);
        let settings = NetworkSettings {
            proxy: Some("not a proxy url".to_string()),
            ..Default::default()
        };

        // Client construction fails; the outcome is an empty list, not a panic.
        let out = send_notifications(&args, &settings, &summary(0, false)).await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn the_request_goes_through_the_configured_proxy() {
        // A mockito server standing in for an HTTP proxy. The webhook host is
        // one that does not resolve, so the only way the request can be
        // answered at all is by going through the proxy — and the Host header
        // the "proxy" sees is the webhook's, not its own.
        let mut proxy = mockito::Server::new_async().await;
        let via_proxy = proxy
            .mock("POST", mockito::Matcher::Any)
            .match_header("host", "webhook.invalid")
            .with_status(204)
            .expect(1)
            .create_async()
            .await;

        let webhook = format!("http://webhook.invalid/hooks/{SECRET}");
        let args = notify_args(&webhook, NotifyOn::Always, NotifyFormat::Json);
        let settings = NetworkSettings {
            proxy: Some(proxy.url()),
            // Scope is deliberately not consulted: the webhook is the
            // operator's own endpoint and takes the egress settings regardless.
            scope: crate::network::NetworkScope::Testers,
            ..Default::default()
        };

        let out = send_notifications(&args, &settings, &summary(0, false)).await;
        assert!(out[0].is_ok(), "{}", out[0]);
        via_proxy.assert_async().await;
    }

    // ---- secret hygiene ---------------------------------------------------

    #[test]
    fn webhook_host_keeps_only_scheme_host_and_port() {
        assert_eq!(
            webhook_host("https://hooks.slack.com/services/T000/B000/secret"),
            "https://hooks.slack.com"
        );
        assert_eq!(
            webhook_host("http://127.0.0.1:8080/hook?token=abc"),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            webhook_host("https://user:pw@example.com/x"),
            "https://example.com"
        );
        assert_eq!(webhook_host("not a url"), "<unparseable webhook URL>");
        assert_eq!(webhook_host("mailto:x@y"), "<webhook with no host>");
    }

    #[tokio::test]
    async fn no_outcome_text_ever_contains_the_webhook_path() {
        // Every string that can reach stderr goes through `Delivery`'s
        // Display; pin that none of the three variants leaks the path.
        let (mut server, url) = webhook_server().await;
        let _ok = server
            .mock("POST", format!("/services/{SECRET}").as_str())
            .with_status(200)
            .create_async()
            .await;
        let client = reqwest::Client::new();

        let sent = deliver(&client, &url, "{}").await;
        assert!(sent.is_ok(), "{sent}");

        let rejected_url = format!("{}/other/{SECRET}", server.url());
        let rejected = deliver(&client, &rejected_url, "{}").await; // 501 from mockito
        assert!(matches!(rejected, Delivery::Rejected { .. }), "{rejected}");

        let failed = deliver(&client, &format!("http://127.0.0.1:1/x/{SECRET}"), "{}").await;
        assert!(matches!(failed, Delivery::Failed { .. }), "{failed}");

        let garbage = deliver(&client, &format!("::nope::{SECRET}"), "{}").await;
        assert!(matches!(garbage, Delivery::Failed { .. }), "{garbage}");

        for outcome in [sent, rejected, failed, garbage] {
            let text = outcome.to_string();
            assert!(!text.contains(SECRET), "leaked secret: {text}");
            assert!(!text.contains("/services/"), "leaked path: {text}");
            assert!(!text.contains("xoxb"), "leaked token: {text}");
            // ...and the debug form, in case it ever reaches a log.
            let debug = format!("{outcome:?}");
            assert!(!debug.contains(SECRET), "leaked secret: {debug}");
        }
    }

    #[test]
    fn the_payload_itself_never_echoes_the_webhook() {
        // A receiver that forwards the body verbatim (a chat channel) must not
        // be handed the credential that reached it.
        let s = summary(3, true);
        for format in [
            NotifyFormat::Json,
            NotifyFormat::Slack,
            NotifyFormat::Discord,
        ] {
            let body = build_payload(format, &s);
            assert!(!body.contains(SECRET));
            assert!(!body.contains("hooks."));
        }
    }
}
