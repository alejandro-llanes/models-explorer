use serde::{Deserialize, Serialize};

/// A stable key used to identify a model across providers.
/// Used as a `HashSet` key in the TUI selection set.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// A full catalog produced by one data source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// Stable identifier for the data source, e.g. `"models.dev"`.
    pub source_id: String,
    /// Unix timestamp (seconds) set by the cache/datasource layer, not core.
    #[serde(default)]
    pub fetched_at: Option<i64>,
    /// Providers, sorted by name.
    #[serde(default)]
    pub providers: Vec<Provider>,
}

impl Catalog {
    /// Total number of models across all providers.
    pub fn total_models(&self) -> usize {
        self.providers.iter().map(|p| p.models.len()).sum()
    }

    /// Look up a provider by its id.
    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Flat iterator over every model in the catalog (provider order preserved).
    pub fn all_models(&self) -> impl Iterator<Item = &Model> {
        self.providers.iter().flat_map(|p| p.models.iter())
    }

    /// Look up a model by its stable `ModelRef` key.
    pub fn find(&self, key: &ModelRef) -> Option<&Model> {
        self.provider(&key.provider_id)
            .and_then(|p| p.models.iter().find(|m| m.id == key.model_id))
    }
}

/// A model provider (e.g. Anthropic, OpenAI).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// Environment variables required to use this provider, e.g. `["ANTHROPIC_API_KEY"]`.
    #[serde(default)]
    pub env: Vec<String>,
    /// npm package name for AI-SDK usage.
    #[serde(default)]
    pub npm: Option<String>,
    /// Base API URL, if non-standard.
    #[serde(default)]
    pub api: Option<String>,
    /// Documentation URL.
    #[serde(default)]
    pub doc: Option<String>,
    /// Models offered by this provider, sorted by name.
    #[serde(default)]
    pub models: Vec<Model>,
}

/// A single LLM model with its capabilities and pricing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Denormalized provider id — enables flat/search views without walking the tree.
    pub provider_id: String,
    pub provider_name: String,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub attachment: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub structured_output: Option<bool>,
    #[serde(default)]
    pub temperature: Option<bool>,
    #[serde(default)]
    pub open_weights: Option<bool>,
    /// Knowledge cutoff, e.g. `"2026-01"`.
    #[serde(default)]
    pub knowledge: Option<String>,
    /// Release date, e.g. `"2026-05-28"`.
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    /// Model status: `alpha`, `beta`, `deprecated`, or `None` (stable).
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reasoning_options: Vec<ReasoningOption>,
    #[serde(default)]
    pub modalities: Modalities,
    #[serde(default)]
    pub limit: Limit,
    #[serde(default)]
    pub cost: Option<Cost>,
    /// Interleaved output config, kept raw (e.g. `{"field":"reasoning_content"}`).
    #[serde(default)]
    pub interleaved: Option<serde_json::Value>,
    /// Model-level provider override object, kept raw.
    #[serde(default)]
    pub provider_override: Option<serde_json::Value>,
    /// Experimental fields, kept raw.
    #[serde(default)]
    pub experimental: Option<serde_json::Value>,
    /// The untouched source object — powers "show everything" in the detail pane.
    #[serde(default)]
    pub raw: serde_json::Value,
}

impl Model {
    /// Build the stable selection key for this model.
    pub fn key(&self) -> ModelRef {
        ModelRef {
            provider_id: self.provider_id.clone(),
            model_id: self.id.clone(),
        }
    }
}

/// One reasoning option offered by a model (e.g. effort levels, budget tokens).
/// The source JSON field `"type"` is mapped to `kind` by the datasource layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReasoningOption {
    /// The type of reasoning control (maps from JSON `"type"` in models.dev).
    pub kind: String,
    /// Allowed values (e.g. `["low","medium","high"]`).
    #[serde(default)]
    pub values: Vec<String>,
}

/// Input and output modalities supported by a model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Token / context limits for a model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    /// Total context window in tokens.
    #[serde(default)]
    pub context: Option<u64>,
    /// Maximum output tokens.
    #[serde(default)]
    pub output: Option<u64>,
    /// Maximum input tokens (if separately capped).
    #[serde(default)]
    pub input: Option<u64>,
}

/// Per-million-token pricing for a model.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Input cost per million tokens (USD).
    #[serde(default)]
    pub input: Option<f64>,
    /// Output cost per million tokens (USD).
    #[serde(default)]
    pub output: Option<f64>,
    /// Cache-read cost per million tokens.
    #[serde(default)]
    pub cache_read: Option<f64>,
    /// Cache-write cost per million tokens.
    #[serde(default)]
    pub cache_write: Option<f64>,
    /// Reasoning-token cost per million tokens.
    #[serde(default)]
    pub reasoning: Option<f64>,
    /// Audio input cost per million tokens.
    #[serde(default)]
    pub input_audio: Option<f64>,
    /// Audio output cost per million tokens.
    #[serde(default)]
    pub output_audio: Option<f64>,
    /// Tiered pricing beyond 200k context (kept raw).
    #[serde(default)]
    pub context_over_200k: Option<serde_json::Value>,
    /// Pricing tiers (kept raw).
    #[serde(default)]
    pub tiers: Option<serde_json::Value>,
}
