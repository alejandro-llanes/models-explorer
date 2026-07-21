# modelx — Architecture

`modelx` is a cross-platform terminal UI for exploring LLM models and providers.
This document is the **contract**: every crate's public surface is specified here so the
pieces integrate cleanly.

## 1. Workspace layout (split by domain)

```
models-explorer/
├── Cargo.toml                 # virtual workspace manifest
├── crates/
│   ├── modelx-core/           # domain model + query engine (pure, no I/O)
│   ├── modelx-datasource/     # DataSource trait + registry + models.dev source (HTTP)
│   ├── modelx-cache/          # on-disk cache in the platform cache dir
│   ├── modelx-export/         # export a selection to JSON / CSV / Markdown / plain list
│   ├── modelx-tui/            # ratatui UI: state machine, widgets, event loop, theme
│   └── modelx-cli/            # binary `modelx`: clap CLI, config, wiring, background refresh
├── docs/                      # architecture / usage / data-sources
└── README.md
```

Dependency graph (a → b means "a depends on b"):

```
cli ──► tui ──► export ──► core
   │      ├────► cache  ──► core
   │      └────► datasource ──► core
   ├────► cache, datasource, export, core
```

`core` has **no I/O and no async**. HTTP is synchronous (`ureq`); background refresh is a
`std::thread` that sends results back over an `std::sync::mpsc` channel — **no tokio**.

## 2. `modelx-core` — domain + queries

Everything is `serde`-(de)serializable so a `Catalog` round-trips straight to the cache.
Deps: `serde` (derive), `serde_json`, `nucleo-matcher`, `thiserror`.

### 2.1 Types (`model.rs`)

```rust
// A full catalog from one data source.
pub struct Catalog {
    pub source_id: String,          // e.g. "models.dev"
    pub fetched_at: Option<i64>,    // unix seconds (set by cache/datasource, not core)
    pub providers: Vec<Provider>,   // sorted by name
}

pub struct Provider {
    pub id: String,
    pub name: String,
    pub env: Vec<String>,           // default empty
    pub npm: Option<String>,
    pub api: Option<String>,
    pub doc: Option<String>,
    pub models: Vec<Model>,         // sorted by name
}

pub struct Model {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider_id: String,        // denormalized back-reference (for flat/search views)
    pub provider_name: String,
    pub family: Option<String>,
    pub attachment: Option<bool>,
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub structured_output: Option<bool>,
    pub temperature: Option<bool>,
    pub open_weights: Option<bool>,
    pub knowledge: Option<String>,      // "2026-01"
    pub release_date: Option<String>,   // "2026-05-28"
    pub last_updated: Option<String>,
    pub status: Option<String>,         // alpha | beta | deprecated
    pub reasoning_options: Vec<ReasoningOption>,
    pub modalities: Modalities,
    pub limit: Limit,
    pub cost: Option<Cost>,
    pub interleaved: Option<serde_json::Value>,   // e.g. {"field":"reasoning_content"}
    pub provider_override: Option<serde_json::Value>, // model-level "provider" object
    pub experimental: Option<serde_json::Value>,  // kept raw
    pub raw: serde_json::Value,        // the untouched source object — powers "show everything"
}

pub struct ReasoningOption { pub kind: String, pub values: Vec<String> } // "type" -> kind
pub struct Modalities { pub input: Vec<String>, pub output: Vec<String> }
pub struct Limit { pub context: Option<u64>, pub output: Option<u64>, pub input: Option<u64> }
pub struct Cost {
    pub input: Option<f64>, pub output: Option<f64>,
    pub cache_read: Option<f64>, pub cache_write: Option<f64>,
    pub reasoning: Option<f64>,
    pub input_audio: Option<f64>, pub output_audio: Option<f64>,
    pub context_over_200k: Option<serde_json::Value>,
    pub tiers: Option<serde_json::Value>,
}
```

`Catalog` helpers: `total_models() -> usize`, `provider(&id) -> Option<&Provider>`,
`all_models() -> impl Iterator<Item=&Model>` (flattened, for the search/flat view),
`ModelRef { provider_id, model_id }` newtype used as a stable selection key
(`Model::key(&self) -> ModelRef`).

### 2.2 Field registry (`field.rs`) — single source of truth for columns/detail/export/sort

```rust
pub enum Field {
    ProviderId, ProviderName, Id, Name, Description, Family, Status,
    ContextLimit, OutputLimit, InputCost, OutputCost, CacheReadCost, CacheWriteCost,
    ReasoningCost, Reasoning, ToolCall, StructuredOutput, Attachment, Temperature,
    OpenWeights, Knowledge, ReleaseDate, LastUpdated, InputModalities, OutputModalities,
    ReasoningEfforts,
}
impl Field {
    pub fn all() -> &'static [Field];
    pub fn key(&self) -> &'static str;      // stable machine key, e.g. "input_cost"
    pub fn label(&self) -> &'static str;    // human label, e.g. "Input $/M"
    pub fn value(&self, m: &Model) -> FieldValue;  // typed value for sort + render
}
pub enum FieldValue { Text(String), Int(Option<i64>), Float(Option<f64>), Bool(Option<bool>), List(Vec<String>) }
impl FieldValue { pub fn display(&self) -> String; }   // "" for None, "yes"/"no" for bool, etc.
```

