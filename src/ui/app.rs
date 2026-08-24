//! Viewer state and the transitions between its modes.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use std::path::{Path, PathBuf};

use crate::content::Content;
use crate::files::Tree;
use crate::layout::{wrap::text_width, RenderedDoc, Theme};
use crate::ui::popup::{Popup, PopupKind, PopupRow};
use crate::ui::search::Search;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    /// Typing a query. Results update live in the popup below.
    SearchInput,
    /// A popup has focus and the arrow keys move its selection.
    Popup,
    /// Typing a filter for the file browser.
    FilterInput,
}

/// Which pane the keyboard is driving.
///
/// Deliberately separate from [`Mode`] rather than folded into it. `Mode`
/// describes what the *document* pane is doing — reading, typing a query,
/// working a list — and those states are modal: they take the keyboard wherever
/// focus happens to be. Multiplying the two together would give a state per
/// combination, most of which mean the same thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Tree,
    Content,
}

pub struct App {
    pub content: Content,
    pub rendered: RenderedDoc,
    pub theme: Theme,
    pub title: String,
    pub scroll: usize,
    /// Height of the document viewport, refreshed by the renderer each frame.
    pub viewport: usize,
    pub mode: Mode,
    pub search: Search,
    pub popup: Option<Popup>,
    /// Scroll position to restore if the popup is cancelled rather than committed.
    restore_scroll: usize,
    pub quit: bool,
    pub message: Option<String>,

    // -- file browser ------------------------------------------------------
    /// Absent when mdlook was pointed at a single file, which is the default.
    pub sidebar: Option<Tree>,
    /// Whether the browser is drawn. Toggling hides it without losing the tree,
    /// so coming back lands where you left rather than at the root.
    pub sidebar_visible: bool,
    pub sidebar_width: usize,
    /// External command used to identify binaries, if the reader configured one.
    pub probe_command: Option<String>,
    /// Columns the browser actually occupied last frame, including its divider.
    ///
    /// Recorded by the renderer so the event loop can tell which pane the mouse
    /// is over. The requested width is not enough: the browser narrows itself on
    /// a small terminal and disappears entirely on a very small one.
    pub sidebar_columns: u16,
    pub focus: Focus,
    /// What the document pane is currently showing, so moving the selection
    /// onto a file already open does not re-read and re-highlight it.
    previewed: Option<PathBuf>,
    /// Filter text being typed, restored on cancel.
    restore_filter: String,
}

impl App {
    pub fn new(content: Content, title: String, theme: Theme, width: usize) -> Self {
        let rendered = content.layout(width, &theme);
        Self {
            content,
            rendered,
            theme,
            // The title is a path from the command line, drawn into the status
            // bar. A filename can carry control characters just as a document
            // can, and this one does not pass through the layout stage.
            title: title.chars().map(crate::layout::wrap::sanitize).collect(),
            scroll: 0,
            viewport: 1,
            mode: Mode::Normal,
            search: Search::default(),
            popup: None,
            restore_scroll: 0,
            quit: false,
            message: None,
            sidebar: None,
            sidebar_visible: false,
            sidebar_width: crate::config::DEFAULT_SIDEBAR_WIDTH,
            probe_command: None,
            sidebar_columns: 0,
            focus: Focus::Content,
            previewed: None,
            restore_filter: String::new(),
        }
    }

    /// Attach a file browser, which starts visible and focused.
    pub fn with_sidebar(mut self, tree: Tree, width: usize, probe: Option<String>) -> Self {
        self.probe_command = probe;
        self.previewed = tree.selected_file().map(Path::to_path_buf);
        self.sidebar = Some(tree);
        self.sidebar_visible = true;
        self.sidebar_width = width;
        self.focus = Focus::Tree;
        // The title was built from the command line, which may be an absolute
        // path; now that there is a root to measure against, shorten it.
        if let Some(path) = self.previewed.clone() {
            self.title = self.display_path(&path);
        }
        self
    }

    // -- the browser -------------------------------------------------------

    /// Whether the browser is attached *and* currently drawn.
    pub fn browsing(&self) -> bool {
        self.sidebar.is_some() && self.sidebar_visible
    }

