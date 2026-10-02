#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleanup {
    White,
    Scrub,
}

#[derive(Default)]
pub struct HourlyCleanup {
    latest_hour: Option<i64>,
}

impl HourlyCleanup {
    /// Local calendar-hour index (not uptime). First valid time arms the clock;
    /// forward hour crossings run one cleanup, backward/repeated DST hours don't.
    pub fn observe(&mut self, local_hour: i64) -> Option<Cleanup> {
        match self.latest_hour {
            None => {
                self.latest_hour = Some(local_hour);
                None
            }
            Some(previous) if local_hour > previous => {
                self.latest_hour = Some(local_hour);
                Some(if local_hour.rem_euclid(24) % 6 == 0 {
                    Cleanup::Scrub
                } else {
                    Cleanup::White
                })
            }
            _ => None,
        }
    }
}

/// Native framebuffer byte bounding box. Preserve all pixels in expanded bytes
/// by sending the corresponding bytes from the complete new framebuffer.
pub fn dirty_window(previous: &[u8], next: &[u8]) -> Option<(u32, u32, u32, u32)> {
    assert_eq!(previous.len(), 240 * 416 / 8);
    assert_eq!(previous.len(), next.len());
    let (mut left, mut top, mut right, mut bottom) = (30, 416, 0, 0);
    for (index, (a, b)) in previous.iter().zip(next).enumerate() {
        if a != b {
            let (x, y) = (index % 30, index / 30);
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    (top < 416).then(|| {
        (
            (left * 8) as u32,
            top as u32,
            (right * 8 + 7) as u32,
            bottom as u32,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_hours_and_six_hour_boundaries() {
        let mut timer = HourlyCleanup::default();
        assert_eq!(timer.observe(5), None);
        assert_eq!(timer.observe(5), None);
        assert_eq!(timer.observe(6), Some(Cleanup::Scrub));
        assert_eq!(timer.observe(7), Some(Cleanup::White));
        assert_eq!(timer.observe(12), Some(Cleanup::Scrub));
        assert_eq!(timer.observe(18), Some(Cleanup::Scrub));
        assert_eq!(timer.observe(24), Some(Cleanup::Scrub));
        assert_eq!(timer.observe(23), None);
        assert_eq!(timer.observe(24), None);
        assert_eq!(timer.observe(25), Some(Cleanup::White));
    }
    #[test]
    fn changed_bytes_only() {
        let old = vec![255; 12480];
        let mut new = old.clone();
        assert_eq!(dirty_window(&old, &new), None);
        new[0] = 0;
        assert_eq!(dirty_window(&old, &new), Some((0, 0, 7, 0)));
        new[12479] = 0;
        assert_eq!(dirty_window(&old, &new), Some((0, 0, 239, 415)));
        new[0] = 255;
        assert_eq!(dirty_window(&old, &new), Some((232, 415, 239, 415)));
    }
}
