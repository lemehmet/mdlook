//! Greedy word wrapping over styled character cells.
//!
//! Wrapping happens at the character level rather than on whole inline runs
//! because a single word can span several runs — `**bold**text` is two runs with
//! no space between them, and breaking there would invent a space the author
//! never wrote. Carrying a style per character makes the word boundary and the
//! style boundary independent, which is the only way to get both right.

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// One character plus the style it renders with.
pub type Cell = (char, Style);

/// Input to the wrapper: a stream of styled characters with explicit breaks.
#[derive(Clone, Debug)]
pub enum Unit {
    Char(char, Style),
    /// A hard break: the author asked for a line ending here.
    Break,
}

/// U+FE0F, which forces the character before it to render as a colour emoji.
const EMOJI_PRESENTATION: char = '\u{FE0F}';

pub fn char_width(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Width contributed by `c`, given the character immediately before it.
///
/// Needed because width is not a per-character property. `⚠` is a width-1 text
/// symbol, but `⚠️` — the same character followed by U+FE0F — renders as a
/// double-width emoji. Summing per-character widths reports 1 and every box-drawn
/// table containing one comes out a column short.
pub fn char_width_after(c: char, previous: Option<char>) -> usize {
    if c == EMOJI_PRESENTATION {
        // The selector itself is zero-width; it promotes its base character from
        // one column to two. A base that is already double-width is unaffected.
        return match previous {
            Some(base) if char_width(base) == 1 => 1,
            _ => 0,
        };
    }
    char_width(c)
}

pub fn cells_width(cells: &[Cell]) -> usize {
    let mut total = 0;
    let mut previous = None;
    for &(c, _) in cells {
        total += char_width_after(c, previous);
        previous = Some(c);
    }
    total
}

/// Display width of a string.
///
/// Delegates to `UnicodeWidthStr`, which is sequence-aware and therefore the
/// authority; the per-character helpers above exist only for the wrapper, which
/// has to make decisions one character at a time.
pub fn text_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Replace control characters with visible stand-ins.
///
/// A markdown file is untrusted input, and nothing stops one containing raw
/// terminal escapes. Passed through, they are not text — they are commands: a
/// README can repaint the screen, rewrite the window title, or on terminals that
/// answer OSC queries do rather worse. Rendering them as their Unicode Control
/// Picture keeps the document honest about what it contains while making it
/// inert.
///
/// Tab and newline are absent on purpose: both are resolved earlier, by
/// [`expand_tabs`] and by the wrapper's line handling.
pub fn sanitize(c: char) -> char {
    match c {
        // C0 controls have a matching picture at U+2400 + the code point.
        '\u{0}'..='\u{8}' | '\u{B}'..='\u{1F}' => {
            char::from_u32(0x2400 + c as u32).unwrap_or('\u{FFFD}')
        }
        '\u{7F}' => '\u{2421}',
        // C1 controls have no pictures, and some terminals still act on them.
        '\u{80}'..='\u{9F}' => '\u{FFFD}',
        _ => c,
    }
}

/// Whether a string carries anything [`sanitize`] would rewrite.
pub fn has_controls(s: &str) -> bool {
    s.chars().any(|c| sanitize(c) != c)
}

/// Expand tabs to the next 4-column stop.
///
/// Terminals disagree about tab stops and ratatui does not lay them out, so the
/// only way to get stable widths is to resolve tabs into spaces up front.
pub fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for c in s.chars() {
        match c {
            '\t' => {
                let stop = (col / 4 + 1) * 4;
                out.extend(std::iter::repeat_n(' ', stop - col));
                col = stop;
            }
            '\n' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += char_width(c);
            }
        }
    }
    out
}

/// Group runs of characters sharing a style back into spans.
pub fn cells_to_spans(cells: &[Cell]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut current: Option<Style> = None;

    for &(c, style) in cells {
        if current != Some(style) {
            if let Some(prev) = current {
                if !text.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut text), prev));
                }
            }
            current = Some(style);
        }
        text.push(c);
    }
    if let Some(style) = current {
        if !text.is_empty() {
            spans.push(Span::styled(text, style));
        }
    }
    spans
}

/// Wrap a styled character stream to `width`, returning one cell vector per line.
///
/// Always produces at least one line so that an empty paragraph still occupies a
/// row and block spacing stays predictable.
pub fn wrap(units: &[Unit], width: usize) -> Vec<Vec<Cell>> {
    let width = width.max(1);
    let mut wrapper = Wrapper {
        width,
        lines: Vec::new(),
        line: Vec::new(),
        line_width: 0,
        spaces: Vec::new(),
        word: Vec::new(),
        word_width: 0,
        previous: None,
    };

    for unit in units {
        match *unit {
            Unit::Break => {
                wrapper.flush_word();
                // Trailing spaces before a hard break are not content.
                wrapper.spaces.clear();
                wrapper.end_line();
            }
            Unit::Char(c, style) => {
                if c == '\n' {
                    wrapper.flush_word();
                    wrapper.spaces.clear();
                    wrapper.end_line();
                } else if c.is_whitespace() {
                    wrapper.flush_word();
                    wrapper.spaces.push((c, style));
                } else {
                    wrapper.word_width += char_width_after(c, wrapper.previous);
                    wrapper.word.push((c, style));
                }
                wrapper.previous = Some(c);
            }
        }
    }
    wrapper.flush_word();
    wrapper.end_line();

    if wrapper.lines.is_empty() {
        wrapper.lines.push(Vec::new());
    }
    wrapper.lines
}

