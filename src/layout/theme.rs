//! The style palette.
//!
//! A `Theme` is an explicit *input* to layout, never something sniffed from the
//! environment. Querying the terminal for its background colour would make the
//! rendered output depend on which terminal you happened to run in, and the whole
//! point of this renderer is that the same document renders identically every
//! time.
//!
//! Body text leans on the terminal's own 16-colour palette rather than fixed RGB,
//! so it inherits whatever colour scheme you already use. Only syntax highlighting
//! uses true colour, because syntect's themes are defined that way.

use ratatui::style::{Color, Modifier, Style};

use crate::doc::InlineStyle;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
    /// No colour at all, only bold/italic/underline. For `NO_COLOR` and pipes.
    Mono,
}

impl ThemeKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "mono" | "none" | "plain" => Some(Self::Mono),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub kind: ThemeKind,
    /// Heading styles, index 0 is H1.
    pub headings: [Style; 6],
    pub text: Style,
    pub emphasis_color: Option<Color>,
    pub code_inline: Style,
    pub code_block_bg: Option<Color>,
    pub code_block_fg: Style,
    pub code_fence_label: Style,
    /// The line-number gutter in the whole-file view.
    pub line_number: Style,
    /// Directory rows in the file browser.
    pub tree_dir: Style,
    /// Rows for the file types a reader scans a directory looking for. The
    /// classes themselves live in [`crate::files::detect::Class`].
    pub tree_image: Style,
    pub tree_pdf: Style,
    pub tree_markdown: Style,
    pub tree_source: Style,
    /// Dotfiles, when they are shown and nothing better describes them.
    pub tree_hidden: Style,
    /// The browser's selection, when the browser has the keyboard.
    pub tree_selection: Style,
    /// The browser's selection when focus is in the document, so the reader can
    /// still see where they were without it competing for attention.
    pub tree_selection_idle: Style,
    /// The rule between the browser and the document.
    pub tree_divider: Style,
    pub link: Style,
    pub quote_bar: Style,
    pub quote_text: Style,
    pub rule: Style,
    pub marker: Style,
    pub table_border: Style,
    pub table_header: Style,
    pub html: Style,
    pub footnote: Style,
    pub image: Style,
    /// Name of the syntect theme used for fenced code.
    pub syntax_theme: &'static str,
    pub search_match: Style,
    pub search_current: Style,
    pub status: Style,
    pub popup_border: Style,
    pub popup_title: Style,
    pub popup_selected: Style,
    pub popup_dim: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(ThemeKind::Dark)
    }
}

