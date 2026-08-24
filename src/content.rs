//! What the viewer is looking at.
//!
//! Markdown is one of three things a file can turn out to be, and the viewer
//! should not have to care which. Every variant here lays out to the same
//! [`RenderedDoc`], so scrolling, search, the match index, resize anchoring and
//! the `--plain` writer are written once and work for all of them.
//!
//! Note what this preserves: layout stays a pure function of `(content, width,
//! theme)`. Reading the file happens here, once, up front; nothing below this
//! module touches the filesystem.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

use crate::doc::Document;
use crate::files::detect::{self, Kind};
use crate::layout::{self, source, RenderedDoc, Theme};

/// Largest file shown as text.
///
/// Past this the whole thing would be read, highlighted and laid out to answer a
/// single arrow key. A file this size is a database or a dump, not something
/// anyone reads in a pager, so it gets described instead.
pub const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum Content {
    Markdown(Document),
    /// Anything else that decodes as text: source, config, logs, prose.
    Text {
        name: String,
        body: String,
    },
    /// A file we are not going to show the contents of, and why.
    ///
    /// Binaries are the common case — their bytes would be noise on a terminal,
    /// so we say what the file *is* and let the reader pick another tool. The
    /// same shape covers everything else that cannot be displayed: a file too
    /// large to lay out, a socket, a directory entry we could not read.
    Summary {
        name: String,
        headline: String,
        detail: String,
    },
}

impl From<Document> for Content {
    fn from(document: Document) -> Self {
        Content::Markdown(document)
    }
}

impl Content {
    /// Lay out at a given width. Pure, exactly like [`layout::layout`].
    pub fn layout(&self, width: usize, theme: &Theme) -> RenderedDoc {
        match self {
            Content::Markdown(document) => layout::layout(document, width, theme),
            Content::Text { name, body } => source::source(name, body, width, theme),
            Content::Summary { name, headline, detail } => {
                source::summary(name, headline, detail, width, theme)
            }
        }
    }

    /// Classify and wrap bytes already in hand.
    ///
    /// `name` decides only whether text is markdown and which syntax to
    /// highlight with; the bytes decide everything else.
    pub fn from_bytes(name: &str, bytes: &[u8], size: u64) -> Self {
        let head = &bytes[..bytes.len().min(detect::SNIFF_LEN)];
        match detect::kind(name, head) {
            Kind::Binary => Self::binary(name, size, detect::describe(head)),
            // Lossy rather than strict: the sniff window proved the *start*
            // decodes, and a stray bad byte halfway down a log is a reason to
            // show a replacement character, not to refuse the file.
            Kind::Markdown => Content::Markdown(crate::doc::parse(&String::from_utf8_lossy(bytes))),
            Kind::Text => Content::Text {
                name: name.to_string(),
                body: String::from_utf8_lossy(bytes).into_owned(),
            },
        }
    }

    fn binary(name: &str, size: u64, detail: String) -> Self {
        Content::Summary {
            name: name.to_string(),
            headline: format!("binary file  ·  {}", detect::human_size(size)),
            detail,
        }
    }

    fn note(name: &str, headline: &str, detail: &str) -> Self {
        Content::Summary {
            name: name.to_string(),
            headline: headline.to_string(),
            detail: detail.to_string(),
        }
    }

    /// Read a file and classify it.
    ///
    /// The head is read first and the rest only if it turns out to be worth
    /// reading: identifying a binary needs its first few hundred bytes, so
    /// there is no reason to pull a gigabyte disk image through memory to
    /// print one line about it.
    pub fn read(path: &Path, probe: Option<&str>) -> Result<Self> {
        let name = path.to_string_lossy().into_owned();
        let metadata = std::fs::metadata(path).with_context(|| format!("reading {name}"))?;

        if metadata.is_dir() {
            anyhow::bail!("{name} is a directory");
        }

        let mut file = File::open(path).with_context(|| format!("reading {name}"))?;
        let mut head = Vec::new();
        file.by_ref()
            .take(detect::SNIFF_LEN as u64)
            .read_to_end(&mut head)
            .with_context(|| format!("reading {name}"))?;

        if detect::kind(&name, &head) == Kind::Binary {
            // The external identifier, when one is configured, otherwise the
            // built-in table. Falling back rather than reporting the failure
            // keeps a missing `file(1)` from turning into an error message
            // where the description belongs.
            let detail = probe
                .and_then(|command| detect::probe(command, path))
                .unwrap_or_else(|| detect::describe(&head));
            return Ok(Self::binary(&name, metadata.len(), detail));
        }
        if metadata.len() > MAX_TEXT_BYTES {
            return Ok(Self::note(
                &name,
                &format!("text file  ·  {}", detect::human_size(metadata.len())),
                "too large to display",
            ));
        }

        let mut bytes = head;
        file.read_to_end(&mut bytes).with_context(|| format!("reading {name}"))?;
        let size = bytes.len() as u64;
        Ok(Self::from_bytes(&name, &bytes, size))
    }

