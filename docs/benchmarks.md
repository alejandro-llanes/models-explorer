# modelx — Benchmark Data

`modelx` enriches the catalog with performance scores from three open leaderboards,
surfaced in the **`modelx bench`** command and the **Benchmarks** section of the
comparison view. This page explains where the data comes from, how models are matched,
how caching works, and what to keep in mind when interpreting the numbers.

---

## Data sources

All three sources are fetched via the Hugging Face **datasets-server** API — no
authentication required.

### LMArena (`lmarena-ai/leaderboard-dataset`)

Bradley-Terry Elo ratings computed from human preference votes collected on LMArena
(formerly LMSYS Chatbot Arena). A separate Elo is maintained per conversation category:

| Key | Label |
|-----|-------|
| `arena_elo` | Arena Elo (overall) |
| `coding_elo` | Coding |
| `math_elo` | Math |
| `creative_elo` | Creative |
| `instruction_elo` | Instruction |
| `hard_prompts_elo` | Hard Prompts |
| `vision_elo` | Vision |
| `imagegen_elo` | Image Gen |

LMArena is updated daily and, crucially, **covers proprietary models** (GPT, Claude,
Gemini, and others) alongside open-weight models — making it the primary source for
frontier model comparisons.

### BigCodeBench (`bigcode/bigcodebench-results`)

Pass@1 percentage on the BigCodeBench coding benchmark. Includes a number of proprietary
models, though **coverage is capped at approximately December 2024** — models released
after that date are unlikely to appear.

| Key | Label |
|-----|-------|
| `code_pass_at_1` | Code Pass@1 |

### Open ASR (`hf-audio/open-asr-leaderboard-results`)

Word Error Rate (WER) on the Open ASR Leaderboard. Lower WER is better. This leaderboard
focuses on speech-recognition models; most general-purpose chat models do not appear here.

| Key | Label |
|-----|-------|
| `asr_wer` | ASR WER |

---

## Model matching

`modelx` joins benchmark entries to the catalog using a **normalized-name exact-version**
strategy:

1. Both the catalog model id and the leaderboard model name are normalized
   (lowercased, punctuation standardized).
2. Date snapshots are stripped before comparison, so `claude-opus-4-5` matches
   `claude-opus-4-5-20251101` — but `claude-opus-4-5` never matches
   `claude-opus-4-6` (different version).
3. If a match is still not found, `modelx` checks an optional **alias file** at
   `<config_dir>/modelx/benchmark-aliases.toml`. This is a TOML file with an
   `[aliases]` table mapping a catalog model id to the leaderboard name used for that
   same model:

   ```toml
   [aliases]
   "anthropic/claude-opus-4-5" = "Claude Opus 4.5 (API)"
   ```

   Aliases are intended for same-version naming mismatches only — they are not a
   mechanism for mapping one model onto a completely different model's scores.

**Coverage:** roughly **~46 % of catalog models** match at least one benchmark source;
among **open-weight models** it is closer to **~50 %**. Models with no match display `—`
in every benchmark field and are still fully comparable on Specs in the comparison view.

Each match is transparent: the `Matched as` row in the comparison view (and the implicit
provenance in `modelx bench`) shows the exact leaderboard name that was matched.

---

## Caching and refresh

Benchmark data is stored in a per-source cache under:

| Platform | Cache path |
|----------|-----------|
| Linux | `~/.cache/modelx/benchmarks/` |
| macOS | `~/Library/Caches/dev.modelx.modelx/benchmarks/` |
| Windows | `%LOCALAPPDATA%\modelx\modelx\cache\benchmarks\` |

One cache file is written per leaderboard source. Cache writes are atomic (write to a
temporary file, then rename).

**Refresh behavior:**

- **`modelx refresh`** always refreshes both the catalog cache and all three benchmark
  caches. Run this at least once before using `modelx bench` or the comparison view's
  Benchmarks section.
- **`modelx bench`** also refreshes a stale benchmark cache automatically (governed by
  `cache.ttl_hours` in your config, default 12 hours), unless `--offline` is set.
- **The TUI** loads benchmark data from the cache only at startup — it does not trigger a
  benchmark refresh on its own. If the cache is empty, the Benchmarks section of the
  comparison view shows `benchmarks: none loaded`.
- **`--offline`** (global flag) suppresses all network requests and uses whatever is on
  disk. If the benchmark cache is empty under `--offline`, all benchmark fields return
  `—`.

---

## Caveats

**Proprietary/frontier models only have LMArena Elo.** GPT, Claude, Gemini, and other
closed-weight models are not included in BigCodeBench or Open ASR, so `code_pass_at_1`
and `asr_wer` will be `—` for them.

**BigCodeBench is a December 2024 snapshot.** Models released after that date are not
present, regardless of how well they would perform. Use `coding_elo` (LMArena) as a
proxy for more recent coding ability on proprietary models.

**Elo is relative, not absolute.** An Elo of 1300 only means something in relation to
the other models on that leaderboard at that point in time. A model added to the
leaderboard later can shift everyone's Elo. Do not compare raw Elo numbers across
different leaderboards or different points in time as if they were the same scale.

**Brand-new models may not appear on any leaderboard yet.** Leaderboards take time to
accumulate enough human votes or benchmark runs. A freshly released model may show `—`
across all benchmark fields even if it is a strong performer.

**Coverage varies by category.** A model may have an overall `arena_elo` but no
`vision_elo` if it has not received enough vision-specific votes. `—` means "no data,"
not "zero."
