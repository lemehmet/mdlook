//! Text selected with the mouse in the document pane.
//!
//! mdlook captures the mouse so the wheel scrolls, and capturing it takes away
//! the terminal's own click-and-drag selection. This puts it back, and does a
//! better job than the terminal could: the selection is held in *document*
//! coordinates, so it survives scrolling and can run past one screen, and it
//! stays inside the document pane instead of sweeping up the file tree and the
//! status bar beside it.

use std::ops::Range;

use crate::layout::wrap::char_width_after;
use crate::layout::RenderedDoc;

/// A cell in the laid-out document: a line index and a display column.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Point {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    /// Where the button went down. Fixed for the life of the selection.
    pub anchor: Point,
    /// Where the pointer is now, or where it was released.
    pub head: Point,
    /// Whether the button is still held, so drags that began somewhere else —
    /// on the tree, in a popup — are not mistaken for this one.
    pub dragging: bool,
}

impl Selection {
    pub fn new(at: Point) -> Self {
        Self { anchor: at, head: at, dragging: true }
    }

    /// A click without a drag selects nothing.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    fn ordered(&self) -> (Point, Point) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// The display columns selected on `line`, end exclusive, or `None` when the
    /// selection does not reach it.
    ///
    /// The cell under the pointer is included at both ends, which is what
    /// dragging across a word in any terminal does. Columns before `gutter` are
    /// never selected: a line-number gutter is the viewer's furniture, not the
    /// file's, and nobody wants it in a paste.
    pub fn columns_on(&self, line: usize, gutter: usize) -> Option<Range<usize>> {
        if self.is_empty() {
            return None;
        }
        let (start, end) = self.ordered();
        if line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line { start.column } else { 0 };
        let to = if line == end.line { end.column + 1 } else { usize::MAX };
        let from = from.max(gutter);
        (from < to).then_some(from..to)
    }

    /// The byte range of `text` that the selection covers on `line`.
    pub fn bytes_on(&self, line: usize, text: &str, gutter: usize) -> Option<Range<usize>> {
        let columns = self.columns_on(line, gutter)?;
        let bytes = byte_range(text, columns);
        (!bytes.is_empty()).then_some(bytes)
    }

    /// The selected text, one document line per line, trailing blanks trimmed.
    pub fn text(&self, doc: &RenderedDoc) -> String {
        if self.is_empty() || doc.plain.is_empty() {
            return String::new();
        }
        let (start, end) = self.ordered();
        let last = end.line.min(doc.plain.len() - 1);
        let mut lines: Vec<&str> = (start.line..=last)
            .map(|index| {
                let text = &doc.plain[index];
                let range = self.columns_on(index, doc.content_offset).unwrap_or(0..0);
                text[byte_range(text, range)].trim_end()
            })
            .collect();
        // Blank rows at the edges are layout spacing the drag happened to cross.
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let leading = lines.iter().take_while(|l| l.is_empty()).count();
        lines.drain(..leading);
        lines.join("\n")
    }
}

/// The bytes of `text` whose cells fall in `columns`.
///
/// A double-width character is taken if either of its cells is, so a drag that
/// ends on the right half of a CJK glyph still gets the glyph. Zero-width
/// characters ride along with whatever they follow.
fn byte_range(text: &str, columns: Range<usize>) -> Range<usize> {
    let mut start = None;
    let mut end = 0;
    let mut column = 0;
    let mut previous = None;
    let mut previous_taken = false;

    for (offset, c) in text.char_indices() {
        let width = char_width_after(c, previous);
        let taken = if width == 0 {
            previous_taken
        } else {
            column < columns.end && column + width > columns.start
        };
        if taken {
            start.get_or_insert(offset);
            end = offset + c.len_utf8();
        } else if start.is_some() && width > 0 {
            break;
        }
        column += width;
        previous = Some(c);
        previous_taken = taken;
    }

    match start {
        Some(start) => start..end,
        None => 0..0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(line: usize, column: usize) -> Point {
        Point { line, column }
    }

    fn drag(from: Point, to: Point) -> Selection {
        Selection { anchor: from, head: to, dragging: false }
    }

    #[test]
    fn columns_map_to_bytes_through_wide_characters() {
        // `日` and `本` are two cells each: columns 0-1 and 2-3.
        let text = "日本語 text";
        assert_eq!(&text[byte_range(text, 0..1)], "日");
        assert_eq!(&text[byte_range(text, 1..3)], "日本", "half of each glyph takes both");
        assert_eq!(&text[byte_range(text, 7..11)], "text");
        assert_eq!(byte_range(text, 40..50), 0..0, "past the end is nothing");
    }

    #[test]
    fn combining_marks_stay_with_their_base() {
        let text = "e\u{301}x";
        assert_eq!(&text[byte_range(text, 0..1)], "e\u{301}");
        assert_eq!(&text[byte_range(text, 1..2)], "x");
    }

    #[test]
    fn a_reversed_drag_selects_the_same_cells() {
        let forward = drag(point(1, 2), point(3, 4));
        let backward = drag(point(3, 4), point(1, 2));
        for line in 0..5 {
            assert_eq!(forward.columns_on(line, 0), backward.columns_on(line, 0));
        }
    }

    #[test]
    fn the_gutter_is_never_selected() {
        let selection = drag(point(0, 0), point(2, 10));
        assert_eq!(selection.columns_on(1, 6), Some(6..usize::MAX));
        assert_eq!(selection.columns_on(2, 6), Some(6..11));
        // A drag entirely inside the gutter selects nothing on that line.
        assert_eq!(drag(point(0, 1), point(0, 3)).columns_on(0, 6), None);
    }

    #[test]
    fn a_click_selects_nothing() {
        let click = Selection::new(point(4, 7));
        assert!(click.is_empty());
        assert_eq!(click.columns_on(4, 0), None);
    }
}
