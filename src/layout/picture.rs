//! Layout for image files: pixels become coloured block characters.
//!
//! Same contract as every other producer here — a pure function of its inputs
//! ending in a [`RenderedDoc`] — so scrolling, the status bar and the `--plain`
//! writer work on images without knowing they are images. No terminal graphics
//! protocol is involved: the picture is made of ordinary styled characters,
//! which is what lets it scroll like text and survive any terminal ratatui
//! runs in.
//!
//! A cell has one foreground and one background colour however many subpixels
//! its glyph carves it into, so every mode above half-blocks is a lossy
//! two-colour quantisation per cell: the subpixels are split into two clusters
//! along the channel that varies most, each cluster averaged into a colour, and
//! the membership bitmap picks the glyph. Half-blocks are the one mode where
//! nothing is lost — two subpixels, two colours — which is why they are the
//! default; the denser modes trade colour fidelity for edge sharpness and need
//! a font that ships the glyphs, a fact no terminal will report. Hence
//! [`BlockMode`] is cycled by a keypress rather than detected: the reader's
//! eyes are the capability query.

use image::RgbaImage;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use serde::Deserialize;

use super::blocks;
use super::theme::{Theme, ThemeKind};
use super::{RenderedDoc, Sink};
use crate::files::detect::human_size;

/// How tall a terminal cell is relative to its width, in most fonts. Used to
/// keep a rendered image's proportions right: the subpixel grids are not
/// square, so the mapping from pixels to cells must account for the cell shape
/// or quadrant mode would draw everything twice as tall as it is.
const CELL_ASPECT: f64 = 2.0;

/// Which subpixel grid draws the image.
///
/// Ordered coarsest to finest, which is the order the cycle key walks. The
/// finer grids resolve more shape but share the same two colours per cell, so
/// they are a trade rather than an upgrade — and past `Quadrant` they need
/// fonts most systems do not ship yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockMode {
    #[default]
    Half,
    Quadrant,
    Sextant,
    Octant,
}

impl BlockMode {
    /// The next mode in the cycle, wrapping.
    pub fn next(self) -> Self {
        match self {
            Self::Half => Self::Quadrant,
            Self::Quadrant => Self::Sextant,
            Self::Sextant => Self::Octant,
            Self::Octant => Self::Half,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::Quadrant => "quadrant",
            Self::Sextant => "sextant",
            Self::Octant => "octant",
        }
    }

    /// Subpixels per cell, as (columns, rows).
    fn grid(self) -> (u32, u32) {
        match self {
            Self::Half => (1, 2),
            Self::Quadrant => (2, 2),
            Self::Sextant => (2, 3),
            Self::Octant => (2, 4),
        }
    }

    fn glyphs(self) -> &'static [char] {
        // Half-blocks are the top halves of the quadrant patterns: bit 0 is
        // the upper subpixel, bit 1 the lower.
        const HALVES: [char; 4] = [' ', '▀', '▄', '█'];
        match self {
            Self::Half => &HALVES,
            Self::Quadrant => &blocks::QUADRANTS,
            Self::Sextant => &blocks::SEXTANTS,
            Self::Octant => &blocks::OCTANTS,
        }
    }
}

/// What the caption above the picture says about the file.
#[derive(Clone, Copy)]
pub struct Caption<'a> {
    pub name: &'a str,
    /// e.g. `"PNG image"`.
    pub format: &'a str,
    /// File size on disk.
    pub size: u64,
}

