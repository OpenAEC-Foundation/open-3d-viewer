//! Structural analysis models in the text format `.e2k`, as a building analysis package writes
//! them: a file of sections that start with `$`, each a list of lines of keywords, quoted names
//! and numbers. Read into the structural analysis model of `bim-structural`, in SI units.
//!
//! What is read:
//!
//! - `CONTROLS`: `UNITS` (force, length) and the titles.
//! - `STORIES - IN SEQUENCE FROM TOP`: each storey with its height; the lowest with its level.
//! - `POINT COORDINATES`: a point in plan (x, y) and, where given, how far it lies below its
//!   storey. A point stands on every storey it is used on: a node is a point on a storey.
//! - `LINE CONNECTIVITIES` and `LINE ASSIGNS`: a line between two points, the first as many
//!   storeys lower as its last number says (a column 1, a beam 0), and on which storeys it stands
//!   with which section, its turn (`ANG`) and its end releases (`RELEASE`).
//! - `AREA CONNECTIVITIES` and `AREA ASSIGNS`: floors and walls by their corner points, each
//!   as many storeys lower as its own number says; their thickness from their property.
//! - `MATERIAL PROPERTIES`, `FRAME SECTIONS`, the slab, deck and wall properties.
//! - `POINT ASSIGNS`: the restraints of a point on a storey.
//! - `LOAD PATTERNS`, the point, line and area loads, `LOAD CASES` and `LOAD COMBINATIONS`.
//!
//! What a file holds besides (grids, diaphragms, mass sources, design settings) is left out; a
//! load of a kind that is not read is noted in the model notes. The file holds no results.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use bim_structural::*;
use glam::{DVec2, DVec3};
use std::collections::HashMap;

pub struct E2kLoader;

impl Loader for E2kLoader {
    fn name(&self) -> &'static str {
        "Structural analysis model (text)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["e2k"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let text = String::from_utf8_lossy(data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data));
        progress.set(Phase::Convert);
        let model = read(&text, &source.name()).map_err(|e| format!("{}: {e}", source.name()))?;
        let doc = SamDocument { origin: DVec3::ZERO, source: model.source.clone(), models: vec![model] };
        let origin = super::sam::anchor(&doc);
        let mut info = super::sam::info(&doc, origin);
        info[0].1 = "Structural analysis model (text, .e2k)".to_string();
        Ok(Loaded { project: ProjectData { info, ..ProjectData::default() }, layers: Vec::new(), views: Vec::new(), origin: Some(origin), structural: doc.models })
    }
}

/// A word of a line: a keyword or number as written, or a name without its quotes.
#[derive(Debug, Clone, PartialEq)]
enum Word {
    Plain(String),
    Quoted(String),
}

impl Word {
    fn text(&self) -> &str {
        match self {
            Word::Plain(s) | Word::Quoted(s) => s,
        }
    }

    fn number(&self) -> Option<f64> {
        self.text().parse().ok().filter(|v: &f64| v.is_finite())
    }
}

/// The words of a line.
fn words(line: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            out.push(Word::Quoted(chars.by_ref().take_while(|&c| c != '"').collect()));
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek().filter(|c| !c.is_whitespace() && **c != '"') {
                word.push(c);
                chars.next();
            }
            out.push(Word::Plain(word));
        }
    }
    out
}

/// A line by its keywords: the value after `key`.
struct Line<'a>(&'a [Word]);

impl Line<'_> {
    fn after(&self, key: &str) -> Option<&Word> {
        let at = self.0.iter().position(|w| matches!(w, Word::Plain(k) if k.eq_ignore_ascii_case(key)))?;
        self.0.get(at + 1)
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.after(key)?.number()
    }

    fn name(&self, key: &str) -> Option<&str> {
        self.after(key).map(Word::text)
    }

    /// The word at `k`, counted from the keyword the line starts with.
    fn at(&self, k: usize) -> Option<&str> {
        self.0.get(k).map(Word::text)
    }
}

/// How many newtons a unit of force is, and how many metres a unit of length.
fn force_unit(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_uppercase().as_str() {
        "N" => 1.0,
        "KN" => 1e3,
        "MN" => 1e6,
        "KGF" => 9.806_65,
        "TONF" => 9_806.65,
        "LB" => 4.448_221_615_260_5,
        "KIP" => 4_448.221_615_260_5,
        _ => return None,
    })
}

