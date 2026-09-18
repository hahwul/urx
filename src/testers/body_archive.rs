//! `--archive-body-dir`: keep the response bodies `--archive-body` replays.
//!
//! `--archive-body` already pays the expensive part of the work — one request
//! per *distinct* archived body, thanks to CDX digest deduplication — and then
//! discards the bytes as soon as the link extractor has walked them. Everything
//! a link extractor cannot see goes with them: the `<!-- staging.internal -->`
//! comment, the token a 2019 build inlined into its bundle, the stack trace on
//! an error page that names a framework version. Those are the reasons people
//! reach for waymore's response download, and urx was throwing away the very
//! bytes that answer them.
//!
//! So this module writes each replayed body to a directory, next to an
//! `index.jsonl` that maps every file back to the URL and capture it came from.
//! The run leaves a corpus behind that `grep -r` works over afterwards, and the
//! digest deduplication means that corpus covers far more of the target per
//! request than one response per URL would.

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Longest slug taken from the URL before the disambiguating hash is appended.
///
/// Short enough that `slug + '.' + 16 hex + extension` clears the 255-byte
/// name limit every mainstream filesystem imposes, with room to spare for a
/// multi-byte character landing on the boundary.
const MAX_SLUG_BYTES: usize = 120;

/// Hex characters of the URL hash appended to every filename. 16 nibbles is
/// 64 bits: collision-free in practice for corpora that top out in the tens of
/// thousands of bodies, and short enough to stay readable.
const HASH_HEX_LEN: usize = 16;

/// One line of `index.jsonl`: what this file is and where it came from.
///
/// Written as JSON Lines rather than a table because a URL can contain a tab,
/// a comma and a quote, and the index is worthless if one hostile URL shifts
/// every later column.
#[derive(Debug, Serialize)]
struct IndexEntry<'a> {
    /// The original URL, as collected — the thing you actually want back when
    /// `grep -r` lands a hit inside one of these files.
    url: &'a str,
    /// The capture that was replayed, as a 14-digit CDX timestamp.
    timestamp: &'a str,
    /// The archive's content digest, when the index reported one. Equal
    /// digests mean byte-identical bodies, which is exactly why only one of
    /// them was fetched.
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<&'a str>,
    /// Name of the file in the directory, relative to it.
    file: &'a str,
    /// `Content-Type` the archive replayed, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'a str>,
    /// Size of the stored body in bytes.
    bytes: usize,
}

/// A directory of stored response bodies plus the index that explains them.
#[derive(Debug)]
pub struct BodyArchive {
    dir: PathBuf,
    /// The append handle for `index.jsonl`, opened once. Behind a mutex
    /// because every `--parallel` worker shares this one archive and a line
    /// interleaved with another line helps nobody.
    index: Mutex<tokio::fs::File>,
    written: AtomicUsize,
    bytes: AtomicU64,
}

impl BodyArchive {
    /// Name of the manifest written alongside the bodies.
    pub const INDEX_FILE: &'static str = "index.jsonl";

    /// Create (or reuse) `dir` and open its index for appending.
    ///
    /// Done eagerly at start-up rather than on the first body: an unwritable
    /// `--archive-body-dir` should stop the run before it spends an hour
    /// replaying an archive, not after.
    pub fn create(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create --archive-body-dir {}", dir.display()))?;
        let index_path = dir.join(Self::INDEX_FILE);
        let index = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&index_path)
            .with_context(|| format!("Failed to open {}", index_path.display()))?;
        Ok(BodyArchive {
            dir,
            index: Mutex::new(tokio::fs::File::from_std(index)),
            written: AtomicUsize::new(0),
            bytes: AtomicU64::new(0),
        })
    }

    /// The directory being written to.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bodies stored so far.
    pub fn written(&self) -> usize {
        self.written.load(Ordering::Relaxed)
    }

    /// Total bytes stored so far.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// [`Self::bytes`] rendered for the run summary. A corpus is measured in
    /// megabytes far more often than in bytes, and "312651776" tells the
    /// reader nothing about whether their disk is about to fill up.
    pub fn human_bytes(&self) -> String {
        let bytes = self.bytes() as f64;
        const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
        let mut value = bytes;
        let mut unit = 0;
        while value >= 1024.0 && unit + 1 < UNITS.len() {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{} {}", self.bytes(), UNITS[unit])
        } else {
            format!("{value:.1} {}", UNITS[unit])
        }
    }

    /// Write one body and record it in the index.
    ///
    /// Returns the name the body was stored under.
    pub async fn store(
        &self,
        url: &str,
        timestamp: &str,
        digest: Option<&str>,
        content_type: Option<&str>,
        body: &str,
    ) -> Result<String> {
        let name = body_filename(url);
        let path = self.dir.join(&name);
        tokio::fs::write(&path, body.as_bytes())
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;

        let entry = IndexEntry {
            url,
            timestamp,
            digest,
            file: &name,
            content_type,
            bytes: body.len(),
        };
        // One `serde_json` line plus the newline, written under the lock as a
        // single buffer so a partial write cannot split a record.
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        {
            let mut index = self.index.lock().await;
            index.write_all(line.as_bytes()).await.with_context(|| {
                format!(
                    "Failed to append to {}",
                    self.dir.join(Self::INDEX_FILE).display()
                )
            })?;
            // `tokio::fs::File` buffers, and dropping it does not reliably
            // flush — so without this the index could be short of its last
            // lines, or empty, while the body files it is supposed to explain
            // sat on disk. The corpus also has to survive a long run being
            // interrupted, which means the index must be durable as it goes
            // rather than at the end.
            index.flush().await.with_context(|| {
                format!(
                    "Failed to flush {}",
                    self.dir.join(Self::INDEX_FILE).display()
                )
            })?;
        }

        self.written.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
        Ok(name)
    }
}

