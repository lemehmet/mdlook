//! The same document must render identically every time.
//!
//! The subtle threat here is `HashMap` iteration. Rust seeds `RandomState` per
//! *process*, so map-order leaks are invisible to a loop inside one test and only
//! show up as flaky output between runs. That is why the important test in this
//! file shells out to the real binary twice rather than calling `layout` twice.

use std::process::Command;

use mdlook::{layout, parse, Theme, ThemeKind};

const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/kitchen-sink.md");

fn source() -> String {
    std::fs::read_to_string(CORPUS).expect("corpus fixture")
}

#[test]
fn layout_is_idempotent_within_a_process() {
    let document = parse(&source());
    for width in [40, 80, 120] {
        let theme = Theme::new(ThemeKind::Dark);
        let first = layout(&document, width, &theme);
        let second = layout(&document, width, &theme);
        assert_eq!(first.plain, second.plain, "text differs at width {width}");
        assert_eq!(
            format!("{:?}", first.lines),
            format!("{:?}", second.lines),
            "styling differs at width {width}"
        );
        assert_eq!(first.anchors, second.anchors);
        assert_eq!(first.links, second.links);
    }
}

#[test]
fn parsing_is_idempotent() {
    assert_eq!(parse(&source()), parse(&source()));
}

#[test]
fn separate_processes_produce_byte_identical_output() {
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_mdlook"))
            .args(["--width", "80", "--theme", "dark", CORPUS])
            .output()
            .expect("running mdlook");
        assert!(out.status.success(), "mdlook exited with {:?}", out.status);
        out.stdout
    };

    let first = run();
    let second = run();
    assert!(!first.is_empty(), "no output produced");
    assert_eq!(
        first, second,
        "output differed between processes — something is iterating a HashMap \
         or otherwise depending on per-process state"
    );
}

#[test]
fn the_lines_and_plain_mirror_never_drift() {
    // Search, scrolling and the match popup all assume `plain[i]` describes
    // `lines[i]`. If that ever stops holding, jumping to a match lands on the
    // wrong row, so assert it directly across a range of widths.
    let document = parse(&source());
    for width in [20, 40, 80, 120, 200] {
        let rendered = layout(&document, width, &Theme::default());
        assert_eq!(
            rendered.lines.len(),
            rendered.plain.len(),
            "mirror length drifted at width {width}"
        );
        for (index, line) in rendered.lines.iter().enumerate() {
            let expected: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert_eq!(
                rendered.plain[index], expected,
                "mirror content drifted at width {width}, line {index}"
            );
        }
    }
}

#[test]
fn anchors_and_links_point_at_lines_that_exist() {
    let document = parse(&source());
    for width in [20, 40, 80, 120] {
        let rendered = layout(&document, width, &Theme::default());
        for anchor in &rendered.anchors {
            assert!(
                anchor.line < rendered.len(),
                "anchor {anchor:?} out of range at width {width}"
            );
        }
        for link in &rendered.links {
            assert!(link.line < rendered.len(), "link {link:?} out of range at width {width}");
        }
    }
}

#[test]
fn themes_change_styling_but_never_the_text() {
    // A theme must be purely presentational: switching it must not add, drop, or
    // rewrap a single character of *content*.
    //
    // Trailing whitespace is excluded on purpose. A theme with a code-block
    // background pads those lines out to the full width so the background paints
    // as a solid rectangle; a theme without one has nothing to paint and emits no
    // padding. That difference is invisible to the reader and to search.
    let document = parse(&source());
    let content = |theme: ThemeKind| -> Vec<String> {
        layout(&document, 80, &Theme::new(theme))
            .plain
            .iter()
            .map(|l| l.trim_end().to_string())
            .collect()
    };

    assert_eq!(content(ThemeKind::Dark), content(ThemeKind::Light));
    assert_eq!(content(ThemeKind::Dark), content(ThemeKind::Mono));
}
