//! IFC 4 (ISO 16739-1:2018, ADD2 TC1) or IFC 4x3 (ISO 16739-1:2024, IFC4X3_ADD2) as a STEP
//! physical file (ISO 10303-21), for every model the viewer opens. Written by the viewer itself,
//! without an IFC library.
//!
//! - The two schemas differ in what this writer writes in three places only: IFC 4x3 point lists
//!   have a `TagList` (written empty), an IfcAnnotation has a `PredefinedType`, and the header
//!   names the schema (`IFC4X3_ADD2`) and its Reference View.
//!
//! - Spatial structure: project, site, building and one storey per level of the model (Revit),
//!   else one storey; each element is contained in its level, or the storey it stands on.
//! - One IFC element per element of the model (Revit elements, Tekla parts, SketchUp components,
//!   glTF nodes) and one per layer for geometry outside elements (OBJ objects, STL, PLY). The
//!   class follows the Revit category, the Tekla part name or the Archicad or Vectorworks element (IfcWall,
//!   IfcSlab, IfcBeam, ...); anything else is an IfcBuildingElementProxy.
//! - Archicad openings (already cut from their walls) are left out; Archicad library objects,
//!   drawn as boxes of any kind, are IfcBuildingElementProxy; Archicad meshes are
//!   IfcGeographicElement (terrain), and zones IfcSpace, part of their storey (IfcRelAggregates).
//! - Body: one IfcTriangulatedFaceSet per colour, relative to the element's placement, welded,
//!   without degenerate triangles, `Closed` when every edge joins two triangles, styled with one
//!   shared IfcSurfaceStyle per colour.
//! - Curves (Rhino curves, hatch lines, dimensions and leaders): an IfcIndexedPolyCurve per
//!   polyline in an 'Annotation' representation, styled with an IfcCurveStyle. An element of
//!   curves only, and Rhino's texts, hatches, dimensions and leaders, are IfcAnnotation.
//! - The element's properties as one property set named after the source format, its materials
//!   as IfcMaterial (one) or IfcMaterialConstituentSet (several), the model's layers as
//!   IfcPresentationLayerAssignment.
//! - A modelled element has the class of its definition (IfcWall, IfcSlab, IfcBeam, ...). A wall
//!   or slab with a build-up has its layers as an IfcMaterialLayerSet (each layer its material,
//!   its thickness, and `IsVentilated` for a cavity), named after its type, through an
//!   IfcMaterialLayerSetUsage: a wall's along its axis, centred on it; a slab's from its top down.
//! - GlobalIds are derived from the file name and the element, so saving again gives the same ids.
//! - Units: metres. OBJ, STL and PLY carry no unit; such a model larger than 500 units is taken to
//!   be in millimetres.

use crate::document::Document;
use crate::model::Model;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

/// The IFC schema a `.ifc` is written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Schema {
    /// IFC 4 ADD2 TC1, what a `.ifc` is unless IFC 4x3 is asked for.
    #[default]
    Ifc4,
    /// IFC 4x3 ADD2, the current ISO 16739-1.
    Ifc4x3,
}

impl Schema {
    /// Its name in the file header.
    fn step(self) -> &'static str {
        match self {
            Schema::Ifc4 => "IFC4",
            Schema::Ifc4x3 => "IFC4X3_ADD2",
        }
    }

    /// Its name for people: "IFC 4", "IFC 4x3".
    pub fn label(self) -> &'static str {
        match self {
            Schema::Ifc4 => "IFC 4",
            Schema::Ifc4x3 => "IFC 4x3",
        }
    }

    /// The schema a name gives: IFC4, IFC4X3, IFC4X3_ADD2, 4x3 (any case, with or without a space).
    pub fn parse(name: &str) -> Option<Schema> {
        let n: String = name.chars().filter(|c| !c.is_whitespace() && *c != '.').collect::<String>().to_uppercase();
        match n.trim_start_matches("IFC") {
            "4" | "4ADD2" | "4ADD2TC1" => Some(Schema::Ifc4),
            "4X3" | "4X3ADD2" | "4X3_ADD2" => Some(Schema::Ifc4x3),
            _ => None,
        }
    }
}

/// IFC 4 for a `.ifc`.
pub struct IfcWriter;

/// IFC 4x3 for a `.ifc`, when that is asked for (File > Export IFC 4x3, `save_file` with a schema).
pub struct Ifc4x3Writer;

impl super::Writer for IfcWriter {
    fn name(&self) -> &'static str {
        "IFC 4"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifc"]
    }

    fn mime(&self) -> &'static str {
        "application/x-step"
    }

    fn write(&self, model: &Model, document: &Document, file_name: &str, mut out: &mut dyn Write) -> io::Result<()> {
        write(model, document, file_name, &mut out)
    }
}

impl super::Writer for Ifc4x3Writer {
    fn name(&self) -> &'static str {
        "IFC 4x3"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifc"]
    }

    fn mime(&self) -> &'static str {
        "application/x-step"
    }

    fn write(&self, model: &Model, document: &Document, file_name: &str, mut out: &mut dyn Write) -> io::Result<()> {
        write_schema(model, document, file_name, Schema::Ifc4x3, &mut out)
    }
}

/// Length unit and coordinate resolution of the file.
#[derive(Clone, Copy)]
struct Units {
    millimetres: bool,
    /// Decimals written for coordinates: 0.01 mm, finer for a small model (a millionth of its
    /// size), so that its thin parts keep their triangles.
    decimals: u32,
    /// One millimetre in quantised coordinates.
    mm: i64,
}

impl Units {
    fn of(model: &Model) -> Self {
        let unitless = matches!(model.format.as_str(), "OBJ" | "STL" | "PLY");
        if unitless && model.bounds.is_valid() && model.bounds.size().iter().any(|&s| s > 500.0) {
            return Units { millimetres: true, decimals: 2, mm: 100 };
        }
        // metres: to 0.01 mm, or a millionth of the model's size when that is smaller
        let size = if model.bounds.is_valid() { model.bounds.size().iter().fold(0.0f64, |m, &s| m.max(s as f64)) } else { 0.0 };
        let decimals = if size > 0.0 && size.is_finite() { ((-(size * 1e-6).log10()).ceil() as i64).clamp(5, 9) as u32 } else { 5 };
        Units { millimetres: false, decimals, mm: 10i64.pow(decimals - 3) }
    }

    /// Quantised coordinate of a model value.
    fn quantise(&self, v: f64) -> i64 {
        (v * 10f64.powi(self.decimals as i32)).round() as i64
    }
}

/// What an IFC element is made from: an element of the model, or a layer's geometry outside
/// elements.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Key {
    Element(u64),
    Layer(String),
}

/// An element to write: its key and where it is drawn (layer, first index, index count).
struct Product {
    key: Key,
    ranges: Vec<(usize, u32, u32)>,
    /// Where its curves are drawn (layer, first index into `lines`, index count).
    lines: Vec<(usize, u32, u32)>,
    /// The layer it is drawn in first, for its presentation layer.
    layer: String,
}

/// A layer's name without the " (part n/m)" of a split layer.
fn base_layer_name(name: &str) -> &str {
    match name.rfind(" (part ") {
        Some(i) if name.ends_with(')') => &name[..i],
        _ => name,
    }
}

/// The elements of a model in drawing order, plus one per layer for its triangles outside
/// elements.
fn products(model: &Model) -> Vec<Product> {
    let mut out: Vec<Product> = Vec::new();
    let mut at: HashMap<Key, usize> = HashMap::new();
    let mut add = |key: Key, range: (usize, u32, u32), layer: &str, curve: bool| {
        if range.2 < if curve { 2 } else { 3 } {
            return;
        }
        let i = *at.entry(key.clone()).or_insert_with(|| {
            out.push(Product { key, ranges: Vec::new(), lines: Vec::new(), layer: layer.to_string() });
            out.len() - 1
        });
        if curve {
            out[i].lines.push(range);
        } else {
            out[i].ranges.push(range);
        }
    };

    for (li, layer) in model.layers.iter().enumerate() {
        // Archicad openings are already cut from their walls. An IfcOpeningElement would need
        // IfcRelVoidsElement and no storey, which this writer does not write: left out.
        if model.format == "Archicad" && layer.name == "Openings" {
            continue;
        }
        let name = base_layer_name(&layer.name);
        let loose = Key::Layer(name.to_string());
        let mut covered = 0u32;
        for &(id, first, count) in &layer.elements {
            if first > covered {
                add(loose.clone(), (li, covered, first - covered), name, false);
            }
            add(Key::Element(id), (li, first, count), name, false);
            covered = covered.max(first + count);
        }
        let len = layer.indices.len() as u32;
        if len > covered {
            add(loose, (li, covered, len - covered), name, false);
        }
        for &(id, first, count) in &layer.line_elements {
            add(Key::Element(id), (li, first, count), name, true);
        }
    }
    out
}

