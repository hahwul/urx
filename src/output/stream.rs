//! Incremental output: emit URLs as providers report them, instead of holding
//! the whole run in memory and printing once at the end.
//!
//! The batch path exists because several features genuinely need the complete
//! result set — `--merge-endpoint` folds URLs together, the cache diffs against
//! a previous run, the testers re-request every URL. Streaming trades those away
//! for latency: `urx big-target.com --stream | grep admin` starts producing
//! matches as soon as the first provider answers, rather than after the slowest
//! one finishes.
//!
//! Every URL still passes the same filters as a batch run — [`StreamSink`] holds
//! the very same [`UrlFilter`], [`UrlTransformer`], and [`HostValidator`] the
//! batch path builds, so the two cannot drift apart. What differs is ordering:
//! streamed output arrives in provider-completion order and is therefore
//! unsorted, where batch output is sorted.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use super::{create_outputter, Outputter, UrlData};
use crate::filters::{HostValidator, UrlFilter};
use crate::utils::UrlTransformer;

/// A sink that filters, deduplicates, and writes URLs as they arrive.
///
/// Cloneable state lives behind mutexes so the concurrent per-provider tasks in
/// [`crate::runner::process_domains`] can all emit into it.
pub struct StreamSink {
    filter: UrlFilter,
    transformer: UrlTransformer,
    host_validator: Option<HostValidator>,
    /// URLs already written, in post-transform form — this is what makes the
    /// stream deduplicated despite never holding the full result set.
    seen: Mutex<HashSet<String>>,
    writer: Mutex<Box<dyn Write + Send>>,
    outputter: Box<dyn Outputter>,
    emitted: AtomicUsize,
    /// Set once the destination stops accepting writes — `urx --stream | head`
    /// closes the pipe long before the providers are done. Later batches are
    /// then dropped instead of re-attempting a write that can only fail again.
    closed: AtomicBool,
}

impl StreamSink {
    /// Build a sink writing to `writer` in `format`.
    ///
    /// `format` must be one the streaming path supports — see
    /// [`format_supports_streaming`]; anything else is rejected by the CLI
    /// before we get here.
    pub fn new(
        filter: UrlFilter,
        transformer: UrlTransformer,
        host_validator: Option<HostValidator>,
        format: &str,
        writer: Box<dyn Write + Send>,
    ) -> Result<Self> {
        let sink = StreamSink {
            filter,
            transformer,
            host_validator,
            seen: Mutex::new(HashSet::new()),
            writer: Mutex::new(writer),
            outputter: create_outputter(format),
            emitted: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
        };

        // CSV needs its header up front. A streamed run carries neither status
        // nor sources (both require the batch path), so the layout is fixed and
        // can be written before the first row.
        if format.eq_ignore_ascii_case("csv") {
            let header = super::formatter::csv_header(false, false);
            let mut w = sink.lock_writer();
            let wrote = w
                .write_all(header.as_bytes())
                .and_then(|()| w.flush())
                .map(|()| true)
                .or_else(broken_pipe_is_ok);
            drop(w);
            if !wrote.context("Failed to write CSV header")? {
                sink.closed.store(true, Ordering::Relaxed);
            }
        }

        Ok(sink)
    }

