//! The I/O runtime: terminal lifecycle, the event loop, background refresh
//! thread, clipboard, and export execution.
//!
//! This is the **only** module in the crate that performs I/O or spawns
//! threads. Everything it drives is pure ([`AppState`] + [`ui::draw`]).

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use modelx_cache::Cache;
use modelx_core::{Catalog, Field, Model};
use modelx_datasource::SourceRegistry;
use modelx_export::{render, write, ExportRequest, Format};
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::event::{AppCommand, AppEvent, ExportDest};
use crate::state::AppState;
use crate::theme::Theme;
use crate::ui;

/// Everything the runtime needs from the binary to do its I/O.
pub struct RuntimeCtx {
    pub registry: SourceRegistry,
    pub cache: Cache,
    pub source_id: String,
    pub ttl_seconds: i64,
    pub offline: bool,
}

/// The poll timeout for input; also the tick cadence.
const POLL: Duration = Duration::from_millis(100);

/// Run the TUI to completion. Owns the terminal and restores it on exit
/// (including on panic, via a hook installed here).
pub fn run(mut state: AppState, ctx: RuntimeCtx) -> Result<()> {
    install_panic_hook();

    let registry = Arc::new(ctx.registry);
    let cache = ctx.cache;
    let mut active_source = ctx.source_id.clone();
    let ttl_seconds = ctx.ttl_seconds;
    let _ = ttl_seconds; // reserved for future staleness display

    let theme = Theme::default();
    let (tx, rx) = mpsc::channel::<AppEvent>();

    // Keep a single clipboard handle alive for the whole run (creating one per
    // copy is slow and, on some platforms, drops the contents immediately).
    let mut clipboard = arboard::Clipboard::new().ok();

    let mut terminal = ratatui::init();

    // Kick an initial refresh unless offline.
    if !ctx.offline {
        spawn_refresh(&registry, &active_source, tx.clone());
    }

    let result = event_loop(
        &mut terminal,
        &mut state,
        &theme,
        &registry,
        &cache,
        &mut active_source,
        &mut clipboard,
        &tx,
        &rx,
        ctx.offline,
    );

    ratatui::restore();
    result
}

/// Install a panic hook that restores the terminal before the default hook
/// runs, so a panic never leaves the terminal in raw/alt-screen mode.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        default(info);
    }));
}

#[allow(clippy::too_many_arguments)]
fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    state: &mut AppState,
    theme: &Theme,
    registry: &Arc<SourceRegistry>,
    cache: &Cache,
    active_source: &mut String,
    clipboard: &mut Option<arboard::Clipboard>,
    tx: &Sender<AppEvent>,
    rx: &Receiver<AppEvent>,
    offline: bool,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, state, theme))?;

        // Input.
        if event::poll(POLL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if let Some(cmd) = state.on_key(key) {
                        match execute(
                            cmd,
                            state,
                            registry,
                            cache,
                            active_source,
                            clipboard,
                            tx,
                            offline,
                        ) {
                            ControlFlow::Continue => {}
                            ControlFlow::Quit => return Ok(()),
                        }
                    }
                }
            }
        }

        // Drain any pending refresh events.
        loop {
            match rx.try_recv() {
                Ok(AppEvent::RefreshDone(catalog)) => {
                    // Persist the fresh catalog before folding it into the UI.
                    // Cache errors are non-fatal: surface them as a toast.
                    if let Err(e) = cache.store(&catalog) {
                        state.push_toast(format!("cache write failed: {e}"));
                    }
                    state.apply(AppEvent::RefreshDone(catalog));
                }
                Ok(ev) => state.apply(ev),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        // Advance spinner/toast timers.
        state.tick();
    }
}

enum ControlFlow {
    Continue,
    Quit,
}

