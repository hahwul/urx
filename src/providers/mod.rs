use anyhow::Result;
use std::future::Future;
use std::pin::Pin;

mod wayback;
mod commoncrawl;
mod otx;

pub use wayback::WaybackMachineProvider;
pub use commoncrawl::CommonCrawlProvider;
pub use otx::OTXProvider;

pub trait Provider: Send + Sync {
    fn clone_box(&self) -> Box<dyn Provider>;
    
    fn fetch_urls<'a>(
        &'a self, 
        domain: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;
    
    // New methods for common functionality
    fn supports_subdomains(&self) -> bool;
    fn with_subdomains(&mut self, include: bool);
    fn with_proxy(&mut self, proxy: Option<String>);
    fn with_proxy_auth(&mut self, auth: Option<String>);
    
    // New methods for the additional features
    fn with_timeout(&mut self, seconds: u64);
    fn with_retries(&mut self, count: u32);
    fn with_random_agent(&mut self, enabled: bool);
    
    // New methods for parallel processing and rate limiting
    fn with_parallel(&mut self, count: u32);
    fn with_rate_limit(&mut self, requests_per_second: Option<f32>);
}