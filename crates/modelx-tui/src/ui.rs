//! Pure rendering for the modelx TUI.
//!
//! [`draw`] takes a [`ratatui::Frame`], the (immutable) [`AppState`], and a
//! [`Theme`], and paints one frame. It performs no I/O and mutates no app
//! state — every widget is derived from the accessors on [`AppState`].

use modelx_benchmarks::{BenchMatch, BenchMetric};
use modelx_core::{Field, Model};
use modelx_export::Format;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Bar, BarChart, BarGroup, Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph,
    Row, Table, Wrap,
};
use ratatui::Frame;

use crate::compare::{self, CompareState, CompareView, METRICS};
use crate::state::{AppState, DetailRow, ExportStep, Focus, Mode, RefreshState, SearchTarget};
use crate::theme::Theme;

/// Distinct colours cycled for models in the comparison charts.
const COMPARE_PALETTE: &[Color] = &[
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::LightBlue,
    Color::LightRed,
    Color::LightGreen,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// Format a Unix timestamp (seconds) as a readable `HH:MM UTC` wall-clock time.
///
/// Pure integer math so the renderer stays dependency- and I/O-free.
fn fmt_clock(ts: i64) -> String {
    let secs_of_day = ts.rem_euclid(86_400);
    let h = secs_of_day / 3_600;
    let m = (secs_of_day % 3_600) / 60;
    format!("{h:02}:{m:02} UTC")
}

/// Paint one full frame.
pub fn draw(frame: &mut Frame, state: &AppState, theme: &Theme) {
    let area = frame.area();

    if state.compare().is_some() {
        // The comparison view takes over the whole screen.
        draw_compare(frame, area, state, theme);
    } else {
        // Vertical split: main area + one-line status bar.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        draw_main(frame, chunks[0], state, theme);
        draw_status_bar(frame, chunks[1], state, theme);
    }

    // Overlays, drawn on top (they work over the browser and the compare view).
    match state.mode() {
        Mode::Search => draw_search(frame, area, state, theme),
        Mode::Sort => draw_sort(frame, area, state, theme),
        Mode::Filter => draw_filter(frame, area, state, theme),
        Mode::Export => draw_export(frame, area, state, theme),
        Mode::SourcePicker => draw_source_picker(frame, area, state, theme),
        Mode::Help => draw_help(frame, area, theme),
        Mode::Normal => {}
    }
}

/// The 3-pane master-detail layout.
fn draw_main(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(40),
            Constraint::Percentage(38),
        ])
        .split(area);

    draw_providers(frame, panes[0], state, theme);
    draw_models(frame, panes[1], state, theme);
    draw_detail(frame, panes[2], state, theme);
}

fn pane_block<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    let border = if focused {
        theme.border_focused
    } else {
        theme.border
    };
    let title_style = if focused { theme.accent } else { theme.dim };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(Span::styled(format!(" {title} "), title_style))
}

fn draw_providers(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let focused = state.focus() == Focus::Providers;
    // Header reflects the filtered provider count when a provider search is active.
    let title = if state.provider_search().is_empty() {
        "Providers".to_string()
    } else {
        format!(
            "Providers {}/{}",
            state.provider_filtered_count(),
            state.provider_total_count()
        )
    };
    let rows = state.provider_rows();
    let items: Vec<ListItem> = rows.into_iter().map(ListItem::new).collect();
    let list = List::new(items)
        .block(pane_block(&title, focused, theme))
        .highlight_style(theme.selected)
        .highlight_symbol("› ");
    let mut lstate = ListState::default();
    if state.provider_row_count() > 0 {
        lstate.select(Some(state.provider_cursor()));
    }
    frame.render_stateful_widget(list, area, &mut lstate);
}

