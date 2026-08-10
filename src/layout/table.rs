//! GFM table layout.
//!
//! Columns are sized from their content, then shrunk proportionally if the table
//! is wider than the terminal. Cells *wrap* rather than truncate — in an API
//! reference the description column is usually the widest and always the one you
//! actually need to read, so silently cutting it off would defeat the purpose.

use ratatui::style::Style;
use ratatui::text::Span;

use super::wrap::{self, cells_to_spans, cells_width, Cell};
use super::Sink;
use crate::doc::{Align, Document, Inlines, Table};

/// Narrowest a column may be squeezed to before we accept overflow instead.
const MIN_COLUMN: usize = 3;

pub(super) fn render(
    sink: &mut Sink,
    table: &Table,
    doc: &Document,
    avail: usize,
    prefix: &[Cell],
) {
    let columns = table.head.len().max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }

    // Flatten every cell once; both sizing and rendering need the same content,
    // and re-flattening would record each link twice.
    let header: Vec<Vec<Cell>> =
        row_cells(sink, &table.head, columns, doc, sink.theme.table_header);
    let body: Vec<Vec<Vec<Cell>>> =
        table.rows.iter().map(|row| row_cells(sink, row, columns, doc, sink.theme.text)).collect();

    let widths = solve_widths(&header, &body, columns, avail);
    let border = sink.theme.table_border;

    push_rule(sink, prefix, &widths, border, '┌', '┬', '┐');
    push_row(sink, prefix, &header, &widths, &table.align, border);
    push_rule(sink, prefix, &widths, border, '├', '┼', '┤');
    for row in &body {
        push_row(sink, prefix, row, &widths, &table.align, border);
    }
    push_rule(sink, prefix, &widths, border, '└', '┴', '┘');
}

/// Flatten one row's cells, padding short rows out to the column count.
fn row_cells(
    sink: &mut Sink,
    row: &[Inlines],
    columns: usize,
    doc: &Document,
    base: Style,
) -> Vec<Vec<Cell>> {
    let mut out = Vec::with_capacity(columns);
    for index in 0..columns {
        let cells = match row.get(index) {
            Some(inlines) => sink
                .flatten(inlines, doc, base)
                .into_iter()
                .map(|u| match u {
                    wrap::Unit::Char(c, style) => (c, style),
                    // A hard break inside a table cell becomes a space; a real
                    // break would desynchronise the row's cell heights.
                    wrap::Unit::Break => (' ', base),
                })
                .collect(),
            None => Vec::new(),
        };
        out.push(cells);
    }
    out
}

/// Choose column widths that fit the budget.
fn solve_widths(
    header: &[Vec<Cell>],
    body: &[Vec<Vec<Cell>>],
    columns: usize,
    avail: usize,
) -> Vec<usize> {
    // "│ " before each column plus a trailing "│".
    let chrome = columns * 3 + 1;
    let budget = avail.saturating_sub(chrome).max(columns * MIN_COLUMN);

    let mut natural: Vec<usize> = (0..columns)
        .map(|index| {
            let head = header.get(index).map(|c| cells_width(c)).unwrap_or(0);
            let cell = body
                .iter()
                .filter_map(|row| row.get(index))
                .map(|c| cells_width(c))
                .max()
                .unwrap_or(0);
            head.max(cell).max(1)
        })
        .collect();

    let total: usize = natural.iter().sum();
    if total <= budget {
        return natural;
    }

    // Over budget: shrink proportionally, but never below MIN_COLUMN, and give
    // the reclaimed space back to the widest columns.
    let shrinkable: usize = natural.iter().map(|w| w.saturating_sub(MIN_COLUMN)).sum();
    let excess = total - budget;

    if shrinkable == 0 {
        return natural;
    }

    let mut removed = 0usize;
    for width in natural.iter_mut() {
        let room = width.saturating_sub(MIN_COLUMN);
        // Proportional share of the cut, rounded down.
        let cut = (room * excess) / shrinkable;
        *width -= cut;
        removed += cut;
    }
    // Rounding leaves a few columns unaccounted for; take them off the widest.
    while removed < excess {
        let Some(widest) = natural.iter_mut().filter(|w| **w > MIN_COLUMN).max_by_key(|w| **w)
        else {
            break;
        };
        *widest -= 1;
        removed += 1;
    }
    natural
}

fn push_rule(
    sink: &mut Sink,
    prefix: &[Cell],
    widths: &[usize],
    style: Style,
    left: char,
    mid: char,
    right: char,
) {
    let mut text = String::new();
    text.push(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            text.push(mid);
        }
        text.push_str(&"─".repeat(width + 2));
    }
    text.push(right);

    let mut spans = cells_to_spans(prefix);
    spans.push(Span::styled(text, style));
    sink.push(spans);
}

fn push_row(
    sink: &mut Sink,
    prefix: &[Cell],
    row: &[Vec<Cell>],
    widths: &[usize],
    align: &[Align],
    border: Style,
) {
    // Wrap each cell to its column, then emit as many physical lines as the
    // tallest cell needs.
    let wrapped: Vec<Vec<Vec<Cell>>> = row
        .iter()
        .zip(widths)
        .map(|(cells, width)| {
            let units: Vec<wrap::Unit> =
                cells.iter().map(|&(c, style)| wrap::Unit::Char(c, style)).collect();
            wrap::wrap(&units, *width)
        })
        .collect();

    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    for line in 0..height {
        let mut spans = cells_to_spans(prefix);
        for (index, width) in widths.iter().enumerate() {
            spans.push(Span::styled("│ ", border));

            let empty = Vec::new();
            let content = wrapped.get(index).and_then(|cell| cell.get(line)).unwrap_or(&empty);
            let used = cells_width(content);
            let slack = width.saturating_sub(used);
            let (before, after) = match align.get(index).copied().unwrap_or(Align::None) {
                Align::Right => (slack, 0),
                Align::Center => (slack / 2, slack - slack / 2),
                Align::Left | Align::None => (0, slack),
            };

            if before > 0 {
                spans.push(Span::raw(" ".repeat(before)));
            }
            spans.extend(cells_to_spans(content));
            spans.push(Span::raw(" ".repeat(after + 1)));
        }
        spans.push(Span::styled("│", border));
        sink.push(spans);
    }
}
