//! The explorer's Library tab: the project's library (`bim-library`) of materials, layered
//! build-ups and linear types, and the steel table to take profiles from. A material shows its
//! hatch, a build-up its layers as a section, a profile its outline. **Import…** adds the types a
//! library file has and the project lacks; **Export…** saves the project's library as a file of
//! types only.

use super::{App, Message, Status, FONT_BOLD};
use crate::theme::Palette;
use bim_library::profile::{self, LinearRole, Series};
use bim_library::{BuildUp, DrawingStandard, HatchKind, Lang, Library, Material, ProfileShape, Role};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, column, container, row, scrollable, text, Column};
use iced::{Center, Color, Element, Fill, Padding, Point, Rectangle, Size, Task};
use openaec_ui::Language;
use std::collections::HashSet;

/// A group of the Library tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Materials,
    BuildUps(Role),
    Linear,
    Steel,
    Series(Series),
}

/// What is picked in the Library tab, shown with its drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Material(String),
    BuildUp(String),
    Linear(String),
    /// A profile of the steel table, by designation.
    Steel(String),
}

/// The Library tab's state: the groups opened and what is picked.
#[derive(Debug, Clone, Default)]
pub struct LibraryView {
    pub open: HashSet<Group>,
    pub picked: Option<Pick>,
}

#[derive(Debug, Clone)]
pub enum LibraryMessage {
    Toggle(Group),
    Pick(Pick),
    Import,
    /// The name and bytes of the library file picked.
    Imported(Option<(String, Vec<u8>)>),
    Export,
    /// The file written, or why not; `None` when the dialog was cancelled.
    Exported(Option<Result<String, String>>),
}

/// The library's language for an interface language.
pub fn lang(l: Language) -> Lang {
    match l {
        Language::Dutch => Lang::Nl,
        Language::English => Lang::En,
        Language::German => Lang::De,
        Language::French => Lang::Fr,
    }
}

fn role_label(role: Role, l: Language) -> &'static str {
    match role {
        Role::Wall => l.text("Wanden", "Walls"),
        Role::Floor => l.text("Vloeren", "Floors"),
        Role::Roof => l.text("Daken", "Roofs"),
        Role::Ceiling => l.text("Plafonds", "Ceilings"),
    }
}

fn linear_role_label(role: LinearRole, l: Language) -> &'static str {
    match role {
        LinearRole::Column => l.text("Kolom", "Column"),
        LinearRole::Beam => l.text("Balk", "Beam"),
        LinearRole::Member => l.text("Staaf", "Member"),
        LinearRole::Mullion => l.text("Stijl", "Mullion"),
        LinearRole::Skirting => l.text("Plint", "Skirting"),
        LinearRole::Pipe => l.text("Leiding", "Pipe"),
        LinearRole::Duct => l.text("Kanaal", "Duct"),
        LinearRole::CableTray => l.text("Kabelgoot", "Cable tray"),
        LinearRole::Pile => l.text("Paal", "Pile"),
    }
}

fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

/// Millimetres with up to one decimal.
fn mm(metres: f64, l: Language) -> String {
    bim_library::millimetres(metres, lang(l))
}

impl App {
    pub(super) fn library_update(&mut self, message: LibraryMessage) -> Task<Message> {
        match message {
            LibraryMessage::Toggle(group) => {
                if !self.library_view.open.remove(&group) {
                    self.library_view.open.insert(group);
                }
            }
            LibraryMessage::Pick(pick) => {
                self.library_view.picked = if self.library_view.picked.as_ref() == Some(&pick) { None } else { Some(pick) };
            }
            LibraryMessage::Import => return import_task(),
            LibraryMessage::Imported(None) => {}
            LibraryMessage::Imported(Some((name, bytes))) => self.status = self.import_library(&name, &bytes),
            LibraryMessage::Export => return export_task(&self.library),
            LibraryMessage::Exported(None) => {}
            LibraryMessage::Exported(Some(Ok(name))) => self.status = Status::Loaded(format!("Library saved as {name}")),
            LibraryMessage::Exported(Some(Err(e))) => self.status = Status::Error(e),
        }
        Task::none()
    }