    /// Read a file for the browser's preview pane.
    ///
    /// Unlike [`Content::read`] this never fails: a browser that quit because
    /// the cursor passed over an unreadable file would be unusable, so every
    /// failure renders as a note in the pane instead.
    ///
    /// Only regular files are opened. That is not tidiness — opening a FIFO or a
    /// character device blocks the process forever, and here the "open" happens
    /// on every press of the down arrow.
    pub fn preview(path: &Path, probe: Option<&str>) -> Self {
        let name = path.to_string_lossy().into_owned();

        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };

        if metadata.is_dir() {
            return Self::note(&name, "directory", "select a file to preview it");
        }
        if !metadata.is_file() {
            // Named pipes, sockets, block and character devices. Reading one can
            // block indefinitely, and there is nothing to show even if it did not.
            return Self::note(&name, "not a regular file", "nothing to display");
        }

        match Self::read(path, probe) {
            Ok(content) => content,
            Err(error) => Self::note(&name, "cannot be read", &root_cause(&error)),
        }
    }
}

/// The innermost error, which is the one that says what actually went wrong.
fn root_cause(error: &anyhow::Error) -> String {
    error.chain().last().map(|e| e.to_string()).unwrap_or_else(|| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_markdown_name_parses_as_markdown() {
        let content = Content::from_bytes("a.md", b"# Title\n\nBody.\n", 15);
        assert!(matches!(content, Content::Markdown(_)));
        let doc = content.layout(60, &Theme::default());
        assert_eq!(doc.anchors.len(), 1, "headings should still be found");
    }

    #[test]
    fn any_other_text_name_goes_through_the_file_view() {
        let content = Content::from_bytes("main.rs", b"fn main() {}\n", 13);
        assert!(matches!(content, Content::Text { .. }));
        let doc = content.layout(60, &Theme::default());
        assert_eq!(doc.plain, vec!["1 fn main() {}"]);
    }

    #[test]
    fn a_binary_is_identified_rather_than_shown() {
        let content = Content::from_bytes("logo.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00", 4096);
        let doc = content.layout(60, &Theme::default());
        let text = doc.plain.join("\n");
        assert!(text.contains("logo.png"));
        assert!(text.contains("PNG image"));
        assert!(text.contains("4.0 KiB"));
    }

    #[test]
    fn a_markdown_extension_does_not_override_binary_bytes() {
        // A `.md` name on a file full of NULs is a mislabelled file, not a
        // reason to hand NULs to the markdown parser.
        let content = Content::from_bytes("a.md", b"\x00\x01\x02\x03", 4);
        assert!(matches!(content, Content::Summary { .. }));
    }

    #[test]
    fn every_variant_lays_out_at_any_width_without_panicking() {
        let contents = [
            Content::from_bytes("a.md", b"# Title\n\n- item\n", 16),
            Content::from_bytes("a.rs", b"fn main() {}\n", 13),
            Content::from_bytes("a.bin", b"\x00\x01\x02", 3),
        ];
        for content in &contents {
            for width in 1..30 {
                let _ = content.layout(width, &Theme::default());
            }
        }
    }

    #[test]
    fn reading_a_missing_file_names_it_in_the_error() {
        let error = Content::read(Path::new("/nonexistent/nope.md"), None).unwrap_err();
        assert!(format!("{error}").contains("nope.md"));
    }

    #[test]
    fn previewing_never_fails_however_bad_the_path() {
        // Whatever is wrong, the browser has to keep running.
        for path in ["/nonexistent/nope.md", "/", "/proc/self/mem", "/dev/null"] {
            let content = Content::preview(Path::new(path), None);
            let doc = content.layout(60, &Theme::default());
            assert!(!doc.is_empty(), "{path} rendered nothing");
        }
    }

    #[test]
    fn a_directory_is_a_note_in_the_preview_but_an_error_when_named() {
        assert!(matches!(Content::preview(Path::new("/"), None), Content::Summary { .. }));
        assert!(Content::read(Path::new("/"), None).is_err());
    }
}
