use anyhow::Result;
use std::path::PathBuf;

mod formatter;
mod writer;

pub use formatter::*;
pub use writer::*;

pub trait Outputter: Send + Sync {
    fn format(&self, url: &str, is_last: bool) -> String;
    fn output(&self, urls: &[String], output_path: Option<PathBuf>, silent: bool) -> Result<()>;
}

pub fn create_outputter(format: &str) -> Box<dyn Outputter> {
    match format.to_lowercase().as_str() {
        "json" => Box::new(JsonOutputter::new()),
        "csv" => Box::new(CsvOutputter::new()),
        _ => Box::new(PlainOutputter::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_outputter_json() {
        let outputter = create_outputter("json");
        // Checks the output of the format method
        assert_eq!(
            outputter.format("https://example.com", false),
            "\"https://example.com\","
        );
    }

    #[test]
    fn test_create_outputter_csv() {
        let outputter = create_outputter("csv");
        assert_eq!(
            outputter.format("https://example.com", false),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_create_outputter_plain() {
        let outputter = create_outputter("plain");
        assert_eq!(
            outputter.format("https://example.com", false),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_create_outputter_default_for_unknown() {
        let outputter = create_outputter("unknown");
        assert_eq!(
            outputter.format("https://example.com", false),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_create_outputter_case_insensitive() {
        let json_outputter = create_outputter("JSON");
        assert_eq!(
            json_outputter.format("https://example.com", false),
            "\"https://example.com\","
        );

        let csv_outputter = create_outputter("CSV");
        assert_eq!(
            csv_outputter.format("https://example.com", false),
            "https://example.com\n"
        );
    }
}
