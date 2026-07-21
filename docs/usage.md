# modelx — Usage Guide

This guide covers everything you can do in the TUI and at the command line. For installation see the [README](../README.md); for the data source abstraction and caching internals see [data-sources.md](data-sources.md).

---

## Launching modelx

```bash
# Open the TUI with the default (or cached) data source
modelx

# Start on a specific source
modelx --source models.dev

# Never touch the network — serve from cache only
modelx --offline

# Use a custom config file
modelx --config /path/to/config.toml
```

On startup, `modelx` loads the cached catalog immediately (so the UI is responsive at once) and fires a background refresh unless `--offline` is set. A spinner in the status bar indicates an in-progress refresh; a brief "updated" toast appears when the hot-swap completes.

---

## The three-pane layout

```
┌─ Providers ─────┬─ Models ──────────────┬─ Detail ─────────────────────────────┐
│                 │                       │                                       │
│                 │                       │                                       │
└─────────────────┴───────────────────────┴───────────────────────────────────────┘
  [status bar / hint line]
```

Focus moves left-to-right through **Providers → Models → Detail** and wraps back. The focused pane has a highlighted border; items in it respond to `j`/`k` (or arrow keys).

### Providers pane

Lists all providers in the catalog. The first row is always **"All providers"** — a synthetic entry that, when selected, shows every model across all providers in the Models pane. Below it, providers are listed alphabetically.

Navigating to a provider narrows the Models pane to that provider's models. The count shown in the pane header updates to reflect the filtered view.

### Models pane

Shows the models for the currently selected provider (or all models when "All providers" is active), after applying any active search, filter, and sort. Each row shows the model name and a few summary columns.

