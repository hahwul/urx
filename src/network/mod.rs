// Network configuration module
//
// This module provides shared network configuration functionality for HTTP requests
// across different parts of the application, such as providers and testers.

pub mod client;
mod settings;
pub mod user_agent;

pub use settings::{NetworkScope, NetworkSettings};
pub use user_agent::random_user_agent;
