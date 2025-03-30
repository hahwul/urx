use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

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
            "{prefix:.bold.dim} [{bar:40.cyan/blue}] {pos}/{len} {wide_msg}",
        )
        .unwrap()
        .progress_chars("=> ");

        let bar = self.multi_progress.add(ProgressBar::new(total as u64));
        bar.set_style(style);
        bar.set_prefix("Domains");
        bar.enable_steady_tick(std::time::Duration::from_millis(100));

        bar
    }

    pub fn create_provider_bars(&self, provider_names: &[String]) -> Vec<ProgressBar> {
        if self.no_progress {
            // Return hidden progress bars when progress is disabled
            return provider_names
                .iter()
                .map(|_| {
                    let bar = ProgressBar::hidden();
                    bar.set_length(100);
                    bar
                })
                .collect();
        }

        let style = ProgressStyle::with_template(
            "{prefix:.bold.dim} [{bar:30.green/white}] {spinner} {wide_msg}",
        )
        .unwrap()
        .progress_chars("=> ")
        .with_key(
            "spinner",
            |state: &indicatif::ProgressState, w: &mut dyn std::fmt::Write| {
                write!(
                    w,
                    "{}",
                    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][state.pos() as usize % 10]
                )
                .unwrap();
            },
        );

        provider_names
            .iter()
            .map(|name| {
                let bar = self.multi_progress.add(ProgressBar::new(100));
                bar.set_style(style.clone());
                bar.set_prefix(format!("{:<15}", name));
                bar.enable_steady_tick(std::time::Duration::from_millis(100));
                bar
            })
            .collect()
    }

    pub fn create_filter_bar(&self) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(100);
            return bar;
        }

        let style =
            ProgressStyle::with_template("{prefix:.bold.dim} [{bar:40.yellow/white}] {wide_msg}")
                .unwrap()
                .progress_chars("=> ");

        let bar = self.multi_progress.add(ProgressBar::new(100));
        bar.set_style(style);
        bar.set_prefix("Filtering");
        bar.enable_steady_tick(std::time::Duration::from_millis(100));

        bar
    }

    pub fn create_transform_bar(&self) -> ProgressBar {
        if self.no_progress {
            // Return a hidden progress bar when progress is disabled
            let bar = ProgressBar::hidden();
            bar.set_length(100);
            return bar;
        }

        let style =
            ProgressStyle::with_template("{prefix:.bold.dim} [{bar:40.magenta/white}] {wide_msg}")
                .unwrap()
                .progress_chars("=> ");

        let bar = self.multi_progress.add(ProgressBar::new(100));
        bar.set_style(style);
        bar.set_prefix("Transforming");
        bar.enable_steady_tick(std::time::Duration::from_millis(100));

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
            "{prefix:.bold.dim} [{bar:40.blue/white}] {pos}/{len} {wide_msg}",
        )
        .unwrap()
        .progress_chars("=> ");

        let bar = self.multi_progress.add(ProgressBar::new(total as u64));
        bar.set_style(style);
        bar.set_prefix("Testing URLs");
        bar.enable_steady_tick(std::time::Duration::from_millis(100));

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
        assert!(true);
    }

    #[test]
    fn test_create_domain_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_domain_bar(10);

        assert_eq!(bar.length(), Some(10));
        assert_eq!(bar.position(), 0);
    }

    #[test]
    fn test_create_provider_bars() {
        let manager = ProgressManager::new(false);
        let provider_names = vec!["wayback".to_string(), "cc".to_string(), "otx".to_string()];

        let bars = manager.create_provider_bars(&provider_names);

        assert_eq!(bars.len(), provider_names.len());
        for (_i, bar) in bars.iter().enumerate() {
            assert_eq!(bar.length(), Some(100));
            assert_eq!(bar.position(), 0);
        }
    }

    #[test]
    fn test_create_filter_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_filter_bar();

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
    fn test_create_test_bar() {
        let manager = ProgressManager::new(false);
        let bar = manager.create_test_bar(50);

        assert_eq!(bar.length(), Some(50));
        assert_eq!(bar.position(), 0);
    }
}
