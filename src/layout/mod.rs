pub mod code;
pub mod table;
pub mod theme;
pub mod wrap;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::doc::*;
pub use theme::{Theme, ThemeKind};
use wrap::{cells_to_spans, cells_width, expand_tabs, text_width, Cell, Unit};

/// A heading, and the rendered line it starts at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub level: u8,
    pub text: String,
    pub line: usize,
}

/// A link occurrence, and the rendered line it appears on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkRef {
    pub text: String,
    pub url: String,
    pub line: usize,
}

/// A laid-out document at one specific width.
///
/// `lines` and `plain` are built together and are always the same length, index
/// for index. Everything downstream — search, scrolling, the match popup —
/// depends on that invariant: a hit found at `plain[i]` is on screen row `i`, with
/// no second pass that could disagree.
#[derive(Clone, Debug, Default)]
pub struct RenderedDoc {
    pub lines: Vec<Line<'static>>,
    pub plain: Vec<String>,
    pub anchors: Vec<Anchor>,
    pub links: Vec<LinkRef>,
    pub width: usize,
}

impl RenderedDoc {
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The nearest heading at or above `line`, used for search breadcrumbs.
    pub fn heading_at(&self, line: usize) -> Option<&Anchor> {
        self.anchors.iter().rev().find(|a| a.line <= line)
    }
}

/// Lay a document out at a given width.
///
/// Pure: the result depends on nothing but these three arguments. No clock, no
/// randomness, no environment, no locale — which is what lets the snapshot tests
/// be meaningful and guarantees run-to-run stability.
pub fn layout(document: &Document, width: usize, theme: &Theme) -> RenderedDoc {
    let mut sink = Sink::new(width.max(8), theme);
    sink.blocks(&document.blocks, document, &[]);
    sink.trim_trailing_blanks();
    sink.finish()
}

/// Accumulates output while enforcing the lines/plain lockstep invariant.
struct Sink<'a> {
    width: usize,
    theme: &'a Theme,
    lines: Vec<Line<'static>>,
    plain: Vec<String>,
    anchors: Vec<Anchor>,
    links: Vec<LinkRef>,
    /// Suppresses exactly one upcoming blank separator. Used to keep tight lists
    /// tight without threading a flag through every block-rendering call.
    skip_separator: bool,
    /// Nesting depth of the enclosing lists, for choosing the bullet glyph.
    list_depth: usize,
}

impl<'a> Sink<'a> {
    fn new(width: usize, theme: &'a Theme) -> Self {
        Self {
            width,
            theme,
            lines: Vec::new(),
            plain: Vec::new(),
            anchors: Vec::new(),
            links: Vec::new(),
            skip_separator: false,
            list_depth: 0,
        }
    }

    /// The single place a line is appended.
    ///
    /// Routing every emission through here is what keeps `plain[i]` describing
    /// `lines[i]`; no caller gets to add one without the other. It is also the
    /// choke point where control characters are neutralised: a markdown file is
    /// untrusted input, and raw terminal escapes reaching the screen would be
    /// commands rather than text. Doing it here rather than in each producer
    /// means a new block type cannot forget to.
    fn push(&mut self, mut spans: Vec<Span<'static>>) {
        for span in spans.iter_mut() {
            if wrap::has_controls(&span.content) {
                span.content = span.content.chars().map(wrap::sanitize).collect::<String>().into();
            }
        }
        let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
        self.plain.push(plain);
        self.lines.push(Line::from(spans));
    }

    fn push_blank(&mut self) {
        self.push(Vec::new());
    }

    /// Add a blank separator unless we are at the top or already have one.
    fn separate(&mut self) {
        if std::mem::take(&mut self.skip_separator) {
            return;
        }
        if self.lines.is_empty() {
            return;
        }
        if self.plain.last().map(|l| l.trim().is_empty()) == Some(true) {
            return;
        }
        self.push_blank();
    }

