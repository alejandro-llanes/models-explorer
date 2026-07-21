//! `DataSource` trait and `DataSourceError` type.

use thiserror::Error;

/// An error returned by a data source.
#[derive(Debug, Error)]
pub enum DataSourceError {
    /// An HTTP-level error (transport, status code, or I/O).
    #[error("http error: {0}")]
    Http(String),

    /// A parse error (malformed JSON or unexpected schema).
    #[error("parse error: {0}")]
    Parse(String),

    /// A requested source was not found in the registry.
    #[error("source not found: {0}")]
    NotFound(String),
}

/// A blocking data source that can fetch a [`modelx_core::Catalog`].
///
/// Implementations must be `Send + Sync` so they can be stored in the registry
/// and fetched from a background thread.
pub trait DataSource: Send + Sync {
    /// Stable machine identifier (e.g. `"models.dev"`).
    fn id(&self) -> &str;

    /// Human-readable display name.
    fn name(&self) -> &str;

    /// Homepage URL for the data source.
    fn homepage(&self) -> &str;

    /// Perform a blocking HTTP fetch and return a parsed catalog.
    fn fetch(&self) -> Result<modelx_core::Catalog, DataSourceError>;
}
