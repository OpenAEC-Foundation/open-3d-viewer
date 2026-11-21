//! A view in 3D of a thing of the library: a piece of a wall or a floor made of a build-up, a
//! length of a linear type or a steel profile, a system as the kernel generates it. A drag turns
//! it; pulled apart, its layers stand free of one another, so one sees how it is made.
//!
//! The solids are the kernel's (`bim_core`): a system's parts as a model would have them, a
//! profile extruded, a layer as a slab. They are drawn into a picture with a depth for every
//! pixel, so what lies behind is covered whatever way the thing is turned: each solid in its
//! material's colour, lit from the upper left, its edges drawn darker. A look at a type, not the
//! model's own viewport: it needs no model and no graphics card.

use super::libraries::{Item, LibrariesMessage, Shelf, Source};
use super::{App, Message, FONT, FONT_BOLD};
use crate::theme::Palette;
use bim_core::{Axes, Mesh, Opening, Profile, Solid, SystemDef};
use bim_library::{BuildUp, HatchKind, Library, LinearRole, ProfileShape, Role, SystemType};
use glam::{DVec2, DVec3};
use iced::mouse;
use iced::widget::canvas::{self, Frame};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, column, row, text, Space};
use iced::{Center, Color, Element, Fill, Point, Rectangle};
use openaec_ui::Language;
use std::cell::RefCell;

/// The colour of a part whose material the library does not have.
const GREY: [u8; 3] = [175, 182, 195];
/// How long and how high the piece of a wall is that shows a build-up, and how wide the square
/// piece of a floor, a roof or a ceiling (metres).
const LENGTH: f64 = 1.6;
const HEIGHT: f64 = 1.2;
/// How far layers pulled apart stand from one another (metres).
const GAP: f64 = 0.16;

/// One solid of a scene, in its material's colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    pub solid: Solid,
    pub colour: [u8; 3],
}

/// What the view shows: the solids of a thing, and the direction its layers follow one another
/// in (zero: it has no layers).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub bodies: Vec<Body>,
    pub across: DVec3,
}

/// A slab from `lo` to `hi`.
fn slab(lo: DVec3, hi: DVec3) -> Option<Solid> {
    let size = hi - lo;
    let profile = Profile::rectangle(size.x, size.y).ok()?;
    let axes = Axes::new(DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z), DVec3::X, DVec3::Z).ok()?;
    Solid::extrude(&profile, &axes, size.z).ok()
}

impl Scene {
    /// A piece of a wall, a floor, a roof or a ceiling of build-up `b`: a wall with its layers
    /// behind one another, the exterior in front; another with its layers under one another, the
    /// top one first. A cavity is left open.
    pub fn of_buildup(library: &Library, b: &BuildUp) -> Scene {
        let lying = b.role != Role::Wall;
        let mut bodies = Vec::new();
        let mut at = 0.0;
        for layer in &b.layers {
            let (from, to) = (at, at + layer.thickness);
            at = to;
            let material = library.material(&layer.material);
            if material.is_some_and(|m| m.hatch == HatchKind::Air) {
                continue;
            }
            let solid = if lying { slab(DVec3::new(0.0, 0.0, -to), DVec3::new(LENGTH, LENGTH, -from)) } else { slab(DVec3::new(0.0, from, 0.0), DVec3::new(LENGTH, to, HEIGHT)) };
            bodies.extend(solid.map(|solid| Body { solid, colour: material.map_or(GREY, |m| m.colour) }));
        }
        Scene { bodies, across: if lying { DVec3::Z } else { DVec3::Y } }
    }

    /// A length of a profile in `colour`: standing (a column, a pile) or lying along x.
    pub fn of_profile(shape: &ProfileShape, colour: [u8; 3], upright: bool) -> Scene {
        let size = shape.size();
        // four times as long as the profile is large, so its section still reads
        let length = (size.x.max(size.y) * 4.0).clamp(0.4, 3.0);
        let solid = shape.rings(5e-4).ok().and_then(|(outer, holes)| Profile::new(outer, holes).ok()).and_then(|profile| {
            let axes = if upright { Axes::new(DVec3::ZERO, DVec3::X, DVec3::Z) } else { Axes::new(DVec3::ZERO, DVec3::Y, DVec3::X) };
            Solid::extrude(&profile, &axes.ok()?, length).ok()
        });
        Scene { bodies: solid.into_iter().map(|solid| Body { solid, colour }).collect(), across: DVec3::ZERO }
    }