Models can be individually **selected** for export (see [Selection and export](#selection-and-export)).

### Detail pane

Shows every field for the focused model: name, provider, family, context/output limits, pricing tiers, capability flags (reasoning, tool call, structured output, attachments, temperature control, open weights), knowledge cutoff, release date, modalities, and reasoning effort options.

Press `J` while the Detail pane is focused to switch to **raw JSON view**, which renders the unprocessed source object exactly as fetched. Press `J` again to return to the formatted view. Raw JSON is useful when a field is not yet surfaced by the UI.

---

## Search

Press `/` to open the search bar. The search is **context-aware**: it targets whichever pane is currently focused.

- **Providers pane focused** — opens a "Search providers" overlay that filters the provider list by case-insensitive substring on provider id or name. The Providers pane header shows a filtered/total count while the search is active.
- **Models pane focused** — opens a "Search models" overlay that fuzzy-searches across provider names, model names, and model IDs simultaneously. The Models pane updates in real time as you type.

Press `Enter` to confirm the search and close the bar while keeping it active. Press `Esc` to clear that pane's search and close the bar. An active search is shown in the status bar. You can combine search with sort and filter — they all compose.

### Benchmarked models in the Models pane

Any model that has benchmark data in the local cache is shown with a leading **`★`** marker and a distinct teal colour in the Models pane, so you can see at a glance which models are benchmarked. Run `modelx refresh` to populate the benchmark cache.

---

## Sort

Press `s` to open the sort menu overlay. The available sort fields are:

- Name
- Provider
- Context limit
- Input cost
- Output cost
- Release date
- Last updated

Select a field with `j`/`k` and press `Enter` (or `s` again) to apply. If you select the field that is already active, the sort direction toggles (ascending ↔ descending). Pressing `d` also toggles direction without changing the field.

The active sort field and direction are shown in the status bar.

---

## Filter

Press `f` to open the filter overlay. Available filters:

| Filter | Description |
|--------|-------------|
| Reasoning | Show only models that support reasoning / chain-of-thought |
| Tool call | Show only models that support tool/function calling |
| Open weights | Show only open-weight models |
| Modality | Filter by a specific input modality (e.g. `image`, `audio`) |
| Min context | Show only models with at least this many context tokens |

Set a filter value with the cursor keys or text input, then confirm with `Enter`. Clear a filter by leaving it blank and confirming. Press `Esc` to close without changes.

Multiple filters are ANDed together. Active filters are summarized in the status bar.

---

## Selection and export

You can build a **selection set** of models and export them to a file or the clipboard.

### Building a selection

| Key | Action |
|-----|--------|
| `Space` | Toggle the focused model in/out of the selection |
| `a` | Add all models currently visible in the Models pane to the selection |
| `A` | Clear the entire selection |

A selection indicator (e.g. `[3 selected]`) appears in the status bar. Selected models are highlighted in the Models pane. The selection persists across navigation and across search/filter changes — it tracks models by their stable `provider_id + model_id` key, not by cursor position.

### Export wizard

Press `e` to open the export wizard. It walks you through three steps:

**Step 1 — Choose fields.**
A checklist of all available fields is shown. Use `j`/`k` to move and `Space` to toggle a field on or off. The fields are exported in the order you see them. Press `Enter` to advance.

**Step 2 — Choose format.**

| Format | Description |
|--------|-------------|
| Plain list | One row per model. With a single field, one value per line (ideal for piping). With multiple fields, tab-separated values. |
| CSV | Comma-separated values with a header row using field labels. |
| Markdown table | GitHub-flavored Markdown table with header and separator row. |
| JSON | Array of objects, one per model, keyed by the field's machine key. |

Select with `j`/`k`, confirm with `Enter`.

**Step 3 — Choose destination.**
- **Clipboard** — the rendered output is copied to the system clipboard.
- **File** — a file-path prompt appears; enter a path and press `Enter`.

Press `Esc` at any step to cancel and return to the normal view.

---

## Comparing models

Select **two or more** models (`Space`, or `a` to select everything in the current
view — selections persist across providers, searches, and filters), then press **`c`**
to open the **comparison view**. It replaces the whole screen; `Esc` (or `c` again)
returns you to the browser with the selection intact. If fewer than two models are
selected, a hint reminds you to select more.

When the comparison opens, if any selected models have no benchmark match a toast appears:
`⚠ N of M selected models have no benchmark data`. If the benchmark cache has not been
populated at all, the toast reads `⚠ benchmark data not loaded — run \`modelx refresh\``.

The comparison has two display modes. Press `Tab` / `BackTab` to toggle between them.

### Table view (default)

A full-screen **table** (metric rows × model columns) split into two labelled sections.

**▌ Specs** covers the numeric spec fields: `Context`, `Output limit`, per-million-token
prices (`Input`, `Output`, `Cache read`, `Cache write`, `Reasoning`), and two derived
value metrics — `Context / $in` (context window per dollar of input cost) and
`Output / $out`. Numbers are formatted for humans: `1.2M`, `256K`, `$3.00`.

**▌ Benchmarks** opens with a `Matched as` row that shows which leaderboard entry each
model was matched to (or `—` if no match was found), followed by one row per benchmark
metric:

| Row | Source |
|-----|--------|
| Arena Elo | LMArena |
| Coding | LMArena |
| Math | LMArena |
| Creative | LMArena |
| Instruction | LMArena |
| Hard Prompts | LMArena |
| Vision | LMArena |
| Image Gen | LMArena |
| Code Pass@1 | BigCodeBench |
| ASR WER | Open ASR |

For every row the **best** value is highlighted green and the **worst** red — except
ASR WER, where *lower is better* so the colouring is inverted. `—` appears when a model
has no benchmark data for that metric.

The title bar shows a coverage note such as `benchmarks: 3/4 models matched` (or
`benchmarks: none loaded` if you haven't run `modelx refresh` yet). A model with no
benchmark data still compares on specs.

### Bar view

A benchmark bar chart grouped by metric. Press `Tab` from the Table view to switch to it.
Use number keys to control which metrics are shown (at least one stays on; default: all three):

| Key | Metric |
|-----|--------|
| `1` | Arena Elo |
| `2` | Coding Elo |
| `3` | Math Elo |

Layout is **grouped by metric**: each active metric forms one group, with one bar per
compared model that has a value for that metric. Models with no value for a metric are
omitted from that group. Within each group the bars are **sorted best → worst, left to
right**, each labelled with its Elo score.

Each model keeps **one consistent colour** across every metric group — the same colour used
for that model in the table header and in the colour-matched legend below — so colour maps
to a model at a glance, while best → worst is conveyed by the sort order and bar height. The
`models:` legend at the bottom pairs each colour swatch with the full model name.

If none of the selected models has any benchmark data, the Bar view shows:
`No benchmark data for the selected models — run \`modelx refresh\`.`

Benchmark data is loaded from the local cache only at startup — run `modelx refresh`
once to populate it. See [benchmarks.md](benchmarks.md) for sources and matching rules.

### Actions from the comparison view

- **`y`** — copy the benchmark **table** (Specs + Benchmarks sections) to the clipboard
  as a GitHub-flavoured **Markdown table**, ready to paste into an issue, PR, or doc.
- **`e`** — open the export wizard scoped to the compared models (JSON / CSV / Markdown /
  plain list, to clipboard or a file).

| Key | Action |
| --- | --- |
| `Tab` / `BackTab` | Switch between Table view and Bar view |
| `1` / `2` / `3` | Toggle Bar view metrics (Arena / Coding / Math Elo) |
| `↑` / `↓` / `j` / `k` | Scroll rows (Table view) |
| `PageUp` / `PageDown` | Page up / down (Table view) |
| `y` | Copy the benchmark table as a Markdown table |
| `e` | Export the compared models |
| `Esc` / `c` | Back to the browser |
| `q` / `Ctrl-C` | Quit |
| `?` | Help |

---

## Copy to clipboard

Without opening the export wizard, you can copy individual values directly:

| Key | What is copied |
|-----|---------------|
| `y` | The value of the focused field in the Detail pane (e.g. just the context limit number) |
| `Y` | The focused model serialized as pretty-printed JSON |

A brief toast confirms the copy. See [Caveats](../README.md#caveats) for the Linux/X11 clipboard-persistence limitation.

---

## Refresh

Press `r` to manually trigger a refresh of the active data source. The refresh runs in a background thread; the UI stays responsive. When the new catalog arrives it is hot-swapped into the UI and the cache is updated atomically.

The status bar shows:
- A spinner while a refresh is in progress
- "Updated `<timestamp>`" on success
- An error message if the fetch failed (the previous cached data remains active)

---

## Source picker

Press `S` to open the source picker overlay. It lists all registered data sources with their cache status (age of the cached catalog, or "no cache"). Select a source and press `Enter` to switch to it. The application immediately loads that source's cache and starts a background refresh.

In the current release, **models.dev** is the only available source.

---

## Help overlay

Press `?` to toggle the in-app help overlay, which shows the full keymap. Press `?` or `Esc` to close it.

---

## Keymap reference

| Key | Action |
|-----|--------|
| `q` / `Ctrl-C` | Quit |
| `Tab` / `l` | Focus next pane (Providers → Models → Detail) |
| `BackTab` / `h` | Focus previous pane |
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `g` | Jump to top of list |
| `G` | Jump to bottom of list |
| `/` | Open search — targets the focused pane (providers or models) |
| `Enter` *(in search)* | Confirm search, close bar |
| `Esc` *(in search)* | Clear that pane's search, close bar |
| `s` | Open sort menu |
| `d` *(in sort)* | Toggle sort direction |
| `f` | Open filter menu |
| `Space` | Toggle selection on focused model |
| `a` | Select all models in current view |
| `A` | Clear entire selection |
| `y` | Copy focused field value to clipboard |
| `Y` | Copy focused model as JSON to clipboard |
| `e` | Open export wizard |
| `r` | Refresh active data source |
| `S` | Open source picker |
| `?` | Toggle help overlay |
| `Esc` | Close current overlay |
| `J` *(Detail pane)* | Toggle raw JSON view |

---

## Headless CLI reference

Running `modelx` with no arguments opens the TUI. Passing a subcommand runs a headless action and exits. All subcommands accept these global flags:

```
modelx [--source <id>] [--offline] [--config <path>] <subcommand>
```

Subcommands: `providers`, `models` (`list`/`export`), `fields`, `show`, `bench` (`benchmarks`), `sources`, `refresh`, `api`, `completions`.

### Auto-refresh behavior

Before any data subcommand (`providers`, `models`, `show`), modelx checks whether the cache is missing or older than 12 hours (configurable via `cache.ttl_hours`). If so, it fetches the active source first and prints a short notice to **stderr** — stdout stays clean for pipes and redirections. Pass `--offline` to suppress all network activity (errors if no cache exists). Use `modelx refresh` to force an update unconditionally.

### `modelx providers`

Lists the LLM providers/vendors in the catalog.

```
modelx providers [--filter <PATTERN>] [--regex] [--fields <keys>] [--sort <col>]
                 [--desc] [--limit <N>] [--count] [--format <fmt>] [--output <FILE>]
```

`--filter` is a case-insensitive substring match on provider `id` or `name`; add `--regex` to treat the pattern as a regular expression.

**Provider columns** (valid for `--fields` and `--sort`):

| Key | Description |
|-----|-------------|
| `id` | Provider identifier |
| `name` | Display name |
| `npm` | npm package name |
| `api` | API base URL |
| `doc` | Documentation URL |
| `env` | API key environment variable |
| `models` | Number of models in the catalog |

Default fields: `id,name,models`.

```bash
modelx providers --filter anthro
modelx providers --fields id,name,models --sort models --desc --limit 3 --format json
```

### `modelx models`

The main query subcommand. **`list` and `export` are aliases** kept for backward compatibility.

```
modelx models [--filter <"FIELD OP VALUE">]… [--provider <P>] [--search <Q>]
              [--regex] [--fields <keys>] [--sort <field>] [--desc]
              [--limit <N>] [--count] [--format <fmt>] [--output <FILE>]
```

- `--filter` is repeatable; all expressions are AND-combined. See [Filter expressions](#filter-expressions) below for syntax and operators.
- `--provider` narrows to models from a specific provider: case-insensitive substring on provider `id` or `name`.
- `--search` is a case-insensitive substring search across provider name, model name, and model id simultaneously.
- `--regex` makes `--provider` and the `~`/`!~` filter operators treat their value as a regular expression.
- `--count` prints a single integer (the matching model count) rather than rows — useful in scripts.
- `--desc` reverses the sort order.
- Default fields: `provider_id,id,name`.

**Examples:**

```bash
# Cheapest big-context models
modelx models --filter "input_cost<=1" --filter "context_limit>=200000" \
              --fields provider_id,id,context_limit,input_cost --sort input_cost --limit 5

# Anthropic models whose name contains "opus", as JSON
modelx models --provider anthropic --filter "name~opus" --fields id,name,input_cost --format json

# Count all reasoning-capable models
modelx models --filter "reasoning=true" --count

# Word-form operator: models with context ≥ 1 M tokens
modelx models --filter "context_limit gte 1000000" --count

# Models released in 2026 or later, newest first, as a Markdown table
modelx models --filter "release_date>=2026-01-01" --sort release_date --desc --limit 3 --format md

# Regex: Anthropic claude-opus-4 or claude-sonnet-4 model IDs only
modelx models --regex --provider anthropic --filter 'id~^claude-(opus|sonnet)-4' --fields id

# Alias: export still works
modelx export --provider anthropic --filter "name~haiku" --fields id,name,input_cost --format csv
```

### `modelx bench`

Alias: `benchmarks`. Queries models enriched with benchmark scores from open leaderboards. Accepts the same flags as `modelx models`; benchmark metric keys are valid everywhere a model field key is expected (`--filter`, `--fields`, `--sort`). A coverage note (`<N>/<M> models have benchmark data`) is printed to **stderr**; missing values render as `—`.

```
modelx bench [--filter <"FIELD OP VALUE">]… [--provider <P>] [--search <Q>]
             [--regex] [--fields <keys>] [--sort <key>] [--desc]
             [--limit <N>] [--count] [--format <fmt>] [--output <FILE>]
             [--offline]
```

Default fields: `provider_id,id,name,arena_elo,coding_elo,math_elo`.

**Benchmark metric keys:**

| Key | Label | Higher better? | Source |
|-----|-------|---------------|--------|
| `arena_elo` | Arena Elo | yes | LMArena |
| `coding_elo` | Coding | yes | LMArena |
| `math_elo` | Math | yes | LMArena |
| `creative_elo` | Creative | yes | LMArena |
| `instruction_elo` | Instruction | yes | LMArena |
| `hard_prompts_elo` | Hard Prompts | yes | LMArena |
| `vision_elo` | Vision | yes | LMArena |
| `imagegen_elo` | Image Gen | yes | LMArena |
| `code_pass_at_1` | Code Pass@1 | yes | BigCodeBench |
| `asr_wer` | ASR WER | **no** (lower better) | Open ASR |

**Examples:**

```bash
# Top Anthropic models by coding score
modelx bench --provider anthropic --fields id,name,arena_elo,coding_elo,math_elo \
             --sort coding_elo --desc --limit 5

# Models with a coding Elo of 1500 or above
modelx bench --filter "coding_elo>=1500" --fields provider_id,id,coding_elo \
             --sort coding_elo --desc --limit 8

# Cheap Anthropic models with their Arena Elo, as JSON
modelx bench --provider anthropic --filter "input_cost<=10" \
             --fields id,name,input_cost,arena_elo --format json
```

Benchmark data must be in the local cache — run `modelx refresh` at least once to populate it. Pass `--offline` to use the cache without hitting the network. See [benchmarks.md](benchmarks.md) for a full explanation of sources, matching, and caveats.

### Filter expressions

The value passed to `--filter` is a quoted string of the form `"FIELD OP VALUE"`. Both symbol and word forms of every operator are accepted interchangeably:

| Symbol | Word | Meaning |
|--------|------|---------|
| `<` | `lt` | less than |
| `<=` | `lte` | less than or equal |
| `=` | `eq` | equals |
| `!=` | `ne` | not equal |
| `>=` | `gte` | greater than or equal |
| `>` | `gt` | greater than |
| `~` | `contains` | contains (substring, or regex when `--regex` is set) |
| `!~` | `ncontains` | does not contain |

Comparison semantics depend on the field's **type**, which `modelx fields` reports:

- **number** — `context_limit`, `output_limit`, and all `*_cost` fields compare numerically.
- **text** — all string fields, including `release_date`, `last_updated`, and `knowledge`, compare case-insensitively. Because ISO dates sort lexically, `release_date>=2025-01-01` works correctly without any special handling.
- **bool** — `reasoning`, `tool_call`, `open_weights`, `structured_output`, `attachment`, `temperature`, `open_weights`: accepts `true`, `false`, `yes`, `no`, `1`, or `0`.
- **list** — `input_modalities`, `output_modalities`, `reasoning_efforts`: use `~` or `!~` to test for membership.

Missing values never satisfy an ordering or equality filter (`<`, `<=`, `=`, `>=`, `>`); they also never satisfy `~` or `!=`. Use `--count` with a negated filter to find models where a field is absent.

### `modelx fields`

Lists every model field with its machine key, human-readable label, and type. Also prints a **Benchmarks** section with the 10 metric keys, their labels, data sources, and higher-is-better flags. Does not touch the network.

```bash
modelx fields
modelx fields --format json
```

**Field keys** and types:

| Key | Label | Type |
|-----|-------|------|
| `provider_id` | Provider ID | text |
| `provider_name` | Provider | text |
| `id` | Model ID | text |
| `name` | Name | text |
| `description` | Description | text |
| `family` | Family | text |
| `status` | Status | text |
| `context_limit` | Context | number |
| `output_limit` | Output limit | number |
| `input_cost` | Input $/M | number |
| `output_cost` | Output $/M | number |
| `cache_read_cost` | Cache read $/M | number |
| `cache_write_cost` | Cache write $/M | number |
| `reasoning_cost` | Reasoning $/M | number |
| `reasoning` | Reasoning | bool |
| `tool_call` | Tool call | bool |
| `structured_output` | Structured output | bool |
| `attachment` | Attachment | bool |
| `temperature` | Temperature | bool |
| `open_weights` | Open weights | bool |
| `knowledge` | Knowledge cutoff | text |
| `release_date` | Release date | text |
| `last_updated` | Last updated | text |
| `input_modalities` | Input modalities | list |
| `output_modalities` | Output modalities | list |
| `reasoning_efforts` | Reasoning efforts | list |

An unknown key in `--fields` causes an error that prints the full list of valid keys.

### `modelx show`

Prints the full detail for a single model. Default format is `json`, which renders the unprocessed source object exactly as fetched.

```
modelx show <provider> <model> [--format <fmt>]
```

Provider and model arguments are resolved by exact id first, then by case-insensitive substring. The command errors clearly on no match or an ambiguous match.

```bash
modelx show anthropic claude-opus-4-5
```

### `modelx refresh`

Force-fetches the active source and updates the on-disk catalog cache. Also refreshes the benchmark cache (all three leaderboard sources: LMArena, BigCodeBench, Open ASR). Exits non-zero if any fetch fails. Run this at least once before using `modelx bench` or the Benchmarks section of the comparison view.

```bash
modelx refresh
modelx refresh --source models.dev
```

### `modelx sources`

Lists all registered data sources and the age/status of their cached catalogs.

```bash
modelx sources
```

Example output:
```
models.dev    models.dev    https://models.dev    cached 2h ago
```

### `modelx api`

Starts a local synchronous HTTP server that exposes the catalog as JSON. The global `--source`, `--offline`, and `--config` flags apply.

```
modelx api [--listen-addr <ADDR>] [--listen-port <PORT>] [--refresh-interval <DUR>]
```

Defaults: `--listen-addr 127.0.0.1`, `--listen-port 8080`, no auto-refresh. `--refresh-interval` accepts `30s`, `10m`, `1h`, `2d`, or a bare integer (seconds). When set, a background thread re-fetches the catalog and benchmarks on that interval and hot-swaps them; a failed refresh keeps serving the previous data.

On start, prints `modelx api listening on http://<addr>:<port>` to stderr. No authentication.

**Route table (all `GET`, all return `application/json`):**

| Path | Query params | Response |
|------|--------------|----------|
| `/health` | — | `{status, source, models, providers, fetched_at, benchmarks}` |
| `/sources` | — | `[{id, name, homepage, cached, age_seconds}]` |
| `/fields` | — | `{model_fields:[…], benchmark_metrics:[…]}` |
| `/providers` | `filter, fields, sort, desc, limit, regex` | provider array |
| `/models` | `filter` (repeatable, AND), `provider`, `search`, `regex`, `fields`, `sort`, `desc`, `limit` | model array |
| `/models/{provider}/{model}` | — | raw source object; `404 {"error":"model not found"}` |
| `/bench` | same as `/models`; benchmark metric keys valid in `filter`/`sort` | model+benchmark array |

Filter values containing `<`, `>`, `=`, or `,` must be URL-encoded (e.g. `<=` → `%3C%3D`). `desc` is true when present bare, as `=true`, or `=1`. An unknown path returns `404`; non-GET returns `405`; a bad filter/field/sort returns `400 {"error":"..."}`.

```bash
modelx api --refresh-interval 1h
curl 'http://127.0.0.1:8080/health'
curl 'http://127.0.0.1:8080/models?provider=anthropic&limit=5&fields=id,name,input_cost'
curl 'http://127.0.0.1:8080/models?filter=input_cost%3C%3D1&filter=context_limit%3E%3D200000&sort=input_cost&limit=10'
curl 'http://127.0.0.1:8080/bench?filter=coding_elo%3E%3D1500&sort=coding_elo&desc=true&limit=10'
curl 'http://127.0.0.1:8080/models/anthropic/claude-opus-4-6'
```

For the full route reference, query-parameter semantics, and Docker workflow see [guide.md](guide.md#part-3--api).

### `modelx completions`

Prints a shell completion script to stdout. Redirect it into a file and source it from your shell's startup script.

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

```bash
modelx completions bash > modelx.bash
source modelx.bash
```

### Output formats

All data subcommands accept `--format <fmt>`:

| Format | Description |
|--------|-------------|
| `plain` / `list` | Default. One row per model; tab-separated when multiple fields are selected. With a single field, one value per line — ideal for piping. |
| `csv` | Comma-separated values with a header row using field labels. |
| `md` / `markdown` | GitHub-flavored Markdown table with a header and separator row. |
| `json` | Array of objects, one per model, keyed by the field's machine key. |

`--output <FILE>` writes to a file instead of stdout; the directory must already exist.

---

## HTTP API reference

`modelx api` exposes the same query engine as the CLI over HTTP, with all routes returning `application/json`. See [guide.md](guide.md#part-3--api) for a task-oriented walk-through; this section is the complete reference.

### Starting the server

```
modelx api [--listen-addr <ADDR>] [--listen-port <PORT>] [--refresh-interval <DUR>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--listen-addr` | `127.0.0.1` | Address to bind to |
| `--listen-port` | `8080` | Port to bind to |
| `--refresh-interval` | *(none)* | Duration string (`30s`, `10m`, `1h`, `2d`, or bare integer = seconds); omit to disable auto-refresh |

When `--refresh-interval` is set, a background thread re-fetches the catalog and all benchmark caches on that interval and hot-swaps them atomically. A failed refresh logs to stderr; the previous data continues to be served.

The server prints `modelx api listening on http://<addr>:<port>` to **stderr** on start. There is no authentication.

### Routes

All routes are GET and return `application/json`. A non-GET request returns `405 {"error":"method not allowed"}`. An unknown path returns `404 {"error":"not found"}`. A bad filter expression, unknown field key, or invalid sort key returns `400 {"error":"..."}`.

#### `GET /health`

Returns a status object. No query parameters.

```json
{
  "status": "ok",
  "source": "models.dev",
  "models": 5691,
  "providers": 167,
  "fetched_at": 1750000000,
  "benchmarks": true
}
```

`fetched_at` is a Unix timestamp (seconds) of the last successful catalog fetch; `benchmarks` is `true` when the benchmark cache is loaded.

#### `GET /sources`

Returns an array of registered data sources with their cache status.

```json
[{"id":"models.dev","name":"models.dev","homepage":"https://models.dev","cached":true,"age_seconds":3600}]
```

#### `GET /fields`

Returns all field metadata. No query parameters.

```json
{
  "model_fields": [{"key": "id", "label": "Model ID", "type": "text"}, …],
  "benchmark_metrics": [{"key": "arena_elo", "label": "Arena Elo", "source": "LmArena", "higher_is_better": true}, …]
}
```

#### `GET /providers`

Returns an array of provider objects.

| Param | Description |
|-------|-------------|
| `filter` | Case-insensitive substring (or regex with `regex=true`) on provider id or name |
| `fields` | Comma-separated provider columns: `id,name,npm,api,doc,env,models` |
| `sort` | Provider column to sort by |
| `desc` | Boolean flag; reverses sort order |
| `limit` | Keep at most N rows |
| `regex` | Boolean flag; treat `filter` as a regex |

#### `GET /models`

Returns a typed JSON array of model objects.

| Param | Description |
|-------|-------------|
| `filter` | Repeatable; `FIELD OP VALUE` expressions, AND-combined. Must URL-encode `<`, `>`, `=`, `,` |
| `provider` | Case-insensitive substring on provider id or name |
| `search` | Case-insensitive substring across provider name, model name, and model id |
| `regex` | Boolean flag; treat `provider` and `~`/`!~` operators as regex |
| `fields` | Comma-separated model field keys; default: all model fields |
| `sort` | Model field key to sort by |
| `desc` | Boolean flag; reverses sort order |
| `limit` | Keep at most N rows |

#### `GET /models/{provider}/{model}`

Returns the raw source object for a single model identified by exact provider id and model id. Returns `404 {"error":"model not found"}` if either is not found.

#### `GET /bench`

Same as `/models` but enriches rows with benchmark scores. Benchmark metric keys (`arena_elo`, `coding_elo`, `math_elo`, …) are valid in `filter`, `fields`, and `sort`. Default `fields`: `provider_id,id,name,arena_elo,coding_elo,math_elo`.

### Filter syntax for the API

Filter expressions follow the same grammar as the CLI `--filter` flag: `FIELD OP VALUE`. The operator and value must be URL-encoded when they contain reserved characters:

| Operator | URL-encoded form |
|----------|-----------------|
| `<=` | `%3C%3D` |
| `>=` | `%3E%3D` |
| `<` | `%3C` |
| `>` | `%3E` |
| `=` | `%3D` |
| `!=` | `!%3D` |

Example: `input_cost<=1` becomes `filter=input_cost%3C%3D1`.

The `filter` parameter is repeatable — pass it multiple times to AND conditions:
```
?filter=input_cost%3C%3D1&filter=context_limit%3E%3D200000
```

### Example curl recipes

```bash
# Health check
curl 'http://127.0.0.1:8080/health'

# Anthropic models with selected fields
curl 'http://127.0.0.1:8080/models?provider=anthropic&limit=5&fields=id,name,input_cost'

# Cheapest big-context models
curl 'http://127.0.0.1:8080/models?filter=input_cost%3C%3D1&filter=context_limit%3E%3D200000&sort=input_cost&limit=10'

# Top coding models by Elo
curl 'http://127.0.0.1:8080/bench?filter=coding_elo%3E%3D1500&fields=provider_id,id,coding_elo&sort=coding_elo&desc=true&limit=10'

# Raw JSON for one model
curl 'http://127.0.0.1:8080/models/anthropic/claude-opus-4-6'

# All fields and benchmark metric definitions
curl 'http://127.0.0.1:8080/fields'
```