fn length_unit(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_uppercase().as_str() {
        "MM" => 1e-3,
        "CM" => 1e-2,
        "M" => 1.0,
        "IN" => 0.0254,
        "FT" => 0.3048,
        _ => return None,
    })
}

/// A line object: its kind, its two points and how many storeys lower the first lies.
struct Bar {
    kind: String,
    points: [String; 2],
    below: i64,
}

/// An area object: its kind and its corner points, each with how many storeys lower it lies.
struct Panel {
    kind: String,
    corners: Vec<(String, i64)>,
}

/// The ids of what a file names, in the order first named.
#[derive(Default)]
struct Names {
    ids: HashMap<String, Id>,
}

impl Names {
    fn id(&mut self, kind: IdKind, name: &str) -> Id {
        let next = self.ids.len() as u64 + 1;
        *self.ids.entry(name.to_string()).or_insert_with(|| sam_id(kind, next))
    }

    fn get(&self, name: &str) -> Option<Id> {
        self.ids.get(name).copied()
    }
}

/// The lines of the sections whose title starts with `title`.
fn lines_of<'a>(sections: &'a [(String, Vec<Vec<Word>>)], title: &'a str) -> impl Iterator<Item = Line<'a>> + 'a {
    sections.iter().filter(move |(t, _)| t.starts_with(title)).flat_map(|(_, l)| l.iter()).map(|w| Line(w))
}

