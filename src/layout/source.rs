//! Layout for files that are not markdown.
//!
//! This is the same contract as [`super::layout`] — a pure function of
//! `(content, width, theme)` producing a [`RenderedDoc`] — so everything
//! downstream works without knowing which producer it came from: scrolling,
//! search, the match index, resize anchoring, and the `--plain` writer.
//!
//! Two things are deliberately absent. There is no reflow: source lines mean
//! what their columns say, so long ones are cut at the edge rather than folded.
//! And there are no anchors or links, so the outline and link lists come up
//! empty, which is the honest answer for a file that has no headings.

use ratatui::text::Span;

use super::theme::Theme;
use super::wrap;
use super::{code, RenderedDoc, Sink};

/// Lay a file out as numbered, syntax-highlighted lines.
///
/// `name` is used only to pick a syntax; nothing is read from disk here.
pub fn source(name: &str, body: &str, width: usize, theme: &Theme) -> RenderedDoc {
    let mut sink = Sink::new(width.max(8), theme);
    let lines = code::highlight_file(name, body, theme);

    if lines.is_empty() {
        // A zero-line document would draw as a blank screen indistinguishable
        // from a failure to load, so say which one it is.
        sink.push(vec![Span::styled("(empty file)", theme.popup_dim)]);
        return sink.finish();
    }

    // Sized for the largest number it will hold, so the gutter never shifts
    // width partway down and take the text with it.
    let gutter = digits(lines.len());
    let avail = sink.width.saturating_sub(gutter + 1).max(1);
    sink.content_offset = gutter + 1;

    for (index, line) in lines.into_iter().enumerate() {
        let mut spans = vec![Span::styled(format!("{:>gutter$} ", index + 1), theme.line_number)];
        spans.extend(wrap::truncate(line, avail));
        sink.push(spans);
    }

    sink.finish()
}

/// Render a note about a file whose contents we are not showing.
///
/// A binary reaches the viewer as an identification rather than as bytes,
/// because knowing *what* it is tells you which other tool to open it with,
/// which is the only useful thing a reader can do from here. The same shape
/// carries every other "nothing to display" case.
pub fn summary(
    name: &str,
    headline: &str,
    detail: &str,
    width: usize,
    theme: &Theme,
) -> RenderedDoc {
    let mut sink = Sink::new(width.max(8), theme);
    let avail = sink.width;

    let rows = vec![
        vec![Span::styled(name.to_string(), theme.heading(1))],
        Vec::new(),
        vec![Span::styled(headline.to_string(), theme.popup_dim)],
        Vec::new(),
        vec![Span::styled(detail.to_string(), theme.text)],
    ];
    for spans in rows {
        sink.push(wrap::truncate(spans, avail));
    }

    sink.finish()
}

/// Decimal width of `n`, at least 1.
fn digits(n: usize) -> usize {
    let mut width = 1;
    let mut n = n;
    while n >= 10 {
        n /= 10;
        width += 1;
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(doc: &RenderedDoc) -> Vec<String> {
        doc.plain.clone()
    }

    #[test]
    fn every_line_is_numbered_in_a_fixed_width_gutter() {
        let body = (1..=12).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let doc = source("x.txt", &body, 40, &Theme::default());
        let lines = plain(&doc);
        assert_eq!(lines.len(), 12);
        assert_eq!(lines[0], " 1 line 1");
        assert_eq!(lines[11], "12 line 12");
        // The gutter is what `content_offset` describes, and it is uniform.
        assert_eq!(doc.content_offset, 3);
    }

    #[test]
    fn long_lines_are_cut_at_the_width_never_wrapped() {
        let doc = source("x.txt", &"x".repeat(500), 40, &Theme::default());
        assert_eq!(doc.len(), 1, "a long line must stay one line");
        assert_eq!(crate::layout::wrap::text_width(&doc.plain[0]), 40);
    }

    #[test]
    fn width_is_never_exceeded_at_any_size() {
        let body = "fn main() {\n    println!(\"hello, world\");\n}\n";
        for width in 1..40 {
            let doc = source("m.rs", body, width, &Theme::default());
            for line in &doc.plain {
                assert!(
                    crate::layout::wrap::text_width(line) <= width.max(8),
                    "width {width} overflowed: {line:?}"
                );
            }
        }
    }

    #[test]
    fn an_empty_file_says_so() {
        let doc = source("x.txt", "", 40, &Theme::default());
        assert_eq!(plain(&doc), vec!["(empty file)"]);
    }

    #[test]
    fn control_characters_are_neutralised_like_everywhere_else() {
        let doc = source("x.txt", "before\u{1b}[31mafter", 60, &Theme::default());
        assert!(doc.plain[0].contains('␛'), "escape should be shown, not forwarded");
        assert!(!doc.plain[0].contains('\u{1b}'));
    }

    #[test]
    fn crlf_does_not_leave_a_carriage_return_on_every_line() {
        let doc = source("x.txt", "one\r\ntwo\r\n", 40, &Theme::default());
        assert_eq!(plain(&doc), vec!["1 one", "2 two"]);
    }

    #[test]
    fn tabs_are_expanded_rather_than_left_to_the_terminal() {
        let doc = source("x.txt", "\tindented", 40, &Theme::default());
        assert_eq!(plain(&doc), vec!["1     indented"]);
    }

    #[test]
    fn a_source_file_has_no_outline_or_links() {
        let doc =
            source("m.rs", "fn main() {}\n// see https://example.com\n", 60, &Theme::default());
        assert!(doc.anchors.is_empty());
        assert!(doc.links.is_empty());
    }

    #[test]
    fn layout_is_pure() {
        let body = "fn main() {}\n";
        let theme = Theme::default();
        assert_eq!(
            plain(&source("m.rs", body, 60, &theme)),
            plain(&source("m.rs", body, 60, &theme))
        );
    }
}