/// One IfcTriangulatedFaceSet under construction: welded points (quantised, relative to the
/// element's placement) and triangles.
#[derive(Default)]
struct FaceSet {
    weld: HashMap<[i64; 3], u32>,
    points: Vec<[i64; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl FaceSet {
    fn point(&mut self, p: [i64; 3]) -> u32 {
        *self.weld.entry(p).or_insert_with(|| {
            self.points.push(p);
            self.points.len() as u32 - 1
        })
    }

    /// Adds a triangle unless it collapsed: two corners on one point, or all three on a line.
    fn triangle(&mut self, t: [[i64; 3]; 3]) {
        let e1 = [0, 1, 2].map(|k| (t[1][k] - t[0][k]) as i128);
        let e2 = [0, 1, 2].map(|k| (t[2][k] - t[0][k]) as i128);
        let cross = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
        if cross == [0, 0, 0] {
            return;
        }
        let t = t.map(|p| self.point(p));
        self.triangles.push(t);
    }

    /// Every edge joins exactly two triangles: the set bounds a volume.
    fn closed(&self) -> bool {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::with_capacity(self.triangles.len() * 2);
        for t in &self.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        !edges.is_empty() && edges.values().all(|&n| n == 2)
    }
}

/// An element's body: its placement (quantised, world) and one face set per colour (sRGB8 and
/// alpha).
struct Body {
    origin: [i64; 3],
    sets: Vec<([u8; 4], FaceSet)>,
    /// Polylines (quantised, relative to the placement) and their colours.
    curves: Vec<([u8; 4], Vec<[i64; 3]>)>,
    min_z: f64,
}

/// `paint`: one colour for all of it, in place of the model's.
fn body(model: &Model, product: &Product, units: Units, paint: Option<[u8; 4]>) -> Option<Body> {
    // the triangles, quantised (world), with their colour
    let mut tris: Vec<([[i64; 3]; 3], [u8; 4])> = Vec::new();
    for &(li, first, count) in &product.ranges {
        let layer = &model.layers[li];
        let base = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let Some(indices) = layer.indices.get(first as usize..(first + count) as usize) else { continue };
        for t in indices.chunks_exact(3) {
            let Some(corners) = t
                .iter()
                .map(|&i| {
                    let p = layer.positions.get(3 * i as usize..3 * i as usize + 3)?;
                    p.iter().all(|v| v.is_finite()).then(|| [0, 1, 2].map(|k| units.quantise(p[k] as f64)))
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let colour = match (paint, &layer.colors) {
                (Some(p), _) => p,
                (None, Some(c)) => c.get(t[0] as usize).copied().unwrap_or([base[0], base[1], base[2], 255]),
                (None, None) => [base[0], base[1], base[2], 255],
            };
            tris.push(([corners[0], corners[1], corners[2]], colour));
        }
    }
    // the curves: segments joined into polylines where one ends at the next one's start
    let mut curves: Vec<([u8; 4], Vec<[i64; 3]>)> = Vec::new();
    for &(li, first, count) in &product.lines {
        let layer = &model.layers[li];
        let base = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let Some(segments) = layer.lines.get(first as usize..(first + count) as usize) else { continue };
        let point = |i: u32| {
            let p = layer.positions.get(3 * i as usize..3 * i as usize + 3)?;
            p.iter().all(|v| v.is_finite()).then(|| [0, 1, 2].map(|k| units.quantise(p[k] as f64)))
        };
        let mut last: Option<u32> = None;
        for s in segments.chunks_exact(2) {
            let (Some(a), Some(b)) = (point(s[0]), point(s[1])) else {
                last = None;
                continue;
            };
            if last != Some(s[0]) || curves.is_empty() {
                let colour = match &layer.colors {
                    Some(c) => c.get(s[0] as usize).copied().unwrap_or([base[0], base[1], base[2], 255]),
                    None => [base[0], base[1], base[2], 255],
                };
                curves.push((colour, vec![a]));
            }
            curves.last_mut().unwrap().1.push(b);
            last = Some(s[1]);
        }
    }
    let mut min = [i64::MAX; 3];
    for p in tris.iter().flat_map(|(t, _)| t).chain(curves.iter().flat_map(|(_, c)| c)) {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
        }
    }
    // the placement: the lower corner of the element, on whole millimetres
    let origin = min.map(|v| v.div_euclid(units.mm) * units.mm);
    let mut sets: Vec<([u8; 4], FaceSet)> = Vec::new();
    let mut by_colour: HashMap<[u8; 4], usize> = HashMap::new();
    for (t, colour) in tris {
        let i = *by_colour.entry(colour).or_insert_with(|| {
            sets.push((colour, FaceSet::default()));
            sets.len() - 1
        });
        sets[i].1.triangle(t.map(|p| [0, 1, 2].map(|k| p[k] - origin[k])));
    }
    sets.retain(|(_, s)| !s.triangles.is_empty());
    for (_, c) in &mut curves {
        for p in c.iter_mut() {
            *p = [0, 1, 2].map(|k| p[k] - origin[k]);
        }
        c.dedup();
    }
    curves.retain(|(_, c)| c.len() >= 2);
    let min_z = min[2] as f64 / 10f64.powi(units.decimals as i32);
    (!sets.is_empty() || !curves.is_empty()).then_some(Body { origin, sets, curves, min_z })
}

/// The IFC class and predefined type for an element of a Revit category (the viewer's batch
/// names).
fn revit_class(category: &str) -> Option<(&'static str, &'static str)> {
    Some(match category {
        "Walls" => ("IFCWALL", "NOTDEFINED"),
        "Floors" => ("IFCSLAB", "FLOOR"),
        "Pads" => ("IFCSLAB", "BASESLAB"),
        "Ceilings" => ("IFCCOVERING", "CEILING"),
        "Roofs" => ("IFCROOF", "NOTDEFINED"),
        "Stairs" => ("IFCSTAIR", "NOTDEFINED"),
        "Ramps" => ("IFCRAMP", "NOTDEFINED"),
        "Railings" => ("IFCRAILING", "NOTDEFINED"),
        "Doors" => ("IFCDOOR", "DOOR"),
        "Windows" => ("IFCWINDOW", "WINDOW"),
        "Columns" => ("IFCCOLUMN", "NOTDEFINED"),
        "Structural framing" => ("IFCBEAM", "NOTDEFINED"),
        "Structural foundations" => ("IFCFOOTING", "NOTDEFINED"),
        "Curtain panels" => ("IFCPLATE", "CURTAIN_PANEL"),
        "Curtain wall mullions" => ("IFCMEMBER", "MULLION"),
        "Furniture and casework" => ("IFCFURNITURE", "NOTDEFINED"),
        "Plumbing fixtures" => ("IFCSANITARYTERMINAL", "NOTDEFINED"),
        "Lighting fixtures" => ("IFCLIGHTFIXTURE", "NOTDEFINED"),
        "Ducts" => ("IFCDUCTSEGMENT", "NOTDEFINED"),
        "Pipes" => ("IFCPIPESEGMENT", "NOTDEFINED"),
        "Pipe fittings" => ("IFCPIPEFITTING", "NOTDEFINED"),
        "Cable trays and conduits" => ("IFCCABLECARRIERSEGMENT", "NOTDEFINED"),
        "Rebar" => ("IFCREINFORCINGBAR", "NOTDEFINED"),
        "Topography" => ("IFCGEOGRAPHICELEMENT", "TERRAIN"),
        "Planting" => ("IFCGEOGRAPHICELEMENT", "NOTDEFINED"),
        _ => return None,
    })
}

/// How a straight part runs: level, plumb, or inclined (neither).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    Level,
    Plumb,
    Inclined,
}

/// How a body runs: the direction most of its edge length lies along. A profile's side faces each
/// add two edges along its length, and only one (longer) diagonal each, in a direction of its
/// own. `None` without edges.
fn run(body: &Body) -> Option<Run> {
    let mut edges: Vec<[f64; 3]> = Vec::new();
    for (_, set) in &body.sets {
        for t in &set.triangles {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (a, b) = (set.points[a as usize], set.points[b as usize]);
                edges.push([0, 1, 2].map(|k| (b[k] - a[k]) as f64));
            }
        }
    }
    let length = |e: &[f64; 3]| (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    // edge length by direction (to a thousandth, either way along), and the longest edge of each
    let mut by_direction: HashMap<[i32; 3], (f64, usize)> = HashMap::new();
    for (i, e) in edges.iter().enumerate() {
        let l = length(e);
        if l == 0.0 {
            continue;
        }
        let first = e.iter().copied().find(|c| *c != 0.0).unwrap_or(1.0);
        let d = e.map(|c| (c / l * first.signum() * 1000.0).round() as i32);
        // its longest edge gives a direction most exactly
        let entry = by_direction.entry(d).or_insert((0.0, i));
        entry.0 += l;
        if l > length(&edges[entry.1]) {
            entry.1 = i;
        }
    }
    let mut candidates: Vec<(f64, usize)> = by_direction.into_values().collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    // edge length along a direction: of the edges within about 0.06° of it (so also those just
    // across a thousandth)
    let along = |d: &[f64; 3]| {
        let l = length(d);
        edges
            .iter()
            .filter(|e| {
                let c = [e[1] * d[2] - e[2] * d[1], e[2] * d[0] - e[0] * d[2], e[0] * d[1] - e[1] * d[0]];
                length(&c) <= 1e-3 * l * length(e)
            })
            .map(length)
            .sum::<f64>()
    };
    let axis = candidates.iter().take(4).map(|&(_, i)| &edges[i]).max_by(|a, b| along(a).total_cmp(&along(b)))?;
    let rise = axis[2].abs() / length(axis);
    Some(if rise < 1e-3 {
        Run::Level
    } else if rise > 1.0 - 1e-6 {
        Run::Plumb
    } else {
        Run::Inclined
    })
}

/// The IFC class and predefined type of a Tekla object, as Tekla's own IFC export gives it: by
/// what the file says it is (its `Type`: column, beam, polybeam, contour plate, slab, pad or strip
/// footing, weld, bolt group, rebar), else by its part type, else by its name (COLUMN, BEAM, ...
/// and their Dutch words) and profile. A beam that runs neither level nor plumb is a member (as
/// Tekla exports an inclined beam); a beam of a plate's section stays a beam (Tekla's "flat wide
/// beams as plates" off). Fasteners have no predefined type (`""`, written `$`).
fn tekla_class(name: &str, kind: &str, part_type: &str, profile: &str, run: Option<Run>) -> (&'static str, &'static str) {
    let beam = if run == Some(Run::Inclined) { ("IFCMEMBER", "NOTDEFINED") } else { ("IFCBEAM", "NOTDEFINED") };
    match kind {
        "Column" => return ("IFCCOLUMN", "NOTDEFINED"),
        "Beam" | "Polybeam" => return beam,
        "Contour plate" => return ("IFCPLATE", "NOTDEFINED"),
        "Slab" => return ("IFCSLAB", "NOTDEFINED"),
        "Pad footing" => return ("IFCFOOTING", "PAD_FOOTING"),
        "Strip footing" => return ("IFCFOOTING", "STRIP_FOOTING"),
        "Weld" => return ("IFCFASTENER", ""),
        "Bolt group" => return ("IFCMECHANICALFASTENER", ""),
        "Rebar group" | "Single rebar" => return ("IFCREINFORCINGBAR", "NOTDEFINED"),
        _ => {}
    }
    match part_type.to_uppercase().as_str() {
        "COLUMN" => return ("IFCCOLUMN", "NOTDEFINED"),
        "BEAM" | "POLYBEAM" => return beam,
        "PLATE" | "CONTOUR_PLATE" => return ("IFCPLATE", "NOTDEFINED"),
        "SLAB" => return ("IFCSLAB", "NOTDEFINED"),
        "PAD_FOOTING" => return ("IFCFOOTING", "PAD_FOOTING"),
        "STRIP_FOOTING" => return ("IFCFOOTING", "STRIP_FOOTING"),
        _ => {}
    }
    // a profile part the file says nothing more of: a beam (Tekla's profile parts are beams)
    if kind == "Profile part" && run.is_some() {
        return beam;
    }
    let name = name.to_uppercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["COLUMN", "KOLOM"]) {
        ("IFCCOLUMN", "COLUMN")
    } else if has(&["PLATE", "PLAAT"]) || profile.to_uppercase().starts_with("PL") {
        ("IFCPLATE", "NOTDEFINED")
    } else if has(&["BRAC", "VERBAND", "TREK", "STAAF", "ROD", "TIE", "STRUT", "SCHOOR"]) {
        ("IFCMEMBER", "BRACE")
    } else if has(&["BEAM", "RAFTER", "PURLIN", "GIRDER", "JOIST", "LIGGER", "SPANT", "GORDING"]) {
        ("IFCBEAM", "BEAM")
    } else {
        ("IFCMEMBER", "NOTDEFINED")
    }
}

/// Tekla objects Tekla's IFC export leaves out: control points, lines and planes, grid planes
/// (the grids are IfcGrid) and loads.
fn tekla_left_out(kind: &str) -> bool {
    matches!(kind, "Control point" | "Control line" | "Control plane" | "Grid plane") || kind.ends_with(" load") || kind == "Load"
}

/// The colour Tekla's IFC export gives a weld or a bolt group, which the model's object
/// representation does not colour: welds amber, bolts black.
fn tekla_colour(kind: &str) -> Option<[u8; 4]> {
    match kind {
        "Weld" => Some([244, 177, 0, 255]),
        "Bolt group" => Some([0, 0, 0, 255]),
        _ => None,
    }
}

/// The material of a Tekla object as Tekla's IFC export names it: a part's material, a rebar's
/// grade, a bolt group's standard; and its category: the material type (a rebar's steel), else
/// the material's own name.
fn tekla_material(props: &[(String, String)]) -> Option<(String, String)> {
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.trim()).filter(|v| !v.is_empty());
    let (name, category) = match get("Type") {
        Some("Bolt group") => (get("Standard")?, Some("STEEL")),
        Some("Rebar group" | "Single rebar") => (get("Grade").or(get("Material"))?, Some("STEEL")),
        _ => (get("Material")?, None),
    };
    let category = category.or(match get("Material type") {
        Some(t @ ("STEEL" | "CONCRETE" | "TIMBER")) => Some(t),
        Some("REBAR" | "REBAR_MESH") => Some("STEEL"),
        _ => None,
    });
    Some((name.to_string(), category.unwrap_or(name).to_string()))
}

