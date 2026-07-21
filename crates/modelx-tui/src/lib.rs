//! `modelx-tui` — the ratatui terminal UI for modelx.
//!
//! The crate separates **pure UI logic** ([`state`], [`ui`], [`theme`]) from
//! **I/O** ([`run`]) so the interesting behaviour is unit-testable without a
//! TTY. See `docs/architecture.md` §6.

pub mod compare;
pub mod event;
pub mod run;
pub mod state;
pub mod theme;
pub mod ui;

// Public surface the cli wires against.
pub use event::{AppCommand, AppEvent, ExportDest};
pub use run::{run, RuntimeCtx};
pub use state::{AppState, Focus, Mode, RefreshState};
pub use theme::Theme;

#[cfg(test)]
mod render_tests {
    use super::*;
    use modelx_core::testkit::sample_catalog;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use ratatui::Terminal;

    /// Concatenate every cell's symbol into a single string for assertions.
    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    fn press(state: &mut AppState, code: KeyCode) {
        state.on_key(KeyEvent::new_with_kind(
            code,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));
    }

    fn new_state() -> AppState {
        AppState::new(
            sample_catalog(),
            vec!["models.dev".to_string()],
            "models.dev".to_string(),
        )
    }

    #[test]
    fn renders_provider_and_model_names() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let state = new_state();
        let theme = Theme::default();

        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();

