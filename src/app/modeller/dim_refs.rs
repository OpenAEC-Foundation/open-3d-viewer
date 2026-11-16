//! Dimensions that hold on to elements: a dimension placed in a floor plan of the Modeller keeps,
//! for each end, the element it was clicked on, so it follows that element and can move it.
//!
//! - **Holding on.** An end clicked on a modelled element (within 50 mm of a wall, a beam, a
//!   slab's edge or an object) keeps that element, the nearest of its points (as edit mode has
//!   them) and where the end lies from that point. An end clicked elsewhere holds on to nothing.
//! - **Following.** After every change of the model each such end is put where its element has
//!   it now. An end whose element is gone stays where it was and the dimension shows red; when
//!   the element comes back (undo), it holds on again.
//! - **Driving.** A length typed for the picked dimension moves the element its last end holds
//!   on to (the one picked last), along the dimension, until the dimension has that length; when
//!   only its first end holds on to an element, that one moves. The other end stays.
//! - Only floor plans: their dimensions are in the model's x and y. A section's are not followed.

use crate::app::edit::edit_points;
use crate::app::transform::Map;
use crate::app::{App, Message};
use crate::document::Anchor;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::widget::text_input;
use iced::Element;

/// An end holds on to an element within this of it (metres).
const HOLD: f64 = 0.05;

/// The points of a shape in plan, and the direction it runs in when it has one (a wall, a beam, a
/// system: from its first point to its second).
fn frame(shape: &Shape) -> (Vec<DVec2>, Option<DVec2>) {
    let (points, closed) = edit_points(shape);
    let plan: Vec<DVec2> = points.iter().map(|p| p.truncate()).collect();
    let along = match (&plan[..], closed) {
        ([a, b], false) => (*b - *a).try_normalize(),
        _ => None,
    };
    (plan, along)
}

/// How far point `q` lies from segment `a`-`b`.
fn off_segment(q: DVec2, a: DVec2, b: DVec2) -> f64 {
    let r = b - a;
    let t = ((q - a).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
    (a + r * t).distance(q)
}

/// How far `q` lies outside the plan of `shape` (0 inside its width); `None` for a shape a
/// dimension cannot hold on to.
fn off_shape(shape: &Shape, q: DVec2) -> Option<f64> {
    let (points, _) = frame(shape);
    match shape {
        Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { .. }, .. }) => Some((off_segment(q, points[0], points[1]) - p.total_thickness() / 2.0).max(0.0)),
        Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. }) => {
            let n = points.len();
            (0..n).map(|i| off_segment(q, points[i], points[(i + 1) % n])).min_by(f64::total_cmp)
        }
        Shape::System(s) => Some((off_segment(q, points[0], points[1]) - s.member.max_element() / 2.0).max(0.0)),
        Shape::Linear(l) => {
            let half = l.profile.profile().map_or(0.1, |p| p.outer().iter().map(|c| c.x.abs().max(c.y.abs())).fold(0.0, f64::max));
            Some((off_segment(q, points[0], points[1]) - half).max(0.0))
        }
        Shape::Point(p) => Some((points[0].distance(q) - p.size.x.max(p.size.y) / 2.0).max(0.0)),
        Shape::Connection(_) => None,
    }
}