    /// System type `t` as the kernel generates it: about 2.4 m of it, whole bays and rows, with
    /// its layers; a framed one that stands with a window in the middle, as it frames one.
    pub fn of_system(library: &Library, t: &SystemType) -> Scene {
        let bays = (2.4 / t.grid.bay).ceil().clamp(2.0, 8.0);
        let rows = (2.4 / t.grid.row).ceil().clamp(1.0, 8.0);
        let (length, height) = (bays * t.grid.bay, rows * t.grid.row);
        let face = SystemDef { start: DVec2::ZERO, end: DVec2::new(length, 0.0), base: 0.0, height, spacing: DVec2::ONE, member: DVec2::ONE, panel: 1.0, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
        let lying = t.lying.is_some();
        let typed = if lying { face.laid(t) } else { face.typed(t) };
        let window = Opening { along: (length - 1.2) / 2.0, sill: 0.9, width: 1.2, height: 1.2, framing: None };
        let framed = !lying && t.members.framing.is_some() && length >= 2.0 && height >= 2.3;
        let with_window = typed.clone().filter(|_| framed).map(|s| SystemDef { openings: vec![window], ..s });
        let parts = with_window.and_then(|s| bim_core::system::parts(&s).ok()).or_else(|| typed.and_then(|s| bim_core::system::parts(&s).ok())).unwrap_or_default();
        let colour = |key: Option<&str>| key.and_then(|k| library.material(k)).map_or(GREY, |m| m.colour);
        let bodies = parts.into_iter().map(|part| Body { colour: colour(part.material.as_deref()), solid: part.solid }).collect();
        Scene { bodies, across: if lying { DVec3::Z } else { DVec3::Y } }
    }

    /// How many layers the scene has: solids whose middles lie at the same place across it are
    /// of one layer (the studs and plates of a frame, the panels of a face).
    pub fn layers(&self) -> usize {
        self.ranks().1
    }

    /// The layer of each solid, counted from 0 along `across`, and how many layers there are.
    fn ranks(&self) -> (Vec<usize>, usize) {
        if self.across == DVec3::ZERO {
            return (vec![0; self.bodies.len()], usize::from(!self.bodies.is_empty()));
        }
        // (to the millimetre)
        let place = |body: &Body| {
            let (lo, hi) = body.solid.bounds();
            (((lo + hi) / 2.0).dot(self.across) * 1000.0).round() as i64
        };
        let places: Vec<i64> = self.bodies.iter().map(place).collect();
        let mut distinct = places.clone();
        distinct.sort_unstable();
        distinct.dedup();
        (places.iter().map(|p| distinct.binary_search(p).unwrap_or(0)).collect(), distinct.len())
    }

    /// The scene with its layers pulled apart: each a gap further along `across` than the one
    /// before it, the middle one where it was.
    pub fn apart(&self) -> Scene {
        let (ranks, layers) = self.ranks();
        let middle = (layers.max(1) - 1) as f64 / 2.0;
        let bodies = self.bodies.iter().zip(ranks).map(|(body, rank)| Body { solid: body.solid.translated(self.across * ((rank as f64 - middle) * GAP)), colour: body.colour }).collect();
        Scene { bodies, across: self.across }
    }

    /// The box around all of it.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.bodies.iter().map(|body| body.solid.bounds()).reduce(|(lo, hi), (a, b)| (lo.min(a), hi.max(b)))
    }