    /// Show or hide the browser. With it hidden the document has the whole
    /// frame, which is the point; focus follows, because there is nothing left
    /// to focus.
    pub fn toggle_sidebar(&mut self) {
        if self.sidebar.is_none() {
            return;
        }
        self.sidebar_visible = !self.sidebar_visible;
        self.focus = if self.sidebar_visible { Focus::Tree } else { Focus::Content };
    }

    pub fn toggle_focus(&mut self) {
        if !self.browsing() {
            self.focus = Focus::Content;
            return;
        }
        self.focus = match self.focus {
            Focus::Tree => Focus::Content,
            Focus::Content => Focus::Tree,
        };
    }

    /// Move the browser's cursor and preview whatever it lands on.
    pub fn tree_move(&mut self, action: impl FnOnce(&mut Tree)) {
        let Some(tree) = self.sidebar.as_mut() else { return };
        action(tree);
        self.sync_preview();
    }

    /// Load the selected file into the document pane, unless it is already
    /// there. Directories and notes leave the pane alone: moving the cursor
    /// over a folder should not blank out what you were reading.
    fn sync_preview(&mut self) {
        let Some(tree) = self.sidebar.as_ref() else { return };
        let Some(path) = tree.selected_file().map(Path::to_path_buf) else { return };
        if self.previewed.as_deref() == Some(path.as_path()) {
            return;
        }
        self.load(&path);
    }

    fn load(&mut self, path: &Path) {
        self.content = Content::preview(path, self.probe_command.as_deref());
        self.title = self.display_path(path);
        self.rendered = self.content.layout(self.rendered.width.max(8), &self.theme);
        self.previewed = Some(path.to_path_buf());
        self.scroll = 0;
        self.restore_scroll = 0;
        self.popup = None;
        // A query survives the move to another file, so you can walk a tree
        // looking for where something is mentioned. It has to be re-run,
        // because the offsets belonged to the old document.
        if self.search.is_active() {
            self.search.refresh(&self.rendered);
        }
    }

    /// How a path is named in the status bar.
    ///
    /// Relative to the browser's root, because that is the tree the reader is
    /// looking at; an absolute path would spend the bar on directories that are
    /// the same for every file in the session.
    fn display_path(&self, path: &Path) -> String {
        let shown = self
            .sidebar
            .as_ref()
            .and_then(|tree| path.strip_prefix(tree.root()).ok())
            .unwrap_or(path);
        shown.to_string_lossy().chars().map(crate::layout::wrap::sanitize).collect()
    }

    /// Open the selection: expand a directory, or move to the document pane if
    /// the cursor is already on the file being shown.
    pub fn open_selection(&mut self) {
        let Some(tree) = self.sidebar.as_mut() else { return };
        match tree.selection().map(|row| (row.is_dir, row.path.clone())) {
            Some((true, _)) => {
                tree.toggle();
            }
            Some((false, path)) => {
                if self.previewed.as_deref() != Some(path.as_path()) {
                    self.load(&path);
                }
                self.focus = Focus::Content;
            }
            None => {}
        }
    }

    // -- browser filter ----------------------------------------------------

    pub fn open_filter(&mut self) {
        let Some(tree) = self.sidebar.as_ref() else { return };
        self.restore_filter = tree.filter.clone();
        self.mode = Mode::FilterInput;
    }

    pub fn filter_push(&mut self, c: char) {
        let Some(tree) = self.sidebar.as_mut() else { return };
        let mut filter = tree.filter.clone();
        filter.push(c);
        tree.set_filter(filter);
        self.sync_preview();
    }

    pub fn filter_pop(&mut self) {
        let Some(tree) = self.sidebar.as_mut() else { return };
        let mut filter = tree.filter.clone();
        filter.pop();
        tree.set_filter(filter);
        self.sync_preview();
    }

