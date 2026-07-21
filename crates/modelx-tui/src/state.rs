//! Pure UI state machine for the modelx TUI.
//!
//! [`AppState`] holds *all* interactive state and contains **no** I/O, no
//! threads, and no ratatui rendering. It is driven by two entry points:
//!
//! - [`AppState::on_key`] — feed a key press, mutate UI state, and optionally
//!   return an [`AppCommand`] describing a side effect for the runtime.
//! - [`AppState::apply`] — fold an external [`AppEvent`] (refresh result, tick)
//!   into the state.
//!
//! The renderer ([`crate::ui`]) reads state through the accessors at the bottom.

use std::collections::HashSet;

use modelx_core::{run_query, Catalog, Field, Model, ModelRef, Query};
use modelx_export::Format;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use modelx_benchmarks::BenchMetric;

use crate::compare::{self, CompareState, CompareView};
use crate::event::{AppCommand, AppEvent, ExportDest};

/// Which of the three panes has keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Providers,
    Models,
    Detail,
}

/// Which pane the active `/` search is targeting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchTarget {
    Providers,
    Models,
}

/// The current interaction mode (Normal or one of the overlays).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Search,
    Sort,
    Filter,
    Export,
    SourcePicker,
    Help,
}

/// State of the background refresh, surfaced in the status bar.
#[derive(Clone, Debug, PartialEq)]
pub enum RefreshState {
    Idle,
    Refreshing,
    Ok(i64),
    Failed(String),
}

/// A single row in the keymap table — powers both the dispatcher's
/// documentation and the Help overlay.
#[derive(Clone, Copy, Debug)]
pub struct KeyBinding {
    pub keys: &'static str,
    pub description: &'static str,
}

/// The full keymap, in one place so the Help overlay never drifts from the
/// implemented behaviour.
pub const KEYMAP: &[KeyBinding] = &[
    KeyBinding {
        keys: "q / Ctrl-C",
        description: "Quit",
    },
    KeyBinding {
        keys: "Tab / h / l",
        description: "Cycle focus (Providers/Models/Detail)",
    },
    KeyBinding {
        keys: "j / k / ↓ / ↑",
        description: "Move in the focused list",
    },
    KeyBinding {
        keys: "g / G",
        description: "Jump to top / bottom",
    },
    KeyBinding {
        keys: "/",
        description: "Search (fuzzy, live)",
    },
    KeyBinding {
        keys: "s",
        description: "Sort",
    },
    KeyBinding {
        keys: "f",
        description: "Filter",
    },
    KeyBinding {
        keys: "space",
        description: "Toggle-select model under cursor",
    },
    KeyBinding {
        keys: "a",
        description: "Select all in current view",
    },
    KeyBinding {
        keys: "A",
        description: "Clear selection",
    },
    KeyBinding {
        keys: "c",
        description: "Compare selected models (2+): specs + benchmarks",
    },
    KeyBinding {
        keys: "Tab (compare)",
        description: "Switch compare view Table/Bar",
    },
    KeyBinding {
        keys: "1/2/3 (bar)",
        description: "Toggle Arena / Coding / Math metric",
    },
    KeyBinding {
        keys: "y",
        description: "Copy focused value / model summary",
    },
    KeyBinding {
        keys: "Y",
        description: "Copy focused model as pretty JSON",
    },
    KeyBinding {
        keys: "e",
        description: "Export selection (wizard)",
    },
    KeyBinding {
        keys: "r",
        description: "Refresh active source",
    },
    KeyBinding {
        keys: "S",
        description: "Switch data source",
    },
    KeyBinding {
        keys: "J (Detail)",
        description: "Toggle raw-JSON detail view",
    },
    KeyBinding {
        keys: "?",
        description: "Help",
    },
    KeyBinding {
        keys: "Esc",
        description: "Close overlay / clear search",
    },
];

/// Sortable fields exposed by the Sort overlay (a sensible subset of
/// [`Field::all`]).
const SORT_FIELDS: &[Field] = &[
    Field::Name,
    Field::ProviderName,
    Field::ContextLimit,
    Field::InputCost,
    Field::OutputCost,
    Field::ReleaseDate,
    Field::OpenWeights,
    Field::Reasoning,
];

/// Input modalities cycled by the Filter overlay.
const MODALITY_CYCLE: &[Option<&str>] = &[None, Some("text"), Some("image"), Some("audio")];

/// Default fields preselected in the Export wizard.
const DEFAULT_EXPORT_FIELDS: &[Field] = &[
    Field::ProviderName,
    Field::Id,
    Field::Name,
    Field::ContextLimit,
    Field::InputCost,
    Field::OutputCost,
];

/// Which step of the export wizard the user is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStep {
    Fields,
    Format,
    Destination,
}

/// Sub-state for the Export wizard overlay.
#[derive(Clone, Debug)]
pub struct ExportWizard {
    pub step: ExportStep,
    /// Cursor within the current step's list.
    pub cursor: usize,
    /// Selected fields (checklist over `Field::all()`).
    pub fields: HashSet<Field>,
    /// Chosen format.
    pub format: Format,
    /// Destination choice: 0 = Clipboard, 1 = File.
    pub dest_choice: usize,
    /// Editable file path (used when dest_choice == 1).
    pub file_path: String,
    /// Set when the user tried to advance with no fields chosen.
    pub error: Option<String>,
}

impl ExportWizard {
    fn new() -> Self {
        let fields: HashSet<Field> = DEFAULT_EXPORT_FIELDS.iter().copied().collect();
        ExportWizard {
            step: ExportStep::Fields,
            cursor: 0,
            fields,
            format: Format::PlainList,
            dest_choice: 0,
            file_path: format!("modelx-export.{}", Format::PlainList.ext()),
            error: None,
        }
    }
}

/// The complete, pure UI state.
pub struct AppState {
    catalog: Catalog,
    query: Query,
    /// Result of `run_query`, as stable keys so selection + cursor survive a
    /// catalog hot-swap.
    view: Vec<ModelRef>,

    focus: Focus,
    mode: Mode,

    /// Cursor into the providers pane (0 == synthetic "All providers").
    provider_cursor: usize,
    /// Cursor into the models pane (index into `view`).
    model_cursor: usize,
    /// Cursor into the detail pane (index into the rendered detail lines).
    detail_cursor: usize,
    /// Whether the detail pane shows raw JSON instead of the field list.
    detail_raw: bool,

    /// Export selection set (stable model keys).
    selection: HashSet<ModelRef>,

    refresh: RefreshState,
    /// A transient toast: (message, ticks remaining).
    toast: Option<(String, u8)>,
    /// Spinner frame index, advanced on tick while refreshing.
    spinner: usize,

    source_ids: Vec<String>,
    active_source: String,

    // --- overlay sub-state ---
    /// Cursor in the Sort overlay.
    sort_cursor: usize,
    /// Cursor in the Filter overlay.
    filter_cursor: usize,
    /// Numeric entry buffer for `min_context` in the Filter overlay.
    filter_context_buf: String,
    /// Cursor in the SourcePicker overlay.
    source_cursor: usize,
    /// Export wizard sub-state.
    export: ExportWizard,

    /// When `Some`, the full-screen comparison view is active and replaces the
    /// 3-pane browser. Overlays (export, help) still draw on top.
    compare: Option<CompareState>,

    /// Optional benchmark database, loaded cache-only at startup.
    bench_db: Option<modelx_benchmarks::BenchmarkDb>,

    /// Precomputed set of catalog models that have benchmark data
    /// (`lookup(m).matched_any`). Built once in [`AppState::with_benchmarks`]
    /// so the renderer can test membership in O(1) instead of calling
    /// `db.lookup` per row per frame.
    benchmarked: HashSet<ModelRef>,

    /// Case-insensitive substring filter for the Providers pane (Feature 1).
    provider_search: String,
    /// Which pane the active `/` search targets. Set when Search mode opens.
    search_target: SearchTarget,
}

/// Number of interactive rows in the Filter overlay.
const FILTER_ROWS: usize = 5; // reasoning, tool_call, open_weights, input_modality, min_context

