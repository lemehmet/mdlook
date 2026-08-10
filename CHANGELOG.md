# Changelog

Notable changes, newest first. This project follows
[semantic versioning](https://semver.org); while the major version is 0,
breaking changes may land in a minor release.

## v0.1.0 — 2026-08-10

First release.

### Reading

- **Paragraphs reflow to the terminal width.** A line ending inside a paragraph
  is a *soft* break that renders as a space; only two trailing spaces or a
  backslash make a hard break. Source hard-wrapped at 80 columns is rejoined and
  re-wrapped, so you read at your width rather than the author's.
- The joining space is dropped between CJK characters, which are written without
  inter-word spaces — but not between emoji, which are double-width without
  being a spaceless script.
- Headings render without their markers at every level, with inline emphasis,
  code and links inside them intact.
- GFM tables with content-derived column widths, shrunk proportionally when over
  budget, cells wrapped rather than truncated.
- Tight and loose lists keep their own spacing, nested lists change glyph with
  depth, ordered lists keep their start number, and task lists render as
  checkboxes rather than a bullet plus a checkbox.
- Blockquotes, footnotes, horizontal rules, and raw HTML shown literally rather
  than interpreted.

### Search

- `/` searches the *rendered* text, so a heading written as ``## `fetch_user()` ``
  is found by typing `fetch_user` rather than the backticks you never see.
  Smart case: an all-lowercase query is case-insensitive, any uppercase makes it
  sensitive.
- Matches appear as a navigable list, each row carrying its line number, the
  enclosing heading, and a context snippet with the hit highlighted.
- Moving through the list scrolls the document behind it, and the popup places
  itself in the half of the screen the match is not in. `Enter` commits, `Esc`
  returns you exactly where you started.
- The same list backs the heading outline (`t`), the link list with URLs (`l`),
  and help (`?`).

### Highlighting

- syntect with bat's extended syntax set, covering 47 common fence tags
  including the ones syntect's bundled syntaxes lack (TypeScript, Kotlin, Swift,
  TOML, Dockerfile, Zig, Nix, C#) and the aliases GitHub accepts but syntax
  definitions do not declare (`csharp`, `shell`, `console`, `golang`).
- An explicit but unrecognised tag renders plain rather than being guessed at.
  Content sniffing applies only to fences with no tag at all.

### Output

- Reproducible by construction: `layout(document, width, theme)` depends on
  nothing but its arguments — no clock, no environment, no locale, and no
  `HashMap` iteration on any path that affects output. The theme is an explicit
  argument rather than something sniffed from the terminal.
- `--plain` writes styled ANSI for pipelines, implied when stdout is not a
  terminal. `--theme dark|light|mono`, `--width`, `--no-color`, and `NO_COLOR`.

### Safety

- Control characters from a document are neutralised into their visible Unicode
  Control Picture at the single point where any line is appended, so a hostile
  README cannot repaint your terminal or rewrite its window title. Link URLs and
  the status-bar filename are sanitised separately, as they do not pass through
  that point.
- No `unsafe`, no network access, no file writes, no subprocesses.
