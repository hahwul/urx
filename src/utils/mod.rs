pub mod url;
use crate::cli::Args;
pub use url::UrlTransformer;

/// Prints messages only when verbose mode is enabled
///
/// This helper function is used throughout the application to conditionally
/// print information messages based on the command-line arguments.
pub fn verbose_print(args: &Args, message: impl AsRef<str>) {
    if args.verbose && !args.silent {
        println!("{}", message.as_ref());
    }
}