/// The IFC class and predefined type of a Vectorworks object: the entity Vectorworks itself
/// keeps for it in its IFC data (`IfcDoor`, `IfcWallStandardCase`, a type entity for the things
/// of that type), or else by the kind of object the reader names.
fn vectorworks_class(entity: &str, category: &str) -> (&'static str, &'static str) {
    match entity.strip_suffix("Type").unwrap_or(entity) {
        "IfcWall" | "IfcWallStandardCase" => ("IFCWALL", "NOTDEFINED"),
        "IfcSlab" => ("IFCSLAB", "FLOOR"),
        // a roof plane is a slab of the roof in IFC 4
        "IfcRoof" => ("IFCSLAB", "ROOF"),
        "IfcColumn" => ("IFCCOLUMN", "COLUMN"),
        "IfcBeam" => ("IFCBEAM", "BEAM"),
        "IfcMember" => ("IFCMEMBER", "NOTDEFINED"),
        "IfcDoor" => ("IFCDOOR", "DOOR"),
        "IfcWindow" => ("IFCWINDOW", "WINDOW"),
        "IfcStair" => ("IFCSTAIR", "NOTDEFINED"),
        "IfcRamp" => ("IFCRAMP", "NOTDEFINED"),
        "IfcRailing" => ("IFCRAILING", "NOTDEFINED"),
        "IfcSpace" => ("IFCSPACE", "SPACE"),
        "IfcCovering" => ("IFCCOVERING", "CEILING"),
        "IfcCurtainWall" => ("IFCCURTAINWALL", "NOTDEFINED"),
        "IfcFooting" => ("IFCFOOTING", "NOTDEFINED"),
        "IfcPlate" => ("IFCPLATE", "NOTDEFINED"),
        "IfcFurniture" | "IfcFurnishingElement" => ("IFCFURNITURE", "NOTDEFINED"),
        "IfcSanitaryTerminal" => ("IFCSANITARYTERMINAL", "NOTDEFINED"),
        "IfcLightFixture" => ("IFCLIGHTFIXTURE", "NOTDEFINED"),
        _ => match category {
            "wall" => ("IFCWALL", "NOTDEFINED"),
            "slab" => ("IFCSLAB", "FLOOR"),
            "roof" | "roof element" => ("IFCSLAB", "ROOF"),
            "door" => ("IFCDOOR", "DOOR"),
            "window" => ("IFCWINDOW", "WINDOW"),
            "stair" => ("IFCSTAIR", "NOTDEFINED"),
            "space" => ("IFCSPACE", "SPACE"),
            "column" => ("IFCCOLUMN", "COLUMN"),
            "beam" => ("IFCBEAM", "BEAM"),
            "railing" => ("IFCRAILING", "NOTDEFINED"),
            _ => ("IFCBUILDINGELEMENTPROXY", "NOTDEFINED"),
        },
    }
}

/// Name, description, object type, tag, IFC class and predefined type (`""` for none) of an
/// element.
struct Identity {
    name: String,
    description: Option<String>,
    object_type: Option<String>,
    tag: Option<String>,
    class: &'static str,
    predefined: &'static str,
}

/// `run`: how the element runs, where its body is known (a Tekla beam's class follows it).
fn identity(format: &str, key: &Key, layer: &str, props: &[(String, String)], run: Option<Run>) -> Identity {
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let category = get("category").unwrap_or(layer);
    if format == "Tekla Structures" && matches!(key, Key::Element(_)) {
        // as Tekla's own export: the name, the profile as description, the material as tag; a
        // bolt group named after its standard; fasteners and rebars without profile and tag
        let kind = get("Type").unwrap_or("");
        let (class, predefined) = tekla_class(get("Name").unwrap_or(""), kind, get("Part type").unwrap_or(""), get("Profile").unwrap_or(""), run);
        let part = !matches!(class, "IFCFASTENER" | "IFCMECHANICALFASTENER" | "IFCREINFORCINGBAR");
        let name = if kind == "Bolt group" { get("Standard").or(get("Name")) } else { get("Name") };
        return Identity {
            name: name.unwrap_or(category).to_string(),
            description: get("Profile").filter(|_| part).map(str::to_string),
            object_type: None,
            tag: get("Material").filter(|_| part).map(str::to_string),
            class,
            predefined,
        };
    }
    let (class, predefined) = match format {
        "Revit" => revit_class(category).unwrap_or(("IFCBUILDINGELEMENTPROXY", "NOTDEFINED")),
        "Archicad" => match category {
            "Wall" => ("IFCWALL", "NOTDEFINED"),
            "Slab" => ("IFCSLAB", "FLOOR"),
            "Column" => ("IFCCOLUMN", "COLUMN"),
            "Beam" => ("IFCBEAM", "BEAM"),
            "Stair" => ("IFCSTAIR", "NOTDEFINED"),
            "Railing" => ("IFCRAILING", "NOTDEFINED"),
            // a roof plane: IfcRoof is the container of such slabs in IFC 4
            "Roof" => ("IFCSLAB", "ROOF"),
            "Mesh" => ("IFCGEOGRAPHICELEMENT", "TERRAIN"),
            "Zone" => ("IFCSPACE", "SPACE"),
            _ => ("IFCBUILDINGELEMENTPROXY", "NOTDEFINED"),
        },
        // Vectorworks: the IFC entity the file gives the object, else what the object is
        "Vectorworks" => vectorworks_class(get("IFC").unwrap_or(""), category),
        // an element this viewer modelled says its class itself
        _ => match get("class").unwrap_or("") {
            "IfcWall" => ("IFCWALL", "NOTDEFINED"),
            "IfcSlab" => ("IFCSLAB", "FLOOR"),
            // a roof plane is a slab of the roof in IFC 4; a ceiling a covering
            "IfcRoof" => ("IFCSLAB", "ROOF"),
            "IfcCovering" => ("IFCCOVERING", "CEILING"),
            "IfcBeam" => ("IFCBEAM", "BEAM"),
            "IfcColumn" => ("IFCCOLUMN", "COLUMN"),
            "IfcMember" => ("IFCMEMBER", "NOTDEFINED"),
            "IfcCurtainWall" => ("IFCCURTAINWALL", "NOTDEFINED"),
            "IfcDoor" => ("IFCDOOR", "DOOR"),
            "IfcWindow" => ("IFCWINDOW", "WINDOW"),
            "IfcSpace" => ("IFCSPACE", "SPACE"),
            _ => ("IFCBUILDINGELEMENTPROXY", "NOTDEFINED"),
        },
    };
    let name = match (get("family"), get("type")) {
        (Some(f), Some(t)) => format!("{f}:{t}"),
        (None, Some(t)) => t.to_string(),
        _ => get("Name").or(get("Definition")).unwrap_or(category).to_string(),
    };
    let (class, predefined) = match (format, get("Type")) {
        // Rhino's drafting: annotations
        ("Rhino", Some("Text" | "Hatch" | "Curve" | "Dimension" | "Leader" | "Center mark" | "Clipping plane" | "Light")) => ("IFCANNOTATION", "NOTDEFINED"),
        _ => (class, predefined),
    };
    let rhino = format == "Rhino";
    let object_type = if rhino { get("Type") } else { ["type", "Profile", "Definition", "Mesh"].iter().find_map(|k| get(k)) }.map(str::to_string);
    let tag = if rhino { get("Id") } else { ["id", "Tekla id"].iter().find_map(|k| get(k)) }.map(str::to_string);
    Identity { name, description: None, object_type, tag, class, predefined }
}

/// The IFC class (upper case) the export gives an element of `format` with these properties on
/// layer `layer`.
pub(crate) fn export_class(format: &str, layer: &str, props: &[(String, String)]) -> &'static str {
    identity(format, &Key::Element(0), layer, props, None).class
}

/// The material names of an element: Revit's "materials" list or a single "Material".
fn materials(props: &[(String, String)]) -> Vec<String> {
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.trim()).filter(|v| !v.is_empty());
    if let Some(list) = get("materials") {
        list.split(", ").filter(|m| !m.is_empty()).map(str::to_string).collect()
    } else {
        get("Material").map(|m| vec![m.to_string()]).unwrap_or_default()
    }
}

/// The build-up of a modelled wall or slab, for its material layer set.
#[derive(PartialEq)]
struct Layered {
    /// Its type's name.
    name: String,
    wall: bool,
    /// Each layer: its material's name, its thickness in metres, and whether it is a cavity.
    layers: Vec<(String, f64, bool)>,
}

/// The build-up of an element this viewer modelled, from its definition; the names of its type
/// and of its materials from its properties, where those name them.
fn layered(props: &[(String, String)]) -> Option<Layered> {
    use bim_core::element::PlanarForm;
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let all: Vec<bim_core::Element> = serde_json::from_str(get(super::ifcx::DEFINITION)?).ok()?;
    let bim_core::Shape::Planar(p) = &all.first()?.shape else { return None };
    let b = p.build_up.as_ref()?;
    // "brickwork 100, air cavity 40": each layer's material name before its thickness
    let named: Vec<&str> = get("Layers").map(|l| l.split(", ").map(|part| part.rsplit_once(' ').map_or(part, |(name, _)| name)).collect()).unwrap_or_default();
    let layers = b.layers.iter().enumerate().map(|(i, l)| (named.get(i).filter(|_| named.len() == b.layers.len()).map_or_else(|| l.material.clone(), |n| n.to_string()), l.thickness, l.cavity)).collect();
    Some(Layered { name: get("Type").unwrap_or(&b.id).to_string(), wall: matches!(p.form, PlanarForm::Wall { .. }), layers })
}

/// Writes the model as an IFC 4 file named `file_name`, with the document's levels as storeys.
pub fn write(model: &Model, document: &Document, file_name: &str, out: &mut impl Write) -> io::Result<()> {
    write_schema(model, document, file_name, Schema::Ifc4, out)
}

