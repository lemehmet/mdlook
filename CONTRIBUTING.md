# Contributing to mdlook

Thanks for your interest — contributions and bug reports are both welcome. A
document that mdlook renders badly is worth as much as a patch, and attaching the
source file is usually the whole report.

## Dev setup

Requirements: Rust 1.88 or newer. Nothing else — no C toolchain and no system
libraries. (syntect is built with `regex-fancy` rather than its default `onig`
specifically to keep it that way.)

```sh
cargo build
cargo test
cargo clippy --all-targets --all-features
cargo fmt --all
```

To see the viewer's drawing layer without a terminal — useful in CI, and for
diffing a rendering change:

```sh
python3 tests/tui_capture.py -- ./target/debug/mdlook README.md
python3 tests/tui_capture.py / u s e r -- ./target/debug/mdlook README.md
```

Each bare argument before `--` is one keystroke.

## The checks

All of these run in CI on Linux and macOS, with warnings denied:

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features`
- `cargo test --all-features`
- `cargo build` on the minimum supported Rust version
- `cargo publish --dry-run`
- The viewer drawing a document, and opening its search list, in a real pty

## What the review will actually ask about

**Rendering changes need a test that pins the output.** Not that it looks better
— that a specific input produces a specific result. Most of `tests/rendering.rs`
exists because something regressed once, including two tests named after defects
found in other viewers during evaluation.

**Layout must stay pure.** `layout(document, width, theme)` may depend on nothing
but those three arguments: no clock, no environment, no locale, and no `HashMap`
iteration on any path that affects output. This is what makes the same document
render identically every run, and `tests/determinism.rs` enforces it by running
the binary twice in separate processes and comparing bytes — the only way to see
hash-order leaks, since Rust seeds `RandomState` per process.

**Nothing from a document may reach the terminal as a control character.** A
markdown file is untrusted input, and a raw escape sequence is a command rather
than text. Neutralisation happens in `Sink::push`, the one place a line is
appended. If you add a path that draws document-derived text *without* going
through it — the link list and the status-bar filename are the two that exist —
it must sanitise for itself, and say in a comment why it is exempt.

**`lines` and `plain` are one thing, not two.** They are built together and must
stay the same length, index for index. Search, scrolling and the match popup all
depend on `plain[i]` describing `lines[i]`; the moment that drifts, jumping to a
match lands on the wrong row. Add output through `Sink::push` and it holds for
free.

**Width is not a per-character property.** `⚠️` is one width-1 character plus a
variation selector, and terminals draw it in two columns. Use the helpers in
`layout::wrap` rather than summing `UnicodeWidthChar::width`; a table came out a
column short exactly once and there is a test for it now.

**Dependencies are argued for, not added.** The current list is
`pulldown-cmark`, `ratatui`, `crossterm`, `syntect`, `two-face`,
`unicode-width`, `clap` and `anyhow`. If the standard library can do it, it
should.

**Comments explain the decision, not the code.** Why this ordering, why this is
refused, what breaks if it changes. The code already says what it does.

## Architecture in one paragraph

`mdlook file.md` parses to a width-independent `Document` (`src/doc/`), where the
soft-break/hard-break distinction is resolved — the thing this project exists to
get right. `layout()` (`src/layout/`) turns that plus a width and a theme into a
`RenderedDoc`: styled lines, a plain-text mirror, heading anchors and link
references, all produced in one pass. The viewer (`src/render/tui.rs`,
`src/ui/`) scrolls that, and search runs over the mirror so a hit is always
exactly where the screen says it is. Resizing re-runs only `layout` and restores
position by nearest heading rather than raw line offset. `--plain`
(`src/render/ansi.rs`) walks the same `RenderedDoc` and writes ANSI instead.

Not every file is markdown, so `Content` (`src/content.rs`) sits in front of that
pipeline: it classifies a file (`src/files/detect.rs`) and picks a producer —
markdown, a numbered whole-file view (`src/layout/source.rs`), or a one-paragraph
identification. All three produce a `RenderedDoc`, which is why the viewer, the
search index and `--plain` needed no cases added for them. The file browser
(`src/files/tree.rs`, `src/ui/sidebar.rs`) is a flat list of visible rows over a
lazily-read tree; showing and hiding it is a width change, handled by the same
re-anchoring as a resize.

If you add a new kind of content, the bar to clear is the one every existing
producer clears: `plain[i]` must be the text of `lines[i]`, no control character
may survive into either, and the output must be a pure function of the input, the
width and the theme.

## Pull requests

1. Branch off `main`; do not push to `main` directly.
2. Keep them focused, and include tests for new behaviour.
3. `cargo fmt --all` before pushing — CI checks it.