    /// A poisoned mutex here means another emitter panicked mid-write. The
    /// already-written output is still valid, so recover the guard and keep
    /// going rather than taking the whole run down.
    fn lock_writer(&self) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_seen(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Filter, transform, dedup and write a batch of freshly fetched URLs.
    /// Returns how many were newly written.
    ///
    /// Output is flushed before returning: when stdout is a pipe it is block
    /// buffered, so without an explicit flush a downstream `grep` would see
    /// nothing until the buffer filled — which would defeat the whole point.
    pub fn emit(&self, urls: &[String]) -> Result<usize> {
        if self.closed.load(Ordering::Relaxed) {
            return Ok(0);
        }

        let mut batch: Vec<UrlData> = Vec::new();

        {
            let mut seen = self.lock_seen();
            for url in urls {
                if !self.filter.matches(url) {
                    continue;
                }
                if let Some(v) = &self.host_validator {
                    if !v.is_valid_host(url) {
                        continue;
                    }
                }
                let Some(transformed) = self.transformer.transform_one(url) else {
                    continue;
                };
                if seen.insert(transformed.clone()) {
                    batch.push(UrlData::new(transformed));
                }
            }
        }

        if batch.is_empty() {
            return Ok(0);
        }

        let mut w = self.lock_writer();
        let written = (|| -> std::io::Result<bool> {
            for entry in &batch {
                let formatted = self.outputter.format(entry, false);
                w.write_all(formatted.as_bytes())?;
            }
            w.flush()?;
            Ok(true)
        })()
        .or_else(broken_pipe_is_ok);
        drop(w);

        if !written.context("Failed to write streamed output")? {
            // `urx --stream | head -1`: the consumer has what it wanted. Stop
            // emitting quietly instead of failing the whole run.
            self.closed.store(true, Ordering::Relaxed);
            return Ok(0);
        }

        self.emitted.fetch_add(batch.len(), Ordering::Relaxed);
        Ok(batch.len())
    }

    /// Total URLs written so far.
    pub fn emitted(&self) -> usize {
        self.emitted.load(Ordering::Relaxed)
    }
}

/// Map a closed destination onto `Ok(false)` and leave every other I/O error
/// alone. A downstream `head`/`grep -q` closing the pipe is how a streaming CLI
/// is normally stopped, not a failure to report.
fn broken_pipe_is_ok(e: std::io::Error) -> std::io::Result<bool> {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        Ok(false)
    } else {
        Err(e)
    }
}

