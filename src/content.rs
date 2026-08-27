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
use std::sync::Arc;

use anyhow::{Context, Result};
use image::RgbaImage;

use crate::doc::Document;
use crate::files::detect::{self, Kind};
use crate::layout::picture::{self, BlockMode};
use crate::layout::{self, source, RenderedDoc, Theme};

/// Largest file shown as text.
///
/// Past this the whole thing would be read, highlighted and laid out to answer a
/// single arrow key. A file this size is a database or a dump, not something
/// anyone reads in a pager, so it gets described instead.
pub const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;

/// Largest file the image decoder is pointed at, and the most pixels it is
/// asked to hold. Both bounds answer the same question — how much work is a
/// preview allowed to cost? — from the two directions a file can be expensive:
/// bytes to read, and pixels to allocate. A file past either is described like
/// any other binary rather than decoded.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 64_000_000;

/// Largest slice of a file the hex view shows.
///
/// A dump line covers at most sixteen bytes, so even this much is sixty-five
/// thousand rows — far past what anyone pages through, and still cheap enough
/// to lay out on the keypress that asked for it. The layout says what was
/// left off.
pub const MAX_HEX_BYTES: u64 = 1024 * 1024;

/// Largest PDF handed to the text extractor.
///
/// Extraction walks every page, so this bounds a cursor-move's worst case the
/// same way [`MAX_IMAGE_BYTES`] does for a decode. Most PDFs past this size are
/// scans, which have no text to extract anyway.
pub const MAX_PDF_BYTES: u64 = 32 * 1024 * 1024;

/// How images should be handled, resolved from the config and command line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageOptions {
    /// Off means an image file is identified like any other binary. The escape
    /// hatch for directories that are mostly photographs.
    pub enabled: bool,
    pub mode: BlockMode,
}

