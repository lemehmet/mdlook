//! Files that are not markdown, end to end.
//!
//! Phase one of the file browser: the viewer no longer assumes what it is
//! looking at. These tests hold the line on the properties that made that
//! generalisation safe — the searchable mirror still lines up, the gutter is not
//! searchable, and the output is still reproducible byte for byte.

use std::process::Command;

use mdlook::layout::wrap::text_width;
use mdlook::ui::app::App;
use mdlook::ui::popup::PopupKind;
use mdlook::{Content, Theme};

const RUST: &str = "\
fn main() {
    let answer = 42;
    println!(\"answer is {answer}\");
}
";

fn app(name: &str, body: &str) -> App {
    let content = Content::from_bytes(name, body.as_bytes(), body.len() as u64);
    let mut app = App::new(content, name.into(), Theme::default(), 80);
    app.viewport = 10;
    app
}

fn scratch(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mdlook-test-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).expect("writing fixture");
    path
}

// -- the searchable mirror -------------------------------------------------

#[test]
fn the_lines_and_plain_mirror_never_drift_for_a_source_file() {
    // Everything downstream assumes `plain[i]` is the text of `lines[i]`. The
    // whole-file view builds its rows a different way from the markdown path,
    // so it has to be held to the same invariant.
    let content = Content::from_bytes("m.rs", RUST.as_bytes(), RUST.len() as u64);
    for width in [12, 20, 40, 80, 200] {
        let rendered = content.layout(width, &Theme::default());
        assert_eq!(rendered.lines.len(), rendered.plain.len(), "lengths differ at width {width}");
        for (index, line) in rendered.lines.iter().enumerate() {
            let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert_eq!(joined, rendered.plain[index], "row {index} differs at width {width}");
        }
    }
}

#[test]
fn search_ranges_still_slice_the_rendered_text() {
    let app = app("m.rs", RUST);
    for query in ["answer", "println", "42", "fn"] {
        for hit in mdlook::ui::search::find(&app.rendered, query) {
            // Panics rather than fails if the offset is not a char boundary,
            // which is exactly the failure worth catching.
            let text = &app.rendered.plain[hit.line][hit.start..hit.end];
            assert_eq!(text.to_lowercase(), query.to_lowercase());
        }
    }
}

#[test]
fn the_line_number_gutter_is_not_searchable() {
    // A file long enough that the numbers themselves contain the digits being
    // searched for: line 42 exists, and so does the literal `42` in the code.
    let body = (1..=60).map(|n| format!("value_{n} = 0")).collect::<Vec<_>>().join("\n");
    let app = app("m.rs", &body);

    let hits = mdlook::ui::search::find(&app.rendered, "42");
    assert_eq!(hits.len(), 1, "expected only the occurrence in the code, got {hits:?}");
    assert_eq!(hits[0].line, 41, "line 42 is index 41");
    assert_eq!(&app.rendered.plain[41][hits[0].start..hits[0].end], "42");
    // Proving the point: the row does start with the number we did not match.
    assert!(app.rendered.plain[41].starts_with("42 value_42"));
}

#[test]
fn searching_a_source_file_navigates_like_a_document() {
    let mut app = app("m.rs", RUST);
    app.open_search();
    for c in "answer".chars() {
        app.search_push(c);
    }
    // The binding, the word in the message, and the interpolation.
    assert_eq!(app.search.matches.len(), 3);
    assert_eq!(app.search.matches[0].line, 1, "the binding comes first");
    app.popup_commit();
    app.step_match(true);
    assert!(app.search.current_match().is_some());
}

// -- what a source file does not have --------------------------------------

#[test]
fn the_outline_and_link_lists_are_empty_rather_than_wrong() {
    // A `#` in a shell script is a comment, and a URL in a comment is not a
    // markdown link. Both indexes should come up empty instead of inventing
    // entries out of syntax that means something else here.
    let mut app = app("deploy.sh", "# not a heading\n# see https://example.com\n");
    for kind in [PopupKind::Outline, PopupKind::Links] {
        app.open_popup(kind);
        assert!(app.popup.as_ref().unwrap().is_empty(), "{kind:?} should be empty");
        app.popup_cancel();
    }
}

// -- resize ----------------------------------------------------------------

#[test]
fn resizing_a_source_file_keeps_the_scroll_in_range() {
    // The markdown path re-anchors on headings; a source file has none, so this
    // exercises the fallback and the clamp that follows it.
    let body = (1..=200).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    let mut app = app("m.rs", &body);
    app.scroll = 150;
    for width in [30, 200, 12, 80] {
        app.relayout(width);
        assert!(app.scroll <= app.max_scroll(), "scroll ran past the end at width {width}");
        for line in &app.rendered.plain {
            assert!(text_width(line) <= width.max(8), "width {width} overflowed");
        }
    }
}

// -- reading from disk -----------------------------------------------------

