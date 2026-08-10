//! Render a laid-out document to ANSI escape sequences for stdout.
//!
//! This exists so the tool is useful in a pipeline, not just interactively — and
//! because the piped path is precisely where `glow` stops reflowing. It walks the
//! same `RenderedDoc` the TUI draws, so piped and interactive output agree.

use std::fmt::Write as _;

use ratatui::style::{Color, Modifier, Style};

use crate::layout::RenderedDoc;

/// Serialise the document, one `\n`-terminated line at a time.
pub fn to_ansi(doc: &RenderedDoc, color: bool) -> String {
    let mut out = String::new();
    for line in &doc.lines {
        let mut styled = false;
        for span in &line.spans {
            if color {
                // Each span's style is absolute, so reset whatever the previous
                // one left set before applying this one. `\x1b[0m` doubles as the
                // "no style" case, which is why plain spans still need it.
                let sgr = sgr(span.style);
                if sgr.is_empty() {
                    if styled {
                        out.push_str("\x1b[0m");
                        styled = false;
                    }
                } else {
                    let _ = write!(out, "\x1b[0m\x1b[{sgr}m");
                    styled = true;
                }
            }
            out.push_str(&span.content);
        }
        if styled {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

/// Build the SGR parameter list for a style, without the surrounding escape.
fn sgr(style: Style) -> String {
    let mut parts: Vec<String> = Vec::new();

    for (modifier, code) in [
        (Modifier::BOLD, 1),
        (Modifier::DIM, 2),
        (Modifier::ITALIC, 3),
        (Modifier::UNDERLINED, 4),
        (Modifier::SLOW_BLINK, 5),
        (Modifier::RAPID_BLINK, 6),
        (Modifier::REVERSED, 7),
        (Modifier::HIDDEN, 8),
        (Modifier::CROSSED_OUT, 9),
    ] {
        if style.add_modifier.contains(modifier) {
            parts.push(code.to_string());
        }
    }

    if let Some(fg) = style.fg {
        if let Some(code) = color_code(fg, false) {
            parts.push(code);
        }
    }
    if let Some(bg) = style.bg {
        if let Some(code) = color_code(bg, true) {
            parts.push(code);
        }
    }

    parts.join(";")
}

fn color_code(color: Color, background: bool) -> Option<String> {
    // Base offsets: 30 foreground / 40 background for the first eight colours,
    // 90 / 100 for their bright variants.
    let offset = if background { 10 } else { 0 };
    let basic = |n: u8| Some((n + offset).to_string());

    match color {
        Color::Reset => None,
        Color::Black => basic(30),
        Color::Red => basic(31),
        Color::Green => basic(32),
        Color::Yellow => basic(33),
        Color::Blue => basic(34),
        Color::Magenta => basic(35),
        Color::Cyan => basic(36),
        Color::Gray => basic(37),
        Color::DarkGray => basic(90),
        Color::LightRed => basic(91),
        Color::LightGreen => basic(92),
        Color::LightYellow => basic(93),
        Color::LightBlue => basic(94),
        Color::LightMagenta => basic(95),
        Color::LightCyan => basic(96),
        Color::White => basic(97),
        Color::Rgb(r, g, b) => Some(format!("{};2;{r};{g};{b}", 38 + offset)),
        Color::Indexed(i) => Some(format!("{};5;{i}", 38 + offset)),
    }
}
