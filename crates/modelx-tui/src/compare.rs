//! Pure model + logic for the full-screen **comparison** view.
//!
//! A [`CompareState`] holds a snapshot of the selected models plus the table
//! scroll offset. The view is **table-only**: metric rows × model columns, with
//! two labelled sections — *Specs* (the numeric spec/derived metrics) and
//! *Benchmarks* (per-[`BenchMetric`] scores looked up from a [`BenchmarkDb`]).
//! It contains no rendering and no I/O; [`crate::ui`] reads it to draw the table.

use modelx_benchmarks::{BenchMetric, BenchmarkDb};
use modelx_core::{Field, Model};

/// A comparable **spec** metric — either a raw numeric [`Field`] or a derived
/// ratio. Benchmark rows are handled separately via [`BenchMetric`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    /// A raw numeric field (context, output limit, or a cost).
    Field(Field),
    /// Context window per dollar of input cost (higher = better value).
    ContextPerDollar,
    /// Output-token limit per dollar of output cost (higher = better value).
    OutputPerDollar,
}

/// All comparable spec metrics, in display order (raw numeric fields, then
/// derived).
pub const METRICS: &[Metric] = &[
    Metric::Field(Field::ContextLimit),
    Metric::Field(Field::OutputLimit),
    Metric::Field(Field::InputCost),
    Metric::Field(Field::OutputCost),
    Metric::Field(Field::CacheReadCost),
    Metric::Field(Field::CacheWriteCost),
    Metric::Field(Field::ReasoningCost),
    Metric::ContextPerDollar,
    Metric::OutputPerDollar,
];

impl Metric {
    /// Short human label for the metric-column cell.
    pub fn label(&self) -> &'static str {
        match self {
            Metric::Field(f) => f.label(),
            Metric::ContextPerDollar => "Context / $in",
            Metric::OutputPerDollar => "Output / $out",
        }
    }

    /// This metric's value for a model, if computable.
    pub fn value(&self, m: &Model) -> Option<f64> {
        match self {
            Metric::Field(f) => f.value(m).as_f64(),
            Metric::ContextPerDollar => ratio(
                Field::ContextLimit.value(m).as_f64(),
                Field::InputCost.value(m).as_f64(),
            ),
            Metric::OutputPerDollar => ratio(
                Field::OutputLimit.value(m).as_f64(),
                Field::OutputCost.value(m).as_f64(),
            ),
        }
    }

    /// Whether a larger value is "better" (drives best/worst highlighting).
    /// Costs are better when lower; everything else better when higher.
    pub fn higher_is_better(&self) -> bool {
        !matches!(self, Metric::Field(f) if f.is_cost())
    }

    /// Whether this metric is a price (rendered with a `$`).
    pub fn is_cost(&self) -> bool {
        matches!(self, Metric::Field(f) if f.is_cost())
    }

    /// Format a value for display (money, or a human count like `1.2M`).
    pub fn format(&self, v: f64) -> String {
        if self.is_cost() {
            format_money(v)
        } else {
            format_count(v)
        }
    }
}

/// `numerator / denominator`, or `None` if either is missing or the
/// denominator is zero (e.g. a free/unpriced model).
fn ratio(numerator: Option<f64>, denominator: Option<f64>) -> Option<f64> {
    match (numerator, denominator) {
        (Some(n), Some(d)) if d > 0.0 => Some(n / d),
        _ => None,
    }
}

/// The total number of metric rows in the transposed table: the two section
/// headers plus every spec metric plus every benchmark metric.
pub fn total_rows() -> usize {
    // Specs header + spec metrics + Benchmarks header + benchmark metrics.
    1 + METRICS.len() + 1 + BenchMetric::all().len()
}

/// Which of the two comparison sub-views is active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareView {
    /// The transposed specs + benchmarks table (default).
    Table,
    /// A grouped bar chart of the three benchmark Elo metrics.
    Bar,
}

/// The three benchmark metrics the Bar view can chart, in display order.
pub const BAR_METRICS: &[BenchMetric] = &[
    BenchMetric::ArenaOverall,
    BenchMetric::ArenaCoding,
    BenchMetric::ArenaMath,
];

