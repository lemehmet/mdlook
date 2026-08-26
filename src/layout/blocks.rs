//! Pattern → glyph tables for the block image renderer.
//!
//! Each table maps a subpixel bitmask to the character that draws it. Subpixels
//! are numbered row-major from the top-left, and bit `i` set means subpixel `i`
//! is drawn in the foreground colour. That is the same numbering Unicode uses
//! in the character names (`BLOCK OCTANT-1356` has octants 1, 3, 5 and 6 set),
//! which is what made these tables generatable rather than typed.
//!
//! Generated from the UCD's `DerivedName.txt`: every `BLOCK SEXTANT-*` /
//! `BLOCK OCTANT-*` name was decoded into its bitmask, and the patterns Unicode
//! deliberately left out — the ones an older character already draws, like `▌`
//! for the left-column sextants — were filled in by hand and cross-checked
//! against the quadrant table. Do not edit an entry without a source: a wrong
//! glyph here is invisible in review and shows up only as a subtly damaged
//! image in some terminal.
//!
//! Sextants are Unicode 13 (Symbols for Legacy Computing); octants are Unicode
//! 16, and fonts that ship them are still the enthusiast tier. That is why the
//! mode these tables serve is chosen by a keypress the reader can immediately
//! judge the result of, rather than detected.

#[rustfmt::skip]
pub const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

#[rustfmt::skip]
pub const SEXTANTS: [char; 64] = [
    ' ', '🬀', '🬁', '🬂', '🬃', '🬄', '🬅', '🬆', '🬇', '🬈', '🬉', '🬊', '🬋', '🬌', '🬍', '🬎',
    '🬏', '🬐', '🬑', '🬒', '🬓', '▌', '🬔', '🬕', '🬖', '🬗', '🬘', '🬙', '🬚', '🬛', '🬜', '🬝',
    '🬞', '🬟', '🬠', '🬡', '🬢', '🬣', '🬤', '🬥', '🬦', '🬧', '▐', '🬨', '🬩', '🬪', '🬫', '🬬',
    '🬭', '🬮', '🬯', '🬰', '🬱', '🬲', '🬳', '🬴', '🬵', '🬶', '🬷', '🬸', '🬹', '🬺', '🬻', '█',
];