fn draw_models(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let focused = state.focus() == Focus::Models;
    let title = format!("Models {}/{}", state.view_len(), state.total_models());

    if state.view_len() == 0 {
        let msg = if state.total_models() == 0 {
            "No data yet — press r to refresh."
        } else {
            "No models match the current filters."
        };
        let p = Paragraph::new(msg)
            .style(theme.dim)
            .wrap(Wrap { trim: true })
            .block(pane_block(&title, focused, theme));
        frame.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = state
        .model_rows()
        .into_iter()
        .map(|r| {
            let mark = if r.selected { "[x] " } else { "[ ] " };
            // Models with benchmark data get a leading ★ marker and a distinct
            // teal name colour so they're recognisable at a glance.
            let (bench_mark, name_style) = if r.has_benchmark {
                ("★ ", theme.bench.add_modifier(Modifier::BOLD))
            } else {
                ("", Style::default().add_modifier(Modifier::BOLD))
            };
            let line = Line::from(vec![
                Span::styled(mark, if r.selected { theme.ok } else { theme.dim }),
                Span::styled(bench_mark, theme.bench),
                Span::styled(format!("{} ", r.name), name_style),
                Span::styled(format!("· {}", r.provider), theme.dim),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(pane_block(&title, focused, theme))
        .highlight_style(theme.selected)
        .highlight_symbol("› ");
    let mut lstate = ListState::default();
    lstate.select(Some(state.model_cursor()));
    frame.render_stateful_widget(list, area, &mut lstate);
}

fn draw_detail(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let focused = state.focus() == Focus::Detail;
    let mode_hint = if state.detail_raw() {
        "raw JSON"
    } else {
        "fields"
    };
    let title = format!("Detail [{mode_hint}] (J: toggle)");

    // Width of the aligned label column (widest field label).
    let label_w = Field::all()
        .iter()
        .map(|f| f.label().chars().count())
        .max()
        .unwrap_or(16);
    // Width available for wrapped free text (pane minus borders + indent).
    let text_w = (area.width as usize).saturating_sub(4).max(8);

    let items: Vec<ListItem> = state
        .detail_rows()
        .into_iter()
        .map(|row| match row {
            DetailRow::Section(name) => ListItem::new(Line::from(Span::styled(
                format!("▌ {name}"),
                theme.accent.add_modifier(Modifier::BOLD),
            ))),
            DetailRow::Field { label, value, .. } => {
                let shown = if value.is_empty() {
                    "—".to_string()
                } else {
                    value
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {label:<label_w$}  "), theme.dim),
                    Span::raw(shown),
                ]))
            }
            DetailRow::Text(t) if state.detail_raw() => ListItem::new(Line::from(t)),
            DetailRow::Text(t) => {
                // Wrap the description to the pane width, indented under its header.
                let lines: Vec<Line> = wrap_text(&t, text_w)
                    .into_iter()
                    .map(|l| Line::from(Span::raw(format!("  {l}"))))
                    .collect();
                ListItem::new(lines)
            }
        })
        .collect();

    let list = List::new(items)
        .block(pane_block(&title, focused, theme))
        .highlight_style(if focused {
            theme.selected
        } else {
            Style::default()
        });
    let mut lstate = ListState::default();
    lstate.select(Some(state.detail_cursor()));
    frame.render_stateful_widget(list, area, &mut lstate);
}

/// Greedily wrap `s` to lines no wider than `width` columns at word
/// boundaries. A single word longer than `width` is left intact.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if cur.is_empty() {
            cur.push_str(word);
        } else if cur.chars().count() + 1 + word.chars().count() <= width {
            cur.push(' ');
            cur.push_str(word);
        } else {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn draw_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let mut spans: Vec<Span> = Vec::new();

    spans.push(Span::styled(
        format!(" {} ", state.active_source()),
        theme.accent,
    ));

    spans.push(Span::styled(
        format!("{}/{} models ", state.view_len(), state.total_models()),
        theme.dim,
    ));

    if state.selection_count() > 0 {
        spans.push(Span::styled(
            format!("{} selected ", state.selection_count()),
            theme.ok,
        ));
    }

    match state.refresh_state() {
        RefreshState::Refreshing => {
            let frame_ch = SPINNER[state.spinner() % SPINNER.len()];
            spans.push(Span::styled(format!("{frame_ch} refreshing "), theme.warn));
        }
        RefreshState::Ok(ts) => {
            spans.push(Span::styled(
                format!("updated {} ", fmt_clock(*ts)),
                theme.ok,
            ));
        }
        RefreshState::Failed(msg) => {
            spans.push(Span::styled(format!("error: {msg} "), theme.err));
        }
        RefreshState::Idle => {}
    }

    if let Some(t) = state.toast() {
        spans.push(Span::styled(format!("· {t} "), theme.accent));
    }

    // Split the bar into two disjoint segments so the left status text and the
    // right-aligned key hints never overwrite each other on a narrow terminal.
    let left = Line::from(spans);
    let hint_str = " /:search s:sort f:filter e:export ?:help q:quit ";
    let hints = Line::from(Span::styled(hint_str, theme.dim)).alignment(Alignment::Right);

    let segments = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(hint_str.len() as u16),
        ])
        .split(area);

    frame.render_widget(Paragraph::new(left), segments[0]);
    frame.render_widget(Paragraph::new(hints), segments[1]);
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

/// Compute a centred rectangle `pct_x` × `pct_y` percent of `area`.
fn centered(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

fn overlay_block<'a>(title: &'a str, theme: &Theme) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_focused)
        .title(Span::styled(format!(" {title} "), theme.accent))
}

