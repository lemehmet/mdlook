# Security Policy

## Reporting a vulnerability

Please report security issues privately through GitHub's **"Report a
vulnerability"** (Security ▸ Advisories) on this repository. Do not open a public
issue for an undisclosed vulnerability. We aim to acknowledge within a few days
and will agree a fix and disclosure timeline with you.

## Security model

mdlook renders untrusted input. A markdown file can come from anywhere — a
cloned repository, a downloaded release, a pasted pipe — and the parser will
happily accept whatever it is handed. That is the whole attack surface, and the
posture is shaped around it:

- **It only reads.** mdlook opens the one file you name, or stdin. It never
  writes a file, never creates a config or cache, never shells out, and never
  starts another process.
- **No network access of any kind.** Links are displayed and listed; they are
  never fetched. Images are shown as `[image: alt]` rather than downloaded.
  mdlook does not phone home, check for updates, or contact any endpoint.
- **No HTML is interpreted.** Raw HTML blocks and inline HTML are rendered as
  literal, dimmed text. There is no HTML parser to confuse and nothing that
  could resolve a remote reference.
- **Terminal escapes in a document are neutralised, not forwarded.** Control
  characters are replaced with their visible Unicode Control Picture (`ESC`
  becomes `␛`) at the single point where any line is appended to the rendered
  document, so a new block type cannot forget to do it. Link URLs and the
  filename in the status bar are handled separately, because they reach the
  screen without passing through that point. This matters: a viewer that
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
  slices every reported range to prove it.

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