/// Rows scrolled per PageUp/PageDown in the comparison table.
const COMPARE_PAGE: usize = 5;

impl AppState {
    /// Build a fresh state from a catalog and the set of known source ids.
    pub fn new(catalog: Catalog, source_ids: Vec<String>, active_source: String) -> Self {
        let query = Query::default();
        let view = compute_view(&catalog, &query);
        let source_cursor = source_ids
            .iter()
            .position(|s| s == &active_source)
            .unwrap_or(0);
        AppState {
            catalog,
            query,
            view,
            focus: Focus::Providers,
            mode: Mode::Normal,
            provider_cursor: 0,
            model_cursor: 0,
            detail_cursor: 0,
            detail_raw: false,
            selection: HashSet::new(),
            refresh: RefreshState::Idle,
            toast: None,
            spinner: 0,
            source_ids,
            active_source,
            sort_cursor: 0,
            filter_cursor: 0,
            filter_context_buf: String::new(),
            source_cursor,
            export: ExportWizard::new(),
            compare: None,
            bench_db: None,
            benchmarked: HashSet::new(),
            provider_search: String::new(),
            search_target: SearchTarget::Models,
        }
    }

    /// Attach a benchmark database loaded from cache (cache-only, never blocks
    /// the network at TUI startup). Follows the builder pattern so it can be
    /// chained on [`AppState::new`].
    ///
    /// When a DB is attached, this precomputes — **once** — the set of every
    /// catalog model that has benchmark data (`lookup(m).matched_any`). This is
    /// ~5–6k lookups at startup, in exchange for O(1) membership tests during
    /// rendering (see [`ModelRow::has_benchmark`]).
    pub fn with_benchmarks(mut self, db: Option<modelx_benchmarks::BenchmarkDb>) -> Self {
        self.benchmarked = match db.as_ref() {
            Some(db) => self
                .catalog
                .all_models()
                .filter(|m| db.lookup(m).matched_any)
                .map(|m| m.key())
                .collect(),
            None => HashSet::new(),
        };
        self.bench_db = db;
        self
    }

    // -----------------------------------------------------------------------
    // Event folding
    // -----------------------------------------------------------------------