/// Reads the text of a file into an analysis model.
pub fn read(text: &str, file: &str) -> Result<AnalysisModel, String> {
    // the lines of each section, by its title
    let mut sections: Vec<(String, Vec<Vec<Word>>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(title) = line.strip_prefix('$') {
            sections.push((title.trim().to_ascii_uppercase(), Vec::new()));
        } else if !line.is_empty() {
            if let Some((_, lines)) = sections.last_mut() {
                lines.push(words(line));
            }
        }
    }
    let lines = |title: &'static str| lines_of(&sections, title);
    let controls: Vec<Line> = lines("CONTROLS").collect();
    let units = controls.iter().find(|l| l.at(0) == Some("UNITS")).ok_or("no units: not a model of this format")?;
    let (force, length) = (units.at(1).and_then(force_unit).ok_or("unknown unit of force")?, units.at(2).and_then(length_unit).ok_or("unknown unit of length")?);
    let application = lines("PROGRAM INFORMATION").find_map(|l| Some(format!("{} {}", l.name("PROGRAM")?, l.name("VERSION").unwrap_or_default()).trim().to_string()));
    let source = SourceInfo { format: "e2k".into(), file: file.to_string(), application, ..SourceInfo::default() };
    let title = controls.iter().filter(|l| matches!(l.at(0), Some("TITLE1" | "TITLE2"))).filter_map(|l| l.at(1)).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join(" · ");
    let mut model = AnalysisModel::new(1, if title.is_empty() { file.to_string() } else { title }, source);

    // the storeys from the top down, each with the level of its floor (metres)
    let storeys: Vec<(String, Option<f64>, Option<f64>)> = lines("STORIES").filter_map(|l| Some((l.name("STORY")?.to_string(), l.number("HEIGHT"), l.number("ELEV")))).collect();
    if storeys.is_empty() {
        return Err("no storeys".into());
    }
    let mut level = storeys.last().and_then(|s| s.2).unwrap_or(0.0) * length;
    let mut levels: Vec<(String, f64)> = Vec::new();
    for (k, (name, height, elev)) in storeys.iter().enumerate().rev() {
        if k + 1 < storeys.len() {
            level = elev.map_or(level + height.unwrap_or(0.0) * length, |e| e * length);
        }
        levels.push((name.clone(), level));
    }
    // from the lowest up
    let storey_at: HashMap<&str, usize> = levels.iter().enumerate().map(|(k, (name, _))| (name.as_str(), k)).collect();

    // points in plan, and how far below their storey they lie
    let points: HashMap<String, (f64, f64, f64)> = lines("POINT COORDINATES")
        .filter_map(|l| Some((l.name("POINT")?.to_string(), (l.0.get(2)?.number()? * length, l.0.get(3)?.number()? * length, l.0.get(4).and_then(Word::number).unwrap_or(0.0) * length))))
        .collect();
    // a node is a point on a storey
    let mut nodes: HashMap<(String, usize), Id> = HashMap::new();
    let mut node = |model: &mut AnalysisModel, point: &str, storey: i64| -> Option<Id> {
        let storey = usize::try_from(storey).ok().filter(|s| *s < levels.len())?;
        let &(x, y, below) = points.get(point)?;
        let key = (point.to_string(), storey);
        if let Some(&id) = nodes.get(&key) {
            return Some(id);
        }
        let id = sam_id(IdKind::Node, nodes.len() as u64 + 1);
        nodes.insert(key, id);
        model.nodes.push(Node { id, number: format!("{point} · {}", levels[storey].0), p: DVec3::new(x, y, levels[storey].1 - below), frame: None, support: None, origin: NodeOrigin::Analytical });
        Some(id)
    };

    // materials: a weight per volume is a density
    let mut names = Names::default();
    let mut materials: Vec<Material> = Vec::new();
    for l in lines("MATERIAL PROPERTIES") {
        let Some(name) = l.name("MATERIAL") else { continue };
        let id = names.id(IdKind::Material, &format!("material {name}"));
        if !materials.iter().any(|m| m.id == id) {
            materials.push(Material { id, name: name.to_string(), e: None, g: None, nu: None, density: None, alpha_t: None });
        }
        let m = materials.iter_mut().find(|m| m.id == id).expect("just added");
        let pressure = force / (length * length);
        m.e = l.number("E").map(|e| e * pressure).or(m.e);
        m.nu = l.number("U").or(m.nu);
        m.alpha_t = l.number("A").or(m.alpha_t);
        m.density = l.number("WEIGHTPERVOLUME").map(|w| w * force / length.powi(3) / 9.806_65).or(m.density);
    }
    model.materials = materials;

    // frame sections: their shape where the file names one that is known here
    for l in lines("FRAME SECTIONS") {
        let Some(name) = l.name("FRAMESECTION") else { continue };
        let id = names.id(IdKind::Section, &format!("section {name}"));
        if model.sections.iter().any(|s| s.id == id) {
            continue;
        }
        let size = |key: &str| l.number(key).map(|v| v * length).filter(|v| *v > 0.0);
        let (d, b, tf, tw) = (size("D"), size("B"), size("TF"), size("TW"));
        let shape = match (l.name("SHAPE").unwrap_or_default().to_ascii_lowercase().as_str(), d, b) {
            (s, Some(h), Some(b)) if s.contains("rectangular") || s == "general" => Shape::Rect { b, h },
            (s, Some(h), Some(b)) if s.contains("i/wide") || s.contains("wide flange") => match (tf, tw) {
                (Some(tf), Some(tw)) => Shape::I { h, b, tw, tf, r: 0.0, slope: 0.0 },
                _ => Shape::Rect { b, h },
            },
            (s, Some(h), Some(b)) if s.contains("box") || s.contains("tube") => match tf.or(tw) {
                Some(t) => Shape::Rhs { b, h, t, r_out: 0.0, r_in: 0.0 },
                None => Shape::Rect { b, h },
            },
            (s, Some(d), _) if s.contains("pipe") => match tw.or(size("T")) {
                Some(t) => Shape::Chs { d, t },
                None => Shape::Circle { d },
            },
            (s, Some(d), _) if s.contains("circle") => Shape::Circle { d },
            _ => Shape::Unknown,
        };
        let imported = ImportedProps {
            area: l.number("AREA").map(|v| v * length.powi(2)),
            iy: l.number("I33").map(|v| v * length.powi(4)),
            iz: l.number("I22").map(|v| v * length.powi(4)),
            it: l.number("TORSION").map(|v| v * length.powi(4)),
            ..ImportedProps::default()
        };
        let given = imported.area.is_some() || imported.iy.is_some();
        let (props, props_source) = section::compute(&shape, given.then_some(&imported));
        let material = l.name("MATERIAL").and_then(|m| names.get(&format!("material {m}")));
        model.sections.push(Section { id, name: name.to_string(), shape, props, props_source, imported: given.then_some(imported), material });
    }

    // line objects, placed on a storey by their assignment
    let bars: HashMap<String, Bar> = lines("LINE CONNECTIVITIES")
        .filter_map(|l| {
            let below = l.0.get(5).and_then(Word::number).unwrap_or(0.0) as i64;
            Some((l.name("LINE")?.to_string(), Bar { kind: l.at(2)?.to_ascii_uppercase(), points: [l.at(3)?.to_string(), l.at(4)?.to_string()], below }))
        })
        .collect();
    // a member by its line and storey, for its loads
    let mut members: HashMap<(String, usize), Id> = HashMap::new();
    for l in lines("LINE ASSIGNS") {
        let (Some(name), Some(storey)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied())) else { continue };
        let Some(bar) = bars.get(name) else { continue };
        let (Some(a), Some(b)) = (node(&mut model, &bar.points[0], storey as i64 - bar.below), node(&mut model, &bar.points[1], storey as i64)) else {
            model.diagnostics.push(Diagnostic { object: None, code: "member_without_ends".into(), message: format!("line {name} on {} has an end that is not there", levels[storey].0) });
            continue;
        };
        let id = sam_id(IdKind::Member, members.len() as u64 + 1);
        members.insert((name.to_string(), storey), id);
        let (pa, pb) = (model.nodes.iter().find(|n| n.id == a).map(|n| n.p), model.nodes.iter().find(|n| n.id == b).map(|n| n.p));
        let axis = pb.zip(pa).map_or(DVec3::X, |(pb, pa)| pb - pa);
        // end releases: a bending or twisting moment let go at the start (I) or the end (J)
        let mut releases: [Option<Hinge>; 2] = [None, None];
        for word in l.name("RELEASE").unwrap_or_default().split_whitespace() {
            let (what, end) = word.split_at(word.len().saturating_sub(1));
            let end = match end {
                "I" => 0,
                "J" => 1,
                _ => continue,
            };
            let dof = match what {
                "TOR" | "T" => 3,
                "M3" => 4,
                "M2" => 5,
                _ => continue,
            };
            let hinge = releases[end].get_or_insert_with(|| Hinge { dofs: dofs_all(Dof::Fixed), named: None });
            hinge.dofs[dof] = Dof::Free;
        }
        let section = l.name("SECTION").and_then(|s| names.get(&format!("section {s}")));
        let material = section.and_then(|s| model.sections.iter().find(|x| x.id == s)).and_then(|s| s.material);
        model.members.push(Member {
            id,
            number: if levels.len() > 2 { format!("{name} · {}", levels[storey].0) } else { name.to_string() },
            kind: MemberKind::Beam,
            nodes: vec![a, b],
            geometry: Curve::Line,
            z_ref: frame::default_z_ref(axis),
            orientation: Orientation::Default,
            section: section.map(|s| (s, None)),
            section_rotation: l.number("ANG").unwrap_or(0.0).to_radians(),
            insertion: Insertion::default(),
            releases,
            material,
            links: Vec::new(),
        });
        let _ = &bar.kind;
    }

    // floors and walls: their thickness from their property
    let mut thickness: HashMap<String, (f64, Option<Id>)> = HashMap::new();
    for title in ["SLAB PROPERTIES", "DECK PROPERTIES", "WALL PROPERTIES"] {
        for l in lines(title) {
            let Some(name) = l.name("SHELLPROP") else { continue };
            let t = ["SLABTHICKNESS", "WALLTHICKNESS", "DECKSLABDEPTH"].iter().find_map(|k| l.number(k)).map(|t| t * length);
            let entry = thickness.entry(name.to_string()).or_insert((0.0, None));
            entry.0 = t.unwrap_or(entry.0);
            entry.1 = l.name("MATERIAL").and_then(|m| names.get(&format!("material {m}"))).or(entry.1);
        }
    }
    let panels: HashMap<String, Panel> = lines("AREA CONNECTIVITIES")
        .filter_map(|l| {
            let n = l.0.get(3)?.number()? as usize;
            let corners: Vec<(String, i64)> = (0..n).filter_map(|k| Some((l.at(4 + k)?.to_string(), l.0.get(4 + n + k).and_then(Word::number).unwrap_or(0.0) as i64))).collect();
            (corners.len() == n && n >= 3).then(|| (l.at(1).unwrap_or_default().to_string(), Panel { kind: l.at(2).unwrap_or_default().to_ascii_uppercase(), corners }))
        })
        .collect();
    let mut surfaces: HashMap<(String, usize), Id> = HashMap::new();
    for l in lines("AREA ASSIGNS") {
        let (Some(name), Some(storey)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied())) else { continue };
        let Some(panel) = panels.get(name) else { continue };
        let boundary: Vec<Id> = panel.corners.iter().filter_map(|(point, below)| node(&mut model, point, storey as i64 - below)).collect();
        if boundary.len() != panel.corners.len() {
            continue;
        }
        let id = sam_id(IdKind::Surface, surfaces.len() as u64 + 1);
        surfaces.insert((name.to_string(), storey), id);
        let (t, material) = l.name("SECTION").and_then(|s| thickness.get(s)).copied().unwrap_or((0.0, None));
        model.surfaces.push(SurfaceMember {
            id,
            number: if levels.len() > 2 { format!("{name} · {}", levels[storey].0) } else { name.to_string() },
            kind: if panel.kind == "WALL" { SurfaceKind::Shell } else { SurfaceKind::Plate },
            boundary,
            openings: Vec::new(),
            thickness: if t > 0.0 { Thickness::Uniform(t) } else { Thickness::Unknown },
            offset_z: 0.0,
            x_dir: None,
            material,
            links: Vec::new(),
        });
    }

    // restraints of a point on a storey
    for l in lines("POINT ASSIGNS") {
        let (Some(point), Some(storey), Some(held)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied()), l.name("RESTRAINT")) else { continue };
        let Some(id) = node(&mut model, point, storey as i64) else { continue };
        let mut dofs = dofs_all(Dof::Free);
        for (k, key) in ["UX", "UY", "UZ", "RX", "RY", "RZ"].iter().enumerate() {
            if held.split_whitespace().any(|w| w.eq_ignore_ascii_case(key)) {
                dofs[k] = Dof::Fixed;
            }
        }
        if dofs.iter().all(|d| *d == Dof::Free) {
            continue;
        }
        let preset = support::classify(&dofs, Dimension::Space3d);
        if let Some(n) = model.nodes.iter_mut().find(|n| n.id == id) {
            n.support = Some(Support { dofs, frame: DofFrame::Global, named: None, preset });
        }
    }

    // load patterns are the load cases here; a case of the file that runs one pattern goes by
    // that pattern in a combination
    for l in lines("LOAD PATTERNS") {
        let Some(name) = l.name("LOADPATTERN") else { continue };
        let id = names.id(IdKind::LoadCase, &format!("pattern {name}"));
        let kind = l.name("TYPE").unwrap_or_default().to_string();
        let own = l.number("SELFWEIGHT").filter(|f| *f != 0.0);
        model.load_cases.push(LoadCase {
            id,
            number: (model.load_cases.len() + 1).to_string(),
            name: name.to_string(),
            action_type: match kind.to_ascii_lowercase().as_str() {
                "dead" | "super dead" | "superdead" => ActionType::Permanent,
                "live" | "roof live" | "reducible live" | "snow" | "wind" | "temperature" => ActionType::Variable,
                "seismic" | "quake" => ActionType::Accidental,
                _ => ActionType::User(kind.clone()),
            },
            action_source: kind,
            self_weight: own.map(|f| DVec3::new(0.0, 0.0, -f)),
            coefficient: 1.0,
        });
    }
    let mut runs: HashMap<String, Vec<String>> = HashMap::new();
    for l in lines("LOAD CASES") {
        if let (Some(case), Some(pattern)) = (l.name("LOADCASE"), l.name("LOADPAT")) {
            runs.entry(case.to_string()).or_default().push(pattern.to_string());
        }
    }
    let case_of = |names: &Names, name: &str| -> Option<Id> {
        let pattern = match runs.get(name).map(Vec::as_slice) {
            Some([one]) => one.as_str(),
            _ => name,
        };
        names.get(&format!("pattern {pattern}"))
    };

    // loads: which way they act, and on what
    let direction = |l: &Line| -> (DVec3, LoadFrame) {
        match l.name("DIR").unwrap_or("GRAV").to_ascii_uppercase().as_str() {
            "X" => (DVec3::X, LoadFrame::Global),
            "Y" => (DVec3::Y, LoadFrame::Global),
            "Z" => (DVec3::Z, LoadFrame::Global),
            "1" => (DVec3::X, LoadFrame::Local),
            "2" => (DVec3::Y, LoadFrame::Local),
            "3" => (DVec3::Z, LoadFrame::Local),
            _ => (DVec3::NEG_Z, LoadFrame::Global),
        }
    };
    let mut skipped: HashMap<String, usize> = HashMap::new();
    let act = |model: &mut AnalysisModel, case: Id, target: Target, frame: LoadFrame, kind: Load| {
        let id = sam_id(IdKind::Action, model.actions.len() as u64 + 1);
        model.actions.push(Action { id, load: LoadRef::Case(case), target, frame, kind, destabilising: false });
    };
    for l in lines("POINT OBJECT LOADS") {
        let (Some(point), Some(storey), Some(case)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied()), l.name("LC").and_then(|c| case_of(&names, c))) else { continue };
        let Some(at) = node(&mut model, point, storey as i64) else { continue };
        let v = |a: &str, b: &str, c: &str, unit: f64| DVec3::new(l.number(a).unwrap_or(0.0), l.number(b).unwrap_or(0.0), l.number(c).unwrap_or(0.0)) * unit;
        act(&mut model, case, Target::Node(at), LoadFrame::Global, Load::NodalForce { f: v("FX", "FY", "FZ", force), m: v("MX", "MY", "MZ", force * length) });
    }
    for title in ["FRAME OBJECT LOADS", "LINE OBJECT LOADS"] {
        for l in lines(title) {
            let (Some(name), Some(storey), Some(case)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied()), l.name("LC").and_then(|c| case_of(&names, c))) else { continue };
            let Some(&member) = members.get(&(name.to_string(), storey)) else { continue };
            let (dir, frame) = direction(&l);
            let kind = l.name("TYPE").unwrap_or_default().to_ascii_uppercase();
            let load = match (kind.as_str(), l.number("FVAL")) {
                ("UNIFF", Some(q)) => Some(Load::MemberDistributed { dir, moment: false, dist: Distribution::Uniform(q * force / length) }),
                ("UNIFM", Some(q)) => Some(Load::MemberDistributed { dir, moment: true, dist: Distribution::Uniform(q * force) }),
                ("POINTF", Some(f)) => Some(Load::MemberPoint { x: l.number("RDIST").unwrap_or(0.5), f: dir * f * force, m: DVec3::ZERO }),
                ("TRAPF", _) => match (l.number("RDISTA"), l.number("RDISTB"), l.number("FVALA"), l.number("FVALB")) {
                    (Some(a), Some(b), Some(q1), Some(q2)) => Some(Load::MemberDistributed { dir, moment: false, dist: Distribution::Trapezoidal { a, b, q1: q1 * force / length, q2: q2 * force / length } }),
                    _ => None,
                },
                _ => None,
            };
            match load {
                Some(load) => act(&mut model, case, Target::Member { id: member, span: None }, frame, load),
                None => *skipped.entry(format!("line load {kind}")).or_default() += 1,
            }
        }
    }
    for title in ["SHELL OBJECT LOADS", "AREA OBJECT LOADS"] {
        for l in lines(title) {
            let (Some(name), Some(storey), Some(case)) = (l.at(1), l.at(2).and_then(|s| storey_at.get(s).copied()), l.name("LC").and_then(|c| case_of(&names, c))) else { continue };
            let Some(&surface) = surfaces.get(&(name.to_string(), storey)) else { continue };
            let (dir, frame) = direction(&l);
            let kind = l.name("TYPE").unwrap_or_default().to_ascii_uppercase();
            match (kind.as_str(), l.number("FVAL")) {
                ("UNIFF", Some(q)) => act(&mut model, case, Target::Surface(surface), frame, Load::Surface { dir, dist: SurfaceDistribution::Uniform(q * force / (length * length)) }),
                _ => *skipped.entry(format!("area load {kind}")).or_default() += 1,
            }
        }
    }
    for (what, n) in skipped {
        model.diagnostics.push(Diagnostic { object: None, code: "load_not_read".into(), message: format!("{n} × {what}: this kind of load is not read") });
    }

    // combinations: their cases and other combinations, each with its factor
    for l in lines("LOAD COMBINATIONS") {
        let Some(name) = l.name("COMBO") else { continue };
        let id = names.id(IdKind::Combination, &format!("combination {name}"));
        if !model.combinations.iter().any(|c| c.id == id) {
            let number = (model.combinations.len() + 1).to_string();
            model.combinations.push(Combination { id, number, name: name.to_string(), purpose: Purpose::User(String::new()), terms: Vec::new() });
        }
        let term = l.name("LOADCASE").and_then(|c| case_of(&names, c)).map(LoadRef::Case).or_else(|| l.name("LOADCOMBO").and_then(|c| names.get(&format!("combination {c}"))).map(LoadRef::Combination));
        let combination = model.combinations.iter_mut().find(|c| c.id == id).expect("just added");
        if let Some(kind) = l.name("TYPE") {
            combination.purpose = Purpose::User(kind.to_string());
        }
        if let Some(term) = term {
            combination.terms.push((term, l.number("SF").unwrap_or(1.0)));
        }
    }

    if model.nodes.is_empty() {
        return Err("no members, floors or walls".into());
    }
    // the plan size of a section in the model's notes would be noise; the index is what the
    // viewer looks things up by
    let duplicates = model.rebuild_index();
    debug_assert!(duplicates.is_empty(), "{duplicates:?}");
    let _ = DVec2::ZERO;
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: &str = r#"$ File saved frame.e2k
$ PROGRAM INFORMATION
  PROGRAM  "Analysis"  VERSION "1.0"
