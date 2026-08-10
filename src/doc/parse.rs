//! `pulldown-cmark` event stream → [`Document`].
//!
//! The one thing this file exists to get right is the `SoftBreak` / `HardBreak`
//! distinction. CommonMark says a line ending inside a paragraph is a *soft* break
//! that renders as a space; only two trailing spaces or a backslash make a *hard*
//! break. Authors routinely hard-wrap source at 80 columns, so a renderer that
//! honors soft breaks reproduces the author's editor width instead of the reader's
//! terminal width. `glow`'s CLI path does exactly that (it passes
//! `WithPreservedNewLines()` unconditionally), which is the bug this tool exists
//! to not have.

use pulldown_cmark::{
    Alignment as CmAlign, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use super::model::*;

/// Containers we can be inside of. Blocks are pushed into the innermost frame.
enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List {
        kind: ListKind,
        items: Vec<ListItem>,
        loose: bool,
    },
    Item {
        task: Option<bool>,
        blocks: Vec<Block>,
        /// Set when a `Paragraph` tag opens directly inside this item.
        /// pulldown-cmark only emits those for loose lists — tight item content
        /// arrives as bare text — so this is how we recover the distinction.
        loose: bool,
    },
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
    Table {
        align: Vec<Align>,
        head: Vec<Inlines>,
        rows: Vec<Vec<Inlines>>,
        row: Vec<Inlines>,
        in_head: bool,
    },
}

/// Which leaf block is currently collecting inline content.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    None,
    Paragraph,
    Heading(u8),
    Cell,
}

pub fn parse(source: &str) -> Document {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    // Smart punctuation is deliberately off: it rewrites quotes and dashes, which
    // would mean the text you search is not the text the file contains.

    Builder::new().run(Parser::new_ext(source, opts))
}

struct Builder {
    stack: Vec<Frame>,
    links: Vec<LinkTarget>,
    /// Emphasis/link state, one entry per open inline tag. The top is current.
    styles: Vec<InlineStyle>,
    inlines: Inlines,
    leaf: Leaf,
    code: Option<(Option<String>, String)>,
    html: Option<String>,
    /// Alt-text buffer; `Some` while inside an image tag.
    alt: Option<String>,
}

impl Builder {
    fn new() -> Self {
        Self {
            stack: vec![Frame::Root(Vec::new())],
            links: Vec::new(),
            styles: vec![InlineStyle::default()],
            inlines: Vec::new(),
            leaf: Leaf::None,
            code: None,
            html: None,
            alt: None,
        }
    }

    fn style(&self) -> InlineStyle {
        *self.styles.last().expect("style stack is never empty")
    }

