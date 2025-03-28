use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub struct ProgressManager {
    multi_progress: MultiProgress,
}

impl ProgressManager {
    pub fn new() -> Self {
        ProgressManager {
            multi_progress: MultiProgress::new(),
        }
    }

    pub fn create_domain_bar(&self, total: usize) -> ProgressBar {
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
