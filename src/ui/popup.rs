//! A scrollable list overlay, shared by every index in the viewer.
//!
//! Search results, the link list, the outline and the help screen are all "a
//! titled list you move through, where the selection points at a line in the
//! document". Writing that once means the four of them cannot drift apart in
//! behaviour, and moving between them costs the reader nothing to learn.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Widget};

use crate::layout::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopupKind {
    Search,
    Links,
    Outline,
    Help,
}

impl PopupKind {
    /// Whether moving the selection should scroll the document behind the popup.
    ///
    /// True for every index that points *into* the document — the preview is the
    /// whole value of the list. Help points nowhere, so it stays put.
    pub fn previews(self) -> bool {
        !matches!(self, PopupKind::Help)
    }
}

#[derive(Clone, Debug)]
pub struct PopupRow {
    /// One or more display lines. Links use two: text, then the URL beneath it.
    pub lines: Vec<Line<'static>>,
    /// The document line this row points at.
    pub target: usize,
}

impl PopupRow {
    pub fn height(&self) -> usize {
        self.lines.len().max(1)
    }
}

#[derive(Clone, Debug)]
pub struct Popup {
    pub kind: PopupKind,
    pub title: String,
    pub rows: Vec<PopupRow>,
    pub selected: usize,
    /// Index of the first row drawn, maintained by [`Popup::scroll_into_view`].
    pub offset: usize,
}

impl Popup {
    pub fn new(kind: PopupKind, title: impl Into<String>, rows: Vec<PopupRow>) -> Self {
        Self { kind, title: title.into(), rows, selected: 0, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn target(&self) -> Option<usize> {
        self.rows.get(self.selected).map(|r| r.target)
    }

    /// Move the selection, wrapping at both ends to match `n`/`N` in the body.
    pub fn step(&mut self, forward: bool) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = if forward {
            (self.selected + 1) % self.rows.len()
        } else {
            (self.selected + self.rows.len() - 1) % self.rows.len()
        };
    }

    pub fn jump(&mut self, to: usize) {
        self.selected = to.min(self.rows.len().saturating_sub(1));
    }

    /// Adjust `offset` so the selected row is fully visible in `height` rows.
    pub fn scroll_into_view(&mut self, height: usize) {
        if self.rows.is_empty() || height == 0 {
            return;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
            return;
        }
        // Walk backwards from the selection until the accumulated height would
        // overflow; whatever we reach is the earliest row that can still be
        // drawn with the selection on screen.
        let mut used = 0;
        let mut first = self.selected;
        loop {
            used += self.rows[first].height();
            if used > height {
                first += 1;
                break;
            }
            if first == 0 {
                break;
            }
            first -= 1;
        }
        self.offset = self.offset.max(first.min(self.selected));
    }

    /// The rows that fit in `height`, as (row index, is_selected, line).
    fn visible(&self, height: usize) -> Vec<(usize, bool, Line<'static>)> {
        let mut out = Vec::new();
        let mut used = 0;
        for (index, row) in self.rows.iter().enumerate().skip(self.offset) {
            if used + row.height() > height && used > 0 {
                break;
            }
            for line in &row.lines {
                if used >= height {
                    break;
                }
                out.push((index, index == self.selected, line.clone()));
                used += 1;
            }
        }
        out
    }
}

/// Place a popup of `height` rows inside `area`, avoiding `avoid_row`.
///
/// The list exists to preview a location in the document, so covering that
/// location with the list itself would defeat it. When the target is in the top
/// half the popup drops to the bottom, and vice versa.
pub fn placement(area: Rect, height: u16, avoid_row: Option<u16>) -> Rect {
    // Inset from the edges, but never wider than what we were given — on a very
    // narrow terminal the minimum would otherwise overflow the screen.
    let width = area.width.saturating_sub(4).clamp(20, 100).min(area.width);
    let height = height.min(area.height.saturating_sub(2)).max(3);
    let x = area.x + (area.width.saturating_sub(width)) / 2;

    let top = area.y + 1;
    let bottom = area.y + area.height.saturating_sub(height + 1);

    let y = match avoid_row {
        // Target in the upper half: sit below it.
        Some(row) if row < area.y + area.height / 2 => bottom,
        Some(_) => top,
        None => area.y + (area.height.saturating_sub(height)) / 2,
    };

    Rect { x, y: y.max(area.y), width, height }
}

pub fn render(popup: &Popup, area: Rect, theme: &Theme, buffer: &mut ratatui::buffer::Buffer) {
    Clear.render(area, buffer);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme.popup_border)
        .title(Line::styled(format!(" {} ", popup.title), theme.popup_title));
    let inner = block.inner(area);
    block.render(area, buffer);

    if popup.rows.is_empty() {
        Paragraph::new(Line::styled("  no matches", theme.popup_dim)).render(inner, buffer);
        return;
    }

    let lines: Vec<Line<'static>> = popup
        .visible(inner.height as usize)
        .into_iter()
        .map(|(_, selected, line)| {
            if selected {
                highlight(line, theme.popup_selected, inner.width as usize)
            } else {
                line
            }
        })
        .collect();

    Paragraph::new(lines).render(inner, buffer);
}

/// Paint a row as selected, padding it so the highlight spans the full width.
fn highlight(line: Line<'static>, style: Style, width: usize) -> Line<'static> {
    let used: usize = line.spans.iter().map(|s| crate::layout::wrap::text_width(&s.content)).sum();

    let mut spans = line.spans;
    for span in spans.iter_mut() {
        // Patch rather than replace so the row keeps its own emphasis; only the
        // colours are overridden.
        span.style = span.style.patch(style);
    }
    if used < width {
        spans.push(ratatui::text::Span::styled(" ".repeat(width - used), style));
    }
    Line::from(spans)
}
