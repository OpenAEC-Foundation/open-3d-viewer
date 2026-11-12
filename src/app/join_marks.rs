//! The marks of joins: a small diamond where the members of every join of modelled linear
//! elements meet. A click on one selects its join, so a mitre or a butt (which has no body of
//! its own) can be picked as an end plate can by its plate.

use super::{App, Message};
use bim_core::ElementId;
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{mouse, Color, Point, Rectangle, Size};

/// Clicks within this of a mark (logical pixels) take it.
const MARK_PIXELS: f32 = 8.0;

/// The marks of the joins over the viewport.
pub struct JoinMarks {
    /// Every join's id, where its members meet (scene coordinates) and whether it is selected.
    pub marks: Vec<(ElementId, Vec3, bool)>,
    /// Model to clip space.
    pub view_proj: Mat4,
    pub ink: Color,
    pub fill: Color,
}

impl App {
    /// The marks of the modelled joins; none while a tool or edit mode is on.
    pub(super) fn join_marks(&self, ink: Color, fill: Color) -> Option<JoinMarks> {
        if self.modeller.tool.is_some() || self.modeller.edit.is_some() {
            return None;
        }
        let picked = self.selected_modelled();
        let marks: Vec<(ElementId, Vec3, bool)> = self
            .modeller
            .model
            .iter()
            .filter_map(|e| Some((e.id, self.modeller.model.join_node(e.id)?.as_vec3(), picked.contains(&e.id))))
            .collect();
        (!marks.is_empty()).then(|| JoinMarks { marks, view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink, fill })
    }
}

/// Where a model point shows in a viewport of `size`, if in front of the camera.
fn project(view_proj: &Mat4, size: Size, p: Vec3) -> Option<Point> {
    let c = view_proj.project_point3(p);
    ((0.0..=1.0).contains(&c.z)).then(|| Point::new((c.x + 1.0) / 2.0 * size.width, (1.0 - c.y) / 2.0 * size.height))
}

impl JoinMarks {
    /// The join whose mark is under `at`, the nearest within reach.
    fn mark_at(&self, size: Size, at: Point) -> Option<ElementId> {
        self.marks
            .iter()
            .filter_map(|(id, p, _)| project(&self.view_proj, size, *p).map(|s| (*id, s.distance(at))))
            .filter(|(_, d)| *d <= MARK_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}

/// What the marks keep between events: the one under the cursor.
#[derive(Default)]
pub struct MarksState {
    hover: Option<ElementId>,
}

impl canvas::Program<Message> for JoinMarks {
    type State = MarksState;

    fn update(&self, state: &mut MarksState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hover = cursor.position_in(bounds).and_then(|p| self.mark_at(bounds.size(), p));
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let id = self.mark_at(bounds.size(), cursor.position_in(bounds)?)?;
                Some(canvas::Action::publish(Message::PickJoin(id)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &MarksState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        for (id, p, picked) in &self.marks {
            let Some(at) = project(&self.view_proj, bounds.size(), *p) else { continue };
            let r = if state.hover == Some(*id) { 7.0 } else { 5.5 };
            let diamond = Path::new(|path| {
                path.move_to(Point::new(at.x, at.y - r));
                path.line_to(Point::new(at.x + r, at.y));
                path.line_to(Point::new(at.x, at.y + r));
                path.line_to(Point::new(at.x - r, at.y));
                path.close();
            });
            frame.fill(&diamond, if *picked { self.ink } else { self.fill });
            frame.stroke(&diamond, canvas::Stroke::default().with_color(self.ink).with_width(1.5));
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, _state: &MarksState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match cursor.position_in(bounds) {
            Some(p) if self.mark_at(bounds.size(), p).is_some() => mouse::Interaction::Pointer,
            _ => mouse::Interaction::None,
        }
    }
}
