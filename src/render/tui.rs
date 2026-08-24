//! The interactive viewer: event loop, drawing, and key handling.

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

use crate::files::Tree;
use crate::layout::wrap::text_width;
use crate::layout::Theme;
use crate::ui::app::{App, Focus, Mode};
use crate::ui::popup::{self, PopupKind};
use crate::ui::sidebar;

/// Longest line we lay text out to, however wide the terminal is.
///
/// Beyond roughly this, prose gets hard to track from the end of one line to the
/// start of the next, so extra columns are left empty rather than used.
pub const DEFAULT_MAX_WIDTH: usize = 100;

pub fn run(mut app: App, max_width: Option<usize>) -> Result<()> {
    let mut terminal = ratatui::init();
    // ratatui::init() does not enable mouse reporting, and scrolling is the one
    // thing people reach for the mouse to do in a pager.
    let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);

    let result = event_loop(&mut terminal, &mut app, max_width);

    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    max_width: Option<usize>,
) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app, max_width))?;
        if app.quit {
            return Ok(());
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.message = None;
                handle_key(app, key);
            }
            Event::Mouse(mouse) => {
                // Scroll whichever pane the pointer is over, which is the only
                // reading of the gesture that does not surprise anyone.
                let over_tree = app.browsing() && mouse.column < app.sidebar_columns;
                match (mouse.kind, over_tree) {
                    (MouseEventKind::ScrollDown, true) => app.tree_move(|t| t.step(3)),
                    (MouseEventKind::ScrollUp, true) => app.tree_move(|t| t.step(-3)),
                    (MouseEventKind::ScrollDown, false) => app.scroll_by(3),
                    (MouseEventKind::ScrollUp, false) => app.scroll_by(-3),
                    _ => {}
                }
            }
            // The next draw re-reads the area, so just let it fall through.
            Event::Resize(..) => {}
            _ => {}
        }
    }
}