impl App {
    /// What a dimension's end at `q` (the model's x and y) would hold on to: the modelled element
    /// it lies on, the nearest of that element's points, and where it lies from that point.
    pub(crate) fn anchor_at(&self, q: [f64; 2]) -> Option<Anchor> {
        let q = DVec2::from_array(q);
        let grid = self.modeller.model.grid;
        let (id, shape, _) = self
            .modeller
            .model
            .iter()
            .filter_map(|e| {
                let shape = e.shape_in_project(&grid);
                let off = off_shape(&shape, q)?;
                (off <= HOLD).then_some((e.id, shape, off))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        let (points, along) = frame(&shape);
        let point = (0..points.len()).min_by(|a, b| points[*a].distance(q).total_cmp(&points[*b].distance(q)))?;
        let from = q - points[point];
        let offset = match along {
            Some(u) => [from.dot(u), from.dot(u.perp())],
            None => from.to_array(),
        };
        Some(Anchor { element: id.0, point, offset, lost: false })
    }

    /// Where an anchor's end lies now; `None` when its element is gone.
    pub(crate) fn anchored(&self, anchor: &Anchor) -> Option<[f64; 2]> {
        let e = self.modeller.model.get(ElementId(anchor.element))?;
        let (points, along) = frame(&e.shape_in_project(&self.modeller.model.grid));
        let at = *points.get(anchor.point)?;
        let [a, b] = anchor.offset;
        Some(match along {
            Some(u) => at + u * a + u.perp() * b,
            None => at + DVec2::new(a, b),
        }
        .to_array())
    }

    /// The dimension added last to floor plan `view`: its ends hold on to the modelled elements
    /// they lie on. Nothing outside the Modeller, or in a view that is no floor plan.
    pub(in crate::app) fn anchor_last_dimension(&mut self, view: &str) {
        if !self.modeller.in_modeller || self.modeller.model.is_empty() || self.document.plan(view).is_none() {
            return;
        }
        let Some((ends, index)) = self.document.plan(view).and_then(|p| p.dimensions.last().map(|d| ([d.from, d.to], p.dimensions.len() - 1))) else { return };
        let anchors = ends.map(|q| self.anchor_at(q));
        if let Some(d) = self.document.dimension_mut(view, index) {
            d.anchors = anchors;
        }
    }

    /// After a change of the model: every end of a floor plan's dimension that holds on to an
    /// element goes where the element has it now; one whose element is gone is marked.
    pub(in crate::app) fn follow_dimensions(&mut self) {
        let model = &self.modeller.model;
        if model.is_empty() && self.document.plans().iter().all(|p| p.dimensions.iter().all(|d| d.anchors == [None, None])) {
            return;
        }
        // where every anchor is now, worked out before the document is changed
        let mut moves: Vec<(usize, usize, Option<[f64; 2]>)> = Vec::new();
        let dimensions: Vec<crate::document::Dimension> = self.document.plans().iter().flat_map(|p| p.dimensions.iter().copied()).collect();
        for (k, d) in dimensions.iter().enumerate() {
            for (end, anchor) in d.anchors.iter().enumerate() {
                if let Some(anchor) = anchor {
                    moves.push((k, end, self.anchored(anchor)));
                }
            }
        }
        let mut all: Vec<&mut crate::document::Dimension> = self.document.plan_dimensions_mut().collect();
        for (k, end, at) in moves {
            let Some(d) = all.get_mut(k) else { continue };
            let Some(anchor) = d.anchors[end].as_mut() else { continue };
            anchor.lost = at.is_none();
            if let Some(at) = at {
                if end == 0 {
                    d.from = at;
                } else {
                    d.to = at;
                }
            }
        }
    }

    /// Gives dimension `index` of floor plan `view` the length `length` (metres): the element its
    /// last end holds on to moves along the dimension (else the one its first end holds on to).
    /// False when it holds on to nothing that is, or the length is not above zero.
    pub(crate) fn set_dimension_length(&mut self, view: &str, index: usize, length: f64) -> bool {
        let Some(d) = self.document.plan(view).and_then(|p| p.dimensions.get(index)).copied() else { return false };
        if !(length.is_finite() && length > 0.0) {
            return false;
        }
        let (from, to) = (DVec2::from_array(d.from), DVec2::from_array(d.to));
        let Some(along) = (to - from).try_normalize() else { return false };
        let grow = length - d.length();
        let held = |end: usize| d.anchors[end].filter(|a| !a.lost).map(|a| ElementId(a.element));
        // the last end moves away from the first; the first away from the last
        let (element, step) = match (held(1), held(0)) {
            (Some(e), other) if other != Some(e) => (e, along * grow),
            (None, Some(e)) => (e, -along * grow),
            _ => return false,
        };
        self.modeller.refusal = None;
        self.transform_elements(&[element], &Map::Shift(DVec3::new(step.x, step.y, 0.0)), false);
        self.modeller.refusal.is_none()
    }

    /// Enter in the field of the picked dimension: the length typed moves its element.
    pub(in crate::app) fn apply_dimension_value(&mut self) {
        let Some(text) = self.modeller.dimension_value.take() else { return };
        let (Some(view), Some(crate::app::marks::MarkPick::Dimension(k))) = (self.shown_view().map(str::to_string), self.mark_pick) else { return };
        if let Some(length) = crate::document::parse_elevation(&text) {
            self.set_dimension_length(&view, k, length);
        }
    }

    /// The field of the picked dimension in the view's bar: its length, to type another over.
    /// `None` when it holds on to no element, so no length can move anything.
    pub(in crate::app) fn dimension_value_field(&self, view: &str, index: usize) -> Option<Element<'_, Message>> {
        let d = self.document.plan(view)?.dimensions.get(index)?;
        d.anchors.iter().flatten().find(|a| !a.lost)?;
        let value = self.modeller.dimension_value.clone().unwrap_or_else(|| format!("{:.3}", d.length()));
        Some(text_input("", &value).on_input(Message::DimensionValue).on_submit(Message::DimensionValueApply).size(12).padding([3, 6]).width(72).into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::click;
    use super::super::ModelTool;
    use super::*;
    use std::sync::Arc;

    /// Two walls along x, at y = 0 and y = 3, in the Modeller, with a floor plan and a dimension
    /// between them at x = 2, from the first wall to the second.
    fn dimensioned() -> (App, Arc<str>) {
        let mut app = super::super::tests::modelling();
        app.modeller.in_modeller = true;
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.0);
        app.set_model_tool(None);
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 3.0);
        click(&mut app, 6.0, 3.0);
        app.set_model_tool(None);
        let level = app.document.levels()[0].path.clone();
        let plan: Arc<str> = Arc::from(app.document.add_plan(&level).expect("a floor plan").as_str());
        let _ = app.update(Message::AddDimension(plan.clone(), [2.0, 0.0], [2.0, 3.0]));
        app.keep_model_history();
        (app, plan)
    }

    fn dimension(app: &App, plan: &str) -> crate::document::Dimension {
        app.document.plan(plan).unwrap().dimensions[0]
    }

    #[test]
    fn a_dimension_holds_on_to_the_walls_it_was_clicked_on_and_follows_them() {
        let (mut app, plan) = dimensioned();
        let d = dimension(&app, &plan);
        assert_eq!(d.anchors.map(|a| a.map(|a| (a.element, a.point))), [Some((1, 0)), Some((2, 0))]);
        // 2 m along each wall from its start, on its axis
        assert!(d.anchors.iter().flatten().all(|a| (a.offset[0] - 2.0).abs() < 1e-9 && a.offset[1].abs() < 1e-9));
        // the second wall moved a metre north: the dimension is a metre longer
        app.select_modelled(ElementId(2));
        app.transform_elements(&[ElementId(2)], &Map::Shift(DVec3::new(0.0, 1.0, 0.0)), false);
        let d = dimension(&app, &plan);
        assert_eq!((d.from, d.to), ([2.0, 0.0], [2.0, 4.0]));
        assert!((d.length() - 4.0).abs() < 1e-9);
        // its end dragged, the wall turns: the end stays 2 m along it
        let turned = with_end(&app, ElementId(2), DVec2::new(6.0, 7.0));
        app.modeller.model.replace(turned).unwrap();
        app.show_modelled();
        let d = dimension(&app, &plan);
        let along = DVec2::new(6.0, 3.0).normalize() * 2.0;
        assert!((DVec2::from_array(d.to) - (DVec2::new(0.0, 4.0) + along)).length() < 1e-9, "{:?}", d.to);
    }

    /// Wall `id` with its end at `end`.
    fn with_end(app: &App, id: ElementId, end: DVec2) -> bim_core::Element {
        let e = app.modeller.model.get(id).unwrap().clone();
        let grid = app.modeller.model.grid;
        let mut shape = e.shape_in_project(&grid);
        if let Shape::Planar(PlanarDef { form: PlanarForm::Wall { end: its, .. }, .. }) = &mut shape {
            *its = end;
        }
        e.reshaped(&grid, shape)
    }

    #[test]
    fn an_end_whose_wall_is_gone_is_marked_and_holds_on_again_when_it_comes_back() {
        let (mut app, plan) = dimensioned();
        app.select_modelled(ElementId(2));
        let _ = app.update(Message::DeletePicked);
        let d = dimension(&app, &plan);
        assert_eq!(d.anchors.map(|a| a.map(|a| a.lost)), [Some(false), Some(true)]);
        assert_eq!(d.to, [2.0, 3.0], "it stays where it was");
        let _ = app.update(Message::Undo);
        assert_eq!(dimension(&app, &plan).anchors.map(|a| a.map(|a| a.lost)), [Some(false), Some(false)]);
    }

    #[test]
    fn a_length_typed_moves_the_wall_picked_last_in_one_step() {
        let (mut app, plan) = dimensioned();
        assert!(app.set_dimension_length(&plan, 0, 3.6));
        let (p, _) = edit_points(&app.modeller.model.get(ElementId(2)).unwrap().shape_in_project(&app.modeller.model.grid));
        assert!((p[0].y - 3.6).abs() < 1e-9 && (p[1].y - 3.6).abs() < 1e-9, "{p:?}");
        let (first, _) = edit_points(&app.modeller.model.get(ElementId(1)).unwrap().shape_in_project(&app.modeller.model.grid));
        assert!(first[0].y.abs() < 1e-9, "the wall picked first stays");
        assert!((dimension(&app, &plan).length() - 3.6).abs() < 1e-9);
        app.keep_model_history();
        let _ = app.update(Message::Undo);
        assert!((dimension(&app, &plan).length() - 3.0).abs() < 1e-9, "one step back, and the dimension with it");
        // no length, or a dimension that holds on to nothing: nothing moves
        assert!(!app.set_dimension_length(&plan, 0, 0.0));
        let _ = app.update(Message::AddDimension(plan.clone(), [10.0, 10.0], [12.0, 10.0]));
        assert_eq!(app.document.plan(&plan).unwrap().dimensions[1].anchors, [None, None]);
        assert!(!app.set_dimension_length(&plan, 1, 5.0));
    }

    #[test]
    fn a_dimension_keeps_what_it_holds_on_to_in_the_file() {
        let (app, plan) = dimensioned();
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document_now(), "house.ifcx").unwrap();
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        let read = back.document.plans()[0].dimensions[0];
        assert_eq!(read.anchors, dimension(&app, &plan).anchors);
    }
}
