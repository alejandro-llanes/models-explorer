use thiserror::Error;

/// Errors produced by `modelx-core`.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A JSON serialization / deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
