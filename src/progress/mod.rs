use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Dense braille spinner frames. Cycling these at ~80ms reads as the smooth,
/// continuous motion modern CLIs/agents use — a clear upgrade over a 10-frame
/// dot spinner that visibly stutters.
pub const SPINNER_FRAMES: &[&str] = &["⣷", "⣯", "⣟", "⡿", "⢿", "⣻", "⣽", "⣾"];

/// Eighth-block gradient for determinate bars. The first char is "full", the
/// last is "empty", and the ones in between render sub-cell fractions, so the
/// leading edge of the bar slides smoothly instead of jumping a whole cell.
const BAR_FILL: &str = "█▉▊▋▌▍▎▏ ";

/// Steady-tick interval for spinners (ms). Fast enough to feel alive, slow
/// enough not to flicker or burn CPU.
const SPINNER_TICK_MS: u64 = 80;

/// Steady-tick interval for determinate bars (ms).
const BAR_TICK_MS: u64 = 80;

/// Style for a provider line while a fetch is in flight: animated spinner,
/// bold provider name, free-form status message, and a dimmed elapsed timer.
pub fn provider_running_style() -> ProgressStyle {
    ProgressStyle::with_template("  {spinner:.cyan.bold} {prefix:.bold} {wide_msg} {elapsed:>5.dim}")
        .expect("static provider running template is valid")
        .tick_strings(SPINNER_FRAMES)
}

/// Terminal style for a provider line after a successful fetch. The message is
/// expected to lead with a ✓ glyph; the whole line is tinted green.
pub fn provider_success_style() -> ProgressStyle {
    ProgressStyle::with_template("  {prefix:.green.bold} {wide_msg:.green}")
        .expect("static provider success template is valid")
}

/// Terminal style for a provider line after a failed fetch. The message is
/// expected to lead with a ✗ glyph; the whole line is tinted red.
pub fn provider_error_style() -> ProgressStyle {
    ProgressStyle::with_template("  {prefix:.red.bold} {wide_msg:.red}")
        .expect("static provider error template is valid")
}

/// Terminal style for a provider line that succeeded but returned *incomplete*
/// results (e.g. a paginating fetch lost a page mid-cursor). Tinted yellow so a
/// partial result is visually distinct from a clean ✓ and from a hard ✗.
pub fn provider_partial_style() -> ProgressStyle {
    ProgressStyle::with_template("  {prefix:.yellow.bold} {wide_msg:.yellow}")
        .expect("static provider partial template is valid")
}

/// A small, cloneable handle that providers use to surface fine-grained
/// progress (e.g. "page 3/12") on their own line without knowing anything
/// about `indicatif`. Cloning is cheap and updates are no-ops on a hidden bar,
/// so passing one in costs nothing when progress is disabled.
#[derive(Clone)]
pub struct ProgressReporter {
    bar: ProgressBar,
    /// Stable leading context (e.g. "(1/3) example.com · ") prepended to every
    /// detail so the line keeps identifying which domain is being worked.
    prefix: String,
    /// Set by a provider when the results it is about to return are known to be
    /// incomplete (e.g. a paginating fetch lost a page after already collecting
    /// some). Shared across clones via `Arc`, so the runner that handed the
    /// reporter in can read it back after the fetch resolves and avoid
    /// presenting a truncated result as a clean success.
    partial: Arc<AtomicBool>,
}

