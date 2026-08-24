//! Deciding what a file is, from its bytes and its name.
//!
//! Identification is done in-process against a table of magic numbers rather
//! than by running `file(1)`. That keeps the binary self-contained — the same
//! answer on a machine with no `file`, on Windows, and inside a scratch
//! container — and keeps identification a pure function of the bytes, which is
//! the property the rest of this crate is built around. `file` is available as
//! an opt-in for the cases where its much larger database earns its keep.

use std::path::Path;
use std::process::{Command, Stdio};

/// How much of a file is inspected to decide what it is.
///
/// Every signature here lives in the first few hundred bytes; the rest of the
/// window is for the text/binary decision, which wants enough of the file to be
/// confident and not so much that opening a large file costs a full read.
pub const SNIFF_LEN: usize = 8192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Markdown,
    /// Anything that decodes as text. Shown at best effort, highlighted if we
    /// recognise it.
    Text,
    /// Shown as an identification rather than as content.
    Binary,
}

/// Extensions that mean "this is markdown".
///
/// Extension only, never content sniffing: a text file is not markdown just
/// because it happens to contain a `#`, and rendering `notes.txt` as markdown
/// would silently eat its punctuation.
const MARKDOWN_EXTENSIONS: &[&str] =
    &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdtxt", "mdtext", "rmd", "qmd"];

/// Classify a file from its name and the first [`SNIFF_LEN`] bytes.
pub fn kind(name: &str, head: &[u8]) -> Kind {
    if is_binary(head) {
        return Kind::Binary;
    }
    match extension(name) {
        Some(ext) if MARKDOWN_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) => {
            Kind::Markdown
        }
        _ => Kind::Text,
    }
}

fn extension(name: &str) -> Option<&str> {
    Path::new(name).extension().and_then(|e| e.to_str())
}

/// Whether these bytes should be treated as binary.
///
/// Two tests, in the order that costs least. A NUL byte is the classic one and
/// settles nearly every real case; UTF-8 validity catches the rest, including
/// UTF-16 without a BOM and most compressed formats.
fn is_binary(head: &[u8]) -> bool {
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        // The sniff window ends at an arbitrary byte, which can fall in the
        // middle of a multi-byte character. That is a fact about where we
        // stopped reading, not about the file, so only a sequence that is
        // *complete and invalid* is evidence of anything.
        Err(error) => error.error_len().is_some(),
    }
}

// ---------------------------------------------------------------------------
// Identification
// ---------------------------------------------------------------------------

/// A magic number and what it means.
struct Signature {
    offset: usize,
    magic: &'static [u8],
    label: &'static str,
}

const fn sig(offset: usize, magic: &'static [u8], label: &'static str) -> Signature {
    Signature { offset, magic, label }
}

