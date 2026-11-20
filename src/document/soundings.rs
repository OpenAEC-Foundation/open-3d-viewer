//! Site plan symbols from measured cone penetration test coordinates in the plan presentation.
use super::{Align, Note, PlanView, NOTE_HEIGHT};
use bim_gis::{
    rd::{Frame, Rd},
    sounding::Sounding,
};
use std::collections::HashSet;

impl PlanView {
    pub(super) fn append_sounding_marks(&mut self) {
        let presentation = &self.presentation;
        let Some(soundings) = presentation["soundings"].as_array() else {
            return;
        };
        if presentation["crs"] != "EPSG:28992" || presentation["rd_unit"] != "metre" {
            return;
        }
        let Ok([x, y]) = serde_json::from_value::<[f64; 2]>(presentation["rd_origin"].clone())
        else {
            return;
        };
        let rotation = match presentation.get("rd_rotation_degrees") {
            Some(v) => match v.as_f64() {
                Some(r) => r,
                None => return,
            },
            None => 0.0,
        };
        let frame = Frame {
            origin: Rd::new(x, y),
            rotation,
        };
        let radius = self.scale * 0.002;
        let mut seen = HashSet::new();
        for value in soundings {
            let Ok(sounding) = serde_json::from_value::<Sounding>(value.clone()) else {
                continue;
            };
            let Some([x, y]) = sounding.local(frame) else {
                continue;
            };
            if !seen.insert(sounding.id.trim().to_owned()) {
                continue;
            }
            if !has_cross(&self.lines, [x, y]) {
                let point = |i: usize| {
                    let a = i as f64 * std::f64::consts::TAU / 32.;
                    [x + radius * a.cos(), y + radius * a.sin()]
                };
                self.symbols
                    .extend((0..32).map(|i| [point(i), point(i + 1)]));
                self.symbols.extend([
                    [[x - radius * 1.35, y], [x + radius * 1.35, y]],
                    [[x, y - radius * 1.35], [x, y + radius * 1.35]],
                ]);
            }
            let authored = self.notes.iter().any(|n| {
                n.text.trim() == sounding.id.trim()
                    && (n.at[0] - x).hypot(n.at[1] - y) <= self.scale * 0.02
            });
            if !authored {
                self.tags.push(Note {
                    at: [x + radius * 1.65, y - radius * 0.5],
                    text: sounding.id,
                    size: NOTE_HEIGHT,
                    angle: 0.,
                    align: Align::Left,
                    face: Default::default(),
                    level: false,
                });
            }
        }
    }
}

/// An authored survey cross has two orthogonal arms centred on the same measured point.
fn has_cross(lines: &[[[f64; 2]; 2]], [x, y]: [f64; 2]) -> bool {
    let (mut horizontal, mut vertical) = (false, false);
    for &[a, b] in lines {
        if ((a[0] + b[0]) * 0.5 - x).abs() > 0.001 || ((a[1] + b[1]) * 0.5 - y).abs() > 0.001 {
            continue;
        }
        let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
        horizontal |= dx > 0.001 && dy < 0.001;
        vertical |= dy > 0.001 && dx < 0.001;
        if horizontal && vertical {
            return true;
        }
    }
    false
}
