//! Datum annotations over the viewport: the bubbles at the ends of the grid lines (the lines
//! themselves are drawn by the renderer, with depth). As Revit's grid heads: a circle with the
//! grid's name, just past each end of the line; a picked grid is drawn over in the accent colour.
//! And the measurements, with their values.

use crate::model::GridLine;
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{alignment, mouse, Color, Point, Rectangle};

/// Bubble radius (logical pixels).
const RADIUS: f32 = 10.0;
/// How close a click must come to a grid line to pick it (logical pixels).
pub const PICK: f32 = 5.0;

pub struct GridOverlay<'a> {
    pub grids: &'a [GridLine],
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
    /// Bubble fill and line colour (from the theme).
    pub fill: Color,
    pub ink: Color,
    /// The picked grid, drawn over in `accent`.
    pub picked: Option<usize>,
    pub accent: Color,
}

fn project(view_proj: &Mat4, size: (f32, f32), world: Vec3) -> Option<Point> {
    project_w(view_proj, size, world).map(|(p, _)| p)
}

/// Where a world point is on screen, and its clip `w` (its depth, for interpolating along a line
/// as seen); `None` behind the camera.
fn project_w(view_proj: &Mat4, size: (f32, f32), world: Vec3) -> Option<(Point, f32)> {
    let clip = *view_proj * world.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some((Point::new((ndc.x + 1.0) * 0.5 * size.0, (1.0 - ndc.y) * 0.5 * size.1), clip.w))
}

