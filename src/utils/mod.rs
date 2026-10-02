pub mod url;
use crate::cli::Args;
pub use url::{ParamView, UrlTransformer};

/// Prints messages only when verbose mode is enabled
///
/// This helper function is used throughout the application to conditionally
/// print information messages based on the command-line arguments.
pub fn verbose_print(args: &Args, message: impl AsRef<str>) {
    if args.verbose && !args.silent {
        println!("{}", message.as_ref());
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
