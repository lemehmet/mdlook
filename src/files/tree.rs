//! The file browser's tree.
//!
//! The tree is stored as nodes but *used* as a flat list of visible rows, which
//! is rebuilt whenever something changes what is visible. That trade is
//! deliberate: every operation the reader performs — move, page, jump to the
//! end, scroll into view — is an index into a flat list, and duplicating that
//! arithmetic over a recursive structure is where bugs of this kind live.
//!
//! Directories are read when they are first expanded and remembered afterwards,
//! so opening a large checkout costs one directory read rather than a walk.
//!
//! Two things this deliberately does not do. It does not follow symbolic links
//! to directories — that is how a browser ends up walking a cycle — so a symlink
//! is a leaf whatever it points at. And it does not consult `.gitignore`; hiding
//! files the reader can see in their shell would be its own kind of confusing.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::layout::wrap::sanitize;

/// Most entries read from a single directory.
///
/// A directory with more entries than this is a data store, not something to
/// scroll through, and reading all of it would stall the interface. What is
/// dropped is reported in the tree rather than silently omitted.
const MAX_CHILDREN: usize = 5000;

/// One node in the tree. Children are `None` until the node is first expanded.
#[derive(Debug)]
struct Node {
    path: PathBuf,
    name: String,
    is_dir: bool,
    /// A symlink, whatever it points at. Shown, never descended into.
    is_link: bool,
    expanded: bool,
    children: Option<Vec<Node>>,
    /// Set when the directory could not be read, so the reader is told rather
    /// than shown an empty directory that is not empty.
    error: Option<String>,
    /// Entries beyond [`MAX_CHILDREN`] that were not read.
    dropped: usize,
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_link: bool,
    pub expanded: bool,
    /// A row that is a message rather than an entry: an unreadable directory or
    /// a truncated listing. It has no path worth previewing.
    pub note: Option<String>,
}

impl Row {
    pub fn is_note(&self) -> bool {
        self.note.is_some()
    }
}

pub struct Tree {
    root: Node,
    /// Flattened visible rows, rebuilt by [`Tree::refresh`].
    pub rows: Vec<Row>,
    pub selected: usize,
    /// Index of the first row drawn, maintained by [`Tree::scroll_into_view`].
    pub offset: usize,
    /// Whether dotfiles are listed.
    pub hidden: bool,
    /// Live filter over the visible rows. See [`Tree::set_filter`].
    pub filter: String,
}

impl Tree {
    /// Build a tree rooted at `root`, with the root's own children expanded.
    pub fn new(root: &Path, hidden: bool) -> Self {
        let name = display_name(root);
        let mut node = Node {
            path: root.to_path_buf(),
            name,
            is_dir: true,
            is_link: false,
            expanded: true,
            children: None,
            error: None,
            dropped: 0,
        };
        node.load(hidden);

        let mut tree = Self {
            root: node,
            rows: Vec::new(),
            selected: 0,
            offset: 0,
            hidden,
            filter: String::new(),
        };
        tree.refresh();
        tree
    }

    pub fn root(&self) -> &Path {
        &self.root.path
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The entry under the cursor, if it is a real entry rather than a note.
    pub fn selection(&self) -> Option<&Row> {
        self.rows.get(self.selected).filter(|row| !row.is_note())
    }

    /// The path to preview, which is a file rather than a directory.
    pub fn selected_file(&self) -> Option<&Path> {
        self.selection().filter(|row| !row.is_dir).map(|row| row.path.as_path())
    }

    // -- moving ------------------------------------------------------------

    pub fn step(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
    }

    pub fn to_top(&mut self) {
        self.selected = 0;
    }

    pub fn to_bottom(&mut self) {
        self.selected = self.rows.len().saturating_sub(1);
    }

    /// Keep the selection on screen, given how many rows fit.
    pub fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + height {
            self.offset = self.selected + 1 - height;
        }
        let max = self.rows.len().saturating_sub(height);
        self.offset = self.offset.min(max);
    }

    // -- expanding ---------------------------------------------------------