    pub fn filter_commit(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn filter_cancel(&mut self) {
        let restore = std::mem::take(&mut self.restore_filter);
        if let Some(tree) = self.sidebar.as_mut() {
            tree.set_filter(restore);
        }
        self.mode = Mode::Normal;
        self.sync_preview();
    }

    // -- scrolling ---------------------------------------------------------

    pub fn max_scroll(&self) -> usize {
        self.rendered.len().saturating_sub(self.viewport)
    }

    fn clamp(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    pub fn scroll_by(&mut self, delta: isize) {
        self.scroll = self.scroll.saturating_add_signed(delta);
        self.clamp();
    }

    pub fn scroll_pages(&mut self, pages: isize) {
        let page = self.viewport.max(1) as isize;
        self.scroll_by(pages * page);
    }

    pub fn scroll_half_pages(&mut self, pages: isize) {
        let half = (self.viewport.max(2) / 2) as isize;
        self.scroll_by(pages * half);
    }

    pub fn to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn to_bottom(&mut self) {
        self.scroll = self.max_scroll();
    }

    /// Scroll so `line` is visible, sitting a third of the way down.
    ///
    /// A third rather than centred: when you jump to a match you almost always
    /// want to read *forwards* from it, so it earns more space below than above.
    pub fn reveal(&mut self, line: usize) {
        self.scroll = line.saturating_sub(self.viewport / 3);
        self.clamp();
    }

    /// Re-lay out at a new width, keeping the reader's place.
    ///
    /// Restoring the raw line offset would be wrong: a narrower terminal makes
    /// every paragraph taller, so line 400 is a different part of the document
    /// before and after. Anchoring to the nearest heading and preserving the
    /// offset *within that section* keeps you where you were reading.
    pub fn relayout(&mut self, width: usize) {
        if width == self.rendered.width || width == 0 {
            return;
        }

        let anchored = self
            .rendered
            .anchors
            .iter()
            .enumerate()
            .rev()
            .find(|(_, a)| a.line <= self.scroll)
            .map(|(index, a)| (index, self.scroll - a.line));
        let fraction = self.scroll as f64 / self.rendered.len().max(1) as f64;

        self.rendered = self.content.layout(width, &self.theme);

        self.scroll = match anchored {
            Some((index, offset)) => match self.rendered.anchors.get(index) {
                // Do not let the preserved offset run past the following
                // heading; that would land you in the wrong section entirely.
                Some(anchor) => {
                    let next = self
                        .rendered
                        .anchors
                        .get(index + 1)
                        .map(|a| a.line)
                        .unwrap_or(self.rendered.len());
                    (anchor.line + offset).min(next.saturating_sub(1)).max(anchor.line)
                }
                None => 0,
            },
            // No heading above us (we are in the preamble): fall back to keeping
            // the same proportion of the document above the fold.
            None => (fraction * self.rendered.len() as f64) as usize,
        };
        self.clamp();

        if self.search.is_active() {
            self.search.refresh(&self.rendered);
        }
        // Popup rows carry line targets that the new layout invalidated.
        if let Some(kind) = self.popup.as_ref().map(|p| p.kind) {
            let selected = self.popup.as_ref().map(|p| p.selected).unwrap_or(0);
            self.rebuild_popup(kind);
            if let Some(popup) = self.popup.as_mut() {
                popup.jump(selected);
            }
        }
    }

    // -- search ------------------------------------------------------------

    pub fn open_search(&mut self) {
        self.restore_scroll = self.scroll;
        self.mode = Mode::SearchInput;
        self.search.query.clear();
        self.search.matches.clear();
        self.rebuild_popup(PopupKind::Search);
    }

    pub fn search_push(&mut self, c: char) {
        self.search.query.push(c);
        self.after_query_change();
    }

    pub fn search_pop(&mut self) {
        self.search.query.pop();
        self.after_query_change();
    }

    fn after_query_change(&mut self) {
        self.search.refresh(&self.rendered);
        self.rebuild_popup(PopupKind::Search);
        // Preview the first hit as you type, so the query is self-evidently
        // finding the right thing before you commit to it.
        if let Some(m) = self.search.current_match() {
            self.reveal(m.line);
        } else {
            self.scroll = self.restore_scroll;
            self.clamp();
        }
    }

    /// Move to the next/previous match with no popup open.
    pub fn step_match(&mut self, forward: bool) {
        if let Some(m) = self.search.step(forward) {
            self.reveal(m.line);
            self.message =
                Some(format!("match {} of {}", self.search.current + 1, self.search.matches.len()));
        } else if self.search.is_active() {
            self.message = Some(format!("no matches for {:?}", self.search.query));
        }
    }

    // -- popups ------------------------------------------------------------

    pub fn open_popup(&mut self, kind: PopupKind) {
        self.restore_scroll = self.scroll;
        self.rebuild_popup(kind);
        if let Some(popup) = self.popup.as_mut() {
            // Open the outline and link list at whatever is already on screen,
            // not at the top of the document.
            if kind.previews() {
                let scroll = self.scroll;
                if let Some(index) = popup.rows.iter().rposition(|r| r.target <= scroll) {
                    popup.jump(index);
                }
            }
        }
        self.mode = Mode::Popup;
    }

    pub fn popup_step(&mut self, forward: bool) {
        let Some(popup) = self.popup.as_mut() else { return };
        popup.step(forward);
        let (previews, target, selected) = (popup.kind.previews(), popup.target(), popup.selected);

        // Scroll the document behind the popup so the selection is previewed in
        // context; this is what makes the list a way to *read* rather than a menu.
        if previews {
            if let Some(line) = target {
                self.reveal(line);
            }
            if self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Search) {
                self.search.current = selected.min(self.search.matches.len().saturating_sub(1));
            }
        }
    }

    /// Accept the selection and close the popup.
    pub fn popup_commit(&mut self) {
        if let Some(popup) = self.popup.take() {
            if let Some(line) = popup.target() {
                if popup.kind.previews() {
                    self.reveal(line);
                }
            }
            if popup.kind == PopupKind::Search {
                self.search.focus_near(self.scroll);
                self.message = if self.search.matches.is_empty() {
                    Some(format!("no matches for {:?}", self.search.query))
                } else {
                    Some(format!("{} matches", self.search.matches.len()))
                };
            }
        }
        self.mode = Mode::Normal;
    }

    /// Abandon the popup and go back to where the reader was.
    pub fn popup_cancel(&mut self) {
        let was_search = self.popup.as_ref().map(|p| p.kind) == Some(PopupKind::Search);
        self.popup = None;
        self.mode = Mode::Normal;
        self.scroll = self.restore_scroll;
        self.clamp();
        if was_search {
            self.search.clear();
        }
    }

    pub fn rebuild_popup(&mut self, kind: PopupKind) {
        let popup = match kind {
            PopupKind::Search => self.search_popup(),
            PopupKind::Links => self.links_popup(),
            PopupKind::Outline => self.outline_popup(),
            PopupKind::Help => help_popup(&self.theme, self.sidebar.is_some()),
        };
        self.popup = Some(popup);
    }

    fn search_popup(&self) -> Popup {
        let rows = self
            .search
            .matches
            .iter()
            .map(|m| {
                let mut spans =
                    vec![Span::styled(format!("{:>5}  ", m.line + 1), self.theme.popup_dim)];

                // The breadcrumb is the point: in an API reference you need to
                // know which section a hit is in before deciding to jump to it.
                if let Some(anchor) = self.rendered.heading_at(m.line) {
                    spans.push(Span::styled(
                        format!("{}  ", truncate(&anchor.text, 28)),
                        self.theme.popup_title,
                    ));
                }
                spans.extend(snippet(&self.rendered.plain[m.line], m.start, m.end, &self.theme));
                PopupRow { lines: vec![Line::from(spans)], target: m.line }
            })
            .collect();

        Popup::new(PopupKind::Search, format!("{} matches", self.search.matches.len()), rows)
    }

    fn links_popup(&self) -> Popup {
        let rows = self
            .rendered
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| PopupRow {
                lines: vec![
                    Line::from(vec![
                        Span::styled(format!("{:>4}  ", index + 1), self.theme.popup_dim),
                        Span::styled(link.text.clone(), self.theme.link),
                    ]),
                    Line::from(vec![
                        Span::raw("      "),
                        Span::styled(link.url.clone(), self.theme.popup_dim),
                    ]),
                ],
                target: link.line,
            })
            .collect();
        Popup::new(PopupKind::Links, "Links", rows)
    }