/// Formats worth naming, in the order they are tested.
///
/// Ordering matters only where one signature is a prefix of another, so the
/// longer and more specific ones come first.
const SIGNATURES: &[Signature] = &[
    // Archives and compression
    sig(0, b"PK\x03\x04", "Zip archive"),
    sig(0, b"PK\x05\x06", "Zip archive (empty)"),
    sig(0, b"PK\x07\x08", "Zip archive (spanned)"),
    sig(0, b"\x1f\x8b", "gzip compressed data"),
    sig(0, b"BZh", "bzip2 compressed data"),
    sig(0, b"\xfd7zXZ\x00", "XZ compressed data"),
    sig(0, b"\x28\xb5\x2f\xfd", "Zstandard compressed data"),
    sig(0, b"\x04\x22\x4d\x18", "LZ4 compressed data"),
    sig(0, b"7z\xbc\xaf\x27\x1c", "7-zip archive"),
    sig(0, b"Rar!\x1a\x07", "RAR archive"),
    sig(0, b"\x1f\x9d", "compress'd data"),
    sig(257, b"ustar", "tar archive"),
    sig(0, b"!<arch>", "ar archive"),
    sig(0, b"\xed\xab\xee\xdb", "RPM package"),
    // Documents
    sig(0, b"%PDF-", "PDF document"),
    sig(0, b"%!PS", "PostScript document"),
    sig(0, b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", "Microsoft Office document (OLE2)"),
    // Images
    sig(0, b"\x89PNG\r\n\x1a\n", "PNG image"),
    sig(0, b"\xff\xd8\xff", "JPEG image"),
    sig(0, b"GIF87a", "GIF image"),
    sig(0, b"GIF89a", "GIF image"),
    sig(0, b"BM", "BMP image"),
    sig(0, b"II*\x00", "TIFF image (little-endian)"),
    sig(0, b"MM\x00*", "TIFF image (big-endian)"),
    sig(0, b"\x00\x00\x01\x00", "Windows icon"),
    sig(0, b"qoif", "QOI image"),
    // Audio and video
    sig(0, b"OggS", "Ogg media"),
    sig(0, b"fLaC", "FLAC audio"),
    sig(0, b"ID3", "MP3 audio"),
    sig(0, b"\x1a\x45\xdf\xa3", "Matroska media"),
    // Fonts
    sig(0, b"wOFF", "WOFF font"),
    sig(0, b"wOF2", "WOFF2 font"),
    sig(0, b"OTTO", "OpenType font"),
    sig(0, b"ttcf", "TrueType font collection"),
    sig(0, b"\x00\x01\x00\x00\x00", "TrueType font"),
    // Data and code
    sig(0, b"SQLite format 3\x00", "SQLite 3 database"),
    sig(0, b"\x00asm", "WebAssembly module"),
    sig(0, b"PACK", "Git pack file"),
    sig(0, b"DICM", "DICOM medical image"),
    sig(0, b"\x62\x76\x78\x32", "LZFSE compressed data"),
    // Text encodings we cannot show as text
    sig(0, b"\xff\xfe\x00\x00", "UTF-32 text (little-endian)"),
    sig(0, b"\x00\x00\xfe\xff", "UTF-32 text (big-endian)"),
    sig(0, b"\xff\xfe", "UTF-16 text (little-endian)"),
    sig(0, b"\xfe\xff", "UTF-16 text (big-endian)"),
];

/// Identify a binary file from the head of its contents.
///
/// Returns something you could act on — the format, and for executables the
/// architecture, since "which machine is this for" is the usual question. Falls
/// back to `"data"`, the same admission `file(1)` makes.
pub fn describe(head: &[u8]) -> String {
    if let Some(description) = elf(head).or_else(|| pe(head)).or_else(|| mach_o(head)) {
        return description;
    }
    // Checked before the table because a Mach-O fat binary shares its magic.
    if let Some(description) = java_class(head) {
        return description;
    }
    if let Some(description) = riff(head).or_else(|| ftyp(head)) {
        return description;
    }
    for signature in SIGNATURES {
        let end = signature.offset + signature.magic.len();
        if head.len() >= end && &head[signature.offset..end] == signature.magic {
            return signature.label.to_string();
        }
    }
    "data".to_string()
}

// Offsets read out of a header are attacker-controlled, so every one of these
// is checked rather than trusted: a bogus pointer must return `None`, not index
// past the buffer or overflow the addition that bounds-checks it.
fn u16_at(bytes: &[u8], at: usize, big_endian: bool) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(if big_endian { u16::from_be_bytes(raw) } else { u16::from_le_bytes(raw) })
}

fn u32_at(bytes: &[u8], at: usize, big_endian: bool) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if big_endian { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) })
}

fn elf(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"\x7fELF") {
        return None;
    }
    let class = match head.get(4)? {
        1 => "32-bit",
        2 => "64-bit",
        _ => "",
    };
    let big_endian = *head.get(5)? == 2;
    let order = if big_endian { "MSB" } else { "LSB" };

    let file_type = match u16_at(head, 16, big_endian)? {
        1 => "relocatable",
        2 => "executable",
        // Every modern distribution builds position-independent executables,
        // which are of type DYN. Telling one from a library needs a program
        // header scan, so this stays with the name the format actually uses.
        3 => "shared object",
        4 => "core dump",
        _ => "object",
    };

    let machine = match u16_at(head, 18, big_endian)? {
        0x02 => "SPARC",
        0x03 => "x86",
        0x08 => "MIPS",
        0x14 => "PowerPC",
        0x15 => "PowerPC64",
        0x16 => "S/390",
        0x28 => "ARM",
        0x2a => "SuperH",
        0x32 => "IA-64",
        0x3e => "x86-64",
        0xb7 => "ARM aarch64",
        0xf3 => "RISC-V",
        0x102 => "LoongArch",
        _ => "unknown architecture",
    };

    Some(format!("ELF {class} {order} {file_type}, {machine}"))
}

