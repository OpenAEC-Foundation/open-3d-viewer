//! The Door and Window tools: a click on a wall or a timber frame wall puts an opening there, as Revit's and Archicad's
//! door and window tools do. The opening is centred on the point clicked along the wall, kept
//! [`JAMB`] inside the wall's ends and rounded to 10 mm; a door stands on the wall's base, a window on its sill.
//! While the cursor is over a wall, the opening shows there as a box through the wall.
//!
//! The wall is found by the cursor's ray against each wall's box (axis, thickness, base, height),
//! so a click on a wall's face in 3D finds it as well as one on its axis in a floor plan.

use super::element_props::{DOOR, WINDOW};
use super::modeller::ModelTool;
use super::App;
use bim_core::element::{Opening, PlanarForm};
use bim_core::{Element, ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3, Vec3};

/// Openings are placed to this (metres).
const ROUND: f64 = 0.01;
/// The wall an opening keeps beside it at the wall's ends (metres): the kernel makes an opening
/// with wall on both sides, and a corner reaches into a wall's end.
const JAMB: f64 = 0.1;

/// Where a door or window would go: the wall, the opening, the point of the wall under the cursor,
/// and the wall's frame to draw it.
#[derive(Debug, Clone, PartialEq)]
pub struct Aim {
    pub wall: ElementId,
    pub opening: Opening,
    /// Where the cursor's ray meets the wall (scene coordinates).
    pub hit: DVec3,
    /// The wall's axis start, unit direction and square to it (in plan), its base and thickness.
    start: DVec2,
    along: DVec2,
    across: DVec2,
    base: f64,
    thickness: f64,
}

impl Aim {
    /// The opening as a box a little thicker than the wall: six faces of one ring each.
    pub fn faces(&self) -> Vec<Vec<Vec<Vec3>>> {
        let o = &self.opening;
        let half = self.thickness / 2.0 + 0.01;
        let corner = |s: f64, c: f64, z: f64| (self.start + self.along * s + self.across * c).extend(self.base + z).as_vec3();
        let (s0, s1, z0, z1) = (o.along, o.along + o.width, o.sill, o.sill + o.height);
        let box_corners = |i: usize| corner(if i & 1 == 0 { s0 } else { s1 }, if i & 2 == 0 { -half } else { half }, if i & 4 == 0 { z0 } else { z1 });
        // the corners of each face, by the bits of along (1), across (2) and up (4)
        [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]]
            .iter()
            .map(|f| vec![f.iter().map(|&i| box_corners(i)).collect()])
            .collect()
    }
}