/// Lay an image out as block characters, headed by what the file is.
///
/// `width` and `rows` describe the pane in cells; `rows` is `None` when there
/// is no pane to fit — the piped path — in which case the image fits the width
/// alone and takes however many lines that needs.
pub fn picture(
    caption: Caption,
    image: &RgbaImage,
    mode: BlockMode,
    width: usize,
    rows: Option<usize>,
    theme: &Theme,
) -> RenderedDoc {
    let mut sink = Sink::new(width.max(8), theme);

    let info = format!(
        "{}  ·  {}×{}  ·  {}",
        caption.format,
        image.width(),
        image.height(),
        human_size(caption.size)
    );
    let header = [
        vec![Span::styled(caption.name.to_string(), theme.heading(1))],
        Vec::new(),
        vec![Span::styled(info, theme.popup_dim)],
        Vec::new(),
    ];
    let header_rows = header.len();
    for spans in header {
        sink.push(super::wrap::truncate(spans, width.max(8)));
    }

    if image.width() == 0 || image.height() == 0 {
        sink.push(vec![Span::styled("(empty image)", theme.popup_dim)]);
        return sink.finish();
    }

    // The image gets whatever the header left, but never less than one row:
    // a pane too short to fit is a reason to scroll, not to vanish.
    let rows = rows.map(|r| r.saturating_sub(header_rows).max(1));
    let (cells_w, cells_h) = fit(image.width(), image.height(), mode, sink.width, rows);

    let (sub_w, sub_h) = mode.grid();
    let scaled = image::imageops::resize(
        image,
        cells_w as u32 * sub_w,
        cells_h as u32 * sub_h,
        image::imageops::FilterType::Triangle,
    );

    // Transparency has to become some colour, and the honest choice is the
    // page the image sits on: the theme's side of light versus dark.
    let matte: [u8; 3] = match theme.kind {
        ThemeKind::Light => [255, 255, 255],
        ThemeKind::Dark | ThemeKind::Mono => [0, 0, 0],
    };

    let glyphs = mode.glyphs();
    let mut subpixels = vec![[0u8; 3]; (sub_w * sub_h) as usize];
    for cell_y in 0..cells_h {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut run = String::new();
        let mut run_style = Style::new();
        for cell_x in 0..cells_w {
            for sy in 0..sub_h {
                for sx in 0..sub_w {
                    let px =
                        scaled.get_pixel(cell_x as u32 * sub_w + sx, cell_y as u32 * sub_h + sy).0;
                    subpixels[(sy * sub_w + sx) as usize] = composite(px, matte);
                }
            }
            let (mask, fg, bg) = split(&subpixels);
            let style = Style::new().fg(fg).bg(bg);
            if style != run_style && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), run_style));
            }
            run_style = style;
            run.push(glyphs[mask]);
        }
        spans.push(Span::styled(run, run_style));
        sink.push(spans);
    }

    let mut doc = sink.finish();
    // The status bar's line about the image. Built here because only this
    // function knows what the fit actually chose.
    doc.status =
        Some(format!("{}  {}×{} px", mode.label(), cells_w as u32 * sub_w, cells_h as u32 * sub_h));
    doc
}

/// The drawing's size in cells: as large as the pane allows, aspect preserved,
/// never asking for more subpixels than the file has pixels.
///
/// Worked in physical units — a cell is 1 wide and [`CELL_ASPECT`] tall — so
/// the answer is right for every grid: the same photo fills the same area of
/// the screen in every mode, and the finer grids spend their extra subpixels
/// on detail rather than on size.
fn fit(px_w: u32, px_h: u32, mode: BlockMode, cols: usize, rows: Option<usize>) -> (usize, usize) {
    let (sub_w, sub_h) = mode.grid();
    let (w, h) = (px_w as f64, px_h as f64);

    // Physical units per image pixel. Each bound is a reason not to grow:
    // the pane's width, the pane's height, and the file's own resolution in
    // each direction (rendering more subpixels than pixels invents detail).
    let mut scale = (cols as f64 / w).min(1.0 / sub_w as f64).min(CELL_ASPECT / sub_h as f64);
    if let Some(rows) = rows {
        scale = scale.min(rows as f64 * CELL_ASPECT / h);
    }

    let cells_w = (scale * w).round().max(1.0) as usize;
    let cells_h = (scale * h / CELL_ASPECT).round().max(1.0) as usize;
    (cells_w.min(cols), cells_h.min(rows.unwrap_or(cells_h)))
}

