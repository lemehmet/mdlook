//! Viewer behaviour: scrolling, search navigation, popups, and resize.
//!
//! These drive `App` directly rather than a terminal. The state machine is where
//! the behaviour actually lives, and testing it without a pty keeps these fast
//! and deterministic. `tests/tui_capture.py` covers the drawing layer manually.

use mdlook::content::Content;
use mdlook::parse;
use mdlook::ui::app::{App, Mode};
use mdlook::ui::popup::PopupKind;
use mdlook::Theme;
use ratatui::layout::Rect;

const DOC: &str = "\
# Alpha

Alpha body mentioning needle once.

## Beta

Beta body. Filler filler filler.

Another paragraph with needle in it.

## Gamma

Gamma body, no hits here.

See [the guide](https://example.com/guide) and [rfc](https://x.test/r).

## Delta

Final section with a needle at the end.
";

fn app() -> App {
    let mut app = App::new(parse(DOC).into(), "test.md".into(), Theme::default(), 80);
    app.viewport = 10;
    app
}

// -- scrolling -------------------------------------------------------------

#[test]
fn scrolling_is_clamped_at_both_ends() {
    let mut app = app();
    app.scroll_by(-5);
    assert_eq!(app.scroll, 0, "scrolled above the top");

    app.to_bottom();
    let bottom = app.scroll;
    app.scroll_by(1000);
    assert_eq!(app.scroll, bottom, "scrolled past the end");
    assert_eq!(bottom, app.rendered.len().saturating_sub(app.viewport));
}

#[test]
fn half_and_full_page_scrolling_move_by_the_viewport() {
    let mut app = app();
    app.viewport = 10;
    app.scroll_pages(1);
    assert_eq!(app.scroll, 10);
    app.scroll_half_pages(-1);
    assert_eq!(app.scroll, 5);
}

#[test]
fn reveal_leaves_room_to_read_forwards() {
    let mut app = app();
    app.viewport = 12;
    let target = app.rendered.len() - 1;
    app.reveal(target);
    // The target sits a third of the way down, not pinned to the top edge.
    let row = target - app.scroll;
    assert!(row < app.viewport, "target off screen");
}

// -- search ----------------------------------------------------------------

#[test]
fn typing_a_query_finds_matches_and_opens_the_list() {
    let mut app = app();
    app.open_search();
    assert_eq!(app.mode, Mode::SearchInput);
    for c in "needle".chars() {
        app.search_push(c);
    }
    assert_eq!(app.search.matches.len(), 3, "expected three hits");
    let popup = app.popup.as_ref().expect("results popup");
    assert_eq!(popup.kind, PopupKind::Search);
    assert_eq!(popup.rows.len(), 3);
}

#[test]
fn search_matches_rendered_text_not_markdown_source() {
    // The heading is `## Beta` in source but `Beta` on screen, and inline code /
    // emphasis markers are likewise absent from what you can search.
    let mut app = App::new(
        parse("## `fetch_user()`\n\nSome **bold** text.").into(),
        "t".into(),
        Theme::default(),
        80,
    );
    app.viewport = 10;
    app.open_search();
    for c in "fetch_user".chars() {
        app.search_push(c);
    }
    assert_eq!(app.search.matches.len(), 1, "backticked heading not searchable");

    app.open_search();
    for c in "bold".chars() {
        app.search_push(c);
    }
    assert_eq!(app.search.matches.len(), 1);
}

#[test]
fn every_match_line_actually_contains_the_query() {
    let mut app = app();
    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    for m in &app.search.matches {
        let line = &app.rendered.plain[m.line];
        assert_eq!(&line[m.start..m.end], "needle", "bad offsets in {line:?}");
    }
}

#[test]
fn moving_through_results_scrolls_the_document_behind_the_popup() {
    let mut app = app();
    app.viewport = 6;
    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }

    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(app.scroll);
        app.popup_step(true);
    }
    assert!(
        seen.windows(2).any(|w| w[0] != w[1]),
        "document never scrolled while stepping results: {seen:?}"
    );
}

