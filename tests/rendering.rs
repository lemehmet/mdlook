//! Rendering behaviour, including regressions for the specific defects found in
//! the existing tools during evaluation.

use mdlook::{layout, parse, Theme, ThemeKind};

fn render(source: &str, width: usize) -> Vec<String> {
    let document = parse(source);
    layout(&document, width, &Theme::new(ThemeKind::Mono))
        .plain
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

fn joined(source: &str, width: usize) -> String {
    render(source, width).join("\n")
}

// ---------------------------------------------------------------------------
// Regressions against md-tui 0.10.3, which renders `## Plain H2` literally and
// splits `## Bold **word** here` across two lines.
// ---------------------------------------------------------------------------

#[test]
fn heading_markers_are_stripped_at_every_level() {
    for level in 1..=6 {
        let source = format!("{} Heading {level}", "#".repeat(level));
        let out = joined(&source, 80);
        assert!(out.contains(&format!("Heading {level}")), "level {level} lost its text: {out:?}");
        assert!(!out.contains('#'), "level {level} kept its marker: {out:?}");
    }
}

#[test]
fn inline_styles_inside_a_heading_stay_on_one_line() {
    let out = render("## Bold **word** here", 80);
    assert_eq!(out[0], "Bold word here", "heading was split or mangled: {out:#?}");
}

#[test]
fn inline_code_in_a_heading_renders_without_backticks() {
    let out = render("## `fetch_user(id, *, timeout=30)`", 80);
    assert_eq!(out[0], "fetch_user(id, *, timeout=30)");
}

// ---------------------------------------------------------------------------
// General rendering
// ---------------------------------------------------------------------------

#[test]
fn emphasis_markers_are_removed() {
    let out = joined("**bold** _italic_ ~~struck~~ `code`", 80);
    assert_eq!(out, "bold italic struck code");
}

#[test]
fn link_text_is_shown_without_the_url() {
    let out = joined("see the [users guide](https://example.com/users) now", 80);
    assert_eq!(out, "see the users guide now");
}

#[test]
fn links_are_indexed_with_their_target() {
    let document =
        parse("see [users guide](https://example.com/users) and [rfc](https://x.test/r)");
    let rendered = layout(&document, 80, &Theme::default());
    let urls: Vec<&str> = rendered.links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(urls, vec!["https://example.com/users", "https://x.test/r"]);
    assert_eq!(rendered.links[0].text, "users guide");
}

#[test]
fn headings_are_indexed_for_the_outline() {
    let document = parse("# Title\n\ntext\n\n## Section\n\nmore\n\n### Deep\n");
    let rendered = layout(&document, 80, &Theme::default());
    let outline: Vec<(u8, &str)> =
        rendered.anchors.iter().map(|a| (a.level, a.text.as_str())).collect();
    assert_eq!(outline, vec![(1, "Title"), (2, "Section"), (3, "Deep")]);

    // Every anchor must point at a line that actually holds its text — this is
    // the invariant that makes jumping to an outline entry land correctly.
    for anchor in &rendered.anchors {
        assert!(
            rendered.plain[anchor.line].contains(&anchor.text),
            "anchor {anchor:?} does not match line {:?}",
            rendered.plain[anchor.line]
        );
    }
}

#[test]
fn tight_lists_do_not_gain_blank_lines() {
    let out = render("- one\n- two\n- three", 80);
    assert!(!out.iter().any(|l| l.is_empty()), "tight list was double-spaced: {out:#?}");
}

#[test]
fn loose_lists_keep_their_spacing() {
    let out = render("- one\n\n- two", 80);
    assert!(out.iter().any(|l| l.is_empty()), "loose list lost its spacing: {out:#?}");
}

#[test]
fn a_list_does_not_swallow_the_separator_of_the_block_after_it() {
    let out = render("- one\n- two\n\nAfter the list.", 80);
    let index = out.iter().position(|l| l.contains("After")).expect("paragraph");
    assert!(
        out[index - 1].is_empty(),
        "missing blank line between list and next paragraph: {out:#?}"
    );
}

#[test]
fn task_list_items_use_a_checkbox_instead_of_a_bullet() {
    let out = joined("- [ ] todo\n- [x] done", 80);
    assert!(out.contains("☐ todo"), "{out:?}");
    assert!(out.contains("☑ done"), "{out:?}");
    assert!(!out.contains('•'), "bullet duplicated the checkbox: {out:?}");
}

#[test]
fn nested_lists_are_indented_and_change_glyph() {
    let out = joined("- outer\n  - inner", 80);
    assert!(out.contains("• outer"), "{out:?}");
    assert!(out.contains("  ◦ inner"), "{out:?}");
}

#[test]
fn ordered_lists_keep_their_start_number() {
    let out = joined("5. five\n6. six", 80);
    assert!(out.contains("5. five"), "{out:?}");
    assert!(out.contains("6. six"), "{out:?}");
}

#[test]
fn table_cells_wrap_rather_than_truncate() {
    let source = "| Param | Description |\n|---|---|\n\
                  | timeout | how long to wait before giving up entirely |";
    let out = joined(source, 40);
    // Every word of the description must survive somewhere in the output.
    for word in ["how", "long", "wait", "giving", "entirely"] {
        assert!(out.contains(word), "table dropped {word:?}: {out}");
    }
}

#[test]
fn table_respects_the_available_width() {
    let source =
        "| A | B | C |\n|---|---|---|\n| a very long cell value here | another long one | third |";
    for width in [20, 30, 50, 80] {
        for line in render(source, width) {
            let w: usize =
                line.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            assert!(w <= width, "width {width}: {w}-column line {line:?}");
        }
    }
}

#[test]
fn code_block_contents_are_preserved_verbatim() {
    let out = joined("```rust\nfn main() {\n    let x = 1;\n}\n```", 80);
    assert!(out.contains("fn main() {"), "{out}");
    assert!(out.contains("    let x = 1;"), "indentation lost: {out}");
    assert!(!out.contains("```"), "fence leaked into output: {out}");
}

#[test]
fn unknown_code_language_still_renders_the_code() {
    let out = joined("```notalanguage\nsome code here\n```", 80);
    assert!(out.contains("some code here"), "{out}");
}

#[test]
fn code_blocks_are_not_reflowed() {
    // Code must keep its own line structure even though prose around it does not.
    let out = joined("```\nline one\nline two\n```", 80);
    assert!(out.contains("line one"), "{out}");
    assert!(out.contains("line two"), "{out}");
    assert!(!out.contains("line one line two"), "code was reflowed: {out}");
}

#[test]
fn empty_input_produces_no_lines() {
    assert!(render("", 80).is_empty());
    assert!(render("   \n\n  \n", 80).is_empty());
}

#[test]
fn very_narrow_width_does_not_panic() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/corpus/kitchen-sink.md"
    ))
    .expect("corpus fixture");
    for width in 1..=12 {
        let _ = render(&source, width);
    }
}

