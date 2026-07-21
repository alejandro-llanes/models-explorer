# modelx — Complete Usage Guide

This guide covers every use case `modelx` supports: the interactive TUI, the headless CLI, and the local HTTP API (including Docker). For installation see the [README](../README.md); for benchmark data sources and matching rules see [benchmarks.md](benchmarks.md); for the data-source abstraction and cache internals see [data-sources.md](data-sources.md).

---

## Part 1 — TUI

Running `modelx` with no arguments opens the terminal UI.

```bash
modelx                          # default data source, background refresh
modelx --source models.dev      # explicit source
modelx --offline                # serve from cache only, no network
modelx --config /path/to/config.toml
```

On startup, `modelx` loads the cached catalog immediately (so the UI is responsive at once) and fires a background refresh unless `--offline` is set. A spinner in the status bar indicates an in-progress refresh; a brief "updated" toast appears when the hot-swap completes.

### The three-pane layout

```
┌─ Providers ─────┬─ Models ──────────────┬─ Detail ─────────────────────────────┐
│                 │                       │                                       │
│                 │                       │                                       │
└─────────────────┴───────────────────────┴───────────────────────────────────────┘
  [status bar / hint line]
```

Focus moves left-to-right through **Providers → Models → Detail** with `Tab` / `l` and wraps back with `BackTab` / `h`. The focused pane has a highlighted border.

**Providers pane** — lists all providers alphabetically. The first row is always **"All providers"** — a synthetic entry that shows every model in the Models pane. Navigating to a provider narrows the Models pane to that provider's models.

**Models pane** — shows models for the selected provider (or all models) after applying any active search, filter, and sort. Models that have benchmark data in the local cache appear with a leading **`★`** marker and a distinct teal colour. Run `modelx refresh` once to populate the benchmark cache. Models can be marked for export or comparison with `Space`.

**Detail pane** — shows every field for the focused model: name, provider, family, context/output limits, pricing tiers, capability flags (reasoning, tool call, structured output, attachments, temperature control, open weights), knowledge cutoff, release date, modalities, and reasoning effort options.

Press `J` while the Detail pane is focused to switch to **raw JSON view**, which renders the unprocessed source object exactly as fetched. Press `J` again to return to the formatted view. Raw JSON is useful when a field is not yet surfaced by the formatted view.

### Context-aware search

Press `/` to open the search bar. The search targets the **focused pane**:

- **Providers pane focused** — filters the provider list by case-insensitive substring on provider id or name. The Providers pane header shows a filtered/total count while the search is active.
- **Models pane focused** — fuzzy-searches across provider names, model names, and model IDs simultaneously. The Models pane updates in real time as you type.

Press `Enter` to confirm the search and close the bar while keeping it active. Press `Esc` to clear that pane's search and close the bar. Active searches compose with sort and filter — all three stack.

### Sort

Press `s` to open the sort menu overlay. Available sort fields: Name, Provider, Context limit, Input cost, Output cost, Release date, Last updated.

Select a field with `j`/`k` and confirm with `Enter` (or `s`). Selecting the already-active field toggles direction (ascending ↔ descending). `d` also toggles direction without changing the field. The active sort field and direction appear in the status bar.

### Filter

Press `f` to open the filter overlay. Available filters:

| Filter | Description |
|--------|-------------|
| Reasoning | Show only models that support reasoning / chain-of-thought |
| Tool call | Show only models that support tool/function calling |
| Open weights | Show only open-weight models |
| Modality | Filter by a specific input modality (e.g. `image`, `audio`) |
| Min context | Show only models with at least this many context tokens |

Set a value with the cursor keys or text input, confirm with `Enter`. Leave a filter blank and confirm to clear it. Press `Esc` to close without changes. Multiple filters are ANDed together. Active filters are summarized in the status bar.

### Selection set

You can build a selection of models and export or compare them. The selection persists across navigation, search, and filter changes — it tracks models by their stable `provider_id + model_id` key, not by cursor position.

| Key | Action |
|-----|--------|
| `Space` | Toggle the focused model in/out of the selection |
| `a` | Add all models currently visible in the Models pane |
| `A` | Clear the entire selection |

A `[N selected]` indicator appears in the status bar.

### Export wizard

Press `e` to open the export wizard for the selection. It walks through three steps:

**Step 1 — Choose fields.** A checklist of all available fields. Use `j`/`k` to navigate, `Space` to toggle. Fields export in the order shown.

**Step 2 — Choose format.**

| Format | Description |
|--------|-------------|
| Plain list | One row per model; tab-separated with multiple fields. Single-field output is one value per line — ideal for piping. |
| CSV | Comma-separated values with a header row. |
| Markdown table | GitHub-flavored Markdown table. |
| JSON | Array of objects keyed by the field's machine key. |

**Step 3 — Choose destination.** Clipboard or a file path.

Press `Esc` at any step to cancel.

### Copy to clipboard

Without opening the export wizard, you can copy individual values directly:

| Key | What is copied |
|-----|---------------|
| `y` | The value of the focused field in the Detail pane |
| `Y` | The focused model serialized as pretty-printed JSON |

A brief toast confirms the copy. See [Caveats](../README.md#caveats) for the Linux/X11 clipboard-persistence limitation.

### Comparing models

Select **two or more** models, then press **`c`** to open the full-screen comparison view. `Esc` (or `c` again) returns you to the browser with the selection intact.

When the comparison opens, if any selected models have no benchmark match a toast appears: `⚠ N of M selected models have no benchmark data`. If the benchmark cache has not been populated at all: `⚠ benchmark data not loaded — run \`modelx refresh\``.

The comparison has two display modes, toggled with `Tab` / `BackTab`.

#### Table view (default)

A transposed table (metric rows × model columns) in two labelled sections:

**▌ Specs** — `Context`, `Output limit`, per-million-token prices (`Input`, `Output`, `Cache read`, `Cache write`, `Reasoning`), and two derived value metrics (`Context / $in`, `Output / $out`). Numbers are formatted for humans: `1.2M`, `256K`, `$3.00`.

**▌ Benchmarks** — a `Matched as` provenance row (the exact leaderboard entry matched, or `—`), then one row per metric:

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

For every row the **best** value is green and the **worst** red — except ASR WER, where lower is better so the colouring is inverted. `—` appears when a model has no data for that metric. The title bar shows coverage like `benchmarks: 3/4 models matched`.

A model with no benchmark data still compares fully on Specs. See [benchmarks.md](benchmarks.md) for sources and matching rules.

#### Bar view

A benchmark bar chart grouped by metric. Use the number keys to control which metrics are visible (at least one stays on; default: all three):

| Key | Metric |
|-----|--------|
| `1` | Arena Elo |
| `2` | Coding Elo |
| `3` | Math Elo |

Each active metric forms one group, with one bar per compared model that has a value. Within each group, bars are **sorted best → worst, left to right** and labelled with their value. Each model keeps **one consistent colour** across every metric group — the same colour used for that model in the table header and in the colour-matched legend below — so colour identifies the model at a glance, while best → worst is read from the sort order and bar height. The `models:` legend at the bottom pairs each colour swatch with the full model name. Models with no value for a metric are omitted from that group.

If no selected model has any benchmark data: `No benchmark data for the selected models — run \`modelx refresh\`.`

#### Actions from the comparison view

- **`y`** — copy the Specs + Benchmarks table to the clipboard as a GitHub-flavoured Markdown table.
- **`e`** — open the export wizard scoped to the compared models.

### Refresh

Press `r` to manually trigger a refresh of the active data source. The refresh runs in a background thread; the UI stays responsive. On success the new catalog is hot-swapped and the cache is updated atomically. The status bar shows a spinner while in progress, "Updated `<timestamp>`" on success, or an error message on failure (the previous data remains active).

### Source picker

Press `S` to open the source picker overlay. It lists all registered data sources with their cache status. Select a source and press `Enter` to switch to it. In the current release, **models.dev** is the only available source.

### Full keymap reference

#### Browser

| Key | Action |
|-----|--------|
| `q` / `Ctrl-C` | Quit |
| `Tab` / `l` | Focus next pane (Providers → Models → Detail) |
| `BackTab` / `h` | Focus previous pane |
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `g` | Jump to top of list |
| `G` | Jump to bottom of list |
| `/` | Open search (targets the focused pane) |
| `Enter` *(in search)* | Confirm search, close bar |
| `Esc` *(in search)* | Clear that pane's search, close bar |
| `s` | Open sort menu |
| `d` *(in sort)* | Toggle sort direction |
| `f` | Open filter menu |
| `Space` | Toggle selection on focused model |
| `a` | Select all models in current view |
| `A` | Clear entire selection |
| `c` | Open comparison view (2+ selected models) |
| `y` | Copy focused field value to clipboard |
| `Y` | Copy focused model as JSON to clipboard |
| `e` | Open export wizard |
| `r` | Refresh active data source |
| `S` | Open source picker |
| `?` | Toggle help overlay |
| `Esc` | Close current overlay |
| `J` *(Detail pane)* | Toggle raw JSON view |

#### Comparison view

| Key | Action |
|-----|--------|
| `Tab` / `BackTab` | Switch between Table view and Bar view |
| `1` / `2` / `3` | Toggle Bar view metrics (Arena / Coding / Math Elo) |
| `↑` / `↓` / `j` / `k` | Scroll rows (Table view) |
| `PageUp` / `PageDown` | Page up / down (Table view) |
| `y` | Copy benchmark table as Markdown |
| `e` | Export compared models |
| `Esc` / `c` | Return to the browser |
| `q` / `Ctrl-C` | Quit |
| `?` | Help |

---

## Part 2 — CLI

Running `modelx` with a subcommand runs a headless action and exits. All subcommands accept these global flags:

```
modelx [--source <id>] [--offline] [--config <path>] <subcommand>
```

### Auto-refresh behavior

Before any data subcommand (`providers`, `models`, `show`), `modelx` checks whether the cache is missing or older than 12 hours (configurable via `cache.ttl_hours`). If so, it fetches the active source first and prints a short notice to **stderr** — stdout stays clean for pipes and redirections. Pass `--offline` to suppress all network activity (errors if no cache exists). Use `modelx refresh` to force an update unconditionally.

### `modelx providers`

Lists the LLM providers/vendors in the catalog.

```
modelx providers [--filter <PATTERN>] [--regex] [--fields <keys>] [--sort <col>]
                 [--desc] [--limit <N>] [--count] [--format <fmt>] [--output <FILE>]
```

`--filter` is a case-insensitive substring match on provider `id` or `name`; add `--regex` to treat it as a regular expression.

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

The main query subcommand. `list` and `export` are aliases kept for backward compatibility.

```
modelx models [--filter <"FIELD OP VALUE">]… [--provider <P>] [--search <Q>]
              [--regex] [--fields <keys>] [--sort <field>] [--desc]
              [--limit <N>] [--count] [--format <fmt>] [--output <FILE>]
```

- `--filter` is repeatable; all expressions are AND-combined. See [Filter expressions](#filter-expressions) below.
- `--provider` narrows to models from a specific provider: case-insensitive substring on provider `id` or `name`.
- `--search` is a case-insensitive substring across provider name, model name, and model id simultaneously.
- `--regex` makes `--provider` and the `~`/`!~` filter operators treat their value as a regular expression.
- `--count` prints a single integer (the matching model count) rather than rows — useful in scripts.
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

# Export all Anthropic haiku models to CSV
modelx export --provider anthropic --filter "name~haiku" --fields id,name,input_cost --format csv
```

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

Comparison semantics depend on the field's **type** (inspect with `modelx fields`):

- **number** — `context_limit`, `output_limit`, and all `*_cost` fields compare numerically.
- **text** — all string fields, including `release_date`, `last_updated`, and `knowledge`, compare case-insensitively. Because ISO dates sort lexically, `release_date>=2025-01-01` works correctly without any special handling.
- **bool** — `reasoning`, `tool_call`, `open_weights`, `structured_output`, `attachment`, `temperature`: accepts `true`, `false`, `yes`, `no`, `1`, or `0`.
- **list** — `input_modalities`, `output_modalities`, `reasoning_efforts`: use `~` or `!~` to test for membership.

Missing values never satisfy an ordering or equality filter (`<`, `<=`, `=`, `>=`, `>`); they also never satisfy `~` or `!=`.

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

# Export the top-10 Arena Elo leaders to a Markdown table
modelx bench --sort arena_elo --desc --limit 10 \
             --fields provider_id,id,name,arena_elo --format md
```

Benchmark data must be in the local cache — run `modelx refresh` at least once to populate it. Pass `--offline` to use the cache without hitting the network. See [benchmarks.md](benchmarks.md) for a full explanation of sources, matching, and caveats.

### `modelx fields`

Lists every model field with its machine key, human-readable label, and type. Also prints a **Benchmarks** section with the 10 metric keys, their labels, data sources, and higher-is-better flags. Does not touch the network.

```bash
modelx fields
modelx fields --format json
```

**Field keys and types:**

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

Force-fetches the active source and updates the on-disk catalog cache. Also refreshes the benchmark cache (all three leaderboard sources: LMArena, BigCodeBench, Open ASR). Exits non-zero if any fetch fails. Run this at least once before using `modelx bench` or the comparison view's Benchmarks section.

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
models.dev  models.dev  https://models.dev  [cached (7200s ago)]
```

### `modelx api`

Starts a local synchronous HTTP server (no async runtime) that exposes the full catalog as JSON. The global `--source`, `--offline`, and `--config` flags apply. All routes return `application/json`.

```
modelx api [--listen-addr <ADDR>] [--listen-port <PORT>] [--refresh-interval <DUR>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--listen-addr` | `127.0.0.1` | Address to bind to |
| `--listen-port` | `8080` | Port to bind to |
| `--refresh-interval` | *(none)* | Auto-refresh interval; omit to disable |

`--refresh-interval` accepts a duration string: `30s`, `10m`, `1h`, `2d`, or a bare integer (seconds). When set, a background thread re-fetches the catalog and benchmarks on that interval and hot-swaps them atomically. A failed refresh is logged to stderr and the previous data continues to be served.

On start, `modelx api` prints `modelx api listening on http://<addr>:<port>` to **stderr**. There is no authentication — this is a local tool.

```bash
modelx api                              # bind 127.0.0.1:8080, no auto-refresh
modelx api --refresh-interval 1h       # refresh catalog + benchmarks every hour
modelx api --listen-port 9000          # custom port
modelx api --offline --refresh-interval 30m  # serve offline cache, no network
```

See [Part 3 — API](#part-3--api) for the full route table and query-parameter reference.

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
| `plain` / `list` | Default. One row per model; tab-separated when multiple fields are selected. Single-field output is one value per line — ideal for piping. |
| `csv` | Comma-separated values with a header row using field labels. |
| `md` / `markdown` | GitHub-flavored Markdown table with a header and separator row. |
| `json` | Array of objects, one per model, keyed by the field's machine key. |

`--output <FILE>` writes to a file instead of stdout; the directory must already exist.

### Offline mode

Pass `--offline` to suppress all network requests. `modelx` will use whatever is in the cache. If no cache file exists, the command errors with a message directing you to run `modelx refresh`. Useful on air-gapped machines or when you want to avoid any network activity.

---

## Part 3 — API

`modelx api` starts a local JSON HTTP server suitable for scripting, CI pipelines, dashboards, or any tool that can make HTTP calls.

### Starting the server

```bash
# Serve on 127.0.0.1:8080, no auto-refresh
modelx api

# Refresh catalog and benchmarks every hour
modelx api --refresh-interval 1h

# Serve on all interfaces (useful in Docker)
modelx api --listen-addr 0.0.0.0 --listen-port 8080 --refresh-interval 1h

# Use a specific source, serve without touching the network
modelx api --source models.dev --offline
```

The server is synchronous (no async runtime). It is designed for local use; there is no authentication.

### Route table

All routes are GET and return `application/json`. A non-GET request returns `405`. An unknown path returns `404 {"error":"not found"}`.

| Method | Path | Query params | Response |
|--------|------|--------------|----------|
| GET | `/health` | — | `{status, source, models, providers, fetched_at, benchmarks}` |
| GET | `/sources` | — | `[{id, name, homepage, cached, age_seconds}]` |
| GET | `/fields` | — | `{model_fields:[{key,label,type}], benchmark_metrics:[{key,label,source,higher_is_better}]}` |
| GET | `/providers` | `filter, fields, sort, desc, limit, regex` | array of provider objects |
| GET | `/models` | `filter` (repeatable, AND), `provider`, `search`, `regex`, `fields`, `sort`, `desc`, `limit` | typed JSON array |
| GET | `/models/{provider}/{model}` | — | raw source object for that model; `404 {"error":"model not found"}` if not found |
| GET | `/bench` | `filter` (repeatable; keys may be benchmark metric keys OR core fields), `provider`, `search`, `regex`, `fields`, `sort`, `desc`, `limit` | array (default `provider_id,id,name,arena_elo,coding_elo,math_elo`) |

A bad filter, field key, or sort key returns `400 {"error":"..."}` with a descriptive message.

### Query-parameter semantics

The query parameters for `/models`, `/providers`, and `/bench` mirror the CLI flags exactly:

- **`filter`** — repeatable; all expressions are AND-combined. Format: `FIELD OP VALUE` (same operators as the CLI). Values containing `<`, `>`, `=`, or `,` **must be URL-encoded** — e.g. `coding_elo>=1500` must be sent as `filter=coding_elo%3E%3D1500`.
- **`provider`** — case-insensitive substring on provider id or name.
- **`search`** — case-insensitive substring across provider name, model name, and model id.
- **`regex`** — boolean flag; true when present bare (`?regex`), as `regex=true`, or `regex=1`. Makes `provider` and `~`/`!~` filter operators treat their value as a regex.
- **`fields`** — comma-separated list of field or benchmark metric keys (see [Field keys](#modelx-fields) above). Default for `/models`: all model fields. Default for `/bench`: `provider_id,id,name,arena_elo,coding_elo,math_elo`.
- **`sort`** — field or benchmark metric key to sort by.
- **`desc`** — boolean flag; reverses the sort order when present.
- **`limit`** — integer; keep at most N rows.

The `/bench` route additionally accepts benchmark metric keys in `filter` and `sort`.

### curl recipes

**Health check:**
```bash
curl 'http://127.0.0.1:8080/health'
# {"status":"ok","source":"models.dev","models":5691,"providers":167,"fetched_at":1750000000,"benchmarks":true}
```

**Inspect available fields:**
```bash
curl 'http://127.0.0.1:8080/fields'
curl 'http://127.0.0.1:8080/sources'
```

**Cheapest big-context models (input ≤ $1/M, context ≥ 200 k tokens):**
```bash
curl 'http://127.0.0.1:8080/models?filter=input_cost%3C%3D1&filter=context_limit%3E%3D200000&sort=input_cost&limit=10'
```

(`%3C%3D` = `<=`, `%3E%3D` = `>=`)

**Anthropic models with selected fields:**
```bash
curl 'http://127.0.0.1:8080/models?provider=anthropic&limit=5&fields=id,name,input_cost'
```

**Raw JSON for a specific model:**
```bash
curl 'http://127.0.0.1:8080/models/anthropic/claude-opus-4-6'
# returns the raw source object, or 404 {"error":"model not found"}
```

**Top coding models (coding Elo ≥ 1500), sorted descending:**
```bash
curl 'http://127.0.0.1:8080/bench?filter=coding_elo%3E%3D1500&fields=provider_id,id,coding_elo&sort=coding_elo&desc=true&limit=10'
```

(`%3E%3D` = `>=`)

**All providers as JSON:**
```bash
curl 'http://127.0.0.1:8080/providers?fields=id,name,models&sort=models&desc=true&limit=20'
```

### Docker

A `Dockerfile` in the repository root builds a fully static musl binary and runs it on a minimal Alpine image. The default `CMD` runs `modelx api --listen-addr 0.0.0.0 --listen-port 8080 --refresh-interval 1h`. The `ENTRYPOINT` is `modelx`, so any subcommand can be run by overriding the command.

**Build:**
```bash
docker build -t modelx .
```

**Run the API:**
```bash
# Ephemeral — cache is lost when the container stops
docker run --rm -p 8080:8080 modelx

# Persist the cache across restarts with a named volume
docker run --rm -p 8080:8080 -v modelx-data:/data modelx
```

The server binds `0.0.0.0:8080` inside the container (mapped to the host via `-p 8080:8080`). The `/data` volume holds the catalog and benchmark caches — mounting it means a restart skips the initial fetch and serves immediately from the persisted data.

**Run a CLI command in Docker:**
```bash
docker run --rm modelx models --provider anthropic --fields id,name --format json
```

Note that the TUI requires a real terminal and is not the intended Docker use case.