    /// Expand the selected directory. Returns whether anything changed.
    pub fn expand(&mut self) -> bool {
        let Some(path) =
            self.selection().filter(|r| r.is_dir && !r.expanded).map(|r| r.path.clone())
        else {
            return false;
        };
        let hidden = self.hidden;
        if let Some(node) = self.root.find_mut(&path) {
            node.expanded = true;
            node.load(hidden);
        }
        self.refresh_keeping(&path);
        true
    }

    /// Collapse the selected directory, or move to the parent when there is
    /// nothing to collapse. That second behaviour is what makes `h` usable for
    /// walking back out of a tree rather than a key that sometimes does nothing.
    pub fn collapse(&mut self) -> bool {
        let Some(row) = self.rows.get(self.selected).cloned() else { return false };

        if row.is_dir && row.expanded {
            if let Some(node) = self.root.find_mut(&row.path) {
                node.expanded = false;
            }
            self.refresh_keeping(&row.path);
            return true;
        }

        // Walk up: find the enclosing directory row above this one.
        if row.depth > 0 {
            if let Some(index) =
                self.rows[..self.selected].iter().rposition(|r| r.depth < row.depth && r.is_dir)
            {
                self.selected = index;
                return true;
            }
        }
        false
    }

    /// Expand or collapse the selection, whichever applies.
    pub fn toggle(&mut self) -> bool {
        match self.rows.get(self.selected).map(|r| (r.is_dir, r.expanded)) {
            Some((true, true)) => self.collapse(),
            Some((true, false)) => self.expand(),
            _ => false,
        }
    }

    // -- options -----------------------------------------------------------

    /// Show or hide dotfiles.
    ///
    /// Every directory already read is re-read, because what was filtered out
    /// was never stored; keeping both sets in memory to save a `readdir` on a
    /// keypress nobody presses twice is the wrong trade.
    pub fn toggle_hidden(&mut self) {
        self.hidden = !self.hidden;
        let keep = self.selection().map(|r| r.path.clone());
        let hidden = self.hidden;
        self.root.reload(hidden);
        match keep {
            Some(path) => self.refresh_keeping(&path),
            None => self.refresh(),
        }
    }

    /// Narrow the visible rows to those matching `filter`.
    ///
    /// The filter applies to what is *visible*: rows in collapsed directories
    /// are not searched, because finding them would mean walking the whole tree
    /// on every keystroke. A directory is kept when it or any of its visible
    /// descendants match, so the path to a hit stays intact.
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        let keep = self.selection().map(|r| r.path.clone());
        match keep {
            Some(path) => self.refresh_keeping(&path),
            None => self.refresh(),
        }
    }

    pub fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.set_filter(String::new());
        }
    }

    // -- revealing ---------------------------------------------------------

    /// Expand everything down to `path` and put the cursor on it.
    ///
    /// This is what makes `mdlook --browse some/deep/file.md` open with the file
    /// both selected and visible rather than at the top of the tree.
    pub fn reveal(&mut self, path: &Path) {
        let Ok(relative) = path.strip_prefix(&self.root.path) else { return };

        let hidden = self.hidden;
        let mut walked = self.root.path.clone();
        let mut node = &mut self.root;

        for component in relative.components() {
            walked.push(component);
            node.expanded = true;
            node.load(hidden);
            let Some(next) = node
                .children
                .as_mut()
                .and_then(|children| children.iter_mut().find(|c| c.path == walked))
            else {
                break;
            };
            node = next;
        }

        self.refresh();
        if let Some(index) = self.rows.iter().position(|r| r.path == path) {
            self.selected = index;
        }
    }

    // -- flattening --------------------------------------------------------

    fn refresh(&mut self) {
        let mut rows = Vec::new();
        flatten(&self.root, 0, &mut rows);
        if !self.filter.is_empty() {
            rows = apply_filter(rows, &self.filter);
        }
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    /// Rebuild the rows, then put the cursor back on `path` if it is still
    /// visible. Rebuilding renumbers everything, so an index kept across it
    /// would silently point at a different file.
    fn refresh_keeping(&mut self, path: &Path) {
        self.refresh();
        if let Some(index) = self.rows.iter().position(|r| r.path == path) {
            self.selected = index;
        } else if let Some(index) = self.rows.iter().rposition(|r| path.starts_with(&r.path)) {
            // The row is gone because an ancestor collapsed; settle on it.
            self.selected = index;
        } else {
            self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        }
    }
}

