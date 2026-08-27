//! Hex layout: offset, bytes, and an ASCII gutter, xxd-style.
//!
//! The same contract as [`super::source`] — a pure function of
//! `(bytes, width, theme)` producing a [`RenderedDoc`] — so scrolling, search
//! and the `--plain` writer work unchanged. Rows adapt to the pane: sixteen
//! bytes where they fit, eight or four where they do not, which matters
//! because the browser's preview pane is usually narrower than the terminal.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::files::detect;

use super::theme::Theme;
use super::{wrap, RenderedDoc, Sink};

/// Columns before the bytes: an eight-digit offset and two spaces.
const GUTTER: usize = 10;

/// Lay bytes out as a hex dump.
///
/// `size` is the whole file on disk; when `bytes` holds less than that — the
/// read is capped, see [`crate::content::MAX_HEX_BYTES`] — the dump opens by
/// saying how much of the file it shows.
pub fn hex(bytes: &[u8], size: u64, width: usize, theme: &Theme) -> RenderedDoc {
    let mut sink = Sink::new(width.max(8), theme);
    let width = sink.width;

    if bytes.is_empty() {
        sink.push(vec![Span::styled("(empty file)", theme.popup_dim)]);
        return sink.finish();
    }

    if (bytes.len() as u64) < size {
        sink.push(vec![Span::styled(
            format!(
                "showing the first {} of {}",
                detect::human_size(bytes.len() as u64),
                detect::human_size(size)
            ),
            theme.popup_dim,
        )]);
        sink.push_blank();
    }

    let per_row = per_row(width);
    // A full row's byte columns, for padding the last row so the ASCII gutter
    // keeps a straight edge.
    let bytes_width = row_width(per_row) - GUTTER - per_row - 4;
    // Search should look at the bytes and the ASCII, not the margin: a query
    // of `20` means the byte, not the row whose offset happens to contain it.
    sink.content_offset = GUTTER;

    for (row, chunk) in bytes.chunks(per_row).enumerate() {
        let mut spans = vec![Span::styled(format!("{:08x}  ", row * per_row), theme.line_number)];

        let mut used = 0;
        for (index, &byte) in chunk.iter().enumerate() {
            let mut piece = format!("{byte:02x}");
            if index + 1 < chunk.len() {
                piece.push(' ');
                if (index + 1) % 8 == 0 {
                    piece.push(' ');
                }
            }
            used += piece.len();
            push_run(&mut spans, piece, byte_style(byte, theme));
        }
        push_run(&mut spans, " ".repeat(bytes_width - used + 2), theme.line_number);

        push_run(&mut spans, "|".to_string(), theme.line_number);
        for &byte in chunk {
            let (glyph, style) = match byte {
                0x20..=0x7e => (byte as char, theme.text),
                _ => ('·', theme.popup_dim),
            };
            push_run(&mut spans, glyph.to_string(), style);
        }
        push_run(&mut spans, "|".to_string(), theme.line_number);

        sink.push(wrap::truncate(spans, width));
    }

    sink.finish()
}

/// The widest of sixteen, eight and four bytes that fits the pane. Four is the
/// floor: below that the row is cut at the edge like any other overlong line.
fn per_row(width: usize) -> usize {
    [16, 8].into_iter().find(|&n| row_width(n) <= width).unwrap_or(4)
}

/// Columns a full row of `n` bytes occupies: the offset gutter, `xx` pairs
/// separated by spaces with a wider gap every eight, and the ASCII column
/// between pipes.
fn row_width(n: usize) -> usize {
    GUTTER + (3 * n - 1) + (n - 1) / 8 + 2 + (n + 2)
}

/// Printable bytes stand out; everything else recedes. That two-tone split is
/// what makes embedded strings leap out of a binary, which is most of what a
/// pager's hex view is for.
fn byte_style(byte: u8, theme: &Theme) -> Style {
    match byte {
        0x20..=0x7e => theme.text,
        _ => theme.popup_dim,
    }
}

/// Append a piece, merging it into the previous span when the style matches,
/// so a run of similar bytes is one span rather than sixteen.
fn push_run(spans: &mut Vec<Span<'static>>, piece: String, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(&piece),
        _ => spans.push(Span::styled(piece, style)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(bytes: &[u8], width: usize) -> Vec<String> {
        hex(bytes, bytes.len() as u64, width, &Theme::default()).plain
    }

    #[test]
    fn a_row_is_offset_bytes_and_ascii() {
        let lines = plain(b"Hello, world! 0123", 80);
        assert_eq!(
            lines[0],
            "00000000  48 65 6c 6c 6f 2c 20 77  6f 72 6c 64 21 20 30 31  |Hello, world! 01|"
        );
        // The last row is padded so the ASCII column keeps its edge.
        assert_eq!(lines[1], format!("00000010  32 33{}|23|", " ".repeat(45)));
    }

    #[test]
    fn non_printable_bytes_are_dots_in_the_ascii_column() {
        let lines = plain(&[0x00, 0x1b, 0xff, 0x41], 80);
        assert!(lines[0].ends_with("|···A|"), "{}", lines[0]);
    }

    #[test]
    fn rows_narrow_with_the_pane() {
        assert_eq!(plain(&[0u8; 16], 80).len(), 1, "sixteen bytes per row when it fits");
        assert_eq!(plain(&[0u8; 16], 50).len(), 2, "eight at fifty columns");
        assert_eq!(plain(&[0u8; 16], 30).len(), 4, "four at thirty");
    }

    #[test]
    fn width_is_never_exceeded_at_any_size() {
        let bytes: Vec<u8> = (0..=255).collect();
        for width in 1..90 {
            let doc = hex(&bytes, 256, width, &Theme::default());
            for line in &doc.plain {
                assert!(
                    wrap::text_width(line) <= width.max(8),
                    "width {width} overflowed: {line:?}"
                );
            }
        }
    }

    #[test]
    fn a_truncated_file_says_how_much_is_missing() {
        let doc = hex(&[0u8; 16], 5 * 1024 * 1024, 80, &Theme::default());
        assert!(doc.plain[0].contains("showing the first 16 B of 5.0 MiB"), "{}", doc.plain[0]);
        assert!(doc.plain[2].starts_with("00000000"), "rows follow the notice");
    }

    #[test]
    fn an_empty_file_says_so() {
        assert_eq!(plain(b"", 80), vec!["(empty file)"]);
    }

    #[test]
    fn search_skips_the_offset_gutter() {
        let doc = hex(b"needle", 6, 80, &Theme::default());
        assert_eq!(doc.content_offset, GUTTER);
    }

    #[test]
    fn layout_is_pure() {
        assert_eq!(plain(b"same bytes", 40), plain(b"same bytes", 40));
    }
}
