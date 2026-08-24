//! Drawing the file browser.
//!
//! The sidebar owns the divider column on its right edge, so the caller hands
//! over one rectangle and gets back a pane that is visually finished. Nothing
//! here reads the filesystem or mutates the tree: it draws whatever
//! [`crate::files::Tree`] currently says is visible.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::files::tree::{Row, Tree};
use crate::layout::wrap::{self, text_width};
use crate::layout::Theme;

/// Below this the sidebar is hidden rather than squeezed.
///
/// A pane too narrow to show a file name is worse than no pane: it costs the
/// document columns and gives back nothing readable.
pub const MIN_SPLIT_WIDTH: u16 = 60;

/// Split an area into (sidebar, document), or `None` when there is no room.
///
/// Returning `None` rather than a tiny sidebar is what makes the browser
/// degrade gracefully on a narrow terminal: the document simply gets the whole
/// frame back until there is room again.
pub fn split(area: Rect, sidebar_width: usize) -> Option<(Rect, Rect)> {
    if area.width < MIN_SPLIT_WIDTH {
        return None;
    }
    // Never take more than a third: the document is what the reader came for.
    let width = (sidebar_width as u16).min(area.width / 3).max(12);
    let sidebar = Rect { width, ..area };
    let document = Rect { x: area.x + width, width: area.width - width, ..area };
    Some((sidebar, document))
}

pub fn render(tree: &Tree, area: Rect, theme: &Theme, focused: bool, buf: &mut Buffer) {
    if area.width < 2 || area.height < 1 {
        return;
    }
    let body_width = area.width - 1;

    for y in area.y..area.y + area.height {
        buf[(area.x + body_width, y)].set_char('│').set_style(theme.tree_divider);
    }

    let header = Rect { width: body_width, height: 1, ..area };
    Paragraph::new(head_line(tree, theme, body_width as usize)).render(header, buf);

    if area.height < 2 {
        return;
    }
    let list = Rect { y: area.y + 1, height: area.height - 1, width: body_width, ..area };

    let end = (tree.offset + list.height as usize).min(tree.rows.len());
    let lines: Vec<Line<'static>> = (tree.offset..end)
        .map(|index| {
            row_line(&tree.rows[index], index == tree.selected, focused, theme, body_width as usize)
        })
        .collect();
    Paragraph::new(lines).render(list, buf);
}

/// The root's name, plus the filter when one is active.
fn head_line(tree: &Tree, theme: &Theme, width: usize) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{}/", tree.root().file_name().map(|n| n.to_string_lossy()).unwrap_or_default()),
        theme.popup_title,
    )];
    if !tree.filter.is_empty() {
        spans.push(Span::styled(format!("  /{}", tree.filter), theme.popup_dim));
    }
    Line::from(wrap::truncate(spans, width))
}

fn row_line(
    row: &Row,
    selected: bool,
    focused: bool,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    let indent = "  ".repeat(row.depth);

    if let Some(note) = &row.note {
        let spans = vec![Span::styled(format!("{indent}{note}"), theme.popup_dim)];
        return Line::from(wrap::truncate(spans, width));
    }

    // A directory says whether it is open before it says its name, so the shape
    // of the tree reads down the left edge without looking at the text.
    let glyph = match (row.is_dir, row.expanded) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        (false, _) => "  ",
    };
    let mut name = row.name.clone();
    if row.is_dir {
        name.push('/');
    }
    if row.is_link {
        name.push_str(" →");
    }

    let style = if row.is_dir { theme.tree_dir } else { theme.text };
    let mut spans = wrap::truncate(
        vec![Span::styled(format!("{indent}{glyph}"), theme.popup_dim), Span::styled(name, style)],
        width,
    );

    if selected {
        // Pad to the full width so the selection reads as a bar rather than as
        // a highlight that stops at the end of the name.
        let used: usize = spans.iter().map(|s| text_width(&s.content)).sum();
        if used < width {
            spans.push(Span::raw(" ".repeat(width - used)));
        }
        let selection = if focused { theme.tree_selection } else { theme.tree_selection_idle };
        for span in spans.iter_mut() {
            span.style = span.style.patch(selection);
        }
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_terminal_gives_the_whole_frame_to_the_document() {
        let narrow = Rect { x: 0, y: 0, width: MIN_SPLIT_WIDTH - 1, height: 20 };
        assert_eq!(split(narrow, 30), None);
    }

    #[test]
    fn the_split_covers_the_area_exactly() {
        for width in MIN_SPLIT_WIDTH..200 {
            let area = Rect { x: 0, y: 0, width, height: 20 };
            let (sidebar, document) = split(area, 30).expect("wide enough to split");
            assert_eq!(sidebar.width + document.width, width, "a column went missing at {width}");
            assert_eq!(document.x, sidebar.width);
        }
    }

    #[test]
    fn the_sidebar_never_takes_more_than_a_third() {
        let area = Rect { x: 0, y: 0, width: 60, height: 20 };
        let (sidebar, _) = split(area, 50).expect("wide enough");
        assert!(sidebar.width <= 20, "sidebar took {} of 60 columns", sidebar.width);
    }
}