    fn push_style(&mut self, f: impl FnOnce(&mut InlineStyle)) {
        let mut s = self.style();
        f(&mut s);
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    /// Append a block to the innermost container that can hold one.
    fn push_block(&mut self, block: Block) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                Frame::Root(b) | Frame::Quote(b) | Frame::Item { blocks: b, .. } => {
                    b.push(block);
                    return;
                }
                Frame::Footnote { blocks, .. } => {
                    blocks.push(block);
                    return;
                }
                // A list holds items, not blocks; keep looking outward. A table
                // holds cells, and stray blocks inside one are dropped.
                Frame::List { .. } | Frame::Table { .. } => {}
            }
        }
    }

    fn push_text(&mut self, text: String, style: InlineStyle) {
        if text.is_empty() {
            return;
        }
        self.inlines.push(Inline::Text { text, style });
    }

    /// Close the open leaf block and emit it.
    fn finish_leaf(&mut self) {
        let inlines = normalize(std::mem::take(&mut self.inlines));
        match std::mem::replace(&mut self.leaf, Leaf::None) {
            Leaf::None => {}
            Leaf::Paragraph => self.push_block(Block::Paragraph(inlines)),
            Leaf::Heading(level) => self.push_block(Block::Heading { level, content: inlines }),
            Leaf::Cell => {
                if let Some(Frame::Table { row, head, in_head, .. }) = self.stack.last_mut() {
                    if *in_head {
                        head.push(inlines);
                    } else {
                        row.push(inlines);
                    }
                }
            }
        }
    }

    fn run(mut self, parser: Parser) -> Document {
        for event in parser {
            self.event(event);
        }
        // Unbalanced input can leave frames open; unwind rather than panic.
        while self.stack.len() > 1 {
            self.close_frame();
        }
        let blocks = match self.stack.pop() {
            Some(Frame::Root(b)) => b,
            _ => Vec::new(),
        };
        Document { blocks, links: self.links }
    }

    /// Pop the innermost frame and fold it into its parent.
    fn close_frame(&mut self) {
        let frame = match self.stack.pop() {
            Some(f) => f,
            None => return,
        };
        match frame {
            Frame::Root(blocks) => self.stack.push(Frame::Root(blocks)),
            Frame::Quote(blocks) => self.push_block(Block::BlockQuote(blocks)),
            Frame::List { kind, items, loose } => {
                self.push_block(Block::List { kind, tight: !loose, items })
            }
            Frame::Item { task, blocks, loose } => {
                if let Some(Frame::List { items, loose: list_loose, .. }) = self.stack.last_mut() {
                    // One loose item makes the whole list loose, per CommonMark.
                    *list_loose |= loose;
                    items.push(ListItem { task, blocks });
                }
            }
            Frame::Footnote { label, blocks } => {
                self.push_block(Block::FootnoteDef { label, blocks })
            }
            Frame::Table { align, head, rows, .. } => {
                self.push_block(Block::Table(Table { align, head, rows }))
            }
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),

            Event::Text(t) => {
                if let Some(alt) = self.alt.as_mut() {
                    alt.push_str(&t);
                } else if let Some((_, buf)) = self.code.as_mut() {
                    buf.push_str(&t);
                } else {
                    if self.leaf == Leaf::None {
                        // Text outside any leaf (loose list content, odd nesting).
                        self.leaf = Leaf::Paragraph;
                    }
                    let style = self.style();
                    self.push_text(t.into_string(), style);
                }
            }

            // CommonMark converts line endings inside a code span to spaces, so
            // an inline span never carries a SoftBreak of its own.
            Event::Code(t) => {
                let mut style = self.style();
                style.code = true;
                let text = t.replace('\n', " ");
                self.push_text(text, style);
            }

            Event::SoftBreak => self.inlines.push(Inline::SoftBreak),
            Event::HardBreak => self.inlines.push(Inline::HardBreak),

            Event::FootnoteReference(label) => {
                self.inlines.push(Inline::FootnoteRef(label.into_string()))
            }

            Event::Rule => {
                self.finish_leaf();
                self.push_block(Block::Rule);
            }

            Event::TaskListMarker(checked) => {
                // Arrives just inside the item it belongs to.
                if let Some(Frame::Item { task, .. }) = self.stack.last_mut() {
                    *task = Some(checked);
                }
            }

            Event::Html(h) => {
                if let Some(buf) = self.html.as_mut() {
                    buf.push_str(&h);
                }
            }
            Event::InlineHtml(h) => {
                // Inline HTML is shown literally rather than interpreted; a viewer
                // that silently drops `<br>` or `<sub>` hides content from you.
                let style = self.style();
                self.push_text(h.into_string(), style);
            }

            // Math extensions are not enabled; render any stray markers as text.
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                let style = self.style();
                self.push_text(t.into_string(), style);
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.finish_leaf();
                // A paragraph opening as an item's direct child means the author
                // left a blank line inside the list, i.e. the list is loose.
                if let Some(Frame::Item { loose, .. }) = self.stack.last_mut() {
                    *loose = true;
                }
                self.leaf = Leaf::Paragraph;
            }
            Tag::Heading { level, .. } => {
                self.finish_leaf();
                self.leaf = Leaf::Heading(heading_level(level));
            }
            Tag::BlockQuote(_) => {
                self.finish_leaf();
                self.stack.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.finish_leaf();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // The info string may carry more than a language
                        // (```rust,ignore); the first token is the language.
                        let token = info
                            .split(|c: char| c.is_whitespace() || c == ',')
                            .find(|s| !s.is_empty())
                            .unwrap_or("");
                        (!token.is_empty()).then(|| token.to_ascii_lowercase())
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.finish_leaf();
                let kind = match start {
                    Some(n) => ListKind::Ordered(n),
                    None => ListKind::Bullet,
                };
                self.stack.push(Frame::List { kind, items: Vec::new(), loose: false });
            }
            Tag::Item => {
                self.finish_leaf();
                self.stack.push(Frame::Item { task: None, blocks: Vec::new(), loose: false });
            }
            Tag::FootnoteDefinition(label) => {
                self.finish_leaf();
                self.stack.push(Frame::Footnote { label: label.into_string(), blocks: Vec::new() });
            }
            Tag::Table(aligns) => {
                self.finish_leaf();
                self.stack.push(Frame::Table {
                    align: aligns.into_iter().map(convert_align).collect(),
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = true;
                }
                self.leaf = Leaf::None;
            }
            Tag::TableRow => {
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.clear();
                }
            }
            Tag::TableCell => {
                self.inlines.clear();
                self.leaf = Leaf::Cell;
            }
            Tag::Emphasis => self.push_style(|s| s.italic = true),
            Tag::Strong => self.push_style(|s| s.bold = true),
            Tag::Strikethrough => self.push_style(|s| s.strike = true),
            Tag::Link { dest_url, title, .. } => {
                let index = self.links.len() as u32;
                self.links
                    .push(LinkTarget { url: dest_url.into_string(), title: title.into_string() });
                self.push_style(|s| s.link = Some(index));
            }
            Tag::Image { .. } => self.alt = Some(String::new()),
            Tag::HtmlBlock => {
                self.finish_leaf();
                self.html = Some(String::new());
            }
            // Extensions we do not enable.
            Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.finish_leaf(),
            TagEnd::BlockQuote(_) => {
                self.finish_leaf();
                self.close_frame();
            }
            TagEnd::CodeBlock => {
                if let Some((lang, code)) = self.code.take() {
                    // Fenced blocks always end with a newline; keeping it would
                    // render a spurious blank line inside every code block.
                    let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
                    self.push_block(Block::CodeBlock { lang, code });
                }
            }
            TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition | TagEnd::Table => {
                self.finish_leaf();
                self.close_frame();
            }
            TagEnd::TableHead => {
                self.finish_leaf();
                if let Some(Frame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = false;
                }
            }
            TagEnd::TableRow => {
                self.finish_leaf();
                if let Some(Frame::Table { row, rows, .. }) = self.stack.last_mut() {
                    let finished = std::mem::take(row);
                    if !finished.is_empty() {
                        rows.push(finished);
                    }
                }
            }
            TagEnd::TableCell => self.finish_leaf(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.pop_style()
            }
            TagEnd::Image => {
                if let Some(alt) = self.alt.take() {
                    self.inlines.push(Inline::Image { alt });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(html) = self.html.take() {
                    let trimmed = html.trim_end().to_string();
                    if !trimmed.is_empty() {
                        self.push_block(Block::Html(trimmed));
                    }
                }
            }
            TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn convert_align(a: CmAlign) -> Align {
    match a {
        CmAlign::None => Align::None,
        CmAlign::Left => Align::Left,
        CmAlign::Center => Align::Center,
        CmAlign::Right => Align::Right,
    }
}

/// True for characters that are *typographically* CJK.
///
/// Deliberately not "display width is 2": emoji are also double-width, but two
/// emoji separated by a source line break should still be separated by a space.
/// Only CJK scripts, which are written without inter-word spaces, want the space
/// dropped.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F   // Hangul Jamo
        | 0x2E80..=0x303E // CJK radicals, Kangxi, CJK symbols and punctuation
        | 0x3041..=0x33FF // Kana, Bopomofo, Hangul compat jamo, enclosed CJK
        | 0x3400..=0x4DBF // CJK unified ideographs extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xA000..=0xA4CF // Yi
        | 0xAC00..=0xD7A3 // Hangul syllables
        | 0xF900..=0xFAFF // CJK compatibility ideographs
        | 0xFE30..=0xFE4F // CJK compatibility forms
        | 0xFF00..=0xFF60 // Fullwidth forms
        | 0xFFE0..=0xFFE6
        | 0x20000..=0x2FA1F // CJK extensions B through F
    )
}