/// The grid under screen point `cursor`: inside one of its bubbles (always seen: they are drawn
/// on top), or within `PICK` pixels of its line where `seen` keeps the world point of the line
/// there (something in front of it hides it). A bubble goes before a line, then the nearest.
pub fn grid_at(view_proj: &Mat4, size: (f32, f32), grids: &[GridLine], cursor: Point, seen: impl Fn(Vec3) -> bool) -> Option<usize> {
    let mut best: Option<((u8, f32), usize)> = None;
    for (i, grid) in grids.iter().enumerate() {
        let (wa, wb) = (Vec3::from_array(grid.a), Vec3::from_array(grid.b));
        let (Some((a, za)), Some((b, zb))) = (project_w(view_proj, size, wa), project_w(view_proj, size, wb)) else { continue };
        let bubble = bubble_centres(view_proj, size, grid).and_then(|ends| ends.iter().map(|c| c.distance(cursor)).filter(|d| *d <= RADIUS).min_by(f32::total_cmp));
        let rank = match bubble {
            Some(d) => (0, d),
            None => {
                // the nearest point of the line on screen, and that point on the line in the
                // world (1/w goes linearly over the screen)
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let length2 = dx * dx + dy * dy;
                if length2 < 1.0 {
                    continue;
                }
                let s = (((cursor.x - a.x) * dx + (cursor.y - a.y) * dy) / length2).clamp(0.0, 1.0);
                let d = Point::new(a.x + s * dx, a.y + s * dy).distance(cursor);
                let t = (s / zb) / ((1.0 - s) / za + s / zb);
                if d > PICK || !seen(wa.lerp(wb, t)) {
                    continue;
                }
                (1, d)
            }
        };
        if best.is_none_or(|(r, _)| rank < r) {
            best = Some((rank, i));
        }
    }
    best.map(|(_, i)| i)
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
        // the picked grid last, over the others
        let picked = self.picked.filter(|&g| g < self.grids.len());
        let order = (0..self.grids.len()).filter(|&g| Some(g) != picked).chain(picked);
        for g in order {
            let grid = &self.grids[g];
            let Some(ends) = bubble_centres(&self.view_proj, size, grid) else { continue };
            let (ink, width) = if Some(g) == picked { (self.accent, 2.0) } else { (self.ink, 1.2) };
            if Some(g) == picked {
                // its whole line, over what hides it
                let a = project(&self.view_proj, size, Vec3::from_array(grid.a));
                let b = project(&self.view_proj, size, Vec3::from_array(grid.b));
                if let (Some(a), Some(b)) = (a, b) {
                    frame.stroke(&Path::line(a, b), canvas::Stroke::default().with_color(ink).with_width(width));
                }
            }
            for c in ends {
                if c.x < -RADIUS || c.y < -RADIUS || c.x > size.0 + RADIUS || c.y > size.1 + RADIUS {
                    continue;
                }
                let circle = Path::circle(c, RADIUS);
                frame.fill(&circle, self.fill);
                frame.stroke(&circle, canvas::Stroke::default().with_color(ink).with_width(width));
                frame.fill_text(canvas::Text {
                    content: grid.name.clone(),
                    position: c,
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    size: iced::Pixels(if grid.name.chars().count() > 2 { 8.0 } else { 10.0 }),
                    color: ink,
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

/// A measurement shown in the viewport: a distance (two points) or an angle (three points, the
/// vertex in the middle), world coordinates, with its label.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub points: Vec<Vec3>,
    pub label: String,
}

/// The measurements over the viewport: lines with dots at their points and a label in the middle
/// (at the vertex for an angle).
pub struct MeasureOverlay<'a> {
    pub items: &'a [Measurement],
    pub view_proj: Mat4,
    pub ink: Color,
    pub fill: Color,
    pub text: Color,
}

impl<Message> canvas::Program<Message> for MeasureOverlay<'_> {
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
        for m in self.items {
            let Some(points) = m.points.iter().map(|p| project(&self.view_proj, size, *p)).collect::<Option<Vec<Point>>>() else {
                continue;
            };
            let line = Path::new(|b| {
                b.move_to(points[0]);
                points[1..].iter().for_each(|p| b.line_to(*p));
            });
            frame.stroke(&line, canvas::Stroke::default().with_color(self.ink).with_width(2.0));
            for p in &points {
                frame.fill(&Path::circle(*p, 3.5), self.ink);
            }
            let (first, last) = (points[0], points[points.len() - 1]);
            let at = if points.len() == 3 { points[1] } else { Point::new((first.x + last.x) * 0.5, (first.y + last.y) * 0.5) };
            let width = 7.0 * m.label.chars().count() as f32 + 12.0;
            let label = Path::rounded_rectangle(Point::new(at.x - width * 0.5, at.y - 22.0), iced::Size::new(width, 18.0), 4.0.into());
            frame.fill(&label, self.fill);
            frame.stroke(&label, canvas::Stroke::default().with_color(self.ink).with_width(1.0));
            frame.fill_text(canvas::Text {
                content: m.label.clone(),
                position: Point::new(at.x, at.y - 13.0),
                align_x: alignment::Horizontal::Center.into(),
                align_y: alignment::Vertical::Center,
                size: iced::Pixels(12.0),
                color: self.text,
                font: openaec_ui::fonts::MEDIUM,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &(), _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
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

    /// A click picks a grid in one of its bubbles (always: they are drawn on top) or near its
    /// line where that is seen; the nearest wins.
    #[test]
    fn a_grid_is_taken_by_its_bubble_or_near_its_line() {
        let mut cam = crate::viewport::camera::Camera::default();
        cam.set_preset(crate::viewport::camera::ViewPreset::Top);
        cam.target = Vec3::ZERO;
        cam.distance = 50.0;
        let vp = cam.view_proj(1.0);
        let size = (800.0, 800.0);
        let grids = [
            GridLine { name: "A".into(), a: [-10.0, 0.0, 0.0], b: [10.0, 0.0, 0.0] },
            GridLine { name: "1".into(), a: [0.0, -10.0, 0.0], b: [0.0, 10.0, 0.0] },
        ];
        let at = |x: f32, y: f32| project(&vp, size, Vec3::new(x, y, 0.0)).unwrap();
        let near = |p: Point, dx: f32, dy: f32| Point::new(p.x + dx, p.y + dy);
        let all = |_: Vec3| true;
        // on the line, a few pixels beside it, too far beside it
        assert_eq!(grid_at(&vp, size, &grids, at(-5.0, 0.0), all), Some(0));
        assert_eq!(grid_at(&vp, size, &grids, near(at(-5.0, 0.0), 0.0, PICK - 1.0), all), Some(0));
        assert_eq!(grid_at(&vp, size, &grids, near(at(-5.0, 0.0), 0.0, PICK + 3.0), all), None);
        assert_eq!(grid_at(&vp, size, &grids, near(at(0.0, 6.0), 2.0, 0.0), all), Some(1));
        // where they cross the nearer one wins
        assert_eq!(grid_at(&vp, size, &grids, near(at(0.0, 0.0), 1.0, 3.0), all), Some(1));
        // the line is hidden where something lies in front of it; its bubble is not
        let [_, end] = bubble_centres(&vp, size, &grids[0]).unwrap();
        let hidden = |_: Vec3| false;
        assert_eq!(grid_at(&vp, size, &grids, at(-5.0, 0.0), hidden), None);
        assert_eq!(grid_at(&vp, size, &grids, end, hidden), Some(0));
        assert_eq!(grid_at(&vp, size, &grids, near(end, RADIUS - 2.0, 0.0), hidden), Some(0));
        // the point tested is where the pointer is on the line, in the world
        let left = |w: Vec3| w.x < 0.0;
        assert_eq!(grid_at(&vp, size, &grids, at(-5.0, 0.0), left), Some(0));
        assert_eq!(grid_at(&vp, size, &grids, at(5.0, 0.0), left), None);
    }
}
