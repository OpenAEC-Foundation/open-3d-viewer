//! Draws the measurements over the viewport: lines with their value on a label, the arc of an
//! angle, the outline and a light fill of a measured face or polygon, the picks of the
//! measurement in progress, and what a click would pick under the cursor (a ring on a point, a
//! triangle on the middle of an edge, the edge itself, or the face outlined), with its kind on a
//! tag. Drawn only: the viewport below handles the pointer.

use super::format::{Category, UnitSettings};
use super::geometry::{self, Angle, Entity, FaceRegion};
use super::hover::{Hover, SnapKind};
use super::{arc_radius, Measurement, Snap, Tool};
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::{alignment, mouse, Color, Point, Rectangle, Size};
use openaec_ui::Language;

/// Faces with more triangles than this are drawn as outline only.
const MAX_FILL_TRIANGLES: usize = 20_000;
/// The same for the face under the cursor, drawn again on every move.
const MAX_HOVER_FILL_TRIANGLES: usize = 2_000;

pub struct MeasureOverlay<'a> {
    pub results: &'a [Measurement],
    pub picks: &'a [Snap],
    /// What a click would pick, and the tool it is for.
    pub hover: Option<&'a Hover>,
    pub tool: Option<Tool>,
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
        self.tag(frame, c, content, 12.0);
    }

    /// Text of `size` on a rounded label centred on `c` (screen).
    fn tag(&self, frame: &mut Frame, c: Point, content: String, size: f32) {
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

/// The tool's next result with `hover` as its last pick, drawn as a preview.
fn preview(tool: Option<Tool>, picks: &[Snap], hover: &Hover) -> Option<Measurement> {
    match (tool?, picks) {
        (Tool::Distance, [first]) => Some(Measurement::Distance(geometry::perpendicular(&first.entity, &hover.snap.entity))),
        _ => None,
    }
}

impl MeasureOverlay<'_> {
    /// A line with a halo in the label colour, so it stands out on any surface.
    fn halo_line(&self, frame: &mut Frame, path: &Path, width: f32) {
        frame.stroke(path, Stroke::default().with_color(self.label_bg).with_width(width + 3.0));
        frame.stroke(path, self.stroke(width));
    }

    /// The face under the cursor: a light fill (small faces) and its outline.
    fn hover_face(&self, frame: &mut Frame, pr: &Projector, face: &FaceRegion) {
        if face.triangles.len() <= MAX_HOVER_FILL_TRIANGLES {
            let fill = Color { a: 0.22, ..self.ink };
            for t in &face.triangles {
                if let Some(path) = pr.path(t, true) {
                    frame.fill(&path, fill);
                }
            }
        }
        let segments: Vec<(Point, Point)> =
            face.outline.iter().filter_map(|&(a, b)| Some((pr.point(a)?, pr.point(b)?))).collect();
        let outline = Path::new(|b| {
            for &(p, q) in &segments {
                b.move_to(p);
                b.line_to(q);
            }
        });
        self.halo_line(frame, &outline, 2.0);
    }

    /// What a click would pick: the marker of its kind, and the kind (or for the area tool the
    /// area) on a tag beside it.
    fn hover(&self, frame: &mut Frame, pr: &Projector, h: &Hover) {
        let Some(c) = pr.point(h.snap.entity.pick()) else { return };
        if let Some(m) = preview(self.tool, self.picks, h) {
            if let Measurement::Distance(d) = &m {
                self.line(frame, pr, d.p1, d.p2, self.dashed());
            }
            self.label(frame, pr, m.anchor(), m.label(&self.units, self.language));
        }
        let mut tag = h.kind.label(self.language).to_string();
        match (h.kind, &h.face) {
            (SnapKind::Face, Some(face)) => {
                self.hover_face(frame, pr, face);
                if self.tool == Some(Tool::Area) {
                    tag = format!("{tag} · {}", self.units.format(self.language, Category::Area, face.area));
                }
                let square = Path::rectangle(Point::new(c.x - 4.0, c.y - 4.0), Size::new(8.0, 8.0));
                frame.fill(&square, self.label_bg);
                frame.stroke(&square, self.stroke(1.5));
            }
            (SnapKind::Face, None) => {}
            (SnapKind::Edge, _) => {
                if let Entity::Line { a, b, .. } = h.snap.entity {
                    if let Some(path) = pr.path(&[a, b], false) {
                        self.halo_line(frame, &path, 3.0);
                    }
                }
                let diamond = Path::new(|b| {
                    b.move_to(Point::new(c.x, c.y - 6.0));
                    b.line_to(Point::new(c.x + 6.0, c.y));
                    b.line_to(Point::new(c.x, c.y + 6.0));
                    b.line_to(Point::new(c.x - 6.0, c.y));
                    b.close();
                });
                frame.fill(&diamond, self.label_bg);
                frame.stroke(&diamond, self.stroke(2.0));
            }
            (SnapKind::Middle, _) => {
                let triangle = Path::new(|b| {
                    b.move_to(Point::new(c.x, c.y - 8.0));
                    b.line_to(Point::new(c.x + 7.0, c.y + 5.0));
                    b.line_to(Point::new(c.x - 7.0, c.y + 5.0));
                    b.close();
                });
                frame.fill(&triangle, self.label_bg);
                frame.stroke(&triangle, self.stroke(2.0));
                frame.fill(&Path::circle(c, 2.0), self.ink);
            }
            (SnapKind::Corner, _) => {
                let ring = Path::circle(c, 7.0);
                frame.fill(&ring, self.label_bg);
                frame.stroke(&ring, self.stroke(2.0));
                frame.fill(&Path::circle(c, 3.0), self.ink);
            }
        }
        // the tag up and to the right of the marker, clear of the cursor
        let half = tag.chars().count() as f32 * 11.0 * 0.29 + 6.0;
        self.tag(frame, Point::new(c.x + 12.0 + half, c.y - 18.0), tag, 11.0);
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
        if let Some(h) = self.hover {
            // the polygon in progress runs on to the cursor
            if let (Some(Tool::Polygon), Some(last)) = (self.tool, points.last()) {
                self.line(&mut frame, &pr, *last, h.snap.entity.pick(), self.dashed());
            }
            self.hover(&mut frame, &pr, h);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &(), _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        mouse::Interaction::None
    }
}