    fn trim_trailing_blanks(&mut self) {
        while self.plain.last().map(|l| l.trim().is_empty()) == Some(true) {
            self.plain.pop();
            self.lines.pop();
        }
    }

    fn finish(self) -> RenderedDoc {
        RenderedDoc {
            lines: self.lines,
            plain: self.plain,
            anchors: self.anchors,
            links: self.links,
            width: self.width,
        }
    }

    fn blocks(&mut self, blocks: &[Block], doc: &Document, prefix: &[Cell]) {
        for block in blocks {
            self.block(block, doc, prefix);
        }
    }

    fn block(&mut self, block: &Block, doc: &Document, prefix: &[Cell]) {
        let indent = cells_width(prefix);
        let avail = self.width.saturating_sub(indent).max(1);

        match block {
            Block::Paragraph(inlines) => {
                self.separate();
                let units = self.flatten(inlines, doc, self.theme.text);
                self.emit_wrapped(&units, avail, prefix, prefix);
            }

            Block::Heading { level, content } => {
                self.separate();
                let style = self.theme.heading(*level);
                let units = self.flatten(content, doc, style);
                let start = self.lines.len();
                self.emit_wrapped(&units, avail, prefix, prefix);

                // Record the anchor from the rendered text, so the outline and
                // search breadcrumbs show exactly what is on screen.
                let text = self.plain[start..].join(" ").trim().to_string();
                self.anchors.push(Anchor { level: *level, text, line: start });

                // An underline under H1/H2 gives the eye a section boundary
                // without reintroducing the `#` characters we just removed.
                if *level <= 2 {
                    let rule_width = self.plain[start..]
                        .iter()
                        .map(|l| text_width(l))
                        .max()
                        .unwrap_or(0)
                        .saturating_sub(indent)
                        .clamp(1, avail);
                    let glyph = if *level == 1 { '━' } else { '─' };
                    let mut spans = cells_to_spans(prefix);
                    spans.push(Span::styled(
                        glyph.to_string().repeat(rule_width),
                        style.remove_modifier(Modifier::BOLD),
                    ));
                    self.push(spans);
                }
            }

            Block::CodeBlock { lang, code } => {
                self.separate();
                self.code_block(lang.as_deref(), code, avail, prefix);
            }

            Block::BlockQuote(inner) => {
                self.separate();
                let mut nested = prefix.to_vec();
                nested.push(('▌', self.theme.quote_bar));
                nested.push((' ', self.theme.quote_bar));
                self.blocks(inner, doc, &nested);
            }

            Block::List { kind, tight, items } => {
                self.separate();
                self.list(*kind, *tight, items, doc, prefix);
            }

            Block::Table(table) => {
                self.separate();
                table::render(self, table, doc, avail, prefix);
            }

            Block::Rule => {
                self.separate();
                let mut spans = cells_to_spans(prefix);
                spans.push(Span::styled("─".repeat(avail), self.theme.rule));
                self.push(spans);
            }

            Block::Html(html) => {
                self.separate();
                for raw in html.lines() {
                    let mut spans = cells_to_spans(prefix);
                    spans.push(Span::styled(expand_tabs(raw), self.theme.html));
                    self.push(spans);
                }
            }

            Block::FootnoteDef { label, blocks } => {
                self.separate();
                let mut spans = cells_to_spans(prefix);
                spans.push(Span::styled(format!("[{label}]"), self.theme.footnote));
                self.push(spans);
                // The body belongs to the label, so it starts on the next line
                // rather than across a blank.
                self.skip_separator = true;
                let mut nested = prefix.to_vec();
                nested.extend([(' ', Style::new()), (' ', Style::new())]);
                self.blocks(blocks, doc, &nested);
            }
        }
    }

