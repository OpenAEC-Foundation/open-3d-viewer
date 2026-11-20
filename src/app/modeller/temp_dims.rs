//! Temporary dimensions, as in Revit: a selected wall (or standing system) shows how far it stands
//! from what runs beside it, and a value typed there moves the wall.
//!
//! - **What is measured.** From the wall's axis, square to it, to the nearest parallel line on
//!   either side: the axis of another wall or standing system that the wall's middle lies
//!   beside, or a grid line. At most one dimension to each side.
//! - **How it shows.** A line from the middle of the wall to the foot on the other line, with a
//!   field holding the distance in metres.
//! - **Typing a value** and Enter moves the selected wall, square to itself, until it stands that
//!   far from the other line; what it is measured to stays. The walls joined to it follow, as
//!   when its middle grip is dragged. It is one step to take back; what the kernel refuses moves
//!   nothing and the banner says why.
//! - They show with the Select tool, outside edit mode and sketches, while nothing is dragged.

use crate::app::edit::{edit_points, Edit};
use crate::app::{App, Message};
use crate::theme::Palette;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3, Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::widget::{container, stack, text_input};
use iced::{mouse, Color, Element, Fill, Padding, Point, Rectangle};

/// Lines run parallel when the sine of the angle between them is at most this.
const PARALLEL: f64 = 0.01;
/// A wall is measured to another when its middle lies beside that one, or this far past its end
/// (metres).
const BESIDE: f64 = 0.5;
/// The width of a dimension's field (logical pixels).
const FIELD: f32 = 64.0;

/// A temporary dimension of the selected wall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempDim {
    /// The middle of the wall's axis, at its base (scene coordinates).
    pub from: DVec3,
    /// The foot on the line it is measured to.
    pub to: DVec3,
    pub distance: f64,
    /// From the wall towards that line, in plan, of length 1.
    pub toward: DVec2,
}

/// What a temporary dimension's field says.
#[derive(Debug, Clone, PartialEq)]
pub enum TempDimMessage {
    /// Dimension `0` has this text typed in its field.
    Typed(usize, String),
    /// Enter: the value typed moves the wall.
    Apply(usize),
}

/// The axis of a wall or a standing system in plan, and its base; `None` for anything else.
fn axis(shape: &Shape) -> Option<(DVec2, DVec2, f64)> {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => Some((*start, *end, *base)),
        Shape::System(s) if !s.lies() => Some((s.start, s.end, s.base)),
        _ => None,
    }
}

impl App {
    /// The wall whose temporary dimensions show: the one element with grips, when nothing of it
    /// is being dragged.
    fn temp_dimmed(&self) -> Option<ElementId> {
        if !self.modeller.in_modeller || self.modeller.slab_drag.is_some() {
            return None;
        }
        self.gripped()
    }

    /// The temporary dimensions of the selected wall: to the nearest parallel line on its left,
    /// then on its right, where there is one.
    pub(crate) fn temp_dims(&self) -> Vec<TempDim> {
        let Some(id) = self.temp_dimmed() else { return Vec::new() };
        let grid = self.modeller.model.grid;
        let Some((a, b, base)) = self.modeller.model.get(id).and_then(|e| axis(&e.shape_in_project(&grid))) else { return Vec::new() };
        let Some(along) = (b - a).try_normalize() else { return Vec::new() };
        let (left, middle) = (along.perp(), (a + b) / 2.0);
        // how far to the left of the wall's middle each parallel line runs
        let mut offsets: Vec<f64> = Vec::new();
        for e in self.modeller.model.iter().filter(|e| e.id != id) {
            let Some((c, d, _)) = axis(&e.shape_in_project(&grid)) else { continue };
            let Some(theirs) = (d - c).try_normalize() else { continue };
            let beside = (middle - c).dot(theirs);
            if along.perp_dot(theirs).abs() <= PARALLEL && beside >= -BESIDE && beside <= c.distance(d) + BESIDE {
                offsets.push((c - middle).dot(left));
            }
        }
        for g in self.modeller.model.grids() {
            if (g.end - g.start).try_normalize().is_some_and(|theirs| along.perp_dot(theirs).abs() <= PARALLEL) {
                offsets.push((g.start - middle).dot(left));
            }
        }
        let nearest = |side: f64| offsets.iter().copied().filter(|s| s * side > 1e-6).min_by(|x, y| x.abs().total_cmp(&y.abs()));
        [1.0, -1.0]
            .into_iter()
            .filter_map(|side| {
                let s = nearest(side)?;
                Some(TempDim { from: middle.extend(base), to: (middle + left * s).extend(base), distance: s.abs(), toward: left * side })
            })
            .collect()
    }