// ---------------------------------------------------------------------------
// Syntax highlighting coverage
// ---------------------------------------------------------------------------

#[test]
fn common_fence_languages_are_all_highlightable() {
    // syntect's bundled syntaxes are missing much of what modern API docs are
    // written in, and GitHub accepts aliases the syntax definitions do not
    // declare. Both gaps are closed deliberately, so both are pinned here.
    const TAGS: &[&str] = &[
        "rust",
        "python",
        "typescript",
        "tsx",
        "kotlin",
        "swift",
        "toml",
        "dockerfile",
        "zig",
        "nix",
        "elixir",
        "go",
        "bash",
        "json",
        "yaml",
        "csharp",
        "golang",
        "objc",
        "shell",
        "console",
        "jsonc",
        "psql",
        "proto",
        "graphql",
        "terraform",
        "c",
        "cpp",
        "java",
        "ruby",
        "php",
        "scala",
        "haskell",
        "lua",
        "perl",
        "sql",
        "html",
        "css",
        "xml",
        "diff",
        "makefile",
        "vue",
        "svelte",
        "ini",
        "http",
    ];
    let missing: Vec<&str> =
        TAGS.iter().copied().filter(|tag| !mdlook::layout::code::supports(tag)).collect();
    assert!(missing.is_empty(), "no syntax for: {missing:?}");
}

