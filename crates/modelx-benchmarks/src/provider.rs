//! Benchmark providers: fetch leaderboard rows from the Hugging Face
//! datasets-server and normalize them into [`ProviderData`].
//!
//! Each provider exposes a `pub(crate)` parse function that operates on raw
//! response bytes so it can be unit-tested against committed fixtures without
//! any network access.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::BenchError;
use crate::metric::BenchMetric;

const DATASETS_SERVER: &str = "https://datasets-server.huggingface.co";
const PAGE_LEN: usize = 100;

/// One provider's fetched-and-normalized leaderboard data (cached to disk).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderData {
    /// The producing provider's stable id (e.g. `"lmarena"`).
    pub provider_id: String,
    /// Unix timestamp (seconds) stamped by the cache layer when fetched.
    #[serde(default)]
    pub fetched_at: Option<i64>,
    /// One entry per distinct leaderboard model name.
    #[serde(default)]
    pub entries: Vec<SourceEntry>,
}

/// A single leaderboard model with its per-metric scores.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceEntry {
    /// The raw leaderboard model name (e.g. `claude-opus-4-6`, `GPT-4o`).
    pub model_name: String,
    /// Organization / vendor, when the source provides one.
    #[serde(default)]
    pub organization: Option<String>,
    /// Scores keyed by [`BenchMetric::key`].
    #[serde(default)]
    pub scores: BTreeMap<String, f64>,
}

/// A blocking benchmark data source.
///
/// Implementations must be `Send + Sync` so the join layer can fetch them from
/// a background thread.
pub trait BenchmarkProvider: Send + Sync {
    /// Stable machine id (e.g. `"lmarena"`).
    fn id(&self) -> &str;
    /// Human-readable display name.
    fn name(&self) -> &str;
    /// Perform a blocking fetch (paging all rows) and normalize to entries.
    fn fetch(&self, agent: &ureq::Agent) -> Result<ProviderData, BenchError>;
}

// ---------------------------------------------------------------------------
// datasets-server response shapes (only the fields we consume)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RowsResponse {
    #[serde(default)]
    rows: Vec<RowWrapper>,
    /// Present on live responses; absent in trimmed fixtures.
    #[serde(default)]
    num_rows_total: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RowWrapper {
    row: serde_json::Value,
}

/// Fetch a URL and read the full body as bytes.
fn get_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, BenchError> {
    // The datasets-server occasionally returns transient 502/503s under load;
    // GETs are idempotent, so retry a few times with a short backoff.
    let mut last: Option<BenchError> = None;
    for attempt in 0..3u32 {
        match get_bytes_once(agent, url) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(400 * (attempt as u64 + 1)));
            }
        }
    }
    Err(last.expect("loop runs at least once"))
}

fn get_bytes_once(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, BenchError> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| BenchError::Http(e.to_string()))?;
    response
        .body_mut()
        .read_to_vec()
        .map_err(|e| BenchError::Http(e.to_string()))
}