    /// Fold an external event into the state.
    pub fn apply(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::RefreshStarted => {
                self.refresh = RefreshState::Refreshing;
            }
            AppEvent::RefreshDone(catalog) => {
                // Ignore a refresh that finished for a source we've since
                // switched away from — otherwise its data would clobber the
                // now-active source's view.
                if catalog.source_id != self.active_source {
                    return;
                }
                let ts = catalog.fetched_at.unwrap_or(0);
                self.swap_catalog(catalog);
                self.refresh = RefreshState::Ok(ts);
                self.set_toast("updated");
            }
            AppEvent::RefreshFailed(msg) => {
                self.refresh = RefreshState::Failed(msg.clone());
                self.set_toast(format!("refresh failed: {msg}"));
            }
            AppEvent::Tick => self.tick(),
        }
    }

    /// Hot-swap the catalog, preserving selection (by `ModelRef`) and clamping
    /// cursors against the recomputed view.
    fn swap_catalog(&mut self, catalog: Catalog) {
        self.catalog = catalog;
        // Drop any selection entries that no longer exist in the new catalog.
        self.selection.retain(|k| self.catalog.find(k).is_some());
        // Recompute the benchmarked-model set against the new catalog so the
        // Models-pane markers stay correct after a hot-swap.
        if let Some(db) = self.bench_db.as_ref() {
            self.benchmarked = self
                .catalog
                .all_models()
                .filter(|m| db.lookup(m).matched_any)
                .map(|m| m.key())
                .collect();
        }
        self.recompute_view();
    }

    /// Decay the toast and advance the spinner.
    pub fn tick(&mut self) {
        if let Some((_, ticks)) = self.toast.as_mut() {
            if *ticks > 0 {
                *ticks -= 1;
            }
        }
        if matches!(self.toast, Some((_, 0))) {
            self.toast = None;
        }
        if matches!(self.refresh, RefreshState::Refreshing) {
            self.spinner = self.spinner.wrapping_add(1);
        }
    }

    // -----------------------------------------------------------------------
    // Key dispatch
    // -----------------------------------------------------------------------

    /// Handle a key press. Mutates UI state and may return a side-effect command.
    pub fn on_key(&mut self, key: KeyEvent) -> Option<AppCommand> {
        // Ctrl-C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Some(AppCommand::Quit);
        }

        match self.mode {
            // In Normal mode the comparison view (when open) takes over input;
            // overlays below still route to their own handlers on top of it.
            Mode::Normal if self.compare.is_some() => self.on_key_compare(key),
            Mode::Normal => self.on_key_normal(key),
            Mode::Search => self.on_key_search(key),
            Mode::Sort => self.on_key_sort(key),
            Mode::Filter => self.on_key_filter(key),
            Mode::Export => self.on_key_export(key),
            Mode::SourcePicker => self.on_key_source_picker(key),
            Mode::Help => self.on_key_help(key),
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Char('q') => return Some(AppCommand::Quit),
            KeyCode::Tab | KeyCode::Char('l') => self.cycle_focus(true),
            KeyCode::BackTab | KeyCode::Char('h') => self.cycle_focus(false),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('g') => self.cursor_to_top(),
            KeyCode::Char('G') => self.cursor_to_bottom(),
            KeyCode::Char('/') => {
                // Context-aware: `/` in the Providers pane searches providers;
                // anywhere else it searches models.
                self.search_target = if self.focus == Focus::Providers {
                    SearchTarget::Providers
                } else {
                    SearchTarget::Models
                };
                self.mode = Mode::Search;
            }
            KeyCode::Char('s') => {
                self.sort_cursor = SORT_FIELDS
                    .iter()
                    .position(|f| *f == self.query.sort.field)
                    .unwrap_or(0);
                self.mode = Mode::Sort;
            }
            KeyCode::Char('f') => {
                self.filter_cursor = 0;
                self.filter_context_buf = self
                    .query
                    .filters
                    .min_context
                    .map(|c| c.to_string())
                    .unwrap_or_default();
                self.mode = Mode::Filter;
            }
            KeyCode::Char(' ') => self.toggle_select_under_cursor(),
            KeyCode::Char('a') => self.select_all_in_view(),
            KeyCode::Char('A') => self.selection.clear(),
            KeyCode::Char('y') => return self.copy_focused_value(),
            KeyCode::Char('Y') => return self.copy_focused_json(),
            KeyCode::Char('e') => {
                self.export = ExportWizard::new();
                self.mode = Mode::Export;
            }
            KeyCode::Char('c') => self.enter_compare(),
            KeyCode::Char('r') => return Some(AppCommand::Refresh),
            KeyCode::Char('S') => {
                self.source_cursor = self
                    .source_ids
                    .iter()
                    .position(|s| s == &self.active_source)
                    .unwrap_or(0);
                self.mode = Mode::SourcePicker;
            }
            KeyCode::Char('J') if self.focus == Focus::Detail => {
                self.detail_raw = !self.detail_raw;
                self.detail_cursor = 0;
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
            }
            // In Normal mode, Esc clears an active search (models or providers).
            KeyCode::Esc if !self.query.search.is_empty() => {
                self.query.search.clear();
                self.recompute_view();
            }
            KeyCode::Esc if !self.provider_search.is_empty() => {
                self.provider_search.clear();
                self.recompute_view();
            }
            _ => {}
        }
        None
    }

    // --- Comparison view ---

    /// Enter the full-screen comparison view for the current selection.
    /// Requires at least two selected models; otherwise shows a hint.
    fn enter_compare(&mut self) {
        // Catalog-ordered snapshot of the selected models (stable, includes
        // models currently filtered out of the view).
        let models: Vec<ModelRef> = self
            .catalog
            .all_models()
            .map(|m| m.key())
            .filter(|k| self.selection.contains(k))
            .collect();
        if models.len() < 2 {
            self.set_toast("select 2+ models (space) to compare");
            return;
        }
        let total = models.len();
        // Warn when some (or all) compared models lack benchmark data.
        match self.bench_db.as_ref() {
            None => {
                self.set_toast("⚠ benchmark data not loaded — run `modelx refresh`");
            }
            Some(db) => {
                let missing = models
                    .iter()
                    .filter_map(|k| self.catalog.find(k))
                    .filter(|m| !db.lookup(m).matched_any)
                    .count();
                if missing > 0 {
                    self.set_toast(format!(
                        "⚠ {missing} of {total} selected models have no benchmark data"
                    ));
                }
            }
        }
        self.compare = Some(CompareState::new(models));
    }

    fn on_key_compare(&mut self, key: KeyEvent) -> Option<AppCommand> {
        // The compare view has two sub-views (Table / Bar). Tab/BackTab switch;
        // scroll keys apply to the Table view; number keys toggle bar metrics.
        let view = self.compare.as_ref()?.view;
        match key.code {
            KeyCode::Char('q') => return Some(AppCommand::Quit),
            KeyCode::Esc | KeyCode::Char('c') => self.compare = None,
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(c) = self.compare.as_mut() {
                    c.toggle_view();
                }
            }
            // Scroll keys only affect the Table view.
            KeyCode::Up | KeyCode::Char('k') if view == CompareView::Table => {
                if let Some(c) = self.compare.as_mut() {
                    c.scroll_table(false);
                }
            }
            KeyCode::Down | KeyCode::Char('j') if view == CompareView::Table => {
                if let Some(c) = self.compare.as_mut() {
                    c.scroll_table(true);
                }
            }
            KeyCode::PageUp if view == CompareView::Table => {
                if let Some(c) = self.compare.as_mut() {
                    c.scroll_table_by(false, COMPARE_PAGE);
                }
            }
            KeyCode::PageDown if view == CompareView::Table => {
                if let Some(c) = self.compare.as_mut() {
                    c.scroll_table_by(true, COMPARE_PAGE);
                }
            }
            // Bar-view metric toggles (1=Arena, 2=Coding, 3=Math).
            KeyCode::Char('1') if view == CompareView::Bar => {
                if let Some(c) = self.compare.as_mut() {
                    c.toggle_bar_metric(BenchMetric::ArenaOverall);
                }
            }
            KeyCode::Char('2') if view == CompareView::Bar => {
                if let Some(c) = self.compare.as_mut() {
                    c.toggle_bar_metric(BenchMetric::ArenaCoding);
                }
            }
            KeyCode::Char('3') if view == CompareView::Bar => {
                if let Some(c) = self.compare.as_mut() {
                    c.toggle_bar_metric(BenchMetric::ArenaMath);
                }
            }
            KeyCode::Char('y') => {
                let md = compare::markdown_table(&self.compare_models(), self.bench_db.as_ref());
                self.set_toast("copied comparison table");
                return Some(AppCommand::CopyText(md));
            }
            KeyCode::Char('e') => {
                self.export = ExportWizard::new();
                self.mode = Mode::Export;
            }
            KeyCode::Char('?') => self.mode = Mode::Help,
            _ => {}
        }
        None
    }

    // --- Search overlay ---

    fn on_key_search(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Esc => {
                // Esc clears only the buffer this search targets.
                self.search_buffer_mut().clear();
                self.recompute_view();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.search_buffer_mut().pop();
                self.recompute_view();
            }
            KeyCode::Char(c) => {
                self.search_buffer_mut().push(c);
                self.recompute_view();
            }
            _ => {}
        }
        None
    }

    /// A mutable handle to whichever search buffer the active search targets.
    fn search_buffer_mut(&mut self) -> &mut String {
        match self.search_target {
            SearchTarget::Providers => &mut self.provider_search,
            SearchTarget::Models => &mut self.query.search,
        }
    }

    /// The provider list after applying the `provider_search` filter, as
    /// `(index_into_catalog_providers, provider)` pairs. Providers are kept when
    /// their id **or** name contains the query (case-insensitive substring).
    fn filtered_provider_indices(&self) -> Vec<usize> {
        if self.provider_search.is_empty() {
            return (0..self.catalog.providers.len()).collect();
        }
        let q = self.provider_search.to_lowercase();
        self.catalog
            .providers
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id.to_lowercase().contains(&q) || p.name.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    // --- Sort overlay ---

    fn on_key_sort(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.sort_cursor + 1 < SORT_FIELDS.len() {
                    self.sort_cursor += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.sort_cursor = self.sort_cursor.saturating_sub(1);
            }
            KeyCode::Char('d') => {
                self.query.sort.descending = !self.query.sort.descending;
                self.recompute_view();
            }
            KeyCode::Enter => {
                let field = SORT_FIELDS[self.sort_cursor];
                if self.query.sort.field == field {
                    // Re-selecting the current field toggles direction.
                    self.query.sort.descending = !self.query.sort.descending;
                } else {
                    self.query.sort.field = field;
                }
                self.recompute_view();
                self.mode = Mode::Normal;
            }
            _ => {}
        }
        None
    }

    // --- Filter overlay ---

    fn on_key_filter(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.commit_context_buf();
                self.recompute_view();
                self.mode = Mode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.filter_cursor + 1 < FILTER_ROWS {
                    self.filter_cursor += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.filter_cursor = self.filter_cursor.saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                self.toggle_filter_row();
                self.recompute_view();
            }
            KeyCode::Char(c) if c.is_ascii_digit() && self.filter_cursor == 4 => {
                self.filter_context_buf.push(c);
            }
            KeyCode::Backspace if self.filter_cursor == 4 => {
                self.filter_context_buf.pop();
            }
            _ => {}
        }
        None
    }

    /// Toggle the currently-selected filter row between its states.
    fn toggle_filter_row(&mut self) {
        match self.filter_cursor {
            0 => self.query.filters.reasoning = cycle_tri(self.query.filters.reasoning),
            1 => self.query.filters.tool_call = cycle_tri(self.query.filters.tool_call),
            2 => self.query.filters.open_weights = cycle_tri(self.query.filters.open_weights),
            3 => {
                let cur = self.query.filters.input_modality.as_deref();
                let idx = MODALITY_CYCLE.iter().position(|m| *m == cur).unwrap_or(0);
                let next = MODALITY_CYCLE[(idx + 1) % MODALITY_CYCLE.len()];
                self.query.filters.input_modality = next.map(|s| s.to_string());
            }
            _ => {}
        }
    }

    fn commit_context_buf(&mut self) {
        if self.filter_context_buf.is_empty() {
            self.query.filters.min_context = None;
        } else if let Ok(v) = self.filter_context_buf.parse::<u64>() {
            self.query.filters.min_context = Some(v);
        }
    }

    // --- Export wizard ---

    fn on_key_export(&mut self, key: KeyEvent) -> Option<AppCommand> {
        if key.code == KeyCode::Esc {
            self.mode = Mode::Normal;
            return None;
        }
        match self.export.step {
            ExportStep::Fields => self.export_step_fields(key),
            ExportStep::Format => self.export_step_format(key),
            ExportStep::Destination => return self.export_step_destination(key),
        }
        None
    }

    fn export_step_fields(&mut self, key: KeyEvent) {
        let n = Field::all().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.export.cursor + 1 < n {
                    self.export.cursor += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.export.cursor = self.export.cursor.saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                let field = Field::all()[self.export.cursor];
                if self.export.fields.contains(&field) {
                    self.export.fields.remove(&field);
                } else {
                    self.export.fields.insert(field);
                }
                self.export.error = None;
            }
            KeyCode::Enter => {
                if self.export.fields.is_empty() {
                    self.export.error = Some("Select at least one field".to_string());
                } else {
                    self.export.step = ExportStep::Format;
                    self.export.cursor = 0;
                }
            }
            _ => {}
        }
    }

    fn export_step_format(&mut self, key: KeyEvent) {
        let formats = Format::all();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.export.cursor + 1 < formats.len() {
                    self.export.cursor += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.export.cursor = self.export.cursor.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.export.format = formats[self.export.cursor];
                // Keep the default file path in sync with the chosen extension,
                // unless the user has edited it away from the default pattern.
                if self.export.file_path.starts_with("modelx-export.") {
                    self.export.file_path = format!("modelx-export.{}", self.export.format.ext());
                }
                self.export.step = ExportStep::Destination;
                self.export.cursor = 0;
            }
            _ => {}
        }
    }

    fn export_step_destination(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('k') | KeyCode::Up => {
                self.export.dest_choice = 1 - self.export.dest_choice;
            }
            KeyCode::Char(c) if self.export.dest_choice == 1 => {
                self.export.file_path.push(c);
            }
            KeyCode::Backspace if self.export.dest_choice == 1 => {
                self.export.file_path.pop();
            }
            KeyCode::Enter => {
                if self.export.fields.is_empty() {
                    // Guard: never emit an export with no fields.
                    self.export.step = ExportStep::Fields;
                    self.export.error = Some("Select at least one field".to_string());
                    return None;
                }
                let fields = self.export_fields_in_order();
                let format = self.export.format;
                let destination = if self.export.dest_choice == 0 {
                    ExportDest::Clipboard
                } else {
                    ExportDest::File(self.export.file_path.clone().into())
                };
                self.mode = Mode::Normal;
                return Some(AppCommand::Export {
                    fields,
                    format,
                    destination,
                });
            }
            _ => {}
        }
        None
    }

    /// The chosen export fields, in canonical `Field::all()` order.
    fn export_fields_in_order(&self) -> Vec<Field> {
        Field::all()
            .iter()
            .copied()
            .filter(|f| self.export.fields.contains(f))
            .collect()
    }

    // --- SourcePicker overlay ---

    fn on_key_source_picker(&mut self, key: KeyEvent) -> Option<AppCommand> {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.source_cursor + 1 < self.source_ids.len() {
                    self.source_cursor += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.source_cursor = self.source_cursor.saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some(id) = self.source_ids.get(self.source_cursor).cloned() {
                    self.mode = Mode::Normal;
                    return Some(AppCommand::SwitchSource(id));
                }
            }
            _ => {}
        }
        None
    }

    // --- Help overlay ---

    fn on_key_help(&mut self, key: KeyEvent) -> Option<AppCommand> {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
        ) {
            self.mode = Mode::Normal;
        }
        None
    }

    // -----------------------------------------------------------------------
    // Focus & cursor movement
    // -----------------------------------------------------------------------

    fn cycle_focus(&mut self, forward: bool) {
        self.focus = match (self.focus, forward) {
            (Focus::Providers, true) => Focus::Models,
            (Focus::Models, true) => Focus::Detail,
            (Focus::Detail, true) => Focus::Providers,
            (Focus::Providers, false) => Focus::Detail,
            (Focus::Models, false) => Focus::Providers,
            (Focus::Detail, false) => Focus::Models,
        };
    }

    fn move_cursor(&mut self, delta: i32) {
        match self.focus {
            Focus::Providers => {
                let len = self.provider_row_count();
                self.provider_cursor = step(self.provider_cursor, delta, len);
                self.on_provider_changed();
            }
            Focus::Models => {
                let len = self.view.len();
                self.model_cursor = step(self.model_cursor, delta, len);
                self.detail_cursor = 0;
            }
            Focus::Detail => {
                let len = self.detail_line_count();
                self.detail_cursor = step(self.detail_cursor, delta, len);
            }
        }
    }

    fn cursor_to_top(&mut self) {
        match self.focus {
            Focus::Providers => {
                self.provider_cursor = 0;
                self.on_provider_changed();
            }
            Focus::Models => {
                self.model_cursor = 0;
                self.detail_cursor = 0;
            }
            Focus::Detail => {
                self.detail_cursor = 0;
            }
        }
    }

    fn cursor_to_bottom(&mut self) {
        match self.focus {
            Focus::Providers => {
                self.provider_cursor = self.provider_row_count().saturating_sub(1);
                self.on_provider_changed();
            }
            Focus::Models => {
                self.model_cursor = self.view.len().saturating_sub(1);
                self.detail_cursor = 0;
            }
            Focus::Detail => {
                self.detail_cursor = self.detail_line_count().saturating_sub(1);
            }
        }
    }

    /// After the provider cursor changes, recompute the view (which re-derives
    /// the provider filter from the highlighted row).
    fn on_provider_changed(&mut self) {
        self.recompute_view();
    }

    /// Derive `filters.provider_ids` from the highlighted provider row.
    ///
    /// Row 0 is the synthetic "All providers" entry (no provider filter);
    /// otherwise the filter is the provider at `provider_cursor - 1`. The
    /// provider filter is *solely* a function of the cursor, so recomputing it
    /// here keeps it correct even after the catalog is hot-swapped.
    fn derive_provider_filter(&mut self) {
        self.query.filters.provider_ids = if self.provider_cursor == 0 {
            Vec::new()
        } else {
            // Map the (filtered) cursor row back to a catalog provider index.
            let filtered = self.filtered_provider_indices();
            filtered
                .get(self.provider_cursor - 1)
                .and_then(|&idx| self.catalog.providers.get(idx))
                .map(|p| vec![p.id.clone()])
                .unwrap_or_default()
        };
    }

    // -----------------------------------------------------------------------
    // Selection
    // -----------------------------------------------------------------------

    fn toggle_select_under_cursor(&mut self) {
        if let Some(key) = self.view.get(self.model_cursor).cloned() {
            if self.selection.contains(&key) {
                self.selection.remove(&key);
            } else {
                self.selection.insert(key);
            }
        }
    }

    fn select_all_in_view(&mut self) {
        for key in &self.view {
            self.selection.insert(key.clone());
        }
    }

    // -----------------------------------------------------------------------
    // Copy helpers (return commands; no I/O here)
    // -----------------------------------------------------------------------

    fn copy_focused_value(&mut self) -> Option<AppCommand> {
        let text = match self.focus {
            Focus::Detail => {
                // Copy whatever the highlighted detail row holds.
                match self.detail_rows().into_iter().nth(self.detail_cursor) {
                    Some(DetailRow::Field { value, .. }) => value,
                    Some(DetailRow::Text(t)) => t,
                    Some(DetailRow::Section(s)) => s,
                    None => model_summary(self.current_model()?),
                }
            }
            _ => model_summary(self.current_model()?),
        };
        self.set_toast("copied");
        Some(AppCommand::CopyText(text))
    }

    fn copy_focused_json(&mut self) -> Option<AppCommand> {
        let model = self.current_model()?;
        let text = serde_json::to_string_pretty(&model.raw).unwrap_or_else(|_| "{}".to_string());
        self.set_toast("copied JSON");
        Some(AppCommand::CopyText(text))
    }

    // -----------------------------------------------------------------------
    // View recomputation
    // -----------------------------------------------------------------------

    fn recompute_view(&mut self) {
        // Clamp the provider cursor against the current provider set first, then
        // re-derive the provider filter from the (clamped) highlighted row so the
        // models pane always matches the selected provider — including after a
        // hot-swap that changed the provider set.
        if self.provider_cursor >= self.provider_row_count() {
            self.provider_cursor = self.provider_row_count().saturating_sub(1);
        }
        self.derive_provider_filter();
        self.view = compute_view(&self.catalog, &self.query);
        if self.model_cursor >= self.view.len() {
            self.model_cursor = self.view.len().saturating_sub(1);
        }
        self.detail_cursor = 0;
    }

    // -----------------------------------------------------------------------
    // Toast
    // -----------------------------------------------------------------------

    fn set_toast(&mut self, msg: impl Into<String>) {
        // ~30 ticks at 100ms ≈ 3 seconds.
        self.toast = Some((msg.into(), 30));
    }

    // -----------------------------------------------------------------------
    // Derived counts
    // -----------------------------------------------------------------------

    /// Number of rows in the providers pane (synthetic "All" + each provider
    /// that survives the `provider_search` filter).
    pub fn provider_row_count(&self) -> usize {
        self.filtered_provider_indices().len() + 1
    }

    fn detail_line_count(&self) -> usize {
        self.detail_rows().len()
    }

    // -----------------------------------------------------------------------
    // Read-only accessors used by the renderer
    // -----------------------------------------------------------------------

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn active_source(&self) -> &str {
        &self.active_source
    }

    pub fn refresh_state(&self) -> &RefreshState {
        &self.refresh
    }

    pub fn toast(&self) -> Option<&str> {
        self.toast.as_ref().map(|(s, _)| s.as_str())
    }

    pub fn selection_count(&self) -> usize {
        self.selection.len()
    }

    pub fn total_models(&self) -> usize {
        self.catalog.total_models()
    }

    pub fn view_len(&self) -> usize {
        self.view.len()
    }

    pub fn provider_cursor(&self) -> usize {
        self.provider_cursor
    }

    pub fn model_cursor(&self) -> usize {
        self.model_cursor
    }

    pub fn detail_cursor(&self) -> usize {
        self.detail_cursor
    }

    pub fn detail_raw(&self) -> bool {
        self.detail_raw
    }

    pub fn spinner(&self) -> usize {
        self.spinner
    }

    pub fn source_ids(&self) -> &[String] {
        &self.source_ids
    }

    pub fn source_cursor(&self) -> usize {
        self.source_cursor
    }

    pub fn sort_cursor(&self) -> usize {
        self.sort_cursor
    }

    pub fn sort_fields(&self) -> &'static [Field] {
        SORT_FIELDS
    }

    pub fn sort(&self) -> (Field, bool) {
        (self.query.sort.field, self.query.sort.descending)
    }

    pub fn filter_cursor(&self) -> usize {
        self.filter_cursor
    }

    pub fn search_input(&self) -> &str {
        &self.query.search
    }

    pub fn export_wizard(&self) -> &ExportWizard {
        &self.export
    }

    /// A read-only snapshot of the tri-state / modality filters for rendering.
    pub fn filters_snapshot(&self) -> FiltersSnapshot {
        FiltersSnapshot {
            reasoning: self.query.filters.reasoning,
            tool_call: self.query.filters.tool_call,
            open_weights: self.query.filters.open_weights,
            input_modality: self.query.filters.input_modality.clone(),
        }
    }

    /// The value shown for the `min_context` filter row: the live edit buffer
    /// while the Filter overlay is open, else the committed value.
    pub fn filter_context_display(&self) -> Option<String> {
        if self.mode == Mode::Filter {
            if self.filter_context_buf.is_empty() {
                None
            } else {
                Some(self.filter_context_buf.clone())
            }
        } else {
            self.query.filters.min_context.map(|c| c.to_string())
        }
    }

    /// The label rows for the providers pane, first row = "All providers".
    /// Filtered by `provider_search` (case-insensitive id/name substring).
    pub fn provider_rows(&self) -> Vec<String> {
        let filtered = self.filtered_provider_indices();
        let mut rows = Vec::with_capacity(filtered.len() + 1);
        rows.push(format!("All providers ({})", self.catalog.total_models()));
        for &idx in &filtered {
            if let Some(p) = self.catalog.providers.get(idx) {
                rows.push(format!("{} ({})", p.name, p.models.len()));
            }
        }
        rows
    }

    /// The active provider-search query (empty when no provider filter is set).
    pub fn provider_search(&self) -> &str {
        &self.provider_search
    }

    /// Which pane the currently-open Search overlay is targeting.
    pub fn search_target(&self) -> SearchTarget {
        self.search_target
    }

    /// The number of provider rows currently shown (excluding the synthetic
    /// "All providers" row) — used by the Providers pane header.
    pub fn provider_filtered_count(&self) -> usize {
        self.filtered_provider_indices().len()
    }

    /// The total number of providers in the catalog (unfiltered).
    pub fn provider_total_count(&self) -> usize {
        self.catalog.providers.len()
    }

    /// The model rows for the models pane, with a selection marker and a
    /// benchmark-membership flag (O(1) via the precomputed `benchmarked` set).
    pub fn model_rows(&self) -> Vec<ModelRow> {
        self.view
            .iter()
            .filter_map(|k| self.catalog.find(k).map(|m| (k, m)))
            .map(|(k, m)| ModelRow {
                selected: self.selection.contains(k),
                provider: m.provider_name.clone(),
                name: m.name.clone(),
                id: m.id.clone(),
                has_benchmark: self.benchmarked.contains(k),
            })
            .collect()
    }

    /// Whether the model at the given view index is selected.
    pub fn is_selected_at(&self, idx: usize) -> bool {
        self.view
            .get(idx)
            .map(|k| self.selection.contains(k))
            .unwrap_or(false)
    }

    /// The model currently under the models cursor (if any).
    pub fn current_model(&self) -> Option<&Model> {
        self.view
            .get(self.model_cursor)
            .and_then(|k| self.catalog.find(k))
    }

    /// The models to export: the current selection, or — if the selection is
    /// empty — the focused model. Order follows the current view.
    pub fn export_models(&self) -> Vec<&Model> {
        if self.selection.is_empty() {
            self.current_model().into_iter().collect()
        } else {
            self.view
                .iter()
                .filter(|k| self.selection.contains(k))
                .filter_map(|k| self.catalog.find(k))
                .collect()
        }
    }

    /// Push a transient toast message (used by the runtime for I/O outcomes).
    pub fn push_toast(&mut self, msg: impl Into<String>) {
        self.set_toast(msg);
    }

    /// The comparison view state, if it is currently open.
    pub fn compare(&self) -> Option<&CompareState> {
        self.compare.as_ref()
    }

    /// The models being compared, resolved against the catalog (catalog order).
    pub fn compare_models(&self) -> Vec<&Model> {
        match &self.compare {
            Some(c) => c
                .models
                .iter()
                .filter_map(|k| self.catalog.find(k))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Whether a benchmark database is attached (drives the renderer's decision
    /// to show real values vs. `—`).
    pub fn has_benchmarks(&self) -> bool {
        self.bench_db.is_some()
    }

    /// Per-model benchmark lookup for the compared models, in the same order as
    /// [`AppState::compare_models`]. Each entry is `Some(BenchMatch)` when a
    /// benchmark db is attached (the match may still be empty / unmatched), or
    /// `None` when no db is attached at all.
    pub fn compare_benchmarks(&self) -> Vec<Option<modelx_benchmarks::BenchMatch>> {
        let models = self.compare_models();
        match self.bench_db.as_ref() {
            Some(db) => models.iter().map(|m| Some(db.lookup(m))).collect(),
            None => models.iter().map(|_| None).collect(),
        }
    }

    /// The rows shown in the detail pane for the current model.
    ///
    /// In field mode: fields grouped into labelled sections (rendered as an
    /// aligned two-column table). In raw mode: pretty-printed JSON, one
    /// [`DetailRow::Text`] per line. `detail_cursor` indexes into this vec.
    pub fn detail_rows(&self) -> Vec<DetailRow> {
        let Some(model) = self.current_model() else {
            return vec![DetailRow::Text("No model selected.".to_string())];
        };
        if self.detail_raw {
            let json =
                serde_json::to_string_pretty(&model.raw).unwrap_or_else(|_| "{}".to_string());
            return json
                .lines()
                .map(|l| DetailRow::Text(l.to_string()))
                .collect();
        }

        let mut rows: Vec<DetailRow> = Vec::new();
        for (title, fields) in DETAIL_SECTIONS {
            rows.push(DetailRow::Section((*title).to_string()));
            for f in *fields {
                rows.push(DetailRow::Field {
                    label: f.label(),
                    value: f.value(model).display(),
                    field: Some(*f),
                });
            }
        }
        // Description is long/free-form, so give it its own section and render
        // it as wrapped text rather than a cramped table cell.
        if !model.description.is_empty() {
            rows.push(DetailRow::Section("Description".to_string()));
            rows.push(DetailRow::Text(model.description.clone()));
        }
        rows
    }
}

/// Fields grouped into labelled sections for the detail pane. Every
/// [`Field`] except `Description` (handled separately) appears exactly once.
const DETAIL_SECTIONS: &[(&str, &[Field])] = &[
    (
        "Identity",
        &[
            Field::Name,
            Field::Id,
            Field::ProviderName,
            Field::ProviderId,
            Field::Family,
            Field::Status,
        ],
    ),
    (
        "Capabilities",
        &[
            Field::Reasoning,
            Field::ReasoningEfforts,
            Field::ToolCall,
            Field::StructuredOutput,
            Field::Attachment,
            Field::Temperature,
            Field::OpenWeights,
        ],
    ),
    ("Limits", &[Field::ContextLimit, Field::OutputLimit]),
    (
        "Pricing ($ / million tokens)",
        &[
            Field::InputCost,
            Field::OutputCost,
            Field::CacheReadCost,
            Field::CacheWriteCost,
            Field::ReasoningCost,
        ],
    ),
    (
        "Modalities",
        &[Field::InputModalities, Field::OutputModalities],
    ),
    (
        "Dates",
        &[Field::Knowledge, Field::ReleaseDate, Field::LastUpdated],
    ),
];

/// A row in the detail pane, prepared for rendering.
#[derive(Clone, Debug)]
pub enum DetailRow {
    /// A section header (e.g. "Identity", "Pricing").
    Section(String),
    /// An aligned label/value field row. `field` records the source field so
    /// `y` can copy its value.
    Field {
        label: &'static str,
        value: String,
        field: Option<Field>,
    },
    /// Free text spanning the pane width (description, or a raw-JSON line).
    Text(String),
}

/// A read-only snapshot of the tri-state / modality filters for the renderer.
#[derive(Clone, Debug)]
pub struct FiltersSnapshot {
    pub reasoning: Option<bool>,
    pub tool_call: Option<bool>,
    pub open_weights: Option<bool>,
    pub input_modality: Option<String>,
}

/// A row in the models pane, prepared for rendering.
#[derive(Clone, Debug)]
pub struct ModelRow {
    pub selected: bool,
    pub provider: String,
    pub name: String,
    pub id: String,
    /// Whether this model has benchmark data (drives a distinct colour/marker).
    pub has_benchmark: bool,
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Run the query and map the resulting models to their stable keys.
fn compute_view(catalog: &Catalog, query: &Query) -> Vec<ModelRef> {
    run_query(catalog, query)
        .into_iter()
        .map(|m| m.key())
        .collect()
}

/// Step a cursor by `delta`, clamped to `[0, len)`.
fn step(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = len - 1;
    if delta < 0 {
        cur.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        (cur + delta as usize).min(max)
    }
}

/// Cycle a tri-state `Option<bool>`: None → Some(true) → Some(false) → None.
fn cycle_tri(v: Option<bool>) -> Option<bool> {
    match v {
        None => Some(true),
        Some(true) => Some(false),
        Some(false) => None,
    }
}

/// A one-line summary of a model for quick-copy.
fn model_summary(m: &Model) -> String {
    format!("{} / {} ({})", m.provider_name, m.name, m.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use modelx_core::testkit::sample_catalog;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn new_state() -> AppState {
        AppState::new(
            sample_catalog(),
            vec!["models.dev".to_string()],
            "models.dev".to_string(),
        )
    }

    #[test]
    fn focus_cycles_forward_and_back() {
        let mut s = new_state();
        assert_eq!(s.focus(), Focus::Providers);
        s.on_key(key(KeyCode::Tab));
        assert_eq!(s.focus(), Focus::Models);
        s.on_key(ch('l'));
        assert_eq!(s.focus(), Focus::Detail);
        s.on_key(ch('l'));
        assert_eq!(s.focus(), Focus::Providers);
        s.on_key(ch('h'));
        assert_eq!(s.focus(), Focus::Detail);
    }

    #[test]
    fn jk_moves_models_cursor() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // focus Models
        assert_eq!(s.model_cursor(), 0);
        s.on_key(ch('j'));
        assert_eq!(s.model_cursor(), 1);
        s.on_key(ch('k'));
        assert_eq!(s.model_cursor(), 0);
        // Can't go below 0.
        s.on_key(ch('k'));
        assert_eq!(s.model_cursor(), 0);
    }

    #[test]
    fn provider_filter_narrows_and_all_widens() {
        let mut s = new_state();
        let total = s.view_len();
        assert_eq!(total, sample_catalog().total_models());
        // Move provider cursor to the first real provider (index 1).
        s.on_key(ch('j'));
        assert_eq!(s.provider_cursor(), 1);
        let narrowed = s.view_len();
        assert!(
            narrowed < total,
            "selecting a provider should narrow the view"
        );
        assert!(narrowed > 0);
        // Back to "All providers".
        s.on_key(ch('k'));
        assert_eq!(s.provider_cursor(), 0);
        assert_eq!(s.view_len(), total, "All providers should widen back");
    }

    #[test]
    fn search_narrows_and_esc_restores() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // focus Models so `/` searches models
        let total = s.view_len();
        s.on_key(ch('/'));
        assert_eq!(s.mode(), Mode::Search);
        for c in "opus".chars() {
            s.on_key(ch(c));
        }
        let narrowed = s.view_len();
        assert!(narrowed < total, "search should narrow the view");
        assert!(narrowed >= 1);
        // Esc clears search and returns to Normal.
        s.on_key(key(KeyCode::Esc));
        assert_eq!(s.mode(), Mode::Normal);
        assert_eq!(s.view_len(), total, "Esc should restore full view");
    }

    #[test]
    fn slash_in_providers_focus_filters_providers() {
        let mut s = new_state();
        assert_eq!(s.focus(), Focus::Providers);
        let all_providers = s.provider_filtered_count();
        assert!(all_providers > 1, "sample catalog has multiple providers");
        s.on_key(ch('/'));
        assert_eq!(s.mode(), Mode::Search);
        assert_eq!(s.search_target(), SearchTarget::Providers);
        // Type a substring that matches at least one but not all providers.
        for c in "open".chars() {
            s.on_key(ch(c));
        }
        let filtered = s.provider_filtered_count();
        assert!(
            filtered < all_providers,
            "provider search should narrow the provider list ({filtered} < {all_providers})"
        );
        assert!(filtered >= 1, "at least one provider should match 'open'");
        // The models search buffer is untouched.
        assert!(s.search_input().is_empty());
        // Esc from Normal clears the provider search.
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.provider_search(), "open");
        s.on_key(key(KeyCode::Esc));
        assert!(s.provider_search().is_empty());
        assert_eq!(s.provider_filtered_count(), all_providers);
    }

    #[test]
    fn slash_in_models_focus_still_filters_models() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // focus Models
        let total = s.view_len();
        s.on_key(ch('/'));
        assert_eq!(s.search_target(), SearchTarget::Models);
        for c in "opus".chars() {
            s.on_key(ch(c));
        }
        assert!(s.view_len() < total, "models search should still narrow");
        // Providers untouched.
        assert!(s.provider_search().is_empty());
        // Esc clears the models search buffer, not the provider one.
        s.on_key(key(KeyCode::Esc));
        assert_eq!(s.mode(), Mode::Normal);
        assert!(s.search_input().is_empty());
        assert_eq!(s.view_len(), total);
    }

    #[test]
    fn provider_search_esc_in_search_mode_clears_only_providers() {
        let mut s = new_state();
        // Set a models search first (from Models focus).
        s.on_key(key(KeyCode::Tab));
        s.on_key(ch('/'));
        for c in "opus".chars() {
            s.on_key(ch(c));
        }
        s.on_key(key(KeyCode::Enter));
        let models_view = s.view_len();
        // Now open a providers search and Esc it — models search must survive.
        // From Models focus, one BackTab reaches Providers.
        s.on_key(key(KeyCode::BackTab));
        assert_eq!(s.focus(), Focus::Providers);
        s.on_key(ch('/'));
        for c in "open".chars() {
            s.on_key(ch(c));
        }
        s.on_key(key(KeyCode::Esc)); // clears provider search, keeps models search
        assert!(s.provider_search().is_empty());
        assert_eq!(s.search_input(), "opus");
        assert_eq!(s.view_len(), models_view, "models search preserved");
    }

    #[test]
    fn search_enter_keeps_filter() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // focus Models so `/` searches models
        let total = s.view_len();
        s.on_key(ch('/'));
        for c in "opus".chars() {
            s.on_key(ch(c));
        }
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.mode(), Mode::Normal);
        assert!(s.view_len() < total, "Enter should keep the search filter");
    }

    #[test]
    fn space_toggles_selection_and_capital_a_clears() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // focus Models
        assert_eq!(s.selection_count(), 0);
        s.on_key(ch(' '));
        assert_eq!(s.selection_count(), 1);
        s.on_key(ch(' '));
        assert_eq!(s.selection_count(), 0);
        // select-all then clear
        s.on_key(ch('a'));
        assert_eq!(s.selection_count(), s.view_len());
        s.on_key(ch('A'));
        assert_eq!(s.selection_count(), 0);
    }

    #[test]
    fn sort_toggles_order() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // Models
        let before: Vec<String> = s.model_rows().iter().map(|r| r.name.clone()).collect();
        s.on_key(ch('s'));
        assert_eq!(s.mode(), Mode::Sort);
        // Re-select the same (Name) field → toggles descending.
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.mode(), Mode::Normal);
        let after: Vec<String> = s.model_rows().iter().map(|r| r.name.clone()).collect();
        let mut rev = before.clone();
        rev.reverse();
        assert_eq!(after, rev, "toggling Name sort should reverse the order");
    }

    #[test]
    fn refresh_done_swaps_data_preserving_selection() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // Models
        s.on_key(ch('j')); // move to model index 1
        s.on_key(ch(' ')); // select it
        assert_eq!(s.selection_count(), 1);

        // Build a new catalog for the active source (same models, new
        // timestamp) and swap it in.
        let mut new_catalog = sample_catalog();
        new_catalog.source_id = "models.dev".to_string();
        new_catalog.fetched_at = Some(1_800_000_000);
        s.apply(AppEvent::RefreshDone(new_catalog));

        assert_eq!(s.selection_count(), 1, "selection should survive the swap");
        assert!(matches!(s.refresh_state(), RefreshState::Ok(1_800_000_000)));
    }

    #[test]
    fn refresh_done_for_other_source_is_ignored() {
        // A refresh that completes for a source we've switched away from must
        // not clobber the active source's data or status.
        let mut s = new_state();
        let mut stale = sample_catalog();
        stale.source_id = "some-other-source".to_string();
        stale.fetched_at = Some(1_800_000_000);
        s.apply(AppEvent::RefreshDone(stale));
        // Refresh state stays Idle (unchanged) because the event was dropped.
        assert!(matches!(s.refresh_state(), RefreshState::Idle));
    }

    #[test]
    fn provider_filter_survives_refresh() {
        // Select a concrete provider, then hot-swap the catalog. The models
        // pane must still be scoped to the highlighted provider (the filter is
        // re-derived from the cursor, not left stale).
        let mut s = new_state();
        s.on_key(ch('j')); // provider row 1 (first real provider)
        let scoped: Vec<String> = s.model_rows().iter().map(|r| r.name.clone()).collect();
        assert!(!scoped.is_empty());

        let mut refreshed = sample_catalog();
        refreshed.source_id = "models.dev".to_string();
        s.apply(AppEvent::RefreshDone(refreshed));

        let after: Vec<String> = s.model_rows().iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            after, scoped,
            "provider-scoped view should be preserved across a refresh"
        );
    }

    #[test]
    fn refresh_done_drops_stale_selection() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab));
        s.on_key(ch('a')); // select all
        assert!(s.selection_count() > 0);

        // Swap in an empty catalog: nothing matches → selection drops.
        let empty = Catalog {
            source_id: "models.dev".to_string(),
            fetched_at: Some(1),
            providers: vec![],
        };
        s.apply(AppEvent::RefreshDone(empty));
        assert_eq!(s.selection_count(), 0);
        assert_eq!(s.view_len(), 0);
    }

    #[test]
    fn q_quits() {
        let mut s = new_state();
        assert_eq!(s.on_key(ch('q')), Some(AppCommand::Quit));
    }

    #[test]
    fn ctrl_c_quits() {
        let mut s = new_state();
        let ev = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        assert_eq!(s.on_key(ev), Some(AppCommand::Quit));
    }

    #[test]
    fn export_wizard_reaches_export_command() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // Models
        s.on_key(ch(' ')); // select one model so there's something to export
        s.on_key(ch('e'));
        assert_eq!(s.mode(), Mode::Export);
        // Step 1: default fields preselected → Enter advances.
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.export_wizard().step, ExportStep::Format);
        // Step 2: choose the first format (PlainList) → Enter advances.
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.export_wizard().step, ExportStep::Destination);
        // Step 3: default dest = Clipboard → Enter emits the command.
        let cmd = s.on_key(key(KeyCode::Enter));
        match cmd {
            Some(AppCommand::Export {
                fields,
                format,
                destination,
            }) => {
                assert_eq!(format, Format::PlainList);
                assert_eq!(destination, ExportDest::Clipboard);
                // Default fields, in canonical order.
                assert_eq!(
                    fields,
                    vec![
                        Field::ProviderName,
                        Field::Id,
                        Field::Name,
                        Field::ContextLimit,
                        Field::InputCost,
                        Field::OutputCost,
                    ]
                );
            }
            other => panic!("expected Export command, got {other:?}"),
        }
        assert_eq!(s.mode(), Mode::Normal);
    }

    #[test]
    fn export_wizard_blocks_with_no_fields() {
        let mut s = new_state();
        s.on_key(ch('e'));
        // Deselect all default fields.
        for _ in 0..Field::all().len() {
            let cur = s.export_wizard().cursor;
            if s.export_wizard().fields.contains(&Field::all()[cur]) {
                s.on_key(ch(' '));
            }
            s.on_key(ch('j'));
        }
        // Try to advance — should stay on Fields with an error.
        s.on_key(key(KeyCode::Enter));
        assert_eq!(s.export_wizard().step, ExportStep::Fields);
        assert!(s.export_wizard().error.is_some());
    }

    #[test]
    fn help_opens_and_closes() {
        let mut s = new_state();
        s.on_key(ch('?'));
        assert_eq!(s.mode(), Mode::Help);
        s.on_key(key(KeyCode::Esc));
        assert_eq!(s.mode(), Mode::Normal);
    }

    #[test]
    fn source_picker_emits_switch() {
        let mut s = AppState::new(
            sample_catalog(),
            vec!["models.dev".to_string(), "other".to_string()],
            "models.dev".to_string(),
        );
        s.on_key(ch('S'));
        assert_eq!(s.mode(), Mode::SourcePicker);
        s.on_key(ch('j')); // move to "other"
        let cmd = s.on_key(key(KeyCode::Enter));
        assert_eq!(cmd, Some(AppCommand::SwitchSource("other".to_string())));
    }

    #[test]
    fn refresh_command_from_r() {
        let mut s = new_state();
        assert_eq!(s.on_key(ch('r')), Some(AppCommand::Refresh));
    }

    #[test]
    fn copy_json_returns_command() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab)); // Models
        let cmd = s.on_key(ch('Y'));
        assert!(matches!(cmd, Some(AppCommand::CopyText(_))));
    }

    #[test]
    fn detail_raw_toggle() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab));
        s.on_key(key(KeyCode::Tab)); // Detail
        assert!(!s.detail_raw());
        s.on_key(ch('J'));
        assert!(s.detail_raw());
    }

    #[test]
    fn empty_catalog_does_not_panic() {
        let empty = Catalog {
            source_id: "models.dev".to_string(),
            fetched_at: None,
            providers: vec![],
        };
        let mut s = AppState::new(
            empty,
            vec!["models.dev".to_string()],
            "models.dev".to_string(),
        );
        assert_eq!(s.view_len(), 0);
        // Movement and selection are all no-ops but must not panic.
        s.on_key(key(KeyCode::Tab));
        s.on_key(ch('j'));
        s.on_key(ch(' '));
        s.on_key(ch('a'));
        let rows = s.detail_rows();
        assert!(!rows.is_empty());
    }

    fn select_two(s: &mut AppState) {
        s.on_key(key(KeyCode::Tab)); // focus Models
        s.on_key(ch(' '));
        s.on_key(ch('j'));
        s.on_key(ch(' '));
    }

    #[test]
    fn compare_requires_two_models() {
        let mut s = new_state();
        s.on_key(key(KeyCode::Tab));
        s.on_key(ch(' ')); // one model
        s.on_key(ch('c'));
        assert!(s.compare().is_none(), "one model must not open compare");
        s.on_key(ch('j'));
        s.on_key(ch(' ')); // two models
        s.on_key(ch('c'));
        assert!(s.compare().is_some(), "two models should open compare");
    }

    /// A tiny benchmark DB whose entries match the first two sample models in
    /// default (Name-ascending) sort order — GPT OSS 20B and Qwen3 30B — by
    /// their normalized ids.
    fn tiny_bench_db() -> modelx_benchmarks::BenchmarkDb {
        use modelx_benchmarks::{AliasTable, BenchMetric, BenchmarkDb, ProviderData, SourceEntry};
        use std::collections::BTreeMap;
        let entry = |name: &str, elo: f64| {
            let mut scores = BTreeMap::new();
            scores.insert(BenchMetric::ArenaOverall.key().to_string(), elo);
            SourceEntry {
                model_name: name.to_string(),
                organization: None,
                scores,
            }
        };
        let data = ProviderData {
            provider_id: "lmarena".to_string(),
            fetched_at: None,
            // The default sort is by model name ascending, so the first two models
            // in view are "GPT OSS 20B" (gpt-oss-20b) and "Qwen3 30B" (qwen3-30b).
            entries: vec![entry("gpt-oss-20b", 1500.0), entry("qwen3-30b", 1400.0)],
        };
        BenchmarkDb::from_sources(vec![data], AliasTable::embedded())
    }

    #[test]
    fn compare_esc_exits_and_c_toggles() {
        let mut s = new_state();
        select_two(&mut s);
        s.on_key(ch('c'));
        assert!(s.compare().is_some());
        s.on_key(key(KeyCode::Esc));
        assert!(s.compare().is_none(), "Esc should exit compare");
        // `c` (from Normal, with the selection intact) reopens it.
        s.on_key(ch('c'));
        assert!(s.compare().is_some());
        // `c` from within compare closes it.
        s.on_key(ch('c'));
        assert!(s.compare().is_none(), "c should exit compare");
    }

    #[test]
    fn compare_tab_toggles_view() {
        // Tab switches Table ↔ Bar; BackTab switches back.
        let mut s = new_state();
        select_two(&mut s);
        s.on_key(ch('c'));
        assert_eq!(s.compare().unwrap().view, CompareView::Table);
        s.on_key(key(KeyCode::Tab));
        assert_eq!(s.compare().unwrap().view, CompareView::Bar);
        s.on_key(key(KeyCode::BackTab));
        assert_eq!(s.compare().unwrap().view, CompareView::Table);
    }

    #[test]
    fn compare_bar_metric_keys_toggle_only_in_bar_view() {
        let mut s = new_state();
        select_two(&mut s);
        s.on_key(ch('c'));
        // In Table view, `1`/`2`/`3` are inert.
        s.on_key(ch('2'));
        assert_eq!(s.compare().unwrap().bar_metrics.len(), 3);
        // Switch to Bar; toggling Coding off leaves two metrics.
        s.on_key(key(KeyCode::Tab));
        assert_eq!(s.compare().unwrap().view, CompareView::Bar);
        s.on_key(ch('2'));
        assert!(!s.compare().unwrap().bar_metric_on(BenchMetric::ArenaCoding));
        assert_eq!(s.compare().unwrap().bar_metrics.len(), 2);
    }

    #[test]
    fn compare_arrows_inert_in_bar_view() {
        // In Bar view the scroll keys must not move the table offset.
        let mut s = new_state();
        select_two(&mut s);
        s.on_key(ch('c'));
        s.on_key(key(KeyCode::Tab)); // → Bar
        let scroll0 = s.compare().unwrap().table_scroll;
        s.on_key(ch('j'));
        s.on_key(key(KeyCode::Down));
        assert_eq!(s.compare().unwrap().table_scroll, scroll0);
    }

    #[test]
    fn compare_jk_scrolls_table() {
        let mut s = new_state();
        select_two(&mut s);
        s.on_key(ch('c'));
        assert_eq!(s.compare().unwrap().table_scroll, 0);
        s.on_key(ch('j'));
        assert_eq!(s.compare().unwrap().table_scroll, 1);
        s.on_key(key(KeyCode::Down));
        assert_eq!(s.compare().unwrap().table_scroll, 2);
        s.on_key(ch('k'));
        assert_eq!(s.compare().unwrap().table_scroll, 1);
        // Can't scroll above the top.
        s.on_key(ch('k'));
        s.on_key(ch('k'));
        assert_eq!(s.compare().unwrap().table_scroll, 0);
    }

    #[test]
    fn compare_y_copies_markdown_with_benchmarks() {
        let mut s = new_state().with_benchmarks(Some(tiny_bench_db()));
        select_two(&mut s);
        s.on_key(ch('c'));
        let md = match s.on_key(ch('y')) {
            Some(AppCommand::CopyText(t)) => t,
            _ => panic!("expected a CopyText command"),
        };
        assert!(md.contains("| Metric |"), "should copy a markdown table");
        // The markdown now includes the benchmark rows.
        assert!(
            md.contains("Arena Elo"),
            "markdown should include a benchmark metric label"
        );
    }

    #[test]
    fn compare_benchmarks_reflects_db() {
        let mut s = new_state().with_benchmarks(Some(tiny_bench_db()));
        assert!(s.has_benchmarks());
        select_two(&mut s);
        s.on_key(ch('c'));
        let compare_models = s.compare_models();
        assert!(!compare_models.is_empty(), "compare should have models");
        let matches = s.compare_benchmarks();
        assert_eq!(matches.len(), compare_models.len());
        // At least one compared model (GPT OSS 20B / gpt-oss-20b) should match.
        let any = matches
            .iter()
            .flatten()
            .any(|m| m.matched_any && !m.scores.is_empty());
        assert!(any, "expected at least one benchmark match");
    }

    #[test]
    fn compare_open_toasts_when_some_models_unmatched() {
        // tiny_bench_db matches the first two models (gpt-oss-20b, qwen3-30b).
        // Selecting three models means at least one has no benchmark data.
        let mut s = new_state().with_benchmarks(Some(tiny_bench_db()));
        s.on_key(key(KeyCode::Tab)); // focus Models
        s.on_key(ch(' ')); // model 0
        s.on_key(ch('j'));
        s.on_key(ch(' ')); // model 1
        s.on_key(ch('j'));
        s.on_key(ch(' ')); // model 2 (no benchmark data)
        s.on_key(ch('c'));
        assert!(s.compare().is_some());
        let toast = s.toast().expect("a coverage toast should be set");
        assert!(
            toast.contains("no benchmark data"),
            "toast should mention missing benchmark data, got: {toast:?}"
        );
        assert!(
            toast.contains("of 3"),
            "toast should mention the selected count, got: {toast:?}"
        );
    }

    #[test]
    fn compare_open_toasts_when_no_db() {
        let mut s = new_state(); // no benchmark db
        select_two(&mut s);
        s.on_key(ch('c'));
        assert!(s.compare().is_some());
        let toast = s.toast().expect("a no-db toast should be set");
        assert!(
            toast.contains("not loaded"),
            "toast should say benchmarks aren't loaded, got: {toast:?}"
        );
    }

    #[test]
    fn compare_benchmarks_none_without_db() {
        let mut s = new_state();
        assert!(!s.has_benchmarks());
        select_two(&mut s);
        s.on_key(ch('c'));
        let matches = s.compare_benchmarks();
        assert!(
            matches.iter().all(|m| m.is_none()),
            "no db → every entry is None"
        );
    }
}