    /// Adds the types of the library file `name` that the project lacks.
    pub(super) fn import_library(&mut self, name: &str, bytes: &[u8]) -> Status {
        let read = serde_json::from_slice(bytes).map_err(|e| e.to_string()).and_then(|v| bim_library::ifcx::read(&v).map_err(|e| e.to_string()));
        match read {
            Ok(other) => {
                let added = self.library.merge(&other);
                Status::Loaded(format!("{name}: {added} added to the library"))
            }
            Err(e) => Status::Error(format!("{name}: {e}")),
        }
    }

    /// The Library tab.
    pub(super) fn library_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lib = &self.library;
        let lg = lang(l);
        let view = &self.library_view;
        let open = |g: Group| view.open.contains(&g);
        let send = |m: LibraryMessage| Message::Library(m);
        let header = |label: String, count: usize, group: Group| -> Element<'_, Message> {
            let mark = if open(group) { "▾" } else { "▸" };
            button(
                row![
                    text(mark).size(11).color(p.text_secondary),
                    text(label).size(12).font(FONT_BOLD).width(Fill),
                    text(count.to_string()).size(11).color(p.text_secondary),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 6])
            .width(Fill)
            .style(move |_, s| p.ribbon_button(false, s))
            .on_press(send(LibraryMessage::Toggle(group)))
            .into()
        };
        let item = |label: Element<'static, Message>, pick: Pick| -> Element<'_, Message> {
            let picked = view.picked.as_ref() == Some(&pick);
            button(label)
                .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 18.0 })
                .width(Fill)
                .style(move |_, s| if picked { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(send(LibraryMessage::Pick(pick)))
                .into()
        };
        let name = |s: String| text(s).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);

        let actions = row![
            button(text(l.text("Importeren…", "Import…")).size(12)).padding([4, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(send(LibraryMessage::Import)),
            button(text(l.text("Exporteren…", "Export…")).size(12)).padding([4, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(send(LibraryMessage::Export)),
        ]
        .spacing(6);
        let mut body = Column::new().spacing(1).push(container(actions).padding([2, 4]));

        // materials
        body = body.push(header(l.text("Materialen", "Materials").into(), lib.materials.len(), Group::Materials));
        if open(Group::Materials) {
            for m in &lib.materials {
                let colour = rgb(m.colour);
                let swatch = container(text("")).width(12).height(12).style(move |_| Palette::swatch(colour));
                let pick = Pick::Material(m.key.clone());
                body = body.push(item(row![swatch, name(m.name(lg).to_string())].spacing(6).align_y(Center).into(), pick.clone()));
                if view.picked.as_ref() == Some(&pick) {
                    body = body.push(detail(material_detail(p, m)));
                }
            }
        }

        // build-ups, per role
        for role in Role::ALL {
            let group = Group::BuildUps(role);
            let names = lib.names(role, lg);
            body = body.push(header(role_label(role, l).into(), names.len(), group));
            if !open(group) {
                continue;
            }
            for (id, label) in names {
                let pick = Pick::BuildUp(id.to_string());
                body = body.push(item(name(label).into(), pick.clone()));
                if view.picked.as_ref() == Some(&pick) {
                    if let Some(b) = lib.buildup(id) {
                        body = body.push(detail(buildup_detail(p, l, lib, b)));
                    }
                }
            }
        }

        // linear types, by role
        body = body.push(header(l.text("Lijntypen", "Linear types").into(), lib.linear_types.len(), Group::Linear));
        if open(Group::Linear) {
            for role in LinearRole::ALL {
                for (id, label) in lib.linear_names(role, lg) {
                    let pick = Pick::Linear(id.to_string());
                    let line = column![name(label), text(linear_role_label(role, l)).size(10).color(p.text_secondary)];
                    body = body.push(item(line.into(), pick.clone()));
                    if view.picked.as_ref() == Some(&pick) {
                        if let Some(t) = lib.linear_type(id) {
                            body = body.push(detail(profile_detail(p, &t.profile, lib.material(&t.material), None)));
                        }
                    }
                }
            }
        }

        // the steel table, per series
        let steel = profile::steel_profiles();
        body = body.push(header(l.text("Staalprofielen", "Steel profiles").into(), steel.len(), Group::Steel));
        if open(Group::Steel) {
            for series in Series::ALL {
                let group = Group::Series(series);
                let of: Vec<_> = steel.iter().filter(|s| s.series == series).collect();
                let title = match series {
                    Series::LEqual => format!("L {}", l.text("gelijkzijdig", "equal")),
                    Series::LUnequal => format!("L {}", l.text("ongelijkzijdig", "unequal")),
                    s => s.name().to_string(),
                };
                body = body.push(container(header(title, of.len(), group)).padding(Padding { left: 12.0, ..Padding::ZERO }));
                if !open(group) {
                    continue;
                }
                for s in of {
                    let pick = Pick::Steel(s.designation.clone());
                    body = body.push(item(name(s.designation.clone()).into(), pick.clone()));
                    if view.picked.as_ref() == Some(&pick) {
                        body = body.push(detail(profile_detail(p, &s.shape, lib.material("steel"), Some(s.series.standard()))));
                    }
                }
            }
        }
        scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }
}

/// The detail of a picked item, indented under it.
fn detail(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content).padding(Padding { top: 4.0, right: 8.0, bottom: 8.0, left: 18.0 }).width(Fill).into()
}

fn small<'a>(s: String, p: Palette) -> Element<'a, Message> {
    text(s).size(11).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).into()
}

