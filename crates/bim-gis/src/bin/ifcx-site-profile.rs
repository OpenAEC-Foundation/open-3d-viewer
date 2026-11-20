//! Rebuild native plan and section annotations from a registered orthophoto and a raw AHN coverage.
//! Usage: ifcx-site-profile input.ifcx settings.json ortho.jpg dtm.tif output.ifcx
use base64::{engine::general_purpose::STANDARD, Engine};
use bim_gis::{
    heights,
    rd::{Frame, Rd},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{error::Error, fs, path::Path};

type Pt = [f64; 2];
type Rect = [Pt; 2];

#[derive(Deserialize)]
struct Point {
    name: String,
    local: Pt,
}

#[derive(Deserialize)]
struct Settings {
    plan: String,
    coordinates: String,
    section: String,
    orthophoto_bounds_rd: Rect,
    source: String,
    registration_note: String,
    model_origin_rd: Pt,
    rotation_degrees: f64,
    wall_points: Vec<Point>,
    section_marks: [String; 2],
    section_local: [Pt; 2],
    /// Section coordinates are station from the first mark and elevation in metres NAP.
    toe: [Pt; 2],
    heel: [Pt; 2],
    slope_end: Pt,
    wall: Rect,
    footing: Rect,
    piles: Vec<Rect>,
    crest_search: Pt,
}

#[derive(Default)]
struct Marks {
    lines: Vec<[Pt; 2]>,
    notes: Vec<Value>,
    fills: Vec<Value>,
    pens: Vec<Value>,
}

impl Marks {
    fn pen(&mut self, colour: [u8; 3], width: f64) {
        self.pens.push(json!([self.lines.len(), colour, width]));
    }
    fn path(&mut self, points: &[Pt]) {
        self.lines.extend(points.windows(2).map(|v| [v[0], v[1]]));
    }
    fn note(&mut self, at: Pt, text: impl Into<String>) {
        self.notes
            .push(json!({"at":at,"text":text.into(),"size":0.0028}));
    }
    fn outline(&mut self, [a, b]: Rect, filled: bool) {
        let path = [a, [b[0], a[1]], b, [a[0], b[1]], a];
        self.path(&path);
        if filled {
            self.fills
                .push(json!({"loops":[&path[..4]],"color":[210,208,204]}));
        }
    }
    fn circle(&mut self, c: Pt, r: f64) {
        let points = (0..=24)
            .map(|i| {
                let t = i as f64 * std::f64::consts::TAU / 24.0;
                [c[0] + r * t.cos(), c[1] + r * t.sin()]
            })
            .collect::<Vec<_>>();
        self.path(&points);
    }
    fn apply(self, view: &mut Value) {
        view["lines"] = json!(self.lines);
        view["notes"] = json!(self.notes);
        view["fills"] = json!(self.fills);
        view["presentation"]["pens"] = json!(self.pens);
        view["presentation"]["grids"] = json!(false);
    }
}

fn view<'a>(data: &'a mut [Value], id: &str, kind: &str) -> Result<&'a mut Value, Box<dyn Error>> {
    data.iter_mut()
        .find(|r| r["path"] == id)
        .and_then(|r| r["attributes"].get_mut(kind))
        .ok_or_else(|| format!("Missing {kind}: {id}").into())
}

