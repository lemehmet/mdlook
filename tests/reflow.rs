//! Tests for the property this tool exists to provide: a paragraph reflows to the
//! reader's width regardless of where the author wrapped the source.

use mdlook::{layout, parse, Theme, ThemeKind};

/// Render to plain text at a given width.
fn render(source: &str, width: usize) -> Vec<String> {
    let document = parse(source);
    let rendered = layout(&document, width, &Theme::new(ThemeKind::Mono));
    rendered.plain.iter().map(|l| l.trim_end().to_string()).collect()
}

fn joined(source: &str, width: usize) -> String {
    render(source, width).join("\n")
}

#[test]
fn hard_wrapped_source_is_rejoined_and_rewrapped() {
    // The exact failure mode of `glow`: this source keeps its 3 source breaks
    // there instead of filling the available width.
    let source = "This paragraph is hard-wrapped at eighty characters in the source,\n\
                  which is a very common authoring style. A correct renderer joins\n\
                  these lines back into one paragraph.";

    let wide = render(source, 100);
    assert_eq!(
        wide.len(),
        2,
        "at width 100 this should fill 2 lines, not echo the source's 3: {wide:#?}"
    );

    let narrow = render(source, 30);
    assert!(narrow.len() > 5, "at width 30 it should wrap into many short lines: {narrow:#?}");

    // Same words, both times — only the line breaks moved.
    let words = |lines: &[String]| lines.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(words(&wide), words(&narrow));
}

#[test]
fn width_is_never_exceeded() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/corpus/kitchen-sink.md"
    ))
    .expect("corpus fixture");

    for width in [30, 40, 60, 80, 100, 120] {
        for line in render(&source, width) {
            let w: usize =
                line.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            assert!(w <= width, "width {width}: line of {w} columns: {line:?}");
        }
    }
}

#[test]
fn two_trailing_spaces_still_break_the_line() {
    // A hard break is the author explicitly asking for a line ending; unlike a
    // soft break it must survive reflow.
    let out = render("alpha  \nbeta", 80);
    assert_eq!(out, vec!["alpha", "beta"]);
}

#[test]
fn backslash_also_makes_a_hard_break() {
    let out = render("alpha\\\nbeta", 80);
    assert_eq!(out, vec!["alpha", "beta"]);
}

#[test]
fn soft_break_becomes_exactly_one_space() {
    assert_eq!(render("alpha\nbeta", 80), vec!["alpha beta"]);
    // Trailing whitespace before the break must not produce a double space.
    assert_eq!(render("alpha \nbeta", 80), vec!["alpha beta"]);
    assert_eq!(render("alpha\n beta", 80), vec!["alpha beta"]);
}

#[test]
fn cjk_lines_join_without_an_inserted_space() {
    // CJK is written without inter-word spaces, so a space at the author's wrap
    // point would be a visible artefact rather than a join.
    assert_eq!(render("日本語の文章\nこれは続きです", 80), vec!["日本語の文章これは続きです"]);
}

#[test]
fn latin_adjacent_to_cjk_keeps_its_space() {
    // Only a break with CJK on *both* sides drops the space.
    let out = joined("rust\n日本語", 80);
    assert_eq!(out, "rust 日本語");
}

#[test]
fn emoji_are_not_treated_as_cjk() {
    // Emoji are double-width but not a spaceless script; dropping the space here
    // would run two sentences together.
    assert_eq!(render("done 🎉\n🎊 party", 80), vec!["done 🎉 🎊 party"]);
}

#[test]
fn line_endings_inside_an_inline_code_span_become_spaces() {
    let out = joined("call `foo\nbar` now", 80);
    assert_eq!(out, "call foo bar now");
}

#[test]
fn reflow_applies_inside_blockquotes_and_list_items() {
    let quote = joined("> one\n> two", 80);
    assert!(quote.ends_with("one two"), "blockquote did not reflow: {quote:?}");

    let item = joined("- one\n  two", 80);
    assert!(item.ends_with("one two"), "list item did not reflow: {item:?}");
}

#[test]
fn crlf_input_reflows_the_same_as_lf() {
    assert_eq!(joined("alpha\r\nbeta", 80), joined("alpha\nbeta", 80));
}