fn flatten(node: &Node, depth: usize, rows: &mut Vec<Row>) {
    // The root itself is not a row: it is the header above the list, and giving
    // it a row would waste a line and offer a "collapse everything" nobody wants.
    if depth > 0 {
        rows.push(Row {
            path: node.path.clone(),
            name: node.name.clone(),
            depth: depth - 1,
            is_dir: node.is_dir,
            is_link: node.is_link,
            expanded: node.expanded,
            note: None,
        });
    }

    if !node.expanded {
        return;
    }

    if let Some(error) = &node.error {
        rows.push(note_row(node, depth, error.clone()));
        return;
    }

    for child in node.children.as_deref().unwrap_or_default() {
        flatten(child, depth + 1, rows);
    }

    if node.dropped > 0 {
        rows.push(note_row(node, depth, format!("… {} more not listed", node.dropped)));
    }
}

fn note_row(node: &Node, depth: usize, note: String) -> Row {
    Row {
        path: node.path.clone(),
        name: note.clone(),
        depth,
        is_dir: false,
        is_link: false,
        expanded: false,
        note: Some(note),
    }
}

/// Keep matching rows and every ancestor that leads to one.
fn apply_filter(rows: Vec<Row>, filter: &str) -> Vec<Row> {
    let needle = filter.to_lowercase();
    let matches: Vec<bool> =
        rows.iter().map(|r| !r.is_note() && r.name.to_lowercase().contains(&needle)).collect();

    // Walk backwards so a directory learns whether anything below it survived
    // before we decide its own fate.
    let mut keep = vec![false; rows.len()];
    let mut wanted_depth: Option<usize> = None;
    for index in (0..rows.len()).rev() {
        let row = &rows[index];
        let leads_to_match = wanted_depth.is_some_and(|depth| row.depth < depth);
        if matches[index] || leads_to_match {
            keep[index] = true;
            wanted_depth = Some(row.depth);
        }
    }

    rows.into_iter().zip(keep).filter_map(|(row, keep)| keep.then_some(row)).collect()
}

impl Node {
    /// Read this directory's children, if it is a directory and they are not
    /// already known.
    fn load(&mut self, hidden: bool) {
        if !self.is_dir || self.children.is_some() {
            return;
        }
        match read_dir(&self.path, hidden) {
            Ok((children, dropped)) => {
                self.children = Some(children);
                self.dropped = dropped;
                self.error = None;
            }
            Err(error) => {
                self.children = Some(Vec::new());
                self.dropped = 0;
                self.error = Some(error);
            }
        }
    }

    /// Drop and re-read every directory already loaded, preserving which ones
    /// were expanded.
    fn reload(&mut self, hidden: bool) {
        let expanded: Vec<PathBuf> = self
            .children
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|c| c.expanded)
            .map(|c| c.path.clone())
            .collect();

        self.children = None;
        self.load(hidden);

        for child in self.children.iter_mut().flatten() {
            if expanded.contains(&child.path) {
                child.expanded = true;
                child.reload(hidden);
            }
        }
    }

    fn find_mut(&mut self, path: &Path) -> Option<&mut Node> {
        if self.path == path {
            return Some(self);
        }
        if !path.starts_with(&self.path) {
            return None;
        }
        self.children.as_mut()?.iter_mut().find_map(|child| child.find_mut(path))
    }
}