fn pe(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"MZ") {
        return None;
    }
    // The DOS stub ends with a pointer to the real header.
    let at = u32_at(head, 0x3c, false)? as usize;
    if head.get(at..at.checked_add(4)?)? != b"PE\x00\x00" {
        return None;
    }
    let machine = match u16_at(head, at.checked_add(4)?, false)? {
        0x014c => "x86",
        0x8664 => "x86-64",
        0x01c0 | 0x01c4 => "ARM",
        0xaa64 => "ARM aarch64",
        0x0200 => "IA-64",
        _ => "unknown architecture",
    };
    let characteristics = u16_at(head, at.checked_add(22)?, false)?;
    let kind = if characteristics & 0x2000 != 0 { "DLL" } else { "executable" };
    Some(format!("PE32+ {kind} (Windows), {machine}"))
}

fn mach_o(head: &[u8]) -> Option<String> {
    let magic = u32_at(head, 0, false)?;
    let (bits, big_endian) = match magic {
        0xfeed_face => ("32-bit", false),
        0xfeed_facf => ("64-bit", false),
        0xcefa_edfe => ("32-bit", true),
        0xcffa_edfe => ("64-bit", true),
        _ => return None,
    };
    let machine = match u32_at(head, 4, big_endian)? {
        7 => "x86",
        0x0100_0007 => "x86-64",
        12 => "ARM",
        0x0100_000c => "ARM64",
        18 => "PowerPC",
        _ => "unknown architecture",
    };
    let file_type = match u32_at(head, 12, big_endian)? {
        1 => "object",
        2 => "executable",
        6 => "dynamically linked shared library",
        8 => "bundle",
        10 => "dSYM companion",
        _ => "object",
    };
    Some(format!("Mach-O {bits} {file_type}, {machine}"))
}

fn java_class(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"\xca\xfe\xba\xbe") {
        return None;
    }
    // A Mach-O universal binary opens with the same four bytes. The field that
    // follows tells them apart: for Java it is the class-file version, which has
    // started at 45 since 1.0, and for Mach-O it is an architecture count, which
    // is a handful at most.
    let major = u16_at(head, 6, true)?;
    if major >= 45 {
        Some(format!("Java class data (version {major})"))
    } else {
        Some("Mach-O universal binary".to_string())
    }
}

fn riff(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"RIFF") {
        return None;
    }
    Some(
        match head.get(8..12)? {
            b"WEBP" => "WebP image",
            b"WAVE" => "WAV audio",
            b"AVI " => "AVI video",
            _ => "RIFF data",
        }
        .to_string(),
    )
}

fn ftyp(head: &[u8]) -> Option<String> {
    if head.get(4..8)? != b"ftyp" {
        return None;
    }
    Some(
        match head.get(8..12)? {
            b"avif" | b"avis" => "AVIF image",
            b"heic" | b"heix" | b"hevc" => "HEIF image",
            b"qt  " => "QuickTime video",
            b"M4A " => "MPEG-4 audio",
            _ => "MPEG-4 media",
        }
        .to_string(),
    )
}

/// Ask an external command what a file is.
///
/// Off unless the reader sets `probe_command`, and the one place mdlook starts a
/// process. The command is split on whitespace, `--` is appended to stop the
/// path being read as an option, and the path goes last. No shell is involved,
/// so nothing in a file name can become a command — which matters, since file
/// names are attacker-controlled.
///
/// Returns `None` on any failure, leaving the built-in identification in place:
/// a missing `file(1)` should be a quieter outcome than an error message where
/// the description belongs.
pub fn probe(command: &str, path: &Path) -> Option<String> {
    let mut parts = command.split_whitespace();
    let program = parts.next()?;

    let output =
        Command::new(program).args(parts).arg("--").arg(path).stdin(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }

    // The command's output is not trusted: it echoes parts of the file, and of
    // the file's name. Take one line, cap it, and neutralise control characters
    // exactly as the rest of the renderer does.
    let text = String::from_utf8_lossy(&output.stdout);
    let line: String = text
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(512)
        .map(crate::layout::wrap::sanitize)
        .collect();

    (!line.is_empty()).then_some(line)
}

