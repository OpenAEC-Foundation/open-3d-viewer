//! Elements between levels: a wall, a standing system and a column stand on their level (the
//! element's `level`) and reach either a height of their own or a level above, with an offset
//! (the element's `top`, see `bim_core::Top`).
//!
//! The shape keeps the absolute geometry; it is worked out again from the levels whenever one
//! moves or a top changes: a wall's or a system's height, a column's upper end. Moving a level
//! moves what stands on it and stretches what reaches up to it; what cannot be made so (a top at
//! or below its base) is refused and nothing changes. Removing a level frees the tops that reach
//! it, each keeping its height.

use crate::app::App;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Refusal, Shape, Top};

/// A member is a column when its ends lie this near above each other in plan (metres).
const UPRIGHT: f64 = 1e-6;

/// Whether an element of `shape` can reach a level with its top: a wall, a standing system, a
/// column.
pub(in crate::app) fn has_top(shape: &Shape) -> bool {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => true,
        Shape::System(s) => !s.lies(),
        Shape::Linear(l) => (l.end - l.start).truncate().length() <= UPRIGHT && l.end.z != l.start.z,
        _ => false,
    }
}

/// Where `shape` stands and how high its top is: (base, top), in its own coordinates.
pub(in crate::app) fn base_and_top(shape: &Shape) -> Option<(f64, f64)> {
    if !has_top(shape) {
        return None;
    }
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, height, .. }, .. }) => Some((*base, base + height)),
        Shape::System(s) => Some((s.base, s.base + s.height)),
        Shape::Linear(l) => Some((l.start.z.min(l.end.z), l.start.z.max(l.end.z))),
        _ => None,
    }
}

/// `shape` with its top at height `z`, its base where it was; a top at or below the base is
/// refused. `None` for a shape that has no top.
pub(in crate::app) fn topped(shape: &Shape, z: f64) -> Option<Result<Shape, Refusal>> {
    let (base, _) = base_and_top(shape)?;
    if z - base <= bim_core::LINEAR_TOLERANCE {
        return Some(Err(Refusal::NotPositive));
    }
    let mut shape = shape.clone();
    match &mut shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { height, .. }, .. }) => *height = z - base,
        Shape::System(s) => s.height = z - base,
        Shape::Linear(l) => {
            if l.end.z >= l.start.z {
                l.end.z = z;
            } else {
                l.start.z = z;
            }
        }
        _ => {}
    }
    Some(Ok(shape))
}

/// Gives every element of `model` that reaches a level the top that level says (`elevation`: a
/// level's height by its path; a level that is gone leaves its elements as they are). Returns
/// the elements that changed; the first one the kernel cannot make so is the refusal.
pub(in crate::app) fn fit_tops(model: &mut bim_core::Model, elevation: &dyn Fn(&str) -> Option<f64>) -> Result<Vec<ElementId>, Refusal> {
    let grid = model.grid;
    let reaching: Vec<bim_core::Element> = model.iter().filter(|e| e.top.is_some()).cloned().collect();
    let mut changed = Vec::new();
    for e in reaching {
        let Some(top) = &e.top else { continue };
        let Some(z) = elevation(&top.level).map(|z| z + top.offset) else { continue };
        let shape = e.shape_in_project(&grid);
        let Some(fitted) = topped(&shape, z) else { continue };
        let fitted = fitted?;
        if fitted != shape {
            model.replace(e.reshaped(&grid, fitted))?;
            changed.push(e.id);
        }
    }
    Ok(changed)
}

impl App {
    /// Gives element `id` the top `top` (`None`: a height of its own, the one it has): its shape
    /// follows. When the kernel cannot make it so, nothing changes.
    pub(in crate::app) fn set_top(&mut self, id: ElementId, top: Option<Top>) -> Result<(), Refusal> {
        let mut model = self.modeller.model.clone();
        let mut element = model.get(id).cloned().ok_or(Refusal::Missing)?;
        if top.is_some() && !has_top(&element.shape) {
            return Err(Refusal::NotPositive);
        }
        element.top = top;
        model.replace(element)?;
        fit_tops(&mut model, &|path| self.document.level(path).map(|l| l.elevation))?;
        self.modeller.model = model;
        self.joins_follow(&[id]);
        self.show_modelled();
        self.select_modelled(id);
        Ok(())
    }

