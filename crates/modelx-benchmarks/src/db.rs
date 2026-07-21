//! [`BenchmarkDb`] — the join layer that matches catalog models to benchmark
//! leaderboard rows and merges their scores.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use modelx_core::Model;

use crate::cache::BenchCache;
use crate::error::BenchError;
use crate::matcher::{normalize, AliasTable};
use crate::metric::BenchMetric;
use crate::provider::{
    BenchmarkProvider, BigCodeBenchProvider, LmArenaProvider, OpenAsrProvider, ProviderData,
};

/// The result of looking up a model in the benchmark database.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BenchMatch {
    /// Merged scores across all matched sources.
    pub scores: BTreeMap<BenchMetric, f64>,
    /// `(source display name, matched benchmark model_name)` per hit.
    pub matched: Vec<(String, String)>,
    /// Whether at least one source matched.
    pub matched_any: bool,
}

/// One source's normalized index: `normalized(model_name) -> entry position`.
struct SourceIndex {
    /// Display name of the source (from `provider_id`).
    name: String,
    data: ProviderData,
    /// normalized model_name -> index into `data.entries`.
    by_norm: BTreeMap<String, usize>,
}

/// The joined benchmark database used by the CLI and TUI.
pub struct BenchmarkDb {
    sources: Vec<SourceIndex>,
    aliases: AliasTable,
}

impl BenchmarkDb {
    /// Build a database from already-loaded provider data and an alias table.
    pub fn from_sources(sources: Vec<ProviderData>, aliases: AliasTable) -> BenchmarkDb {
        let indexed = sources
            .into_iter()
            .map(|data| {
                let mut by_norm = BTreeMap::new();
                for (i, entry) in data.entries.iter().enumerate() {
                    // First writer wins on collision (stable, source order).
                    by_norm.entry(normalize(&entry.model_name)).or_insert(i);
                }
                SourceIndex {
                    name: data.provider_id.clone(),
                    data,
                    by_norm,
                }
            })
            .collect();
        BenchmarkDb {
            sources: indexed,
            aliases,
        }
    }

    /// Load each default provider from cache, fetching if missing/stale (unless
    /// `offline`), stamping `fetched_at = now` and storing, then build the DB.
    ///
    /// Progress notices are written to stderr. A provider that fails to fetch
    /// (and has no cached copy) is skipped rather than aborting the whole load.
    pub fn load(
        cache: &BenchCache,
        aliases: AliasTable,
        ttl_seconds: i64,
        offline: bool,
    ) -> Result<BenchmarkDb, BenchError> {
        let providers: Vec<Box<dyn BenchmarkProvider>> = vec![
            Box::new(LmArenaProvider),
            Box::new(BigCodeBenchProvider),
            Box::new(OpenAsrProvider),
        ];
        let agent = crate::provider::default_agent();
        let mut sources = Vec::new();

        for provider in &providers {
            let id = provider.id();
            let cached = cache.load(id)?;
            let stale = cache.is_stale(id, ttl_seconds);

            if offline || (cached.is_some() && !stale) {
                if let Some(data) = cached {
                    sources.push(data);
                } else if offline {
                    eprintln!("modelx-benchmarks: offline and no cache for {id}, skipping");
                }
                continue;
            }

            // Need to fetch (missing or stale, and online).
            eprintln!("modelx-benchmarks: fetching {} …", provider.name());
            match provider.fetch(&agent) {
                Ok(mut data) => {
                    data.fetched_at = Some(now_unix());
                    if let Err(e) = cache.store(&data) {
                        eprintln!("modelx-benchmarks: failed to cache {id}: {e}");
                    }
                    sources.push(data);
                }
                Err(e) => {
                    eprintln!("modelx-benchmarks: fetch failed for {id}: {e}");
                    if let Some(data) = cached {
                        eprintln!("modelx-benchmarks: using stale cache for {id}");
                        sources.push(data);
                    }
                }
            }
        }

        Ok(BenchmarkDb::from_sources(sources, aliases))
    }

    /// The set of normalized candidate keys for a model.
    fn candidate_keys(&self, model: &Model) -> Vec<String> {
        let mut keys = Vec::new();
        let mut push = |k: String| {
            if !k.is_empty() && !keys.contains(&k) {
                keys.push(k);
            }
        };
        // normalize(model.id) — normalize already drops a leading `provider/`.
        push(normalize(&model.id));
        // Explicitly strip any `provider/` prefix from the id, then normalize.
        if let Some(stripped) = model.id.split_once('/').map(|(_, rest)| rest) {
            push(normalize(stripped));
        }
        // normalize(model.name)
        push(normalize(&model.name));
        keys
    }

    /// Look up a model across all sources, merging matched scores.
    pub fn lookup(&self, model: &Model) -> BenchMatch {
        let candidates = self.candidate_keys(model);
        let mut result = BenchMatch::default();

        for source in &self.sources {
            // Alias override: if the alias table maps one of this model's
            // candidate keys (or its raw ids) to a benchmark name, try that
            // benchmark name (normalized) as an additional candidate.
            let mut source_candidates = candidates.clone();
            for raw in [model.id.as_str(), model.name.as_str()] {
                if let Some(bench_name) = self.aliases.get(&normalize(raw)) {
                    let alias_key = normalize(bench_name);
                    if !source_candidates.contains(&alias_key) {
                        source_candidates.push(alias_key);
                    }
                }
            }

            // First candidate that resolves to an entry wins for this source.
            let hit = source_candidates
                .iter()
                .find_map(|cand| source.by_norm.get(cand).copied());

            if let Some(idx) = hit {
                let entry = &source.data.entries[idx];
                for (key, value) in &entry.scores {
                    if let Some(metric) = BenchMetric::from_key(key) {
                        // Merge; first source to supply a metric wins.
                        result.scores.entry(metric).or_insert(*value);
                    }
                }
                result
                    .matched
                    .push((source.name.clone(), entry.model_name.clone()));
                result.matched_any = true;
            }
        }

        result
    }

    /// Convenience: the merged value of a single metric for a model.
    pub fn metric_value(&self, model: &Model, metric: BenchMetric) -> Option<f64> {
        self.lookup(model).scores.get(&metric).copied()
    }
}

/// Current unix time in seconds.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