/// Writes the model as an IFC file in `schema` named `file_name`, with the document's levels as
/// storeys.
pub fn write_schema(model: &Model, document: &Document, file_name: &str, schema: Schema, out: &mut impl Write) -> io::Result<()> {
    let x3 = schema == Schema::Ifc4x3;
    // a point list's end: IFC 4x3 adds its (empty) TagList
    let list_end = if x3 { "),$)" } else { "))" };
    let units = Units::of(model);
    let app = format!("Open BIM Suite {}", env!("CARGO_PKG_VERSION"));
    let ids = Guids { seed: file_name.to_string() };

    writeln!(out, "ISO-10303-21;")?;
    writeln!(out, "HEADER;")?;
    let view = if x3 { "ReferenceView" } else { "ReferenceView_V1.2" };
    writeln!(out, "FILE_DESCRIPTION(('ViewDefinition [{view}]'),'2;1');")?;
    writeln!(out, "FILE_NAME({},{},(''),(''),{},{},'');", text(file_name), text(&timestamp()), text(&app), text(&app))?;
    writeln!(out, "FILE_SCHEMA(('{}'));", schema.step())?;
    writeln!(out, "ENDSEC;")?;
    writeln!(out, "DATA;")?;
    let mut f = Step { out, next: 1 };

    // units and the geometric context
    let length = if units.millimetres {
        f.add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)")?
    } else {
        f.add("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)")?
    };
    let area = f.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)")?;
    let volume = f.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)")?;
    let angle = f.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)")?;
    let unit_assignment = f.add(format!("IFCUNITASSIGNMENT({})", refs(&[length, area, volume, angle])))?;
    let origin = f.add("IFCCARTESIANPOINT((0.,0.,0.))")?;
    let world = f.add(format!("IFCAXIS2PLACEMENT3D(#{origin},$,$)"))?;
    let north = f.add("IFCDIRECTION((0.,1.))")?;
    let precision = format!("1.E-{:02}", units.decimals);
    let context = f.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,{precision},#{world},#{north})"))?;
    let body_context = f.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{context},$,.MODEL_VIEW.,$)"))?;
    // made when the first curve or annotation is written
    let mut annotation_context: Option<u64> = None;
    let mut curve_styles: HashMap<[u8; 4], u64> = HashMap::new();

    // project, site, building
    let tekla = model.format == "Tekla Structures";
    let info = |k: &str| model.project.info.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let project_name = info("name").or(info("Project name")).map(str::to_string).unwrap_or(stem);
    let project = f.add(format!(
        "IFCPROJECT({},$,{},$,$,{},$,(#{context}),#{unit_assignment})",
        text(&ids.get("project")),
        text(&project_name),
        opt(info("number")),
    ))?;
    let project_props: Vec<(String, String)> = model.project.info.iter().filter(|(_, v)| !v.is_empty()).cloned().collect();
    property_set(&mut f, &ids, "project", "Project information", &project_props, project)?;

    // Tekla: as Tekla's own export, site, building and storey are 'Unknown' where the model's
    // locations (its Organizer) do not name them, with the project's address
    let tekla_places = if tekla { TeklaPlaces::of(model, document) } else { TeklaPlaces::default() };
    let address = if tekla { postal_address(&mut f, &info)? } else { None };
    let address = address.map_or("$".to_string(), |a| format!("#{a}"));
    let (site_name, building_name) = if tekla { ("Unknown", tekla_places.building.as_deref().unwrap_or("Unknown")) } else { ("Site", "Building") };
    let site_placement = f.add(format!("IFCLOCALPLACEMENT($,#{world})"))?;
    let site = f.add(format!("IFCSITE({},$,{},$,$,#{site_placement},$,$,.ELEMENT.,$,$,$,$,{address})", text(&ids.get("site")), text(site_name)))?;
    let building_placement = f.add(format!("IFCLOCALPLACEMENT(#{site_placement},#{world})"))?;
    let building = f.add(format!(
        "IFCBUILDING({},$,{},$,$,#{building_placement},$,$,.ELEMENT.,$,$,{address})",
        text(&ids.get("building")),
        text(building_name)
    ))?;
    f.add(format!("IFCRELAGGREGATES({},$,$,$,#{project},(#{site}))", text(&ids.get("aggregates/project"))))?;
    f.add(format!("IFCRELAGGREGATES({},$,$,$,#{site},(#{building}))", text(&ids.get("aggregates/site"))))?;

    // Tekla: the grids, one per level, in the building
    let mut grid_layers: Vec<(String, Vec<u64>)> = Vec::new();
    if tekla && !model.project.grids.is_empty() {
        let footprint = f.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('FootPrint','Model',*,*,*,*,#{context},$,.MODEL_VIEW.,$)"))?;
        let mut grids = Vec::new();
        for level in document.levels() {
            if let Some((grid, representation)) = grid(&mut f, &ids, schema, units, &model.project.grids, &level.name, level.elevation, building_placement, footprint)? {
                grids.push(grid);
                grid_layers.push((format!("Grid {}", level.name), vec![representation]));
            }
        }
        if !grids.is_empty() {
            f.add(format!("IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,{},#{building})", text(&ids.get("contained/building")), refs(&grids)))?;
        }
    }

    // storeys: the document's levels (low to high: the model's and those added), else one at
    // zero; Tekla's: those of its locations, else one
    let mut levels: Vec<(String, f64)> = document.levels().iter().map(|l| (l.name.clone(), l.elevation)).collect();
    if tekla {
        levels = tekla_places.storeys.clone();
    }
    if levels.is_empty() {
        levels.push(("Level 0".to_string(), 0.0));
    }
    let mut storeys: Vec<(u64, u64, i64)> = Vec::new(); // entity, placement, elevation (quantised)
    for (i, (name, z)) in levels.iter().enumerate() {
        let elevation = units.quantise(*z);
        let point = f.add(format!("IFCCARTESIANPOINT((0.,0.,{}))", fixed(elevation, units.decimals)))?;
        let axes = f.add(format!("IFCAXIS2PLACEMENT3D(#{point},$,$)"))?;
        let placement = f.add(format!("IFCLOCALPLACEMENT(#{building_placement},#{axes})"))?;
        let storey = f.add(format!(
            "IFCBUILDINGSTOREY({},$,{},$,$,#{placement},$,$,.ELEMENT.,{})",
            text(&ids.get(&format!("storey/{i}/{name}"))),
            text(name),
            fixed(elevation, units.decimals),
        ))?;
        storeys.push((storey, placement, elevation));
    }
    let storey_ids: Vec<u64> = storeys.iter().map(|s| s.0).collect();
    f.add(format!("IFCRELAGGREGATES({},$,$,$,#{building},{})", text(&ids.get("aggregates/building")), refs(&storey_ids)))?;

    // elements
    // a surface style per colour; Tekla's per colour and material, named after the material
    let mut styles: HashMap<([u8; 4], Option<String>), u64> = HashMap::new();
    // the category of each material (Tekla's: steel, concrete, timber, else its name)
    let mut categories: HashMap<String, String> = HashMap::new();
    let mut contained: Vec<Vec<u64>> = vec![Vec::new(); storeys.len()];
    let mut spaces: Vec<Vec<u64>> = vec![Vec::new(); storeys.len()];
    let mut by_material: Vec<(Vec<String>, Vec<u64>)> = Vec::new();
    let mut by_build_up: Vec<(Layered, Vec<u64>)> = Vec::new();
    let mut by_layer: Vec<(String, Vec<u64>)> = Vec::new();
    let mut material_ids: HashMap<String, u64> = HashMap::new();
    // the GlobalIds the source file gives its elements (Tekla's, from its object GUIDs), once each
    let mut own_ids: HashSet<String> = HashSet::new();
    for product in products(model) {
        let props = match product.key {
            Key::Element(id) => model.element_props(id),
            Key::Layer(_) => Vec::new(),
        };
        let prop = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
        let kind = if tekla { prop("Type").unwrap_or("") } else { "" };
        if tekla && tekla_left_out(kind) {
            continue;
        }
        let Some(body) = body(model, &product, units, if tekla { tekla_colour(kind) } else { None }) else { continue };
        let key = match &product.key {
            Key::Element(id) => format!("element/{id}"),
            Key::Layer(name) => format!("layer/{name}"),
        };
        let mut who = identity(&model.format, &product.key, &product.layer, &props, if tekla { run(&body) } else { None });
        // Tekla: its material as Tekla's export names it, with its category
        let tekla_material = if tekla { tekla_material(&props) } else { None };
        if let Some((name, category)) = &tekla_material {
            categories.entry(name.clone()).or_insert_with(|| category.clone());
        }
        if body.sets.is_empty() {
            // curves only
            who.class = "IFCANNOTATION";
        }
        let annotation = who.class == "IFCANNOTATION";

        // its storey: the level it names, else the highest level at or below its lowest point;
        // Tekla's: the storey of its location, else the one for those without
        let storey = if tekla {
            tekla_places.storey(prop("Storey"))
        } else {
            prop("level")
                .and_then(|l| levels.iter().position(|(n, _)| n == l))
                .unwrap_or_else(|| levels.iter().rposition(|(_, z)| *z <= body.min_z + 0.01).unwrap_or(0))
        };
        let (_, storey_placement, elevation) = storeys[storey];

        let mut items = Vec::new();
        let style_name = tekla_material.as_ref().map(|(m, _)| m.clone());
        for (colour, set) in &body.sets {
            let style = match styles.get(&(*colour, style_name.clone())) {
                Some(&s) => s,
                None => {
                    let s = surface_style(&mut f, *colour, style_name.as_deref())?;
                    styles.insert((*colour, style_name.clone()), s);
                    s
                }
            };
            let mut coords = String::with_capacity(set.points.len() * 30);
            coords.push_str("IFCCARTESIANPOINTLIST3D((");
            for (i, p) in set.points.iter().enumerate() {
                if i > 0 {
                    coords.push(',');
                }
                let _ = write!(
                    coords,
                    "({},{},{})",
                    fixed(p[0], units.decimals),
                    fixed(p[1], units.decimals),
                    fixed(p[2], units.decimals)
                );
            }
            coords.push_str(list_end);
            let points = f.add(coords)?;
            let mut faces = String::with_capacity(set.triangles.len() * 20);
            let _ = write!(faces, "IFCTRIANGULATEDFACESET(#{points},$,{},(", if set.closed() { ".T." } else { ".F." });
            for (i, t) in set.triangles.iter().enumerate() {
                if i > 0 {
                    faces.push(',');
                }
                let _ = write!(faces, "({},{},{})", t[0] + 1, t[1] + 1, t[2] + 1);
            }
            faces.push_str("),$)");
            let item = f.add(faces)?;
            f.add(format!("IFCSTYLEDITEM(#{item},(#{style}),$)"))?;
            items.push(item);
        }
        let mut curve_items = Vec::new();
        for (colour, points) in &body.curves {
            let style = match curve_styles.get(colour) {
                Some(&s) => s,
                None => {
                    let s = curve_style(&mut f, *colour)?;
                    curve_styles.insert(*colour, s);
                    s
                }
            };
            let mut coords = String::with_capacity(points.len() * 30);
            coords.push_str("IFCCARTESIANPOINTLIST3D((");
            for (i, p) in points.iter().enumerate() {
                if i > 0 {
                    coords.push(',');
                }
                let _ = write!(coords, "({},{},{})", fixed(p[0], units.decimals), fixed(p[1], units.decimals), fixed(p[2], units.decimals));
            }
            coords.push_str(list_end);
            let list = f.add(coords)?;
            let item = f.add(format!("IFCINDEXEDPOLYCURVE(#{list},$,.F.)"))?;
            f.add(format!("IFCSTYLEDITEM(#{item},(#{style}),$)"))?;
            curve_items.push(item);
        }
        if annotation || !curve_items.is_empty() {
            if annotation_context.is_none() {
                annotation_context = Some(f.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Annotation','Model',*,*,*,*,#{context},$,.MODEL_VIEW.,$)"))?);
            }
        }
        let mut representations = Vec::new();
        if !items.is_empty() {
            representations.push(if annotation {
                f.add(format!("IFCSHAPEREPRESENTATION(#{},'Annotation','Tessellation',{})", annotation_context.unwrap_or(body_context), refs(&items)))?
            } else {
                f.add(format!("IFCSHAPEREPRESENTATION(#{body_context},'Body','Tessellation',{})", refs(&items)))?
            });
        }
        if !curve_items.is_empty() {
            representations.push(f.add(format!("IFCSHAPEREPRESENTATION(#{},'Annotation','Curve3D',{})", annotation_context.unwrap_or(body_context), refs(&curve_items)))?);
        }
        let shape = f.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,{})", refs(&representations)))?;
        let at = [body.origin[0], body.origin[1], body.origin[2] - elevation];
        let point = f.add(format!(
            "IFCCARTESIANPOINT(({},{},{}))",
            fixed(at[0], units.decimals),
            fixed(at[1], units.decimals),
            fixed(at[2], units.decimals)
        ))?;
        let axes = f.add(format!("IFCAXIS2PLACEMENT3D(#{point},$,$)"))?;
        let placement = f.add(format!("IFCLOCALPLACEMENT(#{storey_placement},#{axes})"))?;
        // its own GlobalId where its file gives one (so the IFC matches the source's own export),
        // else one derived from the file and the element
        let global_id = own_global_id(&props).filter(|g| own_ids.insert(g.clone())).unwrap_or_else(|| ids.get(&key));
        let head = format!(
            "{}({},$,{},{},{},#{placement},#{shape},{}",
            who.class,
            text(&global_id),
            text(&who.name),
            opt(who.description.as_deref()),
            opt(who.object_type.as_deref()),
            opt(who.tag.as_deref()),
        );
        let predefined = enumeration(who.predefined);
        let element = f.add(match who.class {
            // an annotation has no tag; a predefined type in IFC 4x3 only
            "IFCANNOTATION" => format!(
                "IFCANNOTATION({},$,{},$,{},#{placement},#{shape}{})",
                text(&ids.get(&key)),
                text(&who.name),
                opt(who.object_type.as_deref()),
                if x3 { ",.NOTDEFINED." } else { "" },
            ),
            "IFCDOOR" => format!("{head},$,$,{predefined},.NOTDEFINED.,$)"),
            "IFCWINDOW" => format!("{head},$,$,{predefined},.NOTDEFINED.,$)"),
            "IFCREINFORCINGBAR" => format!("{head},$,$,$,$,{predefined},$)"),
            // nominal diameter and length unknown here
            "IFCMECHANICALFASTENER" => format!("{head},$,$,{predefined})"),
            // the tag is the long name; an element of its storey, not a composite
            "IFCSPACE" => format!("{head},.ELEMENT.,{predefined},$)"),
            _ => format!("{head},{predefined})"),
        })?;
        property_set(&mut f, &ids, &key, &model.format, &props, element)?;

        // a space is part of its storey (IfcRelAggregates), not contained in it
        if who.class == "IFCSPACE" {
            spaces[storey].push(element);
        } else {
            contained[storey].push(element);
        }
        // a build-up is a layer set; else the materials the properties name
        let build_up = if annotation { None } else { layered(&props) };
        let mats = if build_up.is_some() || annotation {
            Vec::new()
        } else if tekla {
            tekla_material.iter().map(|(m, _)| m.clone()).collect()
        } else {
            materials(&props)
        };
        if let Some(build_up) = build_up {
            match by_build_up.iter_mut().find(|(b, _)| *b == build_up) {
                Some((_, list)) => list.push(element),
                None => by_build_up.push((build_up, vec![element])),
            }
        }
        if !mats.is_empty() {
            match by_material.iter_mut().find(|(m, _)| *m == mats) {
                Some((_, list)) => list.push(element),
                None => by_material.push((mats, vec![element])),
            }
        }
        // its presentation layer: the layer it is drawn in; Tekla's export: its phase
        let layer = if tekla { prop("Phase name").unwrap_or(&product.layer) } else { &product.layer };
        match by_layer.iter_mut().find(|(l, _)| l == layer) {
            Some((_, list)) => list.extend(&representations),
            None => by_layer.push((layer.to_string(), representations)),
        }
    }
    by_layer.extend(grid_layers);

    // relations
    for (i, elements) in spaces.iter().enumerate().filter(|(_, e)| !e.is_empty()) {
        f.add(format!("IFCRELAGGREGATES({},$,$,$,#{},{})", text(&ids.get(&format!("spaces/{i}"))), storeys[i].0, refs(elements)))?;
    }
    for (i, elements) in contained.iter().enumerate().filter(|(_, e)| !e.is_empty()) {
        f.add(format!(
            "IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,{},#{})",
            text(&ids.get(&format!("contained/{i}"))),
            refs(elements),
            storeys[i].0
        ))?;
    }
    for (mats, elements) in &by_material {
        let mut one = Vec::new();
        for m in mats {
            let id = match material_ids.get(m) {
                Some(&id) => id,
                None => {
                    let id = f.add(format!("IFCMATERIAL({},$,{})", text(m), opt(categories.get(m).map(String::as_str))))?;
                    material_ids.insert(m.clone(), id);
                    id
                }
            };
            one.push(id);
        }
        let material = if one.len() == 1 {
            one[0]
        } else {
            let mut constituents = Vec::new();
            for (m, id) in mats.iter().zip(&one) {
                constituents.push(f.add(format!("IFCMATERIALCONSTITUENT({},$,#{id},$,$)", text(m)))?);
            }
            f.add(format!("IFCMATERIALCONSTITUENTSET({},$,{})", text(&mats.join(", ")), refs(&constituents)))?
        };
        f.add(format!(
            "IFCRELASSOCIATESMATERIAL({},$,$,$,{},#{material})",
            text(&ids.get(&format!("material/{}", mats.join("|")))),
            refs(elements)
        ))?;
    }
    for (i, (build_up, elements)) in by_build_up.iter().enumerate() {
        let in_units = |metres: f64| real(metres * if units.millimetres { 1000.0 } else { 1.0 });
        let mut layers = Vec::new();
        for (name, thickness, cavity) in &build_up.layers {
            let material = match material_ids.get(name) {
                Some(&id) => id,
                None => {
                    let id = f.add(format!("IFCMATERIAL({},$,$)", text(name)))?;
                    material_ids.insert(name.clone(), id);
                    id
                }
            };
            let ventilated = if *cavity { ".T." } else { ".F." };
            layers.push(f.add(format!("IFCMATERIALLAYER(#{material},{},{ventilated},{},$,$,$)", in_units(*thickness), text(name)))?);
        }
        let set = f.add(format!("IFCMATERIALLAYERSET({},{},$)", refs(&layers), text(&build_up.name)))?;
        let total: f64 = build_up.layers.iter().map(|l| l.1).sum();
        let usage = if build_up.wall {
            f.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS2.,.POSITIVE.,{},$)", in_units(-total / 2.0)))?
        } else {
            f.add(format!("IFCMATERIALLAYERSETUSAGE(#{set},.AXIS3.,.NEGATIVE.,0.,$)"))?
        };
        f.add(format!(
            "IFCRELASSOCIATESMATERIAL({},$,$,$,{},#{usage})",
            text(&ids.get(&format!("layers/{i}/{}", build_up.name))),
            refs(elements)
        ))?;
    }
    for (name, representations) in &by_layer {
        f.add(format!("IFCPRESENTATIONLAYERASSIGNMENT({},$,{},$)", text(name), refs(representations)))?;
    }

    writeln!(f.out, "ENDSEC;")?;
    writeln!(f.out, "END-ISO-10303-21;")?;
    Ok(())
}