#[test]
fn a_binary_is_identified_without_reading_all_of_it() {
    // 4 MiB of NULs behind an ELF header. If this were slurped into a string the
    // test would still pass, but the point of the head-first read is that it is
    // not, and the reported size must still be the whole file.
    let mut bytes = vec![0u8; 4 * 1024 * 1024];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[16] = 2;
    bytes[18] = 0x3e;
    let path = scratch("big.bin", &bytes);

    let content = Content::read(&path, None).expect("reading fixture");
    let text = content.layout(80, &Theme::default()).plain.join("\n");
    assert!(text.contains("ELF 64-bit LSB executable, x86-64"), "got: {text}");
    assert!(text.contains("4.0 MiB"), "size should be the whole file, got: {text}");

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_markdown_file_read_from_disk_is_still_markdown() {
    let path = scratch("doc.md", b"# Title\n\nSoft\nwrapped.\n");
    let content = Content::read(&path, None).expect("reading fixture");
    let rendered = content.layout(80, &Theme::default());
    assert_eq!(rendered.anchors.len(), 1);
    assert!(
        rendered.plain.iter().any(|l| l.contains("Soft wrapped.")),
        "reflow should still apply: {:?}",
        rendered.plain
    );
    std::fs::remove_file(&path).ok();
}

// -- reproducibility -------------------------------------------------------

#[test]
fn a_source_file_renders_byte_identically_across_processes() {
    // syntect resolves syntaxes through maps, and Rust seeds hashers per
    // process, so highlighting a file by *name* is a fresh chance to leak map
    // order into the output. One loop in one process would never see it.
    let path = scratch("determinism.rs", RUST.as_bytes());
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_mdlook"))
            .args(["--width", "80", "--theme", "dark"])
            .arg(&path)
            .output()
            .expect("running mdlook");
        assert!(out.status.success(), "mdlook exited with {:?}", out.status);
        out.stdout
    };

    let first = run();
    let second = run();
    assert!(!first.is_empty(), "no output produced");
    assert_eq!(first, second, "highlighting a file by name is not reproducible");

    std::fs::remove_file(&path).ok();
}

#[test]
fn no_control_character_survives_into_a_file_view() {
    // The same guarantee markdown gets: a hostile file cannot repaint the
    // terminal by being opened. Source files reach the screen through a
    // different producer, so the property is asserted again here.
    let hostile = "\u{1b}]0;pwned\u{7}\u{1b}[31mred\u{1b}[0m\nplain\n";
    let content = Content::from_bytes("x.txt", hostile.as_bytes(), hostile.len() as u64);
    let rendered = content.layout(80, &Theme::default());
    for line in &rendered.plain {
        assert!(
            !line.chars().any(|c| c.is_control()),
            "control character reached the rendered text: {line:?}"
        );
    }
    assert!(rendered.plain.join("").contains('␛'), "the escape should be shown, not dropped");
}

// -- the optional external identifier --------------------------------------

#[test]
fn a_configured_probe_command_replaces_the_built_in_description() {
    // `echo` stands in for `file(1)` so the test does not depend on it being
    // installed: what matters is that the configured command's output is what
    // lands in the pane.
    let path = scratch("probe.bin", b"\x89PNG\r\n\x1a\n\x00\x00");
    let content = Content::read(&path, Some("echo identified-by-command")).expect("reading");
    let text = content.layout(80, &Theme::default()).plain.join("\n");
    assert!(text.contains("identified-by-command"), "got: {text}");
    assert!(!text.contains("PNG image"), "the command should have replaced it");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_probe_command_that_does_not_exist_falls_back_quietly() {
    // A missing `file(1)` should leave the built-in answer in place, not put an
    // error message where the description belongs.
    let path = scratch("fallback.bin", b"\x89PNG\r\n\x1a\n\x00\x00");
    let content = Content::read(&path, Some("mdlook-no-such-command-exists")).expect("reading");
    let text = content.layout(80, &Theme::default()).plain.join("\n");
    assert!(text.contains("PNG image"), "got: {text}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn probe_output_is_neutralised_like_everything_else_that_reaches_the_screen() {
    // The command echoes the file's name, and file names are attacker-supplied.
    let path = scratch("escape.bin", b"\x89PNG\r\n\x1a\n\x00\x00");
    let content = Content::read(&path, Some("printf hostile\\x1b[31mred\\n")).expect("reading");
    let text = content.layout(80, &Theme::default()).plain.join("\n");
    assert!(!text.contains('\u{1b}'), "an escape reached the rendered text");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_probe_command_is_not_run_through_a_shell() {
    // If this were handed to a shell the redirection would create a file and the
    // description would come back empty. Run directly, it is just arguments.
    let path = scratch("noshell.bin", b"\x89PNG\r\n\x1a\n\x00\x00");
    let marker = std::env::temp_dir().join(format!("mdlook-shell-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let command = format!("echo pwned > {}", marker.display());

    let content = Content::read(&path, Some(&command)).expect("reading");
    let text = content.layout(80, &Theme::default()).plain.join("\n");
    assert!(!marker.exists(), "the redirection was interpreted — a shell was involved");
    assert!(text.contains("pwned >"), "the arguments should be literal, got: {text}");

    std::fs::remove_file(&path).ok();
}
