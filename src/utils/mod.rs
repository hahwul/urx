pub mod url;
use crate::cli::Args;
use std::io::{self, Write};
pub use url::{ParamView, UrlTransformer};

/// Write one line to stdout. A closed pipe (`| head -1`) is ignored rather than
/// panicking the way `println!` does.
pub fn stdout_line(msg: &str) {
    let _ = writeln!(io::stdout(), "{msg}");
}

/// Write one line to stderr. Like [`stdout_line`], a closed stream is ignored.
pub fn stderr_line(msg: &str) {
    let _ = writeln!(io::stderr(), "{msg}");
}

/// Prints messages only when verbose mode is enabled
///
/// This helper function is used throughout the application to conditionally
/// print information messages based on the command-line arguments.
pub fn verbose_print(args: &Args, message: impl AsRef<str>) {
    if args.verbose && !args.silent {
        stdout_line(message.as_ref());
    }
}

/// Split a comma-separated list, trimming each entry and dropping blanks, so
/// every source agrees on what `"k1, k2,"` means.
pub fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}
