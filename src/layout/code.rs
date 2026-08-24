//! Syntax highlighting, for fenced code blocks and for whole files.
//!
//! syntect's syntax and theme sets are embedded in the binary and loaded once.
//! Nothing is read from disk or the environment, so a given (code, language,
//! theme) triple always highlights the same way — on any machine, in any
//! terminal.

use std::path::Path;
use std::sync::LazyLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SynStyle, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use super::theme::{Theme, ThemeKind};
use super::wrap::expand_tabs;

/// bat's syntax set rather than syntect's bundled one, which has no TypeScript,
/// Kotlin, Swift, TOML, Dockerfile, Zig, Nix or C#.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Fence tags people actually write that no syntax claims under that name.
///
/// GitHub's highlighter accepts a wider set of aliases than Sublime's syntax
/// definitions declare, so docs are full of ```csharp and ```shell that would
/// otherwise render unhighlighted. Mapping them is cheaper and more predictable
/// than fuzzy-matching the tag.
const ALIASES: &[(&str, &str)] = &[
    ("csharp", "cs"),
    ("golang", "go"),
    ("objc", "objective-c"),
    ("shell", "bash"),
    ("console", "bash"),
    ("shell-session", "bash"),
    ("jsonc", "json"),
    ("json5", "json"),
    ("psql", "sql"),
    ("postgres", "sql"),
    ("postgresql", "sql"),
    ("htm", "html"),
    ("markdown", "md"),
];

fn resolve(lang: &str) -> Option<&'static syntect::parsing::SyntaxReference> {
    SYNTAXES.find_syntax_by_token(lang).or_else(|| {
        ALIASES
            .iter()
            .find(|(from, _)| *from == lang)
            .and_then(|(_, to)| SYNTAXES.find_syntax_by_token(to))
    })
}

/// Whether a fenced-code language tag resolves to a known syntax.
pub fn supports(lang: &str) -> bool {
    resolve(lang).is_some()
}

/// Resolve a syntax for a whole file, from its name and its opening line.
///
/// The ladder deliberately differs from the fenced-code path. A fence tag is the
/// author telling us what the block is, so an explicit-but-unknown tag is left
/// alone rather than second-guessed. A file *name* is much weaker evidence — it
/// may be `notes.txt`, or carry no extension at all — so when the name resolves
/// to nothing we fall through to the shebang, which is usually right and is the
/// only thing that identifies an extensionless script.
fn resolve_file(name: &str, body: &str) -> Option<&'static SyntaxReference> {
    let path = Path::new(name);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    // The whole file name goes first: a syntax's declared extension list is also
    // where bare names live, which is what matches `Makefile` and `Dockerfile`.
    SYNTAXES
        .find_syntax_by_extension(file_name)
        .or_else(|| SYNTAXES.find_syntax_by_extension(extension))
        .or_else(|| (!extension.is_empty()).then(|| resolve(extension)).flatten())
        .or_else(|| SYNTAXES.find_syntax_by_first_line(body))
}

/// Highlight `code`, returning one span vector per line.
///
/// Falls back to unhighlighted-but-styled lines when the language is unknown or
/// unsupported. That is deliberately not the same as raw text: the block still
/// reads as code, it just is not coloured by token.
pub fn highlight(code: &str, lang: Option<&str>, theme: &Theme) -> Vec<Vec<Span<'static>>> {
    let code = expand_tabs(code);

    if theme.kind == ThemeKind::Mono || theme.syntax_theme.is_empty() {
        return plain(&code, theme);
    }

    // Fall back to sniffing the first line only when the fence carried no tag at
    // all. An explicit-but-unknown tag ("```pseudocode") means the author told us
    // what this is, and guessing over them produces confidently wrong colours.
    let syntax = match lang {
        Some(tag) => resolve(tag),
        None => SYNTAXES.find_syntax_by_first_line(&code),
    };
    let Some(syntax) = syntax else {
        return plain(&code, theme);
    };

    let Some(syntax_theme) = THEMES.themes.get(theme.syntax_theme) else {
        return plain(&code, theme);
    };

    run(syntax, syntax_theme, &code, theme)
}

/// Above this, a file is shown unhighlighted.
///
/// syntect is linear in the input with a regex engine behind it, and the browser
/// re-highlights whenever the selection moves. A quarter-megabyte is past
/// anything anyone reads top to bottom and comfortably inside a keypress budget.
const MAX_HIGHLIGHT: usize = 256 * 1024;

/// Highlight a whole file, identified by its name rather than a fence tag.
pub fn highlight_file(name: &str, body: &str, theme: &Theme) -> Vec<Vec<Span<'static>>> {
    let body = expand_tabs(body);

    if theme.kind == ThemeKind::Mono || theme.syntax_theme.is_empty() || body.len() > MAX_HIGHLIGHT
    {
        return plain(&body, theme);
    }

    let Some(syntax) = resolve_file(name, &body) else {
        return plain(&body, theme);
    };
    let Some(syntax_theme) = THEMES.themes.get(theme.syntax_theme) else {
        return plain(&body, theme);
    };

    run(syntax, syntax_theme, &body, theme)
}

/// The highlighting loop itself, shared by the fence and whole-file paths.
fn run(
    syntax: &SyntaxReference,
    syntax_theme: &syntect::highlighting::Theme,
    code: &str,
    theme: &Theme,
) -> Vec<Vec<Span<'static>>> {
    let mut highlighter = HighlightLines::new(syntax, syntax_theme);
    let mut out = Vec::new();

    for line in LinesWithEndings::from(code) {
        match highlighter.highlight_line(line, &SYNTAXES) {
            Ok(ranges) => out.push(
                ranges
                    .into_iter()
                    .filter_map(|(style, text)| {
                        let text = text.trim_end_matches(['\n', '\r']);
                        (!text.is_empty()).then(|| Span::styled(text.to_string(), convert(style)))
                    })
                    .collect(),
            ),
            // A syntax that fails mid-file should not lose the rest of the block.
            Err(_) => out.push(vec![Span::styled(
                line.trim_end_matches(['\n', '\r']).to_string(),
                theme.code_block_fg,
            )]),
        }
    }
    out
}

fn plain(code: &str, theme: &Theme) -> Vec<Vec<Span<'static>>> {
    code.lines().map(|line| vec![Span::styled(line.to_string(), theme.code_block_fg)]).collect()
}

/// syntect style → ratatui style.
///
/// The background is dropped on purpose: the code block paints its own, and
/// letting per-token backgrounds through produces a patchwork.
fn convert(style: SynStyle) -> Style {
    let fg = style.foreground;
    let mut out = Style::new().fg(Color::Rgb(fg.r, fg.g, fg.b));
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}
