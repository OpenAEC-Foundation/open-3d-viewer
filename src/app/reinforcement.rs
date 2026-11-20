use super::{
    edit::{EditEvent, EditOverlay},
    App, Message,
};
use bim_core::{
    element::PlanarForm,
    reinforcement::{spans, Area, BarRun, Layout},
    Element, ElementId, LinearDef, ProfileDef, Shape,
};
use glam::{DVec2, DVec3, Vec3};
use iced::{
    widget::{button, column, text},
    Color,
};
use openaec_ui::Language;

#[derive(Clone, Debug)]
pub enum Action {
    Select(ElementId, String),
    Edit(ElementId, String),
    Hairpins(ElementId),
    Redraw,
    Finish,
    Cancel,
    Parameter(usize, String),
}

#[derive(Clone, Debug)]
pub struct Editor {
    wall: ElementId,
    area: Area,
    drawing: bool,
    cursor: Option<DVec2>,
    held: Option<usize>,
    error: Option<&'static str>,
    params: [String; 4],
}

#[derive(Clone, Copy)]
struct Frame {
    origin: DVec3,
    axis: DVec2,
    length: f64,
    height: f64,
    thickness: f64,
}
impl Frame {
    fn world(self, p: DVec2, depth: f64) -> DVec3 {
        self.origin + (self.axis * p.x + self.axis.perp() * depth).extend(p.y)
    }
    fn local(self, p: DVec3) -> DVec2 {
        let d = p - self.origin;
        DVec2::new(d.truncate().dot(self.axis), d.z)
    }
}

fn frame(model: &bim_core::Model, wall: ElementId) -> Option<Frame> {
    let Shape::Planar(p) = model.get(wall)?.shape_in_project(&model.grid) else {
        return None;
    };
    let PlanarForm::Wall {
        start,
        end,
        base,
        height,
        ..
    } = p.form
    else {
        return None;
    };
    Some(Frame {
        origin: start.extend(base),
        axis: (end - start).try_normalize()?,
        length: end.distance(start),
        height,
        thickness: p.thickness,
    })
}

// Legacy project exports identify their bar sets by host mark, face and direction.
fn legacy(e: &Element) -> Option<(&str, &str, bool)> {
    let Shape::Linear(LinearDef {
        profile: ProfileDef::Circle { .. },
        ..
    }) = &e.shape
    else {
        return None;
    };
    let mut words = e.name.split_whitespace();
    let host = words.next()?;
    let face = words.next()?;
    if !matches!(face, "dijk" | "kas") {
        return None;
    }
    let vertical = match words.next()? {
        "verticaal" => true,
        "horizontaal" => false,
        _ => return None,
    };
    words.next()?.parse::<u32>().ok()?;
    if words.next()? != "-"
        || !words.next()?.starts_with('B')
        || !words.next()?.starts_with('D')
        || words.next().is_some()
    {
        return None;
    }
    Some((host, face, vertical))
}