/// A surface style for a colour (sRGB8, alpha 255 = opaque), named `name` or after its colour
/// (`#RRGGBB`).
fn surface_style(f: &mut Step<impl Write>, c: [u8; 4], name: Option<&str>) -> io::Result<u64> {
    let ratio = |v: u8| real(v as f64 / 255.0);
    let colour = f.add(format!("IFCCOLOURRGB($,{},{},{})", ratio(c[0]), ratio(c[1]), ratio(c[2])))?;
    let shading = f.add(format!("IFCSURFACESTYLESHADING(#{colour},{})", real(1.0 - c[3] as f64 / 255.0)))?;
    let name = name.map_or_else(|| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]), str::to_string);
    f.add(format!("IFCSURFACESTYLE({},.BOTH.,(#{shading}))", text(&name)))
}

/// Where Tekla's export puts things: the storeys of the model's locations (its Organizer) by name
/// and height, the one for objects without, and the building's name where the locations give
/// one. Without locations, one storey 'Unknown' at zero, as Tekla's export without them.
#[derive(Default)]
struct TeklaPlaces {
    storeys: Vec<(String, f64)>,
    /// The storey of objects without a location.
    unplaced: usize,
    building: Option<String>,
}

impl TeklaPlaces {
    fn of(model: &Model, document: &Document) -> Self {
        let mut names: Vec<String> = Vec::new();
        let mut buildings: Vec<String> = Vec::new();
        let mut any_unplaced = false;
        let mut ids: Vec<&u64> = model.props.keys().collect();
        ids.sort();
        for id in ids {
            let props = model.element_props(*id);
            let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.trim()).filter(|v| !v.is_empty());
            if get("Type").is_some_and(tekla_left_out) {
                continue;
            }
            match get("Storey") {
                Some(s) if !names.iter().any(|n| n == s) => names.push(s.to_string()),
                Some(_) => {}
                None => any_unplaced = true,
            }
            if let Some(b) = get("Building").filter(|b| !buildings.iter().any(|n| n == b)) {
                buildings.push(b.to_string());
            }
        }
        // a storey's height: the level of its name, else zero
        let height = |name: &str| document.levels().iter().find(|l| l.name == name).map_or(0.0, |l| l.elevation);
        let mut storeys: Vec<(String, f64)> = names.iter().map(|n| (n.clone(), height(n))).collect();
        storeys.sort_by(|a, b| a.1.total_cmp(&b.1));
        if any_unplaced || storeys.is_empty() {
            storeys.push(("Unknown".to_string(), 0.0));
        }
        let unplaced = storeys.len() - 1;
        let building = (buildings.len() == 1).then(|| buildings.remove(0));
        TeklaPlaces { storeys, unplaced, building }
    }

    /// The storey of an object in the storey `name` of the locations.
    fn storey(&self, name: Option<&str>) -> usize {
        name.and_then(|n| self.storeys.iter().position(|(s, _)| s == n)).unwrap_or(self.unplaced)
    }
}

/// The project's postal address (Tekla's project information), when it has a town or country.
fn postal_address<'a>(f: &mut Step<impl Write>, info: &impl Fn(&str) -> Option<&'a str>) -> io::Result<Option<u64>> {
    if info("Town").is_none() && info("Country").is_none() {
        return Ok(None);
    }
    let lines = info("Address").map_or("$".to_string(), |a| format!("({})", text(a)));
    Ok(Some(f.add(format!(
        "IFCPOSTALADDRESS($,$,$,$,{lines},{},{},{},{},{})",
        opt(info("Postal box")),
        opt(info("Town")),
        opt(info("Region")),
        opt(info("Postal code")),
        opt(info("Country")),
    ))?))
}

