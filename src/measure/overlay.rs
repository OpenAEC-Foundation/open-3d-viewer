//! Draws the measurements over the viewport: lines with their value on a label, the arc of an
//! angle, the outline and a light fill of a measured face or polygon, and the picks of the
//! measurement in progress. Drawn only: the viewport below handles the pointer.

use super::format::UnitSettings;
use super::geometry::{Angle, Entity};
use super::{arc_radius, Measurement, Snap};
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::{alignment, mouse, Color, Point, Rectangle, Size};
use openaec_ui::Language;

/// Faces with more triangles than this are drawn as outline only.
const MAX_FILL_TRIANGLES: usize = 20_000;

pub struct MeasureOverlay<'a> {
    pub results: &'a [Measurement],
    pub picks: &'a [Snap],
    pub units: UnitSettings,
    pub language: Language,
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
    /// Line colour, label background and label text (from the theme).
    pub ink: Color,
    pub label_bg: Color,
    pub label_text: Color,
}

struct Projector {
    view_proj: Mat4,
    size: (f32, f32),
}

impl Projector {
    fn point(&self, world: Vec3) -> Option<Point> {
        let clip = self.view_proj * world.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Point::new((ndc.x + 1.0) * 0.5 * self.size.0, (1.0 - ndc.y) * 0.5 * self.size.1))
    }

    /// A polyline through world points (pieces behind the camera are left out).
    fn path(&self, points: &[Vec3], close: bool) -> Option<Path> {
        let screen: Vec<Point> = points.iter().filter_map(|&p| self.point(p)).collect();
        if screen.len() < 2 || screen.len() != points.len() {
            return None;
        }
        Some(Path::new(|b| {
            b.move_to(screen[0]);
            for &p in &screen[1..] {
                b.line_to(p);
            }
            if close {
                b.close();
            }
        }))
    }
}

/// Points along the arc of an angle (world).
fn arc_points(a: &Angle) -> Vec<Vec3> {
    let r = arc_radius(a);
    let w = (a.v2 - a.v1 * a.v1.dot(a.v2)).try_normalize();
    let steps = 24;
    (0..=steps)
        .map(|i| {
            let t = a.angle * i as f32 / steps as f32;
            let dir = match w {
                Some(w) => a.v1 * t.cos() + w * t.sin(),
                None => a.v1,
            };
            a.center + dir * r
        })
        .collect()
}

impl MeasureOverlay<'_> {
    fn stroke(&self, width: f32) -> Stroke<'static> {
        Stroke::default().with_color(self.ink).with_width(width)
    }

    fn dashed(&self) -> Stroke<'static> {
        Stroke { line_dash: LineDash { segments: &[4.0, 4.0], offset: 0 }, ..self.stroke(1.0) }
    }

    fn line(&self, frame: &mut Frame, pr: &Projector, a: Vec3, b: Vec3, stroke: Stroke<'_>) {
        if let Some(path) = pr.path(&[a, b], false) {
            frame.stroke(&path, stroke);
        }
    }

    fn dot(&self, frame: &mut Frame, pr: &Projector, p: Vec3) {
        if let Some(c) = pr.point(p) {
            let circle = Path::circle(c, 3.5);
            frame.fill(&circle, self.label_bg);
            frame.stroke(&circle, self.stroke(1.5));
        }
    }

    /// A value on a rounded label centred on `at`.
    fn label(&self, frame: &mut Frame, pr: &Projector, at: Vec3, content: String) {
        let Some(c) = pr.point(at) else { return };
        let size = 12.0;
        let width = content.chars().count() as f32 * size * 0.58 + 12.0;
        let height = size + 8.0;
        let rect = Path::rounded_rectangle(
            Point::new(c.x - width / 2.0, c.y - height / 2.0),
            Size::new(width, height),
            4.0.into(),
        );
        frame.fill(&rect, self.label_bg);
        frame.stroke(&rect, self.stroke(1.0));
        frame.fill_text(canvas::Text {
            content,
            position: c,
            align_x: alignment::Horizontal::Center.into(),
            align_y: alignment::Vertical::Center,
            size: iced::Pixels(size),
            color: self.label_text,
            font: openaec_ui::fonts::MEDIUM,
            ..canvas::Text::default()
        });
    }

    fn fill(&self) -> Color {
        Color { a: 0.18, ..self.ink }
    }

    fn measurement(&self, frame: &mut Frame, pr: &Projector, m: &Measurement) {
        match m {
            Measurement::Distance(d) => {
                for &(a, b) in &d.extensions {
                    self.line(frame, pr, a, b, self.dashed());
                }
                self.line(frame, pr, d.p1, d.p2, self.stroke(2.0));
                self.dot(frame, pr, d.p1);
                self.dot(frame, pr, d.p2);
            }
            Measurement::Angle(a) => {
                self.line(frame, pr, a.center, a.center + a.v1 * a.len1.max(arc_radius(a)), self.stroke(1.5));
                self.line(frame, pr, a.center, a.center + a.v2 * a.len2.max(arc_radius(a)), self.stroke(1.5));
                if let Some(path) = pr.path(&arc_points(a), false) {
                    frame.stroke(&path, self.stroke(2.0));
                }
                self.dot(frame, pr, a.center);
            }
            Measurement::Area(r) => {
                if r.triangles.len() <= MAX_FILL_TRIANGLES {
                    for t in &r.triangles {
                        if let Some(path) = pr.path(t, true) {
                            frame.fill(&path, self.fill());
                        }
                    }
                }
                for &(a, b) in &r.outline {
                    self.line(frame, pr, a, b, self.stroke(2.0));
                }
            }
            Measurement::Volume { .. } => {}
            Measurement::Polygon { points, .. } => {
                if let Some(path) = pr.path(points, true) {
                    frame.fill(&path, self.fill());
                    frame.stroke(&path, self.stroke(2.0));
                }
                for &p in points {
                    self.dot(frame, pr, p);
                }
            }
        }
        self.label(frame, pr, m.anchor(), m.label(&self.units, self.language));
    }

    fn pick(&self, frame: &mut Frame, pr: &Projector, s: &Snap) {
        match s.entity {
            Entity::Point(p) => self.dot(frame, pr, p),
            Entity::Line { a, b, p } => {
                self.line(frame, pr, a, b, self.stroke(3.0));
                self.dot(frame, pr, p);
            }
            Entity::Plane { p, .. } => {
                if let Some(c) = pr.point(p) {
                    let square = Path::rectangle(Point::new(c.x - 5.0, c.y - 5.0), Size::new(10.0, 10.0));
                    frame.fill(&square, self.fill());
                    frame.stroke(&square, self.stroke(1.5));
                }
            }
        }
    }
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
        let pr = Projector { view_proj: self.view_proj, size: (bounds.width, bounds.height) };
        for m in self.results {
            self.measurement(&mut frame, &pr, m);
        }
        // the polygon in progress
        let points: Vec<Vec3> = self.picks.iter().map(|s| s.entity.pick()).collect();
        if points.len() >= 2 {
            if let Some(path) = pr.path(&points, false) {
                frame.stroke(&path, self.dashed());
            }
        }
        for s in self.picks {
            self.pick(&mut frame, &pr, s);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &(), _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        mouse::Interaction::None
    }
}
