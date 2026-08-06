//! Wiring between the CLI surface and the engine.
//!
//! The domain modules (`providers`, `filters`, `cache`, `runner`, `output`) know
//! nothing about clap. Everything that turns [`Args`](crate::cli::Args) into
//! their inputs — and everything that reports back to the operator — lives
//! here, so `main` stays a readable sequence of steps.
//!
//! - [`catalog`] — the provider registry and the id validation built on it
//! - [`keys`] — API keys and their precedence rules
//! - [`selection`] — which providers run, and constructing them
//! - [`pipeline`] — filters, transformers, sinks, and testers
//! - [`caching`] — the cache layer wrapped around a run
//! - [`report`] — the header, stats table, and per-domain files

pub mod caching;
pub mod catalog;
pub mod keys;
pub mod pipeline;
pub mod report;
pub mod selection;
