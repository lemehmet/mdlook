# mdlook

[![CI](https://github.com/lemehmet/mdlook/actions/workflows/ci.yml/badge.svg)](https://github.com/lemehmet/mdlook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mdlook.svg)](https://crates.io/crates/mdlook)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A terminal markdown reader for the docs you read to answer a question — API
references, READMEs, changelogs. It reflows paragraphs to *your* width, and its
search gives you a navigable list of matches rather than a blind jump.

```
mdlook README.md          # open the viewer
mdlook docs/              # open the file browser rooted there
mdlook --browse          # browse the working directory
mdlook --browse README.md # browser alongside the file, with it selected
mdlook --plain README.md  # dump styled ANSI (also implied when piped)
cat README.md | mdlook    # reads stdin, dumps rather than opening a viewer
```

## Install

```sh
cargo install mdlook
```

Or take a binary from [releases](https://github.com/lemehmet/mdlook/releases) —
Linux x86-64 and arm64, macOS Apple Silicon. Each archive ships a SHA-256
checksum:

```sh
shasum -a 256 -c mdlook-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

Building from source needs Rust 1.88 or newer and nothing else — no C toolchain,
no system libraries.

## Why this exists

Several good terminal markdown viewers already exist. Each was measured against a
probe document before this one was written:

| | reflows soft breaks | in-document search | match index | strips heading marks |
|---|---|---|---|---|
| `glow` 2.1.2 (piped) | ❌ | ❌ | ❌ | ✅ |
| `glow` 2.1.2 (TUI) | ✅ | ❌ | ❌ | ✅ |
| `md-tui` 0.10.3 | ✅ | ⚠️ prompt only | ❌ | ❌ |
| `frogmouth` | ✅ | ❌ | ❌ | ✅ |
| **`mdlook`** | ✅ | ✅ | ✅ | ✅ |

Two findings in particular:

- **`glow` cannot reflow when piped.** Its CLI path calls
  `glamour.WithPreservedNewLines()` unconditionally (`main.go:299`), so the `-n`
  flag's `false` default never takes effect and hard-wrapped source keeps the
  author's line breaks at every width. Tracked upstream as
  [glow#647](https://github.com/charmbracelet/glow/issues/647).
- **`glow`'s pager has no search.** Its complete keymap is `ui/pager.go:368-405`:
  scroll, top/bottom, copy, edit, reload, back, quit. The `/` in `glow` filters
  filenames in the file list, not text in the document.

`md-tui` is the closest existing tool — it reflows and highlights correctly — but
its search is a `less`-style prompt with no match list, it renders `## Plain H2`
literally as `## Plain H2`, and it splits `## Bold **word** here` across two
lines. Those last two are pinned as regression tests in `tests/rendering.rs`.

## The four things it gets right

**Paragraphs reflow.** Markdown treats a line ending inside a paragraph as a
*soft* break that renders as a space; only two trailing spaces or a backslash
make a hard break. Authors routinely wrap source at 80 columns, so honoring those
breaks reproduces the author's editor width instead of your terminal's. `mdlook`
joins soft breaks and re-wraps, while preserving genuine hard breaks. It also
drops the joining space between CJK characters, which are written without
inter-word spaces — but not between emoji, which are double-width without being a
spaceless script.

**Output is reproducible.** `layout(document, width, theme)` is a pure function:
no clock, no randomness, no environment, no locale, and no `HashMap` iteration on
any path that affects output. The theme is an explicit argument rather than
something sniffed from the terminal. `tests/determinism.rs` runs the binary twice
in separate processes and compares bytes, which is the only way to catch
hash-order leaks — Rust seeds `RandomState` per process, so a loop inside one
test would never see them.

**Search has an index.** `/` searches the *rendered* text, so a heading written
as ``## `fetch_user()` `` is found by typing `fetch_user`, not by typing the
backticks you never see. Results appear as a list, each row showing the line
number, the enclosing heading, and a context snippet with the match highlighted.
Moving through the list scrolls the document behind it, and the popup places
itself in the half of the screen the match is *not* in.

**Not everything in a repository is markdown.** Point it at a `.rs`, a
`Dockerfile`, a `Makefile` or an extensionless shell script and you get the same
viewer with syntax highlighting and the same searchable index — markdown is
decided by extension, never guessed at from content. Point it at a binary and it
says what the file *is* (`ELF 64-bit LSB shared object, x86-64`) rather than
spraying bytes at your terminal. Identification is a built-in table of magic
numbers, so it works the same on a machine with no `file(1)` on it.

**There is a file browser when you want one.** `mdlook docs/` opens a tree on the
left and previews whatever the cursor is on. It stays out of the way otherwise:
with no `--browse` flag and no config asking for it, `mdlook README.md` behaves
exactly as it always has. `mdlook --browse` on its own browses the working
directory — asking for the browser is asking for a session, so there is nothing
to wait on standard input for. A search stays live as you move through the tree, so
you can walk a directory looking for where something is mentioned.

**Code is highlighted, and unknown code is left alone.** 47 common fence tags
resolve, including the ones syntect's bundled syntaxes lack (TypeScript, Kotlin,
Swift, TOML, Dockerfile, Zig, Nix) and the aliases GitHub accepts but syntax
definitions do not declare (`csharp`, `shell`, `console`, `golang`). An explicit
but unrecognised tag is rendered plain rather than guessed at, because the author
told us what it is.

## Keys

| | |
|---|---|
| `j` `k` `↓` `↑` | line down / up |
| `d` `u` | half page down / up |
| `f` `b` `PgDn` `PgUp` `Space` | page down / up |
| `g` `G` `Home` `End` | top / bottom |
| `/` | search — the result list opens as you type |
| `n` `N` | next / previous match |
| `t` | outline of headings |
| `l` | list of links with their URLs |
| `?` | help |
| `Enter` | jump to the selected entry |
| `Esc` | cancel a list and return, or clear a search |
| `q` | quit |

Inside any list, `↑`/`↓` move the selection and preview it in place; `Enter`
commits and `Esc` puts you back exactly where you started. Mouse wheel scrolls.

With the file browser open:

| | |
|---|---|
| `Tab` | switch between the tree and the document |
| `Ctrl-B` | show or hide the tree |
| `j` `k` `↓` `↑` | move the selection — the document previews as you go |
| `l` `h` `→` `←` | expand / collapse a directory |
| `Enter` | open the selection |
| `/` | filter the tree by name |
| `.` | show or hide dotfiles |
| `Esc` | clear the filter, or step back to the tree |

Each pane owns its own letters, which is why `l` expands a directory in the tree
and opens the link list in the document. The mouse wheel scrolls whichever pane
it is over.

## Options

```
-p, --plain          write ANSI to stdout instead of opening the viewer
-w, --width <N>      wrap width (default: terminal width, capped at 100)
-t, --theme <NAME>   dark, light, or mono
    --no-color       disable colour (NO_COLOR is also honoured)
    --browse         open the file browser (rooted here if no path is given)
    --no-browse      go straight to the file, overriding the config
    --config <PATH>  read this config instead of the default one
    --no-config      ignore the config file entirely
```

Long lines are capped at 100 columns even on a wide terminal, because prose gets
hard to track from the end of one line to the start of the next beyond that.
Override with `--width`.

## Config

Optional, and read but never written — mdlook creates nothing. It lives at
`$XDG_CONFIG_HOME/mdlook/config.toml`, falling back to
`~/.config/mdlook/config.toml`, or `%APPDATA%\mdlook\config.toml` on Windows.
Every key is optional and an empty file is valid:

```toml
browse = false        # start with the file browser open
theme  = "dark"       # dark, light, or mono
width  = 100          # omit to follow the terminal

[browser]
hidden        = false # list dotfiles
sidebar_width = 30    # columns, clamped to 12..60
probe_command = ""    # e.g. "file --brief" to identify binaries with file(1)
```

A command-line flag always beats the config, which always beats the default. A
key mdlook does not recognise is an error rather than a shrug: a config that
silently does nothing is the harder problem to debug.

## How it fits together

```
source ──parse──▶ Document ──layout(width, theme)──▶ RenderedDoc ──▶ screen
                  (semantic,                         (styled lines +
                   width-free)                        plain mirror + index)
```

`RenderedDoc` carries the styled lines *and* a plain-text mirror built in the
same pass, so `plain[i]` always describes `lines[i]`. Search runs on the mirror,
which is why jumping to a match lands exactly right instead of approximately —
there is no second pass that could disagree. Resizing re-runs only `layout`, and
restores your position by nearest heading rather than raw line offset, since a
narrower terminal makes every paragraph taller.

A file that is not markdown takes a different road into the same `RenderedDoc`:
`Content` decides what the file is and picks a producer — the markdown pipeline,
a numbered whole-file view, or a one-paragraph identification. Everything below
that point is written once, which is why search, the match index and `--plain`
work on a `.rs` file without knowing it is one. Showing and hiding the browser is
a width change like any other, and re-anchors the same way as a resize.

| | |
|---|---|
| `src/doc/` | markdown → semantic tree; soft/hard break resolution |
| `src/layout/` | tree + width + theme → styled lines, anchors, links |
| `src/files/` | what a file is, and the browser's directory tree |
| `src/render/` | the ratatui viewer and the ANSI dumper |
| `src/ui/` | viewer state, search, the shared list popup, the sidebar |
| `src/content.rs` | markdown / text / binary, all laying out to `RenderedDoc` |
| `src/config.rs` | the config file, and how it merges with the flags |

## Tests

```
cargo test        # 181 tests
cargo clippy --all-targets
```

`tests/viewer.rs` drives the viewer's state machine directly — fast and
deterministic, no pty needed. The drawing layer needs a real terminal, so
`tests/tui_capture.py` runs the binary in a pty and prints the resulting screen:

```
tests/tui_capture.py -- ./target/debug/mdlook README.md
tests/tui_capture.py / u s e r -- ./target/debug/mdlook README.md
```

Each bare argument before `--` is one keystroke.

## Scope

A viewer, not an editor — documents are assumed to be in good shape already, and
nothing here writes, renames or deletes anything. The file browser is for
reading: it walks a tree and previews what it finds, and that is the whole of it.
Links are listed rather than followed.

## Safety

mdlook renders untrusted input, so a markdown file can contain raw terminal
escape sequences — which are commands, not text. It replaces control characters
with their visible Unicode Control Picture (`ESC` becomes `␛`) rather than
forwarding them, so a hostile README cannot repaint your terminal or rewrite its
window title — and the same treatment is applied to file names in the browser,
which are equally untrusted. It makes no network requests, writes no files, and
starts no subprocess unless you configure `probe_command` yourself. See
[SECURITY.md](SECURITY.md).

## Contributing

Bug reports and patches are both welcome — see
[CONTRIBUTING.md](CONTRIBUTING.md). For a rendering problem, the source markdown
is usually the whole report.

## License

[Apache-2.0](LICENSE).