    /// A number that changes when the scene does: what a kept picture is told by.
    fn stamp(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut add = |bits: u64| hash = (hash ^ bits).wrapping_mul(0x0000_0100_0000_01b3);
        for body in &self.bodies {
            add(u64::from(body.colour[0]) << 16 | u64::from(body.colour[1]) << 8 | u64::from(body.colour[2]));
            for p in &body.solid.positions {
                add(p.x.to_bits());
                add(p.y.to_bits());
                add(p.z.to_bits());
            }
        }
        hash
    }
}

/// `p` about the origin as the view sees it: turned `yaw` about the upright axis and tipped
/// `pitch` towards the eye. Its x runs to the right, its z up, and its y away from the eye.
fn turned(p: DVec3, yaw: f64, pitch: f64) -> DVec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (x, y) = (p.x * cy - p.y * sy, p.x * sy + p.y * cy);
    DVec3::new(x, y * cp - p.z * sp, y * sp + p.z * cp)
}

/// A picture of `scene` seen at `yaw` and `pitch` (radians), `width` by `height` pixels, as RGBA
/// on `paper`: every solid in its colour, lit from the upper left, the edges of its faces darker.
/// The scene keeps its size while it is turned. A pixel shows what is nearest the eye there.
pub fn picture(scene: &Scene, yaw: f64, pitch: f64, width: u32, height: u32, paper: [u8; 3]) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut pixels: Vec<u8> = [paper[0], paper[1], paper[2], 255].repeat(w * h);
    let Some((lo, hi)) = scene.bounds().filter(|_| w > 1 && h > 1) else { return pixels };
    let centre = (lo + hi) / 2.0;
    let radius = scene.bodies.iter().flat_map(|body| &body.solid.positions).map(|p| (*p - centre).length()).fold(1e-6, f64::max);
    let scale = (w.min(h) as f64) / 2.0 * 0.92 / radius;
    // a point on the picture: across, down, and how far from the eye
    let seen = |p: DVec3| {
        let t = turned(p - centre, yaw, pitch);
        (w as f64 / 2.0 + t.x * scale, h as f64 / 2.0 - t.z * scale, t.y)
    };
    let light = DVec3::new(-0.35, -0.62, 0.70).normalize();
    let mut depth = vec![f64::INFINITY; w * h];
    let paint = |pixels: &mut Vec<u8>, at: usize, colour: [u8; 3]| pixels[at * 4..at * 4 + 3].copy_from_slice(&colour);
    for body in &scene.bodies {
        let Ok(mesh) = Mesh::of(&body.solid) else { continue };
        for triangle in mesh.indices.chunks_exact(3) {
            let normal = turned(mesh.normals[triangle[0] as usize], yaw, pitch);
            // (a face turned away from the eye is behind its solid)
            if normal.y >= -1e-9 {
                continue;
            }
            let shade = 0.56 + 0.44 * normal.dot(light).max(0.0);
            let colour = body.colour.map(|c| (f64::from(c) * shade).round().clamp(0.0, 255.0) as u8);
            let [a, b, c] = [0, 1, 2].map(|k| seen(mesh.positions[triangle[k] as usize]));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if area.abs() < 1e-9 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize, (a.0.max(b.0).max(c.0).ceil().min(w as f64 - 1.0)).max(0.0) as usize);
            let (y0, y1) = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize, (a.1.max(b.1).max(c.1).ceil().min(h as f64 - 1.0)).max(0.0) as usize);
            for y in y0..=y1 {
                let py = y as f64 + 0.5;
                for x in x0..=x1 {
                    let px = x as f64 + 0.5;
                    // how much of each corner the pixel's middle has
                    let u = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                    let v = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                    let t = 1.0 - u - v;
                    if u < 0.0 || v < 0.0 || t < 0.0 {
                        continue;
                    }
                    let far = u * a.2 + v * b.2 + t * c.2;
                    let at = y * w + x;
                    if far < depth[at] {
                        depth[at] = far;
                        paint(&mut pixels, at, colour);
                    }
                }
            }
        }
    }
    // the edges of the faces that look at the eye, where nothing lies before them
    let near = radius * 4e-3;
    for body in &scene.bodies {
        let ink = body.colour.map(|c| (f64::from(c) * 0.34).round() as u8);
        for face in &body.solid.faces {
            let ring = |k: usize| -> Vec<DVec3> { face.rings[k].iter().map(|&i| body.solid.positions[i as usize]).collect() };
            let outer = ring(0);
            if outer.len() < 3 {
                continue;
            }
            // (Newell's sum: the face's normal whatever its shape)
            let normal = (0..outer.len()).fold(DVec3::ZERO, |n, k| {
                let (p, q) = (outer[k], outer[(k + 1) % outer.len()]);
                n + DVec3::new((p.y - q.y) * (p.z + q.z), (p.z - q.z) * (p.x + q.x), (p.x - q.x) * (p.y + q.y))
            });
            if turned(normal, yaw, pitch).y >= -1e-9 {
                continue;
            }
            for k in 0..face.rings.len() {
                let points = ring(k);
                for e in 0..points.len() {
                    let (from, to) = (seen(points[e]), seen(points[(e + 1) % points.len()]));
                    let steps = ((to.0 - from.0).abs().max((to.1 - from.1).abs()).ceil() as usize).max(1);
                    for s in 0..=steps {
                        let f = s as f64 / steps as f64;
                        let (x, y, far) = (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f, from.2 + (to.2 - from.2) * f);
                        if x < 0.0 || y < 0.0 || x >= w as f64 || y >= h as f64 {
                            continue;
                        }
                        let at = y as usize * w + x as usize;
                        if far <= depth[at] + near {
                            paint(&mut pixels, at, ink);
                        }
                    }
                }
            }
        }
    }
    pixels
}