impl ProgressReporter {
    /// Build a reporter that writes to `bar`, prefixing each detail with
    /// `prefix` (which should already include any trailing separator).
    pub fn new(bar: ProgressBar, prefix: impl Into<String>) -> Self {
        ProgressReporter {
            bar,
            prefix: prefix.into(),
            partial: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Replace the trailing status detail, keeping the stable prefix.
    pub fn detail(&self, detail: impl AsRef<str>) {
        self.bar
            .set_message(format!("{}{}", self.prefix, detail.as_ref()));
    }

    /// Flag the result as incomplete. The runner reads this via [`is_partial`]
    /// after the fetch to mark the line partial and warn instead of reporting a
    /// clean success.
    ///
    /// [`is_partial`]: ProgressReporter::is_partial
    pub fn mark_partial(&self) {
        self.partial.store(true, Ordering::Relaxed);
    }

    /// Whether the provider flagged the result as incomplete.
    pub fn is_partial(&self) -> bool {
        self.partial.load(Ordering::Relaxed)
    }
}

pub struct ProgressManager {
    multi_progress: MultiProgress,
    no_progress: bool,
}

impl ProgressManager {
    pub fn new(no_progress: bool) -> Self {
        ProgressManager {
            multi_progress: MultiProgress::new(),
            no_progress,
        }
    }

    pub fn create_domain_bar(&self, total: usize) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(total as u64);
            return bar;
        }

        let style = ProgressStyle::with_template(
            "  {prefix:.bold.cyan} {bar:28.cyan/blue} {pos:>3}/{len:<3} {wide_msg:.dim}",
        )
        .unwrap()
        .progress_chars(BAR_FILL);

        let bar = self.multi_progress.add(ProgressBar::new(total as u64));
        bar.set_style(style);
        bar.set_prefix("Domains");
        bar.enable_steady_tick(std::time::Duration::from_millis(BAR_TICK_MS));

        bar
    }

    pub fn create_provider_bars(&self, provider_names: &[String]) -> Vec<ProgressBar> {
        if self.no_progress {
            // Hidden spinners still accept set_message/set_style calls, so the
            // runner can drive them unconditionally without branching.
            return provider_names
                .iter()
                .map(|_| ProgressBar::hidden())
                .collect();
        }

        let style = provider_running_style();

        // Indeterminate spinner per provider: a fetch has no honest percentage
        // (the archive doesn't tell us up front how much it will return), so we
        // animate motion + elapsed time rather than faking a filling bar.
        let bars: Vec<ProgressBar> = provider_names
            .iter()
            .map(|name| {
                let bar = self.multi_progress.add(ProgressBar::new_spinner());
                bar.set_prefix(format!("{name:<15}"));
                bar.set_style(style.clone());
                bar.enable_steady_tick(std::time::Duration::from_millis(SPINNER_TICK_MS));
                bar.set_message("queued…");
                bar
            })
            .collect();

        // Force an initial draw so every line claims its row in the terminal.
        for bar in &bars {
            bar.tick();
        }

        bars
    }