    /// Gives temporary dimension `index` the value `distance` (metres): the selected wall moves
    /// square to itself until it stands that far from the line measured to. False when there is
    /// no such dimension or the value is below zero.
    pub(in crate::app) fn set_temp_dim(&mut self, index: usize, distance: f64) -> bool {
        let (Some(element), Some(dim)) = (self.temp_dimmed(), self.temp_dims().get(index).copied()) else { return false };
        if distance < 0.0 {
            return false;
        }
        let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return false };
        let step = (dim.toward * (dim.distance - distance)).extend(0.0);
        let (points, _) = edit_points(&shape);
        let moved: Vec<DVec3> = points.iter().map(|p| *p + step).collect();
        // as the middle grip dragged that far: the walls joined to it follow
        self.modeller.edit = Some(Edit { element, picked: vec![0, 1], grab: None, grips: true });
        self.put_grip_down(moved, step != DVec3::ZERO);
        true
    }

    /// A temporary dimension's field was typed in, or Enter pressed there.
    pub(in crate::app) fn temp_dim_message(&mut self, message: TempDimMessage) {
        match message {
            TempDimMessage::Typed(index, text) => self.modeller.temp_dim = Some((index, text)),
            TempDimMessage::Apply(index) => {
                let typed = self.modeller.temp_dim.take().filter(|(i, _)| *i == index);
                if let Some(value) = typed.and_then(|(_, text)| crate::document::parse_elevation(&text)) {
                    self.set_temp_dim(index, value);
                }
            }
        }
    }

    /// Esc while a value is typed: what was typed goes. False when nothing was.
    pub(in crate::app) fn temp_dim_escape(&mut self) -> bool {
        self.modeller.temp_dim.take().is_some()
    }

    /// The lines of the temporary dimensions over the viewport.
    pub(in crate::app) fn temp_dim_marks(&self, ink: Color) -> Option<TempDimMarks> {
        if !self.annotates { return None; }
        let dims = self.temp_dims();
        (!dims.is_empty()).then(|| TempDimMarks { lines: dims.iter().map(|d| [d.from.as_vec3(), d.to.as_vec3()]).collect(), view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink })
    }

    /// The fields of the temporary dimensions, each at the middle of its line: the distance, to
    /// type another over.
    pub(in crate::app) fn temp_dim_fields(&self, _p: Palette) -> Option<Element<'_, Message>> {
        if !self.annotates { return None; }
        let dims = self.temp_dims();
        if dims.is_empty() {
            return None;
        }
        let (width, height) = self.viewport_size;
        let mut layers = stack![].width(Fill).height(Fill);
        for (i, d) in dims.iter().enumerate() {
            let Some((x, y)) = self.on_screen((d.from + d.to) / 2.0, width.max(1.0), height.max(1.0)) else { continue };
            let typed = self.modeller.temp_dim.as_ref().filter(|(k, _)| *k == i).map(|(_, text)| text.clone());
            let value = typed.unwrap_or_else(|| format!("{:.3}", d.distance));
            let field = text_input("", &value)
                .on_input(move |text| Message::TempDim(TempDimMessage::Typed(i, text)))
                .on_submit(Message::TempDim(TempDimMessage::Apply(i)))
                .size(12)
                .padding([2, 6])
                .width(FIELD);
            let at = Padding { left: (x - FIELD / 2.0).clamp(0.0, (width - FIELD).max(0.0)), top: (y - 11.0).clamp(0.0, (height - 22.0).max(0.0)), ..Padding::ZERO };
            layers = layers.push(container(field).padding(at));
        }
        Some(layers.into())
    }
}

