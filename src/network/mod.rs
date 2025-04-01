// Network configuration module
//
// This module provides shared network configuration functionality for HTTP requests
// across different parts of the application, such as providers and testers.

mod settings;

pub use settings::{NetworkScope, NetworkSettings};