fn content_width(area: Rect, max_width: Option<usize>) -> usize {
    let cap = max_width.unwrap_or(DEFAULT_MAX_WIDTH);
    (area.width as usize).min(cap).max(8)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw(frame: &mut Frame, app: &mut App, max_width: Option<usize>) {
    let area = frame.area();
    if area.height < 2 || area.width < 4 {
        return;
    }

    let panes = Rect { height: area.height - 1, ..area };
    let bar = Rect { y: area.y + area.height - 1, height: 1, ..area };

    // The browser gives up its columns rather than squeeze the document on a
    // narrow terminal, so the split can come back refused.
    let split = app.browsing().then(|| sidebar::split(panes, app.sidebar_width)).flatten();
    let body = match split {
        Some((tree_area, document)) => {
            app.sidebar_columns = tree_area.width;
            if let Some(tree) = app.sidebar.as_mut() {
                tree.scroll_into_view(tree_area.height.saturating_sub(1) as usize);
            }
            let focused = app.focus == Focus::Tree;
            sidebar::render(
                app.sidebar.as_ref().unwrap(),
                tree_area,
                &app.theme,
                focused,
                frame.buffer_mut(),
            );
            document
        }
        None => {
            app.sidebar_columns = 0;
            panes
        }
    };

    // Relayout uses the document pane's width, not the frame's: toggling the
    // browser is a width change like any other, and re-anchors the same way.
    app.relayout(content_width(body, max_width));
    app.viewport = body.height as usize;
    // Height changes can leave the scroll past the new end of the document.
    app.scroll_by(0);

    draw_body(frame, app, body);
    draw_bar(frame, app, bar);

    if let Some(pop) = &app.popup {
        // Keep the previewed line out from under the popup.
        let focus = pop
            .target()
            .filter(|_| pop.kind.previews())
            .and_then(|line| line.checked_sub(app.scroll))
            .map(|offset| body.y + (offset as u16).min(body.height.saturating_sub(1)));

        let wanted = pop.rows.iter().map(|r| r.height()).sum::<usize>() + 2;
        let height = (wanted as u16).min(body.height.saturating_sub(2)).max(3);
        let area = popup::placement(body, height, focus);

        // `scroll_into_view` needs the height we actually got, not the one we
        // asked for, or the selection can sit just off the bottom edge.
        if let Some(pop) = app.popup.as_mut() {
            pop.scroll_into_view(area.height.saturating_sub(2) as usize);
        }
        popup::render(app.popup.as_ref().unwrap(), area, &app.theme, frame.buffer_mut());
    }
}

fn draw_body(frame: &mut Frame, app: &App, area: Rect) {
    let end = (app.scroll + area.height as usize).min(app.rendered.len());
    let lines: Vec<Line<'static>> = (app.scroll..end)
        .map(|index| {
            let line = &app.rendered.lines[index];
            if app.search.is_active() {
                let hits: Vec<_> = app.search.on_line(index).collect();
                paint_matches(line, &hits, &app.theme)
            } else {
                line.clone()
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_bar(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // While typing, the bar becomes the prompt.
    if let Some(prompt) = match app.mode {
        Mode::SearchInput => Some(("/", app.search.query.clone())),
        Mode::FilterInput => {
            Some(("filter ", app.sidebar.as_ref().map(|t| t.filter.clone()).unwrap_or_default()))
        }
        _ => None,
    } {
        let spans = vec![
            Span::styled(prompt.0, theme.status),
            Span::styled(prompt.1, theme.status),
            Span::styled("▏", theme.status),
        ];
        render_bar(frame, area, spans, theme);
        return;
    }

    let percent = if app.max_scroll() == 0 { 100 } else { app.scroll * 100 / app.max_scroll() };

    let mut left = format!(" {} ", app.title);
    if app.search.is_active() {
        left.push_str(&format!(
            " │ /{}  {}/{} ",
            app.search.query,
            if app.search.matches.is_empty() { 0 } else { app.search.current + 1 },
            app.search.matches.len()
        ));
    }
    if let Some(message) = &app.message {
        left.push_str(&format!(" │ {message} "));
    }

    // The browser's key is worth advertising: it is the one thing on screen
    // whose absence is not obvious once it is hidden.
    let toggle = if app.sidebar.is_some() { "^B:files  " } else { "" };
    let right = format!(" {percent:>3}%  {toggle}?:help  q:quit ");
    let gap = (area.width as usize).saturating_sub(text_width(&left) + text_width(&right));

    let spans = vec![
        Span::styled(left, theme.status),
        Span::styled(" ".repeat(gap), theme.status),
        Span::styled(right, theme.status),
    ];
    render_bar(frame, area, spans, theme);
}

fn render_bar(frame: &mut Frame, area: Rect, spans: Vec<Span<'static>>, theme: &Theme) {
    let used: usize = spans.iter().map(|s| text_width(&s.content)).sum();
    let mut spans = spans;
    if used < area.width as usize {
        spans.push(Span::styled(" ".repeat(area.width as usize - used), theme.status));
    }
    Paragraph::new(Line::from(spans)).render(area, frame.buffer_mut());
}

/// Repaint a line with search hits highlighted.
///
/// Hits are byte ranges into the line's plain text, so this walks the spans
/// tracking byte offsets and splits any span a hit starts or ends inside of. The
/// original style is patched rather than replaced, so highlighted code stays
/// recognisably code.
fn paint_matches(
    line: &Line<'static>,
    hits: &[(crate::ui::search::Match, bool)],
    theme: &Theme,
) -> Line<'static> {
    if hits.is_empty() {
        return line.clone();
    }

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut offset = 0usize;

    for span in &line.spans {
        let text = span.content.as_ref();
        let (span_start, span_end) = (offset, offset + text.len());
        offset = span_end;

        // Every boundary that falls strictly inside this span becomes a cut.
        let mut cuts = vec![span_start, span_end];
        for (hit, _) in hits {
            for edge in [hit.start, hit.end] {
                if edge > span_start && edge < span_end {
                    cuts.push(edge);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();

        for pair in cuts.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let piece = &text[from - span_start..to - span_start];
            if piece.is_empty() {
                continue;
            }
            let style = match hits.iter().find(|(h, _)| from >= h.start && to <= h.end) {
                Some((_, true)) => span.style.patch(theme.search_current),
                Some((_, false)) => span.style.patch(theme.search_match),
                None => span.style,
            };
            out.push(Span::styled(piece.to_string(), style));
        }
    }

    Line::from(out)
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

fn handle_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.quit = true;
        return;
    }

    match app.mode {
        Mode::SearchInput => search_input_key(app, key),
        Mode::FilterInput => filter_input_key(app, key),
        Mode::Popup => popup_key(app, key),
        Mode::Normal => normal_key(app, key),
    }
}

/// Keys that mean the same thing whichever pane has focus.
///
/// Handled before the per-pane tables so neither can shadow them; without this
/// the browser would have to re-implement quitting and help to stay usable.
fn shared_key(app: &mut App, key: KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('b')) {
        app.toggle_sidebar();
        return true;
    }
    match key.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('?') => app.open_popup(PopupKind::Help),
        KeyCode::Tab | KeyCode::BackTab => app.toggle_focus(),
        _ => return false,
    }
    true
}

fn normal_key(app: &mut App, key: KeyEvent) {
    if shared_key(app, key) {
        return;
    }
    match app.focus {
        Focus::Tree if app.browsing() => tree_key(app, key),
        _ => content_key(app, key),
    }
}

/// Keys for the file browser.
///
/// `h`/`l` expand and collapse here while `l` opens the link list in the
/// document pane. Each pane owning its own letters is what keeps both tables
/// short; the alternative is a second key for everything the browser adds.
fn tree_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.tree_move(|t| t.step(1)),
        KeyCode::Char('k') | KeyCode::Up => app.tree_move(|t| t.step(-1)),
        KeyCode::Char('d') => app.tree_move(|t| t.step(8)),
        KeyCode::Char('u') => app.tree_move(|t| t.step(-8)),
        KeyCode::Char('f') | KeyCode::PageDown => app.tree_move(|t| t.step(16)),
        KeyCode::Char('b') | KeyCode::PageUp => app.tree_move(|t| t.step(-16)),
        KeyCode::Char('g') | KeyCode::Home => app.tree_move(Tree::to_top),
        KeyCode::Char('G') | KeyCode::End => app.tree_move(Tree::to_bottom),

        KeyCode::Char('l') | KeyCode::Right => app.tree_move(|t| {
            t.expand();
        }),
        KeyCode::Char('h') | KeyCode::Left => app.tree_move(|t| {
            t.collapse();
        }),
        KeyCode::Enter => app.open_selection(),

        KeyCode::Char('.') => app.tree_move(Tree::toggle_hidden),
        KeyCode::Char('/') => app.open_filter(),
        KeyCode::Esc => {
            if app.sidebar.as_ref().is_some_and(|t| !t.filter.is_empty()) {
                app.tree_move(Tree::clear_filter);
            } else {
                app.quit = true;
            }
        }
        _ => {}
    }
}

fn content_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            if app.search.is_active() {
                app.search.clear();
            } else if app.browsing() {
                // With a browser open, Escape steps back to it rather than
                // quitting: leaving the application is what `q` is for.
                app.focus = Focus::Tree;
            } else {
                app.quit = true;
            }
        }

        KeyCode::Char('j') | KeyCode::Down => app.scroll_by(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_by(-1),
        KeyCode::Char('d') => app.scroll_half_pages(1),
        KeyCode::Char('u') => app.scroll_half_pages(-1),
        KeyCode::Char('f') | KeyCode::PageDown | KeyCode::Char(' ') => app.scroll_pages(1),
        KeyCode::Char('b') | KeyCode::PageUp => app.scroll_pages(-1),
        KeyCode::Char('g') | KeyCode::Home => app.to_top(),
        KeyCode::Char('G') | KeyCode::End => app.to_bottom(),

        KeyCode::Char('/') => app.open_search(),
        KeyCode::Char('n') => app.step_match(true),
        KeyCode::Char('N') => app.step_match(false),

        KeyCode::Char('t') => app.open_popup(PopupKind::Outline),
        KeyCode::Char('l') => app.open_popup(PopupKind::Links),
        _ => {}
    }
}

