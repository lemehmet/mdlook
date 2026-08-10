//! Incremental search over the *rendered* text.
//!
//! Searching the rendered mirror rather than the markdown source is what makes
//! this useful for API docs: a heading written as `` ## `fetch_user()` `` is
//! rendered as `fetch_user()`, so that is what you can search for. Searching the
//! source would mean typing the backticks you never see.

use crate::layout::RenderedDoc;

/// One hit, as a byte range within `RenderedDoc::plain[line]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Search {
    pub query: String,
    pub matches: Vec<Match>,
    /// Index into `matches` of the hit currently focused.
    pub current: usize,
}

impl Search {
    pub fn is_active(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current = 0;
    }

    /// Re-run the query, keeping the focus near where it already was.
    ///
    /// Preserving position matters while typing: extending "fet" to "fetch"
    /// should not fling you back to the top of the document.
    pub fn refresh(&mut self, doc: &RenderedDoc) {
        let anchor = self.matches.get(self.current).map(|m| m.line);
        self.matches = find(doc, &self.query);
        self.current = match anchor {
            Some(line) => self.matches.iter().position(|m| m.line >= line).unwrap_or(0),
            None => 0,
        };
    }

    pub fn current_match(&self) -> Option<Match> {
        self.matches.get(self.current).copied()
    }

    /// Advance the focus, wrapping around at the ends.
    pub fn step(&mut self, forward: bool) -> Option<Match> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = if forward {
            (self.current + 1) % self.matches.len()
        } else {
            (self.current + self.matches.len() - 1) % self.matches.len()
        };
        self.current_match()
    }

    /// Focus the first match at or after `line`, for jumping from a scroll spot.
    pub fn focus_near(&mut self, line: usize) {
        if let Some(index) = self.matches.iter().position(|m| m.line >= line) {
            self.current = index;
        }
    }

    /// All hits on one line, for painting highlights.
    pub fn on_line(&self, line: usize) -> impl Iterator<Item = (Match, bool)> + '_ {
        let current = self.current_match();
        self.matches.iter().filter(move |m| m.line == line).map(move |m| (*m, Some(*m) == current))
    }
}

/// Case-fold a single character while preserving one-char-per-char mapping.
///
/// `char::to_lowercase` can expand one character into several, which would
/// desynchronise the byte offsets we report. Taking the first character of the
/// mapping keeps offsets exact at the cost of some exotic ligature cases.
fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Find every occurrence of `query`, using smart case.
///
/// An all-lowercase query matches case-insensitively; any uppercase character
/// makes the whole query case-sensitive. That is the behaviour people already
/// expect from vim and ripgrep, so it needs no explaining.
pub fn find(doc: &RenderedDoc, query: &str) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let needle: Vec<char> =
        if sensitive { query.chars().collect() } else { query.chars().map(fold).collect() };

    let mut out = Vec::new();
    for (line, text) in doc.plain.iter().enumerate() {
        find_in_line(text, &needle, sensitive, line, &mut out);
    }
    out
}

/// Naive scan, which is the right call here: documents are small, the query
/// changes on every keystroke, and building an index would cost more than it
/// saves.
fn find_in_line(
    haystack: &str,
    needle: &[char],
    sensitive: bool,
    line: usize,
    out: &mut Vec<Match>,
) {
    // (byte offset, character) pairs, so a hit can be reported as a byte range.
    let chars: Vec<(usize, char)> = haystack.char_indices().collect();
    if chars.len() < needle.len() {
        return;
    }

    let mut start = 0;
    while start + needle.len() <= chars.len() {
        let hit = needle.iter().enumerate().all(|(offset, &want)| {
            let got = chars[start + offset].1;
            let got = if sensitive { got } else { fold(got) };
            got == want
        });

        if hit {
            let begin = chars[start].0;
            let last = chars[start + needle.len() - 1];
            let end = last.0 + last.1.len_utf8();
            out.push(Match { line, start: begin, end });
            // Non-overlapping: "aa" in "aaaa" is two hits, not three.
            start += needle.len();
        } else {
            start += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(lines: &[&str]) -> RenderedDoc {
        RenderedDoc {
            plain: lines.iter().map(|s| s.to_string()).collect(),
            lines: lines.iter().map(|s| s.to_string().into()).collect(),
            ..Default::default()
        }
    }

    fn texts<'a>(d: &'a RenderedDoc, ms: &[Match]) -> Vec<&'a str> {
        ms.iter().map(|m| &d.plain[m.line][m.start..m.end]).collect()
    }

    #[test]
    fn lowercase_query_ignores_case() {
        let d = doc(&["Fetch User", "fetch user"]);
        let ms = find(&d, "fetch");
        assert_eq!(ms.len(), 2);
        assert_eq!(texts(&d, &ms), vec!["Fetch", "fetch"]);
    }

    #[test]
    fn uppercase_in_query_makes_it_case_sensitive() {
        let d = doc(&["Fetch User", "fetch user"]);
        let ms = find(&d, "Fetch");
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].line, 0);
    }

    #[test]
    fn reported_ranges_slice_the_original_text() {
        // The offsets must be valid byte indices into the *unfolded* line, which
        // is the whole reason case folding is done one character at a time.
        let d = doc(&["Grüße, WELT", "straße"]);
        let ms = find(&d, "grüße");
        assert_eq!(texts(&d, &ms), vec!["Grüße"]);
    }

    #[test]
    fn matches_are_non_overlapping() {
        let d = doc(&["aaaa"]);
        assert_eq!(find(&d, "aa").len(), 2);
    }

    #[test]
    fn multibyte_offsets_stay_valid() {
        let d = doc(&["日本語のテキスト", "テキスト"]);
        let ms = find(&d, "テキスト");
        assert_eq!(texts(&d, &ms), vec!["テキスト", "テキスト"]);
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(find(&doc(&["anything"]), "").is_empty());
    }

    #[test]
    fn stepping_wraps_around() {
        let d = doc(&["x", "x", "x"]);
        let mut s = Search { query: "x".into(), ..Default::default() };
        s.refresh(&d);
        assert_eq!(s.matches.len(), 3);
        assert_eq!(s.step(true).unwrap().line, 1);
        assert_eq!(s.step(true).unwrap().line, 2);
        assert_eq!(s.step(true).unwrap().line, 0, "should wrap to the start");
        assert_eq!(s.step(false).unwrap().line, 2, "should wrap backwards");
    }

    #[test]
    fn refining_a_query_keeps_the_focus_nearby() {
        // Typing another character should not throw you back to the top.
        let d = doc(&["fetch", "filler", "filler", "fetch_user"]);
        let mut s = Search { query: "fetch".into(), ..Default::default() };
        s.refresh(&d);
        s.current = 1; // focused on line 3
        s.query = "fetch_".into();
        s.refresh(&d);
        assert_eq!(s.current_match().unwrap().line, 3);
    }
}
