//! Source registry — holds all registered [`DataSource`]s.

use crate::modelsdev::source::ModelsDevSource;
use crate::source::DataSource;

/// A registry of data sources.
///
/// Use [`SourceRegistry::with_defaults`] to get one pre-loaded with the
/// bundled sources (currently only `models.dev`).
pub struct SourceRegistry {
    sources: Vec<Box<dyn DataSource>>,
    default_id: String,
}

impl SourceRegistry {
    /// Build a registry pre-populated with the default sources.
    ///
    /// The default source is `"models.dev"`.
    pub fn with_defaults() -> Self {
        let mut reg = Self {
            sources: Vec::new(),
            default_id: "models.dev".to_string(),
        };
        reg.register(Box::new(ModelsDevSource::new()));
        reg
    }

    /// Register a new data source.
    pub fn register(&mut self, s: Box<dyn DataSource>) {
        self.sources.push(s);
    }

    /// Return all registered source IDs in insertion order.
    pub fn ids(&self) -> Vec<&str> {
        self.sources.iter().map(|s| s.id()).collect()
    }

    /// Look up a data source by its stable ID.
    pub fn get(&self, id: &str) -> Option<&dyn DataSource> {
        self.sources
            .iter()
            .find(|s| s.id() == id)
            .map(|s| s.as_ref())
    }

    /// The ID of the default source (used when no `--source` flag is given).
    pub fn default_id(&self) -> &str {
        &self.default_id
    }
}
