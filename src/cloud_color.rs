//! How the points of a cloud are coloured in the 3D view.
//!
//! A point is stored with the colour its file gives it (or, without one, the colour of its class
//! or scan shaded by what it reflected) and, in the fourth byte of that colour, its intensity
//! from 0 to 255 (255 where the file has none). The other ways to colour a cloud are worked out
//! of that when its points are sent to the graphics card, so switching costs no memory.

/// A way to colour the points of a cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudColor {
    /// As the file stores it: its colours, or its classes or scans shaded by intensity.
    #[default]
    Stored,
    /// By height, blue low to red high.
    Elevation,
    /// By what a point reflected, dark to light.
    Intensity,
    /// One colour per class (a classified cloud) or per scan.
    Class,
}

impl CloudColor {
    pub const ALL: [CloudColor; 4] = [CloudColor::Stored, CloudColor::Elevation, CloudColor::Intensity, CloudColor::Class];

    /// The colour of a point: `stored` as its layer holds it, `height` from 0 (the lowest point of
    /// its cloud) to 1, `base` the colour of its layer.
    pub fn of(self, stored: [u8; 4], height: f32, base: [u8; 4]) -> [u8; 4] {
        let [r, g, b] = match self {
            CloudColor::Stored => [stored[0], stored[1], stored[2]],
            CloudColor::Elevation => ramp(height),
            CloudColor::Intensity => [stored[3]; 3],
            CloudColor::Class => [base[0], base[1], base[2]],
        };
        [r, g, b, 255]
    }
}

/// A muted blue, green, yellow, red ramp over 0 to 1.
pub fn ramp(t: f32) -> [u8; 3] {
    const RAMP: [[f32; 3]; 4] = [[0.23, 0.42, 0.78], [0.35, 0.72, 0.45], [0.93, 0.80, 0.30], [0.86, 0.32, 0.22]];
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 } * (RAMP.len() - 1) as f32;
    let k = (t as usize).min(RAMP.len() - 2);
    let f = t - k as f32;
    [0, 1, 2].map(|i| ((RAMP[k][i] * (1.0 - f) + RAMP[k + 1][i] * f) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_coloured_in_each_way() {
        let (stored, base) = ([10, 20, 30, 200], [1, 2, 3, 255]);
        assert_eq!(CloudColor::Stored.of(stored, 0.5, base), [10, 20, 30, 255]);
        assert_eq!(CloudColor::Intensity.of(stored, 0.5, base), [200, 200, 200, 255]);
        assert_eq!(CloudColor::Class.of(stored, 0.5, base), [1, 2, 3, 255]);
        // low is blue, high is red
        let (low, high) = (CloudColor::Elevation.of(stored, 0.0, base), CloudColor::Elevation.of(stored, 1.0, base));
        assert!(low[2] > low[0] && high[0] > high[2], "{low:?} {high:?}");
        assert_eq!(ramp(f32::NAN), ramp(0.0));
    }
}
