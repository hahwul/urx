//! Shell-facing artifacts generated from the clap command: completion scripts
//! and the man page.
//!
//! Both are produced at runtime behind a flag rather than by a `build.rs`.
//! urx ships as a single binary — `cargo install`, Homebrew, Docker, the
//! release archives — and none of those channels carry a side file out of
//! `OUT_DIR`, so a build-time script would exist only inside the build tree of
//! whoever compiled it. Generated on demand, `urx --completions zsh` and
//! `urx --manpage` travel with the binary and work wherever it was installed
//! from.

use std::io::{self, Write};

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Args;

/// The name completions and the man page are generated for. Both files are
/// keyed by the command the user types, not by the crate name.
const BIN_NAME: &str = "urx";

/// Render the completion script for `shell`.
///
/// Rendered into a buffer rather than straight to the sink because
/// `clap_complete` panics on a write error, and the obvious invocation
/// (`urx --completions zsh | head`) closes the pipe mid-script.
pub fn completion_script(shell: Shell) -> Vec<u8> {
    let mut cmd = Args::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, BIN_NAME, &mut buf);
    buf
}

/// Render the roff man page for the whole CLI.
///
/// The description is attached here rather than on [`Args`] itself: a man page
/// with an empty `DESCRIPTION` reads as unfinished, but putting an `about` on
/// the parser would also prepend it to every `urx --help`.
pub fn man_page() -> io::Result<Vec<u8>> {
    let cmd = Args::command()
        .name(BIN_NAME)
        .about(env!("CARGO_PKG_DESCRIPTION"));
    let mut buf = Vec::new();
    clap_mangen::Man::new(cmd).render(&mut buf)?;
    Ok(buf)
}

/// Write `bytes` to stdout, treating a reader that hung up (`| head`) as a
/// clean stop rather than a failure — the same contract the URL writer uses.
fn write_stdout(bytes: &[u8]) -> Result<()> {
    let mut out = io::stdout();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => Ok(other?),
    }
}

/// `--completions <SHELL>`: print the script and exit.
pub fn print_completions(shell: Shell) -> Result<()> {
    write_stdout(&completion_script(shell))
}

/// `--manpage`: print the roff source and exit.
pub fn print_man_page() -> Result<()> {
    write_stdout(&man_page()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every supported shell must produce a non-empty script that actually
    /// mentions the binary — an empty file installs silently and completes
    /// nothing.
    #[test]
    fn every_shell_renders_a_non_empty_script() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ] {
            let script = String::from_utf8(completion_script(shell))
                .unwrap_or_else(|e| panic!("{shell} completions are not UTF-8: {e}"));
            assert!(!script.trim().is_empty(), "{shell} completions are empty");
            assert!(
                script.contains(BIN_NAME),
                "{shell} completions never name the binary"
            );
        }
    }

    /// The scripts are only useful if they carry the flags. Checking a handful
    /// of representative ones catches a command that was built without its
    /// arguments (e.g. a bare `Command::new`).
    #[test]
    fn completions_cover_the_real_flags() {
        let script = String::from_utf8(completion_script(Shell::Zsh)).unwrap();
        for flag in [
            "--providers",
            "--extract-links",
            "--completions",
            "--manpage",
        ] {
            assert!(script.contains(flag), "zsh completions are missing {flag}");
        }
    }

    /// The whole point of these flags is that they stand alone: a user
    /// installing completions has no domain to name, and the parser must not
    /// demand one.
    #[test]
    fn the_flags_parse_without_a_target() {
        use clap::Parser;

        let args = Args::parse_from(["urx", "--completions", "zsh"]);
        assert_eq!(args.completions, Some(Shell::Zsh));
        assert!(args.domains.is_empty());

        assert!(Args::parse_from(["urx", "--manpage"]).manpage);
    }

    #[test]
    fn man_page_renders_roff_with_the_options() {
        let page = String::from_utf8(man_page().unwrap()).unwrap();
        // `.TH` is the roff title macro every man page carries, after the
        // quote-escaping preamble clap_mangen emits first.
        assert!(page.contains("\n.TH urx 1"), "not a man page: {:.80}", page);
        assert!(page.contains("urx"));
        // roff escapes every hyphen, so the flag appears as `\-\-extract\-links`.
        assert!(page.contains(r"\-\-extract\-links"), "options are missing");
        // A man page whose DESCRIPTION is blank reads as unfinished.
        assert!(
            page.contains("OSINT Archives"),
            "DESCRIPTION section is empty"
        );
    }
}
