//! `modelx-datasource` — the `DataSource` trait, a registry, and the models.dev source.
//!
//! See `docs/architecture.md`.

pub mod modelsdev;
pub mod registry;
pub mod source;

pub use modelsdev::parse::parse_catalog;
pub use modelsdev::source::ModelsDevSource;
pub use registry::SourceRegistry;
pub use source::{DataSource, DataSourceError};