/// A material's hatch (NEN 47) on a square, and its key.
fn material_detail<'a>(p: Palette, m: &Material) -> Element<'a, Message> {
    let sample = Section { bands: vec![(1.0, m.hatch, m.colour)], depth: 1.0, line: p.text };
    column![canvas_widget(sample).width(Fill).height(60), small(m.key.clone(), p)].spacing(4).into()
}

/// A build-up drawn as a section through its layers, the exterior (or the top) at the top, and
/// its layers listed.
fn buildup_detail<'a>(p: Palette, l: Language, lib: &Library, b: &BuildUp) -> Element<'a, Message> {
    let colour = |key: &str| lib.material(key).map_or([175, 182, 195], |m| m.colour);
    let hatch = |key: &str| lib.material(key).map_or(HatchKind::Generic, |m| m.hatch);
    let bands = b.layers.iter().map(|layer| (layer.thickness, hatch(&layer.material), colour(&layer.material))).collect();
    let total = b.thickness();
    let height = (total * 400.0).clamp(24.0, 150.0) as f32;
    let mut list = Column::new().spacing(2);
    let side = if b.role.counted_from_top() { l.text("Bovenzijde boven", "Top side up") } else { l.text("Buitenzijde boven", "Exterior side up") };
    list = list.push(small(side.to_string(), p));
    for layer in &b.layers {
        let material = lib.material(&layer.material);
        let c = rgb(colour(&layer.material));
        let swatch = container(text("")).width(10).height(10).style(move |_| Palette::swatch(c));
        let label = material.map_or(layer.material.clone(), |m| m.name(lang(l)).to_string());
        list = list.push(
            row![swatch, text(label).size(11).color(p.text).width(Fill).wrapping(Wrapping::WordOrGlyph), text(mm(layer.thickness, l)).size(11).color(p.text_secondary)]
                .spacing(6)
                .align_y(Center),
        );
    }
    let class = b.role.ifc_type_class();
    list = list.push(small(format!("{} mm · {class} {}", mm(total, l), b.role.predefined_type()), p));
    column![canvas_widget(Section { bands, depth: total, line: p.text }).width(Fill).height(height), list].spacing(6).into()
}