fn legacy_area(model: &bim_core::Model, wall: ElementId, face: &str) -> Option<Area> {
    let f = frame(model, wall)?;
    let host = model.get(wall)?.name.split_whitespace().next()?;
    // Copied walls can retain the same mark. Imported bars belong to the original host.
    let original = model.iter().find(|e| {
        e.name.split_whitespace().next() == Some(host)
            && matches!(&e.shape,Shape::Planar(p) if matches!(p.form,PlanarForm::Wall{..}))
    })?;
    if original.id != wall {
        return None;
    }
    let mut members = Vec::new();
    let mut samples = [Vec::new(), Vec::new()];
    let mut low = DVec2::splat(f64::INFINITY);
    let mut high = -low;
    for e in model.iter() {
        let Some((mark, side, v)) = legacy(e) else {
            continue;
        };
        if mark != host || side != face {
            continue;
        }
        let Shape::Linear(l) = e.shape_in_project(&model.grid) else {
            continue;
        };
        let ProfileDef::Circle { diameter } = l.profile else {
            continue;
        };
        let a = f.local(l.start);
        let b = f.local(l.end);
        low = low.min(a).min(b);
        high = high.max(a).max(b);
        let depth = (l.start - f.origin).truncate().dot(f.axis.perp());
        samples[usize::from(!v)].push((if v { a.x } else { a.y }, depth, diameter));
        members.push(e.id);
    }
    let mut run = |i: usize| -> Option<BarRun> {
        let s = &mut samples[i];
        s.sort_by(|a, b| a.0.total_cmp(&b.0));
        let &(first, depth, diameter) = s.first()?;
        let pitch = (s.last()?.0 - first) / (s.len().checked_sub(1)? as f64);
        (pitch.is_finite() && pitch > 0.).then_some(BarRun {
            pitch,
            depth,
            diameter,
            first,
        })
    };
    Some(Area {
        name: face.into(),
        members,
        layout: Layout::Net {
            vertical: run(0)?,
            horizontal: run(1)?,
        },
        contour: vec![
            low,
            DVec2::new(high.x, low.y),
            high,
            DVec2::new(low.x, high.y),
        ],
    })
}

fn area(model: &bim_core::Model, wall: ElementId, name: &str) -> Option<Area> {
    model
        .get(wall)?
        .reinforcement_areas
        .iter()
        .find(|a| a.name == name)
        .cloned()
        .or_else(|| legacy_area(model, wall, name))
}

impl App {
    pub(super) fn selected_rebar_key(&self) -> Option<(ElementId, String)> {
        if !self.modeller.in_modeller || !self.annotates {
            return None;
        }
        let primary = self.sel.primary()?;
        let crate::selection::Pickable::Element(shown) = primary else {
            return None;
        };
        let key = self.model.as_ref()?.element_key(shown);
        let entry = self
            .models
            .iter()
            .position(|e| Some(e.model.id) == self.modeller.shown)?;
        if key.model != entry as u32 {
            return None;
        }
        let id = ElementId(key.source);
        let model = &self.modeller.model;
        let e = model.get(id)?;
        if !matches!(e.shape, Shape::Linear(_)) {
            return None;
        }
        for wall in model.iter().filter(|w| !w.reinforcement_areas.is_empty()) {
            if let Some(a) = wall
                .reinforcement_areas
                .iter()
                .find(|a| a.members.contains(&id))
            {
                return Some((wall.id, a.name.clone()));
            }
        }
        let (host, face, _) = legacy(e)?;
        let wall = model.iter().find(|w| {
            w.name.split_whitespace().next() == Some(host)
                && matches!(&w.shape,Shape::Planar(p) if matches!(p.form,PlanarForm::Wall{..}))
        })?;
        Some((wall.id, face.into()))
    }
    fn select_members(&mut self, ids: &[ElementId]) {
        let Some(scene) = self.model.as_ref() else {
            return;
        };
        let Some(entry) = self
            .models
            .iter()
            .position(|e| Some(e.model.id) == self.modeller.shown)
        else {
            return;
        };
        let picks = ids
            .iter()
            .filter_map(|id| {
                scene.element_with_key(crate::model::ElementKey {
                    model: entry as u32,
                    source: id.0,
                })
            })
            .map(crate::selection::Pickable::Element);
        self.sel.clear();
        self.sel.extend(picks);
    }
    pub(super) fn expand_rebar_pick(&mut self) {
        if let Some((wall, name)) = self.selected_rebar_key() {
            if let Some(a) = area(&self.modeller.model, wall, &name) {
                self.select_members(&a.members);
            }
        }
    }