/// The filename one URL's body is stored under: a readable slug of the URL,
/// then a hash of the whole URL.
///
/// The slug alone cannot be the name. It is lossy by construction — every
/// character a filesystem dislikes becomes `_`, and a long URL is truncated —
/// so `/a?x=1` and `/a?x=2` collapse onto each other, and on a case-insensitive
/// filesystem `/Login` and `/login` do too. The hash is taken over the full,
/// untouched URL, so two different URLs never share a name however much their
/// slugs agree. The slug is kept only so a `grep -r` hit is recognisable
/// without opening the index.
fn body_filename(url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest
        .iter()
        .take(HASH_HEX_LEN.div_ceil(2))
        .map(|b| format!("{b:02x}"))
        .collect();

    let slug = slugify(url);
    if slug.is_empty() {
        hash
    } else {
        format!("{slug}.{hash}")
    }
}

/// Reduce a URL to the recognisable part of a filename: host, path and query,
/// with every character outside `[A-Za-z0-9._-]` folded to a single `_`.
///
/// Leading dots are dropped so a URL can never produce a hidden file, and
/// `..` can never appear as a path component — the name is joined onto a
/// user-supplied directory, so a URL must not be able to climb out of it.
fn slugify(url: &str) -> String {
    let stripped = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    let mut out = String::with_capacity(stripped.len().min(MAX_SLUG_BYTES));
    let mut last_was_sep = false;
    for ch in stripped.chars() {
        // `/` is a separator like any other here: the body files live flat in
        // one directory, so a URL's path must not become a directory tree (and
        // must not be able to escape one).
        let keep = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-');
        let mapped = if keep { ch } else { '_' };
        if mapped == '_' {
            if last_was_sep {
                continue;
            }
            last_was_sep = true;
        } else {
            last_was_sep = false;
        }
        if out.len() + mapped.len_utf8() > MAX_SLUG_BYTES {
            break;
        }
        out.push(mapped);
    }

    // `.` and `_` carry meaning to a shell and to `..`; never start with them.
    out.trim_matches(['.', '_']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_readable_but_unique_per_url() {
        let a = body_filename("https://example.com/shop/item?id=1");
        let b = body_filename("https://example.com/shop/item?id=2");
        // The recognisable part survives...
        assert!(a.starts_with("example.com_shop_item_id_1."), "{a}");
        // ...and the two do not collide even though their slugs nearly agree.
        assert_ne!(a, b);
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn slugs_cannot_escape_the_directory_or_hide_the_file() {
        // A URL is attacker-influenced input joined onto a user-named
        // directory: `..` must never survive as a path component, and a name
        // must never start with a dot.
        let name = body_filename("https://example.com/../../etc/passwd");
        assert!(!name.contains('/'), "{name}");
        assert!(!name.starts_with('.'), "{name}");
        // The decisive check: joined onto a directory, the name resolves to
        // one entry *inside* it and cannot walk anywhere.
        assert_eq!(Path::new(&name).components().count(), 1, "{name}");
        assert_eq!(
            Path::new("/tmp/corpus").join(&name).parent().unwrap(),
            Path::new("/tmp/corpus")
        );
    }

    #[test]
    fn a_url_that_slugifies_to_nothing_still_gets_a_name() {
        let name = body_filename("https://///");
        assert!(!name.is_empty());
        assert_eq!(name.len(), HASH_HEX_LEN);
    }

    #[test]
    fn long_urls_stay_within_the_filesystem_name_limit() {
        let url = format!("https://example.com/{}", "segment/".repeat(200));
        let name = body_filename(&url);
        assert!(
            name.len() <= MAX_SLUG_BYTES + 1 + HASH_HEX_LEN,
            "{}",
            name.len()
        );
    }

    #[tokio::test]
    async fn stored_bodies_land_next_to_an_index_that_maps_them_back() {
        let dir = tempfile::tempdir().unwrap();
        let archive = BodyArchive::create(dir.path().to_path_buf()).unwrap();

        let name = archive
            .store(
                "https://example.com/gone",
                "20180101000000",
                Some("ABCDEF"),
                Some("text/html"),
                "<html><!-- staging.internal --></html>",
            )
            .await
            .unwrap();

        let body = std::fs::read_to_string(dir.path().join(&name)).unwrap();
        assert!(body.contains("staging.internal"));

        let index = std::fs::read_to_string(dir.path().join(BodyArchive::INDEX_FILE)).unwrap();
        let entry: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(entry["url"], "https://example.com/gone");
        assert_eq!(entry["timestamp"], "20180101000000");
        assert_eq!(entry["digest"], "ABCDEF");
        assert_eq!(entry["file"], name);
        assert_eq!(entry["content_type"], "text/html");
        assert_eq!(entry["bytes"], 38);

        assert_eq!(archive.written(), 1);
        assert_eq!(archive.bytes(), 38);
        assert_eq!(archive.human_bytes(), "38 B");
    }

    #[tokio::test]
    async fn the_index_is_readable_while_the_run_is_still_going() {
        // The regression this pins: tokio's File buffers and does not flush on
        // drop, so the index could be empty on disk while the body files it
        // explains were already there — and a run interrupted halfway would
        // leave a corpus nothing could map back to a URL.
        //
        // Note this only *fails* where the buffering actually bites; macOS
        // happened to pass it unflushed, which is how the bug reached CI in
        // the first place. It is a Linux guard, not a portable one.
        let dir = tempfile::tempdir().unwrap();
        let archive = BodyArchive::create(dir.path().to_path_buf()).unwrap();
        for i in 0..3 {
            archive
                .store(
                    &format!("https://example.com/{i}"),
                    "20200101000000",
                    None,
                    None,
                    "body",
                )
                .await
                .unwrap();
        }
        // Read without dropping the archive: the handle is still open.
        let index = std::fs::read_to_string(dir.path().join(BodyArchive::INDEX_FILE)).unwrap();
        assert_eq!(index.lines().count(), 3, "{index}");
        for (i, line) in index.lines().enumerate() {
            let entry: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(entry["url"], format!("https://example.com/{i}"));
        }
    }

    #[tokio::test]
    async fn a_capture_without_a_digest_simply_omits_the_field() {
        let dir = tempfile::tempdir().unwrap();
        let archive = BodyArchive::create(dir.path().to_path_buf()).unwrap();
        archive
            .store("https://example.com/x", "20200101000000", None, None, "hi")
            .await
            .unwrap();
        let index = std::fs::read_to_string(dir.path().join(BodyArchive::INDEX_FILE)).unwrap();
        let entry: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert!(entry.get("digest").is_none());
        assert!(entry.get("content_type").is_none());
    }

    #[test]
    fn sizes_are_reported_in_the_unit_a_reader_can_act_on() {
        let dir = tempfile::tempdir().unwrap();
        let archive = BodyArchive::create(dir.path().to_path_buf()).unwrap();
        archive
            .bytes
            .store(5 * 1024 * 1024 + 512 * 1024, Ordering::Relaxed);
        assert_eq!(archive.human_bytes(), "5.5 MiB");
    }

    #[test]
    fn an_unwritable_directory_fails_at_creation_rather_than_mid_run() {
        let dir = tempfile::tempdir().unwrap();
        // A *file* where the directory should go: `create_dir_all` fails, and
        // the run must learn about it before it starts replaying an archive.
        let blocked = dir.path().join("taken");
        std::fs::write(&blocked, b"").unwrap();
        assert!(BodyArchive::create(blocked).is_err());
    }
}
