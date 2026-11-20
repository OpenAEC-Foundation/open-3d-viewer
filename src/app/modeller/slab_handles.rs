//! Shape handles of a floor: a selected slab has a grip on every edge of its outline, one on its
//! top and one on its underside, without edit mode. A lying system (a floor of slabs, a ceiling of
//! tiles) has the same grips on the four sides of its rectangle and on its top.
//!
//! - **An edge's grip** pushes that side in or out, along the edge's normal in plan. Its two
//!   corners slide along the neighbouring edges, so those keep their direction; where a neighbour
//!   runs parallel, the corner goes with the edge.
//! - **The top's grip** moves the top and keeps the underside; **the underside's** moves the
//!   underside and keeps the top. Either way the floor's thickness changes. A floor with a
//!   build-up is as thick as its layers: its top's grip moves the whole floor and its underside
//!   has none. A top that comes within [`LEVEL_SNAP`] of a level goes to it.
//! - **A lying system** is as thick as its type makes it: its top's grip moves the whole system
//!   and it has none on its underside. Its sides stay square to one another: the side on its line
//!   takes the line along, the far side changes how far it reaches, the two ends make its line
//!   longer or shorter. The kernel then divides it anew into its slabs or tiles.
//! - **Where a grip shows.** Only where its way shows on the screen: seen from above the edges
//!   have theirs, in a section the sides the view cuts and the top and underside. So the side of
//!   a floor is dragged in a section as it is in the floor plan.
//! - **A drag** runs along the grip's one line; it is the point of that line nearest the cursor,
//!   rounded as other moves are. Esc drops it. Put down, the kernel makes the floor anew; what it
//!   refuses (an outline that crosses itself, no thickness left) leaves the floor as it was and
//!   the banner says why.

use crate::app::edit::{EditEvent, EditOverlay};
use crate::app::grips::crossing;
use crate::app::App;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Shape, SystemDef};
use glam::{DVec2, DVec3, Vec3};
use iced::Color;

/// A drag is rounded to this (metres), as moves in edit mode are.
const ROUND: f64 = 0.01;
/// A top dragged to within this of a level (metres) goes to the level.
const LEVEL_SNAP: f64 = 0.1;
/// A grip shows when the view looks along its way by less than this (the cosine of the angle
/// between them): nearer than that, the way hardly shows on the screen.
const ALONG_VIEW: f64 = 0.94;

/// What a grip of a floor moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlabHandle {
    /// The edge from corner `i` of the outline to the next.
    Edge(usize),
    Top,
    Under,
}

/// A drag of a floor's grip under way.
#[derive(Debug, Clone, PartialEq)]
pub struct SlabDrag {
    pub element: ElementId,
    pub handle: SlabHandle,
    /// Where the grip was when taken (scene coordinates).
    pub origin: DVec3,
    /// The way it goes, of length 1.
    pub axis: DVec3,
    /// How far it has gone along `axis` (metres).
    pub along: f64,
}

/// The rectangle of a lying system in plan, counter-clockwise from its start: along its line,
/// then as far to the left of it as it reaches. `None` when its line has no length.
pub(in crate::app) fn lying_outline(s: &SystemDef) -> Option<Vec<DVec2>> {
    if !s.outline.is_empty() {
        return Some(s.outline.clone());
    }
    let left = (s.end - s.start).try_normalize()?.perp() * s.height;
    Some(vec![s.start, s.end, s.end + left, s.start + left])
}

/// The parts of a slab or of a lying system: its outline, its top, its thickness and whether
/// that thickness is given (by a build-up, by a system's type). `None` for anything else.
fn slab(shape: &Shape) -> Option<(Vec<DVec2>, f64, f64, bool)> {
    match shape {
        Shape::Planar(p @ PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) if outline.len() >= 3 => Some((outline.clone(), *top, p.total_thickness(), p.build_up.is_some())),
        Shape::System(s) if s.lies() => Some((lying_outline(s)?, s.base, s.member.y.max(s.panel), true)),
        _ => None,
    }
}