    pub(super) fn rebar_action(&mut self, action: Action) {
        if !self.modeller.in_modeller || !self.annotates {
            return;
        }
        match action {
            Action::Select(wall, name) => {
                if let Some(a) = area(&self.modeller.model, wall, &name) {
                    self.select_members(&a.members);
                    self.selection_changed();
                }
            }
            Action::Edit(wall, name) => {
                if let Some(a) = area(&self.modeller.model, wall, &name) {
                    self.begin_area(wall, a, false)
                }
            }
            Action::Hairpins(wall) => {
                let Some(f) = frame(&self.modeller.model, wall) else {
                    return;
                };
                let areas = &self.modeller.model.get(wall).unwrap().reinforcement_areas;
                let Some(n) =
                    (1..).find(|n| !areas.iter().any(|a| a.name == format!("Haarspelden {n}")))
                else {
                    return;
                };
                let half = f.thickness * 0.5 - 0.04;
                self.begin_area(
                    wall,
                    Area {
                        name: format!("Haarspelden {n}"),
                        contour: vec![],
                        members: vec![],
                        layout: Layout::Hairpins {
                            pitch: DVec2::splat(0.15),
                            diameter: 0.01,
                            leg: 0.15,
                            near: -half,
                            far: half,
                        },
                    },
                    true,
                );
            }
            Action::Redraw => {
                if let Some(e) = &mut self.modeller.rebar_edit {
                    e.area.contour.clear();
                    e.drawing = true;
                    e.held = None;
                    e.error = None
                }
            }
            Action::Cancel => self.modeller.rebar_edit = None,
            Action::Finish => self.finish_area(),
            Action::Parameter(i, s) => {
                if let Some(p) = self
                    .modeller
                    .rebar_edit
                    .as_mut()
                    .and_then(|e| e.params.get_mut(i))
                {
                    *p = s
                }
            }
        }
    }
    fn begin_area(&mut self, wall: ElementId, area: Area, drawing: bool) {
        let Some(f) = frame(&self.modeller.model, wall) else {
            return;
        };
        self.drop_grab();
        self.set_model_tool(None);
        self.modeller.edit = None;
        self.modeller.sketch = None;
        self.modeller.transform = None;
        let depth = match &area.layout {
            Layout::Net { vertical, .. } => vertical.depth,
            Layout::Hairpins { near, .. } => *near,
        };
        let normal = (f.axis.perp() * if depth < 0. { -1. } else { 1. })
            .extend(0.)
            .as_vec3();
        let m = self.model_matrix();
        let n = m.transform_vector3(normal).normalize();
        self.anim = None;
        self.camera.ortho = true;
        self.camera.yaw = n.y.atan2(n.x);
        self.camera.pitch = n.z.asin();
        self.camera.target =
            m.transform_point3(f.world(DVec2::new(f.length, f.height) * 0.5, 0.).as_vec3());
        self.camera.distance = (f.height.max(f.length / self.aspect().max(0.1) as f64) as f32
            * 0.65
            * m.transform_vector3(Vec3::Z).length())
            / (self.camera.fov_y * 0.5).tan();
        let params = match &area.layout {
            Layout::Hairpins {
                pitch,
                diameter,
                leg,
                ..
            } => [pitch.x, pitch.y, *diameter, *leg],
            _ => [0.; 4],
        }
        .map(|x| format!("{:.0}", x * 1000.));
        self.modeller.rebar_edit = Some(Editor {
            wall,
            area,
            drawing,
            cursor: None,
            held: None,
            error: None,
            params,
        });
    }
    fn area_point(&self, x: f32, y: f32, w: f32, h: f32) -> Option<DVec2> {
        let e = self.modeller.rebar_edit.as_ref()?;
        let f = frame(&self.modeller.model, e.wall)?;
        let inv = (self.camera.view_proj(w / h) * self.model_matrix()).inverse();
        let (u, v) = (2. * x / w - 1., 1. - 2. * y / h);
        let a = inv.project_point3(Vec3::new(u, v, 0.)).as_dvec3();
        let ray = inv.project_point3(Vec3::new(u, v, 1.)).as_dvec3() - a;
        let n = f.axis.perp().extend(0.);
        let den = ray.dot(n);
        if den.abs() < 1e-10 {
            return None;
        }
        let depth = match &e.area.layout {
            Layout::Net { vertical, .. } => vertical.depth,
            Layout::Hairpins { near, .. } => *near,
        };
        let t = ((f.origin - a).dot(n) + depth) / den;
        (t >= 0.).then(|| {
            ((f.local(a + t * ray) * 1000.).round() / 1000.)
                .clamp(DVec2::ZERO, DVec2::new(f.length, f.height))
        })
    }
    pub(super) fn rebar_hover(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        if self.modeller.rebar_edit.is_none() {
            return false;
        }
        let p = self.area_point(x, y, w, h);
        if let Some(e) = &mut self.modeller.rebar_edit {
            e.cursor = p
        }
        true
    }
    pub(super) fn rebar_point_click(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        if self.modeller.rebar_edit.is_none() {
            return false;
        }
        if let Some(p) = self.area_point(x, y, w, h) {
            let e = self.modeller.rebar_edit.as_mut().unwrap();
            if e.drawing && e.area.contour.len() >= 3 && e.area.contour[0].distance(p) <= 0.001 {
                e.drawing = false;
                return true;
            }
            if e.drawing && e.area.contour.last().is_none_or(|q| q.distance(p) > 0.001) {
                e.area.contour.push(p);
                e.error = None
            }
        }
        true
    }
    pub(super) fn rebar_event(&mut self, event: EditEvent) -> bool {
        if self.modeller.rebar_edit.is_none() {
            return false;
        }
        let p = if let EditEvent::Drag {
            x,
            y,
            width,
            height,
        } = event
        {
            self.area_point(x, y, width, height)
        } else {
            None
        };
        let e = self.modeller.rebar_edit.as_mut().unwrap();
        match event {
            EditEvent::Press { index, .. }
                if e.drawing && index == 0 && e.area.contour.len() >= 3 =>
            {
                e.drawing = false
            }
            EditEvent::Press { index, .. } if !e.drawing => e.held = Some(index),
            EditEvent::Drag { .. } => {
                if let (Some(i), Some(p)) = (e.held, p) {
                    e.area.contour[i] = p
                }
            }
            EditEvent::Release => e.held = None,
            _ => {}
        }
        true
    }
    pub(super) fn rebar_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let e = self.modeller.rebar_edit.as_ref()?;
        let f = frame(&self.modeller.model, e.wall)?;
        let depth = match &e.area.layout {
            Layout::Net { vertical, .. } => vertical.depth,
            Layout::Hairpins { near, .. } => *near,
        };
        let points: Vec<_> = e
            .area
            .contour
            .iter()
            .map(|p| f.world(*p, depth).as_vec3())
            .collect();
        let rubber = if e.drawing {
            points
                .last()
                .copied()
                .zip(e.cursor)
                .map(|(a, b)| vec![a, f.world(b, depth).as_vec3()])
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Some(EditOverlay {
            picked: vec![false; points.len()],
            points,
            closed: !e.drawing,
            lines: None,
            selected_sketch_line: Some(rubber),
            ink,
            fill,
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
        })
    }
    fn finish_area(&mut self) {
        let Some(editor) = self.modeller.rebar_edit.as_ref() else {
            return;
        };
        let mut a = editor.area.clone();
        let wall = editor.wall;
        let Some(f) = frame(&self.modeller.model, wall) else {
            return;
        };
        let result = (|| {
            if editor.drawing {
                return Err("Sluit de contour door op het beginpunt te klikken.");
            }
            bim_core::Profile::new(a.contour.clone(), vec![])
                .map_err(|_| "De contour moet een gesloten vlak zonder kruisingen zijn.")?;
            if let Layout::Hairpins {
                pitch,
                diameter,
                leg,
                ..
            } = &mut a.layout
            {
                let mut values = [0.; 4];
                for (i, s) in editor.params.iter().enumerate() {
                    values[i] = s.replace(',', ".").parse::<f64>().unwrap_or(0.) / 1000.;
                    if !values[i].is_finite() || values[i] <= 0. {
                        return Err("Gebruik positieve maten in millimeters.");
                    }
                }
                *pitch = DVec2::new(values[0], values[1]);
                *diameter = values[2];
                *leg = values[3];
            }
            if !editor.area.members.is_empty()
                && matches!(a.layout, Layout::Net { .. })
                && area(&self.modeller.model, wall, &a.name)
                    .is_some_and(|old| old.contour == a.contour)
            {
                return Ok(false);
            }
            let bars = make_bars(&a, f)?;
            let mut next = self.modeller.model.clone();
            let mut host = next.get(wall).cloned().ok_or("De wand ontbreekt.")?;
            let mut ids = Vec::with_capacity(bars.len());
            for (i, (start, end, diameter)) in bars.into_iter().enumerate() {
                let shape = Shape::Linear(LinearDef {
                    start,
                    end,
                    profile: ProfileDef::Circle { diameter },
                    roll: 0.,
                });
                if let Some(e) = a.members.get(i).and_then(|id| next.get(*id)) {
                    let mut e = e.reshaped(&next.grid, shape);
                    e.reinforcing_bar = true;
                    e.hairpin_leg = match a.layout {
                        Layout::Hairpins { leg, .. } => Some(leg),
                        _ => None,
                    };
                    let id = e.id;
                    next.replace(e).map_err(|_| "Staafgeometrie ongeldig.")?;
                    ids.push(id);
                } else {
                    let mut e = Element::in_project(
                        &next.grid,
                        format!("{} · {} · {}", host.name, a.name, i + 1),
                        host.level.clone(),
                        shape,
                    );
                    e.reinforcing_bar = true;
                    e.hairpin_leg = match a.layout {
                        Layout::Hairpins { leg, .. } => Some(leg),
                        _ => None,
                    };
                    ids.push(next.add(e).map_err(|_| "Staafgeometrie ongeldig.")?);
                }
            }
            for id in a.members.iter().skip(ids.len()) {
                next.remove(*id);
            }
            a.members = ids;
            host.reinforcement_areas.retain(|old| old.name != a.name);
            host.reinforcement_areas.push(a.clone());
            next.insert(host);
            self.modeller.model = next;
            Ok(true)
        })();
        match result {
            Ok(changed) => {
                self.modeller.rebar_edit = None;
                if changed {
                    self.show_modelled();
                }
                self.select_members(&a.members);
                self.selection_changed();
            }
            Err(msg) => self.modeller.rebar_edit.as_mut().unwrap().error = Some(msg),
        }
    }