#[test]
fn committing_leaves_you_at_the_selected_match() {
    let mut app = app();
    app.viewport = 6;
    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    app.popup_step(true);
    let target = app.popup.as_ref().unwrap().target().unwrap();

    app.popup_commit();
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.popup.is_none());
    assert!(
        target >= app.scroll && target < app.scroll + app.viewport,
        "match {target} not visible in {}..{}",
        app.scroll,
        app.scroll + app.viewport
    );
}

#[test]
fn cancelling_a_search_restores_the_original_position() {
    let mut app = app();
    app.viewport = 6;
    app.scroll_by(4);
    let before = app.scroll;

    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    app.popup_step(true);
    app.popup_cancel();

    assert_eq!(app.scroll, before, "Esc did not restore the scroll position");
    assert_eq!(app.mode, Mode::Normal);
    assert!(!app.search.is_active(), "cancelled search left highlights on");
}

#[test]
fn backspacing_the_query_updates_the_results() {
    let mut app = app();
    app.open_search();
    for c in "needlex".chars() {
        app.search_push(c);
    }
    assert_eq!(app.search.matches.len(), 0);
    app.search_pop();
    assert_eq!(app.search.matches.len(), 3, "results did not recover");
}

#[test]
fn stepping_matches_wraps_and_reveals() {
    let mut app = app();
    app.viewport = 6;
    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    app.popup_commit();

    for _ in 0..5 {
        app.step_match(true);
        let m = app.search.current_match().expect("a current match");
        assert!(
            m.line >= app.scroll && m.line < app.scroll + app.viewport,
            "stepped to a match that is off screen"
        );
    }
}

// -- popups ----------------------------------------------------------------

#[test]
fn outline_lists_every_heading_and_jumps_to_it() {
    let mut app = app();
    app.open_popup(PopupKind::Outline);
    let popup = app.popup.as_ref().unwrap();
    assert_eq!(popup.rows.len(), 4, "expected Alpha, Beta, Gamma, Delta");

    app.popup_step(true);
    let target = app.popup.as_ref().unwrap().target().unwrap();
    app.popup_commit();
    assert!(app.rendered.plain[target].contains("Beta"));
}

#[test]
fn link_list_pairs_each_link_with_its_url() {
    let mut app = app();
    app.open_popup(PopupKind::Links);
    let popup = app.popup.as_ref().unwrap();
    assert_eq!(popup.rows.len(), 2);
    // Two display lines each: the text, then the URL beneath it.
    assert_eq!(popup.rows[0].lines.len(), 2);
}

#[test]
fn opening_an_index_selects_what_is_already_on_screen() {
    // Opening the outline near the end should not dump you back at the first
    // heading; the list should start where you are.
    let mut app = app();
    app.viewport = 6;
    app.to_bottom();
    app.open_popup(PopupKind::Outline);
    let popup = app.popup.as_ref().unwrap();
    assert!(popup.selected > 0, "outline opened at the top of the document");
}

#[test]
fn help_does_not_move_the_document() {
    let mut app = app();
    app.viewport = 6;
    app.scroll_by(3);
    let before = app.scroll;
    app.open_popup(PopupKind::Help);
    app.popup_step(true);
    app.popup_step(true);
    assert_eq!(app.scroll, before, "help scrolled the document");
}

// -- resize ----------------------------------------------------------------

#[test]
fn resize_keeps_you_in_the_same_section() {
    let mut app = app();
    app.viewport = 6;

    // Park inside the Gamma section.
    let gamma =
        app.rendered.anchors.iter().find(|a| a.text == "Gamma").expect("Gamma heading").line;
    app.scroll = gamma;

    app.relayout(40);

    let section = app.rendered.heading_at(app.scroll).expect("a heading above the scroll position");
    assert_eq!(
        section.text, "Gamma",
        "resize moved the reader into the {:?} section",
        section.text
    );
}

