//! Name normalization and the alias table used to join catalog models to
//! benchmark leaderboard rows.
//!
//! Matching is **exact-version**: two names match only if they refer to the
//! same model version. We deliberately do *not* collapse version numbers, so
//! `claude-opus-4-5` never matches `claude-opus-4-6`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// Normalize a model name or id for exact-version matching.
///
/// The transformation, in order:
/// 1. Lowercase.
/// 2. Drop a single leading `org/` path segment (everything up to and
///    including the first `/`), e.g. `qwen/qwen3-30b` → `qwen3-30b`.
/// 3. Strip trailing route/date noise, repeatedly, from the end of the string:
///    - pure date snapshots: `-YYYYMMDD` / `@YYYYMMDD` (e.g. `-20250805`),
///      and dashed dates `-YYYY-MM-DD` (e.g. `-2025-08-05`);
///    - the route suffixes `-latest` and `-preview`.
/// 4. Remove the punctuation characters ` `, `-`, `_`, `.`, `(`, `)`, `:`,
///    `@`, `/` (collapsing the remaining alphanumerics).
///
/// Note what is **kept**: version numbers and the `-thinking` suffix are
/// preserved (`claude-opus-4-6-thinking` stays distinct from
/// `claude-opus-4-6`), because this is exact-version matching.
pub fn normalize(s: &str) -> String {
    let mut t = s.trim().to_lowercase();

    // Drop a single leading `org/` path segment.
    if let Some(idx) = t.find('/') {
        t = t[idx + 1..].to_string();
    }

    // Repeatedly strip trailing date snapshots and route suffixes.
    loop {
        let before = t.len();
        t = strip_trailing_noise(&t);
        if t.len() == before {
            break;
        }
    }

    // Collapse remaining separators / punctuation.
    t.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '.' | '(' | ')' | ':' | '@' | '/'))
        .collect()
}

/// Strip one trailing date-snapshot or route suffix, if present.
fn strip_trailing_noise(s: &str) -> String {
    // Route suffixes.
    for suffix in ["-latest", "-preview"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            return rest.to_string();
        }
    }

    // Dashed date: `-YYYY-MM-DD`.
    if let Some(rest) = strip_dashed_date(s) {
        return rest;
    }

    // Compact date snapshot: `-YYYYMMDD` or `@YYYYMMDD`.
    if let Some(rest) = strip_compact_date(s, '-') {
        return rest;
    }
    if let Some(rest) = strip_compact_date(s, '@') {
        return rest;
    }

    s.to_string()
}

/// Match a trailing `-YYYY-MM-DD` and return the prefix, if present.
fn strip_dashed_date(s: &str) -> Option<String> {
    // Expect `...-YYYY-MM-DD`, exactly 4-2-2 digit groups at the tail.
    let bytes = s.as_bytes();
    if bytes.len() < 11 {
        return None;
    }
    // Inspect the last 11 BYTES directly — slicing `&s[..]` at a byte offset
    // can land inside a multi-byte UTF-8 char and panic (e.g. an en-dash).
    let tb = &bytes[bytes.len() - 11..];
    let digit = |b: u8| b.is_ascii_digit();
    // pattern: - d d d d - d d - d d
    if tb[0] == b'-'
        && digit(tb[1])
        && digit(tb[2])
        && digit(tb[3])
        && digit(tb[4])
        && tb[5] == b'-'
        && digit(tb[6])
        && digit(tb[7])
        && tb[8] == b'-'
        && digit(tb[9])
        && digit(tb[10])
    {
        return Some(s[..s.len() - 11].to_string());
    }
    None
}

/// Match a trailing `<sep>YYYYMMDD` (8 digits) and return the prefix.
fn strip_compact_date(s: &str, sep: char) -> Option<String> {
    let (head, last) = s.rsplit_once(sep)?;
    if last.len() == 8 && last.bytes().all(|b| b.is_ascii_digit()) {
        return Some(head.to_string());
    }
    None
}

/// A table of same-version naming overrides mapping a normalized catalog key to
/// a raw benchmark `model_name`.
///
/// Aliases exist **only** to bridge naming mismatches for the *same* model
/// version (e.g. a catalog id vs. a leaderboard's display name). They must
/// never be used to substitute a different version.
#[derive(Clone, Debug, Default)]
pub struct AliasTable {
    /// normalized-catalog-key -> raw benchmark model_name
    map: BTreeMap<String, String>,
}