/// Resolve soft breaks, trim, and coalesce runs.
///
/// Split out from event handling because every rule here needs to see the
/// characters on *both* sides of the break, which a pull parser cannot offer
/// mid-stream. Being a plain function over a plain vector also makes each rule
/// directly unit-testable.
pub fn normalize(inlines: Inlines) -> Inlines {
    // Resolve each SoftBreak against its neighbours.
    let mut out: Inlines = Vec::with_capacity(inlines.len());
    for (i, item) in inlines.iter().enumerate() {
        match item {
            Inline::SoftBreak => {
                let prev = last_char(&out);
                let next = next_char(&inlines[i + 1..]);
                match (prev, next) {
                    // Start or end of the block: nothing to join.
                    (None, _) | (_, None) => {}
                    // A space is already present on one side.
                    (Some(p), _) if p.is_whitespace() => {}
                    (_, Some(n)) if n.is_whitespace() => {}
                    // CJK is written without inter-word spaces; inserting one at
                    // the author's wrap point would be visible and wrong.
                    (Some(p), Some(n)) if is_cjk(p) && is_cjk(n) => {}
                    _ => {
                        // The break lives between inline elements, not inside
                        // one, so the joining space must not inherit a code
                        // background or the gap renders as a highlighted block.
                        let mut style = trailing_style(&out).unwrap_or_default();
                        style.code = false;
                        out.push(Inline::Text { text: " ".into(), style });
                    }
                }
            }
            other => out.push(other.clone()),
        }
    }

    trim_edges(&mut out);
    coalesce(out)
}

