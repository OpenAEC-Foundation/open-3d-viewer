//! Box selection (Ctrl + left drag in the viewport): the units whose bounds lie in the box, and
//! the box drawn over the scene while dragging. Dragged to the right it is a window, which takes
//! what lies wholly inside; dragged to the left a crossing, which takes what it touches.

use crate::explode::ExplodeIndex;
use glam::{Mat4, Vec2, Vec3};
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Size};

/// The selection amber (#D97706), as the selection is drawn.
const AMBER: Color = Color::from_rgb(0.851, 0.467, 0.024);

/// The units (1..) whose bounds at rest, moved by their offset, fall in the box from `rect.0` to
/// `rect.1` (viewport pixels of a viewport of `size`), seen through `view_proj` (with the model
/// matrix). Units wholly behind the camera, and those `hidden` says, are never taken.
pub fn units_in_box(index: &ExplodeIndex, offsets: &[[f32; 4]], view_proj: Mat4, size: (f32, f32), rect: (Vec2, Vec2), hidden: &dyn Fn(usize) -> bool) -> Vec<usize> {
    let crossing = rect.1.x < rect.0.x;
    let (lo, hi) = (rect.0.min(rect.1), rect.0.max(rect.1));
    let to_screen = |p: Vec3| -> Option<Vec2> {
        let c = view_proj * p.extend(1.0);
        (c.w > 1e-6).then(|| Vec2::new((c.x / c.w + 1.0) * 0.5 * size.0, (1.0 - c.y / c.w) * 0.5 * size.1))
    };
    (1..index.len())
        .filter(|&u| !hidden(u))
        .filter(|&u| {
            let Some(b) = index.unit_bounds(u as u32) else {
                return false;
            };
            let shift = offsets.get(u).map_or(Vec3::ZERO, |o| Vec3::new(o[0], o[1], o[2]));
            let (min, max) = (Vec3::from(b.min) + shift, Vec3::from(b.max) + shift);
            // the corners on screen, without allocating: 10^5 units are boxed at once
            let (mut smin, mut smax, mut seen) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY), 0);
            for i in 0..8 {
                let corner = Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z });
                if let Some(p) = to_screen(corner) {
                    (smin, smax, seen) = (smin.min(p), smax.max(p), seen + 1);
                }
            }
            if seen == 0 || (!crossing && seen < 8) {
                return false;
            }
            if crossing {
                smax.x >= lo.x && smin.x <= hi.x && smax.y >= lo.y && smin.y <= hi.y
            } else {
                smin.x >= lo.x && smax.x <= hi.x && smin.y >= lo.y && smax.y <= hi.y
            }
        })
        .collect()
}

/// The box while it is dragged: from where the drag started to the cursor (viewport pixels).
pub struct BoxOverlay {
    pub from: Vec2,
    pub to: Vec2,
}

impl<Message> canvas::Program<Message> for BoxOverlay {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (lo, hi) = (self.from.min(self.to), self.from.max(self.to));
        let outline = Path::rectangle(Point::new(lo.x, lo.y), Size::new(hi.x - lo.x, hi.y - lo.y));
        let mut stroke = Stroke::default().with_color(AMBER).with_width(1.0);
        if self.to.x < self.from.x {
            // a crossing: dashed
            stroke.line_dash = canvas::LineDash { segments: &[5.0, 4.0], offset: 0 };
        }
        frame.stroke(&outline, stroke);
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explode::{ExplodeIndex, UnitOwner};
    use crate::model::{Layer, Model};
    use std::path::PathBuf;

    /// Three unit squares in a row at x = 0, 3 and 6 (elements 10, 20 and 30), seen so that x
    /// 0..9 and y 0..1 fill a viewport of 900 × 100: square i covers x 300i..300i + 100.
    fn row() -> (ExplodeIndex, Mat4) {
        let mut positions = Vec::new();
        for x in [0.0, 3.0, 6.0] {
            positions.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
        }
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 8, 9, 10, 8, 10, 11],
            elements: vec![(10, 0, 6), (20, 6, 6), (30, 12, 6)],
            ..Layer::default()
        };
        let model = Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]);
        (ExplodeIndex::new(&model), Mat4::orthographic_rh(0.0, 9.0, 0.0, 1.0, -10.0, 10.0))
    }

    fn ids(index: &ExplodeIndex, units: &[usize]) -> Vec<u64> {
        let mut ids: Vec<u64> = units
            .iter()
            .filter_map(|&u| match index.owner(u) {
                Some(UnitOwner::Element(id)) => Some(id),
                _ => None,
            })
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn window_takes_units_fully_inside() {
        let (index, vp) = row();
        let units = units_in_box(&index, &[], vp, (900.0, 100.0), (Vec2::new(250.0, -10.0), Vec2::new(750.0, 110.0)), &|_| false);
        assert_eq!(ids(&index, &units), vec![20, 30]);
    }

    #[test]
    fn crossing_takes_units_touching() {
        let (index, vp) = row();
        let units = units_in_box(&index, &[], vp, (900.0, 100.0), (Vec2::new(350.0, 50.0), Vec2::new(50.0, 60.0)), &|_| false);
        assert_eq!(ids(&index, &units), vec![10, 20]);
    }

    #[test]
    fn hidden_units_are_not_boxed() {
        let (index, vp) = row();
        let hidden = index.element_unit(20).unwrap();
        let units = units_in_box(&index, &[], vp, (900.0, 100.0), (Vec2::new(-10.0, -10.0), Vec2::new(1000.0, 110.0)), &|u| u == hidden);
        assert_eq!(ids(&index, &units), vec![10, 30]);
    }
}