/// TOML shape for an alias config file: `[aliases]\n"catalog-id" = "Bench Name"`.
#[derive(Debug, Deserialize)]
struct AliasFile {
    #[serde(default)]
    aliases: BTreeMap<String, String>,
}

impl AliasTable {
    /// Build an empty table.
    pub fn new() -> AliasTable {
        AliasTable::default()
    }

    /// Insert an override: `catalog_id` (raw, un-normalized) → benchmark name.
    ///
    /// The catalog id is normalized on insertion so lookups are cheap.
    pub fn insert(&mut self, catalog_id: &str, bench_name: &str) {
        self.map
            .insert(normalize(catalog_id), bench_name.to_string());
    }

    /// Look up the benchmark name registered for a normalized catalog key.
    pub fn get(&self, normalized_catalog_key: &str) -> Option<&str> {
        self.map.get(normalized_catalog_key).map(|s| s.as_str())
    }

    /// The built-in set of same-version naming overrides.
    ///
    /// Seeded minimally; extend as concrete naming mismatches are found.
    pub fn embedded() -> AliasTable {
        AliasTable::new()
    }

    /// Merge the embedded table with an optional TOML config file.
    ///
    /// Missing file (or `None`) yields just the embedded table. Config-file
    /// entries override embedded ones on key collision. A malformed file is
    /// ignored (best-effort), leaving the embedded entries intact.
    pub fn load_merged(config_path: Option<&Path>) -> AliasTable {
        let mut table = AliasTable::embedded();
        if let Some(path) = config_path {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(parsed) = toml::from_str::<AliasFile>(&text) {
                    for (catalog_id, bench_name) in parsed.aliases {
                        table.insert(&catalog_id, &bench_name);
                    }
                }
            }
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_and_strips_punctuation() {
        assert_eq!(normalize("Claude Opus 4.6"), "claudeopus46");
        assert_eq!(normalize("GPT-4o"), "gpt4o");
    }

    #[test]
    fn normalize_drops_leading_org_segment() {
        assert_eq!(normalize("qwen/qwen3-30b"), "qwen330b");
        assert_eq!(normalize("openai/gpt-oss-20b"), "gptoss20b");
    }

    #[test]
    fn normalize_is_exact_version_not_collapsing() {
        // Different versions MUST normalize differently.
        assert_ne!(normalize("claude-opus-4-5"), normalize("claude-opus-4-6"));
        assert_eq!(normalize("claude-opus-4-5"), "claudeopus45");
        assert_eq!(normalize("claude-opus-4-6"), "claudeopus46");
    }

    #[test]
    fn normalize_keeps_thinking_suffix() {
        assert_ne!(
            normalize("claude-opus-4-6"),
            normalize("claude-opus-4-6-thinking")
        );
        assert_eq!(
            normalize("claude-opus-4-6-thinking"),
            "claudeopus46thinking"
        );
    }

    #[test]
    fn normalize_strips_date_snapshots() {
        assert_eq!(normalize("claude-3-5-sonnet-20241022"), "claude35sonnet");
        assert_eq!(normalize("gpt-4o@20241022"), "gpt4o");
        assert_eq!(normalize("gpt-oss-20b-2025-08-05"), "gptoss20b");
    }

    #[test]
    fn normalize_strips_route_suffixes() {
        assert_eq!(normalize("gpt-4o-latest"), "gpt4o");
        assert_eq!(normalize("some-model-preview"), "somemodel");
    }

    #[test]
    fn normalize_does_not_strip_bare_numbers_as_dates() {
        // A version like `-4-6` is not an 8-digit date, so it stays.
        assert_eq!(normalize("claude-opus-4-6"), "claudeopus46");
        // A 4-digit tail is not a date snapshot.
        assert_eq!(normalize("model-2024"), "model2024");
    }

    #[test]
    fn alias_table_insert_and_get_by_normalized_key() {
        let mut t = AliasTable::new();
        t.insert("gpt-4o", "GPT-4o");
        assert_eq!(t.get(&normalize("gpt-4o")), Some("GPT-4o"));
        assert_eq!(t.get(&normalize("unknown")), None);
    }

    #[test]
    fn embedded_is_empty_by_default() {
        assert_eq!(AliasTable::embedded().get("anything"), None);
    }

    #[test]
    fn load_merged_missing_file_is_just_embedded() {
        let table = AliasTable::load_merged(Some(std::path::Path::new("/no/such/file.toml")));
        assert_eq!(table.get("anything"), None);
    }
}
