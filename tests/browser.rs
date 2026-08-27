//! The file browser, driven through `App` rather than a terminal.
//!
//! Same approach as `tests/viewer.rs`: the behaviour lives in the state
//! machine, so that is what gets tested. `tests/tui_capture.py` covers the
//! drawing.

use std::path::{Path, PathBuf};

use mdlook::files::Tree;
use mdlook::ui::app::{App, Focus, Mode};
use mdlook::{Content, Theme};

/// A small tree on disk, removed when the guard drops.
struct Fixture(PathBuf);

impl Fixture {
    fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("mdlook-browser-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("README.md"), "# Readme\n\nMentions needle once.\n").unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {\n    // needle here\n}\n").unwrap();
        std::fs::write(root.join("blob.bin"), b"\x89PNG\r\n\x1a\n\x00\x00").unwrap();
        std::fs::write(root.join("docs").join("guide.md"), "# Guide\n\nno hits\n").unwrap();
        Self(root)
    }

    fn app(&self) -> App {
        let tree = Tree::new(&self.0, false);
        let mut app =
            App::new(Content::Markdown(mdlook::parse("")), "x".into(), Theme::default(), 80)
                .with_sidebar(tree, 30, None);
        app.viewport = 10;
        app
    }

    fn select(&self, app: &mut App, name: &str) {
        let index = app
            .sidebar
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .position(|r| r.name == name)
            .unwrap_or_else(|| panic!("no row named {name}"));
        app.tree_move(|tree| {
            tree.selected = index;
        });
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn body(app: &App) -> String {
    app.rendered.plain.join("\n")
}

// -- focus -----------------------------------------------------------------

#[test]
fn a_browser_starts_focused_on_the_tree() {
    let fixture = Fixture::new("focus");
    let app = fixture.app();
    assert!(app.browsing());
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn focus_moves_back_and_forth() {
    let fixture = Fixture::new("toggle-focus");
    let mut app = fixture.app();
    app.toggle_focus();
    assert_eq!(app.focus, Focus::Content);
    app.toggle_focus();
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn hiding_the_tree_moves_focus_to_the_document() {
    // There is nothing to focus once the tree is gone, and leaving focus there
    // would swallow every key with no visible cause.
    let fixture = Fixture::new("hide");
    let mut app = fixture.app();
    app.toggle_sidebar();
    assert!(!app.browsing());
    assert_eq!(app.focus, Focus::Content);
    app.toggle_sidebar();
    assert!(app.browsing());
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn hiding_the_tree_keeps_it_rather_than_forgetting_where_you_were() {
    let fixture = Fixture::new("keep");
    let mut app = fixture.app();
    fixture.select(&mut app, "main.rs");
    app.toggle_sidebar();
    app.toggle_sidebar();
    assert_eq!(app.sidebar.as_ref().unwrap().selection().unwrap().name, "main.rs");
}

#[test]
fn focus_cannot_land_on_a_tree_that_is_not_there() {
    let fixture = Fixture::new("nosidebar");
    let mut app =
        App::new(Content::Markdown(mdlook::parse("# x")), "x".into(), Theme::default(), 80);
    app.toggle_focus();
    assert_eq!(app.focus, Focus::Content);
    app.toggle_sidebar();
    assert!(!app.browsing(), "there is no tree to show");
    drop(fixture);
}

// -- previewing ------------------------------------------------------------

#[test]
fn moving_the_selection_previews_the_file() {
    let fixture = Fixture::new("preview");
    let mut app = fixture.app();

    fixture.select(&mut app, "README.md");
    assert!(body(&app).contains("Mentions needle once."));
    assert_eq!(app.rendered.anchors.len(), 1, "markdown should still be markdown");

    fixture.select(&mut app, "main.rs");
    assert!(body(&app).contains("fn main()"));
    assert!(body(&app).starts_with("1 "), "source files get a line-number gutter");

    fixture.select(&mut app, "blob.bin");
    assert!(body(&app).contains("PNG image"), "a binary is identified, not dumped");
}

#[test]
fn moving_onto_a_directory_leaves_the_document_alone() {
    // Passing over a folder on the way somewhere should not blank out what you
    // were reading.
    let fixture = Fixture::new("dir");
    let mut app = fixture.app();
    fixture.select(&mut app, "README.md");
    let before = body(&app);
    fixture.select(&mut app, "docs");
    assert_eq!(body(&app), before);
}

#[test]
fn the_title_is_relative_to_the_root() {
    let fixture = Fixture::new("title");
    let mut app = fixture.app();
    fixture.select(&mut app, "main.rs");
    assert_eq!(app.title, "main.rs", "an absolute path would fill the status bar");
}

#[test]
fn opening_a_file_hands_the_keyboard_to_the_document() {
    let fixture = Fixture::new("open");
    let mut app = fixture.app();
    fixture.select(&mut app, "main.rs");
    app.open_selection();
    assert_eq!(app.focus, Focus::Content);
    assert!(body(&app).contains("fn main()"));
}

#[test]
fn opening_a_directory_expands_it_and_keeps_the_keyboard() {
    let fixture = Fixture::new("expand");
    let mut app = fixture.app();
    fixture.select(&mut app, "docs");
    app.open_selection();
    assert_eq!(app.focus, Focus::Tree, "you are still choosing, not reading");
    let names: Vec<_> = app.sidebar.as_ref().unwrap().rows.iter().map(|r| r.name.clone()).collect();
    assert!(names.contains(&"guide.md".to_string()));
}

#[test]
fn scrolling_resets_when_another_file_is_opened() {
    let fixture = Fixture::new("scroll");
    let mut app = fixture.app();
    fixture.select(&mut app, "README.md");
    app.scroll_by(2);
    fixture.select(&mut app, "main.rs");
    assert_eq!(app.scroll, 0, "a new file starts at its top");
}

// -- search across files ---------------------------------------------------

#[test]
fn a_query_survives_moving_to_another_file() {
    // Walking a tree with a query live is the point of keeping it: you are
    // looking for where something is mentioned, not searching one file.
    let fixture = Fixture::new("search");
    let mut app = fixture.app();
    fixture.select(&mut app, "README.md");

    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    app.popup_commit();
    assert_eq!(app.search.matches.len(), 1);

    fixture.select(&mut app, "main.rs");
    assert_eq!(app.search.query, "needle", "the query should still be live");
    assert_eq!(app.search.matches.len(), 1, "and re-run against the new file");
    let hit = app.search.matches[0];
    assert_eq!(&app.rendered.plain[hit.line][hit.start..hit.end], "needle");

    fixture.select(&mut app, "docs");
    fixture.select(&mut app, "blob.bin");
    assert!(app.search.matches.is_empty(), "no hits here, and no stale ones either");
}

// -- filtering -------------------------------------------------------------

#[test]
fn typing_a_filter_narrows_the_tree_and_previews_the_survivor() {
    let fixture = Fixture::new("filter");
    let mut app = fixture.app();
    app.open_filter();
    assert_eq!(app.mode, Mode::FilterInput);
    for c in "main".chars() {
        app.filter_push(c);
    }
    let rows = &app.sidebar.as_ref().unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "main.rs");
    assert!(body(&app).contains("fn main()"), "the survivor should be previewed");

    app.filter_commit();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.sidebar.as_ref().unwrap().filter, "main");
}

#[test]
fn cancelling_a_filter_puts_the_tree_back() {
    let fixture = Fixture::new("filter-cancel");
    let mut app = fixture.app();
    let before = app.sidebar.as_ref().unwrap().rows.len();

    app.open_filter();
    for c in "main".chars() {
        app.filter_push(c);
    }
    app.filter_cancel();
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.sidebar.as_ref().unwrap().rows.len(), before);
    assert!(app.sidebar.as_ref().unwrap().filter.is_empty());
}

#[test]
fn backspacing_a_filter_widens_it_again() {
    let fixture = Fixture::new("filter-back");
    let mut app = fixture.app();
    app.open_filter();
    for c in "mainx".chars() {
        app.filter_push(c);
    }
    assert!(app.sidebar.as_ref().unwrap().rows.is_empty());
    app.filter_pop();
    assert_eq!(app.sidebar.as_ref().unwrap().rows.len(), 1);
}

// -- resize ----------------------------------------------------------------

#[test]
fn toggling_the_tree_reflows_the_document() {
    // The browser takes columns from the document, so showing and hiding it is
    // a width change and has to re-lay out rather than clip.
    let fixture = Fixture::new("reflow");
    let mut app = fixture.app();
    fixture.select(&mut app, "README.md");

    app.relayout(50);
    assert_eq!(app.rendered.width, 50);
    app.toggle_sidebar();
    app.relayout(80);
    assert_eq!(app.rendered.width, 80);
    for line in &app.rendered.plain {
        assert!(mdlook::layout::wrap::text_width(line) <= 80);
    }
}

// -- robustness ------------------------------------------------------------

#[test]
fn an_unreadable_root_does_not_stop_the_viewer_starting() {
    let tree = Tree::new(Path::new("/nonexistent/nowhere"), false);
    let app = App::new(Content::Markdown(mdlook::parse("# x")), "x".into(), Theme::default(), 80)
        .with_sidebar(tree, 30, None);
    assert!(app.browsing());
    assert_eq!(app.sidebar.as_ref().unwrap().selection(), None);
}

#[test]
fn a_note_row_is_never_treated_as_a_file() {
    let mut app =
        App::new(Content::Markdown(mdlook::parse("# x")), "x".into(), Theme::default(), 80)
            .with_sidebar(Tree::new(Path::new("/nonexistent/nowhere"), false), 30, None);
    let before = body(&app);
    app.open_selection();
    assert_eq!(body(&app), before, "opening a note should do nothing at all");
}

// -- hex -------------------------------------------------------------------

#[test]
fn x_shows_the_selected_file_as_hex_and_x_again_leaves() {
    let fixture = Fixture::new("hex");
    let mut app = fixture.app();
    fixture.select(&mut app, "blob.bin");

    app.toggle_hex();
    assert!(matches!(app.content, Content::Hex { .. }), "x opens the dump");
    let dump = body(&app);
    assert!(dump.contains("89 50 4e 47"), "the bytes are on the page: {dump}");
    assert!(dump.contains("PNG"), "printable bytes read in the ASCII column: {dump}");

    app.toggle_hex();
    assert!(!matches!(app.content, Content::Hex { .. }), "x again goes back");
}

#[test]
fn hex_does_not_follow_the_cursor_to_the_next_file() {
    let fixture = Fixture::new("hex-per-file");
    let mut app = fixture.app();
    fixture.select(&mut app, "main.rs");
    app.toggle_hex();
    assert!(matches!(app.content, Content::Hex { .. }));

    fixture.select(&mut app, "README.md");
    assert!(
        matches!(app.content, Content::Markdown(_)),
        "the next file arrives in its normal view"
    );
}

#[test]
fn a_text_file_round_trips_through_hex() {
    let fixture = Fixture::new("hex-roundtrip");
    let mut app = fixture.app();
    fixture.select(&mut app, "main.rs");
    assert!(matches!(app.content, Content::Text { .. }));

    app.toggle_hex();
    assert!(body(&app).contains("fn main"), "the source shows in the ASCII column");
    app.toggle_hex();
    assert!(matches!(app.content, Content::Text { .. }), "back to highlighted source");
}
