/// Inclusive controller window for a landscape rectangle. Rotation makes logical
/// Y the byte-aligned native X axis. Expanded edge pixels must be copied from the
/// existing framebuffer, not cleared, to preserve neighboring artwork.
pub fn controller_window(x: u32, y: u32, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 || x.checked_add(width)? > 416 || y.checked_add(height)? > 240 {
        return None;
    }
    Some(((240 - y - height) & !7, x, (239 - y) | 7, x + width - 1))
}

/// Fixed mosaic: tall glyph, wide text, two squares, two shallow strips.
/// Gutters deliberately sit on unaligned Y edges to exercise preserved bytes.
pub const GRID_CELLS: [(u32, u32, u32, u32); 6] = [
    (4, 4, 128, 232),
    (136, 4, 276, 44),
    (136, 52, 132, 132),
    (272, 52, 140, 132),
    (136, 188, 132, 48),
    (272, 188, 140, 48),
];

pub fn text_patch(
    cell_width: u32,
    cell_height: u32,
    count: u16,
    draw: u32,
) -> (u32, u32, u32, u32) {
    let columns = 416 / cell_width;
    let rows = 240 / cell_height;
    let count = u32::from(count).clamp(1, columns);
    let column = draw.wrapping_mul(17) % (columns - count + 1);
    let row = draw.wrapping_mul(7) % rows;
    (
        column * cell_width,
        row * cell_height,
        count * cell_width,
        cell_height,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Vertical,
    Horizontal,
    Grid,
    Black,
    WhiteGrid,
    White,
    Dither,
    Sparse,
}

pub const PATTERNS: [(Pattern, &str); 14] = [
    (Pattern::Vertical, "vertical lines"),
    (Pattern::Horizontal, "horizontal lines"),
    (Pattern::Grid, "grid"),
    (Pattern::Black, "solid black"),
    (Pattern::WhiteGrid, "white grid on black"),
    (Pattern::Grid, "invert 1"),
    (Pattern::WhiteGrid, "invert 2"),
    (Pattern::White, "solid white"),
    (Pattern::Dither, "50% dither"),
    (Pattern::Black, "black over dither"),
    (Pattern::White, "white after black"),
    (Pattern::Sparse, "sparse shapes"),
    (Pattern::Black, "black over shapes"),
    (Pattern::White, "final white"),
];

pub const PATTERN_DRAWS: u16 = PATTERNS.len() as u16;

pub fn pattern_stage(item: u16) -> (Pattern, &'static str) {
    PATTERNS[item as usize % PATTERNS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_patches_are_character_exact_and_fit() {
        for (cw, ch) in [(6, 10), (10, 20)] {
            for count in [0, 1, 8, 69, 1000] {
                for draw in 0..500 {
                    let (x, y, w, h) = text_patch(cw, ch, count, draw);
                    assert_eq!(x % cw, 0);
                    assert_eq!(y % ch, 0);
                    assert_eq!(w % cw, 0);
                    assert_eq!(h, ch);
                    assert!(controller_window(x, y, w, h).is_some());
                }
            }
        }
    }

    #[test]
    fn pattern_sequence_has_no_identical_consecutive_frames() {
        assert_eq!(PATTERN_DRAWS, 14);
        for i in 1..PATTERN_DRAWS {
            assert_ne!(pattern_stage(i - 1).0, pattern_stage(i).0);
        }
    }

    #[test]
    fn grid_cells_fit_and_never_overlap() {
        for (i, &(x, y, w, h)) in GRID_CELLS.iter().enumerate() {
            assert!(controller_window(x, y, w, h).is_some());
            for &(other_x, other_y, other_w, other_h) in &GRID_CELLS[i + 1..] {
                assert!(
                    x + w <= other_x
                        || other_x + other_w <= x
                        || y + h <= other_y
                        || other_y + other_h <= y
                );
            }
        }
    }

    #[test]
    fn full_and_corner_windows() {
        assert_eq!(controller_window(0, 0, 416, 240), Some((0, 0, 239, 415)));
        assert_eq!(controller_window(415, 239, 1, 1), Some((0, 415, 7, 415)));
        assert_eq!(controller_window(0, 0, 1, 1), Some((232, 0, 239, 0)));
        assert_eq!(controller_window(8, 24, 192, 200), Some((16, 8, 215, 199)));
    }

    #[test]
    fn invalid_rectangles_are_rejected() {
        for r in [
            (0, 0, 0, 1),
            (0, 0, 1, 0),
            (416, 0, 1, 1),
            (0, 240, 1, 1),
            (415, 239, 2, 1),
            (0, 0, u32::MAX, 1),
            (u32::MAX, 0, 2, 1),
        ] {
            assert_eq!(controller_window(r.0, r.1, r.2, r.3), None);
        }
    }

    #[test]
    fn every_vertical_extent_is_minimally_byte_aligned() {
        for y in 0..240 {
            for height in 1..=240 - y {
                let (x0, y0, x1, y1) = controller_window(7, y, 409, height).unwrap();
                assert_eq!(x0 % 8, 0);
                assert_eq!((x1 + 1) % 8, 0);
                assert_eq!((y0, y1), (7, 415));
                let low = 240 - y - height;
                let high = 239 - y;
                assert!(x0 <= low && low - x0 < 8);
                assert!(x1 >= high && x1 - high < 8);
                assert!(x1 < 240);
            }
        }
    }
}