fn filter_input_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.filter_cancel(),
        KeyCode::Enter => app.filter_commit(),
        KeyCode::Backspace => app.filter_pop(),
        KeyCode::Down => app.tree_move(|t| t.step(1)),
        KeyCode::Up => app.tree_move(|t| t.step(-1)),
        KeyCode::Char(c) if is_text(&key) => app.filter_push(c),
        _ => {}
    }
}

fn search_input_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.popup_cancel(),
        KeyCode::Enter => app.popup_commit(),
        KeyCode::Backspace => app.search_pop(),
        // Move through results without leaving the prompt, so you can keep
        // refining the query after looking at a few hits.
        KeyCode::Down | KeyCode::Tab => app.popup_step(true),
        KeyCode::Up | KeyCode::BackTab => app.popup_step(false),
        KeyCode::Char(c) if is_text(&key) => app.search_push(c),
        _ => {}
    }
}

/// Whether a `Char` event is someone typing, rather than a chord.
///
/// A terminal reports Ctrl-J as `Char('j')` with a modifier set, so a prompt
/// that matches on `Char` alone quietly types the letter when the reader meant
/// a control key. Alt is excluded for the same reason.
fn is_text(key: &KeyEvent) -> bool {
    !key.modifiers.intersects(KeyModifiers::CONTROL.union(KeyModifiers::ALT))
}

fn popup_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.popup_cancel(),
        KeyCode::Enter => app.popup_commit(),
        KeyCode::Char('j') | KeyCode::Down => app.popup_step(true),
        KeyCode::Char('k') | KeyCode::Up => app.popup_step(false),
        KeyCode::Char('g') | KeyCode::Home => {
            if let Some(pop) = app.popup.as_mut() {
                pop.jump(0);
            }
            preview(app);
        }
        KeyCode::Char('G') | KeyCode::End => {
            if let Some(pop) = app.popup.as_mut() {
                let last = pop.rows.len().saturating_sub(1);
                pop.jump(last);
            }
            preview(app);
        }
        _ => {}
    }
}

fn preview(app: &mut App) {
    let target = app.popup.as_ref().filter(|p| p.kind.previews()).and_then(|p| p.target());
    if let Some(line) = target {
        app.reveal(line);
    }
}

/// Style used for the cursor in the search prompt.
#[allow(dead_code)]
fn cursor_style() -> Style {
    Style::new().add_modifier(Modifier::SLOW_BLINK)
}