/// Twice the area of `outline`, above zero when it runs counter-clockwise.
fn turn(outline: &[DVec2]) -> f64 {
    let n = outline.len();
    (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum()
}

/// The outward normal of edge `i` of `outline` in plan, of length 1.
fn normal(outline: &[DVec2], i: usize) -> Option<DVec2> {
    let n = outline.len();
    let d = (outline[(i + 1) % n] - outline[i]).try_normalize()?;
    Some(if turn(outline) >= 0.0 { DVec2::new(d.y, -d.x) } else { DVec2::new(-d.y, d.x) })
}

/// The grips of a slab and the way each goes: every edge's middle (at half the floor's depth)
/// with its normal, the top's and the underside's middle with the vertical.
fn handles(shape: &Shape) -> Vec<(SlabHandle, DVec3, DVec3)> {
    let Some((outline, top, thickness, built_up)) = slab(shape) else { return Vec::new() };
    let outline = &outline[..];
    let n = outline.len();
    let mut out: Vec<(SlabHandle, DVec3, DVec3)> = (0..n)
        .filter_map(|i| {
            let middle = (outline[i] + outline[(i + 1) % n]) / 2.0;
            Some((SlabHandle::Edge(i), middle.extend(top - thickness / 2.0), normal(outline, i)?.extend(0.0)))
        })
        .collect();
    let middle = outline.iter().sum::<DVec2>() / n as f64;
    out.push((SlabHandle::Top, middle.extend(top), DVec3::Z));
    if !built_up {
        out.push((SlabHandle::Under, middle.extend(top - thickness), DVec3::Z));
    }
    out
}

/// Lying system `s` with `handle` gone `along` its way: the side on its line takes the line
/// along, the far side changes how far it reaches, an end makes the line longer or shorter, the
/// top moves all of it. `None` when nothing of it would be left, or it has no such grip.
fn pushed_lying(s: &SystemDef, handle: SlabHandle, along: f64) -> Option<SystemDef> {
    let dir = (s.end - s.start).try_normalize()?;
    let mut out = s.clone();
    match handle {
        SlabHandle::Edge(0) => {
            out.start -= dir.perp() * along;
            out.end -= dir.perp() * along;
            out.height += along;
        }
        SlabHandle::Edge(1) => out.end += dir * along,
        SlabHandle::Edge(2) => out.height += along,
        SlabHandle::Edge(3) => out.start -= dir * along,
        SlabHandle::Top => out.base += along,
        SlabHandle::Edge(_) | SlabHandle::Under => return None,
    }
    ((out.end - out.start).dot(dir) > ROUND / 2.0 && out.height > ROUND / 2.0).then_some(out)
}

/// `shape` (a slab or a lying system) with `handle` gone `along` its way: an edge out along its
/// normal, the top or the underside up. `None` when `shape` is neither or has no such edge, or
/// when nothing of a lying system would be left.
pub fn pushed(shape: &Shape, handle: SlabHandle, along: f64) -> Option<Shape> {
    if let Shape::System(s) = shape {
        if !s.lies() {
            return None;
        }
        if s.outline.is_empty() {
            return pushed_lying(s, handle, along).map(Shape::System);
        }
        // laid in an outline: an edge of the outline goes, as a slab's does
        let mut out = s.clone();
        match handle {
            SlabHandle::Edge(i) => {
                let (outline, n) = (&s.outline, s.outline.len());
                if i >= n {
                    return None;
                }
                let step = normal(outline, i)? * along;
                let (j, before, after) = ((i + 1) % n, (i + n - 1) % n, (i + 2) % n);
                let (a, b) = (outline[i] + step, outline[j] + step);
                out.outline[i] = crossing(outline[before], outline[i], a, b).unwrap_or(a);
                out.outline[j] = crossing(outline[j], outline[after], a, b).unwrap_or(b);
            }
            SlabHandle::Top => out.base += along,
            SlabHandle::Under => return None,
        }
        return Some(Shape::System(out));
    }
    let (outline, _, _, built_up) = slab(shape)?;
    let mut out = shape.clone();
    let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline: ring, top, .. }, thickness, .. }) = &mut out else { return None };
    match handle {
        SlabHandle::Edge(i) => {
            let n = outline.len();
            if i >= n {
                return None;
            }
            let step = normal(&outline, i)? * along;
            let (j, before, after) = ((i + 1) % n, (i + n - 1) % n, (i + 2) % n);
            let (a, b) = (outline[i] + step, outline[j] + step);
            // each corner slides along the neighbouring edge; a parallel neighbour lets it go with the edge
            ring[i] = crossing(outline[before], outline[i], a, b).unwrap_or(a);
            ring[j] = crossing(outline[j], outline[after], a, b).unwrap_or(b);
        }
        SlabHandle::Top => {
            *top += along;
            if !built_up {
                *thickness += along;
            }
        }
        SlabHandle::Under => *thickness -= along,
    }
    Some(out)
}