#[test]
fn resize_rebuilds_search_matches_against_the_new_layout() {
    let mut app = app();
    app.viewport = 6;
    app.open_search();
    for c in "needle".chars() {
        app.search_push(c);
    }
    app.popup_commit();

    app.relayout(38);

    assert_eq!(app.search.matches.len(), 3, "matches lost on resize");
    for m in &app.search.matches {
        assert!(m.line < app.rendered.len(), "stale line index after resize");
        let line = &app.rendered.plain[m.line];
        assert_eq!(&line[m.start..m.end], "needle", "stale offsets after resize");
    }
}

#[test]
fn resize_keeps_the_scroll_inside_the_document() {
    let mut app = app();
    app.viewport = 6;
    app.to_bottom();
    for width in [200, 30, 120, 24, 80] {
        app.relayout(width);
        assert!(
            app.scroll <= app.rendered.len(),
            "scroll {} past end {} at width {width}",
            app.scroll,
            app.rendered.len()
        );
    }
}

// -- hex -------------------------------------------------------------------

#[test]
fn hex_with_no_file_reports_rather_than_toggles() {
    // Stdin: there is a document but no path, so there is nothing to re-read.
    let mut app = app();
    app.toggle_hex();
    assert!(matches!(app.content, mdlook::Content::Markdown(_)), "the document is untouched");
    assert!(app.message.is_some(), "the refusal is said, not silent");
}

#[test]
fn a_named_file_toggles_to_hex_and_back() {
    let dir = std::env::temp_dir().join("mdlook-viewer-hex");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("named.md");
    std::fs::write(&path, "# Title\n").unwrap();

    let mut app = App::new(
        mdlook::Content::read(&path, None, Default::default()).unwrap(),
        "named.md".into(),
        Theme::default(),
        80,
    )
    .with_file(path);
    app.viewport = 10;

    app.toggle_hex();
    assert!(matches!(app.content, mdlook::Content::Hex { .. }));
    assert!(
        app.rendered.plain[0].starts_with("00000000  23 20 54 69 74 6c 65 0a"),
        "{}",
        app.rendered.plain[0]
    );

    app.toggle_hex();
    assert!(matches!(app.content, mdlook::Content::Markdown(_)), "back to rendered markdown");
}

// -- mouse selection -------------------------------------------------------

/// An app whose document pane sits at (`x`, `y`), as if beside a browser.
fn pane_at(mut app: App, x: u16, y: u16) -> App {
    app.body = Rect { x, y, width: 60, height: 10 };
    app.viewport = 10;
    app
}

/// The screen row on which document line `line` is drawn.
fn row_of(app: &App, line: usize) -> u16 {
    app.body.y + (line - app.scroll) as u16
}

fn line_of(app: &App, needle: &str) -> usize {
    app.rendered.plain.iter().position(|l| l.contains(needle)).expect("line not found")
}

#[test]
fn dragging_across_a_line_copies_what_was_under_the_pointer() {
    let mut app = pane_at(app(), 30, 2);
    let line = line_of(&app, "Alpha body");
    let column = app.rendered.plain[line].find("Alpha body").unwrap() as u16;
    let row = row_of(&app, line);

    assert!(app.select_start(30 + column, row));
    app.select_drag(30 + column + 5, row);
    let copied = app.select_end(30 + column + 9, row);
    assert_eq!(copied.as_deref(), Some("Alpha body"));
}

#[test]
fn a_selection_spans_lines_and_trims_trailing_blanks() {
    let mut app = pane_at(app(), 0, 0);
    app.body.height = 30;
    app.viewport = 30;
    let first = line_of(&app, "Beta body");
    let last = line_of(&app, "Another paragraph");
    app.select_start(0, row_of(&app, first));
    let text = app.select_end(6, row_of(&app, last)).unwrap();
    assert_eq!(text, "Beta body. Filler filler filler.\n\nAnother");
}

