# modelx — Data Sources

This document explains how `modelx` acquires and caches model data, the distinction between a *data source* and an *LLM provider*, and how to add a new data source.

---

## Data source vs. LLM provider — an important distinction

These two terms are easy to confuse:

- A **data source** is *where `modelx` gets its catalog* — a service or file that `modelx` fetches and parses to build its internal database of models and providers. `models.dev` is a data source.

- An **LLM provider** (or vendor) is an *entry inside that catalog* — Anthropic, OpenAI, Google, Mistral, and so on. These are the companies or projects that publish the models you browse in the UI.

`modelx` ships a `DataSource` trait and a registry so that additional catalogs (alternative model indexes, local files, enterprise registries, etc.) can be added without changing the core application. The LLM providers it shows you are determined by whichever data source you have active — they are data, not code.

---

## The `DataSource` trait

Every data source implements:

```rust
pub trait DataSource: Send + Sync {
    fn id(&self) -> &str;          // stable machine identifier, e.g. "models.dev"
    fn name(&self) -> &str;        // human-readable label
    fn homepage(&self) -> &str;    // URL for attribution / display
    fn fetch(&self) -> Result<Catalog, DataSourceError>;   // blocking HTTP (or other I/O) + parse
}
```

The `fetch` method is always called on a background thread — the UI thread is never blocked.

---

## `SourceRegistry`

`SourceRegistry` owns all registered sources and provides lookup by ID.

```rust
pub struct SourceRegistry { /* ... */ }
impl SourceRegistry {
    pub fn with_defaults() -> Self;                // registers models.dev
    pub fn register(&mut self, s: Box<dyn DataSource>);
    pub fn ids(&self) -> Vec<&str>;
    pub fn get(&self, id: &str) -> Option<&dyn DataSource>;
    pub fn default_id(&self) -> &str;             // "models.dev"
}
```

`with_defaults` is the entry point used by `modelx-cli` at startup. Registering a new source requires calling `register` before `with_defaults` returns (or on a custom registry you build instead).

---

## The `models.dev` source

[models.dev](https://models.dev) is a community-maintained catalog of LLM providers and models. `modelx` fetches the machine-readable API at:

```
https://models.dev/api.json
```

The response is a single JSON object with all providers and their models. `modelx` maps this to its internal `Catalog` / `Provider` / `Model` types (in `modelx-datasource/src/modelsdev/`):

- `schema.rs` — raw `serde` structs that mirror the `api.json` shape exactly.
- `map.rs` — converts the raw structs to `modelx-core` types, captures the original JSON object into `Model::raw` (used by the Detail pane's raw JSON view), and renames the source field `type` to the internal field name `kind`.

Parsing is **tolerant**: unknown JSON fields are ignored and missing optional fields become `None`. This means `modelx` will continue to work even if models.dev adds new fields that `modelx` doesn't yet know about.

---

## Caching

### Why caching matters

`modelx` is designed to be instant. On every launch it loads from the local cache first so the UI is immediately responsive, then refreshes in the background. The cache also makes `modelx` fully functional when you are offline or when models.dev is temporarily unavailable.

### Cache file locations

The cache is stored using the platform cache directory, identified by the application qualifier `dev/modelx/modelx` (via the `directories` crate).

| Platform | Cache path |
|----------|-----------|
| Linux | `~/.cache/modelx/sources/<source_id>.json` |
| macOS | `~/Library/Caches/dev.modelx.modelx/sources/<source_id>.json` |
| Windows | `%LOCALAPPDATA%\modelx\modelx\cache\sources\<source_id>.json` |

For the default source, `<source_id>` is `models.dev`, so on Linux the file is:

```
~/.cache/modelx/sources/models.dev.json
```

### Cache format

The cache file is the serialized `Catalog` struct as JSON, including a `fetched_at` Unix timestamp (seconds) stamped by the CLI just before the file is written. This timestamp is used to determine staleness and is displayed in the `modelx sources` output.

### TTL and staleness

The default TTL is **24 hours**, configurable via `[cache] ttl_hours` in `config.toml`. `modelx` always loads from cache on startup regardless of age, then decides whether to refresh in the background:

- If the cache is fresh (age < TTL), the background refresh is skipped.
- If the cache is stale or absent, a background refresh begins immediately.
- With `--offline`, no refresh is ever attempted.

You can always force an immediate refresh with `r` in the TUI or `modelx refresh` on the command line.

### Atomic writes

Cache writes are **atomic**: the new catalog is written to a temporary file in the same directory as the cache file, then renamed into place. This guarantees that the cache file is never in a partially-written state, even if `modelx` is killed during a write.

### Offline behavior

With `--offline`, `modelx` loads whatever is in the cache and never makes a network request. If the cache is empty (no file exists), the UI starts with no data and displays a notice that the catalog could not be loaded offline. This mode is useful on air-gapped machines or when you want to avoid any network activity.

---

## Config paths

The configuration file follows the same platform-directory convention:

| Platform | Config path |
|----------|------------|
| Linux | `~/.config/modelx/config.toml` |
| macOS | `~/Library/Application Support/dev.modelx.modelx/config.toml` |
| Windows | `%APPDATA%\modelx\modelx\config\config.toml` |

Override with `modelx --config <path>`.

---

## How to add a new data source

Adding a data source requires changes in the `modelx-datasource` crate only — no other crate needs to change.

### Step 1 — Create your source module

Add a new module under `crates/modelx-datasource/src/`, for example `mysource/mod.rs`.

Implement the `DataSource` trait:

```rust
use modelx_core::Catalog;
use crate::{DataSource, DataSourceError};

pub struct MySource {
    base_url: String,
}

impl MySource {
    pub fn new() -> Self {
        Self { base_url: "https://myapi.example.com".into() }
    }
}

impl DataSource for MySource {
    fn id(&self) -> &str { "my-source" }
    fn name(&self) -> &str { "My Source" }
    fn homepage(&self) -> &str { "https://myapi.example.com" }

    fn fetch(&self) -> Result<Catalog, DataSourceError> {
        // Make a blocking HTTP request (use ureq, which is already a workspace dep)
        let body: serde_json::Value = ureq::get(&self.base_url)
            .call()
            .map_err(|e| DataSourceError::Http(e.to_string()))?
            .into_json()
            .map_err(|e| DataSourceError::Parse(e.to_string()))?;

        // Parse `body` into a `Catalog` and return it.
        // Set `catalog.source_id = self.id().to_string()`.
        // Leave `catalog.fetched_at = None` — the CLI stamps it before caching.
        todo!("map body to Catalog")
    }
}
```

### Step 2 — Register the source

In `crates/modelx-datasource/src/registry.rs`, add your source to `SourceRegistry::with_defaults`:

```rust
pub fn with_defaults() -> Self {
    let mut r = Self::new();
    r.register(Box::new(ModelsDevSource::new()));
    r.register(Box::new(MySource::new()));  // add this line
    r
}
```

### Step 3 — Test it

Write a unit test (ideally against a committed fixture file) that calls `MySource::new().fetch()` against a local mock or a small `with_base_url` override, and asserts the returned `Catalog` has the expected shape.

### What happens next

Once registered, the new source:
- Appears in `modelx sources` output
- Is selectable via `modelx --source my-source` and the `S` source picker in the TUI
- Gets its own cache file at `<cache_dir>/sources/my-source.json`
- Respects the global TTL and `--offline` flag automatically

The field registry, query engine, export formats, and UI widgets all work without modification — they operate on `Catalog`/`Model` types, not on any source-specific shape.