#[rustfmt::skip]
pub const OCTANTS: [char; 256] = [
    ' ', '𜺨', '𜺫', '🮂', '𜴀', '▘', '𜴁', '𜴂',
    '𜴃', '𜴄', '▝', '𜴅', '𜴆', '𜴇', '𜴈', '▀',
    '𜴉', '𜴊', '𜴋', '𜴌', '🯦', '𜴍', '𜴎', '𜴏',
    '𜴐', '𜴑', '𜴒', '𜴓', '𜴔', '𜴕', '𜴖', '𜴗',
    '𜴘', '𜴙', '𜴚', '𜴛', '𜴜', '𜴝', '𜴞', '𜴟',
    '🯧', '𜴠', '𜴡', '𜴢', '𜴣', '𜴤', '𜴥', '𜴦',
    '𜴧', '𜴨', '𜴩', '𜴪', '𜴫', '𜴬', '𜴭', '𜴮',
    '𜴯', '𜴰', '𜴱', '𜴲', '𜴳', '𜴴', '𜴵', '🮅',
    '𜺣', '𜴶', '𜴷', '𜴸', '𜴹', '𜴺', '𜴻', '𜴼',
    '𜴽', '𜴾', '𜴿', '𜵀', '𜵁', '𜵂', '𜵃', '𜵄',
    '▖', '𜵅', '𜵆', '𜵇', '𜵈', '▌', '𜵉', '𜵊',
    '𜵋', '𜵌', '▞', '𜵍', '𜵎', '𜵏', '𜵐', '▛',
    '𜵑', '𜵒', '𜵓', '𜵔', '𜵕', '𜵖', '𜵗', '𜵘',
    '𜵙', '𜵚', '𜵛', '𜵜', '𜵝', '𜵞', '𜵟', '𜵠',
    '𜵡', '𜵢', '𜵣', '𜵤', '𜵥', '𜵦', '𜵧', '𜵨',
    '𜵩', '𜵪', '𜵫', '𜵬', '𜵭', '𜵮', '𜵯', '𜵰',
    '𜺠', '𜵱', '𜵲', '𜵳', '𜵴', '𜵵', '𜵶', '𜵷',
    '𜵸', '𜵹', '𜵺', '𜵻', '𜵼', '𜵽', '𜵾', '𜵿',
    '𜶀', '𜶁', '𜶂', '𜶃', '𜶄', '𜶅', '𜶆', '𜶇',
    '𜶈', '𜶉', '𜶊', '𜶋', '𜶌', '𜶍', '𜶎', '𜶏',
    '▗', '𜶐', '𜶑', '𜶒', '𜶓', '▚', '𜶔', '𜶕',
    '𜶖', '𜶗', '▐', '𜶘', '𜶙', '𜶚', '𜶛', '▜',
    '𜶜', '𜶝', '𜶞', '𜶟', '𜶠', '𜶡', '𜶢', '𜶣',
    '𜶤', '𜶥', '𜶦', '𜶧', '𜶨', '𜶩', '𜶪', '𜶫',
    '▂', '𜶬', '𜶭', '𜶮', '𜶯', '𜶰', '𜶱', '𜶲',
    '𜶳', '𜶴', '𜶵', '𜶶', '𜶷', '𜶸', '𜶹', '𜶺',
    '𜶻', '𜶼', '𜶽', '𜶾', '𜶿', '𜷀', '𜷁', '𜷂',
    '𜷃', '𜷄', '𜷅', '𜷆', '𜷇', '𜷈', '𜷉', '𜷊',
    '𜷋', '𜷌', '𜷍', '𜷎', '𜷏', '𜷐', '𜷑', '𜷒',
    '𜷓', '𜷔', '𜷕', '𜷖', '𜷗', '𜷘', '𜷙', '𜷚',
    '▄', '𜷛', '𜷜', '𜷝', '𜷞', '▙', '𜷟', '𜷠',
    '𜷡', '𜷢', '▟', '𜷣', '▆', '𜷤', '𜷥', '█',
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_has_its_own_glyph() {
        // A duplicate would mean two different subpixel patterns draw the same
        // shape, which quietly loses detail.
        for table in [&QUADRANTS[..], &SEXTANTS[..], &OCTANTS[..]] {
            let mut seen: Vec<char> = table.to_vec();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), table.len());
        }
    }

    #[test]
    fn all_tables_agree_on_the_extremes() {
        for table in [&QUADRANTS[..], &SEXTANTS[..], &OCTANTS[..]] {
            assert_eq!(table[0], ' ');
            assert_eq!(*table.last().unwrap(), '█');
        }
    }

    /// The patterns Unicode left out of the sextant and octant blocks are the
    /// ones an older character already draws; check the fills went to the right
    /// slots. Each mask below is the row-major bitmask of the glyph's shape.
    #[test]
    fn borrowed_glyphs_sit_at_their_own_patterns() {
        assert_eq!(SEXTANTS[0b010101], '▌', "left column of a 2×3 grid");
        assert_eq!(SEXTANTS[0b101010], '▐', "right column of a 2×3 grid");

        assert_eq!(OCTANTS[0x0F], '▀', "top two of four rows");
        assert_eq!(OCTANTS[0xF0], '▄');
        assert_eq!(OCTANTS[0x55], '▌', "left column of a 2×4 grid");
        assert_eq!(OCTANTS[0xAA], '▐');
        assert_eq!(OCTANTS[0x05], '▘');
        assert_eq!(OCTANTS[0xFA], '▟');
        assert_eq!(OCTANTS[0x03], '🮂', "upper one quarter");
        assert_eq!(OCTANTS[0xC0], '▂', "lower one quarter");
    }

    /// Every quadrant pattern, scaled to octant resolution, must pick the same
    /// glyph from both tables — the octant table's borrowed characters are the
    /// quadrant characters.
    #[test]
    fn the_octant_table_embeds_the_quadrant_table() {
        for (pattern, &glyph) in QUADRANTS.iter().enumerate() {
            let mut scaled = 0usize;
            for bit in 0..4 {
                if pattern & (1 << bit) != 0 {
                    let (col, row) = (bit % 2, bit / 2);
                    // A quadrant row spans two octant rows.
                    scaled |= 1 << (2 * (2 * row) + col);
                    scaled |= 1 << (2 * (2 * row + 1) + col);
                }
            }
            assert_eq!(OCTANTS[scaled], glyph, "quadrant pattern {pattern:04b}");
        }
    }
}