/// Execute a side-effect command. Returns whether to keep running.
#[allow(clippy::too_many_arguments)]
fn execute(
    cmd: AppCommand,
    state: &mut AppState,
    registry: &Arc<SourceRegistry>,
    cache: &Cache,
    active_source: &mut String,
    clipboard: &mut Option<arboard::Clipboard>,
    tx: &Sender<AppEvent>,
    offline: bool,
) -> ControlFlow {
    match cmd {
        AppCommand::Quit => return ControlFlow::Quit,

        AppCommand::Refresh => {
            if offline {
                state.apply(AppEvent::RefreshFailed("offline mode".to_string()));
            } else {
                spawn_refresh(registry, active_source, tx.clone());
            }
        }

        AppCommand::SwitchSource(id) => {
            *active_source = id.clone();
            let catalog = cache
                .load(&id)
                .ok()
                .flatten()
                .unwrap_or_else(|| empty_catalog(&id));
            let source_ids: Vec<String> = registry.ids().iter().map(|s| s.to_string()).collect();
            *state = AppState::new(catalog, source_ids, id.clone());
            if !offline {
                spawn_refresh(registry, active_source, tx.clone());
            }
        }

        AppCommand::CopyText(text) => {
            copy(clipboard, &text, state);
        }

        AppCommand::Export {
            fields,
            format,
            destination,
        } => {
            do_export(state, fields, format, destination, clipboard);
        }
    }
    ControlFlow::Continue
}

/// Spawn the background refresh thread for `source_id` and immediately emit
/// `RefreshStarted`.
fn spawn_refresh(registry: &Arc<SourceRegistry>, source_id: &str, tx: Sender<AppEvent>) {
    let _ = tx.send(AppEvent::RefreshStarted);
    let registry = Arc::clone(registry);
    let id = source_id.to_string();
    thread::spawn(move || {
        let event = match registry.get(&id) {
            Some(source) => match source.fetch() {
                Ok(mut catalog) => {
                    catalog.fetched_at = Some(now_unix());
                    AppEvent::RefreshDone(catalog)
                }
                Err(e) => AppEvent::RefreshFailed(e.to_string()),
            },
            None => AppEvent::RefreshFailed(format!("unknown source: {id}")),
        };
        // If the main thread has gone away, the send simply fails; ignore it.
        let _ = tx.send(event);
    });
}

/// Quick-copy text to the clipboard, toasting on failure.
fn copy(clipboard: &mut Option<arboard::Clipboard>, text: &str, state: &mut AppState) {
    let ok = clipboard
        .as_mut()
        .map(|c| c.set_text(text.to_string()).is_ok())
        .unwrap_or(false);
    if !ok {
        state.push_toast("clipboard unavailable");
    }
}

/// Build the export request from the current selection (or focused model) and
/// deliver it to the chosen destination.
fn do_export(
    state: &mut AppState,
    fields: Vec<Field>,
    format: Format,
    destination: ExportDest,
    clipboard: &mut Option<arboard::Clipboard>,
) {
    let models: Vec<&Model> = state.export_models();
    if models.is_empty() {
        state.apply(AppEvent::RefreshFailed("nothing to export".to_string()));
        return;
    }
    let req = ExportRequest {
        models,
        fields,
        format,
    };
    match destination {
        ExportDest::Clipboard => match render(&req) {
            Ok(text) => {
                let ok = clipboard
                    .as_mut()
                    .map(|c| c.set_text(text).is_ok())
                    .unwrap_or(false);
                let msg = if ok {
                    "exported to clipboard".to_string()
                } else {
                    "clipboard unavailable".to_string()
                };
                state.push_toast(msg);
            }
            Err(e) => state.push_toast(format!("export failed: {e}")),
        },
        ExportDest::File(path) => match write(&req, &path) {
            Ok(()) => state.push_toast(format!("wrote {}", path.display())),
            Err(e) => state.push_toast(format!("export failed: {e}")),
        },
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn empty_catalog(source_id: &str) -> Catalog {
    Catalog {
        source_id: source_id.to_string(),
        fetched_at: None,
        providers: Vec::new(),
    }
}