/// The view of a scene on the page: a drag turns it.
struct SolidView {
    scene: Scene,
    paper: Color,
}

/// How the view is turned, and the picture drawn last with what it is a picture of.
pub struct Turn {
    yaw: f32,
    pitch: f32,
    /// Where the pointer took hold, and how the view was turned then.
    grab: Option<(Point, f32, f32)>,
    kept: RefCell<Option<((u64, u32, u32, u32, u32), iced::widget::image::Handle)>>,
}

impl Default for Turn {
    /// From the front, a little from the left and from above.
    fn default() -> Self {
        Turn { yaw: -0.62, pitch: 0.42, grab: None, kept: RefCell::new(None) }
    }
}

impl SolidView {
    /// The most pixels a picture has: enough to be sharp, few enough to follow a drag (fewer in
    /// a build that is not optimised, which draws them an order slower).
    const MOST: f32 = if cfg!(debug_assertions) { 520_000.0 } else { 1_600_000.0 };
}

impl<M> canvas::Program<M> for SolidView {
    type State = Turn;

    fn update(&self, state: &mut Turn, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<M>> {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = cursor.position_over(bounds)?;
                state.grab = Some((at, state.yaw, state.pitch));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (from, yaw, pitch) = state.grab?;
                let at = cursor.position()?;
                state.yaw = yaw + (at.x - from.x) * 0.01;
                state.pitch = (pitch + (at.y - from.y) * 0.01).clamp(-1.45, 1.45);
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.grab.take();
                None
            }
            _ => None,
        }
    }

    fn mouse_interaction(&self, state: &Turn, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match (state.grab.is_some(), cursor.is_over(bounds)) {
            (true, _) => mouse::Interaction::Grabbing,
            (false, true) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(&self, state: &Turn, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        // twice as many pixels as the page has points, while that stays light
        let finer = (Self::MOST / (bounds.width * bounds.height).max(1.0)).sqrt().clamp(0.5, 2.0);
        let (width, height) = ((bounds.width * finer).round().max(2.0) as u32, (bounds.height * finer).round().max(2.0) as u32);
        let paper = [self.paper.r, self.paper.g, self.paper.b].map(|c| (c * 255.0).round() as u8);
        let of = (self.scene.stamp() ^ u64::from(paper[0]) << 40 ^ u64::from(paper[1]) << 48 ^ u64::from(paper[2]) << 56, state.yaw.to_bits(), state.pitch.to_bits(), width, height);
        let mut kept = state.kept.borrow_mut();
        if kept.as_ref().is_none_or(|(was, _)| *was != of) {
            let pixels = picture(&self.scene, f64::from(state.yaw), f64::from(state.pitch), width, height, paper);
            *kept = Some((of, iced::widget::image::Handle::from_rgba(width, height, pixels)));
        }
        if let Some((_, handle)) = kept.as_ref() {
            frame.draw_image(Rectangle::new(Point::ORIGIN, bounds.size()), canvas::Image::new(handle.clone()));
        }
        vec![frame.into_geometry()]
    }
}

/// `scene` in 3D, `height` high and as wide as its place: a drag turns it.
pub(super) fn solid_view<'a, M: 'a>(scene: Scene, p: Palette, height: f32) -> Element<'a, M> {
    canvas_widget(SolidView { scene, paper: p.surface }).width(Fill).height(height).into()
}