impl App {
    /// Where `tool` (a door or a window) would go with a click at (x, y) in a viewport of
    /// width × height: on the nearest wall the cursor's ray meets. `None` when it meets none.
    pub(super) fn opening_at(&self, tool: ModelTool, x: f32, y: f32, width: f32, height: f32) -> Option<Aim> {
        // the size and sill typed in the strip
        let s = self.modeller.placement.of(tool);
        let (w, h, sill) = (s.width, s.height.unwrap_or(if tool == ModelTool::Door { DOOR.1 } else { WINDOW.1 }), if tool == ModelTool::Door { 0.0 } else { s.sill });
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        let d = inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3() - a;
        let grid = self.modeller.model.grid;
        let mut best: Option<(f64, Aim)> = None;
        for e in self.modeller.model.iter() {
            // a wall, or a framed system (a timber frame wall): what can take an opening
            let (start, end, base, height, thickness) = match e.shape_in_project(&grid) {
                Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, base, height }, .. }) => (start, end, base, height, p.total_thickness()),
                Shape::System(s) if s.system_type.as_ref().is_some_and(|t| t.framing.is_some()) => (s.start, s.end, s.base, s.height, s.member.max_element()),
                _ => continue,
            };
            let length = start.distance(end);
            if length <= 0.0 {
                continue;
            }
            let along = (end - start) / length;
            let across = along.perp();
            // the ray in the wall's frame: s along it, c across it, z up from its base
            let local = |q: DVec3| DVec3::new((q.truncate() - start).dot(along), (q.truncate() - start).dot(across), q.z - base);
            let (o, dir) = (local(a), DVec3::new(d.truncate().dot(along), d.truncate().dot(across), d.z));
            let Some(t) = enter_box(o, dir, DVec3::new(0.0, -thickness / 2.0, 0.0), DVec3::new(length, thickness / 2.0, height)) else { continue };
            if best.as_ref().is_some_and(|(b, _)| *b <= t) {
                continue;
            }
            let s = (o + dir * t).x;
            let placed = ((s - w / 2.0).clamp(JAMB, (length - w - JAMB).max(JAMB)) / ROUND).round() * ROUND;
            let aim = Aim { wall: e.id, opening: Opening { along: placed, sill, width: w, height: h }, hit: a + d * t, start, along, across, base, thickness };
            best = Some((t, aim));
        }
        best.map(|(_, aim)| aim)
    }

    /// A click of the Door or Window tool: the wall under the cursor gets the opening, if the
    /// kernel can make the wall with it; else the banner says why. The tool stays on.
    pub(super) fn place_opening(&mut self, tool: ModelTool, x: f32, y: f32, width: f32, height: f32) {
        let Some(aim) = self.opening_at(tool, x, y, width, height) else { return };
        let Some(e) = self.modeller.model.get(aim.wall).cloned() else { return };
        let grid = self.modeller.model.grid;
        let mut shape = e.shape_in_project(&grid);
        if let Some(openings) = super::element_props::openings_mut(&mut shape) {
            openings.push(aim.opening);
        }
        let mut changed = Element::in_project(&grid, e.name.clone(), e.level.clone(), shape);
        changed.id = e.id;
        match self.modeller.model.replace(changed) {
            Ok(()) => {
                self.modeller.refusal = None;
                self.show_modelled();
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }
}

/// Where a ray from `o` along `d` enters the box from `lo` to `hi` (as a multiple of `d`), if it
/// meets it in front of `o`.
fn enter_box(o: DVec3, d: DVec3, lo: DVec3, hi: DVec3) -> Option<f64> {
    let (mut near, mut far) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-12 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
        near = near.max(t0.min(t1));
        far = far.min(t0.max(t1));
    }
    (near <= far).then_some(near)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::viewport::Navigate;

    /// A new model seen from the top, 10 px per metre, the origin in the middle of 1000 × 800,
    /// with a wall from (0, 0) to (6, 0).
    fn a_wall() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        for x in [0.0, 6.0] {
            click(&mut app, x, 0.0);
        }
        app.set_model_tool(None);
        app
    }

    fn at(app: &App, x: f64, y: f64) -> (f32, f32) {
        app.on_screen(DVec3::new(x, y, 3.0), 1000.0, 800.0).unwrap()
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = at(app, x, y);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    fn openings(app: &App) -> Vec<Opening> {
        let Shape::Planar(p) = &app.modeller.model.get(ElementId(1)).unwrap().shape else { panic!() };
        p.openings.clone()
    }

    #[test]
    fn a_click_on_a_wall_puts_a_door_there_and_a_window_on_its_sill() {
        let mut app = a_wall();
        app.set_model_tool(Some((ModelTool::Door, "Door")));
        click(&mut app, 2.0, 0.05);
        app.set_model_tool(Some((ModelTool::Window, "Window")));
        click(&mut app, 4.5, -0.05);
        let placed = openings(&app);
        let expected = [Opening { along: 1.55, sill: DOOR.2, width: DOOR.0, height: DOOR.1 }, Opening { along: 3.9, sill: WINDOW.2, width: WINDOW.0, height: WINDOW.1 }];
        assert_eq!(placed.len(), 2);
        for (o, e) in placed.iter().zip(&expected) {
            assert!((o.along - e.along).abs() < 1e-9 && (o.sill, o.width, o.height) == (e.sill, e.width, e.height), "{o:?}");
        }
        assert!(app.modeller.tool.is_some(), "the tool stays on for the next");
        // beside the wall: nothing
        click(&mut app, 3.0, 2.0);
        assert_eq!(openings(&app).len(), 2);
    }

    #[test]
    fn a_click_on_a_timber_frame_wall_frames_a_door_and_a_window_and_a_curtain_wall_takes_none() {
        use bim_core::SystemDef;
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
        let id = app.add_element("Curtain wall", None, Shape::System(s)).unwrap();
        let framed = |app: &App| match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::System(s) => s.openings,
            _ => panic!(),
        };
        // a curtain wall is no wall to put a door in
        app.set_model_tool(Some((ModelTool::Door, "Door")));
        click(&mut app, 2.0, 0.0);
        assert!(framed(&app).is_empty());
        // as a timber frame it takes the door and the window where they are clicked
        app.select_modelled(id);
        assert_eq!(app.assign_system_type("system.timber-frame-140"), 1);
        // the Door tool is still on
        click(&mut app, 2.0, 0.0);
        assert_eq!((app.modeller.refusal.clone(), framed(&app).len()), (None, 1), "the door is framed");
        app.set_model_tool(Some((ModelTool::Window, "Window")));
        click(&mut app, 4.5, 0.0);
        let placed = framed(&app);
        let expected = [Opening { along: 1.55, sill: DOOR.2, width: DOOR.0, height: DOOR.1 }, Opening { along: 3.9, sill: WINDOW.2, width: WINDOW.0, height: WINDOW.1 }];
        assert_eq!(placed.len(), 2, "{placed:?}");
        for (o, e) in placed.iter().zip(&expected) {
            assert!((o.along - e.along).abs() < 1e-9 && (o.sill, o.width, o.height) == (e.sill, e.width, e.height), "{o:?}");
        }
        // and one step back takes the window out again
        let _ = app.update(Message::Undo);
        assert_eq!(framed(&app).len(), 1);
    }

    #[test]
    fn a_metal_stud_partition_is_built_and_takes_a_door() {
        use bim_core::SystemDef;
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
        let id = app.add_element("Curtain wall", None, Shape::System(s)).unwrap();
        app.select_modelled(id);
        for (kind, boards) in [("system.metal-stud-75-1", 2), ("system.metal-stud-100-2", 4)] {
            app.select_modelled(id);
            assert_eq!(app.assign_system_type(kind), 1, "{kind}");
            let parts = app.modeller.model.evaluate(id).unwrap();
            // studs at 600 over 6 m, between a bottom and a top track
            assert!(parts.len() > 12, "{kind}: {} parts", parts.len());
            let Shape::System(typed) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
            let t = typed.system_type.as_ref().unwrap();
            assert_eq!((t.member_material.as_str(), t.layers.len()), ("steel", boards), "{kind}");
        }
        app.set_model_tool(Some((ModelTool::Door, "Door")));
        click(&mut app, 3.0, 0.0);
        let Shape::System(with_door) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!((app.modeller.refusal.clone(), with_door.openings.len()), (None, 1));
    }

    #[test]
    fn near_the_end_the_opening_stays_inside_the_wall_and_shows_before_the_click() {
        let mut app = a_wall();
        app.set_model_tool(Some((ModelTool::Window, "Window")));
        let (sx, sy) = at(&app, 5.9, 0.0);
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let aim = app.modeller.aim.clone().expect("over the wall");
        assert!((aim.opening.along - (6.0 - WINDOW.0 - JAMB)).abs() < 1e-9, "{}", aim.opening.along);
        let preview = app.placing_overlay(iced::Color::BLACK).unwrap();
        assert_eq!(preview.faces.len(), 6, "the opening's box");
        let z: Vec<f32> = preview.faces.iter().flatten().flatten().map(|p| p.z).collect();
        assert!((z.iter().cloned().fold(f32::INFINITY, f32::min) - WINDOW.2 as f32).abs() < 1e-5);
        click(&mut app, 5.9, 0.0);
        assert_eq!(openings(&app).len(), 1);
    }

    #[test]
    fn a_ray_enters_a_box_where_it_meets_it() {
        let lo = DVec3::ZERO;
        let hi = DVec3::ONE;
        assert_eq!(enter_box(DVec3::new(0.5, 0.5, 5.0), -DVec3::Z, lo, hi), Some(4.0));
        assert_eq!(enter_box(DVec3::new(2.0, 0.5, 5.0), -DVec3::Z, lo, hi), None);
        assert_eq!(enter_box(DVec3::new(0.5, 0.5, -5.0), -DVec3::Z, lo, hi), None, "behind");
    }
}
