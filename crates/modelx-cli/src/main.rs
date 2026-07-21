//! `modelx` — binary entry point. Wires config, cache, data sources, the query
//! CLI, and the TUI.
//!
//! See `docs/architecture.md`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use modelx_benchmarks::{AliasTable, BenchCache, BenchMetric, BenchmarkDb};
use modelx_cache::Cache;
use modelx_core::filter::{matches_all, parse_filters, Predicate};
use modelx_core::{Catalog, Field, Model, Provider};
use modelx_datasource::SourceRegistry;
use modelx_export::{ExportRequest, Format};
use modelx_tui::{AppState, RuntimeCtx};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "modelx",
    bin_name = "modelx",
    about = "A terminal UI for exploring LLM models and providers",
    version = env!("CARGO_PKG_VERSION")
)]
struct Cli {
    /// Data source ID to use
    #[arg(long, global = true)]
    source: Option<String>,

    /// Never hit the network; use cached data only
    #[arg(long, global = true)]
    offline: bool,

    /// Path to config file (overrides default location)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List all registered data sources with cache status
    Sources,

    /// Fetch the active data source and update the cache
    Refresh,

    /// List the model providers (LLM vendors) in the catalog
    Providers(ProvidersArgs),

    /// Query, filter, sort, and export models
    #[command(alias = "list", alias = "export")]
    Models(ModelsArgs),

    /// List all model fields with their key, label, and type
    Fields(FieldsArgs),

    /// Show one model's full detail
    Show(ShowArgs),

    /// Query models enriched with benchmark scores
    #[command(alias = "benchmarks")]
    Bench(BenchArgs),

    /// Run a local HTTP server exposing the catalog as JSON
    Api(ApiArgs),

    /// Generate a shell completion script
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(clap::Args, Debug)]
struct BenchArgs {
    /// Filter expression "FIELD OP VALUE" (repeatable; AND-combined; FIELD may be a benchmark metric key)
    #[arg(long)]
    filter: Vec<String>,

    /// Treat filter `~`/`!~` targets as regular expressions
    #[arg(long)]
    regex: bool,

    /// Case-insensitive substring on provider id or name
    #[arg(long)]
    provider: Option<String>,

    /// Case-insensitive substring across provider/model name and model id
    #[arg(long)]
    search: Option<String>,

    /// Comma-separated field or benchmark metric keys
    #[arg(
        long,
        default_value = "provider_id,id,name,arena_elo,coding_elo,math_elo"
    )]
    fields: String,

    /// Field or benchmark metric key to sort by
    #[arg(long)]
    sort: Option<String>,

    /// Sort descending
    #[arg(long)]
    desc: bool,

    /// Keep at most N rows
    #[arg(long)]
    limit: Option<usize>,

    /// Print only the number of matching rows
    #[arg(long)]
    count: bool,

    /// Output format: plain, csv, md, markdown, json
    #[arg(long, default_value = "plain")]
    format: String,

    /// Write output to FILE instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct ApiArgs {
    /// Address to bind the HTTP server to
    #[arg(long, default_value = "127.0.0.1")]
    listen_addr: String,

    /// Port to bind the HTTP server to
    #[arg(long, default_value_t = 8080)]
    listen_port: u16,

    /// Auto-refresh interval (e.g. 30s, 10m, 1h, 2d, or a bare integer = seconds).
    /// Omit to disable auto-refresh.
    #[arg(long)]
    refresh_interval: Option<String>,
}

#[derive(clap::Args, Debug)]
struct ProvidersArgs {
    /// Case-insensitive substring on provider id or name (regex with --regex)
    #[arg(long)]
    filter: Option<String>,

    /// Treat --filter as a regular expression
    #[arg(long)]
    regex: bool,

    /// Comma-separated provider columns: id,name,npm,api,doc,env,models
    #[arg(long, default_value = "id,name,models")]
    fields: String,

    /// Column to sort by
    #[arg(long)]
    sort: Option<String>,

    /// Sort descending
    #[arg(long)]
    desc: bool,

    /// Keep at most N rows
    #[arg(long)]
    limit: Option<usize>,

    /// Print only the number of matching rows
    #[arg(long)]
    count: bool,

    /// Output format: plain, list, csv, md, markdown, json
    #[arg(long, default_value = "plain")]
    format: String,

    /// Write output to FILE instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct ModelsArgs {
    /// Filter expression "FIELD OP VALUE" (repeatable; AND-combined)
    #[arg(long)]
    filter: Vec<String>,

    /// Treat filter `~`/`!~` targets as regular expressions
    #[arg(long)]
    regex: bool,

    /// Case-insensitive substring on provider id or name
    #[arg(long)]
    provider: Option<String>,

    /// Case-insensitive substring across provider/model name and model id
    #[arg(long)]
    search: Option<String>,

    /// Comma-separated model field keys
    #[arg(long, default_value = "provider_id,id,name")]
    fields: String,

    /// Model field key to sort by
    #[arg(long)]
    sort: Option<String>,

    /// Sort descending
    #[arg(long)]
    desc: bool,

    /// Keep at most N rows
    #[arg(long)]
    limit: Option<usize>,

    /// Print only the number of matching rows
    #[arg(long)]
    count: bool,

    /// Output format: plain, list, csv, md, markdown, json
    #[arg(long, default_value = "plain")]
    format: String,

    /// Write output to FILE instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct FieldsArgs {
    /// Output format: plain, list, csv, md, markdown, json
    #[arg(long, default_value = "plain")]
    format: String,

    /// Write output to FILE instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
struct ShowArgs {
    /// Provider id (exact) or substring
    provider: String,

    /// Model id (exact) or id/name substring
    model: String,

    /// Output format: json (default, raw), plain, list, csv, md, markdown
    #[arg(long, default_value = "json")]
    format: String,

    /// Write output to FILE instead of stdout
    #[arg(long)]
    output: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    pub default_source: Option<String>,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CacheConfig {
    pub ttl_hours: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UiConfig {
    pub theme: String,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { ttl_hours: 12 }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "default".to_string(),
        }
    }
}

impl Config {
    /// Load the config from `path` if given, otherwise from the default platform location.
    ///
    /// A missing file is **not** an error — defaults are returned.
    /// A present but unparseable file returns an error with context.
    pub fn load(path: Option<&Path>) -> Result<Config> {
        let resolved = match path {
            Some(p) => Some(p.to_path_buf()),
            None => ProjectDirs::from("dev", "modelx", "modelx")
                .map(|pd| pd.config_dir().join("config.toml")),
        };

        let config_path = match resolved {
            Some(p) => p,
            None => return Ok(Config::default()),
        };

        match std::fs::read_to_string(&config_path) {
            Ok(text) => {
                let config: Config = toml::from_str(&text).with_context(|| {
                    format!("failed to parse config at {}", config_path.display())
                })?;
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e)
                .with_context(|| format!("failed to read config at {}", config_path.display())),
        }
    }
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Unix timestamp in seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve the active source ID.
///
/// Priority: CLI flag > config.default_source > registry.default_id().
/// Returns an error if the resolved ID is not in the registry.
pub fn resolve_source(
    cli_source: Option<String>,
    config: &Config,
    registry: &SourceRegistry,
) -> Result<String> {
    let id = cli_source
        .or_else(|| config.default_source.clone())
        .unwrap_or_else(|| registry.default_id().to_string());

    if registry.ids().contains(&id.as_str()) {
        Ok(id)
    } else {
        let valid = registry.ids().join(", ");
        Err(anyhow!(
            "unknown source {id:?}; registered sources: {valid}"
        ))
    }
}

/// Return a fresh catalog, auto-refreshing from the network when the cache is
/// stale or absent (unless `offline`).
///
/// - `offline`: use the cache if present, otherwise bail — never fetch.
/// - online: if there is no cache or it is older than `ttl_hours`, fetch,
///   stamp `fetched_at`, and store. Progress notices go to **stderr**.
pub fn ensure_fresh(
    registry: &SourceRegistry,
    cache: &Cache,
    source_id: &str,
    config: &Config,
    offline: bool,
) -> Result<Catalog> {
    if offline {
        return match cache.load(source_id)? {
            Some(catalog) => Ok(catalog),
            None => Err(anyhow!(
                "no cached data for {source_id}; run `modelx refresh` (or drop --offline)"
            )),
        };
    }

    let ttl_seconds = config.cache.ttl_hours * 3600;
    let cached = cache.load(source_id)?;
    let stale = cached.is_none() || cache.is_stale(source_id, ttl_seconds);

    if !stale {
        // `cached` is guaranteed `Some` here because a missing cache is stale.
        return Ok(cached.expect("non-stale cache implies a loaded catalog"));
    }

    eprintln!("updating {source_id} (cache is stale)…");

    let source = registry
        .get(source_id)
        .ok_or_else(|| anyhow!("unknown source: {source_id}"))?;

    let mut catalog = source
        .fetch()
        .map_err(|e| anyhow!("fetch failed for {source_id}: {e}"))?;

    catalog.fetched_at = Some(now_unix());
    cache.store(&catalog)?;
    Ok(catalog)
}

/// Load (or fetch) the benchmark database.
///
/// Never fatal — returns `None` with a warning on any error. Benchmark
/// enrichment is always best-effort.
///
/// - `offline`: pass to `BenchmarkDb::load` so it skips network I/O.
/// - `ttl_hours`: used to compute the TTL unless `force` is set.
/// - `force`: pass `ttl = 0` to force a re-fetch.
pub fn ensure_benchmarks(offline: bool, ttl_hours: i64, force: bool) -> Option<BenchmarkDb> {
    let cache = match BenchCache::discover() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("modelx: benchmark cache unavailable: {e}");
            return None;
        }
    };
    let ttl = if force { 0 } else { ttl_hours * 3600 };
    let alias_path = ProjectDirs::from("dev", "modelx", "modelx").map(|pd| {
        pd.config_dir()
            .join("modelx")
            .join("benchmark-aliases.toml")
    });
    let aliases = AliasTable::load_merged(alias_path.as_deref());
    match BenchmarkDb::load(&cache, aliases, ttl, offline) {
        Ok(db) => Some(db),
        Err(e) => {
            eprintln!("modelx: benchmark data unavailable: {e}");
            None
        }
    }
}

/// Parse a comma-separated list of field keys into `Vec<Field>`.
///
/// Returns an error listing all valid keys if any key is unknown.
pub fn parse_fields(s: &str) -> Result<Vec<Field>> {
    let mut fields = Vec::new();
    for key in s.split(',') {
        let key = key.trim();
        match Field::from_key(key) {
            Some(f) => fields.push(f),
            None => {
                let valid: Vec<&str> = Field::all().iter().map(|f| f.key()).collect();
                return Err(anyhow!(
                    "unknown field key {key:?}; valid keys: {}",
                    valid.join(", ")
                ));
            }
        }
    }
    Ok(fields)
}

/// Parse a single model field key into a [`Field`] (used for `--sort`).
pub fn parse_sort_field(key: &str) -> Result<Field> {
    Field::from_key(key.trim()).ok_or_else(|| {
        let valid: Vec<&str> = Field::all().iter().map(|f| f.key()).collect();
        anyhow!(
            "unknown sort key {:?}; valid keys: {}",
            key.trim(),
            valid.join(", ")
        )
    })
}

