use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

mod status_checker;
mod link_extractor;

pub use status_checker::StatusChecker;
pub use link_extractor::LinkExtractor;

pub trait Tester: Send + Sync {
    fn clone_box(&self) -> Box<dyn Tester>;
    
    fn test_url<'a>(
        &'a self, 
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;
    
    // Common configuration options
    fn with_timeout(&mut self, seconds: u64);
    fn with_retries(&mut self, count: u32);
    fn with_random_agent(&mut self, enabled: bool);
    fn with_proxy(&mut self, proxy: Option<String>);
    fn with_proxy_auth(&mut self, auth: Option<String>);
}