$ CONTROLS
  UNITS  "KN"  "MM"  "C"
  TITLE1  "Portal"
$ STORIES - IN SEQUENCE FROM TOP
  STORY "Roof"  HEIGHT 3000
  STORY "Floor"  HEIGHT 4000
  STORY "Base"  ELEV -500
$ MATERIAL PROPERTIES
  MATERIAL  "S355"  TYPE "Steel"  WEIGHTPERVOLUME 7.7E-08
  MATERIAL  "S355"  SYMTYPE "Isotropic"  E 210  U 0.3  A 1.2E-05
$ FRAME SECTIONS
  FRAMESECTION  "Col"  MATERIAL "S355"  SHAPE "Concrete Rectangular"  D 300 B 200
$ POINT COORDINATES
  POINT "1"  0 0
  POINT "2"  6000 0
$ LINE CONNECTIVITIES
  LINE  "C1"  COLUMN  "1"  "1"  1
  LINE  "C2"  COLUMN  "2"  "2"  1
  LINE  "B1"  BEAM  "1"  "2"  0
$ POINT ASSIGNS
  POINTASSIGN  "1"  "Base"  RESTRAINT "UX UY UZ RX RY RZ"
  POINTASSIGN  "2"  "Base"  RESTRAINT "UX UY UZ"