impl Default for ImageOptions {
    fn default() -> Self {
        Self { enabled: true, mode: BlockMode::default() }
    }
}

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
    /// Raw bytes shown as a hex dump — any file at all, behind the `x` toggle.
    ///
    /// Bytes rather than a path, so layout stays a pure function of the
    /// content like every other variant. At most [`MAX_HEX_BYTES`] are held;
    /// `size` is the whole file, so the layout can say what was left off. The
    /// `Arc` is for the same reason as the image's: `Content` is cloned
    /// freely, and a megabyte is too much to copy casually.
    Hex {
        bytes: Arc<Vec<u8>>,
        size: u64,
    },
    /// A decoded image, drawn as coloured block characters.
    ///
    /// The pixels sit behind an `Arc` because `Content` is cloned freely — the
    /// browser's preview cache holds recent ones — and a photograph is the one
    /// variant whose payload is too large to copy casually.
    Image {
        name: String,
        /// e.g. `"PNG image"`, from the signature table.
        format: String,
        /// File size on disk, for the info line.
        size: u64,
        pixels: Arc<RgbaImage>,
        mode: BlockMode,
    },
    /// An image the browser has identified but deliberately not decoded yet.
    ///
    /// This is what makes holding an arrow key through a directory of
    /// photographs painless: the pane shows this immediately, and the decode
    /// runs only once the selection has rested on the file — the debounce lives
    /// in the event loop, which swaps this for [`Content::Image`].
    PendingImage {
        name: String,
        format: String,
        size: u64,
    },
    /// A PDF whose text has not been extracted yet — the same bargain as
    /// [`Content::PendingImage`], because walking a document's pages on a
    /// cursor move costs the same kind of time a decode does.
    PendingPdf {
        name: String,
        size: u64,
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
        self.layout_sized(width, None, theme)
    }

    /// Lay out for a pane of a known height. Still pure — the height is just
    /// one more argument.
    ///
    /// Only an image uses it, to fit the drawing to the pane instead of only
    /// to its width; every text variant lays out the same however tall the
    /// pane is, which is why [`Content::layout`] exists without it.
    pub fn layout_sized(&self, width: usize, height: Option<usize>, theme: &Theme) -> RenderedDoc {
        match self {
            Content::Markdown(document) => layout::layout(document, width, theme),
            Content::Text { name, body } => source::source(name, body, width, theme),
            Content::Summary { name, headline, detail } => {
                source::summary(name, headline, detail, width, theme)
            }
            Content::Hex { bytes, size } => layout::hex::hex(bytes, *size, width, theme),
            Content::Image { name, format, size, pixels, mode } => {
                let caption = picture::Caption { name, format, size: *size };
                picture::picture(caption, pixels, *mode, width, height, theme)
            }
            Content::PendingImage { name, format, size } => {
                let headline = format!("{format}  ·  {}", detect::human_size(*size));
                source::summary(name, &headline, "rendering…", width, theme)
            }
            Content::PendingPdf { name, size } => {
                let headline = format!("PDF document  ·  {}", detect::human_size(*size));
                source::summary(name, &headline, "extracting text…", width, theme)
            }
        }
    }

    /// Whether layout depends on the pane's height, so the viewer knows a
    /// height change alone requires laying out again.
    pub fn wants_height(&self) -> bool {
        matches!(self, Content::Image { .. })
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
    ///
    /// An image this build can decode is decoded here, unless images are off,
    /// in which case it stays a described binary like before the viewer knew
    /// how.
    pub fn read(path: &Path, probe: Option<&str>, images: ImageOptions) -> Result<Self> {
        Self::read_inner(path, probe, images, false)
    }

    /// `defer` is the browser's variant of the same read: anything expensive to
    /// open — an image to decode, a PDF to extract — comes back as its
    /// `Pending*` placeholder instead, so the cursor can pass over a hundred
    /// such files without paying for one.
    fn read_inner(
        path: &Path,
        probe: Option<&str>,
        images: ImageOptions,
        defer: bool,
    ) -> Result<Self> {
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

        // Before the text/binary split, because it belongs to neither side: a
        // PDF is usually full of compressed streams, but an all-ASCII one is
        // legal and would otherwise pass the UTF-8 test and be shown as its
        // own raw source. The magic number outranks what the bytes decode as.
        // Ungated, unlike images: extraction produces text, which is what this
        // viewer is for, and the debounce already bounds its cost.
        if detect::is_pdf(&head) {
            return Ok(if defer {
                Content::PendingPdf { name, size: metadata.len() }
            } else {
                Self::extract_pdf(path)
            });
        }

        if detect::kind(&name, &head) == Kind::Binary {
            if images.enabled {
                if let Some(format) = detect::supported_image(&head) {
                    return Ok(if defer {
                        Content::PendingImage {
                            name,
                            format: format.to_string(),
                            size: metadata.len(),
                        }
                    } else {
                        Self::decode_image(path, images)
                    });
                }
            }
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
    ///
    /// Anything expensive to open is identified but not opened: it comes back
    /// as a `Pending*` placeholder, and the viewer decides when the real work
    /// is worth running. See [`Content::preview_resolved`].
    pub fn preview(path: &Path, probe: Option<&str>, images: ImageOptions) -> Self {
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

        match Self::read_inner(path, probe, images, true) {
            Ok(content) => content,
            Err(error) => Self::note(&name, "cannot be read", &root_cause(&error)),
        }
    }

    /// The preview's second half: the full-cost read that [`Content::preview`]
    /// deferred, run once the selection has rested on the file.
    ///
    /// This re-reads and re-sniffs rather than trusting what the placeholder
    /// said the file was: the debounce is a window, and a file can change under
    /// it. Whatever the file is *now* is what gets shown.
    pub fn preview_resolved(path: &Path, probe: Option<&str>, images: ImageOptions) -> Self {
        let name = path.to_string_lossy().into_owned();
        match Self::read_inner(path, probe, images, false) {
            Ok(content) => content,
            Err(error) => Self::note(&name, "cannot be read", &root_cause(&error)),
        }
    }

    /// Read a file for the hex view: the first [`MAX_HEX_BYTES`] of anything.
    ///
    /// Never fails, like [`Content::preview`], because the toggle runs inside
    /// the viewer, where every disappointment has to land as something the
    /// pane can show. The regular-file guard is there for the same reason as
    /// the preview's: this opens whatever the toggle was pressed on.
    pub fn read_hex(path: &Path) -> Self {
        let name = path.to_string_lossy().into_owned();
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };
        if !metadata.is_file() {
            return Self::note(&name, "not a regular file", "nothing to display");
        }

        let mut bytes = Vec::new();
        let read =
            File::open(path).and_then(|file| file.take(MAX_HEX_BYTES).read_to_end(&mut bytes));
        match read {
            Ok(_) => {
                // The file may have changed size since the stat; taking the
                // larger number keeps "showing the first…" from ever claiming
                // more than is actually shown.
                let size = metadata.len().max(bytes.len() as u64);
                Content::Hex { bytes: Arc::new(bytes), size }
            }
            Err(error) => Self::note(&name, "cannot be read", &error.to_string()),
        }
    }

    /// Decode an image file into [`Content::Image`], whatever it takes.
    ///
    /// Never fails, for the same reason [`Content::preview`] never fails: this
    /// runs while a browser is open, and every way a file can disappoint —
    /// vanished since it was listed, too large, corrupt past its magic number —
    /// has to land as something the pane can show.
    fn decode_image(path: &Path, options: ImageOptions) -> Self {
        let name = path.to_string_lossy().into_owned();

        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };
        if metadata.len() > MAX_IMAGE_BYTES {
            return Self::note(
                &name,
                &format!("image  ·  {}", detect::human_size(metadata.len())),
                "too large to render",
            );
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };
        let format = detect::supported_image(&bytes).unwrap_or("image").to_string();
        let size = bytes.len() as u64;

        let cursor = || std::io::Cursor::new(&bytes);
        // Dimensions come from the header alone, so an absurd pixel count is
        // refused before the allocation it names, not after.
        let dimensions = image::ImageReader::new(cursor())
            .with_guessed_format()
            .map_err(anyhow::Error::from)
            .and_then(|r| r.into_dimensions().map_err(anyhow::Error::from));
        match dimensions {
            Ok((w, h)) if u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS => {
                return Self::note(
                    &name,
                    &format!("{format}  ·  {w}×{h}  ·  {}", detect::human_size(size)),
                    "too large to render",
                );
            }
            Ok(_) => {}
            Err(error) => {
                return Self::binary_with_detail(&name, size, &format, &error.to_string())
            }
        }

        let decoded = image::ImageReader::new(cursor())
            .with_guessed_format()
            .map_err(anyhow::Error::from)
            .and_then(|r| r.decode().map_err(anyhow::Error::from));
        match decoded {
            Ok(image) => Content::Image {
                name,
                format,
                size,
                pixels: Arc::new(image.to_rgba8()),
                mode: options.mode,
            },
            // Identified as an image but not decodable as one: a truncated
            // download, a novel encoding. Fall back to describing it, keeping
            // the reason on the page rather than in a log nobody sees.
            Err(error) => Self::binary_with_detail(&name, size, &format, &error.to_string()),
        }
    }

    fn binary_with_detail(name: &str, size: u64, format: &str, error: &str) -> Self {
        Content::Summary {
            name: name.to_string(),
            headline: format!("binary file  ·  {}", detect::human_size(size)),
            detail: format!("{format}  ·  does not decode: {error}"),
        }
    }

    /// Extract a PDF's text, whatever it takes. Same never-fail contract as
    /// [`Content::decode_image`], for the same reason.
    ///
    /// The result goes through the whole-file text view: no layout is
    /// reconstructed, which is the deal — the text is enough to read, search
    /// and grep-by-eye, and anything more belongs in a PDF viewer.
    fn extract_pdf(path: &Path) -> Self {
        let name = path.to_string_lossy().into_owned();

        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };
        let headline = format!("PDF document  ·  {}", detect::human_size(metadata.len()));
        if metadata.len() > MAX_PDF_BYTES {
            return Self::note(&name, &headline, "too large to extract text from");
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return Self::note(&name, "cannot be read", &error.to_string()),
        };

        // The extractor is a parser for a hostile format, and its history says
        // some inputs panic it. A panic on a cursor move must not take the
        // browser down, so it is caught — and the default panic hook is
        // silenced for the call, because its stderr message would be drawn
        // straight onto the alternate screen.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let extracted = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&bytes));
        std::panic::set_hook(hook);

        match extracted {
            Ok(Ok(text)) if text.trim().is_empty() => {
                Self::note(&name, &headline, "no extractable text — likely a scanned document")
            }
            Ok(Ok(text)) => Content::Text { name, body: text },
            Ok(Err(error)) => {
                Self::note(&name, &headline, &format!("text extraction failed: {error}"))
            }
            Err(_) => Self::note(&name, &headline, "text extraction failed"),
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
            Content::Hex { bytes: Arc::new(vec![0x00, 0x41, 0xff]), size: 3 },
        ];
        for content in &contents {
            for width in 1..30 {
                let _ = content.layout(width, &Theme::default());
            }
        }
    }

    #[test]
    fn reading_a_missing_file_names_it_in_the_error() {
        let error = Content::read(Path::new("/nonexistent/nope.md"), None, ImageOptions::default())
            .unwrap_err();
        assert!(format!("{error}").contains("nope.md"));
    }

    #[test]
    fn previewing_never_fails_however_bad_the_path() {
        // Whatever is wrong, the browser has to keep running.
        for path in ["/nonexistent/nope.md", "/", "/proc/self/mem", "/dev/null"] {
            let content = Content::preview(Path::new(path), None, ImageOptions::default());
            let doc = content.layout(60, &Theme::default());
            assert!(!doc.is_empty(), "{path} rendered nothing");
        }
    }

    #[test]
    fn a_directory_is_a_note_in_the_preview_but_an_error_when_named() {
        assert!(matches!(
            Content::preview(Path::new("/"), None, ImageOptions::default()),
            Content::Summary { .. }
        ));
        assert!(Content::read(Path::new("/"), None, ImageOptions::default()).is_err());
    }

    // -- images --------------------------------------------------------------

    /// A real 1×1 red PNG under a name this test alone writes — tests run in
    /// parallel, so a shared file would be a race.
    fn png_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("mdlook-test-images");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        image.save(&path).unwrap();
        path
    }

    #[test]
    fn reading_an_image_decodes_it_and_previewing_defers_it() {
        let path = png_file("read-vs-preview.png");

        let read = Content::read(&path, None, ImageOptions::default()).unwrap();
        assert!(matches!(read, Content::Image { .. }), "read decodes: {read:?}");

        let preview = Content::preview(&path, None, ImageOptions::default());
        assert!(
            matches!(preview, Content::PendingImage { ref format, .. } if format == "PNG image"),
            "preview defers: {preview:?}"
        );

        let decoded = Content::preview_resolved(&path, None, ImageOptions::default());
        let doc = decoded.layout(40, &Theme::default());
        assert!(doc.status.is_some(), "an image reports its rendering to the status bar");
    }

    #[test]
    fn with_images_off_a_png_is_identified_like_any_binary() {
        let path = png_file("images-off.png");

        let off = ImageOptions { enabled: false, ..Default::default() };
        let content = Content::read(&path, None, off).unwrap();
        assert!(matches!(content, Content::Summary { .. }));
        let text = content.layout(60, &Theme::default()).plain.join("\n");
        assert!(text.contains("PNG image"), "{text}");
    }

    #[test]
    fn a_file_that_lies_about_being_an_image_falls_back_to_a_description() {
        let dir = std::env::temp_dir().join("mdlook-test-images");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("liar-not-a-png.png");
        // A valid PNG signature followed by garbage: sniffs as an image,
        // decodes as nothing.
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n not actually a png").unwrap();

        let content = Content::preview_resolved(&path, None, ImageOptions::default());
        assert!(matches!(content, Content::Summary { .. }), "{content:?}");
        let text = content.layout(80, &Theme::default()).plain.join("\n");
        assert!(text.contains("does not decode"), "{text}");
    }

    // -- PDFs ----------------------------------------------------------------

    /// A minimal but valid one-page PDF that says `text` in Helvetica, with a
    /// correct xref table — built by hand so the test depends on no fixture
    /// file and no external tool.
    fn tiny_pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];

        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", index + 1));
        }
        let xref_at = out.len();
        out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for offset in offsets {
            out.push_str(&format!("{offset:010} 00000 n \n"));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        out.into_bytes()
    }

    fn pdf_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("mdlook-test-pdfs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn reading_a_pdf_extracts_its_text_and_previewing_defers_it() {
        let path = pdf_file("hello.pdf", &tiny_pdf("Hello mdlook"));

        let preview = Content::preview(&path, None, ImageOptions::default());
        assert!(matches!(preview, Content::PendingPdf { .. }), "preview defers: {preview:?}");

        let read = Content::read(&path, None, ImageOptions::default()).unwrap();
        assert!(matches!(read, Content::Text { .. }), "read extracts: {read:?}");
        let text = read.layout(80, &Theme::default()).plain.join("\n");
        assert!(text.contains("Hello mdlook"), "{text}");
    }

    #[test]
    fn a_pdf_that_does_not_parse_is_a_note_not_a_crash() {
        let path = pdf_file("broken.pdf", b"%PDF-1.4\nthis is not a real pdf at all");
        let content = Content::preview_resolved(&path, None, ImageOptions::default());
        assert!(matches!(content, Content::Summary { .. }), "{content:?}");
        let text = content.layout(80, &Theme::default()).plain.join("\n");
        assert!(text.contains("PDF document"), "{text}");
    }

    // -- hex -----------------------------------------------------------------

    fn hex_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("mdlook-test-hex");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn read_hex_reads_any_file_whole_when_it_fits() {
        let path = hex_file("small.bin", b"\x00\x01binary");
        let content = Content::read_hex(&path);
        let Content::Hex { bytes, size } = &content else { panic!("{content:?}") };
        assert_eq!(bytes.as_slice(), b"\x00\x01binary");
        assert_eq!(*size, 8);
        let text = content.layout(80, &Theme::default()).plain.join("\n");
        assert!(text.contains("00 01 62 69 6e 61 72 79"), "{text}");
    }

    #[test]
    fn read_hex_caps_the_bytes_but_reports_the_whole_size() {
        let path = hex_file("big.bin", &vec![0xab; MAX_HEX_BYTES as usize + 512]);
        let content = Content::read_hex(&path);
        let Content::Hex { bytes, size } = &content else { panic!("{content:?}") };
        assert_eq!(bytes.len() as u64, MAX_HEX_BYTES, "the read stops at the cap");
        assert_eq!(*size, MAX_HEX_BYTES + 512, "the size is still the whole file");
        let first = content.layout(80, &Theme::default()).plain[0].clone();
        assert!(first.contains("showing the first 1.0 MiB of"), "{first}");
    }

    #[test]
    fn read_hex_failures_land_as_notes_not_errors() {
        for path in ["/nonexistent/nope.bin", "/", "/dev/null"] {
            let content = Content::read_hex(Path::new(path));
            assert!(matches!(content, Content::Summary { .. }), "{path}: {content:?}");
        }
    }

    #[test]
    fn a_pdf_with_no_text_says_it_is_probably_scanned() {
        // Structurally valid, but its only content stream draws nothing.
        let path = pdf_file("empty.pdf", &tiny_pdf(""));
        let content = Content::preview_resolved(&path, None, ImageOptions::default());
        let text = content.layout(80, &Theme::default()).plain.join("\n");
        assert!(text.contains("no extractable text"), "{text}");
    }
}