    /// A level is gone: the elements that reached it keep their height as their own.
    pub(in crate::app) fn free_tops_of(&mut self, level: &str) {
        let reaching: Vec<bim_core::Element> = self.modeller.model.iter().filter(|e| e.top.as_ref().is_some_and(|t| t.level == level)).cloned().collect();
        for mut e in reaching {
            e.top = None;
            let _ = self.modeller.model.replace(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::*;
    use crate::app::transform::TransformKind;
    use bim_core::{LinearDef, ProfileDef};
    use glam::{DVec2, DVec3};

    /// A modelling app with Level 0 at 0 m and Level 1 at 3 m: their paths.
    fn two_levels() -> (App, String, String) {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let _ = app.document.add_level("Level 1", 3.0);
        let paths: Vec<String> = app.document.levels().iter().map(|l| l.path.clone()).collect();
        (app, paths[0].clone(), paths[1].clone())
    }

    fn wall(app: &mut App, level: &str, height: f64) -> ElementId {
        let form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(4.0, 0.0), base: 0.0, height };
        app.add_element("Wall", Some(level.to_string()), Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap()
    }

    /// A wall's base and height.
    fn stands(app: &App, id: ElementId) -> (f64, f64) {
        match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, height, .. }, .. }) => (base, height),
            other => panic!("{other:?}"),
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn a_wall_moved_keeps_its_top() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        app.select_modelled(w);
        app.begin_transform(TransformKind::Move);
        click(&mut app, 0.0, 0.0);
        click(&mut app, 0.0, 2.0);
        assert_eq!(app.modeller.model.get(w).unwrap().top, Some(Top { level: first, offset: 0.0 }));
    }

    #[test]
    fn a_wall_reaches_its_top_level_and_follows_it() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        assert!(close(stands(&app, w), (0.0, 3.0)), "{:?}", stands(&app, w));
        app.set_level(&first, "Level 1", 3.4).unwrap();
        assert!(close(stands(&app, w), (0.0, 3.4)), "{:?}", stands(&app, w));
        app.set_level(&ground, "Level 0", 0.2).unwrap();
        assert!(close(stands(&app, w), (0.2, 3.2)), "{:?}", stands(&app, w));
    }

    #[test]
    fn a_top_offset_and_a_column() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: -0.3 })).unwrap();
        assert!(close(stands(&app, w), (0.0, 2.7)));
        let column = Shape::Linear(LinearDef { start: DVec3::new(6.0, 0.0, 0.0), end: DVec3::new(6.0, 0.0, 2.5), profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0 });
        let c = app.add_element("Column", Some(ground.clone()), column).unwrap();
        app.set_top(c, Some(Top { level: first, offset: 0.0 })).unwrap();
        let Shape::Linear(l) = app.modeller.model.get(c).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert!((l.end.z - 3.0).abs() < 1e-9 && l.start.z == 0.0, "{l:?}");
    }

    #[test]
    fn a_level_moved_below_a_base_is_refused() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        assert!(app.set_level(&first, "Level 1", -1.0).is_err());
        assert_eq!(app.document.level(&first).unwrap().elevation, 3.0);
        assert!(close(stands(&app, w), (0.0, 3.0)));
    }

    #[test]
    fn removing_a_level_frees_the_tops_that_reach_it() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        let _ = app.update(crate::app::Message::RemoveLevel(std::sync::Arc::from(first.as_str())));
        assert!(app.document.level(&first).is_none());
        assert_eq!(app.modeller.model.get(w).unwrap().top, None);
        assert!(close(stands(&app, w), (0.0, 3.0)), "its height kept");
    }
}