/// A profile drawn as its outline, with its sizes, area and second moments.
fn profile_detail<'a>(p: Palette, shape: &ProfileShape, material: Option<&Material>, standard: Option<&'static str>) -> Element<'a, Message> {
    let colour = material.map_or([95, 100, 110], |m| m.colour);
    let rings = shape.rings(1e-4).unwrap_or_default();
    let size = shape.size();
    let mut lines = vec![match *shape {
        ProfileShape::Rectangle { width, depth } => format!("b {} · h {} mm", mm_en(width), mm_en(depth)),
        ProfileShape::Circle { diameter } => format!("d {} mm", mm_en(diameter)),
        ProfileShape::I { h, b, tw, tf, r, .. } | ProfileShape::U { h, b, tw, tf, r, .. } => format!("h {} · b {} · tw {} · tf {} · r {} mm", mm_en(h), mm_en(b), mm_en(tw), mm_en(tf), mm_en(r)),
        ProfileShape::L { h, b, t, r1, .. } => format!("h {} · b {} · t {} · r {} mm", mm_en(h), mm_en(b), mm_en(t), mm_en(r1)),
        ProfileShape::T { h, b, tw, tf, .. } => format!("h {} · b {} · tw {} · tf {} mm", mm_en(h), mm_en(b), mm_en(tw), mm_en(tf)),
        ProfileShape::Rhs { h, b, t, .. } => format!("h {} · b {} · t {} mm", mm_en(h), mm_en(b), mm_en(t)),
        ProfileShape::Chs { d, t } => format!("d {} · t {} mm", mm_en(d), mm_en(t)),
    }];
    if let Ok(props) = shape.props() {
        lines.push(format!("A {:.1} cm² · Iy {:.0} cm⁴ · Iz {:.0} cm⁴", props.area * 1e4, props.iu * 1e8, props.iv * 1e8));
    }
    if let Some(standard) = standard {
        lines.push(standard.to_string());
    }
    let mut list = Column::new().spacing(2);
    for line in lines {
        list = list.push(small(line, p));
    }
    let outline = Outline { outer: rings.0, holes: rings.1, size: [size.x, size.y], fill: rgb(colour), line: p.text };
    column![canvas_widget(outline).width(Fill).height(130), list].spacing(6).into()
}

/// Millimetres as in a designation: whole, or one decimal with a point.
fn mm_en(metres: f64) -> String {
    bim_library::millimetres(metres, Lang::En)
}

/// A section through layers: bands from the top down, each `thickness` of `depth` with its hatch.
struct Section {
    bands: Vec<(f64, HatchKind, [u8; 3])>,
    depth: f64,
    line: Color,
}

impl canvas::Program<Message> for Section {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = (bounds.width - 2.0).min(220.0);
        let scale = (bounds.height - 2.0) / self.depth.max(1e-6) as f32; // px per metre
        let mut top = 1.0;
        for &(thickness, kind, _) in &self.bands {
            let h = thickness as f32 * scale;
            let band = Rectangle::new(Point::new(1.0, top), Size::new(width, h));
            draw_hatch(&mut frame, band, kind.pattern(DrawingStandard::Nen47), scale);
            top += h;
        }
        top = 1.0;
        for &(thickness, _, _) in &self.bands {
            let h = thickness as f32 * scale;
            frame.stroke(&Path::rectangle(Point::new(1.0, top), Size::new(width, h)), Stroke::default().with_color(self.line).with_width(1.0));
            top += h;
        }
        vec![frame.into_geometry()]
    }
}

/// A hatch in a band: its fill, then its lines (angle in degrees on paper, spacing in metres on
/// the model at `scale` px per metre), crossed lines, or one zigzag along the band.
fn draw_hatch(frame: &mut Frame, band: Rectangle, hatch: bim_library::HatchPattern, scale: f32) {
    frame.fill_rectangle(band.position(), band.size(), rgb(hatch.fill));
    let ink = Stroke::default().with_color(rgb(hatch.line)).with_width(0.8);
    if hatch.zigzag {
        let (x0, x1, y0, y1) = (band.x, band.x + band.width, band.y, band.y + band.height);
        let step = band.height.max(4.0);
        let path = Path::new(|b| {
            b.move_to(Point::new(x0, y1));
            let mut x = x0;
            let mut up = true;
            while x < x1 {
                x = (x + step / 2.0).min(x1);
                b.line_to(Point::new(x, if up { y0 } else { y1 }));
                up = !up;
            }
        });
        frame.stroke(&path, ink);
        return;
    }
    if hatch.spacing <= 0.0 {
        return;
    }
    let spacing = (hatch.spacing as f32 * scale).max(3.0);
    for angle in [hatch.angle as f32].into_iter().chain(hatch.crossed.then_some(hatch.angle as f32 + 90.0)) {
        for (a, b) in hatch_lines(band, angle, spacing) {
            frame.stroke(&Path::line(a, b), ink);
        }
    }
}

