//! Source debt scanning, temporal priority and durable Soul-compatible archives.
pub mod archive;
mod archive_attach;
mod archive_references;
pub mod database;
pub mod pages;
pub mod recall;
pub mod scan_store;
pub mod scanner;
pub mod semantic_archive;
mod syntax;
pub mod temporal;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
