//! Viewer behaviour: scrolling, search navigation, popups, and resize.
//!
//! These drive `App` directly rather than a terminal. The state machine is where
//! the behaviour actually lives, and testing it without a pty keeps these fast
//! and deterministic. `tests/tui_capture.py` covers the drawing layer manually.

use mdlook::parse;
use mdlook::ui::app::{App, Mode};
use mdlook::ui::popup::PopupKind;
use mdlook::Theme;

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
