//! [`ModelsDevSource`] — a [`DataSource`] backed by `models.dev/api.json`.

use std::time::Duration;

use modelx_core::Catalog;
use ureq::config::Config;
use ureq::Agent;

use super::parse::parse_catalog;
use crate::source::{DataSource, DataSourceError};

const DEFAULT_BASE_URL: &str = "https://models.dev/api.json";
const USER_AGENT: &str = "modelx/0.1";
const TIMEOUT_SECS: u64 = 30;

/// A [`DataSource`] that fetches model data from `https://models.dev/api.json`.
pub struct ModelsDevSource {
    base_url: String,
    agent: Agent,
}

impl ModelsDevSource {
    /// Create a source pointed at the live `models.dev` endpoint.
    pub fn new() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    /// Create a source pointed at a custom URL (useful for tests / local mirrors).
    pub fn with_base_url(url: impl Into<String>) -> Self {
        let config = Config::builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
            .user_agent(USER_AGENT)
            .build();
        Self {
            base_url: url.into(),
            agent: config.new_agent(),
        }
    }
}

impl Default for ModelsDevSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for ModelsDevSource {
    fn id(&self) -> &str {
        "models.dev"
    }

    fn name(&self) -> &str {
        "models.dev"
    }

    fn homepage(&self) -> &str {
        "https://models.dev"
    }

    fn fetch(&self) -> Result<Catalog, DataSourceError> {
        let mut response = self
            .agent
            .get(&self.base_url)
            .call()
            .map_err(|e| DataSourceError::Http(e.to_string()))?;

        let bytes = response
            .body_mut()
            .read_to_vec()
            .map_err(|e| DataSourceError::Http(e.to_string()))?;

        parse_catalog(&bytes)
    }
}
