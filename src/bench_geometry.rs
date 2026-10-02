/// Inclusive controller window for a landscape rectangle. Rotation makes logical
/// Y the byte-aligned native X axis. Expanded edge pixels must be copied from the
/// existing framebuffer, not cleared, to preserve neighboring artwork.
pub fn controller_window(x: u32, y: u32, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 || x.checked_add(width)? > 416 || y.checked_add(height)? > 240 {
        return None;
    }
    Some(((240 - y - height) & !7, x, (239 - y) | 7, x + width - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

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