/// The lines of the temporary dimensions drawn over the viewport, each with a tick at its ends.
pub struct TempDimMarks {
    pub lines: Vec<[Vec3; 2]>,
    /// Model to clip space.
    pub view_proj: Mat4,
    pub ink: Color,
}

impl<M> canvas::Program<M> for TempDimMarks {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let shown = |p: Vec3| {
            let c = self.view_proj.project_point3(p);
            ((0.0..=1.0).contains(&c.z)).then(|| Point::new((c.x + 1.0) / 2.0 * bounds.width, (1.0 - c.y) / 2.0 * bounds.height))
        };
        let stroke = canvas::Stroke::default().with_color(self.ink).with_width(1.0);
        for [a, b] in &self.lines {
            let (Some(a), Some(b)) = (shown(*a), shown(*b)) else { continue };
            frame.stroke(&Path::line(a, b), stroke);
            // a tick across each end
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let length = dx.hypot(dy).max(1e-3);
            let (tx, ty) = (-dy / length * 5.0, dx / length * 5.0);
            for end in [a, b] {
                frame.stroke(&Path::line(Point::new(end.x - tx, end.y - ty), Point::new(end.x + tx, end.y + ty)), stroke);
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::click;
    use super::super::ModelTool;
    use super::*;

    /// A new model in the Modeller, seen from above.
    fn modelling() -> App {
        let mut app = super::super::tests::modelling();
        app.modeller.in_modeller = true;
        app
    }

    fn walls(app: &mut App, points: &[(f64, f64)]) {
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        for &(x, y) in points {
            click(app, x, y);
        }
        app.set_model_tool(None);
    }

    fn ends(app: &App, id: u64) -> (DVec2, DVec2) {
        let (p, _) = edit_points(&app.modeller.model.get(ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid));
        (p[0].truncate(), p[1].truncate())
    }

    #[test]
    fn a_wall_3_m_from_a_grid_line_is_put_at_3_60_by_typing_in_one_step() {
        let mut app = modelling();
        // a grid line along x at y = 0, a wall along x at y = 3
        app.modeller.model.add_grid(DVec2::new(-2.0, 0.0), DVec2::new(10.0, 0.0)).unwrap();
        walls(&mut app, &[(0.0, 3.0), (6.0, 3.0)]);
        app.keep_model_history();
        assert!(app.temp_dims().is_empty(), "nothing selected");
        app.select_modelled(ElementId(1));
        let dims = app.temp_dims();
        assert_eq!(dims.len(), 1);
        assert!((dims[0].distance - 3.0).abs() < 1e-9);
        assert_eq!((dims[0].from, dims[0].to), (DVec3::new(3.0, 3.0, 0.0), DVec3::new(3.0, 0.0, 0.0)));
        assert!(app.temp_dim_marks(Color::BLACK).is_some());
        // 3.60 typed in its field, Enter
        let _ = app.update(Message::TempDim(TempDimMessage::Typed(0, "3,60".into())));
        let _ = app.update(Message::TempDim(TempDimMessage::Apply(0)));
        assert_eq!(ends(&app, 1), (DVec2::new(0.0, 3.6), DVec2::new(6.0, 3.6)));
        assert!((app.temp_dims()[0].distance - 3.6).abs() < 1e-9, "it stays selected, its dimension the new value");
        // one step to take back
        let _ = app.update(Message::Undo);
        assert_eq!(ends(&app, 1), (DVec2::new(0.0, 3.0), DVec2::new(6.0, 3.0)));
    }

    #[test]
    fn a_wall_between_two_others_has_a_dimension_to_each_and_the_joined_walls_follow() {
        let mut app = modelling();
        // seen from this far a click is near every line: the walls go where they are clicked
        app.modeller.snaps.line = false;
        app.modeller.snaps.middle = false;
        // a U open to the east: south wall 1, west wall 2, north wall 3; and a loose wall 4 in between
        walls(&mut app, &[(6.0, 0.0), (0.0, 0.0), (0.0, 4.0), (6.0, 4.0)]);
        walls(&mut app, &[(1.0, 1.5), (5.0, 1.5)]);
        app.select_modelled(ElementId(4));
        let dims = app.temp_dims();
        let mut distances: Vec<f64> = dims.iter().map(|d| d.distance).collect();
        distances.sort_by(f64::total_cmp);
        assert_eq!(distances, vec![1.5, 2.5], "to the south wall and to the north wall");
        // the dimension to the north wall set to 1 m: the loose wall moves north by 1.5
        let north = dims.iter().position(|d| (d.distance - 2.5).abs() < 1e-9).unwrap();
        assert!(app.set_temp_dim(north, 1.0));
        assert_eq!(ends(&app, 4), (DVec2::new(1.0, 3.0), DVec2::new(5.0, 3.0)));
        // the west wall is not parallel to it: no dimension to it; a value below zero is refused
        assert!(!app.set_temp_dim(0, -1.0));
        // the north wall moved by its dimension to the loose one: the west wall's corner follows
        app.select_modelled(ElementId(3));
        let to_loose = app.temp_dims().iter().position(|d| (d.distance - 1.0).abs() < 1e-9).expect("to the loose wall");
        assert!(app.set_temp_dim(to_loose, 2.0));
        assert_eq!(ends(&app, 3), (DVec2::new(0.0, 5.0), DVec2::new(6.0, 5.0)));
        assert_eq!(ends(&app, 2).1, DVec2::new(0.0, 5.0), "the corner goes with it");
    }

    #[test]
    fn a_click_in_a_dimensions_field_types_there_and_leaves_the_selection() {
        let mut app = modelling();
        app.modeller.model.add_grid(DVec2::new(-2.0, 0.0), DVec2::new(10.0, 0.0)).unwrap();
        walls(&mut app, &[(0.0, 3.0), (6.0, 3.0)]);
        app.select_modelled(ElementId(1));
        // the viewport alone, at the size the app knows it has: the field at the middle of the line
        let (x, y) = app.on_screen(DVec3::new(3.0, 1.5, 0.0), 1000.0, 800.0).unwrap();
        let p = app.theme.palette();
        let messages: Vec<Message> = {
            let settings = iced::Settings::default();
            let mut ui = iced_test::Simulator::with_size(settings, iced::Size::new(1000.0, 800.0), app.viewport_view(p, openaec_ui::Language::English));
            ui.point_at(Point::new(x, y));
            let _ = ui.simulate(iced_test::simulator::click());
            let _ = ui.typewrite("7");
            ui.into_messages().collect()
        };
        let typed: Vec<&Message> = messages.iter().filter(|m| matches!(m, Message::TempDim(_))).collect();
        assert!(!typed.is_empty(), "the field takes what is typed: {messages:?}");
        assert!(!messages.iter().any(|m| matches!(m, Message::Navigate(crate::viewport::Navigate::Pick { .. }))), "the click is the field's, not the viewport's: {messages:?}");
    }

    #[test]
    fn esc_drops_what_was_typed_and_a_text_that_is_no_number_moves_nothing() {
        let mut app = modelling();
        walls(&mut app, &[(0.0, 0.0), (6.0, 0.0)]);
        walls(&mut app, &[(0.0, 3.0), (6.0, 3.0)]);
        app.select_modelled(ElementId(2));
        let _ = app.update(Message::TempDim(TempDimMessage::Typed(0, "4".into())));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.temp_dim.is_none());
        assert_eq!(app.selected_modelled(), vec![ElementId(2)], "Esc took the typing, not the selection");
        let _ = app.update(Message::TempDim(TempDimMessage::Typed(0, "abc".into())));
        let _ = app.update(Message::TempDim(TempDimMessage::Apply(0)));
        assert_eq!(ends(&app, 2), (DVec2::new(0.0, 3.0), DVec2::new(6.0, 3.0)));
    }
}
