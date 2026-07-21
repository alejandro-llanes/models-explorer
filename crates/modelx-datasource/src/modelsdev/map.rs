//! Mapping from [`RawApi`] (the `models.dev` schema) to [`modelx_core::Catalog`].

use modelx_core::{Catalog, Cost, Limit, Modalities, Model, Provider, ReasoningOption};
use serde_json::Value;

use super::schema::{RawApi, RawCost, RawLimit, RawModalities, RawModel, RawReasoningOption};
use crate::source::DataSourceError;

/// Convert a parsed `RawApi` map (plus the matching raw JSON `Value`) into a
/// [`Catalog`].
///
/// `raw_value` must be the same JSON object that was deserialised into `raw_api`
/// so that we can extract per-model raw objects for `Model.raw`.
pub fn map_catalog(
    raw_api: RawApi,
    raw_value: Value,
    source_id: &str,
) -> Result<Catalog, DataSourceError> {
    let mut providers: Vec<Provider> = raw_api
        .into_iter()
        .map(|(provider_key, raw_provider)| {
            let provider_id = provider_key.clone();
            let provider_name = raw_provider.name.clone();

            let mut models: Vec<Model> = raw_provider
                .models
                .into_iter()
                .map(|(model_key, raw_model)| {
                    // Extract the untouched raw model object from the original JSON value.
                    let raw_obj = raw_value
                        .get(&provider_key)
                        .and_then(|pv| pv.get("models"))
                        .and_then(|mv| mv.get(&model_key))
                        .cloned()
                        .unwrap_or(Value::Null);

                    map_model(
                        raw_model,
                        raw_obj,
                        provider_id.clone(),
                        provider_name.clone(),
                    )
                })
                .collect();

            // Sort models by name (case-insensitive for stability).
            models.sort_by_key(|a| a.name.to_lowercase());

            Provider {
                id: raw_provider.id,
                name: raw_provider.name,
                env: raw_provider.env,
                npm: raw_provider.npm,
                api: raw_provider.api,
                doc: raw_provider.doc,
                models,
            }
        })
        .collect();

    // Sort providers by name.
    providers.sort_by_key(|a| a.name.to_lowercase());

    Ok(Catalog {
        source_id: source_id.to_string(),
        fetched_at: None,
        providers,
    })
}

fn map_model(raw: RawModel, raw_obj: Value, provider_id: String, provider_name: String) -> Model {
    Model {
        id: raw.id,
        name: raw.name,
        description: raw.description,
        provider_id,
        provider_name,
        family: raw.family,
        attachment: raw.attachment,
        reasoning: raw.reasoning,
        tool_call: raw.tool_call,
        structured_output: raw.structured_output,
        temperature: raw.temperature,
        open_weights: raw.open_weights,
        knowledge: raw.knowledge,
        release_date: raw.release_date,
        last_updated: raw.last_updated,
        status: raw.status,
        reasoning_options: raw
            .reasoning_options
            .into_iter()
            .map(map_reasoning_option)
            .collect(),
        modalities: map_modalities(raw.modalities),
        limit: map_limit(raw.limit),
        cost: raw.cost.map(map_cost),
        interleaved: raw.interleaved,
        provider_override: raw.provider_override,
        experimental: raw.experimental,
        raw: raw_obj,
    }
}

fn map_reasoning_option(raw: RawReasoningOption) -> ReasoningOption {
    ReasoningOption {
        kind: raw.kind,
        // Drop any `null` entries the source may include.
        values: raw.values.into_iter().flatten().collect(),
    }
}

fn map_modalities(raw: RawModalities) -> Modalities {
    Modalities {
        input: raw.input,
        output: raw.output,
    }
}

fn map_limit(raw: RawLimit) -> Limit {
    Limit {
        context: raw.context,
        output: raw.output,
        input: raw.input,
    }
}

fn map_cost(raw: RawCost) -> Cost {
    Cost {
        input: raw.input,
        output: raw.output,
        cache_read: raw.cache_read,
        cache_write: raw.cache_write,
        reasoning: raw.reasoning,
        input_audio: raw.input_audio,
        output_audio: raw.output_audio,
        context_over_200k: raw.context_over_200k,
        tiers: raw.tiers,
    }
}
