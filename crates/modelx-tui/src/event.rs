//! Event and command types that cross the boundary between the pure UI state
//! ([`crate::state::AppState`]) and the I/O runtime ([`crate::run`]).
//!
//! - [`AppEvent`] flows *into* the state (external things: refresh results, ticks).
//! - [`AppCommand`] flows *out of* the state (side effects the runtime performs).

use std::path::PathBuf;

use modelx_core::Catalog;
use modelx_core::Field;
use modelx_export::Format;

/// Things pushed into [`AppState::apply`](crate::state::AppState::apply) by the runtime.
#[derive(Clone, Debug)]
pub enum AppEvent {
    /// A background refresh has started.
    RefreshStarted,
    /// A background refresh finished successfully with a fresh catalog.
    RefreshDone(Catalog),
    /// A background refresh failed; the string is a user-facing message.
    RefreshFailed(String),
    /// A periodic tick (decays toasts, advances the spinner).
    Tick,
}

/// Where an export should be delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportDest {
    /// Copy the rendered export to the system clipboard.
    Clipboard,
    /// Write the rendered export to a file at this path.
    File(PathBuf),
}

/// Side effects requested by the pure state, executed by the runtime.
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    /// Quit the application.
    Quit,
    /// Refresh the active source.
    Refresh,
    /// Switch to a different data source by id.
    SwitchSource(String),
    /// Copy the given text to the clipboard (quick copy, `y` / `Y`).
    CopyText(String),
    /// Export a selection with the chosen fields, format, and destination.
    Export {
        fields: Vec<Field>,
        format: Format,
        destination: ExportDest,
    },
}
