//! `modelx-benchmarks` — benchmark enrichment for modelx.
//!
//! Fetches public benchmark leaderboards (LMArena Elo, BigCodeBench Pass@1,
//! Open ASR WER) from the Hugging Face datasets-server and matches them to
//! catalog models by normalized name. See `docs/benchmarks.md`.
//!
//! # Layers
//! - [`metric`] — the [`BenchMetric`] enum (keys, labels, formatting).
//! - [`provider`] — [`BenchmarkProvider`] impls and their parse functions.
//! - [`matcher`] — [`normalize`] and the [`AliasTable`].
//! - [`cache`] — [`BenchCache`], an atomic per-provider JSON cache.
//! - [`db`] — [`BenchmarkDb`], the join layer used by the CLI/TUI.

pub mod cache;
pub mod db;
pub mod error;
pub mod matcher;
pub mod metric;
pub mod provider;

pub use cache::BenchCache;
pub use db::{BenchMatch, BenchmarkDb};
pub use error::BenchError;
pub use matcher::{normalize, AliasTable};
pub use metric::{BenchMetric, Source};
pub use provider::{
    BenchmarkProvider, BigCodeBenchProvider, LmArenaProvider, OpenAsrProvider, ProviderData,
    SourceEntry,
};