    fn list(
        &mut self,
        kind: ListKind,
        tight: bool,
        items: &[ListItem],
        doc: &Document,
        prefix: &[Cell],
    ) {
        self.list_depth += 1;
        for (index, item) in items.iter().enumerate() {
            // A loose list keeps the author's blank lines between items. This is
            // an explicit push rather than a `skip_separator` dance because an
            // item whose first block is a paragraph never calls `separate()` at
            // all, so a flag set here would survive past the list and swallow the
            // separator belonging to whatever block comes next.
            if !tight && index > 0 {
                self.push_blank();
            }
            let marker = match kind {
                ListKind::Ordered(start) => format!("{}. ", start + index as u64),
                // A task list's checkbox already serves as the marker; adding a
                // bullet in front of it renders "• ☐ todo", which GitHub does not.
                ListKind::Bullet if item.task.is_some() => String::new(),
                ListKind::Bullet => {
                    // Vary the glyph with nesting depth so structure survives
                    // even when the indentation is squeezed on a narrow terminal.
                    let glyph = ['•', '◦', '▪'][(self.list_depth - 1).min(2)];
                    format!("{glyph} ")
                }
            };
            let marker_width = text_width(&marker);

            let mut first = prefix.to_vec();
            first.extend(marker.chars().map(|c| (c, self.theme.marker)));

            // Continuation lines align under the item text, not the marker.
            let mut rest = prefix.to_vec();
            rest.extend(std::iter::repeat_n((' ', Style::new()), marker_width));

            // A task checkbox belongs to the item's first line.
            let checkbox = item.task.map(|checked| {
                let glyph = if checked { "☑ " } else { "☐ " };
                let style = if checked {
                    self.theme.marker.add_modifier(Modifier::BOLD)
                } else {
                    self.theme.marker
                };
                (glyph, style)
            });

            self.list_item(item, doc, &first, &rest, checkbox, tight);
        }
        self.list_depth -= 1;
    }

    fn list_item(
        &mut self,
        item: &ListItem,
        doc: &Document,
        first: &[Cell],
        rest: &[Cell],
        checkbox: Option<(&str, Style)>,
        tight: bool,
    ) {
        let Some((head, tail)) = item.blocks.split_first() else {
            self.push(cells_to_spans(first));
            return;
        };

        // Render the first block with the marker prefix, the rest indented.
        let mut lead = first.to_vec();
        if let Some((glyph, style)) = checkbox {
            lead.extend(glyph.chars().map(|c| (c, style)));
        }
        let indent = cells_width(&lead);
        let avail = self.width.saturating_sub(indent).max(1);

        match head {
            Block::Paragraph(inlines) => {
                let units = self.flatten(inlines, doc, self.theme.text);
                let mut continuation = rest.to_vec();
                if let Some((glyph, _)) = checkbox {
                    continuation
                        .extend(std::iter::repeat_n((' ', Style::new()), text_width(glyph)));
                }
                self.emit_wrapped(&units, avail, &lead, &continuation);
            }
            other => {
                // Non-paragraph first child (a nested list, a code block): give it
                // the marker on its own line rather than trying to inline it. The
                // block would otherwise insert a blank directly under the marker.
                self.push(cells_to_spans(&lead));
                self.skip_separator = true;
                self.block(other, doc, rest);
            }
        }

        for block in tail {
            // A nested list inside a tight item is part of the same visual run;
            // a blank line before it would break the outline apart.
            if tight {
                self.skip_separator = true;
            }
            self.block(block, doc, rest);
        }
    }