The registry drives table columns, the detail pane, export field-selection, and sort keys —
add a `Field` variant once and it appears everywhere.

### 2.3 Query engine (`query.rs`)

```rust
pub struct Query {
    pub search: String,             // fuzzy text (matches provider+model name/id)
    pub filters: Filters,
    pub sort: Sort,
}
pub struct Filters {
    pub provider_ids: Vec<String>,  // empty = all
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub open_weights: Option<bool>,
    pub input_modality: Option<String>,   // e.g. "image"
    pub min_context: Option<u64>,
    pub max_input_cost: Option<f64>,
}
pub struct Sort { pub field: Field, pub descending: bool }

pub fn run_query<'a>(catalog: &'a Catalog, q: &Query) -> Vec<&'a Model>;
```

Fuzzy search uses `nucleo_matcher`; empty search returns everything (filtered+sorted).
`run_query` is pure and unit-tested against a fixture catalog. Errors: `CoreError` (thiserror).

## 3. `modelx-datasource`

Deps: `modelx-core` (path), `serde`, `serde_json`, `ureq`, `thiserror`.

```rust
pub trait DataSource: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn homepage(&self) -> &str;
    fn fetch(&self) -> Result<Catalog, DataSourceError>;   // blocking HTTP + parse
}

pub struct ModelsDevSource { /* base_url, ureq agent */ }
impl ModelsDevSource { pub fn new() -> Self; pub fn with_base_url(url: impl Into<String>) -> Self; }

pub struct SourceRegistry { /* ... */ }
impl SourceRegistry {
    pub fn with_defaults() -> Self;                 // registers models.dev
    pub fn register(&mut self, s: Box<dyn DataSource>);
    pub fn ids(&self) -> Vec<&str>;
    pub fn get(&self, id: &str) -> Option<&dyn DataSource>;
    pub fn default_id(&self) -> &str;               // "models.dev"
}
pub enum DataSourceError { Http(...), Parse(...), NotFound(String) }  // thiserror
```

models.dev mapping lives here (`modelsdev/schema.rs` = raw serde structs mirroring
`api.json`; `modelsdev/map.rs` = `RawApi -> core::Catalog`, including `raw` capture and
`type`→`kind` rename). Parsing is tolerant: unknown fields are ignored, missing optionals
become `None`. Tested against the committed `tests/fixtures/api-sample.json`.

## 4. `modelx-cache`

Deps: `modelx-core` (path), `serde`, `serde_json`, `directories`, `thiserror`.

Location: `ProjectDirs::from("dev","modelx","modelx").cache_dir()/sources/<source_id>.json`.

```rust
pub struct Cache { /* base dir */ }
impl Cache {
    pub fn discover() -> Result<Cache, CacheError>;      // platform cache dir
    pub fn with_dir(dir: PathBuf) -> Cache;              // for tests / override
    pub fn load(&self, source_id: &str) -> Result<Option<Catalog>, CacheError>;
    pub fn store(&self, catalog: &Catalog) -> Result<(), CacheError>;  // atomic: tmp+rename
    pub fn age_seconds(&self, source_id: &str) -> Option<i64>;
    pub fn is_stale(&self, source_id: &str, ttl_seconds: i64) -> bool;
    pub fn path_for(&self, source_id: &str) -> PathBuf;
}
```

Writes are atomic (write temp in the same dir, then rename). `fetched_at` is stamped by the
caller (cli) before `store`. Round-trip tested in a `tempfile` dir.

## 5. `modelx-export`

Deps: `modelx-core` (path), `serde`, `serde_json`, `csv`, `thiserror`.

```rust
pub enum Format { PlainList, Csv, Markdown, Json }
impl Format { pub fn all() -> &'static [Format]; pub fn label(&self)->&str; pub fn ext(&self)->&str; }

pub struct ExportRequest<'a> { pub models: Vec<&'a Model>, pub fields: Vec<Field>, pub format: Format }
pub fn render(req: &ExportRequest) -> Result<String, ExportError>;
pub fn write(req: &ExportRequest, path: &Path) -> Result<(), ExportError>;
```

- **PlainList**: one row per model; if exactly one field → just that value per line
  (the "all ollama model names" case); if multiple → tab-separated.
- **Csv**: header = field labels, one row per model (via the `csv` crate).
- **Markdown**: GitHub table with a header + separator row.
- **Json**: array of objects keyed by `Field::key()`, values typed from `FieldValue`.

Every format is unit-tested for a 2-model, 3-field selection.

## 6. `modelx-tui`