impl App {
    /// The thing picked on the Library page as the 3D view shows it, with its name: the
    /// build-up, the linear type or the system picked of the project, or the one looked at of
    /// another library (a profile of the steel table too). A material, a hatch and a setting of
    /// the drawing standard have no view in 3D.
    pub fn library_scene(&self, l: Language) -> Option<(String, Scene)> {
        let lang = super::library::lang(l);
        let item = if self.libraries.source == Source::Project {
            let picked = |view: &Option<String>, item: fn(Shelf, String) -> Item| view.clone().map(|id| item(Shelf::Project, id));
            picked(&self.buildup_view.picked, Item::BuildUp).or_else(|| picked(&self.profile_view.picked, Item::Linear)).or_else(|| picked(&self.system_view.picked, Item::System))
        } else {
            self.libraries.looked.clone()
        }?;
        let colour = |library: &Library, key: &str| library.material(key).map_or(GREY, |m| m.colour);
        let found = match &item {
            Item::BuildUp(shelf, id) => {
                let library = self.shelf(*shelf);
                library.buildup(id).map(|b| (library.name(id, lang).unwrap_or_default(), Scene::of_buildup(library, b)))
            }
            Item::Linear(shelf, id) => {
                let library = self.shelf(*shelf);
                // a column and a pile stand; the others lie
                library.linear_type(id).map(|t| (library.linear_name(id, lang).unwrap_or_default(), Scene::of_profile(&t.profile, colour(library, &t.material), matches!(t.role, LinearRole::Column | LinearRole::Pile))))
            }
            Item::System(shelf, id) => {
                let library = self.shelf(*shelf);
                library.system_type(id).map(|t| (library.system_name(id, lang).unwrap_or_default(), Scene::of_system(library, t)))
            }
            Item::Steel(designation) => bim_library::profile::steel(designation).map(|s| (s.designation.clone(), Scene::of_profile(&s.shape, colour(self.shelf(Shelf::General), "steel"), false))),
            _ => None,
        };
        found.filter(|(_, scene)| !scene.bodies.is_empty())
    }

