//! [`BenchMetric`] — the enumerated set of benchmark metrics modelx tracks,
//! plus their stable keys, labels, source, orientation, and formatting.

/// The upstream leaderboard a metric originates from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// LMArena crowd-sourced Elo leaderboards.
    LmArena,
    /// BigCodeBench code-generation Pass@1 leaderboard.
    BigCodeBench,
    /// Open ASR word-error-rate leaderboard.
    OpenAsr,
}

/// A single benchmark metric that can enrich a catalog model.
///
/// Elo-style metrics come from LMArena; `CodePassAt1` from BigCodeBench;
/// `AsrWer` from Open ASR (the only metric where lower is better).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BenchMetric {
    /// LMArena overall (text) Elo.
    ArenaOverall,
    /// LMArena coding-category Elo.
    ArenaCoding,
    /// LMArena math-category Elo.
    ArenaMath,
    /// LMArena creative-writing-category Elo.
    ArenaCreative,
    /// LMArena instruction-following-category Elo.
    ArenaInstruction,
    /// LMArena hard-prompts-category Elo.
    ArenaHardPrompts,
    /// LMArena vision (multimodal) Elo.
    ArenaVision,
    /// LMArena text-to-image (image generation) Elo.
    ArenaImageGen,
    /// BigCodeBench instruct-split Pass@1 (0–100).
    CodePassAt1,
    /// Open ASR average cleaned word error rate (percentage, lower is better).
    AsrWer,
}

impl BenchMetric {
    /// All metrics, in a stable display order.
    pub fn all() -> &'static [BenchMetric] {
        use BenchMetric::*;
        &[
            ArenaOverall,
            ArenaCoding,
            ArenaMath,
            ArenaCreative,
            ArenaInstruction,
            ArenaHardPrompts,
            ArenaVision,
            ArenaImageGen,
            CodePassAt1,
            AsrWer,
        ]
    }

    /// Stable machine key used in cached JSON and CLI/TUI column ids.
    pub fn key(&self) -> &'static str {
        use BenchMetric::*;
        match self {
            ArenaOverall => "arena_elo",
            ArenaCoding => "coding_elo",
            ArenaMath => "math_elo",
            ArenaCreative => "creative_elo",
            ArenaInstruction => "instruction_elo",
            ArenaHardPrompts => "hard_prompts_elo",
            ArenaVision => "vision_elo",
            ArenaImageGen => "imagegen_elo",
            CodePassAt1 => "code_pass_at_1",
            AsrWer => "asr_wer",
        }
    }

    /// Human-readable label for headers and detail panes.
    pub fn label(&self) -> &'static str {
        use BenchMetric::*;
        match self {
            ArenaOverall => "Arena Elo",
            ArenaCoding => "Coding Elo",
            ArenaMath => "Math Elo",
            ArenaCreative => "Creative Elo",
            ArenaInstruction => "Instruction Elo",
            ArenaHardPrompts => "Hard Prompts Elo",
            ArenaVision => "Vision Elo",
            ArenaImageGen => "Image Gen Elo",
            CodePassAt1 => "Code Pass@1",
            AsrWer => "ASR WER",
        }
    }

    /// The upstream leaderboard this metric comes from.
    pub fn source(&self) -> Source {
        use BenchMetric::*;
        match self {
            ArenaOverall | ArenaCoding | ArenaMath | ArenaCreative | ArenaInstruction
            | ArenaHardPrompts | ArenaVision | ArenaImageGen => Source::LmArena,
            CodePassAt1 => Source::BigCodeBench,
            AsrWer => Source::OpenAsr,
        }
    }

    /// Whether a higher value is better. Only [`BenchMetric::AsrWer`] is `false`.
    pub fn higher_is_better(&self) -> bool {
        !matches!(self, BenchMetric::AsrWer)
    }

    /// Format a raw value for display.
    ///
    /// Elo metrics render as a rounded integer (e.g. `"1497"`); `CodePassAt1`
    /// and `AsrWer` render with one decimal and a trailing `%` (e.g. `"61.2%"`).
    pub fn format(&self, v: f64) -> String {
        use BenchMetric::*;
        match self {
            CodePassAt1 | AsrWer => format!("{v:.1}%"),
            _ => format!("{}", v.round() as i64),
        }
    }

    /// Parse a metric from its stable [`BenchMetric::key`].
    pub fn from_key(s: &str) -> Option<BenchMetric> {
        use BenchMetric::*;
        Some(match s {
            "arena_elo" => ArenaOverall,
            "coding_elo" => ArenaCoding,
            "math_elo" => ArenaMath,
            "creative_elo" => ArenaCreative,
            "instruction_elo" => ArenaInstruction,
            "hard_prompts_elo" => ArenaHardPrompts,
            "vision_elo" => ArenaVision,
            "imagegen_elo" => ArenaImageGen,
            "code_pass_at_1" => CodePassAt1,
            "asr_wer" => AsrWer,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_elo_is_rounded_integer() {
        assert_eq!(BenchMetric::ArenaOverall.format(1497.4), "1497");
        assert_eq!(BenchMetric::ArenaCoding.format(1535.8765), "1536");
        assert_eq!(BenchMetric::ArenaVision.format(1500.0), "1500");
    }

    #[test]
    fn format_pass_at_1_and_wer_are_percent_one_decimal() {
        assert_eq!(BenchMetric::CodePassAt1.format(61.234), "61.2%");
        assert_eq!(BenchMetric::AsrWer.format(9.878), "9.9%");
    }

    #[test]
    fn key_from_key_roundtrips_all() {
        for m in BenchMetric::all() {
            assert_eq!(BenchMetric::from_key(m.key()), Some(*m));
        }
        assert_eq!(BenchMetric::from_key("nope"), None);
    }

    #[test]
    fn higher_is_better_only_wer_is_false() {
        for m in BenchMetric::all() {
            let expected = !matches!(m, BenchMetric::AsrWer);
            assert_eq!(m.higher_is_better(), expected);
        }
    }

    #[test]
    fn source_mapping() {
        assert_eq!(BenchMetric::ArenaCoding.source(), Source::LmArena);
        assert_eq!(BenchMetric::CodePassAt1.source(), Source::BigCodeBench);
        assert_eq!(BenchMetric::AsrWer.source(), Source::OpenAsr);
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(BenchMetric::ArenaOverall.label(), "Arena Elo");
        assert_eq!(BenchMetric::ArenaCoding.label(), "Coding Elo");
        assert_eq!(BenchMetric::CodePassAt1.label(), "Code Pass@1");
        assert_eq!(BenchMetric::AsrWer.label(), "ASR WER");
    }
}