    fn code_block(&mut self, lang: Option<&str>, code: &str, avail: usize, prefix: &[Cell]) {
        let highlighted = code::highlight(code, lang, self.theme);
        let bg = self.theme.code_block_bg;

        // The language tag rides on the top border so it never eats a code line.
        let mut header = cells_to_spans(prefix);
        let label = lang.unwrap_or("");
        let bar = if label.is_empty() {
            "─".repeat(avail)
        } else {
            let used = text_width(label) + 3;
            format!("─ {label} {}", "─".repeat(avail.saturating_sub(used)))
        };
        header.push(Span::styled(bar, self.theme.code_fence_label));
        self.push(header);

        for line in highlighted {
            let mut spans = cells_to_spans(prefix);
            spans.push(Span::styled("  ", Style::new()));
            let mut used = 2;

            for span in line {
                let w = text_width(&span.content);
                // Long code lines are truncated rather than wrapped: re-wrapping
                // code destroys the alignment that makes it readable, and the
                // horizontal-scroll affordance is a better answer than a fake
                // line break in the middle of an expression.
                if used + w > avail {
                    let room = avail.saturating_sub(used);
                    if room > 0 {
                        let cut: String = span
                            .content
                            .chars()
                            .scan(0usize, |acc, c| {
                                *acc += wrap::char_width(c);
                                (*acc <= room).then_some(c)
                            })
                            .collect();
                        let style = span.style;
                        spans.push(Span::styled(cut, style));
                    }
                    used = avail;
                    break;
                }
                used += w;
                spans.push(span);
            }

            // Pad to the full width so the code background reads as a solid block.
            if let Some(color) = bg {
                if used < avail {
                    spans.push(Span::styled(" ".repeat(avail - used), Style::new()));
                }
                for span in spans.iter_mut().skip(cells_to_spans(prefix).len()) {
                    span.style = span.style.bg(color);
                }
            }
            self.push(spans);
        }

        let mut footer = cells_to_spans(prefix);
        footer.push(Span::styled("─".repeat(avail), self.theme.code_fence_label));
        self.push(footer);
    }

    /// Wrap `units` and emit each line with the appropriate prefix.
    fn emit_wrapped(&mut self, units: &[Unit], avail: usize, first: &[Cell], rest: &[Cell]) {
        let wrapped = wrap::wrap(units, avail);
        for (index, cells) in wrapped.iter().enumerate() {
            let prefix = if index == 0 { first } else { rest };
            let mut spans = cells_to_spans(prefix);
            spans.extend(cells_to_spans(cells));
            self.push(spans);
        }
    }

    /// Turn inline content into styled character units, recording link positions.
    fn flatten(&mut self, inlines: &[Inline], doc: &Document, base: Style) -> Vec<Unit> {
        let mut units = Vec::new();
        let line = self.lines.len();

        for inline in inlines {
            match inline {
                Inline::Text { text, style } => {
                    let resolved = base.patch(self.theme.inline(*style));
                    for c in expand_tabs(text).chars() {
                        units.push(Unit::Char(c, resolved));
                    }
                    if let Some(index) = style.link {
                        self.record_link(index, text, doc, line);
                    }
                }
                Inline::HardBreak => units.push(Unit::Break),
                // Normalisation resolves these; anything left is a no-op.
                Inline::SoftBreak => units.push(Unit::Char(' ', base)),
                Inline::FootnoteRef(label) => {
                    let style = base.patch(self.theme.footnote);
                    for c in format!("[{label}]").chars() {
                        units.push(Unit::Char(c, style));
                    }
                }
                Inline::Image { alt } => {
                    let style = base.patch(self.theme.image);
                    let text = if alt.is_empty() {
                        "[image]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    };
                    for c in text.chars() {
                        units.push(Unit::Char(c, style));
                    }
                }
            }
        }
        units
    }

    /// Record a link occurrence, merging text that arrives as several runs.
    fn record_link(&mut self, index: u32, text: &str, doc: &Document, line: usize) {
        let Some(target) = doc.links.get(index as usize) else {
            return;
        };
        // The link list renders these directly rather than through `push`, so
        // they are neutralised here instead. A URL is an especially good place to
        // hide an escape sequence, since nobody reads one closely.
        let clean = |s: &str| -> String { s.chars().map(wrap::sanitize).collect() };

        // A styled link ("**bold** link") reaches us as multiple runs on the same
        // line; append rather than listing the same link once per run.
        if let Some(last) = self.links.last_mut() {
            if last.line == line && last.url == clean(&target.url) {
                last.text.push_str(&clean(text));
                return;
            }
        }
        self.links.push(LinkRef { text: clean(text), url: clean(&target.url), line });
    }
}