impl App {
    /// The selected floor with grips: one slab or lying system alone, outside edit mode and with
    /// no tool on.
    fn slab_gripped(&self) -> Option<ElementId> {
        if self.modeller.edit.is_some() || self.modeller.sketch.is_some() || self.modeller.tool.is_some() || self.measure.tool.is_some() || self.plane_picking {
            return None;
        }
        let [id] = self.selected_modelled()[..] else { return None };
        slab(&self.modeller.model.get(id)?.shape).map(|_| id)
    }

    /// The ray under viewport point (x, y): a point of it and its direction (scene coordinates).
    fn ray_under(&self, x: f32, y: f32, width: f32, height: f32) -> (DVec3, DVec3) {
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        (a, inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3() - a)
    }

    /// The grips of `shape` that show in the viewport: those whose way the view does not look
    /// along. With each, where it is and the way it goes.
    fn slab_handles_shown(&self, shape: &Shape) -> Vec<(SlabHandle, DVec3, DVec3)> {
        let (width, height) = (self.viewport_size.0.max(1.0), self.viewport_size.1.max(1.0));
        handles(shape)
            .into_iter()
            .filter(|(_, at, axis)| {
                let Some((x, y)) = self.on_screen(*at, width, height) else { return false };
                let (_, dir) = self.ray_under(x, y, width, height);
                dir.try_normalize().is_some_and(|d| d.dot(*axis).abs() < ALONG_VIEW)
            })
            .collect()
    }

    /// The grips of selected floor `id` that show, and where (scene coordinates).
    #[cfg(test)]
    pub(crate) fn slab_grips(&self, id: ElementId) -> Vec<(SlabHandle, DVec3)> {
        let Some(e) = self.modeller.model.get(id) else { return Vec::new() };
        self.slab_handles_shown(&e.shape_in_project(&self.modeller.model.grid)).into_iter().map(|(h, at, _)| (h, at)).collect()
    }

    /// How far the drag under way has gone, its top on a level when it comes near one.
    fn slab_along(&self, drag: &SlabDrag, shape: &Shape) -> f64 {
        let Some((_, top, _, _)) = slab(shape).filter(|_| drag.handle == SlabHandle::Top) else { return drag.along };
        let to = top + drag.along;
        let level = self.document.levels().iter().map(|l| l.elevation).filter(|e| (e - to).abs() <= LEVEL_SNAP).min_by(|a, b| (a - to).abs().total_cmp(&(b - to).abs()));
        level.map_or(drag.along, |e| e - top)
    }

    /// The dragged floor as the drag has it now (scene coordinates).
    fn slab_dragged(&self) -> Option<Shape> {
        let drag = self.modeller.slab_drag.as_ref()?;
        let shape = self.modeller.model.get(drag.element)?.shape_in_project(&self.modeller.model.grid);
        pushed(&shape, drag.handle, self.slab_along(drag, &shape))
    }