struct Wrapper {
    width: usize,
    lines: Vec<Vec<Cell>>,
    line: Vec<Cell>,
    line_width: usize,
    /// Whitespace seen since the last word, held back so it can be dropped if a
    /// line break lands here.
    spaces: Vec<Cell>,
    word: Vec<Cell>,
    word_width: usize,
    /// Last character seen, so a following U+FE0F can widen it.
    previous: Option<char>,
}

impl Wrapper {
    fn end_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.line));
        self.line_width = 0;
        self.spaces.clear();
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let word = std::mem::take(&mut self.word);
        let word_width = std::mem::take(&mut self.word_width);

        // Leading whitespace on a fresh line is dropped, so a wrap point never
        // shows up as a ragged indent on the following line.
        let space_width = if self.line.is_empty() {
            self.spaces.clear();
            0
        } else {
            cells_width(&self.spaces)
        };

        if !self.line.is_empty() && self.line_width + space_width + word_width > self.width {
            self.end_line();
        } else if !self.spaces.is_empty() {
            let spaces = std::mem::take(&mut self.spaces);
            self.line_width += cells_width(&spaces);
            self.line.extend(spaces);
        }
        self.spaces.clear();

        // A word longer than the whole line (a URL, a long identifier) cannot be
        // wrapped at a space, so break it at the margin rather than overflow.
        if word_width > self.width {
            let mut previous = None;
            for (c, style) in word {
                let w = char_width_after(c, previous);
                previous = Some(c);
                if self.line_width + w > self.width && !self.line.is_empty() {
                    self.end_line();
                }
                self.line.push((c, style));
                self.line_width += w;
            }
            return;
        }

        self.line_width += word_width;
        self.line.extend(word);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<Unit> {
        s.chars().map(|c| Unit::Char(c, Style::new())).collect()
    }

    fn render(lines: &[Vec<Cell>]) -> Vec<String> {
        lines.iter().map(|l| l.iter().map(|(c, _)| *c).collect()).collect()
    }

    #[test]
    fn wraps_at_spaces() {
        let out = wrap(&units("the quick brown fox"), 10);
        assert_eq!(render(&out), vec!["the quick", "brown fox"]);
    }

    #[test]
    fn never_exceeds_width() {
        let out = wrap(&units("alpha beta gamma delta epsilon"), 12);
        for line in &out {
            assert!(cells_width(line) <= 12, "line too wide: {line:?}");
        }
    }

    #[test]
    fn breaks_words_longer_than_the_line() {
        let out = wrap(&units("see https://example.com/a/very/long/path here"), 12);
        for line in &out {
            assert!(cells_width(line) <= 12, "line too wide: {line:?}");
        }
        let joined: String = render(&out).join("");
        assert!(joined.contains("https://example.com/a/very/long/path"));
    }

    #[test]
    fn hard_break_forces_a_new_line() {
        let mut u = units("one");
        u.push(Unit::Break);
        u.extend(units("two"));
        assert_eq!(render(&wrap(&u, 40)), vec!["one", "two"]);
    }

    #[test]
    fn double_width_characters_counted_as_two_columns() {
        // Four wide characters exactly fill eight columns.
        let out = wrap(&units("日本語版 abc"), 8);
        assert_eq!(render(&out), vec!["日本語版", "abc"]);
    }

    #[test]
    fn collapses_run_of_spaces_at_a_wrap_point() {
        let out = wrap(&units("alpha    beta"), 7);
        assert_eq!(render(&out), vec!["alpha", "beta"]);
    }

    #[test]
    fn groups_equal_styles_into_one_span() {
        let bold = Style::new().add_modifier(ratatui::style::Modifier::BOLD);
        let cells: Vec<Cell> = "ab".chars().map(|c| (c, bold)).collect();
        assert_eq!(cells_to_spans(&cells).len(), 1);
    }

    #[test]
    fn emoji_presentation_selector_widens_its_base_character() {
        // "⚠️" is U+26A0 plus U+FE0F. Per-character widths sum to 1, but the
        // terminal draws two columns, so anything box-drawn around it is a
        // column short unless the sequence is accounted for.
        assert_eq!(cells_width(&[('\u{26A0}', Style::new())]), 1);
        assert_eq!(cells_width(&[('\u{26A0}', Style::new()), ('\u{FE0F}', Style::new())]), 2);
        // A base that is already double-width must not become three.
        assert_eq!(cells_width(&[('\u{2705}', Style::new()), ('\u{FE0F}', Style::new())]), 2);
    }

    #[test]
    fn wrapping_accounts_for_emoji_presentation_width() {
        let out = wrap(&units("\u{26A0}\u{FE0F}\u{26A0}\u{FE0F}\u{26A0}\u{FE0F} x"), 6);
        for line in &out {
            assert!(cells_width(line) <= 6, "line too wide: {:?}", cells_width(line));
        }
    }

    #[test]
    fn tabs_expand_to_four_column_stops() {
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("\tx"), "    x");
    }
}