/// Format a byte count the way a reader wants to read it.
///
/// Binary units, because that is what every other tool in a terminal reports and
/// a mismatch is more confusing than either convention alone.
pub fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_decided_by_extension_never_by_content() {
        assert_eq!(kind("README.md", b"# Title"), Kind::Markdown);
        assert_eq!(kind("README.MD", b"# Title"), Kind::Markdown);
        assert_eq!(kind("notes.txt", b"# Title"), Kind::Text, "a # does not make it markdown");
        assert_eq!(kind("Makefile", b"all:\n"), Kind::Text);
    }

    #[test]
    fn a_nul_byte_means_binary() {
        assert_eq!(kind("x.txt", b"text\x00more"), Kind::Binary);
        assert_eq!(kind("x.md", b"text\x00more"), Kind::Binary, "even with a markdown name");
    }

    #[test]
    fn invalid_utf8_means_binary() {
        assert_eq!(kind("x.txt", b"caf\xe9 latte"), Kind::Binary);
    }

    #[test]
    fn a_character_cut_by_the_sniff_window_is_not_binary() {
        // "é" is two bytes; hand over only the first one, as a truncated read
        // would. The file is fine, we just stopped mid-character.
        assert_eq!(kind("x.txt", b"caf\xc3"), Kind::Text);
    }

    #[test]
    fn an_empty_file_is_text() {
        assert_eq!(kind("x.txt", b""), Kind::Text);
        assert_eq!(kind("x.md", b""), Kind::Markdown);
    }

    #[test]
    fn elf_headers_are_decoded_not_just_matched() {
        let mut head = vec![0u8; 64];
        head[..4].copy_from_slice(b"\x7fELF");
        head[4] = 2; // 64-bit
        head[5] = 1; // little-endian
        head[16] = 3; // ET_DYN
        head[18] = 0x3e; // x86-64
        assert_eq!(describe(&head), "ELF 64-bit LSB shared object, x86-64");

        head[16] = 2; // ET_EXEC
        head[18] = 0xb7; // aarch64
        assert_eq!(describe(&head), "ELF 64-bit LSB executable, ARM aarch64");
    }

    #[test]
    fn pe_headers_are_followed_through_the_dos_stub() {
        let mut head = vec![0u8; 128];
        head[..2].copy_from_slice(b"MZ");
        head[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        head[64..68].copy_from_slice(b"PE\x00\x00");
        head[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        head[86..88].copy_from_slice(&0x2000u16.to_le_bytes());
        assert_eq!(describe(&head), "PE32+ DLL (Windows), x86-64");
    }

    #[test]
    fn a_fat_macho_is_not_mistaken_for_a_java_class() {
        let mut fat = vec![0u8; 16];
        fat[..4].copy_from_slice(b"\xca\xfe\xba\xbe");
        fat[7] = 2; // two architectures
        assert_eq!(describe(&fat), "Mach-O universal binary");

        let mut class = vec![0u8; 16];
        class[..4].copy_from_slice(b"\xca\xfe\xba\xbe");
        class[6..8].copy_from_slice(&65u16.to_be_bytes()); // Java 21
        assert_eq!(describe(&class), "Java class data (version 65)");
    }

    #[test]
    fn common_formats_are_named() {
        assert_eq!(describe(b"\x89PNG\r\n\x1a\n\x00\x00"), "PNG image");
        assert_eq!(describe(b"%PDF-1.7"), "PDF document");
        assert_eq!(describe(b"PK\x03\x04rest"), "Zip archive");
        assert_eq!(describe(b"SQLite format 3\x00"), "SQLite 3 database");
        assert_eq!(describe(b"\x00asm\x01\x00\x00\x00"), "WebAssembly module");
    }

    #[test]
    fn riff_and_ftyp_containers_report_their_payload() {
        assert_eq!(describe(b"RIFF\x00\x00\x00\x00WEBPVP8 "), "WebP image");
        assert_eq!(describe(b"\x00\x00\x00\x20ftypavif"), "AVIF image");
    }

    #[test]
    fn a_tar_signature_is_found_at_its_offset() {
        let mut head = vec![b'x'; 512];
        head[257..262].copy_from_slice(b"ustar");
        assert_eq!(describe(&head), "tar archive");
    }

    #[test]
    fn unrecognised_bytes_admit_as_much() {
        assert_eq!(describe(&[0x11, 0x22, 0x33, 0x44, 0x55]), "data");
    }

    #[test]
    fn identification_never_panics_on_a_short_or_empty_head() {
        for len in 0..24 {
            let head = vec![0xffu8; len];
            let _ = describe(&head);
            let _ = kind("x", &head);
        }
        // Prefixes of real signatures are the interesting case: they get past
        // the first bytes of a decoder and then run out of input.
        for prefix in [&b"\x7fELF"[..], b"MZ", b"RIFF", b"\xca\xfe\xba\xbe", b"\xfe\xed\xfa\xcf"] {
            for len in 0..prefix.len() + 1 {
                let _ = describe(&prefix[..len.min(prefix.len())]);
            }
        }
    }

    #[test]
    fn sizes_read_the_way_a_terminal_reports_them() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
    }
}