/// Full comparison state: the model snapshot plus the table scroll offset and
/// the active sub-view / bar-chart configuration.
#[derive(Clone, Debug)]
pub struct CompareState {
    /// Snapshot of the compared models (stable keys), catalog order.
    pub models: Vec<modelx_core::ModelRef>,
    /// Vertical scroll offset into the transposed metric rows.
    pub table_scroll: usize,
    /// The active sub-view (Table default).
    pub view: CompareView,
    /// Which benchmark metrics are shown in the Bar view. Never empty; defaults
    /// to all of [`BAR_METRICS`].
    pub bar_metrics: Vec<BenchMetric>,
}

impl CompareState {
    pub fn new(models: Vec<modelx_core::ModelRef>) -> Self {
        CompareState {
            models,
            table_scroll: 0,
            view: CompareView::Table,
            bar_metrics: BAR_METRICS.to_vec(),
        }
    }

    /// Toggle Table ↔ Bar.
    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            CompareView::Table => CompareView::Bar,
            CompareView::Bar => CompareView::Table,
        };
    }

    /// Toggle a benchmark metric in the Bar view. Ignores a toggle that would
    /// leave zero metrics selected. Selected metrics stay in [`BAR_METRICS`]
    /// display order.
    pub fn toggle_bar_metric(&mut self, metric: BenchMetric) {
        if let Some(pos) = self.bar_metrics.iter().position(|m| *m == metric) {
            if self.bar_metrics.len() > 1 {
                self.bar_metrics.remove(pos);
            }
        } else {
            self.bar_metrics.push(metric);
            // Keep canonical display order.
            self.bar_metrics.sort_by_key(|m| {
                BAR_METRICS
                    .iter()
                    .position(|b| b == m)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    /// Whether a given bar metric is currently selected.
    pub fn bar_metric_on(&self, metric: BenchMetric) -> bool {
        self.bar_metrics.contains(&metric)
    }

    /// Scroll the metric list by one row, clamped to the row count.
    pub fn scroll_table(&mut self, forward: bool) {
        if forward {
            self.table_scroll = (self.table_scroll + 1).min(total_rows().saturating_sub(1));
        } else {
            self.table_scroll = self.table_scroll.saturating_sub(1);
        }
    }

    /// Scroll the metric list by `n` rows (for PageUp/PageDown), clamped.
    pub fn scroll_table_by(&mut self, forward: bool, n: usize) {
        if forward {
            self.table_scroll = (self.table_scroll + n).min(total_rows().saturating_sub(1));
        } else {
            self.table_scroll = self.table_scroll.saturating_sub(n);
        }
    }
}

/// The best and worst values of a spec `metric` across `models` (ignoring
/// missing). Returns `(best, worst)` where "best" respects
/// [`Metric::higher_is_better`].
pub fn best_worst(models: &[&Model], metric: Metric) -> (Option<f64>, Option<f64>) {
    best_worst_from(
        models.iter().filter_map(|m| metric.value(m)).collect(),
        metric.higher_is_better(),
    )
}

/// The best and worst of a set of already-collected values, respecting
/// `higher_is_better`. Used by both spec and benchmark rows.
pub fn best_worst_from(vals: Vec<f64>, higher_is_better: bool) -> (Option<f64>, Option<f64>) {
    if vals.len() < 2 {
        return (None, None);
    }
    let max = vals.iter().cloned().fold(f64::MIN, f64::max);
    let min = vals.iter().cloned().fold(f64::MAX, f64::min);
    if (max - min).abs() < f64::EPSILON {
        return (None, None); // all equal — nothing to highlight
    }
    if higher_is_better {
        (Some(max), Some(min))
    } else {
        (Some(min), Some(max))
    }
}

/// Render the comparison as a GitHub-flavoured Markdown table
/// (metric rows × model columns) for copy-to-clipboard.
///
/// Includes the spec rows and — when a [`BenchmarkDb`] is supplied — a
/// benchmark row per [`BenchMetric`]. Cells with no value render as `—`.
pub fn markdown_table(models: &[&Model], db: Option<&BenchmarkDb>) -> String {
    let mut out = String::new();
    // Header: | Metric | Model A | Model B | ...
    out.push_str("| Metric |");
    for m in models {
        out.push_str(&format!(" {} |", m.name.replace('|', r"\|")));
    }
    out.push('\n');
    out.push_str("| --- |");
    for _ in models {
        out.push_str(" --- |");
    }
    out.push('\n');

    // Specs.
    for metric in METRICS {
        out.push_str(&format!("| {} |", metric.label()));
        for m in models {
            let cell = metric
                .value(m)
                .map(|v| metric.format(v))
                .unwrap_or_else(|| "—".to_string());
            out.push_str(&format!(" {cell} |"));
        }
        out.push('\n');
    }

    // Benchmarks. Look each model up once and reuse across metric rows.
    let matches: Vec<Option<modelx_benchmarks::BenchMatch>> =
        models.iter().map(|m| db.map(|d| d.lookup(m))).collect();
    for metric in BenchMetric::all() {
        out.push_str(&format!("| {} |", metric.label()));
        for match_ in &matches {
            let cell = match_
                .as_ref()
                .and_then(|mt| mt.scores.get(metric).copied())
                .map(|v| metric.format(v))
                .unwrap_or_else(|| "—".to_string());
            out.push_str(&format!(" {cell} |"));
        }
        out.push('\n');
    }

    out
}

/// Format a price with a `$` prefix and trimmed trailing zeros.
pub fn format_money(v: f64) -> String {
    // Up to 4 decimals for sub-dollar cache prices, trimmed.
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    format!("${s}")
}

/// Format a large count as a human-friendly `1.2M` / `256K` string.
pub fn format_count(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        trim_unit(v / 1e9, "B")
    } else if a >= 1e6 {
        trim_unit(v / 1e6, "M")
    } else if a >= 1e3 {
        trim_unit(v / 1e3, "K")
    } else {
        // Small counts / ratios: show up to one decimal, trimmed.
        trim_unit(v, "")
    }
}

fn trim_unit(v: f64, unit: &str) -> String {
    let s = format!("{v:.1}");
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    format!("{s}{unit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelx_benchmarks::{AliasTable, ProviderData, SourceEntry};
    use modelx_core::testkit::sample_catalog;
    use std::collections::BTreeMap;

    fn models() -> Vec<Model> {
        sample_catalog()
            .providers
            .into_iter()
            .flat_map(|p| p.models)
            .collect()
    }

    /// A tiny DB whose one entry matches `model-opus` by normalized id.
    fn tiny_db() -> BenchmarkDb {
        let mut scores = BTreeMap::new();
        scores.insert(BenchMetric::ArenaOverall.key().to_string(), 1500.0);
        scores.insert(BenchMetric::CodePassAt1.key().to_string(), 61.2);
        let data = ProviderData {
            provider_id: "lmarena".to_string(),
            fetched_at: None,
            entries: vec![SourceEntry {
                model_name: "model-opus".to_string(),
                organization: None,
                scores,
            }],
        };
        BenchmarkDb::from_sources(vec![data], AliasTable::embedded())
    }

    #[test]
    fn format_money_trims() {
        assert_eq!(format_money(3.0), "$3");
        assert_eq!(format_money(0.5), "$0.5");
        assert_eq!(format_money(0.075), "$0.075");
    }

    #[test]
    fn format_count_human() {
        assert_eq!(format_count(1_000_000.0), "1M");
        assert_eq!(format_count(200_000.0), "200K");
        assert_eq!(format_count(131_072.0), "131.1K");
        assert_eq!(format_count(42.0), "42");
    }

    #[test]
    fn cost_metric_lower_is_better() {
        assert!(!Metric::Field(Field::InputCost).higher_is_better());
        assert!(Metric::Field(Field::ContextLimit).higher_is_better());
        assert!(Metric::ContextPerDollar.higher_is_better());
    }

    #[test]
    fn derived_metric_handles_zero_and_missing_cost() {
        let ms = models();
        // model-qwen has cost: None → context-per-dollar is None.
        let has_none = ms
            .iter()
            .any(|m| Metric::ContextPerDollar.value(m).is_none());
        assert!(
            has_none,
            "expected at least one model with no derived value"
        );
    }

    #[test]
    fn best_worst_picks_extremes() {
        let ms = models();
        let refs: Vec<&Model> = ms.iter().collect();
        let (best, worst) = best_worst(&refs, Metric::Field(Field::InputCost));
        // For cost, best = lowest. If we have varied costs, best <= worst.
        if let (Some(b), Some(w)) = (best, worst) {
            assert!(b <= w, "cost best ({b}) should be <= worst ({w})");
        }
    }

    #[test]
    fn best_worst_from_respects_orientation() {
        // Lower-is-better (like ASR WER): best is the minimum.
        let (best, worst) = best_worst_from(vec![10.0, 5.0, 8.0], false);
        assert_eq!(best, Some(5.0));
        assert_eq!(worst, Some(10.0));
        // Higher-is-better (like Arena Elo): best is the maximum.
        let (best, worst) = best_worst_from(vec![1500.0, 1400.0], true);
        assert_eq!(best, Some(1500.0));
        assert_eq!(worst, Some(1400.0));
    }

    #[test]
    fn total_rows_counts_headers_and_metrics() {
        assert_eq!(
            total_rows(),
            1 + METRICS.len() + 1 + BenchMetric::all().len()
        );
    }

    #[test]
    fn scroll_table_clamps() {
        let mut cs = CompareState::new(vec![]);
        for _ in 0..1000 {
            cs.scroll_table(true);
        }
        assert_eq!(cs.table_scroll, total_rows() - 1);
        cs.scroll_table_by(false, 1000);
        assert_eq!(cs.table_scroll, 0);
    }

    #[test]
    fn markdown_table_has_header_specs_and_benchmarks() {
        let ms = models();
        let refs: Vec<&Model> = ms.iter().take(2).collect();
        let db = tiny_db();
        let md = markdown_table(&refs, Some(&db));
        assert!(md.contains("| Metric |"));
        assert!(md.contains("Context")); // a spec metric row label
        assert!(md.contains("Arena Elo")); // a benchmark metric row label
                                           // Two header lines + all spec rows + all benchmark rows.
        assert_eq!(
            md.lines().count(),
            2 + METRICS.len() + BenchMetric::all().len()
        );
    }

    #[test]
    fn toggle_bar_metric_never_empties_and_keeps_order() {
        let mut cs = CompareState::new(vec![]);
        assert_eq!(cs.bar_metrics.len(), 3);
        cs.toggle_bar_metric(BenchMetric::ArenaCoding);
        assert!(!cs.bar_metric_on(BenchMetric::ArenaCoding));
        assert_eq!(cs.bar_metrics.len(), 2);
        // Re-adding restores canonical order (Arena, Coding, Math).
        cs.toggle_bar_metric(BenchMetric::ArenaCoding);
        assert_eq!(cs.bar_metrics, BAR_METRICS.to_vec());
        // Turning all but one off, then the last is a no-op.
        cs.toggle_bar_metric(BenchMetric::ArenaCoding);
        cs.toggle_bar_metric(BenchMetric::ArenaMath);
        assert_eq!(cs.bar_metrics, vec![BenchMetric::ArenaOverall]);
        cs.toggle_bar_metric(BenchMetric::ArenaOverall);
        assert_eq!(
            cs.bar_metrics,
            vec![BenchMetric::ArenaOverall],
            "cannot empty the selection"
        );
    }

    #[test]
    fn toggle_view_flips() {
        let mut cs = CompareState::new(vec![]);
        assert_eq!(cs.view, CompareView::Table);
        cs.toggle_view();
        assert_eq!(cs.view, CompareView::Bar);
        cs.toggle_view();
        assert_eq!(cs.view, CompareView::Table);
    }

    #[test]
    fn markdown_table_without_db_dashes_benchmarks() {
        let ms = models();
        let refs: Vec<&Model> = ms.iter().take(2).collect();
        let md = markdown_table(&refs, None);
        // Benchmark labels still present, but every benchmark cell is a dash.
        assert!(md.contains("Arena Elo"));
        let arena_line = md
            .lines()
            .find(|l| l.starts_with("| Arena Elo |"))
            .expect("arena elo row");
        assert!(
            arena_line.contains("—"),
            "no-db benchmark cells should be —"
        );
    }
}