    pub(super) fn rebar_bar(&self, l: Language) -> Option<iced::Element<'_, Message>> {
        if !self.modeller.in_modeller || !self.annotates {
            return None;
        }
        use openaec_ui::design::a11y::{self, Node, Role};
        let btn = |s: String, a| {
            let node = Node::new(Role::Button, s.clone());
            a11y::node(
                button(text(s).size(12)).on_press(Message::Reinforcement(a)),
                node,
            )
        };
        let mut col = column![].spacing(4);
        if let Some(e) = &self.modeller.rebar_edit {
            col = col
                .push(text(format!(
                    "{} · {}",
                    l.text("Wapeningsgebied", "Reinforcement area"),
                    e.area.name
                )))
                .push(
                    text(if e.drawing {
                        l.text(
                            "Klik de hoeken en klik het beginpunt om te sluiten.",
                            "Click corners and click the first point to close.",
                        )
                    } else {
                        l.text(
                            "Sleep een hoekpunt of teken de contour opnieuw.",
                            "Drag a corner or redraw the boundary.",
                        )
                    })
                    .size(12),
                );
            if matches!(e.area.layout, Layout::Hairpins { .. }) {
                for (i, label) in [
                    l.text("Afstand langs (mm)", "Horizontal spacing (mm)"),
                    l.text("Afstand hoog (mm)", "Vertical spacing (mm)"),
                    l.text("Diameter (mm)", "Diameter (mm)"),
                    l.text("Recht been (mm)", "Straight leg (mm)"),
                ]
                .iter()
                .enumerate()
                {
                    col = col.push(text(*label).size(12)).push(
                        iced::widget::text_input("", &e.params[i])
                            .on_input(move |s| Message::Reinforcement(Action::Parameter(i, s))),
                    );
                }
            }
            if let Some(msg) = e.error {
                col = col.push(text(msg).size(12))
            }
            return Some(
                col.push(btn(
                    l.text("Contour opnieuw tekenen", "Redraw boundary").into(),
                    Action::Redraw,
                ))
                .push(btn(
                    l.text("Klaar (Tab)", "Finish (Tab)").into(),
                    Action::Finish,
                ))
                .push(btn(l.text("Annuleren", "Cancel").into(), Action::Cancel))
                .into(),
            );
        }
        let selected = self.selected_modelled();
        let active = self.selected_rebar_key();
        let wall = active.as_ref().map(|k| k.0).or_else(|| {
            if let [id] = selected.as_slice() {
                Some(*id)
            } else {
                None
            }
        })?;
        frame(&self.modeller.model, wall)?;
        let host = self.modeller.model.get(wall)?;
        let mark = host.name.split_whitespace().next()?;
        let mut names: Vec<_> = host
            .reinforcement_areas
            .iter()
            .map(|a| a.name.clone())
            .collect();
        for face in ["dijk", "kas"] {
            if !names.iter().any(|n| n == face)
                && self
                    .modeller
                    .model
                    .iter()
                    .any(|e| legacy(e).is_some_and(|(h, s, _)| h == mark && s == face))
            {
                names.push(face.into());
            }
        }
        col = col.push(text(l.text("Wandwapening", "Wall reinforcement")));
        for name in names {
            col = col.push(btn(name.clone(), Action::Select(wall, name)))
        }
        if let Some((_, name)) = active {
            col = col.push(btn(
                format!("{} {} (Tab)", l.text("Bewerk", "Edit"), name),
                Action::Edit(wall, name),
            ))
        }
        Some(
            col.push(btn(
                l.text("Haarspeldgebied tekenen", "Draw hairpin area")
                    .into(),
                Action::Hairpins(wall),
            ))
            .into(),
        )
    }
}

