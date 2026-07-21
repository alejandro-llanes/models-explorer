//! Generate real screenshots of the TUI views by rendering the actual ratatui
//! buffer to SVG (faithful per-cell colours). Run:
//!   cargo run -p modelx-tui --example screenshots
//! then convert with: rsvg-convert -o out.png in.svg
//!
//! Requires a populated cache (`modelx refresh`).
use std::fmt::Write as _;

use modelx_benchmarks::{AliasTable, BenchCache, BenchmarkDb};
use modelx_core::Catalog;
use modelx_tui::{AppState, Theme};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn press(s: &mut AppState, code: KeyCode) {
    s.on_key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
}
fn typ(s: &mut AppState, text: &str) {
    for c in text.chars() {
        press(s, KeyCode::Char(c));
    }
}

fn main() {
    let home = std::env::var("HOME").unwrap();
    let catalog: Catalog = serde_json::from_slice(
        &std::fs::read(format!("{home}/.cache/modelx/sources/models.dev.json"))
            .expect("run `modelx refresh` first"),
    )
    .unwrap();
    // `BenchmarkDb` is not `Clone`, so load a fresh instance per state.
    let load_db = || {
        BenchmarkDb::load(
            &BenchCache::discover().unwrap(),
            AliasTable::embedded(),
            i64::MAX,
            true,
        )
        .ok()
    };

    let out = format!("{home}/Projects/personalProjects/models-explorer/docs/assets/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let theme = Theme::default();

    // Helper: build a fresh state scoped to the "anthropic" provider.
    let scoped = || {
        let mut s = AppState::new(
            catalog.clone(),
            vec!["models.dev".to_string()],
            "models.dev".to_string(),
        )
        .with_benchmarks(load_db());
        // Provider search → anthropic, then select that provider row.
        press(&mut s, KeyCode::Char('/'));
        typ(&mut s, "anthropic");
        press(&mut s, KeyCode::Enter);
        press(&mut s, KeyCode::Char('j')); // move off "All providers" onto anthropic
        s
    };

    // 1) Browser (3-pane), anthropic scoped.
    render(&scoped(), &theme, 120, 40, &format!("{out}/browser.svg"));

    // 2) Compare table — select the first three anthropic models.
    let mut s = scoped();
    press(&mut s, KeyCode::Tab); // focus Models
    for _ in 0..3 {
        press(&mut s, KeyCode::Char(' '));
        press(&mut s, KeyCode::Char('j'));
    }
    press(&mut s, KeyCode::Char('c'));
    render(&s, &theme, 120, 40, &format!("{out}/compare-table.svg"));

    // 3) Compare bar graph.
    press(&mut s, KeyCode::Tab);
    render(&s, &theme, 120, 40, &format!("{out}/compare-bar.svg"));

    println!("wrote SVGs to {out}");
}

fn render(state: &AppState, theme: &Theme, w: u16, h: u16, path: &str) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| modelx_tui::ui::draw(f, state, theme))
        .unwrap();
    let buf = term.backend().buffer();
    std::fs::write(path, buffer_to_svg(buf, w, h)).unwrap();
}

fn buffer_to_svg(buf: &ratatui::buffer::Buffer, w: u16, h: u16) -> String {
    let (cw, ch, fs) = (9.6_f64, 19.0_f64, 15.5_f64);
    let (pw, ph) = (cw * w as f64, ch * h as f64);
    let mut svg = String::new();
    write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{pw}" height="{ph}" viewBox="0 0 {pw} {ph}" font-family="JetBrains Mono, DejaVu Sans Mono, Menlo, monospace" font-size="{fs}">"#
    )
    .unwrap();
    // Page background (terminal default).
    write!(
        svg,
        r##"<rect width="{pw}" height="{ph}" fill="#0d1117"/>"##
    )
    .unwrap();
    let content = buf.content();
    // Background rects.
    for y in 0..h {
        for x in 0..w {
            let cell = &content[(y as usize) * (w as usize) + x as usize];
            if let Some(bg) = color_hex(cell.bg, true) {
                write!(
                    svg,
                    r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{bg}"/>"#,
                    x as f64 * cw,
                    y as f64 * ch,
                    cw + 0.5,
                    ch + 0.5
                )
                .unwrap();
            }
        }
    }
    // Foreground text.
    for y in 0..h {
        for x in 0..w {
            let cell = &content[(y as usize) * (w as usize) + x as usize];
            let sym = cell.symbol();
            if sym == " " || sym.is_empty() {
                continue;
            }
            let fg = color_hex(cell.fg, false).unwrap_or_else(|| "#c9d1d9".to_string());
            let bold = if cell.modifier.contains(Modifier::BOLD) {
                r#" font-weight="bold""#
            } else {
                ""
            };
            let tx = x as f64 * cw + cw / 2.0;
            let ty = y as f64 * ch + fs;
            write!(
                svg,
                r#"<text x="{tx:.1}" y="{ty:.1}" fill="{fg}" text-anchor="middle"{bold}>{}</text>"#,
                xml_escape(sym)
            )
            .unwrap();
        }
    }
    svg.push_str("</svg>");
    svg
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Map a ratatui colour to a hex string. `bg` Reset returns `None` (page default).
fn color_hex(c: Color, is_bg: bool) -> Option<String> {
    let named = |r: u8, g: u8, b: u8| Some(format!("#{r:02x}{g:02x}{b:02x}"));
    match c {
        Color::Reset => {
            if is_bg {
                None
            } else {
                named(0xc9, 0xd1, 0xd9)
            }
        }
        Color::Rgb(r, g, b) => named(r, g, b),
        Color::Black => named(0x0d, 0x11, 0x17),
        Color::Red => named(0xf8, 0x51, 0x49),
        Color::Green => named(0x3f, 0xb9, 0x50),
        Color::Yellow => named(0xd2, 0x9d, 0x00),
        Color::Blue => named(0x58, 0x8e, 0xff),
        Color::Magenta => named(0xbc, 0x8c, 0xff),
        Color::Cyan => named(0x39, 0xc5, 0xcf),
        Color::Gray => named(0x8b, 0x94, 0x9e),
        Color::DarkGray => named(0x6e, 0x76, 0x81),
        Color::LightRed => named(0xff, 0x7b, 0x72),
        Color::LightGreen => named(0x56, 0xd3, 0x64),
        Color::LightYellow => named(0xe3, 0xb3, 0x41),
        Color::LightBlue => named(0x79, 0xc0, 0xff),
        Color::LightMagenta => named(0xd2, 0xa8, 0xff),
        Color::LightCyan => named(0x56, 0xd4, 0xdd),
        Color::White => named(0xf0, 0xf6, 0xfc),
        Color::Indexed(i) => {
            let (r, g, b) = xterm256(i);
            named(r, g, b)
        }
    }
}

fn xterm256(i: u8) -> (u8, u8, u8) {
    const SYS: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    match i {
        0..=15 => SYS[i as usize],
        16..=231 => {
            let n = i - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            (
                steps[(n / 36) as usize],
                steps[((n / 6) % 6) as usize],
                steps[(n % 6) as usize],
            )
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}