$ LINE ASSIGNS
  LINEASSIGN  "C1"  "Floor"  SECTION "Col"
  LINEASSIGN  "C2"  "Floor"  SECTION "Col"
  LINEASSIGN  "B1"  "Floor"  SECTION "Col"  RELEASE "M3I M3J"
  LINEASSIGN  "C1"  "Roof"  SECTION "Col"
$ LOAD PATTERNS
  LOADPATTERN "Dead"  TYPE  "Dead"  SELFWEIGHT  1
  LOADPATTERN "Live"  TYPE  "Live"  SELFWEIGHT  0
$ POINT OBJECT LOADS
  POINTLOAD  "2"  "Floor"  TYPE "FORCE"  LC "Live"  FX 10  FY 0  FZ -5
$ FRAME OBJECT LOADS
  LINELOAD  "B1"  "Floor"  TYPE "UNIFF"  DIR "GRAV"  LC "Live"  FVAL 0.004
  LINELOAD  "B1"  "Floor"  TYPE "TEMP"  LC "Live"  T 10
$ LOAD CASES
  LOADCASE "Dead"  TYPE  "Linear Static"
  LOADCASE "Dead"  LOADPAT  "Dead"  SF 1
  LOADCASE "Live"  LOADPAT  "Live"  SF 1
