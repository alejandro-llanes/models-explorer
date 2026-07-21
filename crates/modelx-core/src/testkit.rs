//! Test helpers for `modelx-core` and downstream crates.
//!
//! `sample_catalog()` returns a deterministic catalog with two providers and
//! four models covering: reasoning on/off, cost None/Some, varied context
//! windows — enough to write meaningful filter, sort, and field tests.

use crate::model::{Catalog, Cost, Limit, Modalities, Model, Provider, ReasoningOption};

/// Build a small but realistic [`Catalog`] for use in tests.
///
/// Layout:
/// - `provider-a` (Anthropic-like)
///   - `model-opus` — reasoning=true, context=1_000_000, input_cost=5.0
///   - `model-haiku` — reasoning=false, context=200_000, input_cost=0.25
/// - `provider-b` (open-weights)
///   - `model-qwen` — reasoning=false, context=262_144, cost=None (open weights)
///   - `model-gpt-oss` — reasoning=true, context=131_072, input_cost=0.0
pub fn sample_catalog() -> Catalog {
    Catalog {
        source_id: "test".to_string(),
        fetched_at: Some(1_700_000_000),
        providers: vec![provider_a(), provider_b()],
    }
}

fn provider_a() -> Provider {
    Provider {
        id: "provider-a".to_string(),
        name: "Anthropic Test".to_string(),
        env: vec!["PROVIDER_A_KEY".to_string()],
        npm: Some("@ai-sdk/provider-a".to_string()),
        api: None,
        doc: Some("https://example.com/docs".to_string()),
        models: vec![model_opus(), model_haiku()],
    }
}

fn provider_b() -> Provider {
    Provider {
        id: "provider-b".to_string(),
        name: "OpenWeights Test".to_string(),
        env: vec!["PROVIDER_B_KEY".to_string()],
        npm: None,
        api: Some("http://localhost:1234/v1".to_string()),
        doc: None,
        models: vec![model_qwen(), model_gpt_oss()],
    }
}

fn model_opus() -> Model {
    Model {
        id: "model-opus".to_string(),
        name: "Test Opus".to_string(),
        description: "Top-tier reasoning model".to_string(),
        provider_id: "provider-a".to_string(),
        provider_name: "Anthropic Test".to_string(),
        family: Some("opus".to_string()),
        attachment: Some(true),
        reasoning: Some(true),
        tool_call: Some(true),
        structured_output: Some(true),
        temperature: Some(false),
        open_weights: Some(false),
        knowledge: Some("2026-01".to_string()),
        release_date: Some("2026-05-28".to_string()),
        last_updated: Some("2026-05-28".to_string()),
        status: None,
        reasoning_options: vec![ReasoningOption {
            kind: "effort".to_string(),
            values: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        }],
        modalities: Modalities {
            input: vec!["text".to_string(), "image".to_string()],
            output: vec!["text".to_string()],
        },
        limit: Limit {
            context: Some(1_000_000),
            output: Some(128_000),
            input: None,
        },
        cost: Some(Cost {
            input: Some(5.0),
            output: Some(25.0),
            cache_read: Some(0.5),
            cache_write: Some(6.25),
            reasoning: None,
            input_audio: None,
            output_audio: None,
            context_over_200k: None,
            tiers: None,
        }),
        interleaved: None,
        provider_override: None,
        experimental: None,
        raw: serde_json::json!({"id": "model-opus"}),
    }
}

fn model_haiku() -> Model {
    Model {
        id: "model-haiku".to_string(),
        name: "Test Haiku".to_string(),
        description: "Fast and cheap model".to_string(),
        provider_id: "provider-a".to_string(),
        provider_name: "Anthropic Test".to_string(),
        family: Some("haiku".to_string()),
        attachment: Some(false),
        reasoning: Some(false),
        tool_call: Some(true),
        structured_output: Some(true),
        temperature: Some(true),
        open_weights: Some(false),
        knowledge: Some("2025-01".to_string()),
        release_date: Some("2025-01-15".to_string()),
        last_updated: Some("2025-01-15".to_string()),
        status: Some("beta".to_string()),
        reasoning_options: vec![],
        modalities: Modalities {
            input: vec!["text".to_string()],
            output: vec!["text".to_string()],
        },
        limit: Limit {
            context: Some(200_000),
            output: Some(8_000),
            input: None,
        },
        cost: Some(Cost {
            input: Some(0.25),
            output: Some(1.25),
            cache_read: Some(0.03),
            cache_write: Some(0.30),
            reasoning: None,
            input_audio: None,
            output_audio: None,
            context_over_200k: None,
            tiers: None,
        }),
        interleaved: None,
        provider_override: None,
        experimental: None,
        raw: serde_json::json!({"id": "model-haiku"}),
    }
}

fn model_qwen() -> Model {
    Model {
        id: "qwen/qwen3-30b".to_string(),
        name: "Qwen3 30B".to_string(),
        description: "Open-weight multilingual model".to_string(),
        provider_id: "provider-b".to_string(),
        provider_name: "OpenWeights Test".to_string(),
        family: Some("qwen".to_string()),
        attachment: Some(false),
        reasoning: Some(false),
        tool_call: Some(true),
        structured_output: None,
        temperature: Some(true),
        open_weights: Some(true),
        knowledge: Some("2025-04".to_string()),
        release_date: Some("2025-07-30".to_string()),
        last_updated: Some("2025-07-30".to_string()),
        status: None,
        reasoning_options: vec![],
        modalities: Modalities {
            input: vec!["text".to_string()],
            output: vec!["text".to_string()],
        },
        limit: Limit {
            context: Some(262_144),
            output: Some(16_384),
            input: None,
        },
        // cost = None to exercise the "no cost" path
        cost: None,
        interleaved: None,
        provider_override: None,
        experimental: None,
        raw: serde_json::json!({"id": "qwen/qwen3-30b"}),
    }
}

fn model_gpt_oss() -> Model {
    Model {
        id: "openai/gpt-oss-20b".to_string(),
        name: "GPT OSS 20B".to_string(),
        description: "Open-weight reasoning model".to_string(),
        provider_id: "provider-b".to_string(),
        provider_name: "OpenWeights Test".to_string(),
        family: Some("gpt-oss".to_string()),
        attachment: Some(false),
        reasoning: Some(true),
        tool_call: Some(true),
        structured_output: None,
        temperature: Some(true),
        open_weights: Some(true),
        knowledge: None,
        release_date: Some("2025-08-05".to_string()),
        last_updated: Some("2025-08-05".to_string()),
        status: None,
        reasoning_options: vec![ReasoningOption {
            kind: "effort".to_string(),
            values: vec!["low".to_string(), "medium".to_string(), "high".to_string()],
        }],
        modalities: Modalities {
            input: vec!["text".to_string()],
            output: vec!["text".to_string()],
        },
        limit: Limit {
            context: Some(131_072),
            output: Some(32_768),
            input: None,
        },
        cost: Some(Cost {
            input: Some(0.0),
            output: Some(0.0),
            cache_read: None,
            cache_write: None,
            reasoning: None,
            input_audio: None,
            output_audio: None,
            context_over_200k: None,
            tiers: None,
        }),
        interleaved: None,
        provider_override: None,
        experimental: None,
        raw: serde_json::json!({"id": "openai/gpt-oss-20b"}),
    }
}
