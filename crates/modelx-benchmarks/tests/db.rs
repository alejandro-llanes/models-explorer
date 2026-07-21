//! Join-layer and cache tests using the public API only (no network).

use std::collections::BTreeMap;

use modelx_benchmarks::{
    AliasTable, BenchCache, BenchMetric, BenchmarkDb, ProviderData, SourceEntry,
};
use modelx_core::testkit::sample_catalog;
use modelx_core::{Model, ModelRef};
use tempfile::tempdir;

/// Build a one-provider `ProviderData` with the given `(model_name, metric, value)` rows.
fn provider(id: &str, rows: &[(&str, BenchMetric, f64)]) -> ProviderData {
    let mut by_name: BTreeMap<String, SourceEntry> = BTreeMap::new();
    for (name, metric, value) in rows {
        let entry = by_name
            .entry(name.to_string())
            .or_insert_with(|| SourceEntry {
                model_name: name.to_string(),
                organization: None,
                scores: BTreeMap::new(),
            });
        entry.scores.insert(metric.key().to_string(), *value);
    }
    ProviderData {
        provider_id: id.to_string(),
        fetched_at: Some(1_700_000_000),
        entries: by_name.into_values().collect(),
    }
}

/// A minimal catalog model with a chosen id / name.
fn model(id: &str, name: &str) -> Model {
    let catalog = sample_catalog();
    // Clone an existing model and override id/name for a valid, complete struct.
    let mut m = catalog
        .find(&ModelRef {
            provider_id: "provider-a".to_string(),
            model_id: "model-opus".to_string(),
        })
        .unwrap()
        .clone();
    m.id = id.to_string();
    m.name = name.to_string();
    m
}

#[test]
fn direct_normalized_match_merges_scores() {
    let lmarena = provider(
        "lmarena",
        &[("claude-opus-4-6", BenchMetric::ArenaCoding, 1535.87)],
    );
    let bcb = provider(
        "bigcodebench",
        &[("GPT-4o", BenchMetric::CodePassAt1, 61.2)],
    );
    let db = BenchmarkDb::from_sources(vec![lmarena, bcb], AliasTable::embedded());

    // Catalog id `claude-opus-4-6` normalizes to the lmarena entry.
    let m = model("claude-opus-4-6", "Claude Opus 4.6");
    let hit = db.lookup(&m);
    assert!(hit.matched_any);
    assert_eq!(
        hit.scores.get(&BenchMetric::ArenaCoding).copied(),
        Some(1535.87)
    );
    assert_eq!(hit.matched.len(), 1);
    assert_eq!(hit.matched[0].0, "lmarena");
    assert_eq!(hit.matched[0].1, "claude-opus-4-6");

    // Convenience accessor agrees.
    assert_eq!(db.metric_value(&m, BenchMetric::ArenaCoding), Some(1535.87));
}

#[test]
fn match_via_alias_override() {
    // Catalog id is `gpt-4o-mini-x`, but the benchmark row is `GPT-4o`.
    let bcb = provider(
        "bigcodebench",
        &[("GPT-4o", BenchMetric::CodePassAt1, 61.2)],
    );

    let mut aliases = AliasTable::embedded();
    aliases.insert("gpt-4o-mini-x", "GPT-4o");

    let db = BenchmarkDb::from_sources(vec![bcb], aliases);
    let m = model("gpt-4o-mini-x", "Some Display Name");
    let hit = db.lookup(&m);
    assert!(hit.matched_any);
    assert_eq!(
        hit.scores.get(&BenchMetric::CodePassAt1).copied(),
        Some(61.2)
    );
    assert_eq!(hit.matched[0].1, "GPT-4o");
}

#[test]
fn different_version_does_not_match() {
    // Benchmark only has 4-6; catalog model is 4-5. Exact-version => no match.
    let lmarena = provider(
        "lmarena",
        &[("claude-opus-4-6", BenchMetric::ArenaCoding, 1535.87)],
    );
    let db = BenchmarkDb::from_sources(vec![lmarena], AliasTable::embedded());

    let m = model("claude-opus-4-5", "Claude Opus 4.5");
    let hit = db.lookup(&m);
    assert!(!hit.matched_any);
    assert!(hit.scores.is_empty());
    assert!(hit.matched.is_empty());
}

#[test]
fn match_via_model_name_candidate() {
    // Match should also work off the display name when the id doesn't line up.
    let lmarena = provider(
        "lmarena",
        &[("claude-opus-4-6", BenchMetric::ArenaCoding, 1535.87)],
    );
    let db = BenchmarkDb::from_sources(vec![lmarena], AliasTable::embedded());

    let m = model("internal-slug-123", "Claude Opus 4.6");
    let hit = db.lookup(&m);
    assert!(hit.matched_any);
}

#[test]
fn provider_prefix_is_stripped_for_matching() {
    // Catalog id carries a `provider/` prefix; benchmark row does not.
    let bcb = provider(
        "bigcodebench",
        &[("qwen3-30b", BenchMetric::CodePassAt1, 42.0)],
    );
    let db = BenchmarkDb::from_sources(vec![bcb], AliasTable::embedded());

    let m = model("openrouter/qwen3-30b", "Qwen3 30B route");
    let hit = db.lookup(&m);
    assert!(hit.matched_any);
    assert_eq!(
        hit.scores.get(&BenchMetric::CodePassAt1).copied(),
        Some(42.0)
    );
}

#[test]
fn cache_round_trip() {
    let dir = tempdir().unwrap();
    let cache = BenchCache::with_dir(dir.path().to_path_buf());

    // Missing => stale, and load returns None.
    assert!(cache.is_stale("lmarena", 3600));
    assert_eq!(cache.load("lmarena").unwrap(), None);

    let data = provider(
        "lmarena",
        &[("claude-opus-4-6", BenchMetric::ArenaCoding, 1535.87)],
    );
    cache.store(&data).unwrap();

    let loaded = cache.load("lmarena").unwrap();
    assert_eq!(loaded, Some(data));

    // Freshly written => not stale under a generous TTL.
    assert!(!cache.is_stale("lmarena", 3600));
    // Age should be tiny.
    assert!(cache.age_seconds("lmarena").unwrap() < 60);
}
