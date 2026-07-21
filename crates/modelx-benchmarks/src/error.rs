//! [`BenchError`] — the error type for benchmark fetch/parse/cache operations.

use thiserror::Error;

/// An error returned by benchmark providers, cache, or the join layer.
#[derive(Debug, Error)]
pub enum BenchError {
    /// An HTTP-level error (transport, status code, or body read).
    #[error("http error: {0}")]
    Http(String),

    /// A parse error (malformed JSON or unexpected schema).
    #[error("parse error: {0}")]
    Parse(String),

    /// A filesystem / I/O error from the cache layer.
    #[error("i/o error: {0}")]
    Io(String),

    /// The platform cache directory could not be determined.
    #[error("could not determine platform cache directory")]
    NoCacheDir,
}
