//! Raw serde structs mirroring the `models.dev` `api.json` shape.
//!
//! These are purely for deserialisation; they are converted to `modelx_core` types
//! by `map.rs`.  Unknown fields are ignored (`deny_unknown_fields` is NOT set).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

/// The top-level `api.json` object: a map from `provider_id → RawProvider`.
pub type RawApi = HashMap<String, RawProvider>;

/// A provider entry from `api.json`.
#[derive(Debug, Deserialize)]
pub struct RawProvider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub doc: Option<String>,
    /// Map from `model_id → model_object`.
    #[serde(default)]
    pub models: HashMap<String, RawModel>,
}

/// A model entry from `api.json`.
#[derive(Debug, Deserialize)]
pub struct RawModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
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
    #[serde(default)]
    pub knowledge: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// Reasoning options; each entry uses `"type"` which maps to `ReasoningOption.kind`.
    #[serde(default)]
    pub reasoning_options: Vec<RawReasoningOption>,
    #[serde(default)]
    pub modalities: RawModalities,
    #[serde(default)]
    pub limit: RawLimit,
    #[serde(default)]
    pub cost: Option<RawCost>,
    /// Interleaved output configuration, kept raw.
    #[serde(default)]
    pub interleaved: Option<Value>,
    /// Model-level provider override object (field name is `"provider"` in the source).
    #[serde(rename = "provider", default)]
    pub provider_override: Option<Value>,
    /// Experimental fields, kept raw.
    #[serde(default)]
    pub experimental: Option<Value>,
}

/// A single reasoning option entry (uses `"type"` in the JSON).
#[derive(Debug, Deserialize)]
pub struct RawReasoningOption {
    /// Maps to `ReasoningOption.kind`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Effort values. The source occasionally includes a `null` element, so
    /// each entry is optional and nulls are dropped during mapping.
    #[serde(default)]
    pub values: Vec<Option<String>>,
}

/// Modalities supported by a model.
#[derive(Debug, Default, Deserialize)]
pub struct RawModalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

/// Token / context limits for a model.
#[derive(Debug, Default, Deserialize)]
pub struct RawLimit {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
    #[serde(default)]
    pub input: Option<u64>,
}

/// Pricing information for a model.
#[derive(Debug, Default, Deserialize)]
pub struct RawCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub reasoning: Option<f64>,
    #[serde(default)]
    pub input_audio: Option<f64>,
    #[serde(default)]
    pub output_audio: Option<f64>,
    /// Tiered pricing beyond 200k context (kept raw).
    #[serde(default)]
    pub context_over_200k: Option<Value>,
    /// Pricing tiers (kept raw).
    #[serde(default)]
    pub tiers: Option<Value>,
}
