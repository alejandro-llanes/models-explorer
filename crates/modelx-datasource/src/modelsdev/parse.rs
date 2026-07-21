//! `parse_catalog` — shared by `fetch()` and integration tests.

use modelx_core::Catalog;

use super::map::map_catalog;
use super::schema::RawApi;
use crate::source::DataSourceError;

/// Parse raw `api.json` bytes into a [`Catalog`].
///
/// This function is called both by [`ModelsDevSource::fetch`] (after the HTTP
/// download) and by integration tests that feed in the committed fixture.
///
/// `source_id` is stamped into [`Catalog::source_id`]; `fetched_at` is left
/// `None` (the CLI stamps it before writing to the cache).
pub fn parse_catalog(bytes: &[u8]) -> Result<Catalog, DataSourceError> {
    // Parse twice: once into typed structs, once into a raw Value so we can
    // extract per-model `raw` objects without fighting the borrow checker.
    let raw_api: RawApi =
        serde_json::from_slice(bytes).map_err(|e| DataSourceError::Parse(e.to_string()))?;

    let raw_value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| DataSourceError::Parse(e.to_string()))?;

    map_catalog(raw_api, raw_value, "models.dev")
}