#[test]
fn a_drag_backwards_copies_the_same_text() {
    let mut forward = pane_at(app(), 0, 0);
    let line = line_of(&forward, "Beta body");
    let row = row_of(&forward, line);
    forward.select_start(0, row);
    let a = forward.select_end(8, row);

    let mut backward = pane_at(app(), 0, 0);
    backward.select_start(8, row);
    let b = backward.select_end(0, row);
    assert_eq!(a, b);
    assert_eq!(a.as_deref(), Some("Beta body"));
}

#[test]
fn a_click_without_a_drag_clears_rather_than_copies() {
    let mut app = pane_at(app(), 0, 0);
    app.select_start(0, 1);
    assert!(app.select_end(4, 1).is_some());
    assert!(app.selection.is_some(), "a finished selection stays visible");

    app.select_start(2, 3);
    assert_eq!(app.select_end(2, 3), None);
    assert!(app.selection.is_none());
}

#[test]
fn presses_outside_the_document_pane_are_not_selections() {
    // The browser occupies the columns left of the pane.
    let mut app = pane_at(app(), 30, 0);
    assert!(!app.select_start(10, 2), "a press on the tree");
    assert!(!app.select_start(40, 12), "a press on the status bar");
    app.select_drag(45, 3);
    assert_eq!(app.select_end(45, 3), None);
    assert!(app.selection.is_none());
}

#[test]
fn dragging_past_the_bottom_scrolls_and_keeps_selecting() {
    let mut app = pane_at(app(), 0, 0);
    app.viewport = 4;
    app.body.height = 4;
    app.select_start(0, 0);
    for _ in 0..3 {
        app.select_drag(5, 10);
    }
    assert_eq!(app.scroll, 3, "each drag past the edge scrolls a line");
    let text = app.select_end(5, 10).unwrap();
    // Lines 0 through 6: the whole of what was scrolled through.
    assert_eq!(text.lines().count(), 7, "{text:?}");
}

#[test]
fn reflowing_drops_the_selection() {
    let mut app = pane_at(app(), 0, 0);
    app.select_start(0, 0);
    app.select_end(5, 2);
    assert!(app.selection.is_some());
    app.relayout(40);
    assert!(app.selection.is_none(), "the cells now hold different text");
}

#[test]
fn line_numbers_are_left_out_of_a_copy_from_source() {
    let content =
        Content::Text { name: "x.rs".into(), body: "fn main() {\n    run();\n}\n".into() };
    let mut app = pane_at(App::new(content, "x.rs".into(), Theme::default(), 60), 0, 0);
    let first = line_of(&app, "fn main");
    let last = line_of(&app, "run();");
    app.select_start(0, row_of(&app, first));
    let text = app.select_end(59, row_of(&app, last)).unwrap();
    assert_eq!(text, "fn main() {\n    run();");
}

#[test]
fn dragging_onto_the_top_row_scrolls_back_up() {
    // The pane sits on the terminal's first row, so the pointer can never be
    // reported above it; the top row has to serve as the edge.
    let mut app = pane_at(app(), 0, 0);
    app.viewport = 4;
    app.body.height = 4;
    app.scroll = 5;
    app.select_start(4, 3);
    app.select_drag(4, 0);
    app.select_drag(4, 0);
    assert_eq!(app.scroll, 3);
    app.select_end(0, 0).unwrap();
    assert_eq!(app.scroll, 3, "letting go does not scroll");
    assert_eq!(app.selection.unwrap().head.line, 3);
}

#[test]
fn sweeping_along_the_top_row_does_not_scroll() {
    let mut app = pane_at(app(), 0, 0);
    app.scroll = 5;
    app.select_start(0, 0);
    app.select_drag(10, 0);
    app.select_drag(20, 0);
    assert_eq!(app.scroll, 5);
}