impl Theme {
    pub fn new(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Self::dark(),
            ThemeKind::Light => Self::light(),
            ThemeKind::Mono => Self::mono(),
        }
    }

    fn dark() -> Self {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        Self {
            kind: ThemeKind::Dark,
            headings: [
                bold.fg(Color::Magenta),
                bold.fg(Color::Cyan),
                bold.fg(Color::Blue),
                bold.fg(Color::Green),
                bold.fg(Color::Yellow),
                bold.fg(Color::Gray),
            ],
            text: Style::new(),
            emphasis_color: None,
            code_inline: Style::new().fg(Color::LightRed),
            code_block_bg: Some(Color::Rgb(0x1c, 0x1f, 0x26)),
            code_block_fg: Style::new().fg(Color::Gray),
            code_fence_label: Style::new().fg(Color::DarkGray),
            line_number: Style::new().fg(Color::DarkGray),
            tree_dir: bold.fg(Color::Blue),
            tree_image: Style::new().fg(Color::Magenta),
            tree_pdf: Style::new().fg(Color::Red),
            tree_markdown: Style::new().fg(Color::Cyan),
            tree_source: Style::new().fg(Color::Yellow),
            tree_hidden: Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            tree_selection: Style::new().bg(Color::Cyan).fg(Color::Black),
            tree_selection_idle: Style::new().bg(Color::Rgb(0x30, 0x34, 0x3d)),
            tree_divider: Style::new().fg(Color::DarkGray),
            link: Style::new().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
            quote_bar: Style::new().fg(Color::DarkGray),
            quote_text: Style::new().fg(Color::Gray).add_modifier(Modifier::ITALIC),
            rule: Style::new().fg(Color::DarkGray),
            marker: Style::new().fg(Color::Yellow),
            table_border: Style::new().fg(Color::DarkGray),
            table_header: bold,
            html: Style::new().fg(Color::DarkGray),
            footnote: Style::new().fg(Color::Magenta),
            image: Style::new().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
            syntax_theme: "base16-ocean.dark",
            search_match: Style::new().bg(Color::Yellow).fg(Color::Black),
            search_current: Style::new()
                .bg(Color::LightYellow)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            status: Style::new().fg(Color::Black).bg(Color::Cyan),
            popup_border: Style::new().fg(Color::Cyan),
            popup_title: bold.fg(Color::Cyan),
            popup_selected: Style::new().bg(Color::Cyan).fg(Color::Black),
            popup_dim: Style::new().fg(Color::DarkGray),
        }
    }

    fn light() -> Self {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        Self {
            kind: ThemeKind::Light,
            headings: [
                bold.fg(Color::Magenta),
                bold.fg(Color::Blue),
                bold.fg(Color::Cyan),
                bold.fg(Color::Green),
                bold.fg(Color::Red),
                bold.fg(Color::DarkGray),
            ],
            code_inline: Style::new().fg(Color::Red),
            code_block_bg: Some(Color::Rgb(0xf2, 0xf2, 0xf2)),
            code_block_fg: Style::new().fg(Color::Black),
            tree_dir: bold.fg(Color::Blue),
            tree_image: Style::new().fg(Color::Magenta),
            tree_pdf: Style::new().fg(Color::Red),
            tree_markdown: bold,
            tree_source: Style::new().fg(Color::Green),
            tree_hidden: Style::new().fg(Color::DarkGray).add_modifier(Modifier::DIM),
            tree_selection: Style::new().bg(Color::Blue).fg(Color::White),
            tree_selection_idle: Style::new().bg(Color::Rgb(0xe2, 0xe4, 0xe8)),
            quote_text: Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            syntax_theme: "InspiredGitHub",
            search_match: Style::new().bg(Color::Yellow).fg(Color::Black),
            status: Style::new().fg(Color::White).bg(Color::Blue),
            ..Self::dark()
        }
    }

    /// Structure conveyed entirely through bold/italic/underline.
    fn mono() -> Self {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let plain = Style::new();
        Self {
            kind: ThemeKind::Mono,
            headings: [
                bold.add_modifier(Modifier::UNDERLINED),
                bold,
                bold,
                bold.add_modifier(Modifier::ITALIC),
                plain.add_modifier(Modifier::ITALIC),
                plain.add_modifier(Modifier::ITALIC),
            ],
            text: plain,
            emphasis_color: None,
            code_inline: plain,
            code_block_bg: None,
            code_block_fg: plain,
            code_fence_label: plain.add_modifier(Modifier::DIM),
            line_number: plain.add_modifier(Modifier::DIM),
            tree_dir: bold,
            tree_image: plain.add_modifier(Modifier::ITALIC),
            tree_pdf: bold.add_modifier(Modifier::UNDERLINED),
            tree_markdown: bold.add_modifier(Modifier::ITALIC),
            tree_source: plain.add_modifier(Modifier::UNDERLINED),
            tree_hidden: plain.add_modifier(Modifier::DIM),
            tree_selection: plain.add_modifier(Modifier::REVERSED),
            tree_selection_idle: plain.add_modifier(Modifier::UNDERLINED),
            tree_divider: plain.add_modifier(Modifier::DIM),
            link: plain.add_modifier(Modifier::UNDERLINED),
            quote_bar: plain,
            quote_text: plain.add_modifier(Modifier::ITALIC),
            rule: plain,
            marker: plain,
            table_border: plain,
            table_header: bold,
            html: plain.add_modifier(Modifier::DIM),
            footnote: plain,
            image: plain.add_modifier(Modifier::ITALIC),
            syntax_theme: "",
            search_match: plain.add_modifier(Modifier::REVERSED),
            search_current: plain.add_modifier(Modifier::REVERSED).add_modifier(Modifier::BOLD),
            status: plain.add_modifier(Modifier::REVERSED),
            popup_border: plain,
            popup_title: bold,
            popup_selected: plain.add_modifier(Modifier::REVERSED),
            popup_dim: plain.add_modifier(Modifier::DIM),
        }
    }

    pub fn heading(&self, level: u8) -> Style {
        let index = (level.clamp(1, 6) - 1) as usize;
        self.headings[index]
    }

    /// Resolve parsed emphasis into a concrete terminal style.
    ///
    /// Code wins over the base text colour because an inline code span reads as a
    /// distinct kind of thing; emphasis modifiers still stack on top of it.
    pub fn inline(&self, style: InlineStyle) -> Style {
        let mut out = if style.code { self.code_inline } else { self.text };
        if style.link.is_some() {
            out = out.patch(self.link);
        }
        if style.bold {
            out = out.add_modifier(Modifier::BOLD);
        }
        if style.italic {
            out = out.add_modifier(Modifier::ITALIC);
        }
        if style.strike {
            out = out.add_modifier(Modifier::CROSSED_OUT);
        }
        out
    }
}