/// The parallel lines `spacing` apart at `angle` degrees (anticlockwise on paper, so clockwise on
/// screen, whose y runs down) clipped to `band`.
fn hatch_lines(band: Rectangle, angle: f32, spacing: f32) -> Vec<(Point, Point)> {
    let (sin, cos) = angle.to_radians().sin_cos();
    let d = (cos, -sin); // along the lines, on screen
    let n = (sin, cos); // across them
    let corners = [(band.x, band.y), (band.x + band.width, band.y), (band.x, band.y + band.height), (band.x + band.width, band.y + band.height)];
    let across: Vec<f32> = corners.iter().map(|c| c.0 * n.0 + c.1 * n.1).collect();
    let (lo, hi) = across.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mut lines = Vec::new();
    let mut c = (lo / spacing).ceil() * spacing;
    while c <= hi {
        // the line n·p = c through the band: clip p = p0 + t d to the rectangle
        let p0 = (n.0 * c, n.1 * c);
        let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
        let mut inside = true;
        for (o, dir, min, max) in [(p0.0, d.0, band.x, band.x + band.width), (p0.1, d.1, band.y, band.y + band.height)] {
            if dir.abs() < 1e-6 {
                inside &= o >= min && o <= max;
            } else {
                let (a, b) = ((min - o) / dir, (max - o) / dir);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if inside && t1 > t0 {
            lines.push((Point::new(p0.0 + d.0 * t0, p0.1 + d.1 * t0), Point::new(p0.0 + d.0 * t1, p0.1 + d.1 * t1)));
        }
        c += spacing;
    }
    lines
}

/// A profile's outline and holes, fitted in the canvas.
struct Outline {
    outer: Vec<glam::DVec2>,
    holes: Vec<Vec<glam::DVec2>>,
    size: [f64; 2],
    fill: Color,
    line: Color,
}

impl canvas::Program<Message> for Outline {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.outer.len() < 3 {
            return vec![frame.into_geometry()];
        }
        let scale = ((bounds.height - 8.0) / self.size[1].max(1e-6) as f32).min((bounds.width.min(220.0) - 8.0) / self.size[0].max(1e-6) as f32);
        let centre = Point::new(4.0 + self.size[0] as f32 * scale / 2.0, bounds.height / 2.0);
        let at = |q: &glam::DVec2| Point::new(centre.x + q.x as f32 * scale, centre.y - q.y as f32 * scale);
        let path = Path::new(|b| {
            for ring in std::iter::once(&self.outer).chain(&self.holes) {
                b.move_to(at(&ring[0]));
                for q in &ring[1..] {
                    b.line_to(at(q));
                }
                b.close();
            }
        });
        frame.fill(&path, canvas::Fill { style: canvas::Style::Solid(self.fill), rule: canvas::fill::Rule::EvenOdd });
        frame.stroke(&path, Stroke::default().with_color(self.line).with_width(1.0));
        vec![frame.into_geometry()]
    }
}

/// Asks for a library file and reads it: the native dialog on the desktop, the browser's file
/// picker on the web.
fn import_task() -> Task<Message> {
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new().set_title("Import library").add_filter("IFCX", &["ifcx"]).pick_file().await?;
            Some((file.file_name(), file.read().await))
        },
        |picked| Message::Library(LibraryMessage::Imported(picked)),
    )
}

/// The library file's bytes: IFCX of the library's types alone.
pub fn library_bytes(library: &Library, name: &str) -> Vec<u8> {
    let file = bim_library::ifcx::file(library, name, &format!("Open 3D Viewer Studio {}", env!("CARGO_PKG_VERSION")), &crate::export::ifcx::timestamp());
    serde_json::to_vec(&file).unwrap_or_default()
}