fn draw(
    s: &Settings,
    data: &mut [Value],
    image: &[u8],
    grid: &heights::Grid,
) -> Result<(), Box<dyn Error>> {
    let [sw, ne] = s.orthophoto_bounds_rd;
    let rd = |p: Pt| Rd::new(p[0], p[1]);
    if !rd(sw).valid()
        || !rd(ne).valid()
        || !rd(s.model_origin_rd).valid()
        || (0..2).any(|k| ne[k] <= sw[k])
        || s.wall_points.len() < 2
        || !s.rotation_degrees.is_finite()
        || s.section_marks.iter().any(|m| m.trim().is_empty())
    {
        return Err("Invalid RD registration or section marks".into());
    }
    if !image.starts_with(&[0xff, 0xd8, 0xff]) && !image.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Expected an orthophoto as JPEG or PNG".into());
    }
    let frame = Frame {
        origin: rd(s.model_origin_rd),
        rotation: s.rotation_degrees,
    };
    let map = |p: Pt| frame.rd(p[0], p[1]);
    let local = |p: Rd| [p.x - sw[0], p.y - sw[1]];
    let [a, b] = s.section_local.map(map);
    let bounds = [[0.0, 0.0], local(rd(ne))];
    let inside = |p: Pt| (0..2).all(|k| p[k] >= 0.0 && p[k] <= bounds[1][k]);
    let wall = s
        .wall_points
        .iter()
        .map(|p| local(map(p.local)))
        .collect::<Vec<_>>();
    if !wall.iter().copied().chain([local(a), local(b)]).all(inside) {
        return Err("Wall or section lies outside the registered orthophoto".into());
    }
    let mut plan = Marks::default();
    plan.pen([198, 52, 44], 0.8);
    plan.path(&wall);
    let mut table = Marks::default();
    let n = s.wall_points.len();
    table.note(
        [0.0, (n + 2) as f64 * 2.0],
        "RD New (EPSG:28992)  |  X / Y in m",
    );
    for (i, (p, q)) in s.wall_points.iter().zip(&wall).enumerate() {
        let r = map(p.local);
        plan.circle(*q, 0.65);
        plan.note([q[0] + 1.1, q[1] + 1.0], &p.name);
        table.note(
            [0.0, (n + 1 - i) as f64 * 2.0],
            format!("{}      {:.1}      {:.1}", p.name, r.x, r.y),
        );
    }
    table.note([0., 2.], &s.registration_note);
    table.note([0., 0.], &s.source);
    table.apply(view(data, &s.coordinates, "o3v::view::plan")?);
    plan.pen([255, 214, 85], 0.45);
    plan.path(&[local(a), local(b)]);
    for (p, name) in [
        (local(a), &s.section_marks[0]),
        (local(b), &s.section_marks[1]),
    ] {
        plan.note([p[0] + 1., p[1] + 1.], name);
    }
    let [x, y] = [bounds[1][0] - 6., bounds[1][1] - 5.];
    plan.path(&[
        [x, y - 5.],
        [x, y],
        [x - 0.9, y - 1.8],
        [x, y],
        [x + 0.9, y - 1.8],
    ]);
    plan.note([x - 0.6, y + 0.6], "N");
    let v = view(data, &s.plan, "o3v::view::plan")?;
    plan.apply(v);
    let p = &mut v["presentation"];
    p["image"] = json!(STANDARD.encode(image));
    p["bounds"] = json!(bounds);
    p["rd_origin"] = json!(sw);
    p["crs"] = json!("EPSG:28992");
    p["rd_unit"] = json!("metre");
    p["source"] = json!(s.source);
    let runs = grid.profile(a, b);
    let crest = runs
        .iter()
        .flatten()
        .filter(|p| p[0] >= s.crest_search[0] && p[0] <= s.crest_search[1])
        .max_by(|a, b| a[1].total_cmp(&b[1]))
        .copied()
        .ok_or("No measured AHN height in crest search interval")?;
    let mut section = Marks::default();
    section.pen([92, 104, 77], 0.28);
    let floor = s.piles.iter().map(|p| p[0][1]).fold(s.footing[0][1], f64::min) - 0.35;
    for run in &runs {
        let mut soil = run.clone();
        soil.push([run.last().unwrap()[0], floor]);
        soil.push([run[0][0], floor]);
        section.fills.push(json!({"loops":[soil],"color":[237,231,212]}));
        section.path(run);
    }
    section.pen([116, 81, 48], 0.5);
    section.path(&s.toe);
    section.path(&[s.heel[0], s.heel[1], s.slope_end]);
    section.pen([45, 44, 45], 0.5);
    section.outline(s.footing, true);
    section.outline(s.wall, true);
    section.pen([60, 57, 60], 0.18);
    for pile in &s.piles {
        section.outline(*pile, false);
    }
    section.note([s.toe[0][0] - 3.0, s.toe[0][1] + 0.6], format!("Teen {:+.2} m NAP", s.toe[0][1]));
    section.note([s.heel[1][0] + 1.0, s.heel[1][1] + 0.7], format!("Hiel {:+.2} m NAP", s.heel[1][1]));
    if runs.len() > 1 {
        section.note([runs[0][0][0], floor - 0.6], "AHN-maaiveld onderbroken waar metingen ontbreken");
    }
    let top = s.wall[1];
    section.note(
        [top[0] - 9., top[1] + 1.4],
        format!("Wandkop {:+.2} m NAP", top[1]),
    );
    section.note(
        [s.footing[1][0] + 1., s.footing[0][1] - 0.8],
        format!("Onderkant voet {:+.2} m NAP", s.footing[0][1]),
    );
    section.note(
        [crest[0] + 1.3, crest[1] + 0.9],
        format!("Kruin {:+.2} m NAP", crest[1]),
    );
    // A touring bicycle in side elevation, dimensions in metres.
    let at = |x: f64, y: f64| [crest[0] + x, crest[1] + y];
    section.circle(at(-0.52, 0.34), 0.34);
    section.circle(at(0.52, 0.34), 0.34);
    section.path(&[
        at(-0.52, 0.34),
        at(-0.15, 0.88),
        at(0.0, 0.35),
        at(-0.52, 0.34),
    ]);
    section.path(&[at(0.0, 0.35), at(0.35, 0.9), at(0.52, 0.34)]);
    section.path(&[at(-0.15, 0.88), at(0.35, 0.9)]);
    section.circle(at(-0.02, 1.52), 0.12);
    section.path(&[
        at(-0.03, 1.39),
        at(-0.24, 1.0),
        at(0.05, 0.78),
        at(0., 0.43),
    ]);
    section.path(&[at(-0.03, 1.36), at(0.13, 1.13), at(0.37, 1.01)]);
    let v = view(data, &s.section, "o3v::view::section")?;
    v["from"] = json!(s.section_local[0]);
    v["to"] = json!(s.section_local[1]);
    section.apply(v);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let [input, settings, ortho, terrain, output] = args.as_slice() else {
        return Err(
            "Usage: ifcx-site-profile input.ifcx settings.json ortho.jpg dtm.tif output.ifcx"
                .into(),
        );
    };
    let mut doc: Value = serde_json::from_slice(&fs::read(input)?)?;
    let s: Settings = serde_json::from_slice(&fs::read(settings)?)?;
    draw(
        &s,
        doc["data"].as_array_mut().ok_or("Invalid IFCX data")?,
        &fs::read(ortho)?,
        &heights::parse(&fs::read(terrain)?)?,
    )?;
    let bytes = serde_json::to_vec(&doc)?;
    let mut file = fs::File::create_new(Path::new(output))?;
    std::io::Write::write_all(&mut file, &bytes)?;
    Ok(())
}