fn draw_search(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rect = centered(60, 20, area);
    frame.render_widget(Clear, rect);
    // Reflect which pane the search targets in both the title and the count.
    let (title, query, count) = match state.search_target() {
        SearchTarget::Providers => (
            "Search providers",
            state.provider_search(),
            state.provider_filtered_count(),
        ),
        SearchTarget::Models => ("Search models", state.search_input(), state.view_len()),
    };
    let text = vec![
        Line::from(vec![
            Span::styled("search: ", theme.accent),
            Span::raw(query.to_string()),
            Span::styled("▏", theme.accent),
        ]),
        Line::from(Span::styled(
            format!("{count} matches · Enter: keep · Esc: clear"),
            theme.dim,
        )),
    ];
    let p = Paragraph::new(text).block(overlay_block(title, theme));
    frame.render_widget(p, rect);
}

fn draw_sort(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rect = centered(40, 50, area);
    frame.render_widget(Clear, rect);

    let (cur_field, descending) = state.sort();
    let dir = if descending { "▼ desc" } else { "▲ asc" };

    let items: Vec<ListItem> = state
        .sort_fields()
        .iter()
        .map(|f| {
            let active = *f == cur_field;
            let label = if active {
                format!("{} ({dir})", f.label())
            } else {
                f.label().to_string()
            };
            let style = if active { theme.ok } else { Style::default() };
            ListItem::new(Span::styled(label, style))
        })
        .collect();

    let list = List::new(items)
        .block(overlay_block(
            "Sort — Enter/re-select toggles dir · d: dir",
            theme,
        ))
        .highlight_style(theme.selected)
        .highlight_symbol("› ");
    let mut lstate = ListState::default();
    lstate.select(Some(state.sort_cursor()));
    frame.render_stateful_widget(list, rect, &mut lstate);
}