    fn outline_popup(&self) -> Popup {
        let rows = self
            .rendered
            .anchors
            .iter()
            .map(|anchor| {
                let indent = "  ".repeat(anchor.level.saturating_sub(1) as usize);
                PopupRow {
                    lines: vec![Line::from(vec![
                        Span::styled(format!("{:>5}  ", anchor.line + 1), self.theme.popup_dim),
                        Span::styled(
                            format!("{indent}{}", anchor.text),
                            self.theme.heading(anchor.level),
                        ),
                    ])],
                    target: anchor.line,
                }
            })
            .collect();
        Popup::new(PopupKind::Outline, "Outline", rows)
    }
}

fn help_popup(theme: &Theme, browsing: bool) -> Popup {
    const READING: &[(&str, &str)] = &[
        ("j / ↓", "down one line"),
        ("k / ↑", "up one line"),
        ("d / u", "half page down / up"),
        ("f / b, PgDn / PgUp", "page down / up"),
        ("g / G", "top / bottom"),
        ("", ""),
        ("/", "search — results list opens as you type"),
        ("n / N", "next / previous match"),
        ("Esc", "clear search"),
        ("", ""),
        ("t", "outline of headings"),
        ("l", "list of links"),
    ];

    /// Shown only when there is a browser to drive; listing keys that do
    /// nothing is worse than not listing them.
    const FILES: &[(&str, &str)] = &[
        ("Tab", "switch between the tree and the document"),
        ("Ctrl-B", "show or hide the tree"),
        ("j / k, ↓ / ↑", "move the selection, previewing as you go"),
        ("l / h, → / ←", "expand / collapse a directory"),
        ("Enter", "open the selection"),
        ("/", "filter the tree by name"),
        (".", "show or hide dotfiles"),
    ];

    const LISTS: &[(&str, &str)] = &[
        ("↑ ↓", "move selection, previewing in place"),
        ("Enter", "jump to selection"),
        ("Esc", "cancel and return"),
        ("", ""),
        ("?", "this help"),
        ("q", "quit"),
    ];

    let mut rows = Vec::new();
    let section = |title: &str, keys: &[(&str, &str)], rows: &mut Vec<PopupRow>| {
        if !rows.is_empty() {
            rows.push(blank_row());
        }
        rows.push(PopupRow {
            lines: vec![Line::from(Span::styled(format!("  {title}"), theme.heading(2)))],
            target: 0,
        });
        for (key, description) in keys {
            rows.push(PopupRow {
                lines: vec![Line::from(vec![
                    Span::styled(format!("  {key:<20}"), theme.popup_title),
                    Span::styled(description.to_string(), theme.text),
                ])],
                target: 0,
            });
        }
    };

    section("Reading", READING, &mut rows);
    if browsing {
        section("Files", FILES, &mut rows);
    }
    section("Lists", LISTS, &mut rows);

    Popup::new(PopupKind::Help, "Keys", rows)
}

