// Implements different URL output formatters
use std::fmt;

pub trait Formatter: fmt::Debug + Send + Sync {
    fn format(&self, url: &str, is_last: bool) -> String;
    fn clone_box(&self) -> Box<dyn Formatter>;
}

impl Clone for Box<dyn Formatter> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

#[derive(Debug, Clone)]
pub struct PlainFormatter;

impl PlainFormatter {
    pub fn new() -> Self {
        PlainFormatter
    }
}

impl Formatter for PlainFormatter {
    fn format(&self, url: &str, _is_last: bool) -> String {
        format!("{}\n", url)
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
pub struct JsonFormatter;

impl JsonFormatter {
    pub fn new() -> Self {
        JsonFormatter
    }
}

impl Formatter for JsonFormatter {
    fn format(&self, url: &str, is_last: bool) -> String {
        if is_last {
            format!("\"{}\"\n", url)
        } else {
            format!("\"{}\",", url)
        }
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone)]
pub struct CsvFormatter;

impl CsvFormatter {
    pub fn new() -> Self {
        CsvFormatter
    }
}

impl Formatter for CsvFormatter {
    fn format(&self, url: &str, _is_last: bool) -> String {
        format!("{}\n", url)
    }

    fn clone_box(&self) -> Box<dyn Formatter> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_formatter() {
        let formatter = PlainFormatter::new();
        assert_eq!(
            formatter.format("https://example.com", false),
            "https://example.com\n"
        );
        assert_eq!(
            formatter.format("https://example.com", true),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter::new();
        assert_eq!(
            formatter.format("https://example.com", false),
            "\"https://example.com\","
        );
        assert_eq!(
            formatter.format("https://example.com", true),
            "\"https://example.com\"\n"
        );
    }

    #[test]
    fn test_csv_formatter() {
        let formatter = CsvFormatter::new();
        assert_eq!(
            formatter.format("https://example.com", false),
            "https://example.com\n"
        );
        assert_eq!(
            formatter.format("https://example.com", true),
            "https://example.com\n"
        );
    }

    #[test]
    fn test_formatter_clone() {
        let plain_formatter: Box<dyn Formatter> = Box::new(PlainFormatter::new());
        let cloned_formatter = plain_formatter.clone();

        assert_eq!(
            plain_formatter.format("https://example.com", false),
            cloned_formatter.format("https://example.com", false)
        );
    }
}