#[test]
fn an_explicit_unknown_language_is_not_guessed_at() {
    // Sniffing the content would colour this as whatever it superficially
    // resembles. The author said what it is; believe them and leave it plain.
    assert!(!mdlook::layout::code::supports("pseudocode"));
    let out = joined("```pseudocode\nfn main() { let x = 1; }\n```", 60);
    assert!(out.contains("fn main() { let x = 1; }"));
}

// ---------------------------------------------------------------------------
// Terminal escape neutralisation
//
// A markdown file is untrusted input. Passed through, a raw escape sequence is
// not text but a command: repaint the screen, rewrite the window title, and on
// some terminals worse. These pin the property that nothing from a document
// reaches the terminal as a control character.
// ---------------------------------------------------------------------------

/// Every control byte in `bytes`, ignoring the newlines that separate lines.
fn control_bytes(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().copied().filter(|b| *b < 0x20 && *b != b'\n').collect()
}

const HOSTILE: &str = concat!(
    "Body \x1b[31mred\x1b[0m and \x1b]0;new title\x07 here.\n\n",
    "```rust\nlet x = 1; \x1b[5mblink\x1b[0m\n```\n\n",
    "- list \x1b[7mitem\x1b[0m\n\n",
    "> quote \x1b[1mbold\x1b[0m\n\n",
    "| a | b |\n|---|---|\n| \x1b[32mcell\x1b[0m | two |\n\n",
    "# Heading \x1b[35mescape\x1b[0m\n\n",
    "[link \x1b[4mtext\x1b[0m](https://example.com/\x1b]0;url\x07path)\n",
);

#[test]
fn escapes_never_reach_the_rendered_text() {
    let rendered = layout(&parse(HOSTILE), 80, &Theme::default());
    for line in &rendered.plain {
        assert!(
            control_bytes(line.as_bytes()).is_empty(),
            "control characters survived into: {line:?}"
        );
    }
}

#[test]
fn escapes_are_shown_as_visible_control_pictures() {
    // Neutralised, not silently dropped: a document that contains an escape
    // should visibly say so rather than quietly rendering as if it did not.
    let out = joined(HOSTILE, 80);
    assert!(out.contains('\u{241B}'), "ESC was dropped rather than shown: {out:?}");
}

#[test]
fn escapes_in_link_urls_are_neutralised() {
    // The link list renders URLs directly rather than through the layout sink,
    // and a URL is a good hiding place because nobody reads one closely.
    let rendered = layout(&parse(HOSTILE), 80, &Theme::default());
    for link in &rendered.links {
        assert!(control_bytes(link.url.as_bytes()).is_empty(), "url: {:?}", link.url);
        assert!(control_bytes(link.text.as_bytes()).is_empty(), "text: {:?}", link.text);
    }
}

#[test]
fn the_ansi_writer_emits_no_escape_it_did_not_choose() {
    // With colour off, the only escapes possible would be ones that came from
    // the document, so the output must contain none at all.
    let rendered = layout(&parse(HOSTILE), 80, &Theme::new(ThemeKind::Mono));
    let plain = mdlook::render::to_ansi(&rendered, false);
    assert!(control_bytes(plain.as_bytes()).is_empty(), "escape leaked through the plain writer");

    // With colour on, ours are CSI-SGR only: no OSC (title, clipboard), no BEL.
    let coloured = mdlook::render::to_ansi(&rendered, true);
    assert!(!coloured.contains("\x1b]"), "an OSC sequence reached the output");
    assert!(!coloured.contains('\x07'), "a BEL reached the output");
}
