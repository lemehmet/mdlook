# Changelog

Notable changes, newest first. This project follows
[semantic versioning](https://semver.org); while the major version is 0,
breaking changes may land in a minor release.

## v0.3.1 — 2026-08-26

### PDFs show their text

- A PDF now opens as its extracted plain text in the numbered, searchable
  whole-file view — no layout reconstruction, just the words. Extraction is
  pure Rust, so nothing needs `pdftotext` installed. A scanned PDF with no
  text layer says so; a corrupt one becomes a note rather than a crash, and
  extraction is hardened so a hostile file can neither panic the viewer nor
  write over the screen.
- In the browser a PDF rides the same debounce and cache as an image: the
  pane answers instantly and extraction runs only once the selection has
  rested on the file. Files over 32 MiB are described instead of extracted.
- The `%PDF-` signature is checked before the text/binary split, so an
  all-ASCII PDF is extracted rather than shown as its own raw source.

## v0.3.0 — 2026-08-26

### Images render as coloured blocks

- A PNG, JPEG, GIF or WebP now draws in the pane as block characters — enough
  shape and colour to know what the image is without leaving the terminal —
  instead of being identified as a binary. Works everywhere mdlook works, in
  any terminal, and `--plain` writes the same picture to a pipe.
- `m` cycles the subpixel grid: half → quadrant → sextant → octant. Half-blocks
  are the lossless default; the finer grids sharpen edges at the cost of colour
  smoothness and need a font that ships the glyphs (sextants are Unicode 13,
  octants Unicode 16), which no terminal reports — the cycle key is the
  capability test, and the status bar names the mode and effective resolution.
- Every mode draws the image the same size on screen; the finer grids spend
  their extra subpixels on detail. Nothing is upscaled past the file's own
  resolution, and the picture fits the pane, both dimensions.
- In the browser, an image decodes only after the selection has rested on it
  for a moment, so holding an arrow key through a directory of photographs
  costs nothing; the last few decodes are cached. Decoding is bounded by file
  size and by pixel count read from the header before anything is allocated.
- `--no-images`, or `enabled = false` under `[images]` in the config, keeps
  images as identified binaries. `block_mode` sets the starting grid. The mono
  theme never renders images, because `NO_COLOR` means no colour.

## v0.2.1 — 2026-08-24

### Changed

- Text now fills the pane it is drawn in instead of stopping at 100 columns.
  The cap left a wide terminal mostly empty, and more so beside the file
  browser, where the pane is already narrower than the frame. Width is
  recomputed every frame, so resizing the window and hiding the browser both
  re-flow. `--width N`, or `width = N` in the config, still sets a fixed
  measure, and either is bounded by the pane so it cannot overflow it.
- `--plain` to a terminal follows the terminal's width for the same reason.
  Piped output is unchanged at 80 columns, since a pipe has no width to ask
  about.

### Deprecated

- `render::tui::DEFAULT_MAX_WIDTH`. Nothing applies it any more; `--width` or
  the config key says the same thing and says it where the reader can see it.

## v0.2.0 — 2026-08-24

### Files that are not markdown

- Every file mdlook opens is now classified before it is rendered. Markdown is
  decided by extension; anything else that decodes as text is shown in a
  numbered, syntax-highlighted whole-file view; a binary is identified rather
  than dumped. This applies to a named file too, so `mdlook main.rs` now shows
  highlighted Rust instead of parsing it as markdown. `mdlook README.md` is
  unchanged.
- Syntax for a whole file is resolved by name, then by extension, then by
  shebang — so `Makefile`, `Dockerfile` and an extensionless script with
  `#!/usr/bin/env bash` all highlight. Files over 256 KB are shown unhighlighted.
- Binaries are identified in-process against a built-in table of magic numbers,
  including decoded ELF, PE and Mach-O headers, so the architecture is reported
  without running `file(1)`.

### File browser

- `mdlook <dir>` opens a tree on the left and previews the selection on the
  right. `--browse` does the same alongside a named file, with that file
  revealed and selected, and on its own browses the working directory rather
  than waiting on standard input. Without the flag, and without a config asking
  for it, nothing changes.
- `Ctrl-B` shows and hides the tree; `Tab` moves focus between the panes; `/`
  filters the tree by name; `.` toggles dotfiles. A live search survives moving
  to another file, so a query can be carried through a directory.
- Read-only by design: no rename, delete or edit. Directory symlinks are shown
  but never followed, listings are sorted explicitly so two machines agree, and
  only regular files are opened — reading a FIFO on a cursor move would hang.

### Config

- An optional `config.toml` at `$XDG_CONFIG_HOME/mdlook/` (or `~/.config/mdlook/`,
  or `%APPDATA%\mdlook\` on Windows), read but never written. Sets `browse`,
  `theme`, `width`, and a `[browser]` section. `--config <PATH>` and `--no-config`
  override where it comes from; a flag always beats the file. An unrecognised key
  is an error rather than silently ignored.
- `browser.probe_command` opts into identifying binaries with an external command
  such as `file --brief`. Unset by default, and unset means mdlook starts no
  subprocess at all.

### Fixed

- Text prompts no longer insert a letter when a control chord is pressed:
  a terminal reports Ctrl-J as `Char('j')` with a modifier, and the search prompt
  was typing the `j`.

### Changed

- `App::new` takes a `Content` rather than a `Document`. `Document` implements
  `Into<Content>`, so `parse(source).into()` is the migration.
- `RenderedDoc` gained `content_offset`, the byte offset at which each line's
  searchable text begins. It is zero for markdown; the whole-file view uses it so
  that searching for `42` finds the number in the code, not the line number in
  the gutter.

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