fn draw_filter(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rect = centered(50, 50, area);
    frame.render_widget(Clear, rect);

    let cursor = state.filter_cursor();

    // Re-derive filter display strings from the public state accessors.
    let rows = filter_rows(state);
    let items: Vec<ListItem> = rows
        .into_iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let marker = if i == cursor { "› " } else { "  " };
            ListItem::new(Line::from(vec![
                Span::styled(marker, theme.accent),
                Span::styled(format!("{label}: "), theme.dim),
                Span::raw(value),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(overlay_block(
            "Filter — space: cycle · digits: context · Enter: apply",
            theme,
        ))
        .highlight_style(theme.selected);
    let mut lstate = ListState::default();
    lstate.select(Some(cursor));
    frame.render_stateful_widget(list, rect, &mut lstate);
}

/// Build the labelled filter rows from the public state accessors.
fn filter_rows(state: &AppState) -> Vec<(&'static str, String)> {
    let f = state.filters_snapshot();
    vec![
        ("Reasoning", tri(f.reasoning)),
        ("Tool call", tri(f.tool_call)),
        ("Open weights", tri(f.open_weights)),
        (
            "Input modality",
            f.input_modality
                .clone()
                .unwrap_or_else(|| "any".to_string()),
        ),
        (
            "Min context",
            state
                .filter_context_display()
                .unwrap_or_else(|| "any".to_string()),
        ),
    ]
}

fn tri(v: Option<bool>) -> String {
    match v {
        None => "any".to_string(),
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
    }
}

fn draw_export(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rect = centered(60, 70, area);
    frame.render_widget(Clear, rect);
    let wiz = state.export_wizard();

    let step_label = match wiz.step {
        ExportStep::Fields => "1/3 Fields",
        ExportStep::Format => "2/3 Format",
        ExportStep::Destination => "3/3 Destination",
    };

    let title = format!("Export — {step_label}");
    let inner = overlay_block(&title, theme);

    match wiz.step {
        ExportStep::Fields => {
            let items: Vec<ListItem> = Field::all()
                .iter()
                .map(|f| {
                    let checked = wiz.fields.contains(f);
                    let mark = if checked { "[x] " } else { "[ ] " };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, if checked { theme.ok } else { theme.dim }),
                        Span::raw(f.label()),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(inner)
                .highlight_style(theme.selected)
                .highlight_symbol("› ");
            let mut lstate = ListState::default();
            lstate.select(Some(wiz.cursor));
            frame.render_stateful_widget(list, rect, &mut lstate);
            if let Some(err) = &wiz.error {
                draw_export_error(frame, rect, err, theme);
            }
        }
        ExportStep::Format => {
            let items: Vec<ListItem> = Format::all()
                .iter()
                .map(|f| ListItem::new(f.label()))
                .collect();
            let list = List::new(items)
                .block(inner)
                .highlight_style(theme.selected)
                .highlight_symbol("› ");
            let mut lstate = ListState::default();
            lstate.select(Some(wiz.cursor));
            frame.render_stateful_widget(list, rect, &mut lstate);
        }
        ExportStep::Destination => {
            let clip_active = wiz.dest_choice == 0;
            let lines = vec![
                Line::from(Span::styled(
                    format!("{} Clipboard", if clip_active { "›" } else { " " }),
                    if clip_active {
                        theme.ok
                    } else {
                        Style::default()
                    },
                )),
                Line::from(vec![
                    Span::styled(
                        format!("{} File: ", if clip_active { " " } else { "›" }),
                        if clip_active {
                            Style::default()
                        } else {
                            theme.ok
                        },
                    ),
                    Span::raw(wiz.file_path.clone()),
                ]),
                Line::from(Span::styled(
                    "j/k: switch · type to edit path · Enter: export · Esc: cancel",
                    theme.dim,
                )),
            ];
            let p = Paragraph::new(lines).block(inner);
            frame.render_widget(p, rect);
        }
    }
}

fn draw_export_error(frame: &mut Frame, area: Rect, err: &str, theme: &Theme) {
    // Draw the error on the bottom border line of the overlay.
    let line_area = Rect {
        x: area.x + 1,
        y: area.y + area.height.saturating_sub(1),
        width: area.width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(Clear, line_area);
    frame.render_widget(
        Paragraph::new(Span::styled(err.to_string(), theme.err)),
        line_area,
    );
}

fn draw_source_picker(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let rect = centered(40, 40, area);
    frame.render_widget(Clear, rect);
    let items: Vec<ListItem> = state
        .source_ids()
        .iter()
        .map(|id| {
            let active = id == state.active_source();
            let label = if active {
                format!("{id}  (active)")
            } else {
                id.clone()
            };
            ListItem::new(Span::styled(
                label,
                if active { theme.ok } else { Style::default() },
            ))
        })
        .collect();
    let list = List::new(items)
        .block(overlay_block("Source — Enter: switch", theme))
        .highlight_style(theme.selected)
        .highlight_symbol("› ");
    let mut lstate = ListState::default();
    lstate.select(Some(state.source_cursor()));
    frame.render_stateful_widget(list, rect, &mut lstate);
}

fn draw_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    let rect = centered(60, 80, area);
    frame.render_widget(Clear, rect);
    let items: Vec<ListItem> = crate::state::KEYMAP
        .iter()
        .map(|b| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<16}", b.keys), theme.accent),
                Span::raw(b.description),
            ]))
        })
        .collect();
    let list = List::new(items).block(overlay_block("Help — Esc/? to close", theme));
    frame.render_widget(list, rect);
}

// ---------------------------------------------------------------------------
// Comparison view (full screen)
// ---------------------------------------------------------------------------