/// Alpha-composite a pixel over the matte.
fn composite(px: [u8; 4], matte: [u8; 3]) -> [u8; 3] {
    let a = px[3] as u16;
    std::array::from_fn(|i| ((px[i] as u16 * a + matte[i] as u16 * (255 - a)) / 255) as u8)
}

/// Split a cell's subpixels into two colours and the bitmap that picks a glyph.
///
/// The split is a threshold on whichever channel varies most inside the cell —
/// cheap, and for the handful of pixels a cell holds, close enough to a real
/// 2-means that the difference never survives being drawn 8 pixels tall. Bit
/// `i` of the mask says subpixel `i` took the foreground colour.
fn split(pixels: &[[u8; 3]]) -> (usize, Color, Color) {
    let channel = (0..3)
        .max_by_key(|&c| {
            let (min, max) = min_max(pixels, c);
            max - min
        })
        .unwrap_or(0);
    let (min, max) = min_max(pixels, channel);
    if min == max {
        // A flat cell: background paints it all, the glyph is a space.
        let bg = rgb(mean(pixels, |_| true));
        return (0, bg, bg);
    }

    let threshold = (min + max) / 2;
    let mut mask = 0usize;
    for (i, px) in pixels.iter().enumerate() {
        if px[channel] as u16 > threshold {
            mask |= 1 << i;
        }
    }
    let fg = mean(pixels, |i| mask & (1 << i) != 0);
    let bg = mean(pixels, |i| mask & (1 << i) == 0);
    (mask, rgb(fg), rgb(bg))
}

fn min_max(pixels: &[[u8; 3]], channel: usize) -> (u16, u16) {
    pixels.iter().fold((u16::MAX, 0), |(min, max), px| {
        let v = px[channel] as u16;
        (min.min(v), max.max(v))
    })
}

fn mean(pixels: &[[u8; 3]], select: impl Fn(usize) -> bool) -> [u8; 3] {
    let mut sum = [0u32; 3];
    let mut n = 0u32;
    for (i, px) in pixels.iter().enumerate() {
        if select(i) {
            for (s, &v) in sum.iter_mut().zip(px) {
                *s += v as u32;
            }
            n += 1;
        }
    }
    if n == 0 {
        return [0; 3];
    }
    std::array::from_fn(|i| (sum[i] / n) as u8)
}

fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(color))
    }

    fn pic(
        image: &RgbaImage,
        mode: BlockMode,
        width: usize,
        rows: Option<usize>,
        theme: &Theme,
    ) -> RenderedDoc {
        let caption = Caption { name: "a.png", format: "PNG image", size: 1 };
        picture(caption, image, mode, width, rows, theme)
    }

    #[test]
    fn half_blocks_lose_nothing() {
        // Two stacked pixels, two colours: the cell must paint the top red and
        // the bottom blue exactly. Which of '▀'/'▄' carries which colour is the
        // splitter's business — the two are complements — so assert the paint,
        // not the glyph.
        let (red, blue) = (Color::Rgb(200, 10, 10), Color::Rgb(10, 10, 200));
        let (mask, fg, bg) = split(&[[200, 10, 10], [10, 10, 200]]);
        let (top, bottom) = if mask & 1 != 0 { (fg, bg) } else { (bg, fg) };
        assert_eq!(top, red);
        assert_eq!(bottom, blue);
        assert!(matches!(BlockMode::Half.glyphs()[mask], '▀' | '▄'));
    }

    #[test]
    fn a_flat_cell_is_painted_by_its_background() {
        let (mask, fg, bg) = split(&[[7, 7, 7]; 4]);
        assert_eq!(mask, 0, "space glyph, so the terminal draws only bg");
        assert_eq!(fg, bg);
    }

    #[test]
    fn a_quadrant_checkerboard_recovers_its_pattern() {
        // Bright top-left and bottom-right on a dark field: the mask must be
        // exactly those two bits, which is the '▚' glyph.
        let cell = [[250, 250, 250], [5, 5, 5], [5, 5, 5], [250, 250, 250]];
        let (mask, fg, bg) = split(&cell);
        assert_eq!(BlockMode::Quadrant.glyphs()[mask], '▚');
        assert_eq!(fg, Color::Rgb(250, 250, 250));
        assert_eq!(bg, Color::Rgb(5, 5, 5));
    }

    #[test]
    fn every_mode_fills_the_same_physical_area() {
        // A 200×100 image in an 80×24 pane: whatever the grid, the picture
        // should come out the same size on screen, because the finer grids buy
        // detail, not real estate.
        let sizes: Vec<(usize, usize)> =
            [BlockMode::Half, BlockMode::Quadrant, BlockMode::Sextant, BlockMode::Octant]
                .iter()
                .map(|&m| fit(2000, 1000, m, 80, Some(24)))
                .collect();
        for pair in sizes.windows(2) {
            assert_eq!(pair[0], pair[1], "modes disagreed on size: {sizes:?}");
        }
        // Width-limited: 80 cells wide, and a 2:1 image over 80 cell-widths is
        // 40 cell-widths tall, which is 20 rows at the 2:1 cell aspect.
        assert_eq!(sizes[0], (80, 20));
    }

    #[test]
    fn small_images_are_not_upscaled() {
        // 10×10 pixels in half mode is 10 columns of 1 subpixel: growing it
        // further would invent pixels.
        assert_eq!(fit(10, 10, BlockMode::Half, 80, Some(24)), (10, 5));
        // The finer grids show the same 10 pixels in fewer, denser cells.
        assert_eq!(fit(10, 10, BlockMode::Octant, 80, Some(24)), (5, 3));
    }

    #[test]
    fn no_row_exceeds_the_pane_width() {
        let image = solid(500, 20, [90, 120, 30, 255]);
        for width in 8..60 {
            let doc = pic(&image, BlockMode::Half, width, None, &Theme::default());
            for line in &doc.plain {
                assert!(
                    crate::layout::wrap::text_width(line) <= width,
                    "width {width} overflowed: {} cells",
                    crate::layout::wrap::text_width(line)
                );
            }
        }
    }

    #[test]
    fn the_pane_height_is_respected_when_given() {
        let image = solid(100, 4000, [10, 10, 10, 255]);
        let doc = pic(&image, BlockMode::Half, 80, Some(20), &Theme::default());
        assert!(doc.len() <= 20, "got {} lines for a 20-row pane", doc.len());
        // And without a pane, a tall image simply takes many lines.
        let doc = pic(&image, BlockMode::Half, 80, None, &Theme::default());
        assert!(doc.len() > 20);
    }

    #[test]
    fn transparency_takes_the_theme_side() {
        let image = solid(4, 4, [0, 0, 0, 0]);
        let doc = pic(&image, BlockMode::Half, 20, None, &Theme::default());
        let cell = doc.lines.last().unwrap().spans.last().unwrap();
        assert_eq!(cell.style.bg, Some(Color::Rgb(0, 0, 0)), "dark theme mattes to black");

        let light = Theme::new(ThemeKind::Light);
        let doc = pic(&image, BlockMode::Half, 20, None, &light);
        let cell = doc.lines.last().unwrap().spans.last().unwrap();
        assert_eq!(cell.style.bg, Some(Color::Rgb(255, 255, 255)));
    }

    #[test]
    fn the_status_line_reports_the_mode_and_rendered_size() {
        let image = solid(64, 64, [1, 2, 3, 255]);
        let doc = pic(&image, BlockMode::Sextant, 80, Some(40), &Theme::default());
        let status = doc.status.as_deref().unwrap();
        assert!(status.starts_with("sextant"), "{status}");
        assert!(status.ends_with("px"), "{status}");
    }

    #[test]
    fn layout_is_pure() {
        let image = solid(37, 23, [200, 100, 50, 255]);
        let theme = Theme::default();
        let a = pic(&image, BlockMode::Octant, 40, Some(20), &theme);
        let b = pic(&image, BlockMode::Octant, 40, Some(20), &theme);
        assert_eq!(a.plain, b.plain);
    }
}
