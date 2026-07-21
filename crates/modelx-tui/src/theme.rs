//! Colour theme for the TUI. Pure data — no ratatui rendering here beyond
//! [`Style`] values.

use ratatui::style::{Color, Modifier, Style};

/// A bundle of styles used across the UI.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Accent colour for titles / hints.
    pub accent: Style,
    /// Style for the selected / cursor row.
    pub selected: Style,
    /// Dimmed style for secondary text.
    pub dim: Style,
    /// Success / "ok" style.
    pub ok: Style,
    /// Warning style.
    pub warn: Style,
    /// Error style.
    pub err: Style,
    /// Border style for unfocused panes.
    pub border: Style,
    /// Border style for the focused pane.
    pub border_focused: Style,
    /// Accent colour for models that have benchmark data (a cyan/teal accent,
    /// distinct from `accent` so benchmarked models stand out in the Models pane).
    pub bench: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            selected: Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            dim: Style::default().fg(Color::DarkGray),
            ok: Style::default().fg(Color::Green),
            warn: Style::default().fg(Color::Yellow),
            err: Style::default().fg(Color::Red),
            border: Style::default().fg(Color::DarkGray),
            border_focused: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            // Teal — visible against the default background, distinct from the
            // cyan `accent`, marking models that carry benchmark data.
            bench: Style::default().fg(Color::Rgb(45, 190, 180)),
        }
    }
}
