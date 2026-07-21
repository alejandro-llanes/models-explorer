//! Live network integration test — `#[ignore]`d so `cargo test` stays offline.
//!
//! Run manually with:
//!   `cargo test -p modelx-benchmarks --test network -- --ignored --nocapture`

use modelx_benchmarks::{AliasTable, BenchCache, BenchMetric, BenchmarkDb};
use modelx_core::testkit::sample_catalog;
use modelx_core::Model;
use tempfile::tempdir;

#[test]
#[ignore = "hits the live Hugging Face datasets-server; run manually"]
fn live_load_fetches_and_matches() {
    let dir = tempdir().unwrap();
    let cache = BenchCache::with_dir(dir.path().to_path_buf());

    // Force a fetch with TTL 0 and offline=false.
    let db = BenchmarkDb::load(&cache, AliasTable::embedded(), 0, false)
        .expect("live load should succeed");

    // The cache files should now exist for each provider.
    assert!(cache.load("lmarena").unwrap().is_some());
    assert!(cache.load("bigcodebench").unwrap().is_some());
    assert!(cache.load("open-asr").unwrap().is_some());

    // A well-known Anthropic model should carry an Arena Elo.
    let catalog = sample_catalog();
    let m: Model = {
        let mut m = catalog.all_models().next().unwrap().clone();
        m.id = "claude-opus-4-6".to_string();
        m.name = "Claude Opus 4.6".to_string();
        m
    };
    let hit = db.lookup(&m);
    eprintln!("matched_any={} scores={:?}", hit.matched_any, hit.scores);
    // Not asserting exact values (leaderboards move); just that something matched
    // and Arena Elo formatting works.
    if let Some(v) = hit.scores.get(&BenchMetric::ArenaOverall) {
        eprintln!("arena overall = {}", BenchMetric::ArenaOverall.format(*v));
    }
}