        let text = buffer_text(&terminal);
        // A provider name from the sample catalog.
        assert!(
            text.contains("Anthropic Test") || text.contains("OpenWeights Test"),
            "expected a provider name in the rendered frame"
        );
        // A model name from the sample catalog.
        assert!(
            text.contains("Test Opus") || text.contains("Test Haiku") || text.contains("Qwen3 30B"),
            "expected a model name in the rendered frame"
        );
    }

    /// Select the first two models and open the comparison view.
    fn enter_compare(s: &mut AppState) {
        press(s, KeyCode::Tab); // focus Models
        press(s, KeyCode::Char(' ')); // select model 0
        press(s, KeyCode::Char('j'));
        press(s, KeyCode::Char(' ')); // select model 1
        press(s, KeyCode::Char('c')); // open compare
    }

    /// A small benchmark DB whose entries match the first two sample models in
    /// default (Name-ascending) sort order — GPT OSS 20B (`openai/gpt-oss-20b`)
    /// and Qwen3 30B (`qwen/qwen3-30b`) — by their normalized ids.
    fn bench_db() -> modelx_benchmarks::BenchmarkDb {
        use modelx_benchmarks::{AliasTable, BenchMetric, BenchmarkDb, ProviderData, SourceEntry};
        use std::collections::BTreeMap;
        let entry = |name: &str, elo: f64, pass: f64| {
            let mut scores = BTreeMap::new();
            scores.insert(BenchMetric::ArenaOverall.key().to_string(), elo);
            scores.insert(BenchMetric::CodePassAt1.key().to_string(), pass);
            SourceEntry {
                model_name: name.to_string(),
                organization: None,
                scores,
            }
        };
        let data = ProviderData {
            provider_id: "lmarena".to_string(),
            fetched_at: None,
            entries: vec![
                entry("gpt-oss-20b", 1500.0, 61.2),
                entry("qwen3-30b", 1400.0, 42.0),
            ],
        };
        BenchmarkDb::from_sources(vec![data], AliasTable::embedded())
    }

    #[test]
    fn compare_table_with_benchmarks_shows_sections_and_values() {
        let mut s = new_state().with_benchmarks(Some(bench_db()));
        enter_compare(&mut s);
        let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &s, &theme)).unwrap();
        let text = buffer_text(&terminal);

        assert!(text.contains("Compare"), "compare header should render");
        assert!(text.contains("Metric"), "table header should render");
        // Two labelled sections.
        assert!(text.contains("Specs"), "Specs section header should render");
        assert!(
            text.contains("Benchmarks"),
            "Benchmarks section header should render"
        );
        // A spec (derived) metric row is still present.
        assert!(
            text.contains("Context / $in"),
            "derived spec metric should appear in the table"
        );
        // A benchmark metric label and a real formatted value.
        assert!(
            text.contains("Arena Elo"),
            "a benchmark metric label should appear"
        );
        assert!(
            text.contains("1500"),
            "a real formatted benchmark value should appear"
        );
        // Coverage note in the title.
        assert!(
            text.contains("models matched"),
            "coverage note should render"
        );
    }

    #[test]
    fn compare_table_without_benchmarks_shows_dashes_and_coverage() {
        // No db attached: benchmark cells are `—` and the coverage note says so.
        let mut s = new_state().with_benchmarks(None);
        enter_compare(&mut s);
        let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &s, &theme)).unwrap();
        let text = buffer_text(&terminal);

        assert!(
            text.contains("Benchmarks"),
            "Benchmarks section still renders"
        );
        assert!(text.contains("Arena Elo"), "benchmark labels still render");
        // Benchmark cells are em-dashes with no db.
        assert!(
            text.contains("—"),
            "benchmark cells should be — without a db"
        );
        assert!(
            text.contains("none loaded"),
            "coverage note should indicate no benchmarks are loaded"
        );
    }

    #[test]
    fn models_pane_marks_benchmarked_models() {
        // With a db attached, a benchmarked model (GPT OSS 20B / gpt-oss-20b)
        // should render the ★ marker in the Models pane.
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let state = new_state().with_benchmarks(Some(bench_db()));
        let theme = Theme::default();

        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains('★'),
            "a benchmarked model should render the ★ marker"
        );
    }

    #[test]
    fn compare_bar_view_renders_groups_and_values() {
        let mut s = new_state().with_benchmarks(Some(bench_db()));
        enter_compare(&mut s);
        // Switch to the Bar view.
        press(&mut s, KeyCode::Tab);
        assert_eq!(s.compare().unwrap().view, crate::compare::CompareView::Bar);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &s, &theme)).unwrap();
        let text = buffer_text(&terminal);

        assert!(text.contains("Bar"), "title should show the Bar view label");
        // The group label for the Arena Elo metric.
        assert!(
            text.contains("Arena Elo"),
            "bar chart should render the Arena Elo group label"
        );
        // A formatted Elo value (1500) rendered on/near a bar.
        assert!(
            text.contains("1500"),
            "bar chart should render a formatted benchmark value"
        );
        // Bars are coloured by model; the colour-matched legend spells out the
        // full model names so the bottom strip is the single name reference.
        assert!(
            text.contains("models:") && text.contains("GPT OSS 20B"),
            "bar view should list the full model names in the legend"
        );
    }

    #[test]
    fn compare_bar_view_without_data_shows_message() {
        // No db → no benchmark values → the centred fallback message.
        let mut s = new_state().with_benchmarks(None);
        enter_compare(&mut s);
        press(&mut s, KeyCode::Tab); // → Bar view

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &s, &theme)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("No benchmark data for the selected models"),
            "bar view should show the no-data message"
        );
    }

    #[test]
    fn provider_search_filters_provider_pane() {
        let mut state = new_state();
        // Confirm both providers are present before filtering.
        assert_eq!(state.provider_filtered_count(), 2);
        // `/` from Providers focus opens a provider search.
        press(&mut state, KeyCode::Char('/'));
        for c in "open".chars() {
            press(&mut state, KeyCode::Char(c));
        }
        // The provider list is narrowed to the single matching provider.
        assert_eq!(state.provider_filtered_count(), 1);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();
        let text = buffer_text(&terminal);
        // The search overlay names the Providers pane.
        assert!(
            text.contains("Search providers"),
            "search overlay should target the Providers pane"
        );
        // The matching provider is shown in the pane.
        assert!(
            text.contains("OpenWeights Test"),
            "matching provider should remain visible"
        );
    }

    #[test]
    fn detail_pane_shows_grouped_sections() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let state = new_state();
        let theme = Theme::default();

        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();

        let text = buffer_text(&terminal);
        // The detail pane groups fields under labelled section headers.
        assert!(text.contains("Identity"), "expected an Identity section");
        assert!(text.contains("Pricing"), "expected a Pricing section");
        assert!(
            text.contains("Capabilities"),
            "expected a Capabilities section"
        );
    }

    #[test]
    fn renders_help_overlay() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut state = new_state();
        let theme = Theme::default();

        press(&mut state, KeyCode::Char('?'));
        assert_eq!(state.mode(), Mode::Help);
        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();

        let text = buffer_text(&terminal);
        assert!(text.contains("Help"), "help overlay title should render");
        assert!(
            text.contains("Quit"),
            "help overlay should list the quit binding"
        );
    }

    #[test]
    fn renders_export_overlay() {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut state = new_state();
        let theme = Theme::default();

        press(&mut state, KeyCode::Char('e'));
        assert_eq!(state.mode(), Mode::Export);
        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();

        let text = buffer_text(&terminal);
        assert!(
            text.contains("Export"),
            "export overlay title should render"
        );
        // Step 1 lists fields; a default field label should be visible.
        assert!(
            text.contains("Name") || text.contains("Provider"),
            "export field checklist should render field labels"
        );
    }

    #[test]
    fn renders_empty_catalog_without_panic() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let empty = modelx_core::Catalog {
            source_id: "models.dev".to_string(),
            fetched_at: None,
            providers: vec![],
        };
        let state = AppState::new(
            empty,
            vec!["models.dev".to_string()],
            "models.dev".to_string(),
        );
        let theme = Theme::default();
        terminal.draw(|f| ui::draw(f, &state, &theme)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("No data yet"),
            "empty state hint should render"
        );
    }
}
