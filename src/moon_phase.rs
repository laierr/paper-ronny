/// Select the nearest icon for Open-Meteo's phase fraction; no lunar calculation.
pub fn phase_index(fraction: f64) -> Option<usize> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    Some(((fraction * 8.0 + 0.5).floor() as usize) % 8)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_fraction_to_icon() {
        for i in 0..8 {
            assert_eq!(phase_index(i as f64 / 8.0), Some(i));
        }
        assert_eq!(phase_index(1.0), Some(0));
        assert_eq!(phase_index(0.706), Some(6));
        assert_eq!(phase_index(0.99), Some(0));
    }

    #[test]
    fn invalid_provider_values_are_hidden() {
        for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert_eq!(phase_index(value), None);
        }
    }
}