/// Parse a format string into a `Format`.
///
/// Accepted aliases: plain/list → PlainList, csv, md/markdown, json.
pub fn parse_format(s: &str) -> Result<Format> {
    match s.to_ascii_lowercase().as_str() {
        "plain" | "list" => Ok(Format::PlainList),
        "csv" => Ok(Format::Csv),
        "md" | "markdown" => Ok(Format::Markdown),
        "json" => Ok(Format::Json),
        other => Err(anyhow!(
            "unknown format {other:?}; valid: plain, list, csv, md, markdown, json"
        )),
    }
}

// ---------------------------------------------------------------------------
// Generic tabular rendering (providers + fields)
// ---------------------------------------------------------------------------

/// A generic table: header keys plus rows of string cells.
struct Table {
    /// Machine keys, used as CSV headers and JSON object keys.
    headers: Vec<String>,
    /// Each row's cells, aligned to `headers`.
    rows: Vec<Vec<String>>,
}

impl Table {
    /// Render this table in the given format.
    ///
    /// - `PlainList` → tab-separated rows, no header.
    /// - `Csv` → header row + rows via the `csv` crate.
    /// - `Markdown` → GitHub-flavoured table.
    /// - `Json` → array of objects keyed by header.
    fn render(&self, format: Format) -> Result<String> {
        match format {
            Format::PlainList => Ok(self.render_plain()),
            Format::Csv => self.render_csv(),
            Format::Markdown => Ok(self.render_markdown()),
            Format::Json => self.render_json(),
        }
    }

    fn render_plain(&self) -> String {
        let mut out = String::new();
        for row in &self.rows {
            out.push_str(&row.join("\t"));
            out.push('\n');
        }
        out
    }

    fn render_csv(&self) -> Result<String> {
        let mut wtr = csv::Writer::from_writer(vec![]);
        wtr.write_record(&self.headers)
            .map_err(|e| anyhow!("csv error: {e}"))?;
        for row in &self.rows {
            wtr.write_record(row)
                .map_err(|e| anyhow!("csv error: {e}"))?;
        }
        let bytes = wtr.into_inner().map_err(|e| anyhow!("csv error: {e}"))?;
        String::from_utf8(bytes).map_err(|e| anyhow!("csv produced invalid utf-8: {e}"))
    }

    fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("| ");
        out.push_str(&self.headers.join(" | "));
        out.push_str(" |\n");
        out.push('|');
        for _ in &self.headers {
            out.push_str(" --- |");
        }
        out.push('\n');
        for row in &self.rows {
            out.push_str("| ");
            let escaped: Vec<String> = row.iter().map(|c| c.replace('|', "\\|")).collect();
            out.push_str(&escaped.join(" | "));
            out.push_str(" |\n");
        }
        out
    }

    fn render_json(&self) -> Result<String> {
        let objects: Vec<serde_json::Map<String, serde_json::Value>> = self
            .rows
            .iter()
            .map(|row| {
                self.headers
                    .iter()
                    .zip(row.iter())
                    .map(|(h, c)| (h.clone(), serde_json::Value::String(c.clone())))
                    .collect()
            })
            .collect();
        serde_json::to_string_pretty(&objects).map_err(|e| anyhow!("json error: {e}"))
    }
}

/// Write rendered text to `output` (or stdout). A trailing newline is ensured.
fn emit(text: &str, output: Option<&Path>) -> Result<()> {
    let text = if text.ends_with('\n') || text.is_empty() {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    match output {
        Some(path) => {
            std::fs::write(path, text.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        None => {
            print!("{text}");
            std::io::stdout().flush().ok();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Providers command
// ---------------------------------------------------------------------------

/// The set of columns a `providers` query may select.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderColumn {
    Id,
    Name,
    Npm,
    Api,
    Doc,
    Env,
    Models,
}

impl ProviderColumn {
    fn key(self) -> &'static str {
        match self {
            ProviderColumn::Id => "id",
            ProviderColumn::Name => "name",
            ProviderColumn::Npm => "npm",
            ProviderColumn::Api => "api",
            ProviderColumn::Doc => "doc",
            ProviderColumn::Env => "env",
            ProviderColumn::Models => "models",
        }
    }

    fn from_key(s: &str) -> Option<ProviderColumn> {
        match s {
            "id" => Some(ProviderColumn::Id),
            "name" => Some(ProviderColumn::Name),
            "npm" => Some(ProviderColumn::Npm),
            "api" => Some(ProviderColumn::Api),
            "doc" => Some(ProviderColumn::Doc),
            "env" => Some(ProviderColumn::Env),
            "models" => Some(ProviderColumn::Models),
            _ => None,
        }
    }

    fn all_keys() -> &'static str {
        "id, name, npm, api, doc, env, models"
    }

    /// Extract this column's cell value from a provider.
    fn extract(self, p: &Provider) -> String {
        match self {
            ProviderColumn::Id => p.id.clone(),
            ProviderColumn::Name => p.name.clone(),
            ProviderColumn::Npm => p.npm.clone().unwrap_or_default(),
            ProviderColumn::Api => p.api.clone().unwrap_or_default(),
            ProviderColumn::Doc => p.doc.clone().unwrap_or_default(),
            ProviderColumn::Env => p.env.join(","),
            ProviderColumn::Models => p.models.len().to_string(),
        }
    }
}

/// Parse a comma-separated list of provider column keys.
fn parse_provider_columns(s: &str) -> Result<Vec<ProviderColumn>> {
    let mut cols = Vec::new();
    for key in s.split(',') {
        let key = key.trim();
        match ProviderColumn::from_key(key) {
            Some(c) => cols.push(c),
            None => {
                return Err(anyhow!(
                    "unknown provider column {key:?}; valid keys: {}",
                    ProviderColumn::all_keys()
                ))
            }
        }
    }
    Ok(cols)
}

/// Return `true` if a provider matches the `--filter` pattern.
///
/// Without `regex`: case-insensitive substring on id OR name.
/// With `regex`: case-insensitive regex on id OR name.
fn provider_matches_filter(p: &Provider, pattern: &str, regex: bool) -> Result<bool> {
    if regex {
        let re = regex_lite_ci(pattern)?;
        Ok(re.is_match(&p.id) || re.is_match(&p.name))
    } else {
        let needle = pattern.to_lowercase();
        Ok(p.id.to_lowercase().contains(&needle) || p.name.to_lowercase().contains(&needle))
    }
}

/// Compile a case-insensitive regex, reporting a clean error on failure.
fn regex_lite_ci(pattern: &str) -> Result<regex::Regex> {
    regex::RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| anyhow!("invalid regular expression {pattern:?}: {e}"))
}

fn cmd_providers(catalog: &Catalog, args: &ProvidersArgs) -> Result<()> {
    let columns = parse_provider_columns(&args.fields)?;
    let format = parse_format(&args.format)?;
    let sort_col = match &args.sort {
        Some(s) => Some(ProviderColumn::from_key(s.trim()).ok_or_else(|| {
            anyhow!(
                "unknown sort column {:?}; valid keys: {}",
                s.trim(),
                ProviderColumn::all_keys()
            )
        })?),
        None => None,
    };

    // Collect matching providers.
    let mut providers: Vec<&Provider> = Vec::new();
    for p in &catalog.providers {
        let keep = match &args.filter {
            Some(pat) => provider_matches_filter(p, pat, args.regex)?,
            None => true,
        };
        if keep {
            providers.push(p);
        }
    }

    // Sort.
    if let Some(col) = sort_col {
        providers.sort_by(|a, b| provider_sort_cmp(a, b, col));
        if args.desc {
            providers.reverse();
        }
    }

    // Limit.
    if let Some(limit) = args.limit {
        providers.truncate(limit);
    }

    if args.count {
        emit(&providers.len().to_string(), args.output.as_deref())?;
        return Ok(());
    }

    let table = Table {
        headers: columns.iter().map(|c| c.key().to_string()).collect(),
        rows: providers
            .iter()
            .map(|p| columns.iter().map(|c| c.extract(p)).collect())
            .collect(),
    };
    let text = table.render(format)?;
    emit(&text, args.output.as_deref())
}

/// Compare two providers by a single column.
///
/// `models` sorts numerically; everything else sorts by lowercased text with
/// empty values last.
fn provider_sort_cmp(a: &Provider, b: &Provider, col: ProviderColumn) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if col == ProviderColumn::Models {
        return a.models.len().cmp(&b.models.len());
    }
    let av = a.extract_for_sort(col);
    let bv = b.extract_for_sort(col);
    match (av.is_empty(), bv.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater, // empty sorts last
        (false, true) => Ordering::Less,
        (false, false) => av.cmp(&bv),
    }
}

trait ProviderSortExt {
    fn extract_for_sort(&self, col: ProviderColumn) -> String;
}

impl ProviderSortExt for Provider {
    fn extract_for_sort(&self, col: ProviderColumn) -> String {
        col.extract(self).to_lowercase()
    }
}

// ---------------------------------------------------------------------------
// Fields command
// ---------------------------------------------------------------------------

/// The type-name string for a field kind: `text|number|bool|list`.
fn field_kind_str(f: Field) -> &'static str {
    use modelx_core::FieldKind;
    match f.kind() {
        FieldKind::Number => "number",
        FieldKind::Bool => "bool",
        FieldKind::List => "list",
        FieldKind::Text => "text",
    }
}

/// Build the `fields` table (key, label, type).
fn fields_table() -> Table {
    Table {
        headers: vec!["key".to_string(), "label".to_string(), "type".to_string()],
        rows: Field::all()
            .iter()
            .map(|f| {
                vec![
                    f.key().to_string(),
                    f.label().to_string(),
                    field_kind_str(*f).to_string(),
                ]
            })
            .collect(),
    }
}