fn read_dir(path: &Path, hidden: bool) -> Result<(Vec<Node>, usize), String> {
    let entries = std::fs::read_dir(path).map_err(|e| e.to_string())?;

    let mut nodes = Vec::new();
    let mut dropped = 0;

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue;
        }
        if nodes.len() >= MAX_CHILDREN {
            dropped += 1;
            continue;
        }

        // `symlink_metadata` rather than `metadata`: a symlink should report as
        // itself, so a link to a directory becomes a leaf and the tree cannot be
        // walked into a cycle.
        let Ok(metadata) = entry.metadata() else { continue };
        let is_link = metadata.is_symlink();

        nodes.push(Node {
            path: entry.path(),
            name: name.chars().map(sanitize).collect(),
            is_dir: metadata.is_dir(),
            is_link,
            expanded: false,
            children: None,
            error: None,
            dropped: 0,
        });
    }

    // `read_dir` yields entries in whatever order the filesystem stores them,
    // which differs between machines and even between runs. Sorting explicitly
    // is what makes the browser show the same thing twice.
    nodes.sort_by(|a, b| match b.is_dir.cmp(&a.is_dir) {
        Ordering::Equal => match a.name.to_lowercase().cmp(&b.name.to_lowercase()) {
            // Case-insensitive first so `Cargo.toml` and `cargo.lock` sort
            // together, then exact bytes so the order is total and stable.
            Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        },
        other => other,
    });

    Ok((nodes, dropped))
}