/// An IfcGrid at a level, as Tekla's export writes one per level of its grids: the grid lines
/// that run along y as its U axes, those along x as its V axes, each a 2D line in the grid's
/// placement; with its 'FootPrint' representation. `None` without lines both ways.
#[allow(clippy::too_many_arguments)]
fn grid(
    f: &mut Step<impl Write>,
    ids: &Guids,
    schema: Schema,
    units: Units,
    lines: &[crate::model::GridLine],
    name: &str,
    elevation: f64,
    building_placement: u64,
    footprint: u64,
) -> io::Result<Option<(u64, u64)>> {
    let mut u: Vec<&crate::model::GridLine> = Vec::new();
    let mut v: Vec<&crate::model::GridLine> = Vec::new();
    let mut seen: HashSet<(String, [i64; 4])> = HashSet::new();
    for line in lines {
        let ends = [line.a[0], line.a[1], line.b[0], line.b[1]].map(|c| units.quantise(c as f64));
        if !seen.insert((line.name.clone(), ends)) || (ends[0], ends[1]) == (ends[2], ends[3]) {
            continue;
        }
        if (ends[2] - ends[0]).abs() <= (ends[3] - ends[1]).abs() {
            u.push(line);
        } else {
            v.push(line);
        }
    }
    if u.is_empty() || v.is_empty() {
        return Ok(None);
    }
    let list_end = if schema == Schema::Ifc4x3 { "),$)" } else { "))" };
    let mut curves = Vec::new();
    let mut axes = |f: &mut Step<_>, lines: &[&crate::model::GridLine]| -> io::Result<Vec<u64>> {
        let mut out = Vec::new();
        for line in lines {
            let c = |x: f32| fixed(units.quantise(x as f64), units.decimals);
            let points = f.add(format!("IFCCARTESIANPOINTLIST2D((({},{}),({},{})){list_end}", c(line.a[0]), c(line.a[1]), c(line.b[0]), c(line.b[1])))?;
            let curve = f.add(format!("IFCINDEXEDPOLYCURVE(#{points},$,.F.)"))?;
            curves.push(curve);
            out.push(f.add(format!("IFCGRIDAXIS({},#{curve},.T.)", text(&line.name)))?);
        }
        Ok(out)
    };
    let u = axes(f, &u)?;
    let v = axes(f, &v)?;
    let point = f.add(format!("IFCCARTESIANPOINT((0.,0.,{}))", fixed(units.quantise(elevation), units.decimals)))?;
    let placement = f.add(format!("IFCAXIS2PLACEMENT3D(#{point},$,$)"))?;
    let placement = f.add(format!("IFCLOCALPLACEMENT(#{building_placement},#{placement})"))?;
    let set = f.add(format!("IFCGEOMETRICCURVESET({})", refs(&curves)))?;
    let representation = f.add(format!("IFCSHAPEREPRESENTATION(#{footprint},'FootPrint','GeometricCurveSet',(#{set}))"))?;
    let shape = f.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{representation}))"))?;
    let grid = f.add(format!(
        "IFCGRID({},$,{},$,$,#{placement},#{shape},{},{},$,.RECTANGULAR.)",
        text(&ids.get(&format!("grid/{name}"))),
        text(name),
        refs(&u),
        refs(&v),
    ))?;
    Ok(Some((grid, representation)))
}

/// An enumeration value: `.VALUE.`, or `$` for none (`""`).
fn enumeration(value: &str) -> String {
    if value.is_empty() {
        "$".to_string()
    } else {
        format!(".{value}.")
    }
}

/// A curve style for a colour (sRGB8; curves are drawn opaque).
fn curve_style(f: &mut Step<impl Write>, c: [u8; 4]) -> io::Result<u64> {
    let ratio = |v: u8| real(v as f64 / 255.0);
    let colour = f.add(format!("IFCCOLOURRGB($,{},{},{})", ratio(c[0]), ratio(c[1]), ratio(c[2])))?;
    let name = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    f.add(format!("IFCCURVESTYLE({},$,$,#{colour},$)", text(&name)))
}

/// A property set of text values for one object (none when there are no values).
fn property_set(
    f: &mut Step<impl Write>,
    ids: &Guids,
    key: &str,
    name: &str,
    props: &[(String, String)],
    object: u64,
) -> io::Result<()> {
    let mut values = Vec::new();
    for (k, v) in props.iter().filter(|(k, v)| !k.is_empty() && !v.is_empty()) {
        // IfcLabel holds at most 255 characters
        let value = if v.chars().count() <= 255 { format!("IFCLABEL({})", text(v)) } else { format!("IFCTEXT({})", text(v)) };
        values.push(f.add(format!("IFCPROPERTYSINGLEVALUE({},$,{value},$)", text(k)))?);
    }
    if values.is_empty() {
        return Ok(());
    }
    let set = f.add(format!("IFCPROPERTYSET({},$,{},$,{})", text(&ids.get(&format!("pset/{key}"))), text(name), refs(&values)))?;
    f.add(format!("IFCRELDEFINESBYPROPERTIES({},$,$,$,(#{object}),#{set})", text(&ids.get(&format!("defines/{key}")))))?;
    Ok(())
}

/// The DATA section under construction: numbered entity instances.
struct Step<'w, W: Write> {
    out: &'w mut W,
    next: u64,
}

impl<W: Write> Step<'_, W> {
    fn add(&mut self, entity: impl AsRef<str>) -> io::Result<u64> {
        let id = self.next;
        self.next += 1;
        writeln!(self.out, "#{id}={};", entity.as_ref())?;
        Ok(id)
    }
}

/// The GlobalId an element's source gives it (`IFC GlobalId`, as the Tekla reader derives it from
/// the object's GUID), when it is one: 22 characters of IFC's base-64 alphabet.
fn own_global_id(props: &[(String, String)]) -> Option<String> {
    let id = props.iter().find(|(k, _)| k == "IFC GlobalId").map(|(_, v)| v.trim().trim_matches('"'))?;
    (id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')).then(|| id.to_string())
}

/// Stable GlobalIds: a 128-bit hash of the file name and a key, in IFC's base-64 form.
struct Guids {
    seed: String,
}

impl Guids {
    fn get(&self, key: &str) -> String {
        let hash = |basis: u64| {
            let mut h = basis;
            for b in self.seed.bytes().chain([0]).chain(key.bytes()) {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            // splitmix64 finaliser: spread the FNV-1a state over all bits
            h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            h ^ (h >> 31)
        };
        let n = ((hash(0xcbf2_9ce4_8422_2325) as u128) << 64) | hash(0x8422_2325_cbf2_9ce4) as u128;
        compress_guid(n)
    }
}

/// A 128-bit number in IFC's 22-character base-64 GlobalId form.
fn compress_guid(n: u128) -> String {
    const DIGITS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
    (0..22).map(|i| DIGITS[((n >> (6 * (21 - i))) & 63) as usize] as char).collect()
}

/// A STEP string: quotes and backslashes doubled, characters outside printable ASCII as
/// `\X2\` UTF-16 hex.
fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    let mut wide = false;
    for c in s.chars() {
        if (' '..='~').contains(&c) {
            if wide {
                out.push_str("\\X0\\");
                wide = false;
            }
            match c {
                '\'' => out.push_str("''"),
                '\\' => out.push_str("\\\\"),
                _ => out.push(c),
            }
        } else {
            if !wide {
                out.push_str("\\X2\\");
                wide = true;
            }
            for u in c.encode_utf16(&mut [0; 2]) {
                let _ = write!(out, "{u:04X}");
            }
        }
    }
    if wide {
        out.push_str("\\X0\\");
    }
    out.push('\'');
    out
}

/// An optional string: the string, or `$`.
fn opt(s: Option<&str>) -> String {
    s.map(text).unwrap_or_else(|| "$".to_string())
}

/// `(#1,#2,...)`
fn refs(ids: &[u64]) -> String {
    let mut out = String::with_capacity(ids.len() * 8 + 2);
    out.push('(');
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "#{id}");
    }
    out.push(')');
    out
}

/// A quantised value as a STEP real with at most `decimals` decimals: 1250 (2) → `12.5`, 0 → `0.`.
fn fixed(v: i64, decimals: u32) -> String {
    let scale = 10u64.pow(decimals);
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    let (int, mut frac) = (a / scale, a % scale);
    if frac == 0 {
        return format!("{sign}{int}.");
    }
    let mut digits = decimals as usize;
    while frac % 10 == 0 {
        frac /= 10;
        digits -= 1;
    }
    format!("{sign}{int}.{frac:0digits$}")
}

/// A STEP real with up to six decimals.
fn real(v: f64) -> String {
    fixed((if v.is_finite() { v } else { 0.0 } * 1e6).round() as i64, 6)
}