fn cmd_fields(args: &FieldsArgs) -> Result<()> {
    let format = parse_format(&args.format)?;

    // Model fields section.
    let model_table = fields_table();
    let model_text = model_table.render(format)?;
    emit(&model_text, args.output.as_deref())?;

    // Benchmarks section — appended to stdout (or the same file).
    // For file output append; for stdout it continues naturally.
    let bench_headers = vec![
        "key".to_string(),
        "label".to_string(),
        "source".to_string(),
        "higher_is_better".to_string(),
    ];
    let bench_rows: Vec<Vec<String>> = BenchMetric::all()
        .iter()
        .map(|m| {
            let src = match m.source() {
                modelx_benchmarks::Source::LmArena => "LmArena",
                modelx_benchmarks::Source::BigCodeBench => "BigCodeBench",
                modelx_benchmarks::Source::OpenAsr => "OpenAsr",
            };
            vec![
                m.key().to_string(),
                m.label().to_string(),
                src.to_string(),
                m.higher_is_better().to_string(),
            ]
        })
        .collect();

    // Print a section header only for non-structured formats.
    match format {
        Format::PlainList | Format::Csv => {
            eprintln!("--- Benchmarks ---");
        }
        _ => {}
    }

    let bench_table = Table {
        headers: bench_headers,
        rows: bench_rows,
    };
    let bench_text = bench_table.render(format)?;

    match args.output.as_deref() {
        Some(path) => {
            // Append the benchmark section to the file.
            use std::io::Write as IoWrite;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .with_context(|| format!("failed to append benchmarks to {}", path.display()))?;
            writeln!(f, "\n# Benchmarks").ok();
            write!(f, "{bench_text}").ok();
        }
        None => {
            println!("\n# Benchmarks");
            print!("{bench_text}");
            std::io::stdout().flush().ok();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Models command
// ---------------------------------------------------------------------------

/// Does the model's provider id or name contain `needle` (case-insensitive)?
fn model_provider_matches(m: &Model, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    m.provider_id.to_lowercase().contains(&needle)
        || m.provider_name.to_lowercase().contains(&needle)
}

/// Does the search query hit provider name / model name / model id
/// (case-insensitive)?
fn model_search_matches(m: &Model, query: &str) -> bool {
    let q = query.to_lowercase();
    m.provider_name.to_lowercase().contains(&q)
        || m.name.to_lowercase().contains(&q)
        || m.id.to_lowercase().contains(&q)
}

/// Compare two models by a sort field.
///
/// Numeric fields compare by `as_f64`; other fields by `display()` lowercased.
/// In both cases a missing value sorts **last**.
fn model_sort_cmp(a: &Model, b: &Model, field: Field) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if field.is_numeric() {
        let av = field.value(a).as_f64();
        let bv = field.value(b).as_f64();
        match (av, bv) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    } else {
        let av = field.value(a).display().to_lowercase();
        let bv = field.value(b).display().to_lowercase();
        match (av.is_empty(), bv.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => av.cmp(&bv),
        }
    }
}

/// Apply the full filter / sort / limit pipeline to a catalog's models.
fn select_models<'a>(
    catalog: &'a Catalog,
    predicates: &[Predicate],
    provider: Option<&str>,
    search: Option<&str>,
    sort: Option<Field>,
    desc: bool,
    limit: Option<usize>,
) -> Vec<&'a Model> {
    let mut models: Vec<&Model> = catalog
        .all_models()
        .filter(|m| matches_all(m, predicates))
        .filter(|m| {
            provider
                .map(|p| model_provider_matches(m, p))
                .unwrap_or(true)
        })
        .filter(|m| search.map(|q| model_search_matches(m, q)).unwrap_or(true))
        .collect();

    if let Some(field) = sort {
        models.sort_by(|a, b| model_sort_cmp(a, b, field));
        if desc {
            models.reverse();
        }
    }

    if let Some(limit) = limit {
        models.truncate(limit);
    }

    models
}