fn last_char(inlines: &[Inline]) -> Option<char> {
    match inlines.last()? {
        Inline::Text { text, .. } => text.chars().next_back(),
        // A hard break means the run already ends in a line ending.
        Inline::HardBreak => Some('\n'),
        Inline::SoftBreak => Some(' '),
        Inline::FootnoteRef(_) | Inline::Image { .. } => Some('x'),
    }
}

fn next_char(rest: &[Inline]) -> Option<char> {
    match rest.first()? {
        Inline::Text { text, .. } => text.chars().next(),
        Inline::HardBreak => Some('\n'),
        Inline::SoftBreak => Some(' '),
        Inline::FootnoteRef(_) | Inline::Image { .. } => Some('x'),
    }
}

fn trailing_style(inlines: &[Inline]) -> Option<InlineStyle> {
    inlines.iter().rev().find_map(|i| match i {
        Inline::Text { style, .. } => Some(*style),
        _ => None,
    })
}

/// Drop leading and trailing whitespace from the block as a whole.
fn trim_edges(inlines: &mut Inlines) {
    while let Some(Inline::Text { text, .. }) = inlines.first_mut() {
        let trimmed = text.trim_start().to_string();
        if trimmed.is_empty() {
            inlines.remove(0);
        } else {
            *text = trimmed;
            break;
        }
    }
    while let Some(Inline::Text { text, .. }) = inlines.last_mut() {
        let trimmed = text.trim_end().to_string();
        if trimmed.is_empty() {
            inlines.pop();
        } else {
            *text = trimmed;
            break;
        }
    }
}

/// Merge adjacent text runs that share a style.
///
/// Purely a normalisation: it keeps span counts low and, more importantly, makes
/// the output canonical so snapshots do not churn on incidental run boundaries.
fn coalesce(inlines: Inlines) -> Inlines {
    let mut out: Inlines = Vec::with_capacity(inlines.len());
    for item in inlines {
        match (out.last_mut(), &item) {
            (
                Some(Inline::Text { text: prev, style: prev_style }),
                Inline::Text { text, style },
            ) if prev_style == style => prev.push_str(text),
            _ => out.push(item),
        }
    }
    out
}
