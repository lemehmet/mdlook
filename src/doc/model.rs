//! The semantic document model.
//!
//! This tree is deliberately *width-independent*: it says what the document means,
//! never how wide anything is. Everything width-dependent lives in `layout`. That
//! split is what lets a resize re-layout without re-parsing, and what makes the
//! layout stage a pure function we can snapshot-test.

/// Emphasis and link state carried by a run of text.
///
/// This is `Copy` and compared by value so the wrapper can group adjacent
/// characters that share a style back into a single span.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct InlineStyle {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    /// Index into [`Document::links`]. Kept as an index rather than an inline
    /// `String` so the style stays `Copy` and cheap to compare per character.
    pub link: Option<u32>,
}

/// A piece of inline content.
///
/// `SoftBreak` survives parsing on purpose. Resolving it needs to inspect the
/// characters on *both* sides (see [`crate::doc::parse::normalize`]), which is
/// awkward to do while pulling events but trivial once the run is assembled.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Inline {
    Text {
        text: String,
        style: InlineStyle,
    },
    /// A line ending preceded by two spaces or a backslash. Reflow must honor it.
    HardBreak,
    /// Any other line ending inside a block. Reflow must *not* honor it — this is
    /// the distinction `glow` collapses, leaving hard-wrapped source mid-paragraph.
    SoftBreak,
    FootnoteRef(String),
    Image {
        alt: String,
    },
}

pub type Inlines = Vec<Inline>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListKind {
    Bullet,
    /// Ordered list carrying its explicit start number.
    Ordered(u64),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ListItem {
    /// `Some(checked)` for a GFM task-list item, `None` for an ordinary one.
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    pub align: Vec<Align>,
    pub head: Vec<Inlines>,
    pub rows: Vec<Vec<Inlines>>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Block {
    Heading {
        level: u8,
        content: Inlines,
    },
    Paragraph(Inlines),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote(Vec<Block>),
    /// `tight` mirrors CommonMark's tight/loose distinction: a tight list has no
    /// blank lines between its items in the source and should not gain any in the
    /// output. Getting this wrong double-spaces every bullet list in a README.
    List {
        kind: ListKind,
        tight: bool,
        items: Vec<ListItem>,
    },
    Table(Table),
    Rule,
    /// Raw HTML block. Rendered verbatim and dimmed rather than interpreted.
    Html(String),
    FootnoteDef {
        label: String,
        blocks: Vec<Block>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct LinkTarget {
    pub url: String,
    pub title: String,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// Link targets, referenced by index from [`InlineStyle::link`].
    pub links: Vec<LinkTarget>,
}