/// What to call the root in the header.
fn display_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    name.chars().map(sanitize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture directory, removed when the guard drops.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("mdlook-tree-{}-{label}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("docs").join("deep")).unwrap();
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("README.md"), "# hi\n").unwrap();
            std::fs::write(root.join(".hidden"), "x").unwrap();
            std::fs::write(root.join("zebra.txt"), "x").unwrap();
            std::fs::write(root.join("docs").join("guide.md"), "# guide\n").unwrap();
            std::fs::write(root.join("docs").join("deep").join("buried.md"), "# deep\n").unwrap();
            std::fs::write(root.join("src").join("main.rs"), "fn main() {}\n").unwrap();
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(tree: &Tree) -> Vec<String> {
        tree.rows.iter().map(|r| format!("{}{}", "  ".repeat(r.depth), r.name)).collect()
    }

    #[test]
    fn directories_come_first_and_dotfiles_are_hidden_by_default() {
        let fixture = Fixture::new("order");
        let tree = Tree::new(&fixture.0, false);
        assert_eq!(names(&tree), vec!["docs", "src", "README.md", "zebra.txt"]);
    }

    #[test]
    fn the_order_is_the_same_every_time() {
        // `read_dir` order is filesystem-dependent, which is exactly the kind of
        // thing that renders differently on two machines if left unsorted.
        let fixture = Fixture::new("stable");
        let first = names(&Tree::new(&fixture.0, false));
        for _ in 0..5 {
            assert_eq!(names(&Tree::new(&fixture.0, false)), first);
        }
    }

    #[test]
    fn dotfiles_appear_when_asked_for() {
        let fixture = Fixture::new("hidden");
        let mut tree = Tree::new(&fixture.0, false);
        assert!(!names(&tree).contains(&".hidden".to_string()));
        tree.toggle_hidden();
        assert!(names(&tree).contains(&".hidden".to_string()));
        tree.toggle_hidden();
        assert!(!names(&tree).contains(&".hidden".to_string()));
    }

    #[test]
    fn expanding_a_directory_inserts_its_children_below_it() {
        let fixture = Fixture::new("expand");
        let mut tree = Tree::new(&fixture.0, false);
        assert!(tree.expand(), "docs should expand");
        assert_eq!(
            names(&tree),
            vec!["docs", "  deep", "  guide.md", "src", "README.md", "zebra.txt"]
        );
        assert!(tree.collapse());
        assert_eq!(names(&tree), vec!["docs", "src", "README.md", "zebra.txt"]);
    }

    #[test]
    fn collapsing_a_file_walks_out_to_its_parent() {
        let fixture = Fixture::new("walkout");
        let mut tree = Tree::new(&fixture.0, false);
        tree.expand();
        tree.step(2); // docs/guide.md
        assert_eq!(tree.selection().unwrap().name, "guide.md");
        assert!(tree.collapse());
        assert_eq!(tree.selection().unwrap().name, "docs", "should walk up, not do nothing");
    }

    #[test]
    fn the_selection_follows_the_file_across_a_rebuild() {
        // Row indices are renumbered whenever anything expands, so a cursor kept
        // as a number would quietly land on a different file.
        let fixture = Fixture::new("keep");
        let mut tree = Tree::new(&fixture.0, false);
        tree.step(1); // src
        let before = tree.selection().unwrap().path.clone();
        tree.selected = 0;
        tree.expand(); // expand docs, which pushes src down
        tree.selected = tree.rows.iter().position(|r| r.path == before).unwrap();
        assert_eq!(tree.selection().unwrap().name, "src");
    }

    #[test]
    fn revealing_a_deep_file_expands_the_path_to_it() {
        let fixture = Fixture::new("reveal");
        let mut tree = Tree::new(&fixture.0, false);
        let target = fixture.0.join("docs").join("deep").join("buried.md");
        tree.reveal(&target);
        assert_eq!(tree.selection().unwrap().path, target);
        assert_eq!(tree.selected_file(), Some(target.as_path()));
        assert!(names(&tree).iter().any(|n| n.trim() == "buried.md"));
    }

    #[test]
    fn revealing_a_path_outside_the_root_does_nothing() {
        let fixture = Fixture::new("outside");
        let mut tree = Tree::new(&fixture.0, false);
        let before = names(&tree);
        tree.reveal(Path::new("/etc/hosts"));
        assert_eq!(names(&tree), before);
    }

    #[test]
    fn a_filter_keeps_the_directories_that_lead_to_a_match() {
        let fixture = Fixture::new("filter");
        let mut tree = Tree::new(&fixture.0, false);
        tree.expand(); // docs
        tree.set_filter("guide".into());
        assert_eq!(names(&tree), vec!["docs", "  guide.md"], "the path to a hit must survive");

        tree.set_filter("zzz".into());
        assert!(tree.rows.is_empty());

        tree.clear_filter();
        assert_eq!(names(&tree).len(), 6);
    }

    #[test]
    fn a_filter_is_case_insensitive() {
        let fixture = Fixture::new("case");
        let mut tree = Tree::new(&fixture.0, false);
        tree.set_filter("readme".into());
        assert_eq!(names(&tree), vec!["README.md"]);
    }

    #[test]
    fn a_directory_is_not_offered_as_a_file_to_preview() {
        let fixture = Fixture::new("nodir");
        let tree = Tree::new(&fixture.0, false);
        assert_eq!(tree.selection().unwrap().name, "docs");
        assert_eq!(tree.selected_file(), None);
    }

    #[test]
    fn an_unreadable_root_reports_itself_instead_of_looking_empty() {
        let tree = Tree::new(Path::new("/nonexistent/nowhere"), false);
        assert_eq!(tree.rows.len(), 1);
        assert!(tree.rows[0].is_note());
        assert_eq!(tree.selection(), None, "a note is not something to preview");
    }

    #[test]
    fn moving_is_clamped_at_both_ends() {
        let fixture = Fixture::new("clamp");
        let mut tree = Tree::new(&fixture.0, false);
        tree.step(-10);
        assert_eq!(tree.selected, 0);
        tree.step(1000);
        assert_eq!(tree.selected, tree.rows.len() - 1);
        tree.to_top();
        assert_eq!(tree.selected, 0);
        tree.to_bottom();
        assert_eq!(tree.selected, tree.rows.len() - 1);
    }

    #[test]
    fn scrolling_keeps_the_selection_on_screen() {
        let fixture = Fixture::new("scroll");
        let mut tree = Tree::new(&fixture.0, false);
        tree.to_bottom();
        tree.scroll_into_view(2);
        assert!(tree.selected >= tree.offset && tree.selected < tree.offset + 2);
        tree.to_top();
        tree.scroll_into_view(2);
        assert_eq!(tree.offset, 0);
    }

    #[test]
    fn a_symlinked_directory_is_a_leaf_rather_than_a_way_into_a_cycle() {
        #[cfg(unix)]
        {
            let fixture = Fixture::new("symlink");
            // A link pointing at its own parent: following it would recurse.
            std::os::unix::fs::symlink(&fixture.0, fixture.0.join("loop")).unwrap();
            let mut tree = Tree::new(&fixture.0, false);
            let index = tree.rows.iter().position(|r| r.name == "loop").unwrap();
            tree.selected = index;
            assert!(tree.rows[index].is_link);
            assert!(!tree.rows[index].is_dir, "a symlink must not present as a directory");
            assert!(!tree.expand(), "there should be nothing to expand into");
        }
    }
}