    /// The grips of the selected floor over the viewport, and its outline; while one is dragged,
    /// the floor as the drag has it. `None` when no floor is selected alone.
    pub(in crate::app) fn slab_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let dragged = self.modeller.slab_drag.as_ref().map(|d| d.handle);
        let shape = match dragged {
            Some(_) => self.slab_dragged()?,
            None => self.modeller.model.get(self.slab_gripped()?)?.shape_in_project(&self.modeller.model.grid),
        };
        let (outline, top, _, _) = slab(&shape)?;
        // the outline as the floor has it: its bowed edges as arcs
        let ring = match &shape {
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { bulges, .. }, .. }) => bim_core::element::bowed(&outline, bulges),
            _ => outline.clone(),
        };
        let shown = self.slab_handles_shown(&shape);
        Some(EditOverlay {
            selected_sketch_line: None,
            picked: shown.iter().map(|(h, _, _)| Some(*h) == dragged).collect(),
            points: shown.iter().map(|(_, at, _)| at.as_vec3()).collect(),
            closed: false,
            lines: Some((0..ring.len()).map(|i| [ring[i], ring[(i + 1) % ring.len()]].map(|p| p.extend(top).as_vec3())).collect()),
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// Something the overlay reports while a floor has its grips: a press takes grip `index` of
    /// those that show, a drag moves it along its way, letting go puts it down. False when no
    /// floor is selected alone and none is dragged: the event is another's.
    pub(in crate::app) fn slab_event(&mut self, event: EditEvent) -> bool {
        match event {
            EditEvent::Press { index, .. } => {
                let Some(element) = self.slab_gripped() else { return false };
                let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return true };
                if let Some(&(handle, origin, axis)) = self.slab_handles_shown(&shape).get(index) {
                    self.modeller.slab_drag = Some(SlabDrag { element, handle, origin, axis, along: 0.0 });
                    self.modeller.refusal = None;
                }
                true
            }
            EditEvent::Drag { x, y, width, height } => {
                let Some((origin, axis)) = self.modeller.slab_drag.as_ref().map(|d| (d.origin, d.axis)) else { return false };
                // the point of the grip's line nearest the cursor's ray
                let (a, dir) = self.ray_under(x, y, width, height);
                let (b, c) = (dir.dot(axis), dir.dot(dir));
                let r = a - origin;
                let denom = c - b * b;
                if denom.abs() > 1e-12 * c.max(1e-300) {
                    let t = (c * axis.dot(r) - b * dir.dot(r)) / denom;
                    if let Some(drag) = self.modeller.slab_drag.as_mut() {
                        drag.along = (t / ROUND).round() * ROUND;
                    }
                }
                true
            }
            EditEvent::Release => {
                let Some(drag) = self.modeller.slab_drag.clone() else { return false };
                let shape = self.slab_dragged();
                self.modeller.slab_drag = None;
                if drag.along == 0.0 {
                    return true;
                }
                // dragged until nothing of it is left: no floor
                let Some(shape) = shape else {
                    self.modeller.refusal = Some(bim_core::Refusal::NotPositive);
                    return true;
                };
                let Some(old) = self.modeller.model.get(drag.element).cloned() else { return true };
                let grid = self.modeller.model.grid;
                // an edge pushed through the far side turns the outline inside out: no floor
                let turned = |s: &Shape| slab(s).map(|(outline, ..)| turn(&outline) >= 0.0);
                let made = match turned(&shape) == turned(&old.shape_in_project(&grid)) {
                    true => self.modeller.model.replace(old.reshaped(&grid, shape)),
                    false => Err(bim_core::Refusal::SelfIntersecting),
                };
                match made {
                    Ok(()) => {
                        self.modeller.refusal = None;
                        self.show_modelled();
                        self.select_modelled(drag.element);
                    }
                    Err(refusal) => self.modeller.refusal = Some(refusal),
                }
                true
            }
        }
    }

    /// Esc while a floor's grip is dragged: the drag is dropped. False when none is.
    pub(in crate::app) fn slab_escape(&mut self) -> bool {
        self.modeller.slab_drag.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::Message;
    use crate::viewport::camera::ViewPreset;

    fn slab_shape(outline: &[(f64, f64)], top: f64, thickness: f64) -> Shape {
        let outline = outline.iter().map(|&(x, y)| DVec2::new(x, y)).collect();
        Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top, bulges: Vec::new(), role: Default::default(), slope: None }, thickness, openings: Vec::new(), build_up: None })
    }

    fn parts(shape: &Shape) -> (Vec<DVec2>, f64, f64) {
        let (outline, top, thickness, _) = slab(shape).unwrap();
        (outline, top, thickness)
    }

    /// A floor 4 × 3 m, its top at 3 m, 0.25 m thick, selected, seen from above.
    fn floor_selected() -> (App, ElementId) {
        let mut app = modelling();
        let id = app.add_element("Floor", None, slab_shape(&[(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)], 3.0, 0.25)).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn now(app: &App, id: ElementId) -> (Vec<DVec2>, f64, f64) {
        parts(&app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid))
    }

    /// Drags `handle` of the selected floor to under scene point `to` and lets go.
    fn drag(app: &mut App, id: ElementId, handle: SlabHandle, to: DVec3) {
        let index = app.slab_grips(id).iter().position(|(h, _)| *h == handle).expect("the grip shows");
        let (sx, sy) = app.on_screen(to, 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
    }

    fn close(a: &[DVec2], b: &[(f64, f64)]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(p, &(x, y))| (*p - DVec2::new(x, y)).length() < 1e-9)
    }

    const RECT: [(f64, f64); 4] = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];

    #[test]
    fn pushed_moves_an_edge_and_its_neighbours_follow() {
        let rect = slab_shape(&RECT, 3.0, 0.25);
        let (outline, ..) = parts(&pushed(&rect, SlabHandle::Edge(1), 1.0).unwrap());
        assert!(close(&outline, &[(0.0, 0.0), (5.0, 0.0), (5.0, 3.0), (0.0, 3.0)]), "{outline:?}");
        // drawn the other way round, out is still out
        let cw = slab_shape(&[(0.0, 0.0), (0.0, 3.0), (4.0, 3.0), (4.0, 0.0)], 3.0, 0.25);
        let (outline, ..) = parts(&pushed(&cw, SlabHandle::Edge(2), 1.0).unwrap());
        assert!(close(&outline, &[(0.0, 0.0), (0.0, 3.0), (5.0, 3.0), (5.0, 0.0)]), "{outline:?}");
        // a slanted neighbour: the corner slides along it
        let slanted = slab_shape(&[(0.0, 0.0), (4.0, 0.0), (6.0, 2.0), (0.0, 2.0)], 3.0, 0.25);
        let (outline, ..) = parts(&pushed(&slanted, SlabHandle::Edge(2), 1.0).unwrap());
        assert!(close(&outline, &[(0.0, 0.0), (4.0, 0.0), (7.0, 3.0), (0.0, 3.0)]), "{outline:?}");
        // an L: its inner edge pushed out, the corners along the neighbours
        let l = slab_shape(&[(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)], 3.0, 0.25);
        let (outline, ..) = parts(&pushed(&l, SlabHandle::Edge(2), 0.5).unwrap());
        assert!(close(&outline, &[(0.0, 0.0), (4.0, 0.0), (4.0, 2.5), (2.0, 2.5), (2.0, 4.0), (0.0, 4.0)]), "{outline:?}");
        assert!(pushed(&rect, SlabHandle::Edge(4), 1.0).is_none(), "no such edge");
    }

    #[test]
    fn top_and_underside_change_the_thickness() {
        let rect = slab_shape(&RECT, 3.0, 0.25);
        let (_, top, thickness) = parts(&pushed(&rect, SlabHandle::Top, 0.1).unwrap());
        assert!((top - 3.1).abs() < 1e-9 && (thickness - 0.35).abs() < 1e-9, "the underside stays at 2.75");
        let (_, top, thickness) = parts(&pushed(&rect, SlabHandle::Under, -0.1).unwrap());
        assert!((top - 3.0).abs() < 1e-9 && (thickness - 0.35).abs() < 1e-9, "the top stays");
    }

    #[test]
    fn a_selected_slab_has_a_grip_on_every_edge_and_dragging_one_moves_the_edge() {
        let (mut app, id) = floor_selected();
        // from above: the four edges; the top and the underside go the way the view looks
        let grips = app.slab_grips(id);
        assert_eq!(grips.iter().map(|(h, _)| *h).collect::<Vec<_>>(), (0..4).map(SlabHandle::Edge).collect::<Vec<_>>());
        assert!((grips[1].1 - DVec3::new(4.0, 1.5, 2.875)).length() < 1e-9);
        let overlay = app.grip_overlay(Color::BLACK, Color::WHITE).expect("grips");
        assert_eq!(overlay.points.len(), 4);
        assert_eq!(overlay.lines.as_ref().map(Vec::len), Some(4));
        // the east edge a metre out; the cursor off its line counts for nothing
        drag(&mut app, id, SlabHandle::Edge(1), DVec3::new(5.0, 2.6, 0.0));
        assert!(close(&now(&app, id).0, &[(0.0, 0.0), (5.0, 0.0), (5.0, 3.0), (0.0, 3.0)]), "{:?}", now(&app, id).0);
        assert!(app.modeller.slab_drag.is_none());
        assert_eq!(app.selected_modelled(), vec![id], "it stays selected");
        let _ = app.update(Message::Undo);
        assert!(close(&now(&app, id).0, &RECT), "one step back");
    }

    #[test]
    fn in_a_section_the_side_of_a_floor_is_dragged() {
        let (mut app, id) = floor_selected();
        app.camera.set_preset(ViewPreset::Front);
        // from the front: the two sides the view cuts, the top and the underside
        let shown: Vec<SlabHandle> = app.slab_grips(id).iter().map(|(h, _)| *h).collect();
        assert_eq!(shown, vec![SlabHandle::Edge(1), SlabHandle::Edge(3), SlabHandle::Top, SlabHandle::Under]);
        drag(&mut app, id, SlabHandle::Edge(1), DVec3::new(5.0, 0.0, 2.9));
        assert!(close(&now(&app, id).0, &[(0.0, 0.0), (5.0, 0.0), (5.0, 3.0), (0.0, 3.0)]), "{:?}", now(&app, id).0);
        // the top up 0.2 m: thicker, the underside where it was
        app.select_modelled(id);
        drag(&mut app, id, SlabHandle::Top, DVec3::new(1.0, 0.0, 3.2));
        let (_, top, thickness) = now(&app, id);
        assert!((top - 3.2).abs() < 1e-9 && (thickness - 0.45).abs() < 1e-9, "{top} {thickness}");
    }

    #[test]
    fn the_underside_dragged_above_the_top_is_refused() {
        let (mut app, id) = floor_selected();
        app.camera.set_preset(ViewPreset::Front);
        drag(&mut app, id, SlabHandle::Under, DVec3::new(2.5, 0.0, 3.4));
        assert!(app.modeller.refusal.is_some());
        let (_, top, thickness) = now(&app, id);
        assert!((top - 3.0).abs() < 1e-9 && (thickness - 0.25).abs() < 1e-9);
        // an edge pushed through the far side: the outline would cross itself
        app.camera.set_preset(ViewPreset::Top);
        drag(&mut app, id, SlabHandle::Edge(1), DVec3::new(-2.0, 1.5, 0.0));
        assert!(app.modeller.refusal.is_some());
        assert!(close(&now(&app, id).0, &RECT));
    }

    #[test]
    fn a_top_near_a_level_goes_to_it_and_esc_drops_a_drag() {
        let (mut app, id) = floor_selected();
        let _ = app.document.add_level("Level 1", 3.3);
        app.camera.set_preset(ViewPreset::Front);
        drag(&mut app, id, SlabHandle::Top, DVec3::new(2.0, 0.0, 3.26));
        assert!((now(&app, id).1 - 3.3).abs() < 1e-9, "{}", now(&app, id).1);
        app.select_modelled(id);
        let index = app.slab_grips(id).iter().position(|(h, _)| *h == SlabHandle::Edge(1)).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        assert!(app.modeller.slab_drag.is_some());
        let _ = app.update(Message::Escape);
        assert!(app.modeller.slab_drag.is_none());
        assert!(close(&now(&app, id).0, &RECT));
        assert_eq!(app.selected_modelled(), vec![id], "the floor stays selected");
    }

    /// A floor of hollow-core slabs, 3.6 m of line along y = 0 and 6 m wide, its top at 3 m,
    /// selected, seen from above.
    fn lying_selected() -> (App, ElementId) {
        let mut app = modelling();
        let library = bim_library::Library::starter();
        let generic = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.6, 0.0), base: 3.0, height: 6.0, spacing: DVec2::new(1.2, 6.0), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new() };
        let floor = generic.laid(library.system_type("system.hollow-core-260").unwrap()).unwrap();
        let id = app.add_element("Floor", None, Shape::System(floor)).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn lying(app: &App, id: ElementId) -> SystemDef {
        let Shape::System(s) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!("a system") };
        s
    }

    #[test]
    fn a_lying_system_has_a_grip_on_each_side_and_they_keep_it_a_rectangle() {
        let (mut app, id) = lying_selected();
        let shown: Vec<SlabHandle> = app.slab_grips(id).into_iter().map(|(h, _)| h).collect();
        assert_eq!(shown, (0..4).map(SlabHandle::Edge).collect::<Vec<_>>(), "from above: its four sides");
        let near = |a: DVec2, x: f64, y: f64| (a - DVec2::new(x, y)).length() < 1e-9;
        // the far side: it reaches farther, its line stays
        drag(&mut app, id, SlabHandle::Edge(2), DVec3::new(1.8, 7.2, 3.0));
        let s = lying(&app, id);
        assert!(near(s.start, 0.0, 0.0) && near(s.end, 3.6, 0.0) && (s.height - 7.2).abs() < 1e-9, "{s:?}");
        // the side on its line: the line goes along, the far side stays
        drag(&mut app, id, SlabHandle::Edge(0), DVec3::new(1.8, -1.2, 3.0));
        let s = lying(&app, id);
        assert!(near(s.start, 0.0, -1.2) && near(s.end, 3.6, -1.2) && (s.height - 8.4).abs() < 1e-9, "{s:?}");
        // its ends: the line longer at either one
        drag(&mut app, id, SlabHandle::Edge(1), DVec3::new(4.8, 3.0, 3.0));
        drag(&mut app, id, SlabHandle::Edge(3), DVec3::new(-1.2, 3.0, 3.0));
        let s = lying(&app, id);
        assert!(near(s.start, -1.2, -1.2) && near(s.end, 4.8, -1.2) && (s.height - 8.4).abs() < 1e-9, "{s:?}");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        assert_eq!(app.selected_modelled(), vec![id], "it stays selected");
    }

    #[test]
    fn a_lying_system_dragged_through_itself_is_refused_and_its_top_moves_all_of_it() {
        let (mut app, id) = lying_selected();
        let before = lying(&app, id);
        drag(&mut app, id, SlabHandle::Edge(2), DVec3::new(1.8, -3.0, 3.0));
        assert_eq!(app.modeller.refusal, Some(bim_core::Refusal::NotPositive));
        assert_eq!(lying(&app, id), before, "nothing changed");
        // in an elevation the top shows, and no underside: its type makes its thickness
        app.select_modelled(id);
        app.camera.set_preset(ViewPreset::Front);
        let shown: Vec<SlabHandle> = app.slab_grips(id).into_iter().map(|(h, _)| h).collect();
        assert!(shown.contains(&SlabHandle::Top) && !shown.contains(&SlabHandle::Under), "{shown:?}");
        drag(&mut app, id, SlabHandle::Top, DVec3::new(1.8, 0.0, 3.5));
        let s = lying(&app, id);
        assert!((s.base - 3.5).abs() < 1e-9 && (s.height - 6.0).abs() < 1e-9, "{s:?}");
    }
}