/// Read `num_rows_total` from a page, so paging can page until exhausted.
fn page_total(bytes: &[u8]) -> Result<Option<u64>, BenchError> {
    let resp: RowsResponse =
        serde_json::from_slice(bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
    Ok(resp.num_rows_total)
}

/// Page through a URL builder until all rows are fetched, concatenating the raw
/// page bytes for each offset. Stops when `num_rows_total` is reached, or (for
/// safety) when a page returns zero rows.
fn fetch_pages(
    agent: &ureq::Agent,
    url_for_offset: impl Fn(usize) -> String,
) -> Result<Vec<Vec<u8>>, BenchError> {
    let mut pages = Vec::new();
    let mut offset = 0usize;
    loop {
        let bytes = get_bytes(agent, &url_for_offset(offset))?;
        let total = page_total(&bytes)?;
        // Count rows on this page to detect exhaustion when total is unknown.
        let resp: RowsResponse =
            serde_json::from_slice(&bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
        let page_rows = resp.rows.len();
        pages.push(bytes);

        offset += PAGE_LEN;
        match total {
            Some(t) if (offset as u64) >= t => break,
            None if page_rows < PAGE_LEN => break,
            _ if page_rows == 0 => break,
            _ => {}
        }
    }
    Ok(pages)
}

/// Read `f64` from a JSON value, accepting numbers only (null/absent → None).
fn as_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
}

/// Read a `String` from a JSON value.
fn as_string(v: &serde_json::Value) -> Option<String> {
    v.as_str().map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// LMArena
// ---------------------------------------------------------------------------

/// LMArena `lmarena-ai/leaderboard-dataset` provider.
///
/// Pulls multiple `(config, category)` slices and merges them per model into
/// one [`SourceEntry`] carrying all its Elo scores.
pub struct LmArenaProvider;

/// The `(config, split, category, metric)` slices we pull from LMArena.
const LMARENA_SLICES: &[(&str, &str, &str, BenchMetric)] = &[
    ("text", "latest", "overall", BenchMetric::ArenaOverall),
    ("text", "latest", "coding", BenchMetric::ArenaCoding),
    ("text", "latest", "math", BenchMetric::ArenaMath),
    (
        "text",
        "latest",
        "instruction_following",
        BenchMetric::ArenaInstruction,
    ),
    (
        "text",
        "latest",
        "hard_prompts",
        BenchMetric::ArenaHardPrompts,
    ),
    (
        "text",
        "latest",
        "creative_writing",
        BenchMetric::ArenaCreative,
    ),
    ("vision", "latest", "overall", BenchMetric::ArenaVision),
    (
        "text_to_image",
        "latest",
        "overall",
        BenchMetric::ArenaImageGen,
    ),
];

impl LmArenaProvider {
    fn filter_url(config: &str, split: &str, category: &str, offset: usize) -> String {
        // URL-encode the `where` value: "category"='<cat>'
        //   %22 = "   %3D = =   %27 = '
        let where_encoded = format!("where=%22category%22%3D%27{category}%27");
        format!(
            "{DATASETS_SERVER}/filter?dataset=lmarena-ai/leaderboard-dataset\
             &config={config}&split={split}&{where_encoded}&offset={offset}&length={PAGE_LEN}"
        )
    }
}

impl BenchmarkProvider for LmArenaProvider {
    fn id(&self) -> &str {
        "lmarena"
    }

    fn name(&self) -> &str {
        "LMArena"
    }

    fn fetch(&self, agent: &ureq::Agent) -> Result<ProviderData, BenchError> {
        // Merge all slices, keyed by raw model_name.
        let mut merged: BTreeMap<String, SourceEntry> = BTreeMap::new();

        for (config, split, category, metric) in LMARENA_SLICES {
            let pages = fetch_pages(agent, |offset| {
                LmArenaProvider::filter_url(config, split, category, offset)
            })?;
            for page in &pages {
                for (name, org, rating) in parse_lmarena(page)? {
                    let entry = merged.entry(name.clone()).or_insert_with(|| SourceEntry {
                        model_name: name.clone(),
                        organization: org.clone(),
                        scores: BTreeMap::new(),
                    });
                    if entry.organization.is_none() {
                        entry.organization = org;
                    }
                    entry.scores.insert(metric.key().to_string(), rating);
                }
            }
        }

        Ok(ProviderData {
            provider_id: self.id().to_string(),
            fetched_at: None,
            entries: merged.into_values().collect(),
        })
    }
}

/// Parse one LMArena `/filter` page into `(model_name, organization, rating)`.
pub(crate) fn parse_lmarena(
    bytes: &[u8],
) -> Result<Vec<(String, Option<String>, f64)>, BenchError> {
    let resp: RowsResponse =
        serde_json::from_slice(bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
    let mut out = Vec::with_capacity(resp.rows.len());
    for wrapper in resp.rows {
        let row = &wrapper.row;
        let name = match as_string(&row["model_name"]) {
            Some(n) => n,
            None => continue,
        };
        let org = as_string(&row["organization"]);
        let rating = match as_f64(&row["rating"]) {
            Some(r) => r,
            None => continue,
        };
        out.push((name, org, rating));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// BigCodeBench
// ---------------------------------------------------------------------------

/// BigCodeBench `bigcode/bigcodebench-results` provider.
pub struct BigCodeBenchProvider;

impl BigCodeBenchProvider {
    fn rows_url(offset: usize) -> String {
        format!(
            "{DATASETS_SERVER}/rows?dataset=bigcode/bigcodebench-results\
             &config=default&split=train&offset={offset}&length={PAGE_LEN}"
        )
    }
}

impl BenchmarkProvider for BigCodeBenchProvider {
    fn id(&self) -> &str {
        "bigcodebench"
    }

    fn name(&self) -> &str {
        "BigCodeBench"
    }

    fn fetch(&self, agent: &ureq::Agent) -> Result<ProviderData, BenchError> {
        let pages = fetch_pages(agent, BigCodeBenchProvider::rows_url)?;
        let mut entries = Vec::new();
        for page in &pages {
            for (name, _org, value) in parse_bigcodebench(page)? {
                let mut scores = BTreeMap::new();
                scores.insert(BenchMetric::CodePassAt1.key().to_string(), value);
                entries.push(SourceEntry {
                    model_name: name,
                    organization: None,
                    scores,
                });
            }
        }
        Ok(ProviderData {
            provider_id: self.id().to_string(),
            fetched_at: None,
            entries,
        })
    }
}

/// Parse one BigCodeBench `/rows` page into `(model, None, instruct_pass@1)`.
///
/// Rows whose `instruct` field is null/absent are skipped.
pub(crate) fn parse_bigcodebench(
    bytes: &[u8],
) -> Result<Vec<(String, Option<String>, f64)>, BenchError> {
    let resp: RowsResponse =
        serde_json::from_slice(bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
    let mut out = Vec::with_capacity(resp.rows.len());
    for wrapper in resp.rows {
        let row = &wrapper.row;
        let name = match as_string(&row["model"]) {
            Some(n) => n,
            None => continue,
        };
        let value = match as_f64(&row["instruct"]) {
            Some(v) => v,
            None => continue, // null instruct -> no code-pass@1 score
        };
        out.push((name, None, value));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Open ASR
// ---------------------------------------------------------------------------

/// Open ASR `hf-audio/open-asr-leaderboard-results` provider.
pub struct OpenAsrProvider;

impl OpenAsrProvider {
    fn rows_url(offset: usize) -> String {
        format!(
            "{DATASETS_SERVER}/rows?dataset=hf-audio/open-asr-leaderboard-results\
             &config=default&split=train&offset={offset}&length={PAGE_LEN}"
        )
    }
}

impl BenchmarkProvider for OpenAsrProvider {
    fn id(&self) -> &str {
        "open-asr"
    }

    fn name(&self) -> &str {
        "Open ASR"
    }

    fn fetch(&self, agent: &ureq::Agent) -> Result<ProviderData, BenchError> {
        let pages = fetch_pages(agent, OpenAsrProvider::rows_url)?;
        let mut entries = Vec::new();
        for page in &pages {
            for (name, _org, value) in parse_open_asr(page)? {
                let mut scores = BTreeMap::new();
                scores.insert(BenchMetric::AsrWer.key().to_string(), value);
                entries.push(SourceEntry {
                    model_name: name,
                    organization: None,
                    scores,
                });
            }
        }
        Ok(ProviderData {
            provider_id: self.id().to_string(),
            fetched_at: None,
            entries,
        })
    }
}

/// Parse one Open ASR `/rows` page into `(model, None, avg_cleaned_wer)`.
///
/// Rows whose `avg cleaned` field is null/absent are skipped.
pub(crate) fn parse_open_asr(
    bytes: &[u8],
) -> Result<Vec<(String, Option<String>, f64)>, BenchError> {
    let resp: RowsResponse =
        serde_json::from_slice(bytes).map_err(|e| BenchError::Parse(e.to_string()))?;
    let mut out = Vec::with_capacity(resp.rows.len());
    for wrapper in resp.rows {
        let row = &wrapper.row;
        let name = match as_string(&row["model"]) {
            Some(n) => n,
            None => continue,
        };
        let value = match as_f64(&row["avg cleaned"]) {
            Some(v) => v,
            None => continue,
        };
        out.push((name, None, value));
    }
    Ok(out)
}

/// Build a ureq agent with the modelx user-agent and a request timeout.
pub(crate) fn default_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .user_agent("modelx/0.2")
        .build()
        .new_agent()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LMARENA: &[u8] = include_bytes!("../tests/fixtures/lmarena_coding.json");
    const BIGCODEBENCH: &[u8] = include_bytes!("../tests/fixtures/bigcodebench.json");
    const OPEN_ASR: &[u8] = include_bytes!("../tests/fixtures/open_asr.json");

    #[test]
    fn lmarena_extracts_models_and_ratings() {
        let rows = parse_lmarena(LMARENA).expect("parse lmarena");
        assert_eq!(rows.len(), 5);

        // First row: claude-opus-4-6, org anthropic, rating ~1535.88.
        let (name, org, rating) = &rows[0];
        assert_eq!(name, "claude-opus-4-6");
        assert_eq!(org.as_deref(), Some("anthropic"));
        assert!((rating - 1535.8765108010277).abs() < 1e-6);

        // The `-thinking` variant is a distinct row.
        assert!(rows.iter().any(|(n, _, _)| n == "claude-opus-4-6-thinking"));
    }

    #[test]
    fn bigcodebench_extracts_instruct_and_skips_nulls() {
        let rows = parse_bigcodebench(BIGCODEBENCH).expect("parse bigcodebench");
        // Fixture has 5 rows but 3 have null `instruct`; only 2 survive.
        assert_eq!(rows.len(), 2);

        let by_name: BTreeMap<_, _> = rows.iter().map(|(n, _, v)| (n.as_str(), *v)).collect();
        assert!((by_name["Magicoder-S-DS-6.7B"] - 36.2).abs() < 1e-6);
        assert!((by_name["StarCoder2-15B-Instruct-v0.1"] - 37.6).abs() < 1e-6);
        // A null-instruct row must not appear.
        assert!(!by_name.contains_key("StarCoder2-3B"));
    }

    #[test]
    fn open_asr_extracts_avg_cleaned_wer() {
        let rows = parse_open_asr(OPEN_ASR).expect("parse open asr");
        assert_eq!(rows.len(), 5);

        let (name, _org, wer) = &rows[0];
        assert_eq!(name, "abr-ai/niagara-19m-batch.en");
        assert!((wer - 9.878571429).abs() < 1e-6);
    }

    #[test]
    fn page_total_absent_in_trimmed_fixtures() {
        // Trimmed fixtures omit num_rows_total, so paging must not depend on it.
        assert_eq!(page_total(LMARENA).unwrap(), None);
    }
}