Deps: `modelx-core`, `modelx-datasource`, `modelx-cache`, `modelx-export` (paths),
`ratatui` (latest), `arboard`, `thiserror`. Uses `ratatui::crossterm` re-export (no separate
crossterm dep → no version skew).

### 6.1 State

```rust
pub enum Focus { Providers, Models, Detail }
pub enum Mode { Normal, Search, Sort, Filter, Export, SourcePicker, Help }
pub enum RefreshState { Idle, Refreshing, Ok(i64 /*ts*/), Failed(String) }

pub struct AppState {
    catalog: Catalog,
    query: Query,
    view: Vec<ModelRef>,           // result of run_query, as stable keys
    focus: Focus, mode: Mode,
    provider_cursor: usize, model_cursor: usize,
    selection: HashSet<ModelRef>,  // export selection set
    refresh: RefreshState,
    toast: Option<(String, /*ticks left*/ u8)>,
    // sub-state for overlays: search input buffer, sort menu cursor, export wizard step, etc.
}
```

`AppState` is UI logic only (no I/O). It exposes `on_key(KeyEvent) -> Option<AppCommand>`
and `apply(AppEvent)`. **`AppCommand`** = side-effects the cli/runtime performs:
`Refresh(source_id)`, `CopyToClipboard(String)`, `Export{req-desc}`, `Quit`,
`SwitchSource(String)`. **`AppEvent`** = things pushed in: `RefreshStarted`,
`RefreshDone(Catalog)`, `RefreshError(String)`, `Tick`. This keeps clipboard/HTTP/file I/O
in the binary and the widget logic pure & testable.

### 6.2 Rendering (`ui.rs`) & theme

3-pane master-detail + bottom status/help bar; overlays drawn as centered popups.
`Theme` struct holds styles (accent, selected, dim, ok, warn, err); one default theme.
Long lists are windowed via ratatui `ListState`/`TableState`. Detail pane renders the full
field set and can drop to a **raw-JSON** sub-view (from `Model::raw`).

### 6.3 Keymap (documented in usage.md and the `?` overlay)

`q`/Ctrl-C quit · `Tab`/`h`/`l` focus · `j`/`k`/`↑`/`↓` move · `g`/`G` top/bottom ·
`/` search · `s` sort · `f` filter · `space` toggle-select · `a` select-all-in-view ·
`A` clear selection · `y` copy focused value · `Y` copy model as JSON · `e` export ·
`r` refresh source · `S` source picker · `?` help · `Esc` close overlay.

### 6.4 Runtime (`run.rs`)

`pub fn run(app: AppState, ctx: RuntimeCtx) -> anyhow::Result<()>` owns the terminal
(raw mode, alt screen, panic-safe restore), polls `ratatui::crossterm` events with a
~100 ms timeout, drains the mpsc refresh channel into `AppEvent`s, ticks the spinner/toast,
executes `AppCommand`s (spawning the refresh thread, arboard copy, export write).

## 7. `modelx-cli` (binary `modelx`)

Deps: all `modelx-*` (paths), `clap` (derive), `directories`, `toml`, `serde`, `anyhow`.
`[[bin]] name = "modelx"`.

```
modelx                      # launch the TUI (default)
modelx --source <id>        # start on a specific data source
modelx --offline            # never hit the network; cache only
modelx sources              # list registered data sources
modelx refresh [--source]   # headless: fetch + update cache
modelx list   [--source] [--provider P]        # headless dump
modelx export --provider ollama --fields id --format plain [--output F] [--source S]
```

The headless `export` mirrors the TUI exporter (same `modelx-export`), so
`modelx export --provider ollama --fields id --format plain` prints every ollama model id —
scriptable without opening the UI.

**Config** (`config_dir()/modelx/config.toml`, all optional):
```toml
default_source = "models.dev"
[cache]
ttl_hours = 24
[ui]
theme = "default"
```

**Startup flow:** load config → `Cache::discover` → `SourceRegistry::with_defaults` →
load cached `Catalog` for the source (instant render; empty state if none) → build
`AppState` → `tui::run`, which immediately fires a background `Refresh` unless `--offline`.
On `RefreshDone`, stamp `fetched_at`, `Cache::store`, hot-swap into the UI, toast "updated".

## 8. Cross-cutting

- **Errors:** libraries use `thiserror`; the binary uses `anyhow`.
- **Latest deps:** versions are resolved with `cargo add` at scaffold time and pinned in
  `Cargo.lock`; shared versions live in `[workspace.dependencies]`.
- **Portability:** no OS-specific code paths; `directories`/`arboard`/`ureq(rustls)` cover
  Linux, macOS, Windows, FreeBSD. Clipboard-manager caveat on Linux is documented.
- **Testing:** `core`, `datasource`, `cache`, `export` are unit-tested (datasource/export
  against committed fixtures); `cargo build`, `cargo clippy -D warnings`, `cargo fmt --check`
  gate integration.
```
