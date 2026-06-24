// Network configuration module
//
// This module provides shared network configuration functionality for HTTP requests
// across different parts of the application, such as providers and testers.

pub mod client;
mod rate_limiter;
mod settings;
pub mod user_agent;

pub use rate_limiter::RateLimiter;
pub use settings::{NetworkScope, NetworkSettings};
pub use user_agent::{default_user_agent, random_user_agent};