type Bar = (DVec3, DVec3, f64);

fn stations(lo: f64, hi: f64, first: f64, pitch: f64) -> Result<Vec<f64>, &'static str> {
    if !pitch.is_finite() || pitch <= 0. {
        return Err("Ongeldige staafafstand.");
    }
    let i = ((lo - first) / pitch - 1e-8).ceil();
    let j = ((hi - first) / pitch + 1e-8).floor();
    if !i.is_finite() || !j.is_finite() || j - i > 10000. {
        return Err("Te veel staven voor dit gebied.");
    }
    Ok((0..(j - i + 1.).max(0.) as usize)
        .map(|k| first + (i + k as f64) * pitch)
        .collect())
}

fn make_bars(a: &Area, f: Frame) -> Result<Vec<Bar>, &'static str> {
    let mut bars = Vec::new();
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = -lo;
    for &p in &a.contour {
        lo = lo.min(p);
        hi = hi.max(p)
    }
    match &a.layout {
        Layout::Net {
            vertical,
            horizontal,
        } => {
            for (v, r) in [(true, vertical), (false, horizontal)] {
                let i = usize::from(!v);
                for at in stations(lo[i], hi[i], r.first, r.pitch)? {
                    for (low, high) in spans(&a.contour, v, at) {
                        let point = |s| {
                            f.world(
                                if v {
                                    DVec2::new(at, s)
                                } else {
                                    DVec2::new(s, at)
                                },
                                r.depth,
                            )
                        };
                        bars.push((point(low), point(high), r.diameter));
                    }
                }
            }
        }
        Layout::Hairpins {
            pitch,
            near,
            far,
            diameter,
            leg,
        } => {
            let radius = (far - near) * 0.5;
            if radius <= *diameter || *leg <= 0. {
                return Err("De haarspeld past niet tussen de wandvlakken.");
            }
            for x in stations(
                lo.x + diameter / 2.,
                hi.x - diameter / 2.,
                lo.x + pitch.x / 2.,
                pitch.x,
            )? {
                for (bottom, top) in spans(&a.contour, true, x) {
                    for z in stations(
                        bottom + radius + diameter / 2.,
                        top - leg - diameter / 2.,
                        lo.y + radius + diameter / 2.,
                        pitch.y,
                    )? {
                        if bars.len() >= 10000 {
                            return Err("Verklein het gebied of vergroot de afstand.");
                        }
                        let point = |h, d| f.world(DVec2::new(x, h), d);
                        bars.push((point(z + leg, *near), point(z + leg, *far), *diameter));
                    }
                }
            }
        }
    }
    if bars.is_empty() {
        return Err("Er passen geen staven binnen deze contour en verdeling.");
    }
    Ok(bars)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> Frame {
        Frame {
            origin: DVec3::new(8., 4., 2.),
            axis: DVec2::Y,
            length: 5.,
            height: 3.,
            thickness: 0.30,
        }
    }
    #[test]
    fn concave_region_splits_mesh_bars_and_retains_face_depth() {
        let contour = [
            (0., 0.),
            (3., 0.),
            (3., 3.),
            (2., 3.),
            (2., 1.),
            (1., 1.),
            (1., 3.),
            (0., 3.),
        ]
        .map(|(x, y)| DVec2::new(x, y))
        .to_vec();
        let run = BarRun {
            pitch: 0.5,
            first: 0.,
            diameter: 0.01,
            depth: 0.09,
        };
        let a = Area {
            name: "kas".into(),
            members: vec![],
            contour,
            layout: Layout::Net {
                vertical: run.clone(),
                horizontal: run,
            },
        };
        let f = wall();
        let horizontal: Vec<_> = make_bars(&a, f)
            .unwrap()
            .into_iter()
            .filter(|(s, e, _)| s.z == 4. && e.z == 4.)
            .collect();
        assert_eq!(horizontal.len(), 2);
        assert_eq!(horizontal[0].0, f.world(DVec2::new(0., 2.), 0.09));
        assert_eq!(horizontal[1].0, f.world(DVec2::new(2., 2.), 0.09));
    }
    #[test]
    fn hairpins_follow_a_rotated_wall_and_keep_both_legs_inside_the_region() {
        let f = wall();
        let a = Area {
            name: "pins".into(),
            members: vec![],
            contour: vec![DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y],
            layout: Layout::Hairpins {
                pitch: DVec2::splat(0.25),
                diameter: 0.01,
                leg: 0.15,
                near: -0.1,
                far: 0.1,
            },
        };
        let bars = make_bars(&a, f).unwrap();
        assert!(!bars.is_empty());
        for (s, e, diameter) in bars {
            assert!((e - s).distance((f.axis.perp() * 0.2).extend(0.)) < 1e-8);
            let l = LinearDef {
                start: s,
                end: e,
                profile: ProfileDef::Circle { diameter },
                roll: 0.,
            };
            let solid = bim_core::reinforcement::hairpin_solid(&l, 0.15).unwrap();
            for p in solid.positions {
                assert!(
                    f.local(p).cmpge(DVec2::splat(-1e-8)).all()
                        && f.local(p).cmple(DVec2::ONE).all()
                );
            }
        }
    }
}