/// Asks where to save the library and writes it there.
#[cfg(not(target_arch = "wasm32"))]
fn export_task(library: &Library) -> Task<Message> {
    let library = library.clone();
    Task::perform(
        async move {
            let path = rfd::AsyncFileDialog::new().set_title("Export library").set_file_name("library.ifcx").add_filter("IFCX", &["ifcx"]).save_file().await?.path().to_path_buf();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = library_bytes(&library, &name);
            Some(std::fs::write(&path, bytes).map(|()| name.clone()).map_err(|e| format!("Cannot save {name}: {e}")))
        },
        |done| Message::Library(LibraryMessage::Exported(done)),
    )
}

/// The browser has no save dialog: the library goes to the downloads.
#[cfg(target_arch = "wasm32")]
fn export_task(library: &Library) -> Task<Message> {
    let bytes = library_bytes(library, "library.ifcx");
    Task::done(Message::Library(LibraryMessage::Exported(Some(crate::web::download("library.ifcx", &bytes, "application/json").map(|()| "library.ifcx".to_string())))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    fn click(app: &mut App, label: &str) {
        let mut ui = simulator(app);
        ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
        let messages: Vec<Message> = ui.into_messages().collect();
        for m in messages {
            let _ = app.update(m);
        }
    }

    #[test]
    fn the_library_tab_lists_the_starter_library_and_draws_a_picked_type() {
        let mut app = App::default();
        click(&mut app, "Library");
        assert_eq!(app.explorer_tab, super::super::ExplorerTab::Library);
        click(&mut app, "Walls");
        click(&mut app, "Brickwork-air cavity-PIR insulation-sand-lime brick 360");
        assert_eq!(app.library_view.picked, Some(Pick::BuildUp("wall.cavity-pir-360".into())));
        // its layers are listed under it
        {
            let mut ui = simulator(&app);
            assert!(ui.find("PIR insulation").is_ok());
            assert!(ui.find("Exterior side up").is_ok());
        }
        // the walls closed again, so the steel table is in view
        click(&mut app, "Walls");
        click(&mut app, "Steel profiles");
        click(&mut app, "HEA");
        click(&mut app, "HEA 200");
        assert_eq!(app.library_view.picked, Some(Pick::Steel("HEA 200".into())));
        let mut ui = simulator(&app);
        assert!(ui.find("A 53.8 cm² · Iy 3692 cm⁴ · Iz 1336 cm⁴").is_ok());
    }

    #[test]
    fn importing_a_library_file_adds_what_the_project_lacks() {
        let mut app = App::default();
        let mut other = Library::starter();
        other.add_linear_type(bim_library::LinearType::steel("column.hea-200", LinearRole::Column, "HEA 200").unwrap()).unwrap();
        let bytes = library_bytes(&other, "other.ifcx");
        let status = app.import_library("other.ifcx", &bytes);
        assert!(matches!(&status, Status::Loaded(s) if s == "other.ifcx: 1 added to the library"), "{status:?}");
        assert!(app.library.linear_type("column.hea-200").is_some());
        assert!(matches!(app.import_library("x.ifcx", b"{}"), Status::Error(_)));
    }

    #[test]
    fn hatch_lines_stay_in_their_band() {
        let band = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 30.0));
        for angle in [0.0, 45.0, 90.0, 135.0] {
            let lines = hatch_lines(band, angle, 8.0);
            assert!(!lines.is_empty(), "{angle}");
            for (a, b) in lines {
                for q in [a, b] {
                    assert!(q.x >= 10.0 - 1e-3 && q.x <= 110.0 + 1e-3 && q.y >= 20.0 - 1e-3 && q.y <= 50.0 + 1e-3, "{angle}: {q:?}");
                }
            }
        }
        assert_eq!(hatch_lines(band, 0.0, 8.0).len(), 4, "horizontal lines 8 px apart in a band 30 high");
    }
}
