//! `models.dev` data source implementation.
//!
//! - `schema.rs`  — raw serde structs mirroring `api.json`
//! - `map.rs`     — `RawApi → core::Catalog` mapping
//! - `parse.rs`   — `parse_catalog(bytes) → Catalog` (shared by fetch + tests)
//! - `source.rs`  — `ModelsDevSource` struct + `DataSource` impl

pub mod map;
pub mod parse;
pub mod schema;
pub mod source;