/// Whether `format` can be produced incrementally.
///
/// `json` cannot: it wraps entries in one array and separates them with commas,
/// so the writer must know which entry is last. `jsonl` is the streaming
/// equivalent and is what the CLI points users to.
pub fn format_supports_streaming(format: &str) -> bool {
    matches!(
        format.to_lowercase().as_str(),
        "plain" | "jsonl" | "csv" | ""
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A writer that keeps everything in memory so tests can assert on bytes.
    #[derive(Clone, Default)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn contents(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sink_with(format: &str, buf: SharedBuf) -> StreamSink {
        StreamSink::new(
            UrlFilter::new(),
            UrlTransformer::new(),
            None,
            format,
            Box::new(buf),
        )
        .unwrap()
    }

    #[test]
    fn test_emits_urls_immediately() {
        let buf = SharedBuf::default();
        let sink = sink_with("plain", buf.clone());

        assert_eq!(sink.emit(&["https://a.com/1".to_string()]).unwrap(), 1);
        // Written before any later batch arrives — that is the whole point.
        assert_eq!(buf.contents(), "https://a.com/1\n");

        assert_eq!(sink.emit(&["https://a.com/2".to_string()]).unwrap(), 1);
        assert_eq!(buf.contents(), "https://a.com/1\nhttps://a.com/2\n");
        assert_eq!(sink.emitted(), 2);
    }

    #[test]
    fn test_dedupes_across_batches() {
        // Two providers reporting the same URL must not print it twice.
        let buf = SharedBuf::default();
        let sink = sink_with("plain", buf.clone());

        sink.emit(&["https://a.com/x".to_string()]).unwrap();
        let second = sink.emit(&["https://a.com/x".to_string()]).unwrap();

        assert_eq!(second, 0);
        assert_eq!(buf.contents(), "https://a.com/x\n");
        assert_eq!(sink.emitted(), 1);
    }

    #[test]
    fn test_applies_the_same_filters_as_batch_mode() {
        let buf = SharedBuf::default();
        let mut filter = UrlFilter::new();
        filter.with_extensions(vec!["js".to_string()]);
        let sink = StreamSink::new(
            filter,
            UrlTransformer::new(),
            None,
            "plain",
            Box::new(buf.clone()),
        )
        .unwrap();

        sink.emit(&[
            "https://a.com/app.js".to_string(),
            "https://a.com/index.html".to_string(),
        ])
        .unwrap();

        assert_eq!(buf.contents(), "https://a.com/app.js\n");
    }

    #[test]
    fn test_dedupes_on_the_transformed_url() {
        // With --normalize-url these two collapse to one; the stream must not
        // print both just because the raw strings differ.
        let buf = SharedBuf::default();
        let mut transformer = UrlTransformer::new();
        transformer.with_normalize_url(true);
        let sink = StreamSink::new(
            UrlFilter::new(),
            transformer,
            None,
            "plain",
            Box::new(buf.clone()),
        )
        .unwrap();

        sink.emit(&["https://a.com/p?b=2&a=1".to_string()]).unwrap();
        let again = sink
            .emit(&["https://a.com/p/?a=1&b=2".to_string()])
            .unwrap();

        assert_eq!(again, 0);
        assert_eq!(buf.contents(), "https://a.com/p?a=1&b=2\n");
    }

    #[test]
    fn test_host_validation_is_enforced() {
        let buf = SharedBuf::default();
        let sink = StreamSink::new(
            UrlFilter::new(),
            UrlTransformer::new(),
            Some(HostValidator::new(&["a.com".to_string()], false)),
            "plain",
            Box::new(buf.clone()),
        )
        .unwrap();

        sink.emit(&[
            "https://a.com/keep".to_string(),
            "https://evil.com/drop".to_string(),
        ])
        .unwrap();

        assert_eq!(buf.contents(), "https://a.com/keep\n");
    }

    #[test]
    fn test_jsonl_emits_one_standalone_object_per_line() {
        let buf = SharedBuf::default();
        let sink = sink_with("jsonl", buf.clone());

        sink.emit(&["https://a.com/1".to_string(), "https://a.com/2".to_string()])
            .unwrap();

        let out = buf.contents();
        // No array wrapper, no trailing commas: each line parses on its own.
        for line in out.lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(v.get("url").is_some(), "{line}");
        }
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn test_csv_header_written_once_before_rows() {
        let buf = SharedBuf::default();
        let sink = sink_with("csv", buf.clone());

        sink.emit(&["https://a.com/1".to_string()]).unwrap();
        sink.emit(&["https://a.com/2".to_string()]).unwrap();

        let out = buf.contents();
        assert_eq!(out.lines().next().unwrap(), "url");
        assert_eq!(out.matches("url\n").count(), 1, "{out}");
        assert_eq!(out.lines().count(), 3);
    }

    /// A writer that fails every write the way a closed pipe does.
    struct ClosedPipe;

    impl Write for ClosedPipe {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Broken pipe (os error 32)",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A writer that fails with something that is *not* a closed pipe.
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "No space left on device",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_closed_pipe_stops_the_stream_instead_of_failing_the_run() {
        // `urx target --stream | head -1`: the consumer stops reading long
        // before the providers finish. That used to bubble up as
        // "Failed to write streamed URL: Broken pipe" and fail the whole run.
        let sink = StreamSink::new(
            UrlFilter::new(),
            UrlTransformer::new(),
            None,
            "plain",
            Box::new(ClosedPipe),
        )
        .unwrap();

        assert_eq!(sink.emit(&["https://a.com/1".to_string()]).unwrap(), 0);
        // Later batches are dropped without retrying a write that cannot work.
        assert_eq!(sink.emit(&["https://a.com/2".to_string()]).unwrap(), 0);
        assert_eq!(sink.emitted(), 0);
    }

    #[test]
    fn test_a_real_write_failure_is_still_reported() {
        let sink = StreamSink::new(
            UrlFilter::new(),
            UrlTransformer::new(),
            None,
            "plain",
            Box::new(FailingWriter),
        )
        .unwrap();

        let err = sink.emit(&["https://a.com/1".to_string()]).unwrap_err();
        assert!(
            err.to_string().contains("Failed to write streamed output"),
            "{err}"
        );
    }

    #[test]
    fn test_csv_header_on_a_closed_pipe_does_not_fail_construction() {
        // The CSV header is written from the constructor, so it needs the same
        // treatment: `urx target --stream -f csv | head -0` must not error.
        let sink = StreamSink::new(
            UrlFilter::new(),
            UrlTransformer::new(),
            None,
            "csv",
            Box::new(ClosedPipe),
        )
        .unwrap();
        assert_eq!(sink.emit(&["https://a.com/1".to_string()]).unwrap(), 0);
    }

    #[test]
    fn test_format_supports_streaming() {
        assert!(format_supports_streaming("plain"));
        assert!(format_supports_streaming("jsonl"));
        assert!(format_supports_streaming("csv"));
        assert!(format_supports_streaming("JSONL"));
        // json needs to know the last entry to close its array.
        assert!(!format_supports_streaming("json"));
    }
}