    /// The 3D tab of the Library page: the picked thing, turned by a drag, under it its name and
    /// the way to pull its layers apart.
    pub(super) fn solid_page(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some((name, scene)) = self.library_scene(l) else {
            let hint = l.text("Kies bij Tegels een opbouw, een staaftype, een staalprofiel of een systeem: hier staat het dan in 3D.", "Pick a build-up, a linear type, a steel profile or a system under Tiles: it stands here in 3D.");
            return text(hint).size(12).color(p.text_secondary).wrapping(Wrapping::Word).into();
        };
        let layered = scene.layers() > 1;
        let apart = self.libraries.apart && layered;
        let shown = if apart { scene.apart() } else { scene };
        let mut under = row![text(name).size(13).font(FONT_BOLD).color(p.text), text(l.text("Sleep om te draaien.", "Drag to turn it.")).size(11).color(p.text_muted), Space::new().width(Fill)].spacing(12).align_y(Center);
        if layered {
            let label = text(l.text("Uit elkaar", "Pulled apart")).size(12).font(if apart { FONT_BOLD } else { FONT }).color(if apart { p.text } else { p.text_secondary });
            under = under.push(button(label).padding([3, 10]).style(move |_, s| p.ribbon_button(apart, s)).on_press(Message::Libraries(LibrariesMessage::Apart(!apart))));
        }
        column![solid_view(shown, p, 520.0), under].spacing(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::SystemGroup;

    /// The colour of the pixel at `x`, `y` of a picture `width` wide.
    fn at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }

    /// Whether `colour` is `of` in some light: the same hue, darker or as it is.
    fn shade_of(colour: [u8; 3], of: [u8; 3]) -> bool {
        let ratio = |k: usize| f64::from(colour[k]) / f64::from(of[k].max(1));
        let r = ratio(0);
        (0.5..=1.01).contains(&r) && (0..3).all(|k| (ratio(k) - r).abs() < 0.03)
    }

    #[test]
    fn a_build_up_is_a_piece_of_wall_or_floor_with_its_layers() {
        let library = Library::starter();
        let wall = library.buildups_of(Role::Wall).find(|b| b.layers.len() >= 3).expect("a layered wall");
        let scene = Scene::of_buildup(&library, wall);
        let cavities = wall.layers.iter().filter(|l| library.material(&l.material).is_some_and(|m| m.hatch == HatchKind::Air)).count();
        assert_eq!((scene.bodies.len(), scene.layers(), scene.across), (wall.layers.len() - cavities, wall.layers.len() - cavities, DVec3::Y));
        // as thick as the build-up, the exterior in front
        let (lo, hi) = scene.bounds().expect("it has solids");
        assert!((hi.y - lo.y - wall.thickness()).abs() < 1e-9 && (hi.x - lo.x - LENGTH).abs() < 1e-9 && (hi.z - lo.z - HEIGHT).abs() < 1e-9, "{lo} {hi}");
        assert_eq!(scene.bodies[0].solid.bounds().0.y, 0.0);
        // pulled apart, every layer stands a gap from the next and none has changed its size
        let apart = scene.apart();
        let (alo, ahi) = apart.bounds().expect("it has solids");
        assert!((ahi.y - alo.y - (wall.thickness() + GAP * (scene.layers() - 1) as f64)).abs() < 1e-9, "{alo} {ahi}");
        for (was, now) in scene.bodies.iter().zip(&apart.bodies) {
            assert!((was.solid.volume() - now.solid.volume()).abs() < 1e-9);
        }
        // a floor lies: its layers under one another, the top one first
        let floor = library.buildups_of(Role::Floor).next().expect("a floor");
        let lying = Scene::of_buildup(&library, floor);
        let (lo, hi) = lying.bounds().expect("it has solids");
        assert!((hi.z - lo.z - floor.thickness()).abs() < 1e-9 && hi.z.abs() < 1e-9 && lying.across == DVec3::Z, "{lo} {hi}");
    }

    #[test]
    fn a_profile_is_a_length_of_it_and_a_system_its_parts() {
        let library = Library::starter();
        let steel = bim_library::profile::steel("HEA 200").expect("a profile of the table");
        let beam = Scene::of_profile(&steel.shape, [95, 100, 110], false);
        let (lo, hi) = beam.bounds().expect("a solid");
        // lying along x, its section upright: 200 wide and 190 high
        assert!((hi.x - lo.x - 0.8).abs() < 1e-9 && (hi.y - lo.y - 0.2).abs() < 1e-6 && (hi.z - lo.z - 0.19).abs() < 1e-6, "{lo} {hi}");
        assert_eq!((beam.layers(), beam.apart(), beam.bodies.len()), (1, beam.clone(), 1));
        let column = Scene::of_profile(&steel.shape, [95, 100, 110], true);
        let (lo, hi) = column.bounds().expect("a solid");
        assert!((hi.z - lo.z - 0.8).abs() < 1e-9 && (hi.x - lo.x - 0.2).abs() < 1e-6, "{lo} {hi}");
        // a timber frame: studs, plates and the layers on both sides, each in its material
        let frame = library.system_types_of(SystemGroup::TimberFrame).next().expect("a timber frame");
        let scene = Scene::of_system(&library, frame);
        assert!(scene.bodies.len() > 8 && scene.layers() >= 2, "{} solids in {} layers", scene.bodies.len(), scene.layers());
        let colours: std::collections::HashSet<[u8; 3]> = scene.bodies.iter().map(|b| b.colour).collect();
        assert!(colours.len() >= 2, "{colours:?}");
        // pulled apart it is thicker, and as long and as high as it was
        let ((lo, hi), (alo, ahi)) = (scene.bounds().expect("solids"), scene.apart().bounds().expect("solids"));
        assert!(ahi.y - alo.y > hi.y - lo.y + GAP * 0.9 && (ahi.x - alo.x - (hi.x - lo.x)).abs() < 1e-9 && (ahi.z - alo.z - (hi.z - lo.z)).abs() < 1e-9);
        // a floor system lies
        let floor = library.system_types_of(SystemGroup::Floor).next().expect("a floor system");
        let lying = Scene::of_system(&library, floor);
        assert!(!lying.bodies.is_empty() && lying.across == DVec3::Z, "{} solids", lying.bodies.len());
    }

    #[test]
    fn a_picture_shows_what_is_nearest_the_eye() {
        // two slabs behind one another, red in front (at y 0), blue behind
        let (red, blue, paper) = ([200, 40, 40], [40, 40, 200], [250, 250, 250]);
        let scene = Scene {
            bodies: vec![
                Body { solid: slab(DVec3::new(0.0, 0.0, 0.0), DVec3::new(1.0, 0.1, 1.0)).expect("a slab"), colour: red },
                Body { solid: slab(DVec3::new(0.0, 0.1, 0.0), DVec3::new(1.0, 0.2, 1.0)).expect("a slab"), colour: blue },
            ],
            across: DVec3::Y,
        };
        let (w, h) = (120, 100);
        // from the front the red one covers the blue one, whichever was drawn first
        let front = picture(&scene, 0.0, 0.0, w, h, paper);
        assert!(shade_of(at(&front, w, 60, 50), red), "{:?}", at(&front, w, 60, 50));
        let other_way = Scene { bodies: scene.bodies.iter().rev().cloned().collect(), across: scene.across };
        assert!(shade_of(at(&picture(&other_way, 0.0, 0.0, w, h, paper), w, 60, 50), red));
        // from behind the blue one shows, and the corners of the picture are paper
        let behind = picture(&scene, std::f64::consts::PI, 0.0, w, h, paper);
        assert!(shade_of(at(&behind, w, 60, 50), blue), "{:?}", at(&behind, w, 60, 50));
        assert_eq!((at(&front, w, 1, 1), at(&front, w, w - 2, h - 2)), (paper, paper));
        // from above and aside both show, and the red one's edges are drawn darker than its face
        let turned = picture(&scene, -0.6, 0.5, w, h, paper);
        let colours: std::collections::HashSet<[u8; 3]> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| at(&turned, w, x, y)).collect();
        assert!(colours.iter().any(|c| shade_of(*c, red) && c[0] > 150) && colours.iter().any(|c| shade_of(*c, blue) && c[2] > 100), "{colours:?}");
        assert!(colours.contains(&red.map(|c| (f64::from(c) * 0.34).round() as u8)), "the edges: {colours:?}");
        // it keeps its size while it turns: nothing reaches the picture's edge at any turn
        for step in 0..8 {
            let p = picture(&scene, f64::from(step) * 0.8, 0.7, w, h, paper);
            assert!((0..w).all(|x| at(&p, w, x, 0) == paper && at(&p, w, x, h - 1) == paper), "turn {step}");
        }
        // an empty scene is paper
        assert!(picture(&Scene::default(), 0.0, 0.0, 8, 8, paper).chunks_exact(4).all(|px| px == [250, 250, 250, 255]));
    }
}