fn color_for(i: usize) -> Color {
    COMPARE_PALETTE[i % COMPARE_PALETTE.len()]
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn draw_compare(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let cs = state.compare().expect("compare view is active");
    let models = state.compare_models();
    let matches = state.compare_benchmarks();

    // Coverage note: how many model columns matched a benchmark row.
    let matched = matches.iter().flatten().filter(|m| m.matched_any).count();
    let coverage = if state.has_benchmarks() {
        format!("benchmarks: {}/{} models matched", matched, models.len())
    } else {
        "benchmarks: none loaded".to_string()
    };

    let view_label = match cs.view {
        CompareView::Table => "Table",
        CompareView::Bar => "Bar",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border_focused)
        .title(Span::styled(
            format!(
                " Compare [{}] · {} models · {} ",
                view_label,
                models.len(),
                coverage
            ),
            theme.accent.add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // An active toast (e.g. the coverage warning) is shown under the title,
    // since the full-screen compare view has no status bar of its own.
    let has_toast = state.toast().is_some();
    let mut constraints = vec![Constraint::Length(1)]; // control strip
    if has_toast {
        constraints.push(Constraint::Length(1)); // toast line
    }
    constraints.push(Constraint::Min(3)); // body
    constraints.push(Constraint::Length(1)); // hint
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut idx = 0;
    draw_compare_control(frame, rows[idx], cs, theme);
    idx += 1;
    if let Some(t) = state.toast() {
        frame.render_widget(
            Paragraph::new(Span::styled(format!(" {t}"), theme.warn)),
            rows[idx],
        );
        idx += 1;
    }
    let body = rows[idx];
    idx += 1;
    match cs.view {
        CompareView::Table => draw_compare_table(frame, body, cs, &models, &matches, theme),
        CompareView::Bar => draw_compare_bar(frame, body, cs, &models, &matches, theme),
    }
    draw_compare_hint(frame, rows[idx], cs, theme);
}

fn draw_compare_control(frame: &mut Frame, area: Rect, cs: &CompareState, theme: &Theme) {
    let spans: Vec<Span> = match cs.view {
        CompareView::Table => vec![
            Span::styled(" specs + benchmarks · best ", theme.dim),
            Span::styled("green", theme.ok),
            Span::styled(" / worst ", theme.dim),
            Span::styled("red", theme.err),
        ],
        CompareView::Bar => {
            // Show which metrics are toggled on (1/2/3), with the enabled ones
            // in the accent colour.
            let mut spans = vec![Span::styled(" metrics ", theme.dim)];
            for (i, metric) in compare::BAR_METRICS.iter().enumerate() {
                let on = cs.bar_metric_on(*metric);
                let style = if on { theme.ok } else { theme.dim };
                spans.push(Span::styled(
                    format!("{}:{} ", i + 1, metric.label()),
                    style,
                ));
            }
            spans.push(Span::styled(
                "· sorted best → worst · colour = model",
                theme.dim,
            ));
            spans
        }
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A labelled section-header row spanning the metric column (reuses the
/// detail-pane section style: a bold accent bar).
fn compare_section_row<'a>(name: &'a str, n_models: usize, theme: &Theme) -> Row<'a> {
    let mut cells = vec![Cell::from(Line::from(Span::styled(
        format!("▌ {name}"),
        theme.accent.add_modifier(Modifier::BOLD),
    )))];
    for _ in 0..n_models {
        cells.push(Cell::from(""));
    }
    Row::new(cells)
}

fn draw_compare_table(
    frame: &mut Frame,
    area: Rect,
    cs: &CompareState,
    models: &[&Model],
    matches: &[Option<BenchMatch>],
    theme: &Theme,
) {
    // Transposed: metric rows × model columns, in two labelled sections —
    // Specs (METRICS), then Benchmarks (a "Matched as" provenance row followed
    // by one row per BenchMetric).
    let start = cs.table_scroll.min(compare::total_rows().saturating_sub(1));

    let mut header_cells: Vec<Cell> = vec![Cell::from(Line::from(Span::styled(
        "Metric",
        theme.dim.add_modifier(Modifier::BOLD),
    )))];
    for (i, m) in models.iter().enumerate() {
        header_cells.push(Cell::from(Line::from(Span::styled(
            truncate(&m.name, 14),
            Style::default()
                .fg(color_for(i))
                .add_modifier(Modifier::BOLD),
        ))));
    }
    let header = Row::new(header_cells).height(1);

    let mut all_rows: Vec<Row> = Vec::with_capacity(compare::total_rows() + 1);

    // --- Specs section ---
    all_rows.push(compare_section_row("Specs", models.len(), theme));
    for metric in METRICS {
        let (best, worst) = compare::best_worst(models, *metric);
        let mut cells: Vec<Cell> = vec![Cell::from(Line::from(Span::styled(
            metric.label(),
            theme.dim,
        )))];
        for m in models {
            let (txt, style) = match metric.value(m) {
                Some(x) if Some(x) == best => {
                    (metric.format(x), theme.ok.add_modifier(Modifier::BOLD))
                }
                Some(x) if Some(x) == worst => (metric.format(x), theme.err),
                Some(x) => (metric.format(x), Style::default()),
                None => ("—".to_string(), theme.dim),
            };
            cells.push(Cell::from(Line::from(Span::styled(txt, style))));
        }
        all_rows.push(Row::new(cells));
    }

    // --- Benchmarks section ---
    all_rows.push(compare_section_row("Benchmarks", models.len(), theme));

    // "Matched as" provenance row: the benchmark name each column matched.
    let mut matched_cells: Vec<Cell> = vec![Cell::from(Line::from(Span::styled(
        "Matched as",
        theme.dim,
    )))];
    for m in matches {
        let name = m
            .as_ref()
            .and_then(|mt| mt.matched.first())
            .map(|(_, benchmark_name)| truncate(benchmark_name, 14))
            .unwrap_or_else(|| "—".to_string());
        matched_cells.push(Cell::from(Line::from(Span::styled(name, theme.dim))));
    }
    all_rows.push(Row::new(matched_cells));

    for bench_metric in BenchMetric::all() {
        let vals: Vec<Option<f64>> = matches
            .iter()
            .map(|bm| {
                bm.as_ref()
                    .and_then(|m| m.scores.get(bench_metric).copied())
            })
            .collect();
        let present: Vec<f64> = vals.iter().filter_map(|v| *v).collect();
        let (best, worst) = compare::best_worst_from(present, bench_metric.higher_is_better());

        let mut cells: Vec<Cell> = vec![Cell::from(Line::from(Span::styled(
            bench_metric.label(),
            theme.dim,
        )))];
        for v in &vals {
            let (txt, style) = match v {
                Some(x) if Some(*x) == best => (
                    bench_metric.format(*x),
                    theme.ok.add_modifier(Modifier::BOLD),
                ),
                Some(x) if Some(*x) == worst => (bench_metric.format(*x), theme.err),
                Some(x) => (bench_metric.format(*x), Style::default()),
                None => ("—".to_string(), theme.dim),
            };
            cells.push(Cell::from(Line::from(Span::styled(txt, style))));
        }
        all_rows.push(Row::new(cells));
    }

    let rows: Vec<Row> = all_rows.into_iter().skip(start).collect();

    let mut widths = vec![Constraint::Length(18)];
    for _ in models {
        widths.push(Constraint::Min(8));
    }
    let table = Table::new(rows, widths).header(header).column_spacing(1);
    frame.render_widget(table, area);
}

/// The Bar view: a grouped bar chart of the selected benchmark Elo metrics.
///
/// One [`BarGroup`] per selected metric; within a group one [`Bar`] per model
/// that has a value for that metric. Bars are coloured by **model identity** —
/// the same colour in every metric group, in the colour-matched legend below,
/// and in the table header — so colour maps to a model at a glance. Best → worst
/// is conveyed by the left-to-right sort, bar height, and the value label.
fn draw_compare_bar(
    frame: &mut Frame,
    area: Rect,
    cs: &CompareState,
    models: &[&Model],
    matches: &[Option<BenchMatch>],
    theme: &Theme,
) {
    // Collect, per selected metric, the (model_index, value) pairs that exist.
    let mut groups: Vec<BarGroup> = Vec::new();
    let mut any_value = false;

    for metric in &cs.bar_metrics {
        let present: Vec<(usize, f64)> = matches
            .iter()
            .enumerate()
            .filter_map(|(i, bm)| {
                bm.as_ref()
                    .and_then(|m| m.scores.get(metric).copied())
                    .map(|v| (i, v))
            })
            .collect();
        if present.is_empty() {
            continue;
        }
        any_value = true;

        // Sort best → worst so bars read left (best) to right (worst).
        let mut present = present;
        if metric.higher_is_better() {
            present.sort_by(|a, b| b.1.total_cmp(&a.1));
        } else {
            present.sort_by(|a, b| a.1.total_cmp(&b.1));
        }

        let bars: Vec<Bar> = present
            .iter()
            .map(|(model_i, v)| {
                // Colour each bar by its MODEL — the same colour in every metric
                // group and in the legend below (and matching the table header),
                // so colour identifies the model. Best → worst is conveyed by the
                // left-to-right sort, the bar height, and the value label.
                let color = color_for(*model_i);
                Bar::default()
                    .value(v.round() as u64)
                    .text_value(metric.format(*v))
                    .style(Style::default().fg(color))
            })
            .collect();

        groups.push(BarGroup::new(bars).label(Line::from(Span::styled(
            metric.label(),
            theme.accent.add_modifier(Modifier::BOLD),
        ))));
    }

    if !any_value {
        let msg = "No benchmark data for the selected models — run `modelx refresh`.";
        let p = Paragraph::new(Span::styled(msg, theme.warn))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        // Vertically centre the message.
        let v = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Min(1),
                Constraint::Percentage(45),
            ])
            .split(area);
        frame.render_widget(p, v[1]);
        return;
    }

    // Split the body: the chart on top, a legend strip at the bottom.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(2)])
        .split(area);

    let mut chart = BarChart::default()
        .bar_width(9)
        .bar_gap(1)
        .group_gap(3)
        .value_style(Style::default().add_modifier(Modifier::BOLD));
    for group in groups {
        chart = chart.data(group);
    }
    frame.render_widget(chart, rows[0]);

    draw_compare_legend(frame, rows[1], models, theme);
}

/// A legend mapping each model's colour (used for its bars, and the table
/// header) to its full name — a `●` swatch plus the bold, colour-matched name.
fn draw_compare_legend(frame: &mut Frame, area: Rect, models: &[&Model], theme: &Theme) {
    let mut spans: Vec<Span> = vec![Span::styled("models: ", theme.dim)];
    for (i, m) in models.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("   ", theme.dim));
        }
        let color = color_for(i);
        spans.push(Span::styled("● ", Style::default().fg(color)));
        spans.push(Span::styled(
            m.name.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_compare_hint(frame: &mut Frame, area: Rect, cs: &CompareState, theme: &Theme) {
    let keys = match cs.view {
        CompareView::Table => {
            " Tab: Bar   ↑/↓/j/k: scroll   PgUp/PgDn: page   y: copy table   e: export   ?: help   Esc: back   q: quit"
        }
        CompareView::Bar => {
            " Tab: Table   1/2/3: metrics   y: copy table   e: export   ?: help   Esc: back   q: quit"
        }
    };
    frame.render_widget(Paragraph::new(Span::styled(keys, theme.dim)), area);
}

#[cfg(test)]
mod tests {
    use super::{fmt_clock, wrap_text};

    #[test]
    fn wrap_text_breaks_on_word_boundaries() {
        let out = wrap_text("the quick brown fox jumps", 10);
        assert!(out.iter().all(|l| l.chars().count() <= 10), "{out:?}");
        assert_eq!(out.join(" "), "the quick brown fox jumps");
    }

    #[test]
    fn wrap_text_keeps_long_word_intact() {
        let out = wrap_text("supercalifragilistic word", 8);
        assert_eq!(out[0], "supercalifragilistic");
    }

    #[test]
    fn wrap_text_empty_yields_one_empty_line() {
        assert_eq!(wrap_text("   ", 10), vec![String::new()]);
    }

    #[test]
    fn fmt_clock_formats_hh_mm_utc() {
        // 2020-01-01T00:00:00Z
        assert_eq!(fmt_clock(1_577_836_800), "00:00 UTC");
        // 1800000000 → 2027-01-15T08:00:00Z
        assert_eq!(fmt_clock(1_800_000_000), "08:00 UTC");
    }

    #[test]
    fn fmt_clock_handles_epoch_zero() {
        assert_eq!(fmt_clock(0), "00:00 UTC");
    }
}