/// The current time as ISO 8601 (UTC), for the header.
fn timestamp() -> String {
    let secs = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}", rest / 3600, rest / 60 % 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectorworks_objects_take_the_class_their_file_gives_them() {
        // the entity of the IFC data Vectorworks keeps, also for the things of a type
        assert_eq!(vectorworks_class("IfcWallStandardCase", "wall"), ("IFCWALL", "NOTDEFINED"));
        assert_eq!(vectorworks_class("IfcDoor", "door"), ("IFCDOOR", "DOOR"));
        assert_eq!(vectorworks_class("IfcColumn", "beam"), ("IFCCOLUMN", "COLUMN"));
        assert_eq!(vectorworks_class("IfcSanitaryTerminalType", "symbol"), ("IFCSANITARYTERMINAL", "NOTDEFINED"));
        // without IFC data: by what the object is
        assert_eq!(vectorworks_class("", "window"), ("IFCWINDOW", "WINDOW"));
        assert_eq!(vectorworks_class("", "roof"), ("IFCSLAB", "ROOF"));
        assert_eq!(vectorworks_class("IfcSomethingElse", "extrude"), ("IFCBUILDINGELEMENTPROXY", "NOTDEFINED"));
    }
    use crate::model::{flat_shaded, Layer, Model};
    use std::path::PathBuf;

    /// A unit cube with `n` triangles drawn per element.
    fn cube(at: [f32; 3]) -> (Vec<f32>, Vec<u32>) {
        let mut p = Vec::new();
        for i in 0..8 {
            p.extend_from_slice(&[at[0] + (i & 1) as f32, at[1] + ((i >> 1) & 1) as f32, at[2] + ((i >> 2) & 1) as f32]);
        }
        let idx = vec![0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
        (p, idx)
    }

    fn revit_model() -> Model {
        let (p1, i1) = cube([0.0, 0.0, 0.0]);
        let (p2, i2) = cube([3.0, 0.0, 3.0]);
        let mut positions = p1;
        positions.extend(p2);
        let mut indices = i1;
        indices.extend(i2.iter().map(|i| i + 8));
        let (positions, normals, indices) = flat_shaded(&positions, &indices);
        let walls = Layer {
            name: "Walls".into(),
            color: [0.9, 0.88, 0.84],
            positions,
            normals,
            indices,
            elements: vec![(101, 0, 36), (102, 36, 36)],
            props: vec![
                (101, r#"{"id":"101","category":"Walls","family":"Basic Wall","type":"Ext 300","level":"Level 1","materials":"Brick, Concrete"}"#.into()),
                (102, r#"{"id":"102","category":"Walls","family":"Basic Wall","type":"Int 100","materials":"Gypsum"}"#.into()),
            ],
            ..Layer::default()
        };
        let mut m = Model::new(PathBuf::from("test.rvt"), "Revit", vec![walls]);
        m.project.levels = vec![("Level 1".into(), 0.0), ("Level 2".into(), 3.0)];
        m.project.info = vec![("name".into(), "Café 't Hoekje".into()), ("number".into(), "P-01".into())];
        m
    }

    fn ifc(model: &Model) -> String {
        let mut out = Vec::new();
        write(model, &Document::from_model(model), "test.ifc", &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn ifc4x3(model: &Model) -> String {
        let mut out = Vec::new();
        write_schema(model, &Document::from_model(model), "test.ifc", Schema::Ifc4x3, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// A Rhino model with a curve of its own (an annotation) and a box.
    fn rhino_curve_model() -> Model {
        let (positions, idx) = cube([0.0, 0.0, 0.0]);
        let (mut positions, normals, indices) = flat_shaded(&positions, &idx);
        let first = (positions.len() / 3) as u32;
        positions.extend_from_slice(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0]);
        let mut normals = normals;
        normals.extend_from_slice(&[0.0; 9]);
        let layer = Layer {
            name: "Default".into(),
            color: [0.2, 0.2, 0.2],
            positions,
            normals,
            indices,
            elements: vec![(1, 0, 36)],
            lines: vec![first, first + 1, first + 1, first + 2],
            line_elements: vec![(2, 0, 4)],
            props: vec![(1, r#"{"Type":"Brep","Id":"a"}"#.into()), (2, r#"{"Type":"Curve","Id":"b"}"#.into())],
            ..Layer::default()
        };
        Model::new(PathBuf::from("test.3dm"), "Rhino", vec![layer])
    }

    #[test]
    fn ifc4x3_names_its_schema_and_writes_its_own_attributes() {
        for model in [revit_model(), rhino_curve_model()] {
            let s = ifc4x3(&model);
            assert!(s.contains("FILE_SCHEMA(('IFC4X3_ADD2'));"), "{s}");
            assert!(s.contains("ViewDefinition [ReferenceView]"), "{s}");
            // point lists: with their (empty) TagList
            let lists: Vec<&str> = s.lines().filter(|l| l.contains("=IFCCARTESIANPOINTLIST3D(")).collect();
            assert!(!lists.is_empty());
            assert!(lists.iter().all(|l| l.ends_with("),$);")), "{lists:?}");
            // the same elements as IFC 4, apart from those attributes
            let four = ifc(&model);
            let classes = |s: &str| s.lines().filter_map(|l| l.split_once('=').map(|(_, r)| r.split('(').next().unwrap_or("").to_string())).collect::<Vec<_>>();
            assert_eq!(classes(&s), classes(&four));
        }
        // an annotation has a predefined type in IFC 4x3, none in IFC 4
        let model = rhino_curve_model();
        let x3 = ifc4x3(&model);
        let annotation = x3.lines().find(|l| l.contains("=IFCANNOTATION(")).expect("an annotation");
        assert!(annotation.ends_with(",.NOTDEFINED.);"), "{annotation}");
        let four = ifc(&model);
        let annotation = four.lines().find(|l| l.contains("=IFCANNOTATION(")).expect("an annotation");
        assert!(annotation.ends_with(")") || annotation.ends_with(");"));
        assert!(!annotation.contains(".NOTDEFINED."), "{annotation}");
        // IFC 4 as it was
        assert!(four.contains("FILE_SCHEMA(('IFC4'));"));
        assert!(four.lines().filter(|l| l.contains("=IFCCARTESIANPOINTLIST3D(")).all(|l| l.ends_with("));")));
    }

    #[test]
    fn schema_names() {
        for n in ["IFC4X3", "ifc4x3", "IFC4X3_ADD2", "4x3", "IFC 4x3"] {
            assert_eq!(Schema::parse(n), Some(Schema::Ifc4x3), "{n}");
        }
        for n in ["IFC4", "ifc4", "IFC 4", "4"] {
            assert_eq!(Schema::parse(n), Some(Schema::Ifc4), "{n}");
        }
        assert_eq!(Schema::parse("IFC2X3"), None);
        assert_eq!(Schema::Ifc4x3.label(), "IFC 4x3");
    }

    #[test]
    fn tekla_objects_get_the_class_tekla_exports_them_as() {
        assert_eq!(tekla_class("KOLOM", "Column", "COLUMN", "HEA300", Some(Run::Plumb)), ("IFCCOLUMN", "NOTDEFINED"));
        assert_eq!(tekla_class("SPANT", "Beam", "BEAM", "IPE400", Some(Run::Level)), ("IFCBEAM", "NOTDEFINED"));
        // a beam neither level nor plumb is a member; a plumb one stays a beam
        assert_eq!(tekla_class("SCHROEF", "Beam", "BEAM", "D8", Some(Run::Inclined)), ("IFCMEMBER", "NOTDEFINED"));
        assert_eq!(tekla_class("ANKER", "Beam", "BEAM", "D12", Some(Run::Plumb)), ("IFCBEAM", "NOTDEFINED"));
        assert_eq!(tekla_class("LUIFELLIGGER", "Polybeam", "", "IPE200", None).0, "IFCBEAM");
        assert_eq!(tekla_class("VLOER", "Slab", "SLAB", "200", None), ("IFCSLAB", "NOTDEFINED"));
        assert_eq!(tekla_class("POER", "Pad footing", "", "1500*1500", None), ("IFCFOOTING", "PAD_FOOTING"));
        assert_eq!(tekla_class("FUNDERINGSBALK", "Strip footing", "", "600*800", None), ("IFCFOOTING", "STRIP_FOOTING"));
        // fasteners without a predefined type
        assert_eq!(tekla_class("WELD", "Weld", "", "", None), ("IFCFASTENER", ""));
        assert_eq!(tekla_class("M20 7990", "Bolt group", "", "", None), ("IFCMECHANICALFASTENER", ""));
        assert_eq!(tekla_class("BEUGEL", "Rebar group", "", "", None), ("IFCREINFORCINGBAR", "NOTDEFINED"));
        assert_eq!(tekla_class("STAAF", "Single rebar", "", "", None), ("IFCREINFORCINGBAR", "NOTDEFINED"));
        // a beam or column of a plate's section stays one ("flat wide beams as plates" off)
        assert_eq!(tekla_class("PLAAT", "Column", "COLUMN", "PL10*150", Some(Run::Plumb)).0, "IFCCOLUMN");
        assert_eq!(tekla_class("SCHOTJE", "Beam", "BEAM", "FL80*10", Some(Run::Level)).0, "IFCBEAM");
        // a profile part: by its part type, else a beam
        assert_eq!(tekla_class("H_PAAL", "Profile part", "COLUMN", "300*300", Some(Run::Plumb)).0, "IFCCOLUMN");
        assert_eq!(tekla_class("H_NOKKAP", "Profile part", "", "PL3", Some(Run::Level)).0, "IFCBEAM");
        // a file that does not say what the part is: by its name, in English or Dutch, or profile
        assert_eq!(tekla_class("COLUMN", "", "", "HEA200", None).0, "IFCCOLUMN");
        assert_eq!(tekla_class("GORDING", "", "", "UNP160", None).0, "IFCBEAM");
        assert_eq!(tekla_class("P1", "", "", "PL10", None).0, "IFCPLATE");
        // what Tekla's export leaves out
        assert!(tekla_left_out("Control point") && tekla_left_out("Grid plane") && tekla_left_out("Point load"));
        assert!(!tekla_left_out("Beam") && !tekla_left_out("Weld"));
    }

    /// A bar of `length` along `along`, 0.1 square (quantised to 0.01 mm), as one face set.
    fn bar(along: [f64; 3], length: f64) -> Body {
        let n = { let l = (along[0] * along[0] + along[1] * along[1] + along[2] * along[2]).sqrt(); along.map(|c| c / l) };
        // two directions across it
        let up = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
        let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
        let u = cross(n, up);
        let lu = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        let u = u.map(|c| c / lu);
        let v = cross(n, u);
        let corner = |i: usize| [0, 1, 2].map(|k| ((i & 1) as f64 * length * n[k] + ((i >> 1) & 1) as f64 * 0.1 * u[k] + ((i >> 2) & 1) as f64 * 0.1 * v[k]) * 1e5).map(|c| c.round() as i64);
        let (_, idx) = cube([0.0; 3]);
        let mut set = FaceSet::default();
        for t in idx.chunks_exact(3) {
            set.triangle([corner(t[0] as usize), corner(t[1] as usize), corner(t[2] as usize)]);
        }
        Body { origin: [0; 3], sets: vec![([0; 4], set)], curves: Vec::new(), min_z: 0.0 }
    }

    #[test]
    fn bodies_run_along_their_longest_edges() {
        assert_eq!(run(&bar([1.0, 0.0, 0.0], 3.0)), Some(Run::Level));
        assert_eq!(run(&bar([1.0, 1.0, 0.0], 0.3)), Some(Run::Level));
        assert_eq!(run(&bar([0.0, 0.0, 1.0], 0.3)), Some(Run::Plumb));
        assert_eq!(run(&bar([1.0, 0.0, 2.0], 0.3)), Some(Run::Inclined));
        assert_eq!(run(&bar([0.0, 1.0, 0.01], 5.0)), Some(Run::Inclined));
        // a cube runs any of its ways, but not along a diagonal
        assert_ne!(run(&bar([1.0, 0.0, 0.0], 0.1)), Some(Run::Inclined));
        assert_eq!(run(&Body { origin: [0; 3], sets: Vec::new(), curves: Vec::new(), min_z: 0.0 }), None);
    }

    /// Two Tekla parts: a column with its GUID, and a beam whose GlobalId the column already has.
    fn tekla_model() -> Model {
        let (p1, i1) = cube([0.0, 0.0, 0.0]);
        let (p2, i2) = cube([0.0, 0.0, 4.2]);
        let mut positions = p1;
        positions.extend(p2);
        let mut indices = i1;
        indices.extend(i2.iter().map(|i| i + 8));
        let (positions, normals, indices) = flat_shaded(&positions, &indices);
        let parts = Layer {
            name: "Class 2".into(),
            positions,
            normals,
            indices,
            elements: vec![(5701, 0, 36), (5726, 36, 36)],
            props: vec![
                (5701, r#"{"Name":"KOLOM","Type":"Column","Profile":"HEA300","Material":"S355J2","Class":"2","Tekla id":5701,"IFC GlobalId":"0jkHcwGzv0G8EA3Z1pFnsL"}"#.into()),
                (5726, r#"{"Name":"LIGGER","Type":"Beam","Profile":"IPE300","Material":"S355J2","Class":"3","Tekla id":5726,"IFC GlobalId":"0jkHcwGzv0G8EA3Z1pFnsL"}"#.into()),
            ],
            ..Layer::default()
        };
        let mut m = Model::new(PathBuf::from("hal.db1"), "Tekla Structures", vec![parts]);
        m.project.levels = vec![("+0".into(), 0.0), ("+4200".into(), 4.2)];
        m
    }

    #[test]
    fn tekla_parts_keep_their_own_global_ids_on_their_levels() {
        let s = ifc(&tekla_model());
        let column = s.lines().find(|l| l.contains("=IFCCOLUMN(")).unwrap();
        assert!(column.contains("('0jkHcwGzv0G8EA3Z1pFnsL',$,'KOLOM',$,'HEA300',") && column.ends_with(",'5701',.COLUMN.);"), "{column}");
        // a GlobalId twice: the second element gets one of its own
        let beam = s.lines().find(|l| l.contains("=IFCBEAM(")).unwrap();
        assert!(!beam.contains("0jkHcwGzv0G8EA3Z1pFnsL") && beam.ends_with(",'5726',.BEAM.);"), "{beam}");
        assert_eq!(s.matches("=IFCBUILDINGSTOREY(").count(), 2);
        assert_eq!(own_global_id(&[("IFC GlobalId".into(), "not a guid".into())]), None);
    }

    #[test]
    fn levels_added_to_the_document_become_storeys_in_order() {
        let model = revit_model();
        let mut document = Document::from_model(&model);
        document.add_level("Roof", 6.0).unwrap();
        document.add_level("Foundation", -1.5).unwrap();
        let mut out = Vec::new();
        write(&model, &document, "test.ifc", &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        let storeys: Vec<&str> = s.lines().filter(|l| l.contains("=IFCBUILDINGSTOREY(")).collect();
        assert_eq!(storeys.len(), 4, "{storeys:?}");
        for (line, (name, elevation)) in storeys.iter().zip([("Foundation", -1.5), ("Level 1", 0.0), ("Level 2", 3.0), ("Roof", 6.0)]) {
            let written = line.rsplit(".ELEMENT.,").next().and_then(|v| v.strip_suffix(");")).and_then(|v| v.parse::<f64>().ok());
            assert!(line.contains(&format!(",'{name}',")) && written == Some(elevation), "{line}");
        }
    }

    #[test]
    fn revit_walls_become_ifc_walls_on_their_storeys() {
        let s = ifc(&revit_model());
        assert_eq!(s.matches("=IFCWALL(").count(), 2);
        assert_eq!(s.matches("=IFCBUILDINGSTOREY(").count(), 2);
        assert!(s.contains("'Basic Wall:Ext 300',$,'Ext 300',#") && s.contains(",'101',.NOTDEFINED.)"));
        // the second wall stands on level 2 (z = 3): placed relative to it at z 0
        assert_eq!(s.matches("=IFCRELCONTAINEDINSPATIALSTRUCTURE(").count(), 2);
        assert!(s.contains("IFCCARTESIANPOINT((3.,0.,0.))"));
        // closed cubes, 12 triangles of 8 welded points each
        assert_eq!(s.matches(",.T.,((").count(), 2);
        // one material set for the first wall, one material for the second
        assert!(s.contains("IFCMATERIALCONSTITUENTSET('Brick, Concrete'"));
        assert_eq!(s.matches("=IFCMATERIAL(").count(), 3);
        assert!(s.contains("IFCPROJECT(") && s.contains("'Caf\\X2\\00E9\\X0\\ ''t Hoekje'"));
        assert!(s.contains("IFCPRESENTATIONLAYERASSIGNMENT('Walls'"));
    }

    #[test]
    fn archicad_elements_keep_their_class() {
        let (positions, indices) = cube([0.0, 0.0, 0.0]);
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices: [indices.clone(), indices.clone(), indices.clone(), indices.clone(), indices.clone(), indices].concat(),
            elements: vec![(1, 0, 36), (2, 36, 36), (3, 72, 36), (4, 108, 36), (5, 144, 36), (6, 180, 36)],
            props: vec![
                (1, r#"{"Name":"W-001","category":"Wall","level":"Ground Floor"}"#.into()),
                (2, r#"{"Name":"Slab","category":"Slab","level":"Ground Floor"}"#.into()),
                (3, r#"{"Name":"CRE - 001","category":"Column","level":"Ground Floor"}"#.into()),
                (4, r#"{"Name":"BMR - 001","category":"Beam","level":"Ground Floor"}"#.into()),
                (5, r#"{"Name":"RT - 002","category":"Roof","level":"Ground Floor"}"#.into()),
                (6, r#"{"Name":"Sofa Layout 26.gsm","category":"Object","level":"Ground Floor"}"#.into()),
            ],
            ..Layer::default()
        };
        let mut m = Model::new(PathBuf::from("house.pln"), "Archicad", vec![layer]);
        m.project.levels = vec![("Ground Floor".into(), 0.0)];
        let s = ifc(&m);
        assert_eq!(s.matches("=IFCWALL(").count(), 1);
        assert!(s.contains("'W-001'") && s.contains(".FLOOR.)"));
        assert_eq!(s.matches("=IFCSLAB(").count(), 2);
        assert!(s.contains("'RT - 002'") && s.contains(".ROOF.)"));
        assert_eq!(s.matches("=IFCCOLUMN(").count(), 1);
        assert_eq!(s.matches("=IFCBEAM(").count(), 1);
        assert!(s.contains("'CRE - 001'") && s.contains(".COLUMN.)") && s.contains(".BEAM.)"));
        // a library object is a box of any kind (furniture, kitchen, text, plant): a proxy
        assert_eq!(s.matches("=IFCBUILDINGELEMENTPROXY(").count(), 1);
        assert!(s.contains("'Sofa Layout 26.gsm'"));
    }

    #[test]
    fn archicad_meshes_are_terrain_and_zones_spaces_of_their_storey() {
        let (positions, indices) = cube([0.0, 0.0, 0.0]);
        let layer = |name: &str, id: u64, props: &str| Layer {
            name: name.into(),
            positions: positions.clone(),
            indices: indices.clone(),
            elements: vec![(id, 0, 36)],
            props: vec![(id, props.into())],
            ..Layer::default()
        };
        let mut m = Model::new(
            PathBuf::from("house.pln"),
            "Archicad",
            vec![
                layer("Terrain", 1, r#"{"Name":"Ter 0.00","category":"Mesh","level":"Ground Floor"}"#),
                layer("Zones", 2, r#"{"Name":"01 Living Room","category":"Zone","level":"Ground Floor"}"#),
            ],
        );
        m.project.levels = vec![("Ground Floor".into(), 0.0)];
        let s = ifc(&m);
        assert!(s.contains("=IFCGEOGRAPHICELEMENT(") && s.contains(".TERRAIN.)"));
        let space = s.lines().find(|l| l.contains("=IFCSPACE(")).expect("a space");
        assert!(space.contains("'01 Living Room'") && space.ends_with(",.ELEMENT.,.SPACE.,$);"), "{space}");
        // the space is aggregated by the storey, and not in its containment
        let id = space.split('=').next().unwrap();
        let storey = s.lines().find(|l| l.contains("=IFCBUILDINGSTOREY(")).unwrap().split('=').next().unwrap();
        assert!(s.lines().any(|l| l.contains("=IFCRELAGGREGATES(") && l.contains(&format!(",{storey},({id})"))), "{s}");
        assert!(!s.lines().any(|l| l.contains("=IFCRELCONTAINEDINSPATIALSTRUCTURE(") && l.contains(&format!("{id})")) || l.contains(&format!("{id},"))));
    }

    #[test]
    fn archicad_openings_stay_out_of_the_export() {
        let (positions, indices) = cube([0.0, 0.0, 0.0]);
        let wall = Layer {
            name: "Walls".into(),
            positions: positions.clone(),
            indices: indices.clone(),
            elements: vec![(1, 0, 36)],
            props: vec![(1, r#"{"Name":"SW - 002","category":"Wall"}"#.into())],
            ..Layer::default()
        };
        let opening = Layer {
            name: "Openings".into(),
            positions,
            indices,
            elements: vec![(2, 0, 36)],
            props: vec![(2, r#"{"Name":"DOO - 001","category":"Opening","Type":"Door"}"#.into())],
            hidden: true,
            ..Layer::default()
        };
        let s = ifc(&Model::new(PathBuf::from("house.pln"), "Archicad", vec![wall, opening]));
        assert_eq!(s.matches("=IFCWALL(").count(), 1);
        assert!(!s.contains("DOO - 001") && !s.contains("IFCBUILDINGELEMENTPROXY(") && !s.contains("IFCOPENINGELEMENT("));
    }

    #[test]
    fn global_ids_are_stable_and_unique() {
        let a = ifc(&revit_model());
        let b = ifc(&revit_model());
        let strip = |s: &str| s.lines().filter(|l| !l.starts_with("FILE_NAME")).collect::<Vec<_>>().join("\n");
        assert_eq!(strip(&a), strip(&b));
        let g = Guids { seed: "x.ifc".into() };
        let all: std::collections::HashSet<String> = (0..10_000).map(|i| g.get(&format!("element/{i}"))).collect();
        assert_eq!(all.len(), 10_000);
        assert!(all.iter().all(|s| s.len() == 22 && matches!(s.as_bytes()[0], b'0'..=b'3')));
    }

    #[test]
    fn layers_without_elements_are_one_proxy_each() {
        let (p, i) = cube([0.0, 0.0, 0.0]);
        let layer = Layer { name: "Body (part 1/2)".into(), color: [1.0, 0.0, 0.0], positions: p, indices: i, ..Layer::default() };
        let m = Model::new(PathBuf::from("cube.stl"), "STL", vec![layer]);
        let s = ifc(&m);
        assert_eq!(s.matches("=IFCBUILDINGELEMENTPROXY(").count(), 1);
        assert!(s.contains("'Body'") && s.contains("IFCCOLOURRGB($,1.,0.,0.)"));
        assert!(s.contains("IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)"));
    }

    #[test]
    fn large_unitless_models_are_millimetres() {
        let (p, i) = cube([0.0, 0.0, 0.0]);
        let p = p.iter().map(|v| v * 2000.0).collect();
        let m = Model::new(PathBuf::from("part.stl"), "STL", vec![Layer { name: "part".into(), positions: p, indices: i, ..Layer::default() }]);
        assert!(ifc(&m).contains("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)"));
    }

    #[test]
    fn degenerate_triangles_are_dropped() {
        let mut s = FaceSet::default();
        s.triangle([[0, 0, 0], [1, 0, 0], [2, 0, 0]]);
        s.triangle([[0, 0, 0], [0, 0, 0], [0, 1, 0]]);
        s.triangle([[0, 0, 0], [1, 0, 0], [0, 1, 0]]);
        assert_eq!(s.triangles.len(), 1);
        assert!(!s.closed());
    }

    #[test]
    fn step_values() {
        assert_eq!(fixed(0, 5), "0.");
        assert_eq!(fixed(1250, 2), "12.5");
        assert_eq!(fixed(-5, 5), "-0.00005");
        assert_eq!(fixed(-300_000, 5), "-3.");
        assert_eq!(real(0.5), "0.5");
        assert_eq!(text("a'b\\c"), "'a''b\\\\c'");
        assert_eq!(text("m²"), "'m\\X2\\00B2\\X0\\'");
        assert_eq!(compress_guid(0), "0000000000000000000000");
        assert_eq!(compress_guid(u128::MAX), "3$$$$$$$$$$$$$$$$$$$$$");
        assert_eq!(base_layer_name("Walls (part 2/3)"), "Walls");
        assert_eq!(timestamp().len(), 19);
    }
}
