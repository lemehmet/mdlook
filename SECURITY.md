# Security Policy

## Reporting a vulnerability

Please report security issues privately through GitHub's **"Report a
vulnerability"** (Security ▸ Advisories) on this repository. Do not open a public
issue for an undisclosed vulnerability. We aim to acknowledge within a few days
and will agree a fix and disclosure timeline with you.

## Security model

mdlook renders untrusted input. A file can come from anywhere — a cloned
repository, a downloaded release, a pasted pipe — and mdlook will happily accept
whatever it is handed. With the file browser it accepts *directory listings* too,
so file names are untrusted input on the same footing as file contents. That is
the whole attack surface, and the posture is shaped around it:

- **It only reads, and it never writes.** mdlook never creates or modifies a
  file, never writes a cache, and offers no rename, delete or edit. The config
  file is read if it exists; mdlook does not create one and has no command that
  would.
- **It reads only what you point it at.** Without the browser that is the one
  file you name, or stdin. With the browser it is the directory you rooted it at
  and, as you expand them, the directories beneath — nothing above the root, and
  no symbolic link is followed into a directory, so the tree cannot be walked out
  of its root or around a cycle.
- **It starts no subprocess by default.** Binary files are identified in-process
  against a built-in table of magic numbers. Setting `probe_command` in the
  config trades that guarantee for `file(1)`'s larger database. That command is
  split on whitespace and executed directly — never through a shell — with `--`
  and then the path appended, so nothing in a file name can be read as an option
  or become a command of its own. It runs only on regular files, and its output
  is truncated to one line and passed through the same control-character
  neutralisation as everything else, because it echoes both the file's contents
  and its name. Leave it unset and no process is ever spawned.
- **Only regular files are opened.** Named pipes, sockets and devices are
  described, not read. This matters most in the browser, where the "open" happens
  on every press of the down arrow: reading a FIFO would hang the viewer with no
  way out.
- **No network access of any kind.** Links are displayed and listed; they are
  never fetched. Images are shown as `[image: alt]` rather than downloaded.
  mdlook does not phone home, check for updates, or contact any endpoint.
- **No HTML is interpreted.** Raw HTML blocks and inline HTML are rendered as
  literal, dimmed text. There is no HTML parser to confuse and nothing that
  could resolve a remote reference.
- **Terminal escapes are neutralised, not forwarded.** Control characters are
  replaced with their visible Unicode Control Picture (`ESC` becomes `␛`) at the
  single point where any line is appended to the rendered document, so a new
  block type — or a new *kind of content*, such as the whole-file view — cannot
  forget to do it. Link URLs, the filename in the status bar and the entry names
  in the browser are handled separately, because they reach the screen without
  passing through that point. A directory full of files with escape sequences for
  names is a real thing, and the tree sanitises every one. This matters: a viewer that
  forwards escapes lets a hostile README repaint your terminal, rewrite the
  window title, or on terminals that answer OSC queries do rather worse. The
  behaviour is pinned by tests in `tests/rendering.rs` that assert no control
  byte survives into the rendered text, into a link URL, or out of the ANSI
  writer.
- **Escapes are shown, not silently dropped.** A document containing one says so
  visibly. Swallowing it would render the file as though it were something it is
  not.
- **`--plain` emits only styles mdlook chose.** The ANSI writer serialises the
  same neutralised cells and adds SGR colour codes from a fixed palette. It
  never emits OSC or BEL, so nothing can set a title or reach a clipboard.
- **No `unsafe`.** The crate contains none, and `unsafe_op_in_unsafe_fn` is
  denied at the manifest level.

## Robustness

A viewer that panics on a malformed file is a bug, not a crash-only design:

- Unbalanced markdown structure unwinds rather than panicking — the parser
  closes whatever containers are still open at end of input.
- Widths, scroll offsets and popup geometry are clamped rather than indexed
  blind. The test suite renders the corpus at every width from 1 to 12 columns
  precisely because that is where off-by-one panics live.
- Search offsets are byte ranges into the rendered text, produced by
  character-wise scanning so they are always valid UTF-8 boundaries. A test
  slices every reported range to prove it, for the whole-file view as well as
  for markdown.
- A file whose bytes are not valid UTF-8, or that contains a NUL, is classified
  as binary and never handed to the parser. Text that goes bad partway through is
  decoded lossily rather than refused.
- Reading is bounded. A file is identified from its first 8 KB, so a large binary
  is described without being read; text past 16 MB is described rather than laid
  out; a directory listing stops at 5000 entries and says how many it dropped.
- The browser never fails fatally on a file. Anything unreadable renders as a
  note in the pane, because a viewer that exits when the cursor passes over a
  file you lack permission on is not usable.

If you find input that panics mdlook, that is a bug report worth filing, and one
that reaches a terminal escape through to the terminal is a security report.

## Supply chain

- Dependencies are few and argued for: `pulldown-cmark`, `ratatui`, `crossterm`,
  `syntect`, `two-face`, `unicode-width`, `clap`, `anyhow`. Dependabot watches
  both the crates and the workflow actions weekly.
- syntect is built with `regex-fancy` rather than its default `onig`, so the
  build is pure Rust with no C toolchain and no vendored C regex engine.
- Released binaries are built with build paths remapped, and the release workflow
  fails if the builder's directories survive into the binary.
- Release archives ship a SHA-256 checksum, verified in the workflow from the
  directory it ships in.

## Known limits

- **Highlighting is best-effort.** syntect's grammars are regular expressions
  over untrusted text; a pathological code block could be slow to highlight.
  It is bounded by the size of the block, and mdlook lays out the whole document
  once rather than per frame.
- **Terminal emulators vary.** mdlook computes widths from Unicode data; a
  terminal that disagrees about a glyph's width can misalign a table. That is a
  rendering defect, not a safety one.
