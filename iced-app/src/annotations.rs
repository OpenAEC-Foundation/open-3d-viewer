//! Datum annotations over the viewport: the bubbles at the ends of the grid lines (the lines
//! themselves are drawn by the renderer, with depth). As Revit's grid heads: a circle with the
//! grid's name, just past each end of the line.

use crate::model::GridLine;
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{alignment, mouse, Color, Point, Rectangle};

/// Bubble radius (logical pixels).
const RADIUS: f32 = 10.0;

pub struct GridOverlay<'a> {
    pub grids: &'a [GridLine],
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
    /// Bubble fill and line colour (from the theme).
    pub fill: Color,
    pub ink: Color,
}

fn project(view_proj: &Mat4, size: (f32, f32), world: Vec3) -> Option<Point> {
    let clip = *view_proj * world.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Point::new((ndc.x + 1.0) * 0.5 * size.0, (1.0 - ndc.y) * 0.5 * size.1))
}

/// Where the two bubbles of a grid line go on screen: just past both ends, along the line.
pub fn bubble_centres(view_proj: &Mat4, size: (f32, f32), grid: &GridLine) -> Option<[Point; 2]> {
    let a = project(view_proj, size, Vec3::from_array(grid.a))?;
    let b = project(view_proj, size, Vec3::from_array(grid.b))?;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return None;
    }
    let (ux, uy) = (dx / len * RADIUS, dy / len * RADIUS);
    Some([Point::new(a.x - ux, a.y - uy), Point::new(b.x + ux, b.y + uy)])
}

impl<Message> canvas::Program<Message> for GridOverlay<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = (bounds.width, bounds.height);
        for grid in self.grids {
            let Some(ends) = bubble_centres(&self.view_proj, size, grid) else { continue };
            for c in ends {
                if c.x < -RADIUS || c.y < -RADIUS || c.x > size.0 + RADIUS || c.y > size.1 + RADIUS {
                    continue;
                }
                let circle = Path::circle(c, RADIUS);
                frame.fill(&circle, self.fill);
                frame.stroke(&circle, canvas::Stroke::default().with_color(self.ink).with_width(1.2));
                frame.fill_text(canvas::Text {
                    content: grid.name.clone(),
                    position: c,
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    size: iced::Pixels(if grid.name.chars().count() > 2 { 8.0 } else { 10.0 }),
                    color: self.ink,
                    font: openaec_ui::fonts::MEDIUM,
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &(), _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        // only drawn: the viewport below handles the pointer
        mouse::Interaction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubbles_sit_past_both_ends() {
        let mut cam = crate::viewport::camera::Camera::default();
        cam.set_preset(crate::viewport::camera::ViewPreset::Top);
        cam.target = Vec3::ZERO;
        cam.distance = 50.0;
        let vp = cam.view_proj(1.0);
        let grid = GridLine { name: "A".into(), a: [-10.0, 0.0, 0.0], b: [10.0, 0.0, 0.0] };
        let [s, e] = bubble_centres(&vp, (800.0, 800.0), &grid).expect("on screen");
        let a = project(&vp, (800.0, 800.0), Vec3::new(-10.0, 0.0, 0.0)).unwrap();
        let b = project(&vp, (800.0, 800.0), Vec3::new(10.0, 0.0, 0.0)).unwrap();
        assert!((s.distance(a) - RADIUS).abs() < 0.01 && (e.distance(b) - RADIUS).abs() < 0.01);
        assert!(s.x < a.x && e.x > b.x, "outside the line: {s:?} {a:?} {b:?} {e:?}");
    }
}
