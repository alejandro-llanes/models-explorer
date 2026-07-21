---
title: "modelx — a terminal UI & CLI for exploring LLM models"
description: "Fast cross-platform terminal UI (TUI) and CLI to explore, compare, and query large language models — context windows, pricing, modalities, and capabilities — sourced from models.dev. Built in Rust with ratatui."
image: /assets/social-preview.png
---

# modelx

**A fast, cross-platform terminal UI (TUI) and command-line tool for exploring, comparing, and querying large language models.** Browse context windows, pricing, modalities, and capabilities across **167 providers and ~5,700 models**, sourced live from [models.dev](https://models.dev). Built in Rust with [ratatui](https://ratatui.rs).

[Download the latest release](https://github.com/alejandro-llanes/models-explorer/releases/latest) · [Source on GitHub](https://github.com/alejandro-llanes/models-explorer)

## Why modelx?

Comparing LLMs means juggling context limits, input/output token prices, cache pricing, reasoning support, modalities, and knowledge cutoffs across dozens of providers. `modelx` puts the whole catalog one keystroke — or one shell command — away, and keeps it fresh automatically.

## Features

- **3-pane terminal UI** — Providers → Models → Detail, fully keyboard-driven, with fuzzy search, sort, and capability filters.
- **Model comparison tool** — pick 2+ models and compare their numeric fields as a **bar chart**, an **X-vs-Y scatter** plot, or an exact-numbers **table**, including derived value metrics like *context-per-dollar*.
- **Rich headless CLI** — query models by any field with comparison operators (`input_cost<=3`, `context_limit>=200000`, `release_date>=2025-01-01`, `name~opus`), then sort, limit, count, choose fields, and output as **plain, CSV, JSON, or Markdown**.
- **Providers & fields commands** — list vendors, inspect every model field and its type, or dump one model's full JSON with `modelx show`.
- **Auto-refreshing cache** — the catalog updates automatically when it's older than 12 hours, so every query hits fresh data. Works offline from cache too.
- **Cross-platform** — prebuilt binaries for **Linux, macOS, Windows, and FreeBSD**.

## Install

Grab a prebuilt binary for your platform from the [latest release](https://github.com/alejandro-llanes/models-explorer/releases/latest), or build from source:

```bash
git clone https://github.com/alejandro-llanes/models-explorer
cd models-explorer
cargo build --release   # binary at target/release/modelx
```

## Quick start

```bash
# Launch the interactive terminal UI
modelx

# The 10 cheapest models with a big context window
modelx models --filter "input_cost<=1" --filter "context_limit>=200000" \
              --fields provider_id,id,context_limit,input_cost --sort input_cost --limit 10

# Every Anthropic "opus" model, as JSON
modelx models --provider anthropic --filter "name~opus" --fields id,name,input_cost --format json

# List providers, or show one model's raw detail
modelx providers --filter anthropic
modelx show anthropic claude-opus-4-5
```

## Documentation

- [Usage guide](https://github.com/alejandro-llanes/models-explorer/blob/main/docs/usage.md) — the full TUI and CLI reference
- [Architecture](https://github.com/alejandro-llanes/models-explorer/blob/main/docs/architecture.md) — how the workspace is structured
- [Data sources](https://github.com/alejandro-llanes/models-explorer/blob/main/docs/data-sources.md) — the provider abstraction and caching

## Links

- **GitHub:** [alejandro-llanes/models-explorer](https://github.com/alejandro-llanes/models-explorer)
- **Releases:** [download binaries](https://github.com/alejandro-llanes/models-explorer/releases)
- **Data:** [models.dev](https://models.dev)

---

_modelx is open source under the [MIT License](https://github.com/alejandro-llanes/models-explorer/blob/main/LICENSE)._