    pub fn create_filter_bar(&self) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(100);
            return bar;
        }

        let style = ProgressStyle::with_template(
            "  {prefix:.bold.yellow} {bar:28.yellow/blue} {wide_msg:.dim}",
        )
        .unwrap()
        .progress_chars(BAR_FILL);

        let bar = self.multi_progress.add(ProgressBar::new(100));
        bar.set_style(style);
        bar.set_prefix("Filtering");
        bar.enable_steady_tick(std::time::Duration::from_millis(BAR_TICK_MS));

        bar
    }

    pub fn create_transform_bar(&self) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(100);
            return bar;
        }

        let style = ProgressStyle::with_template(
            "  {prefix:.bold.magenta} {bar:28.magenta/blue} {wide_msg:.dim}",
        )
        .unwrap()
        .progress_chars(BAR_FILL);

        let bar = self.multi_progress.add(ProgressBar::new(100));
        bar.set_style(style);
        bar.set_prefix("Transforming");
        bar.enable_steady_tick(std::time::Duration::from_millis(BAR_TICK_MS));

        bar
    }

    pub fn create_test_bar(&self, total: usize) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(total as u64);
            return bar;
        }

        let style = ProgressStyle::with_template(
            "  {prefix:.bold.blue} {bar:28.blue/blue} {pos:>5}/{len:<5} {wide_msg:.dim}",
        )
        .unwrap()
        .progress_chars(BAR_FILL);

        let bar = self.multi_progress.add(ProgressBar::new(total as u64));
        bar.set_style(style);
        bar.set_prefix("Testing");
        bar.enable_steady_tick(std::time::Duration::from_millis(BAR_TICK_MS));

        bar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_manager_creation() {
        let _manager = ProgressManager::new(false);
        // Just verify it can be created without error
    }

    #[test]
    fn test_progress_manager_creation_no_progress() {
        let _manager = ProgressManager::new(true);
        // Just verify it can be created without error when no_progress is true
    }

    #[test]
    fn test_create_domain_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_domain_bar(10);

        assert_eq!(bar.length(), Some(10));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_domain_bar_no_progress() {
        let manager = ProgressManager::new(true);
        let bar = manager.create_domain_bar(10);

        // Hidden bar should still have the correct length
        assert_eq!(bar.length(), Some(10));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_provider_bars() {
        let manager = ProgressManager::new(false);
        let provider_names = vec!["wayback".to_string(), "cc".to_string(), "otx".to_string()];

        let bars = manager.create_provider_bars(&provider_names);

        assert_eq!(bars.len(), provider_names.len());
        for bar in bars.iter() {
            // Provider lines are indeterminate spinners, so they carry no length.
            assert_eq!(bar.length(), None);
            assert_eq!(bar.position(), 0);
        }
    }

    #[test]
    fn test_create_provider_bars_no_progress() {
        let manager = ProgressManager::new(true);
        let provider_names = vec!["wayback".to_string(), "cc".to_string(), "otx".to_string()];

        let bars = manager.create_provider_bars(&provider_names);

        assert_eq!(bars.len(), provider_names.len());
        for bar in bars.iter() {
            assert!(bar.is_hidden());
        }
    }

    #[test]
    fn test_progress_reporter_updates_message() {
        let manager = ProgressManager::new(false);
        let bars = manager.create_provider_bars(&["wayback".to_string()]);
        let reporter = ProgressReporter::new(bars[0].clone(), "(1/2) example.com · ");
        reporter.detail("page 3/12");
        assert_eq!(
            bars[0].message(),
            "(1/2) example.com · page 3/12".to_string()
        );
    }

    #[test]
    fn test_progress_reporter_partial_flag_shares_across_clones() {
        let reporter = ProgressReporter::new(ProgressBar::hidden(), "x");
        assert!(!reporter.is_partial());
        let clone = reporter.clone();
        // A provider holds a clone; marking it must be visible to the original
        // handle the runner kept (shared Arc).
        clone.mark_partial();
        assert!(reporter.is_partial());
    }

    #[test]
    fn test_create_provider_bars_empty() {
        let manager = ProgressManager::new(false);
        let provider_names: Vec<String> = vec![];

        let bars = manager.create_provider_bars(&provider_names);

        assert_eq!(bars.len(), 0);
    }

    #[test]
    fn test_create_filter_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_filter_bar();

        assert_eq!(bar.length(), Some(100));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_filter_bar_no_progress() {
        let manager = ProgressManager::new(true);
        let bar = manager.create_filter_bar();

        // Hidden bar should still have the correct length
        assert_eq!(bar.length(), Some(100));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_transform_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_transform_bar();

        assert_eq!(bar.length(), Some(100));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_transform_bar_no_progress() {
        let manager = ProgressManager::new(true);
        let bar = manager.create_transform_bar();

        // Hidden bar should still have the correct length
        assert_eq!(bar.length(), Some(100));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_test_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_test_bar(50);

        assert_eq!(bar.length(), Some(50));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_test_bar_no_progress() {
        let manager = ProgressManager::new(true);
        let bar = manager.create_test_bar(50);

        // Hidden bar should still have the correct length
        assert_eq!(bar.length(), Some(50));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_progress_bar_operations() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_domain_bar(10);

        // Test incrementing position
        bar.set_position(5);
        assert_eq!(bar.position(), 5);

        // Test setting message
        bar.set_message("Processing...");

        // Test finishing
        bar.finish();
    }

    #[test]
    fn test_progress_bar_operations_no_progress() {
        let manager = ProgressManager::new(true);
        let bar = manager.create_domain_bar(10);

        // Test incrementing position on hidden bar
        bar.set_position(5);
        assert_eq!(bar.position(), 5);

        // Test setting message on hidden bar
        bar.set_message("Processing...");

        // Test finishing hidden bar
        bar.finish();
    }
}
