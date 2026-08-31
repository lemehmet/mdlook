//! Drawing the file browser.
//!
//! The sidebar owns the divider column on its right edge, so the caller hands
//! over one rectangle and gets back a pane that is visually finished. Nothing
//! here reads the filesystem or mutates the tree: it draws whatever
//! [`crate::files::Tree`] currently says is visible.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::files::detect::{self, Class};
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

    let style = row_style(row, theme);
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

/// The colour of a row, decided from its name alone.
///
/// A class wins over a dot: a hidden thumbnail is still a picture, and knowing
/// that is most of what the colours are for. A directory keeps the directory
/// colour whatever it is called — the arrow beside it already says a folder is
/// coming — while a dot-directory has no such marker, so the dim style carries
/// that instead.
fn row_style(row: &Row, theme: &Theme) -> Style {
    let class = detect::class(&row.name);
    if row.is_dir {
        return if class == Class::Hidden { theme.tree_hidden } else { theme.tree_dir };
    }
    match class {
        Class::Image => theme.tree_image,
        Class::Pdf => theme.tree_pdf,
        Class::Markdown => theme.tree_markdown,
        Class::Source => theme.tree_source,
        Class::Hidden => theme.tree_hidden,
        Class::Other => theme.text,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::layout::ThemeKind;

    fn entry(name: &str, is_dir: bool) -> Row {
        Row {
            path: PathBuf::from(name),
            name: name.to_string(),
            depth: 0,
            is_dir,
            is_link: false,
            expanded: false,
            note: None,
        }
    }

    #[test]
    fn each_kind_of_file_is_coloured_as_what_it_is() {
        let theme = Theme::default();
        for name in ["diagram.png", "photo.jpg", "banner.gif"] {
            assert_eq!(row_style(&entry(name, false), &theme), theme.tree_image, "{name}");
        }
        assert_eq!(row_style(&entry("spec.pdf", false), &theme), theme.tree_pdf);
        assert_eq!(row_style(&entry("README.md", false), &theme), theme.tree_markdown);
        for name in ["main.c", "widget.hpp", "lib.rs", "server.go", "tool.py", "view.tsx"] {
            assert_eq!(row_style(&entry(name, false), &theme), theme.tree_source, "{name}");
        }
        assert_eq!(row_style(&entry("LICENSE", false), &theme), theme.text);
    }

    #[test]
    fn a_dot_is_dim_only_when_nothing_better_describes_the_file() {
        let theme = Theme::default();
        assert_eq!(row_style(&entry(".gitignore", false), &theme), theme.tree_hidden);
        assert_eq!(row_style(&entry(".cache", true), &theme), theme.tree_hidden);
        assert_eq!(row_style(&entry(".thumb.png", false), &theme), theme.tree_image);
    }

    #[test]
    fn a_directory_stays_the_directory_colour() {
        let theme = Theme::default();
        assert_eq!(row_style(&entry("src", true), &theme), theme.tree_dir);
        assert_eq!(row_style(&entry("notes.md", true), &theme), theme.tree_dir);
    }

    #[test]
    fn no_two_classes_look_alike_in_any_theme() {
        for kind in [ThemeKind::Dark, ThemeKind::Light, ThemeKind::Mono] {
            let theme = Theme::new(kind);
            let styles = [
                ("dir", theme.tree_dir),
                ("image", theme.tree_image),
                ("pdf", theme.tree_pdf),
                ("markdown", theme.tree_markdown),
                ("source", theme.tree_source),
                ("hidden", theme.tree_hidden),
                ("plain", theme.text),
            ];
            for (index, (name, style)) in styles.iter().enumerate() {
                for (other_name, other) in styles[index + 1..].iter() {
                    assert_ne!(style, other, "{name} and {other_name} match in {kind:?}");
                }
            }
        }
    }

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