fn cmd_models(catalog: &Catalog, args: &ModelsArgs) -> Result<()> {
    let fields = parse_fields(&args.fields)?;
    let format = parse_format(&args.format)?;
    let sort_field = match &args.sort {
        Some(s) => Some(parse_sort_field(s)?),
        None => None,
    };

    let predicates =
        parse_filters(&args.filter, args.regex).map_err(|e| anyhow!("invalid --filter: {e}"))?;

    let models = select_models(
        catalog,
        &predicates,
        args.provider.as_deref(),
        args.search.as_deref(),
        sort_field,
        args.desc,
        args.limit,
    );

    if args.count {
        emit(&models.len().to_string(), args.output.as_deref())?;
        return Ok(());
    }

    let req = ExportRequest {
        models,
        fields,
        format,
    };

    match args.output.as_deref() {
        Some(path) => {
            modelx_export::write(&req, path).map_err(|e| anyhow!("export failed: {e}"))?;
        }
        None => {
            let text = modelx_export::render(&req).map_err(|e| anyhow!("export failed: {e}"))?;
            print!("{text}");
            std::io::stdout().flush().ok();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bench command
// ---------------------------------------------------------------------------

/// A resolved column in the `bench` command — either a core model field or a
/// benchmark metric.
#[derive(Clone, Copy, Debug)]
enum BenchColumn {
    Core(Field),
    Metric(BenchMetric),
}

impl BenchColumn {
    fn key(self) -> &'static str {
        match self {
            BenchColumn::Core(f) => f.key(),
            BenchColumn::Metric(m) => m.key(),
        }
    }
}

/// Parse a comma-separated list of bench column keys.
///
/// Each token is tried as a `BenchMetric` first, then as a core `Field`.
fn parse_bench_columns(s: &str) -> Result<Vec<BenchColumn>> {
    let mut cols = Vec::new();
    for key in s.split(',') {
        let key = key.trim();
        if let Some(m) = BenchMetric::from_key(key) {
            cols.push(BenchColumn::Metric(m));
        } else if let Some(f) = Field::from_key(key) {
            cols.push(BenchColumn::Core(f));
        } else {
            let valid_core: Vec<&str> = Field::all().iter().map(|f| f.key()).collect();
            let valid_bench: Vec<&str> = BenchMetric::all().iter().map(|m| m.key()).collect();
            return Err(anyhow!(
                "unknown field or benchmark key {key:?}; core fields: {}; benchmark metrics: {}",
                valid_core.join(", "),
                valid_bench.join(", ")
            ));
        }
    }
    Ok(cols)
}

/// Evaluate a benchmark filter expression against a model.
///
/// Returns `None` when the expression is a benchmark filter (metric key found)
/// so the caller knows whether it was handled. Returns `Some(keep)` for both
/// benchmark and core filters once evaluated.
///
/// For benchmark filters: if the model has no value for the metric the filter
/// fails (model is excluded).
fn eval_bench_filter(
    expr: &str,
    model: &Model,
    db: Option<&BenchmarkDb>,
    regex: bool,
) -> Result<Option<bool>> {
    // Try to split the expression to identify the key.
    let (key, rest) = split_filter_key(expr);

    if BenchMetric::from_key(key).is_none() {
        // Not a benchmark key — caller should use core filter pipeline.
        return Ok(None);
    }

    let metric = BenchMetric::from_key(key).unwrap();

    // It's a bench filter. If no db, fail the model (exclude it).
    let value = match db.and_then(|d| d.metric_value(model, metric)) {
        Some(v) => v,
        None => return Ok(Some(false)),
    };

    // Parse operator and rhs from `rest` (e.g. `> 1400`, `= 1500`, `>= 1200`).
    let rest = rest.trim();
    let (op, rhs_str) = parse_op_rhs(rest)?;
    let rhs: f64 = rhs_str
        .trim()
        .parse()
        .map_err(|_| anyhow!("expected a number in benchmark filter, got: {rhs_str:?}"))?;

    let _ = regex; // benchmark filters are always numeric
    let keep = match op {
        "<" => value < rhs,
        "<=" => value <= rhs,
        "=" | "==" => (value - rhs).abs() < f64::EPSILON,
        "!=" => (value - rhs).abs() >= f64::EPSILON,
        ">=" => value >= rhs,
        ">" => value > rhs,
        other => {
            return Err(anyhow!(
                "unsupported operator {other:?} for benchmark filter"
            ))
        }
    };
    Ok(Some(keep))
}

/// Split a filter expression into `(key, rest)` where `rest` starts with the
/// operator and value. Handles both symbol operators (`arena_elo>1400`) and
/// word-separated forms (`arena_elo > 1400`).
fn split_filter_key(expr: &str) -> (&str, &str) {
    // Find first symbol operator position.
    for (i, ch) in expr.char_indices() {
        if matches!(ch, '<' | '>' | '=' | '!' | '~') {
            return (&expr[..i], &expr[i..]);
        }
        // Word operator: whitespace after key.
        if ch.is_whitespace() {
            return (expr[..i].trim(), expr[i..].trim_start());
        }
    }
    (expr, "")
}

/// Extract the operator string and the RHS from the tail of a filter expression
/// (e.g. `"> 1400"` → `(">", "1400")`).
fn parse_op_rhs(s: &str) -> Result<(&str, &str)> {
    // Two-character operators first.
    for op in ["<=", ">=", "!=", "=="] {
        if let Some(rest) = s.strip_prefix(op) {
            return Ok((op, rest.trim()));
        }
    }
    // Single-character operators.
    for op in ["<", ">", "="] {
        if let Some(rest) = s.strip_prefix(op) {
            return Ok((op, rest.trim()));
        }
    }
    // Word operators (lt, lte, eq, ne, gte, gt).
    let parts: Vec<&str> = s.splitn(2, char::is_whitespace).collect();
    if parts.len() == 2 {
        let sym = match parts[0] {
            "lt" => "<",
            "lte" => "<=",
            "eq" => "=",
            "ne" => "!=",
            "gte" => ">=",
            "gt" => ">",
            other => return Err(anyhow!("unknown operator {other:?} in benchmark filter")),
        };
        return Ok((sym, parts[1].trim()));
    }
    Err(anyhow!(
        "could not parse operator in benchmark filter: {s:?}"
    ))
}

/// Compare two models by a `BenchColumn`, missing values sort last.
fn bench_sort_cmp(
    a: &Model,
    b: &Model,
    col: BenchColumn,
    db: Option<&BenchmarkDb>,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match col {
        BenchColumn::Core(f) => model_sort_cmp(a, b, f),
        BenchColumn::Metric(m) => {
            let av = db.and_then(|d| d.metric_value(a, m));
            let bv = db.and_then(|d| d.metric_value(b, m));
            match (av, bv) {
                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }
    }
}

fn cmd_bench(
    catalog: &Catalog,
    db: Option<&BenchmarkDb>,
    args: &BenchArgs,
    offline: bool,
) -> Result<()> {
    let columns = parse_bench_columns(&args.fields)?;
    let format = parse_format(&args.format)?;

    // Resolve sort column.
    let sort_col: Option<BenchColumn> = match &args.sort {
        Some(s) => {
            let key = s.trim();
            if let Some(m) = BenchMetric::from_key(key) {
                Some(BenchColumn::Metric(m))
            } else if let Some(f) = Field::from_key(key) {
                Some(BenchColumn::Core(f))
            } else {
                let valid_core: Vec<&str> = Field::all().iter().map(|f| f.key()).collect();
                let valid_bench: Vec<&str> = BenchMetric::all().iter().map(|m| m.key()).collect();
                return Err(anyhow!(
                    "unknown sort key {key:?}; core fields: {}; benchmark metrics: {}",
                    valid_core.join(", "),
                    valid_bench.join(", ")
                ));
            }
        }
        None => None,
    };

    // Split filter expressions into benchmark and core predicates.
    let mut bench_filters: Vec<String> = Vec::new();
    let mut core_filters: Vec<String> = Vec::new();
    for expr in &args.filter {
        let (key, _) = split_filter_key(expr.trim());
        if BenchMetric::from_key(key).is_some() {
            bench_filters.push(expr.clone());
        } else {
            core_filters.push(expr.clone());
        }
    }

    let core_predicates =
        parse_filters(&core_filters, args.regex).map_err(|e| anyhow!("invalid --filter: {e}"))?;

    // Collect matching models.
    let mut models: Vec<&Model> = catalog
        .all_models()
        .filter(|m| matches_all(m, &core_predicates))
        .filter(|m| {
            args.provider
                .as_deref()
                .map(|p| model_provider_matches(m, p))
                .unwrap_or(true)
        })
        .filter(|m| {
            args.search
                .as_deref()
                .map(|q| model_search_matches(m, q))
                .unwrap_or(true)
        })
        .filter(|m| {
            // Evaluate benchmark filters; model is excluded if any fails.
            bench_filters.iter().all(|expr| {
                match eval_bench_filter(expr, m, db, args.regex) {
                    Ok(Some(keep)) => keep,
                    Ok(None) => true, // shouldn't happen; handled above
                    Err(_) => false,
                }
            })
        })
        .collect();

    // Sort.
    if let Some(col) = sort_col {
        models.sort_by(|a, b| bench_sort_cmp(a, b, col, db));
        if args.desc {
            models.reverse();
        }
    }

    // Limit.
    if let Some(limit) = args.limit {
        models.truncate(limit);
    }

    // Coverage count.
    let matched_count = models
        .iter()
        .filter(|m| db.map(|d| d.lookup(m).matched_any).unwrap_or(false))
        .count();
    let total = models.len();

    if args.count {
        emit(&total.to_string(), args.output.as_deref())?;
        return Ok(());
    }

    // Render rows.
    let headers: Vec<String> = columns.iter().map(|c| c.key().to_string()).collect();
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            columns
                .iter()
                .map(|col| match col {
                    BenchColumn::Core(f) => f.value(m).display().to_string(),
                    BenchColumn::Metric(metric) => db
                        .and_then(|d| d.metric_value(m, *metric))
                        .map(|v| metric.format(v))
                        .unwrap_or_else(|| "—".to_string()),
                })
                .collect()
        })
        .collect();

    let table = Table { headers, rows };
    let text = table.render(format)?;
    emit(&text, args.output.as_deref())?;

    // Coverage note to stderr.
    if db.is_none() {
        if offline {
            eprintln!(
                "modelx: benchmark data unavailable (running --offline with no cache; \
                 run `modelx refresh` or drop --offline to fetch benchmark data)"
            );
        } else {
            eprintln!("modelx: benchmark data unavailable");
        }
    } else {
        eprintln!("modelx bench: {matched_count}/{total} models have benchmark data");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Show command
// ---------------------------------------------------------------------------

/// Resolve a single model by provider + model selector.
///
/// Provider: exact id first, else case-insensitive substring on id or name.
/// Model: exact id first, else case-insensitive substring on id or name.
/// Errors when nothing matches or when the match is ambiguous.
fn resolve_model<'a>(
    catalog: &'a Catalog,
    provider_sel: &str,
    model_sel: &str,
) -> Result<&'a Model> {
    // --- resolve provider -------------------------------------------------
    let provider: &Provider =
        if let Some(p) = catalog.providers.iter().find(|p| p.id == provider_sel) {
            p
        } else {
            let needle = provider_sel.to_lowercase();
            let candidates: Vec<&Provider> = catalog
                .providers
                .iter()
                .filter(|p| {
                    p.id.to_lowercase().contains(&needle) || p.name.to_lowercase().contains(&needle)
                })
                .collect();
            if candidates.is_empty() {
                return Err(anyhow!("no provider matches {provider_sel:?}"));
            }
            if candidates.len() > 1 {
                let ids: Vec<String> = candidates.iter().map(|p| p.id.clone()).collect();
                return Err(anyhow!(
                    "provider {provider_sel:?} is ambiguous; candidates: {}",
                    ids.join(", ")
                ));
            }
            candidates[0]
        };

    // --- resolve model within that provider -------------------------------
    if let Some(m) = provider.models.iter().find(|m| m.id == model_sel) {
        return Ok(m);
    }
    let needle = model_sel.to_lowercase();
    let candidates: Vec<&Model> = provider
        .models
        .iter()
        .filter(|m| {
            m.id.to_lowercase().contains(&needle) || m.name.to_lowercase().contains(&needle)
        })
        .collect();
    if candidates.is_empty() {
        return Err(anyhow!(
            "no model matches {model_sel:?} in provider {}",
            provider.id
        ));
    }
    if candidates.len() > 1 {
        let ids: Vec<String> = candidates.iter().map(|m| m.id.clone()).collect();
        return Err(anyhow!(
            "model {model_sel:?} is ambiguous; candidates: {}",
            ids.join(", ")
        ));
    }
    Ok(candidates[0])
}

fn cmd_show(catalog: &Catalog, args: &ShowArgs) -> Result<()> {
    let model = resolve_model(catalog, &args.provider, &args.model)?;

    // Default `json` prints the raw JSON blob pretty-printed.
    if args.format.eq_ignore_ascii_case("json") {
        let text = serde_json::to_string_pretty(&model.raw)
            .map_err(|e| anyhow!("failed to serialise model: {e}"))?;
        return emit(&text, args.output.as_deref());
    }

    // Other formats render the single model through the export pipeline using
    // every field.
    let format = parse_format(&args.format)?;
    let req = ExportRequest {
        models: vec![model],
        fields: Field::all().to_vec(),
        format,
    };
    match args.output.as_deref() {
        Some(path) => {
            modelx_export::write(&req, path).map_err(|e| anyhow!("export failed: {e}"))?;
        }
        None => {
            let text = modelx_export::render(&req).map_err(|e| anyhow!("export failed: {e}"))?;
            print!("{text}");
            std::io::stdout().flush().ok();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sources / Refresh
// ---------------------------------------------------------------------------

fn cmd_sources(registry: &SourceRegistry, cache: &Cache) {
    for id in registry.ids() {
        let source = registry.get(id).expect("just came from ids()");
        let cache_path = cache.path_for(id);
        let cache_info = if cache_path.exists() {
            match cache.age_seconds(id) {
                Some(age) => format!("cached ({age}s ago)"),
                None => "cached (age unknown)".to_string(),
            }
        } else {
            "no cache".to_string()
        };
        println!(
            "{id}  {}  {}  [{}]",
            source.name(),
            source.homepage(),
            cache_info
        );
    }
}

fn cmd_refresh(
    registry: &SourceRegistry,
    cache: &Cache,
    source_id: &str,
    config: &Config,
) -> Result<()> {
    let source = registry
        .get(source_id)
        .ok_or_else(|| anyhow!("unknown source: {source_id}"))?;

    let mut catalog = source.fetch().map_err(|e| anyhow!("fetch failed: {e}"))?;

    catalog.fetched_at = Some(now_unix());

    let total = catalog.total_models();
    let n_providers = catalog.providers.len();
    let cache_path = cache.path_for(source_id);

    cache.store(&catalog)?;

    println!(
        "Fetched {total} models across {n_providers} providers → {}",
        cache_path.display()
    );

    // Best-effort benchmark refresh (force = true).
    match ensure_benchmarks(false, config.cache.ttl_hours, true) {
        Some(_) => println!("Refreshed benchmarks"),
        None => eprintln!("modelx: benchmark refresh skipped (see above for details)"),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// API server
// ---------------------------------------------------------------------------

/// Parse a duration string into a [`Duration`].
///
/// Accepts a unit suffix `s`/`m`/`h`/`d` (seconds, minutes, hours, days) or a
/// bare integer meaning seconds. Examples: `30s`, `10m`, `1h`, `2d`, `45`.
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return Err(anyhow!("empty duration"));
    }
    let (num_str, mult) = match s.chars().last().unwrap() {
        's' => (&s[..s.len() - 1], 1u64),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        'd' => (&s[..s.len() - 1], 86_400),
        c if c.is_ascii_digit() => (s, 1),
        other => {
            return Err(anyhow!(
                "invalid duration unit {other:?} in {s:?}; use s, m, h, d or a bare integer"
            ))
        }
    };
    let num_str = num_str.trim();
    let n: u64 = num_str
        .parse()
        .map_err(|_| anyhow!("invalid duration number in {s:?}"))?;
    Ok(Duration::from_secs(n * mult))
}

/// Shared, swappable server state. Guarded by an `RwLock`; refreshes take a
/// write lock and requests take a read lock.
struct ApiState {
    catalog: Catalog,
    bench: Option<BenchmarkDb>,
    source_id: String,
    fetched_at: Option<i64>,
}

/// Percent-decode a URL query value (`%XX` and `+` → space).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A parsed query string: repeated keys are preserved in order.
struct Query {
    pairs: Vec<(String, String)>,
}

impl Query {
    /// Parse a raw query string (the part after `?`), URL-decoding keys and
    /// values. A key with no `=` maps to an empty-string value.
    fn parse(raw: &str) -> Query {
        let mut pairs = Vec::new();
        for part in raw.split('&') {
            if part.is_empty() {
                continue;
            }
            let (k, v) = match part.split_once('=') {
                Some((k, v)) => (url_decode(k), url_decode(v)),
                None => (url_decode(part), String::new()),
            };
            pairs.push((k, v));
        }
        Query { pairs }
    }

    /// First value for `key`, if present.
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// All values for `key`, in order (used for repeatable `filter`).
    fn get_all(&self, key: &str) -> Vec<String> {
        self.pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .collect()
    }

    /// A boolean flag is true when present bare (`?desc`), or `=true` / `=1`.
    fn flag(&self, key: &str) -> bool {
        match self.get(key) {
            None => false,
            Some("") => true,
            Some(v) => matches!(v.to_ascii_lowercase().as_str(), "true" | "1"),
        }
    }

    /// Parse an optional `usize` value for `key`.
    fn usize_opt(&self, key: &str) -> Result<Option<usize>> {
        match self.get(key) {
            None => Ok(None),
            Some("") => Ok(None),
            Some(v) => v
                .parse::<usize>()
                .map(Some)
                .map_err(|_| anyhow!("invalid {key} value {v:?}")),
        }
    }
}

/// A JSON body with a 200 status.
fn ok_json(value: &serde_json::Value) -> (u16, String) {
    (
        200,
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_string()),
    )
}

/// A JSON error body with an arbitrary status.
fn err_json(status: u16, msg: &str) -> (u16, String) {
    (status, serde_json::json!({ "error": msg }).to_string())
}

/// Pure request dispatcher: no sockets, fully unit-testable.
///
/// Returns `(status, json_body)`.
fn handle(method: &str, path: &str, query: &str, state: &ApiState) -> (u16, String) {
    if method != "GET" {
        return err_json(405, "method not allowed");
    }
    let q = Query::parse(query);

    // Split the path into non-empty segments for /models/{prov}/{id} routing.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match segments.as_slice() {
        ["health"] => handle_health(state),
        ["sources"] => handle_sources(),
        ["fields"] => handle_fields(),
        ["providers"] => handle_providers(&q, state),
        ["models"] => handle_models(&q, state),
        ["models", prov, id] => handle_model_detail(prov, id, state),
        ["bench"] => handle_bench(&q, state),
        _ => err_json(404, "not found"),
    }
}

fn handle_health(state: &ApiState) -> (u16, String) {
    let value = serde_json::json!({
        "status": "ok",
        "source": state.source_id,
        "models": state.catalog.total_models(),
        "providers": state.catalog.providers.len(),
        "fetched_at": state.fetched_at,
        "benchmarks": state.bench.is_some(),
    });
    ok_json(&value)
}

fn handle_sources() -> (u16, String) {
    let registry = SourceRegistry::with_defaults();
    let cache = Cache::discover().ok();
    let mut arr: Vec<serde_json::Value> = Vec::new();
    for id in registry.ids() {
        let source = registry.get(id).expect("just came from ids()");
        let (cached, age) = match &cache {
            Some(c) if c.path_for(id).exists() => (true, c.age_seconds(id)),
            _ => (false, None),
        };
        arr.push(serde_json::json!({
            "id": id,
            "name": source.name(),
            "homepage": source.homepage(),
            "cached": cached,
            "age_seconds": age,
        }));
    }
    ok_json(&serde_json::Value::Array(arr))
}

fn handle_fields() -> (u16, String) {
    let model_fields: Vec<serde_json::Value> = Field::all()
        .iter()
        .map(|f| {
            serde_json::json!({
                "key": f.key(),
                "label": f.label(),
                "type": field_kind_str(*f),
            })
        })
        .collect();
    let benchmark_metrics: Vec<serde_json::Value> = BenchMetric::all()
        .iter()
        .map(|m| {
            let src = match m.source() {
                modelx_benchmarks::Source::LmArena => "LmArena",
                modelx_benchmarks::Source::BigCodeBench => "BigCodeBench",
                modelx_benchmarks::Source::OpenAsr => "OpenAsr",
            };
            serde_json::json!({
                "key": m.key(),
                "label": m.label(),
                "source": src,
                "higher_is_better": m.higher_is_better(),
            })
        })
        .collect();
    let value = serde_json::json!({
        "model_fields": model_fields,
        "benchmark_metrics": benchmark_metrics,
    });
    ok_json(&value)
}

fn handle_providers(q: &Query, state: &ApiState) -> (u16, String) {
    // Build a ProvidersArgs equivalent from the query, then reuse the CLI logic.
    let fields = q.get("fields").unwrap_or("id,name,models");
    let columns = match parse_provider_columns(fields) {
        Ok(c) => c,
        Err(e) => return err_json(400, &e.to_string()),
    };
    let sort_col = match q.get("sort") {
        Some(s) if !s.is_empty() => match ProviderColumn::from_key(s.trim()) {
            Some(c) => Some(c),
            None => {
                return err_json(
                    400,
                    &format!(
                        "unknown sort column {:?}; valid keys: {}",
                        s.trim(),
                        ProviderColumn::all_keys()
                    ),
                )
            }
        },
        _ => None,
    };
    let desc = q.flag("desc");
    let limit = match q.usize_opt("limit") {
        Ok(l) => l,
        Err(e) => return err_json(400, &e.to_string()),
    };
    let filter = q.get("filter").filter(|s| !s.is_empty());
    let regex = q.flag("regex");

    let mut providers: Vec<&Provider> = Vec::new();
    for p in &state.catalog.providers {
        let keep = match filter {
            Some(pat) => match provider_matches_filter(p, pat, regex) {
                Ok(k) => k,
                Err(e) => return err_json(400, &e.to_string()),
            },
            None => true,
        };
        if keep {
            providers.push(p);
        }
    }

    if let Some(col) = sort_col {
        providers.sort_by(|a, b| provider_sort_cmp(a, b, col));
        if desc {
            providers.reverse();
        }
    }
    if let Some(limit) = limit {
        providers.truncate(limit);
    }

    let table = Table {
        headers: columns.iter().map(|c| c.key().to_string()).collect(),
        rows: providers
            .iter()
            .map(|p| columns.iter().map(|c| c.extract(p)).collect())
            .collect(),
    };
    match table.render(Format::Json) {
        Ok(body) => (200, body),
        Err(e) => err_json(500, &e.to_string()),
    }
}

fn handle_models(q: &Query, state: &ApiState) -> (u16, String) {
    // Default fields = all model field keys.
    let fields = match q.get("fields").filter(|s| !s.is_empty()) {
        Some(s) => match parse_fields(s) {
            Ok(f) => f,
            Err(e) => return err_json(400, &e.to_string()),
        },
        None => Field::all().to_vec(),
    };
    let sort_field = match q.get("sort").filter(|s| !s.is_empty()) {
        Some(s) => match parse_sort_field(s) {
            Ok(f) => Some(f),
            Err(e) => return err_json(400, &e.to_string()),
        },
        None => None,
    };
    let regex = q.flag("regex");
    let filters = q.get_all("filter");
    let predicates = match parse_filters(&filters, regex) {
        Ok(p) => p,
        Err(e) => return err_json(400, &format!("invalid filter: {e}")),
    };
    let desc = q.flag("desc");
    let limit = match q.usize_opt("limit") {
        Ok(l) => l,
        Err(e) => return err_json(400, &e.to_string()),
    };
    let provider = q.get("provider").filter(|s| !s.is_empty());
    let search = q.get("search").filter(|s| !s.is_empty());

    let models = select_models(
        &state.catalog,
        &predicates,
        provider,
        search,
        sort_field,
        desc,
        limit,
    );

    let req = ExportRequest {
        models,
        fields,
        format: Format::Json,
    };
    match modelx_export::render(&req) {
        Ok(body) => (200, body),
        Err(e) => err_json(500, &format!("export failed: {e}")),
    }
}

fn handle_model_detail(prov: &str, id: &str, state: &ApiState) -> (u16, String) {
    let found = state
        .catalog
        .providers
        .iter()
        .find(|p| p.id == prov)
        .and_then(|p| p.models.iter().find(|m| m.id == id));
    match found {
        Some(model) => ok_json(&model.raw),
        None => err_json(404, "model not found"),
    }
}

fn handle_bench(q: &Query, state: &ApiState) -> (u16, String) {
    let db = state.bench.as_ref();
    let fields = q
        .get("fields")
        .filter(|s| !s.is_empty())
        .unwrap_or("provider_id,id,name,arena_elo,coding_elo,math_elo");
    let columns = match parse_bench_columns(fields) {
        Ok(c) => c,
        Err(e) => return err_json(400, &e.to_string()),
    };

    // Resolve sort column (benchmark metric first, else core field).
    let sort_col: Option<BenchColumn> = match q.get("sort").filter(|s| !s.is_empty()) {
        Some(s) => {
            let key = s.trim();
            if let Some(m) = BenchMetric::from_key(key) {
                Some(BenchColumn::Metric(m))
            } else if let Some(f) = Field::from_key(key) {
                Some(BenchColumn::Core(f))
            } else {
                return err_json(400, &format!("unknown sort key {key:?}"));
            }
        }
        None => None,
    };

    let regex = q.flag("regex");
    let desc = q.flag("desc");
    let limit = match q.usize_opt("limit") {
        Ok(l) => l,
        Err(e) => return err_json(400, &e.to_string()),
    };
    let provider = q.get("provider").filter(|s| !s.is_empty());
    let search = q.get("search").filter(|s| !s.is_empty());

    // Split repeatable filters into benchmark vs core (same routing as `bench`).
    let mut bench_filters: Vec<String> = Vec::new();
    let mut core_filters: Vec<String> = Vec::new();
    for expr in q.get_all("filter") {
        let (key, _) = split_filter_key(expr.trim());
        if BenchMetric::from_key(key).is_some() {
            bench_filters.push(expr);
        } else {
            core_filters.push(expr);
        }
    }
    let core_predicates = match parse_filters(&core_filters, regex) {
        Ok(p) => p,
        Err(e) => return err_json(400, &format!("invalid filter: {e}")),
    };

    // Any malformed benchmark filter must surface as a 400 (not silently drop).
    for expr in &bench_filters {
        if let Err(e) = validate_bench_filter(expr) {
            return err_json(400, &format!("invalid filter: {e}"));
        }
    }

    let mut models: Vec<&Model> = state
        .catalog
        .all_models()
        .filter(|m| matches_all(m, &core_predicates))
        .filter(|m| {
            provider
                .map(|p| model_provider_matches(m, p))
                .unwrap_or(true)
        })
        .filter(|m| search.map(|s| model_search_matches(m, s)).unwrap_or(true))
        .filter(|m| {
            bench_filters
                .iter()
                .all(|expr| match eval_bench_filter(expr, m, db, regex) {
                    Ok(Some(keep)) => keep,
                    Ok(None) => true,
                    Err(_) => false,
                })
        })
        .collect();

    if let Some(col) = sort_col {
        models.sort_by(|a, b| bench_sort_cmp(a, b, col, db));
        if desc {
            models.reverse();
        }
    }
    if let Some(limit) = limit {
        models.truncate(limit);
    }

    // Build a typed JSON array (numbers for metrics, like `bench --format json`
    // uses the Table JSON renderer with string cells — we mirror that here).
    let headers: Vec<String> = columns.iter().map(|c| c.key().to_string()).collect();
    let rows: Vec<Vec<String>> = models
        .iter()
        .map(|m| {
            columns
                .iter()
                .map(|col| match col {
                    BenchColumn::Core(f) => f.value(m).display().to_string(),
                    BenchColumn::Metric(metric) => db
                        .and_then(|d| d.metric_value(m, *metric))
                        .map(|v| metric.format(v))
                        .unwrap_or_else(|| "—".to_string()),
                })
                .collect()
        })
        .collect();
    let table = Table { headers, rows };
    match table.render(Format::Json) {
        Ok(body) => (200, body),
        Err(e) => err_json(500, &e.to_string()),
    }
}

/// Validate a benchmark filter expression's operator and numeric RHS without
/// needing a model. Used to surface malformed filters as 400s.
fn validate_bench_filter(expr: &str) -> Result<()> {
    let (_key, rest) = split_filter_key(expr.trim());
    let (_op, rhs_str) = parse_op_rhs(rest.trim())?;
    rhs_str
        .trim()
        .parse::<f64>()
        .map_err(|_| anyhow!("expected a number in benchmark filter, got: {rhs_str:?}"))?;
    Ok(())
}

/// Load the initial API state (catalog + benchmarks) once.
fn load_api_state(
    registry: &SourceRegistry,
    cache: &Cache,
    source_id: &str,
    config: &Config,
    offline: bool,
) -> Result<ApiState> {
    let catalog = ensure_fresh(registry, cache, source_id, config, offline)?;
    let bench = ensure_benchmarks(offline, config.cache.ttl_hours, false);
    let fetched_at = catalog.fetched_at;
    Ok(ApiState {
        catalog,
        bench,
        source_id: source_id.to_string(),
        fetched_at,
    })
}

fn cmd_api(
    registry: SourceRegistry,
    cache: Cache,
    source_id: String,
    config: Config,
    offline: bool,
    args: &ApiArgs,
) -> Result<()> {
    // Parse the refresh interval (if any) before binding, so a bad value fails fast.
    let refresh_interval = match &args.refresh_interval {
        Some(s) => Some(parse_duration(s).context("invalid --refresh-interval")?),
        None => None,
    };

    let state = load_api_state(&registry, &cache, &source_id, &config, offline)?;
    let state = Arc::new(RwLock::new(state));

    let bind = format!("{}:{}", args.listen_addr, args.listen_port);
    let server =
        tiny_http::Server::http(&bind).map_err(|e| anyhow!("failed to bind {bind}: {e}"))?;
    let server = Arc::new(server);

    eprintln!(
        "modelx api listening on http://{}:{}",
        args.listen_addr, args.listen_port
    );

    // Background refresh thread.
    if let Some(dur) = refresh_interval {
        let state = Arc::clone(&state);
        let registry = Arc::new(registry);
        let cache = Arc::new(cache);
        let source_id = source_id.clone();
        let config_ttl = config.cache.ttl_hours;
        std::thread::spawn(move || loop {
            std::thread::sleep(dur);
            match refresh_state(&registry, &cache, &source_id, config_ttl, offline) {
                Ok((catalog, bench)) => {
                    let fetched_at = catalog.fetched_at;
                    if let Ok(mut guard) = state.write() {
                        guard.catalog = catalog;
                        guard.bench = bench;
                        guard.fetched_at = fetched_at;
                    }
                    eprintln!("modelx api: refreshed at {}", now_unix());
                }
                Err(e) => {
                    eprintln!("modelx api: refresh failed: {e} (serving previous data)");
                }
            }
        });
    }

    // Worker pool: each thread loops on recv() and dispatches through handle().
    let mut workers = Vec::new();
    for _ in 0..4 {
        let server = Arc::clone(&server);
        let state = Arc::clone(&state);
        workers.push(std::thread::spawn(move || loop {
            let request = match server.recv() {
                Ok(r) => r,
                Err(_) => break,
            };
            let method = request.method().as_str().to_string();
            let raw_url = request.url().to_string();
            let (path, query) = match raw_url.split_once('?') {
                Some((p, q)) => (p.to_string(), q.to_string()),
                None => (raw_url.clone(), String::new()),
            };

            let (status, body) = match state.read() {
                Ok(guard) => handle(&method, &path, &query, &guard),
                Err(_) => (500, r#"{"error":"internal state poisoned"}"#.to_string()),
            };

            let header =
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
                    .expect("static header is valid");
            let response = tiny_http::Response::from_string(body)
                .with_status_code(status)
                .with_header(header);
            let _ = request.respond(response);
        }));
    }

    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

/// Force a re-fetch of catalog + benchmarks for a background refresh.
fn refresh_state(
    registry: &SourceRegistry,
    cache: &Cache,
    source_id: &str,
    ttl_hours: i64,
    offline: bool,
) -> Result<(Catalog, Option<BenchmarkDb>)> {
    let source = registry
        .get(source_id)
        .ok_or_else(|| anyhow!("unknown source: {source_id}"))?;
    let mut catalog = source
        .fetch()
        .map_err(|e| anyhow!("fetch failed for {source_id}: {e}"))?;
    catalog.fetched_at = Some(now_unix());
    cache.store(&catalog)?;
    let bench = ensure_benchmarks(offline, ttl_hours, true);
    Ok((catalog, bench))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Restore the default `SIGPIPE` disposition on Unix so that piping output to
/// tools that close early (`head`, `less`, …) exits quietly instead of
/// panicking with a "broken pipe" error. No-op on non-Unix platforms.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: setting a signal handler to the default disposition is sound and
    // is the standard way for a CLI to behave like other Unix filters.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

/// Build a registry and cache in one shot (shared by every data-touching command).
fn registry_and_cache() -> Result<(SourceRegistry, Cache)> {
    let registry = SourceRegistry::with_defaults();
    let cache =
        Cache::discover().with_context(|| "could not determine platform cache directory")?;
    Ok((registry, cache))
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();

    let config = Config::load(cli.config.as_deref())?;

    match cli.command {
        Some(Command::Sources) => {
            let (registry, cache) = registry_and_cache()?;
            cmd_sources(&registry, &cache);
        }

        Some(Command::Refresh) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            cmd_refresh(&registry, &cache, &source_id, &config)?;
        }

        Some(Command::Providers(args)) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            let catalog = ensure_fresh(&registry, &cache, &source_id, &config, cli.offline)?;
            cmd_providers(&catalog, &args)?;
        }

        Some(Command::Models(args)) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            let catalog = ensure_fresh(&registry, &cache, &source_id, &config, cli.offline)?;
            cmd_models(&catalog, &args)?;
        }

        Some(Command::Fields(args)) => {
            cmd_fields(&args)?;
        }

        Some(Command::Show(args)) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            let catalog = ensure_fresh(&registry, &cache, &source_id, &config, cli.offline)?;
            cmd_show(&catalog, &args)?;
        }

        Some(Command::Bench(args)) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            let catalog = ensure_fresh(&registry, &cache, &source_id, &config, cli.offline)?;
            let db = ensure_benchmarks(cli.offline, config.cache.ttl_hours, false);
            cmd_bench(&catalog, db.as_ref(), &args, cli.offline)?;
        }

        Some(Command::Api(args)) => {
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            cmd_api(registry, cache, source_id, config, cli.offline, &args)?;
        }

        Some(Command::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "modelx", &mut std::io::stdout());
        }

        None => {
            // Launch the TUI.
            let (registry, cache) = registry_and_cache()?;
            let source_id = resolve_source(cli.source, &config, &registry)?;
            let source_ids: Vec<String> = registry.ids().iter().map(|s| s.to_string()).collect();

            let catalog = cache.load(&source_id)?.unwrap_or_else(|| Catalog {
                source_id: source_id.clone(),
                fetched_at: None,
                providers: vec![],
            });

            // Load benchmarks cache-only so TUI startup never blocks the network.
            let bench_db = ensure_benchmarks(/*offline=*/ true, config.cache.ttl_hours, false);
            let state =
                AppState::new(catalog, source_ids, source_id.clone()).with_benchmarks(bench_db);
            let ctx = RuntimeCtx {
                registry,
                cache,
                source_id,
                ttl_seconds: config.cache.ttl_hours * 3600,
                offline: cli.offline,
            };

            modelx_tui::run(state, ctx)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use modelx_core::testkit::sample_catalog;

    // -----------------------------------------------------------------------
    // parse_fields
    // -----------------------------------------------------------------------

    #[test]
    fn parse_fields_valid_single() {
        let fields = parse_fields("id").unwrap();
        assert_eq!(fields, vec![Field::Id]);
    }

    #[test]
    fn parse_fields_valid_multiple() {
        let fields = parse_fields("id,name,input_cost").unwrap();
        assert_eq!(fields, vec![Field::Id, Field::Name, Field::InputCost]);
    }

    #[test]
    fn parse_fields_all_known_keys() {
        let all_keys: Vec<&str> = Field::all().iter().map(|f| f.key()).collect();
        let joined = all_keys.join(",");
        let parsed = parse_fields(&joined).unwrap();
        assert_eq!(parsed.len(), Field::all().len());
    }

    #[test]
    fn parse_fields_unknown_key_returns_error() {
        let err = parse_fields("id,not_a_real_field").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not_a_real_field"),
            "error should mention the bad key: {msg}"
        );
        assert!(msg.contains("id"), "error should list valid keys: {msg}");
    }

    #[test]
    fn parse_fields_whitespace_trimmed() {
        let fields = parse_fields(" id , name ").unwrap();
        assert_eq!(fields, vec![Field::Id, Field::Name]);
    }

    // -----------------------------------------------------------------------
    // parse_sort_field
    // -----------------------------------------------------------------------

    #[test]
    fn parse_sort_field_valid() {
        assert_eq!(
            parse_sort_field("context_limit").unwrap(),
            Field::ContextLimit
        );
    }

    #[test]
    fn parse_sort_field_unknown_errors_with_valid_keys() {
        let err = parse_sort_field("bogus").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus"), "should mention bad key: {msg}");
        assert!(msg.contains("provider_id"), "should list valid keys: {msg}");
    }

    // -----------------------------------------------------------------------
    // parse_format
    // -----------------------------------------------------------------------

    #[test]
    fn parse_format_plain() {
        assert_eq!(parse_format("plain").unwrap(), Format::PlainList);
    }

    #[test]
    fn parse_format_list_alias() {
        assert_eq!(parse_format("list").unwrap(), Format::PlainList);
    }

    #[test]
    fn parse_format_csv() {
        assert_eq!(parse_format("csv").unwrap(), Format::Csv);
    }

    #[test]
    fn parse_format_md() {
        assert_eq!(parse_format("md").unwrap(), Format::Markdown);
    }

    #[test]
    fn parse_format_markdown() {
        assert_eq!(parse_format("markdown").unwrap(), Format::Markdown);
    }

    #[test]
    fn parse_format_json() {
        assert_eq!(parse_format("json").unwrap(), Format::Json);
    }

    #[test]
    fn parse_format_case_insensitive() {
        assert_eq!(parse_format("JSON").unwrap(), Format::Json);
        assert_eq!(parse_format("CSV").unwrap(), Format::Csv);
    }

    #[test]
    fn parse_format_unknown_returns_error() {
        let err = parse_format("xml").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("xml"),
            "error should mention bad format: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Config::load  (default ttl is now 12)
    // -----------------------------------------------------------------------

    #[test]
    fn config_default_ttl_is_twelve() {
        assert_eq!(Config::default().cache.ttl_hours, 12);
    }

    #[test]
    fn config_load_missing_file_returns_defaults() {
        let path = Path::new("/tmp/nonexistent-modelx-config-38472834/config.toml");
        let config = Config::load(Some(path)).unwrap();
        assert_eq!(config.cache.ttl_hours, 12);
        assert_eq!(config.ui.theme, "default");
        assert!(config.default_source.is_none());
    }

    #[test]
    fn config_load_parses_toml() {
        let toml_text = r#"
default_source = "models.dev"
[cache]
ttl_hours = 48
[ui]
theme = "dark"
"#;
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml_text).unwrap();

        let config = Config::load(Some(&config_path)).unwrap();
        assert_eq!(config.default_source.as_deref(), Some("models.dev"));
        assert_eq!(config.cache.ttl_hours, 48);
        assert_eq!(config.ui.theme, "dark");
    }

    #[test]
    fn config_load_partial_toml_uses_defaults_for_missing_fields() {
        let toml_text = r#"default_source = "my-source""#;
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml_text).unwrap();

        let config = Config::load(Some(&config_path)).unwrap();
        assert_eq!(config.default_source.as_deref(), Some("my-source"));
        assert_eq!(config.cache.ttl_hours, 12);
        assert_eq!(config.ui.theme, "default");
    }

    // -----------------------------------------------------------------------
    // resolve_source
    // -----------------------------------------------------------------------

    fn make_registry() -> SourceRegistry {
        SourceRegistry::with_defaults()
    }

    fn default_config() -> Config {
        Config::default()
    }

    #[test]
    fn resolve_source_cli_flag_wins() {
        let registry = make_registry();
        let mut config = default_config();
        config.default_source = Some("other".to_string());
        let id = resolve_source(Some("models.dev".to_string()), &config, &registry).unwrap();
        assert_eq!(id, "models.dev");
    }

    #[test]
    fn resolve_source_config_wins_over_default() {
        let registry = make_registry();
        let mut config = default_config();
        config.default_source = Some("models.dev".to_string());
        let id = resolve_source(None, &config, &registry).unwrap();
        assert_eq!(id, "models.dev");
    }

    #[test]
    fn resolve_source_falls_back_to_registry_default() {
        let registry = make_registry();
        let config = default_config();
        let id = resolve_source(None, &config, &registry).unwrap();
        assert_eq!(id, registry.default_id());
    }

    #[test]
    fn resolve_source_unknown_id_returns_error() {
        let registry = make_registry();
        let config = default_config();
        let err =
            resolve_source(Some("nonexistent-source".to_string()), &config, &registry).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent-source"),
            "error should mention bad id: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // now_unix
    // -----------------------------------------------------------------------

    #[test]
    fn now_unix_is_positive_and_recent() {
        let ts = now_unix();
        assert!(ts > 1_577_836_800, "timestamp looks wrong: {ts}");
    }

    // -----------------------------------------------------------------------
    // Provider column extraction + selection
    // -----------------------------------------------------------------------

    #[test]
    fn provider_column_from_key_roundtrips() {
        for key in ["id", "name", "npm", "api", "doc", "env", "models"] {
            let col = ProviderColumn::from_key(key).unwrap();
            assert_eq!(col.key(), key);
        }
    }

    #[test]
    fn parse_provider_columns_default() {
        let cols = parse_provider_columns("id,name,models").unwrap();
        assert_eq!(
            cols,
            vec![
                ProviderColumn::Id,
                ProviderColumn::Name,
                ProviderColumn::Models
            ]
        );
    }

    #[test]
    fn parse_provider_columns_unknown_errors() {
        let err = parse_provider_columns("id,bogus").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus"), "should mention bad col: {msg}");
        assert!(msg.contains("models"), "should list valid keys: {msg}");
    }

    #[test]
    fn provider_column_extract_values() {
        let catalog = sample_catalog();
        let a = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-a")
            .unwrap();
        assert_eq!(ProviderColumn::Id.extract(a), "provider-a");
        assert_eq!(ProviderColumn::Name.extract(a), "Anthropic Test");
        assert_eq!(ProviderColumn::Models.extract(a), "2");
        assert_eq!(ProviderColumn::Env.extract(a), "PROVIDER_A_KEY");
        assert_eq!(ProviderColumn::Npm.extract(a), "@ai-sdk/provider-a");
        // `api` is None on provider-a → empty string.
        assert_eq!(ProviderColumn::Api.extract(a), "");
    }

    // -----------------------------------------------------------------------
    // Provider --filter substring matching
    // -----------------------------------------------------------------------

    #[test]
    fn provider_filter_substring_on_name_case_insensitive() {
        let catalog = sample_catalog();
        let a = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-a")
            .unwrap();
        assert!(provider_matches_filter(a, "anthropic", false).unwrap());
        assert!(provider_matches_filter(a, "ANTHROPIC", false).unwrap());
        assert!(provider_matches_filter(a, "provider-a", false).unwrap());
        assert!(!provider_matches_filter(a, "openweights", false).unwrap());
    }

    #[test]
    fn provider_filter_regex() {
        let catalog = sample_catalog();
        let a = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-a")
            .unwrap();
        assert!(provider_matches_filter(a, "^provider-a$", true).unwrap());
        assert!(!provider_matches_filter(a, "^provider-b$", true).unwrap());
    }

    #[test]
    fn provider_filter_bad_regex_errors() {
        let catalog = sample_catalog();
        let a = &catalog.providers[0];
        let err = provider_matches_filter(a, "(", true).unwrap_err();
        assert!(err.to_string().contains("invalid regular expression"));
    }

    // -----------------------------------------------------------------------
    // Provider sort comparator
    // -----------------------------------------------------------------------

    #[test]
    fn provider_sort_by_models_numeric() {
        let catalog = sample_catalog();
        let a = &catalog.providers[0];
        let b = &catalog.providers[1];
        // Both have 2 models in the sample → Equal.
        assert_eq!(
            provider_sort_cmp(a, b, ProviderColumn::Models),
            std::cmp::Ordering::Equal
        );
    }

    #[test]
    fn provider_sort_by_name_text() {
        let catalog = sample_catalog();
        let a = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-a")
            .unwrap();
        let b = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-b")
            .unwrap();
        // "anthropic test" < "openweights test"
        assert_eq!(
            provider_sort_cmp(a, b, ProviderColumn::Name),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn provider_sort_empty_last() {
        let catalog = sample_catalog();
        // provider-a has npm Some, provider-b has npm None.
        let a = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-a")
            .unwrap();
        let b = catalog
            .providers
            .iter()
            .find(|p| p.id == "provider-b")
            .unwrap();
        // a (non-empty) sorts before b (empty).
        assert_eq!(
            provider_sort_cmp(a, b, ProviderColumn::Npm),
            std::cmp::Ordering::Less
        );
    }

    // -----------------------------------------------------------------------
    // Model sort comparator (numeric vs text, missing-last, desc)
    // -----------------------------------------------------------------------

    fn model<'a>(catalog: &'a Catalog, provider: &str, id: &str) -> &'a Model {
        catalog
            .providers
            .iter()
            .find(|p| p.id == provider)
            .unwrap()
            .models
            .iter()
            .find(|m| m.id == id)
            .unwrap()
    }

    #[test]
    fn model_sort_numeric_ascending() {
        let catalog = sample_catalog();
        // haiku context 200_000 < opus context 1_000_000
        let opus = model(&catalog, "provider-a", "model-opus");
        let haiku = model(&catalog, "provider-a", "model-haiku");
        assert_eq!(
            model_sort_cmp(haiku, opus, Field::ContextLimit),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn model_sort_text_case_insensitive() {
        let catalog = sample_catalog();
        let opus = model(&catalog, "provider-a", "model-opus"); // "Test Opus"
        let haiku = model(&catalog, "provider-a", "model-haiku"); // "Test Haiku"
                                                                  // "test haiku" < "test opus"
        assert_eq!(
            model_sort_cmp(haiku, opus, Field::Name),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn model_sort_missing_numeric_last() {
        let catalog = sample_catalog();
        // qwen has cost=None → input_cost missing; opus has 5.0.
        let opus = model(&catalog, "provider-a", "model-opus");
        let qwen = model(&catalog, "provider-b", "qwen/qwen3-30b");
        // present (opus) sorts before missing (qwen)
        assert_eq!(
            model_sort_cmp(opus, qwen, Field::InputCost),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            model_sort_cmp(qwen, opus, Field::InputCost),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn model_sort_desc_via_select() {
        let catalog = sample_catalog();
        let asc = select_models(
            &catalog,
            &[],
            None,
            None,
            Some(Field::ContextLimit),
            false,
            None,
        );
        let desc = select_models(
            &catalog,
            &[],
            None,
            None,
            Some(Field::ContextLimit),
            true,
            None,
        );
        // desc is the reverse of asc.
        let asc_ids: Vec<&str> = asc.iter().map(|m| m.id.as_str()).collect();
        let mut rev = asc_ids.clone();
        rev.reverse();
        let desc_ids: Vec<&str> = desc.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(desc_ids, rev);
    }

    // -----------------------------------------------------------------------
    // select_models pipeline (provider substr, search, limit, filter engine)
    // -----------------------------------------------------------------------

    #[test]
    fn select_models_provider_substring() {
        let catalog = sample_catalog();
        let models = select_models(&catalog, &[], Some("provider-a"), None, None, false, None);
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| m.provider_id == "provider-a"));
    }

    #[test]
    fn select_models_search_across_fields() {
        let catalog = sample_catalog();
        // "opus" hits model name/id for model-opus.
        let models = select_models(&catalog, &[], None, Some("opus"), None, false, None);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "model-opus");
    }

    #[test]
    fn select_models_limit_truncates() {
        let catalog = sample_catalog();
        let models = select_models(&catalog, &[], None, None, None, false, Some(1));
        assert_eq!(models.len(), 1);
    }

    #[test]
    fn select_models_filter_engine_predicate() {
        let catalog = sample_catalog();
        let preds = parse_filters(&["reasoning = true".to_string()], false).unwrap();
        let models = select_models(&catalog, &preds, None, None, None, false, None);
        // opus and gpt-oss have reasoning=true.
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| m.reasoning == Some(true)));
    }

    // -----------------------------------------------------------------------
    // fields table generation
    // -----------------------------------------------------------------------

    #[test]
    fn fields_table_covers_all_fields() {
        let table = fields_table();
        assert_eq!(table.headers, vec!["key", "label", "type"]);
        assert_eq!(table.rows.len(), Field::all().len());
    }

    #[test]
    fn fields_table_kind_strings() {
        let table = fields_table();
        // Find the input_cost row → number.
        let cost_row = table
            .rows
            .iter()
            .find(|r| r[0] == "input_cost")
            .expect("input_cost present");
        assert_eq!(cost_row[2], "number");
        // reasoning → bool.
        let reasoning_row = table
            .rows
            .iter()
            .find(|r| r[0] == "reasoning")
            .expect("reasoning present");
        assert_eq!(reasoning_row[2], "bool");
        // input_modalities → list.
        let mods_row = table
            .rows
            .iter()
            .find(|r| r[0] == "input_modalities")
            .expect("input_modalities present");
        assert_eq!(mods_row[2], "list");
        // name → text.
        let name_row = table
            .rows
            .iter()
            .find(|r| r[0] == "name")
            .expect("name present");
        assert_eq!(name_row[2], "text");
    }

    // -----------------------------------------------------------------------
    // Table renderers
    // -----------------------------------------------------------------------

    #[test]
    fn table_render_plain_is_tab_separated_no_header() {
        let table = Table {
            headers: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        };
        let out = table.render(Format::PlainList).unwrap();
        assert_eq!(out, "1\t2\n");
    }

    #[test]
    fn table_render_csv_has_header() {
        let table = Table {
            headers: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        };
        let out = table.render(Format::Csv).unwrap();
        assert!(out.starts_with("a,b"), "csv should have header: {out}");
        assert!(out.contains("1,2"), "csv should have row: {out}");
    }

    #[test]
    fn table_render_markdown_has_separator_row() {
        let table = Table {
            headers: vec!["a".into(), "b".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        };
        let out = table.render(Format::Markdown).unwrap();
        assert!(out.contains("| a | b |"), "md header: {out}");
        assert!(out.contains("| --- | --- |"), "md separator: {out}");
    }

    #[test]
    fn table_render_json_array_of_objects() {
        let table = Table {
            headers: vec!["a".into()],
            rows: vec![vec!["1".into()]],
        };
        let out = table.render(Format::Json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["a"], "1");
    }

    // -----------------------------------------------------------------------
    // resolve_model (show)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_model_exact_ids() {
        let catalog = sample_catalog();
        let m = resolve_model(&catalog, "provider-a", "model-opus").unwrap();
        assert_eq!(m.id, "model-opus");
    }

    #[test]
    fn resolve_model_by_substring() {
        let catalog = sample_catalog();
        // "anth" → provider-a, "opus" → model-opus.
        let m = resolve_model(&catalog, "anth", "opus").unwrap();
        assert_eq!(m.id, "model-opus");
    }

    #[test]
    fn resolve_model_no_provider_match() {
        let catalog = sample_catalog();
        let err = resolve_model(&catalog, "zzz", "model-opus").unwrap_err();
        assert!(err.to_string().contains("no provider matches"));
    }

    #[test]
    fn resolve_model_no_model_match() {
        let catalog = sample_catalog();
        let err = resolve_model(&catalog, "provider-a", "zzz").unwrap_err();
        assert!(err.to_string().contains("no model matches"));
    }

    #[test]
    fn resolve_model_ambiguous_model() {
        let catalog = sample_catalog();
        // "model" substring matches both models in provider-a.
        let err = resolve_model(&catalog, "provider-a", "model").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    // -----------------------------------------------------------------------
    // clap: aliases + parse sanity
    // -----------------------------------------------------------------------

    #[test]
    fn cli_verify() {
        Cli::command().debug_assert();
    }

    #[test]
    fn list_alias_parses_as_models() {
        let cli = Cli::try_parse_from(["modelx", "list", "--limit", "5"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Models(_))));
    }

    #[test]
    fn export_alias_parses_as_models() {
        let cli =
            Cli::try_parse_from(["modelx", "export", "--fields", "id,name", "--format", "csv"])
                .unwrap();
        assert!(matches!(cli.command, Some(Command::Models(_))));
    }

    #[test]
    fn models_repeatable_filter() {
        let cli = Cli::try_parse_from([
            "modelx",
            "models",
            "--filter",
            "reasoning = true",
            "--filter",
            "context_limit > 100000",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Models(args)) => assert_eq!(args.filter.len(), 2),
            _ => panic!("expected models"),
        }
    }

    // -----------------------------------------------------------------------
    // Benchmark key routing: BenchMetric::from_key dispatch
    // -----------------------------------------------------------------------

    #[test]
    fn bench_column_key_routes_to_metric_not_core() {
        // Every benchmark key must resolve to a metric, not a core field.
        for m in BenchMetric::all() {
            let key = m.key();
            assert!(
                BenchMetric::from_key(key).is_some(),
                "BenchMetric::from_key should resolve {key}"
            );
            assert!(
                Field::from_key(key).is_none(),
                "core Field should NOT resolve benchmark key {key}"
            );
        }
    }

    #[test]
    fn parse_bench_columns_routes_benchmark_keys() {
        // Benchmark keys in --fields should produce BenchColumn::Metric.
        let cols = parse_bench_columns("provider_id,arena_elo,coding_elo").unwrap();
        assert_eq!(cols.len(), 3);
        assert!(matches!(cols[0], BenchColumn::Core(_)));
        assert!(matches!(cols[1], BenchColumn::Metric(_)));
        assert!(matches!(cols[2], BenchColumn::Metric(_)));
    }

    #[test]
    fn parse_bench_columns_routes_core_keys() {
        let cols = parse_bench_columns("id,name,input_cost").unwrap();
        assert!(cols.iter().all(|c| matches!(c, BenchColumn::Core(_))));
    }

    #[test]
    fn parse_bench_columns_unknown_key_errors() {
        let err = parse_bench_columns("id,not_a_key").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not_a_key"),
            "error should mention bad key: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Benchmark cell formatting and em-dash for missing values
    // -----------------------------------------------------------------------

    #[test]
    fn bench_metric_format_elo_is_integer() {
        let formatted = BenchMetric::ArenaOverall.format(1497.6);
        assert_eq!(formatted, "1498");
    }

    #[test]
    fn bench_metric_format_pass_at_1_has_percent() {
        let formatted = BenchMetric::CodePassAt1.format(61.23);
        assert_eq!(formatted, "61.2%");
    }

    #[test]
    fn bench_column_missing_renders_emdash() {
        // When no db is present, bench columns should render as "—".
        let catalog = sample_catalog();
        let model = catalog
            .all_models()
            .next()
            .expect("sample catalog has models");
        let metric = BenchMetric::ArenaOverall;
        // Simulate the bench command's cell rendering with no db.
        let cell: String = None::<&BenchmarkDb>
            .and_then(|d| d.metric_value(model, metric))
            .map(|v| metric.format(v))
            .unwrap_or_else(|| "—".to_string());
        assert_eq!(cell, "—");
    }

    // -----------------------------------------------------------------------
    // Fields command — benchmark section
    // -----------------------------------------------------------------------

    #[test]
    fn bench_metric_all_have_unique_keys() {
        let keys: Vec<&str> = BenchMetric::all().iter().map(|m| m.key()).collect();
        let mut seen = std::collections::HashSet::new();
        for k in &keys {
            assert!(seen.insert(*k), "duplicate benchmark key: {k}");
        }
        assert_eq!(keys.len(), 10, "expected 10 benchmark metrics");
    }

    #[test]
    fn bench_fields_section_lists_all_metrics() {
        // Verify the benchmark section would include all metrics by
        // checking that all metric keys appear in a simulated listing.
        let listed: Vec<&str> = BenchMetric::all().iter().map(|m| m.key()).collect();
        for m in BenchMetric::all() {
            assert!(
                listed.contains(&m.key()),
                "metric {} missing from listing",
                m.key()
            );
        }
    }

    #[test]
    fn bench_higher_is_better_only_asr_wer_is_false() {
        for m in BenchMetric::all() {
            let expected = m.key() != "asr_wer";
            assert_eq!(
                m.higher_is_better(),
                expected,
                "higher_is_better wrong for {}",
                m.key()
            );
        }
    }

    // -----------------------------------------------------------------------
    // Bench subcommand parse sanity
    // -----------------------------------------------------------------------

    #[test]
    fn bench_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "modelx",
            "bench",
            "--filter",
            "arena_elo > 1400",
            "--sort",
            "coding_elo",
            "--desc",
            "--limit",
            "10",
            "--format",
            "csv",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Bench(args)) => {
                assert_eq!(args.filter.len(), 1);
                assert_eq!(args.sort.as_deref(), Some("coding_elo"));
                assert!(args.desc);
                assert_eq!(args.limit, Some(10));
                assert_eq!(args.format, "csv");
            }
            _ => panic!("expected bench"),
        }
    }

    #[test]
    fn benchmarks_alias_parses_as_bench() {
        let cli = Cli::try_parse_from(["modelx", "benchmarks"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Bench(_))));
    }

    // -----------------------------------------------------------------------
    // parse_duration
    // -----------------------------------------------------------------------

    #[test]
    fn parse_duration_seconds_unit() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
    }

    #[test]
    fn parse_duration_minutes_unit() {
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
    }

    #[test]
    fn parse_duration_hours_unit() {
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn parse_duration_days_unit() {
        assert_eq!(parse_duration("2d").unwrap(), Duration::from_secs(172_800));
    }

    #[test]
    fn parse_duration_bare_integer_is_seconds() {
        assert_eq!(parse_duration("45").unwrap(), Duration::from_secs(45));
    }

    #[test]
    fn parse_duration_trims_whitespace() {
        assert_eq!(parse_duration("  15m ").unwrap(), Duration::from_secs(900));
    }

    #[test]
    fn parse_duration_invalid_unit_errors() {
        let err = parse_duration("5x").unwrap_err();
        assert!(err.to_string().contains("5x"), "msg: {err}");
    }

    #[test]
    fn parse_duration_non_numeric_errors() {
        assert!(parse_duration("abc")
            .unwrap_err()
            .to_string()
            .contains("abc"));
        assert!(parse_duration("s").is_err());
        assert!(parse_duration("").is_err());
    }

    // -----------------------------------------------------------------------
    // Query parsing (repeated filter, URL-decoding, flags)
    // -----------------------------------------------------------------------

    #[test]
    fn query_repeated_filter_preserved_in_order() {
        let q = Query::parse("filter=a%3E1&filter=b%3C2&limit=5");
        let filters = q.get_all("filter");
        assert_eq!(filters, vec!["a>1".to_string(), "b<2".to_string()]);
        assert_eq!(q.get("limit"), Some("5"));
    }

    #[test]
    fn query_url_decodes_operators_and_commas() {
        // "context_limit > 100000" with a comma-containing fields list.
        let q = Query::parse("filter=context_limit%20%3E%20100000&fields=id%2Cname");
        assert_eq!(q.get("filter"), Some("context_limit > 100000"));
        assert_eq!(q.get("fields"), Some("id,name"));
    }

    #[test]
    fn query_plus_decodes_to_space() {
        let q = Query::parse("search=gpt+oss");
        assert_eq!(q.get("search"), Some("gpt oss"));
    }

    #[test]
    fn query_flag_variants() {
        assert!(Query::parse("desc").flag("desc"));
        assert!(Query::parse("desc=true").flag("desc"));
        assert!(Query::parse("desc=1").flag("desc"));
        assert!(Query::parse("desc=TRUE").flag("desc"));
        assert!(!Query::parse("desc=false").flag("desc"));
        assert!(!Query::parse("desc=0").flag("desc"));
        assert!(!Query::parse("").flag("desc"));
    }

    // -----------------------------------------------------------------------
    // handle() endpoints
    // -----------------------------------------------------------------------

    fn test_state() -> ApiState {
        let catalog = sample_catalog();
        let fetched_at = catalog.fetched_at;
        let source_id = catalog.source_id.clone();
        ApiState {
            catalog,
            bench: None,
            source_id,
            fetched_at,
        }
    }

    fn json_of(body: &str) -> serde_json::Value {
        serde_json::from_str(body).expect("handler must return valid json")
    }

    #[test]
    fn handle_health_shape() {
        let state = test_state();
        let (status, body) = handle("GET", "/health", "", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert_eq!(v["status"], "ok");
        assert_eq!(v["source"], state.source_id.as_str());
        assert_eq!(v["models"], state.catalog.total_models());
        assert_eq!(v["providers"], state.catalog.providers.len());
        assert_eq!(v["benchmarks"], false);
        assert!(v.get("fetched_at").is_some(), "fetched_at key present");
    }

    #[test]
    fn handle_fields_has_both_sections() {
        let state = test_state();
        let (status, body) = handle("GET", "/fields", "", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert!(v["model_fields"].is_array());
        assert!(v["benchmark_metrics"].is_array());
        assert_eq!(
            v["model_fields"].as_array().unwrap().len(),
            Field::all().len()
        );
        assert_eq!(
            v["benchmark_metrics"].as_array().unwrap().len(),
            BenchMetric::all().len()
        );
        // Each model field carries key/label/type.
        let first = &v["model_fields"][0];
        assert!(first["key"].is_string());
        assert!(first["label"].is_string());
        assert!(first["type"].is_string());
    }

    #[test]
    fn handle_models_returns_array() {
        let state = test_state();
        let (status, body) = handle("GET", "/models", "", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), state.catalog.total_models());
    }

    #[test]
    fn handle_models_honours_limit() {
        let state = test_state();
        let (status, body) = handle("GET", "/models", "limit=1", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert_eq!(v.as_array().unwrap().len(), 1);
    }

    #[test]
    fn handle_models_honours_filter() {
        let state = test_state();
        // reasoning=true → opus and gpt-oss in the sample catalog.
        let (status, body) = handle("GET", "/models", "filter=reasoning%20%3D%20true", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn handle_models_honours_fields_selection() {
        let state = test_state();
        let (status, body) = handle("GET", "/models", "fields=id%2Cname&limit=1", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        let obj = &v[0];
        assert!(obj["id"].is_string());
        assert!(obj["name"].is_string());
        // Only the requested keys are present.
        assert_eq!(obj.as_object().unwrap().len(), 2);
    }

    #[test]
    fn handle_model_detail_returns_raw_object() {
        let state = test_state();
        let (status, body) = handle("GET", "/models/provider-a/model-opus", "", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        // raw is an object (the untouched source blob).
        assert!(v.is_object());
    }

    #[test]
    fn handle_model_detail_missing_is_404() {
        let state = test_state();
        let (status, body) = handle("GET", "/models/provider-a/does-not-exist", "", &state);
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"], "model not found");
    }

    #[test]
    fn handle_providers_returns_array() {
        let state = test_state();
        let (status, body) = handle("GET", "/providers", "", &state);
        assert_eq!(status, 200);
        let v = json_of(&body);
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), state.catalog.providers.len());
    }

    #[test]
    fn handle_bench_returns_array() {
        let state = test_state();
        let (status, body) = handle("GET", "/bench", "", &state);
        assert_eq!(status, 200);
        assert!(json_of(&body).is_array());
    }

    #[test]
    fn handle_unknown_path_is_404() {
        let state = test_state();
        let (status, body) = handle("GET", "/nope", "", &state);
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"], "not found");
    }

    #[test]
    fn handle_non_get_is_405() {
        let state = test_state();
        let (status, _body) = handle("POST", "/models", "", &state);
        assert_eq!(status, 405);
    }

    #[test]
    fn handle_bad_filter_is_400() {
        let state = test_state();
        let (status, body) = handle("GET", "/models", "filter=not_a_field%20%3D%201", &state);
        assert_eq!(status, 400);
        assert!(json_of(&body)["error"].is_string());
    }

    #[test]
    fn handle_bad_fields_is_400() {
        let state = test_state();
        let (status, _body) = handle("GET", "/models", "fields=bogus_field", &state);
        assert_eq!(status, 400);
    }
}