fn blank_row() -> PopupRow {
    PopupRow { lines: vec![Line::default()], target: 0 }
}

fn truncate(s: &str, width: usize) -> String {
    if text_width(s) <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = text_width(&c.to_string());
        if used + w > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Build a context snippet with the match emphasised.
///
/// The window slides to keep the match visible: a hit 200 columns into a line is
/// useless if the row only ever shows the first 60.
fn snippet(text: &str, start: usize, end: usize, theme: &Theme) -> Vec<Span<'static>> {
    const BEFORE: usize = 24;
    const AFTER: usize = 60;

    // Code lines are padded with trailing spaces so their background paints as a
    // solid block. Measuring against the padded length would append a "there is
    // more" ellipsis to lines that are in fact complete.
    let limit = text.trim_end().len().max(end);
    let text = &text[..limit];

    let lead_start =
        text[..start].char_indices().rev().take(BEFORE).last().map(|(i, _)| i).unwrap_or(start);
    let tail_end = text[end..]
        .char_indices()
        .take(AFTER)
        .last()
        .map(|(i, c)| end + i + c.len_utf8())
        .unwrap_or(end);

    let mut spans = Vec::new();
    if lead_start > 0 {
        spans.push(Span::styled("…", theme.popup_dim));
    }
    let lead = text[lead_start..start].trim_start();
    if !lead.is_empty() {
        spans.push(Span::styled(lead.to_string(), Style::new()));
    }
    spans.push(Span::styled(text[start..end].to_string(), theme.search_match));
    let tail = text[end..tail_end].trim_end();
    if !tail.is_empty() {
        spans.push(Span::styled(tail.to_string(), Style::new()));
    }
    if tail_end < text.len() {
        spans.push(Span::styled("…", theme.popup_dim));
    }
    spans
}