$ LOAD COMBINATIONS
  COMBO "ULS"  TYPE "Linear Add"
  COMBO "ULS"  LOADCASE  "Dead"  SF 1.2
  COMBO "ULS"  LOADCASE  "Live"  SF 1.5
  COMBO "Env"  LOADCOMBO  "ULS"  SF 1
$ END OF MODEL FILE
"#;

    #[test]
    fn a_frame_is_read_in_metres_and_newtons() {
        let m = read(FRAME, "frame.e2k").unwrap();
        assert_eq!((m.name.as_str(), m.source.application.as_deref()), ("Portal", Some("Analysis 1.0")));
        // the storeys stand on the base at -0.5 m: the floor at 3.5 m, the roof at 6.5 m
        let z = |number: &str| m.nodes.iter().find(|n| n.number == number).map(|n| (n.p.z * 1e6).round() / 1e6);
        assert_eq!((z("1 · Base"), z("1 · Floor"), z("1 · Roof")), (Some(-0.5), Some(3.5), Some(6.5)));
        assert_eq!((m.nodes.len(), m.members.len()), (5, 4));
        // a column runs from the storey below up, a beam lies on its storey
        let ends = |number: &str| m.members.iter().find(|x| x.number == number).and_then(|x| m.member_ends(x)).map(|(a, b)| (a.z, b.z, b.x - a.x));
        assert_eq!(ends("C1 · Floor"), Some((-0.5, 3.5, 0.0)));
        assert_eq!(ends("B1 · Floor"), Some((3.5, 3.5, 6.0)));
        // sizes, stiffness and density in SI units
        let section = &m.sections[0];
        assert_eq!(section.shape, Shape::Rect { b: 0.2, h: 0.3 });
        let steel = &m.materials[0];
        assert!((steel.e.unwrap() - 210e9).abs() < 1.0 && (steel.density.unwrap() - 7851.8).abs() < 1.0, "{steel:?}");
        // a point held in every direction is fixed, one held in its moves is pinned
        let support = |number: &str| m.nodes.iter().find(|n| n.number == number).and_then(|n| n.support.as_ref()).map(|s| s.preset);
        assert_eq!((support("1 · Base"), support("2 · Base")), (Some(SupportPreset::Fixed), Some(SupportPreset::Pinned)));
        // the beam's ends let their bending go
        let beam = m.members.iter().find(|x| x.number == "B1 · Floor").unwrap();
        assert!(beam.releases.iter().all(|h| h.as_ref().is_some_and(|h| h.dofs[4] == Dof::Free && h.dofs[3] == Dof::Fixed)));
    }

    #[test]
    fn loads_cases_and_combinations_are_read() {
        let m = read(FRAME, "frame.e2k").unwrap();
        assert_eq!(m.load_cases.iter().map(|c| (c.name.as_str(), c.self_weight.is_some())).collect::<Vec<_>>(), [("Dead", true), ("Live", false)]);
        assert_eq!(m.actions.len(), 2);
        assert!(matches!(&m.actions[0].kind, Load::NodalForce { f, .. } if *f == DVec3::new(10_000.0, 0.0, -5_000.0)));
        // 0.004 kN/mm is 4 kN/m, downwards
        assert!(matches!(&m.actions[1].kind, Load::MemberDistributed { dir, dist: Distribution::Uniform(q), .. } if *dir == DVec3::NEG_Z && (q - 4_000.0).abs() < 1e-6));
        assert!(m.diagnostics.iter().any(|d| d.code == "load_not_read" && d.message.contains("TEMP")), "{:?}", m.diagnostics);
        let uls = &m.combinations[0];
        assert_eq!((uls.name.as_str(), uls.terms.iter().map(|t| t.1).collect::<Vec<_>>()), ("ULS", vec![1.2, 1.5]));
        assert!(matches!(m.combinations[1].terms[..], [(LoadRef::Combination(id), 1.0)] if id == uls.id));
    }

    #[test]
    fn a_file_without_units_or_storeys_is_refused() {
        assert!(read("$ CONTROLS\n  TITLE1 \"x\"\n", "x.e2k").is_err());
        assert!(read("$ CONTROLS\n  UNITS \"KN\" \"M\" \"C\"\n", "x.e2k").is_err());
    }
}
