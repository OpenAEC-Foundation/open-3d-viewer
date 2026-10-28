//! IFC 4 (ISO 16739-1:2018, ADD2 TC1) as a STEP physical file (ISO 10303-21), for every model the
//! viewer opens.
//!
//! - Spatial structure: project, site, building and one storey per level of the model (Revit),
//!   else one storey; each element is contained in its level, or the storey it stands on.
//! - One IFC element per element of the model (Revit elements, Tekla parts, SketchUp components,
//!   glTF nodes) and one per layer for geometry outside elements (OBJ objects, STL, PLY). The
//!   class follows the Revit category or the Tekla part name (IfcWall, IfcSlab, IfcBeam, ...);
//!   anything else is an IfcBuildingElementProxy.
//! - Body: one IfcTriangulatedFaceSet per colour, relative to the element's placement, welded,
//!   without degenerate triangles, `Closed` when every edge joins two triangles, styled with one
//!   shared IfcSurfaceStyle per colour.
//! - The element's properties as one property set named after the source format, its materials
//!   as IfcMaterial (one) or IfcMaterialConstituentSet (several), the model's layers as
//!   IfcPresentationLayerAssignment.
//! - GlobalIds are derived from the file name and the element, so saving again gives the same ids.
//! - Units: metres. OBJ, STL and PLY carry no unit; such a model larger than 500 units is taken to
//!   be in millimetres.

use crate::model::Model;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;

pub struct IfcWriter;

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

    fn write(&self, model: &Model, file_name: &str, mut out: &mut dyn Write) -> io::Result<()> {
        write(model, file_name, &mut out)
    }
}

/// Length unit and coordinate resolution of the file.
#[derive(Clone, Copy)]
struct Units {
    millimetres: bool,
    /// Decimals written for coordinates (0.01 mm).
    decimals: u32,
    /// One millimetre in quantised coordinates.
    mm: i64,
}

impl Units {
    fn of(model: &Model) -> Self {
        let unitless = matches!(model.format.as_str(), "OBJ" | "STL" | "PLY");
        if unitless && model.bounds.is_valid() && model.bounds.size().iter().any(|&s| s > 500.0) {
            Units { millimetres: true, decimals: 2, mm: 100 }
        } else {
            Units { millimetres: false, decimals: 5, mm: 100 }
        }
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
    let mut push = |key: Key, range: (usize, u32, u32), layer: &str| {
        if range.2 < 3 {
            return;
        }
        let i = *at.entry(key.clone()).or_insert_with(|| {
            out.push(Product { key, ranges: Vec::new(), layer: layer.to_string() });
            out.len() - 1
        });
        out[i].ranges.push(range);
    };
    for (li, layer) in model.layers.iter().enumerate() {
        let name = base_layer_name(&layer.name);
        let loose = Key::Layer(name.to_string());
        let mut covered = 0u32;
        for &(id, first, count) in &layer.elements {
            if first > covered {
                push(loose.clone(), (li, covered, first - covered), name);
            }
            push(Key::Element(id), (li, first, count), name);
            covered = covered.max(first + count);
        }
        let len = layer.indices.len() as u32;
        if len > covered {
            push(loose, (li, covered, len - covered), name);
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
    min_z: f64,
}

fn body(model: &Model, product: &Product, units: Units) -> Option<Body> {
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
            let colour = match &layer.colors {
                Some(c) => c.get(t[0] as usize).copied().unwrap_or([base[0], base[1], base[2], 255]),
                None => [base[0], base[1], base[2], 255],
            };
            tris.push(([corners[0], corners[1], corners[2]], colour));
        }
    }
    let mut min = [i64::MAX; 3];
    for (t, _) in &tris {
        for p in t {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
            }
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
    let min_z = min[2] as f64 / 10f64.powi(units.decimals as i32);
    (!sets.is_empty()).then_some(Body { origin, sets, min_z })
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

/// The IFC class for a Tekla part, from its name (BEAM, COLUMN, ...), type and profile.
fn tekla_class(name: &str, kind: &str, profile: &str) -> &'static str {
    let name = name.to_uppercase();
    if name.contains("COLUMN") {
        "IFCCOLUMN"
    } else if name.contains("PLATE") || kind == "Contour plate" || profile.to_uppercase().starts_with("PL") {
        "IFCPLATE"
    } else if name.contains("BRAC") {
        "IFCMEMBER"
    } else if ["BEAM", "RAFTER", "PURLIN", "GIRDER", "JOIST"].iter().any(|k| name.contains(k)) {
        "IFCBEAM"
    } else {
        "IFCMEMBER"
    }
}

/// Name, object type, tag, IFC class and predefined type of an element.
struct Identity {
    name: String,
    object_type: Option<String>,
    tag: Option<String>,
    class: &'static str,
    predefined: &'static str,
}

fn identity(format: &str, key: &Key, layer: &str, props: &[(String, String)]) -> Identity {
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let category = get("category").unwrap_or(layer);
    let (class, predefined) = match format {
        "Revit" => revit_class(category).unwrap_or(("IFCBUILDINGELEMENTPROXY", "NOTDEFINED")),
        "Tekla Structures" if matches!(key, Key::Element(_)) => (
            tekla_class(get("Name").unwrap_or(""), get("Type").unwrap_or(""), get("Profile").unwrap_or("")),
            "NOTDEFINED",
        ),
        _ => ("IFCBUILDINGELEMENTPROXY", "NOTDEFINED"),
    };
    let name = match (get("family"), get("type")) {
        (Some(f), Some(t)) => format!("{f}:{t}"),
        (None, Some(t)) => t.to_string(),
        _ => get("Name").or(get("Definition")).unwrap_or(category).to_string(),
    };
    let object_type = ["type", "Profile", "Definition", "Mesh"].iter().find_map(|k| get(k)).map(str::to_string);
    let tag = ["id", "Tekla id"].iter().find_map(|k| get(k)).map(str::to_string);
    Identity { name, object_type, tag, class, predefined }
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

/// Writes the model as an IFC 4 file named `file_name`.
pub fn write(model: &Model, file_name: &str, out: &mut impl Write) -> io::Result<()> {
    let units = Units::of(model);
    let app = format!("Open 3D Viewer Studio {}", env!("CARGO_PKG_VERSION"));
    let ids = Guids { seed: file_name.to_string() };

    writeln!(out, "ISO-10303-21;")?;
    writeln!(out, "HEADER;")?;
    writeln!(out, "FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');")?;
    writeln!(out, "FILE_NAME({},{},(''),(''),{},{},'');", text(file_name), text(&timestamp()), text(&app), text(&app))?;
    writeln!(out, "FILE_SCHEMA(('IFC4'));")?;
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
    let precision = if units.millimetres { "1.E-02" } else { "1.E-05" };
    let context = f.add(format!("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,{precision},#{world},#{north})"))?;
    let body_context = f.add(format!("IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,#{context},$,.MODEL_VIEW.,$)"))?;

    // project, site, building
    let info = |k: &str| model.project.info.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let project_name = info("name").map(str::to_string).unwrap_or(stem);
    let project = f.add(format!(
        "IFCPROJECT({},$,{},$,$,{},$,(#{context}),#{unit_assignment})",
        text(&ids.get("project")),
        text(&project_name),
        opt(info("number")),
    ))?;
    let project_props: Vec<(String, String)> = model.project.info.iter().filter(|(_, v)| !v.is_empty()).cloned().collect();
    property_set(&mut f, &ids, "project", "Project information", &project_props, project)?;

    let site_placement = f.add(format!("IFCLOCALPLACEMENT($,#{world})"))?;
    let site = f.add(format!("IFCSITE({},$,'Site',$,$,#{site_placement},$,$,.ELEMENT.,$,$,$,$,$)", text(&ids.get("site"))))?;
    let building_placement = f.add(format!("IFCLOCALPLACEMENT(#{site_placement},#{world})"))?;
    let building = f.add(format!(
        "IFCBUILDING({},$,'Building',$,$,#{building_placement},$,$,.ELEMENT.,$,$,$)",
        text(&ids.get("building"))
    ))?;
    f.add(format!("IFCRELAGGREGATES({},$,$,$,#{project},(#{site}))", text(&ids.get("aggregates/project"))))?;
    f.add(format!("IFCRELAGGREGATES({},$,$,$,#{site},(#{building}))", text(&ids.get("aggregates/site"))))?;

    // storeys: the model's levels (low to high), else one at zero
    let mut levels: Vec<(String, f32)> = model.project.levels.iter().filter(|(_, z)| z.is_finite()).cloned().collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    if levels.is_empty() {
        levels.push(("Level 0".to_string(), 0.0));
    }
    let mut storeys: Vec<(u64, u64, i64)> = Vec::new(); // entity, placement, elevation (quantised)
    for (i, (name, z)) in levels.iter().enumerate() {
        let elevation = units.quantise(*z as f64);
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
    let mut styles: HashMap<[u8; 4], u64> = HashMap::new();
    let mut contained: Vec<Vec<u64>> = vec![Vec::new(); storeys.len()];
    let mut by_material: Vec<(Vec<String>, Vec<u64>)> = Vec::new();
    let mut by_layer: Vec<(String, Vec<u64>)> = Vec::new();
    let mut material_ids: HashMap<String, u64> = HashMap::new();
    for product in products(model) {
        let Some(body) = body(model, &product, units) else { continue };
        let props = match product.key {
            Key::Element(id) => model.element_props(id),
            Key::Layer(_) => Vec::new(),
        };
        let key = match &product.key {
            Key::Element(id) => format!("element/{id}"),
            Key::Layer(name) => format!("layer/{name}"),
        };
        let who = identity(&model.format, &product.key, &product.layer, &props);

        // its storey: the level it names, else the highest level at or below its lowest point
        let level = props.iter().find(|(k, _)| k == "level").map(|(_, v)| v.as_str());
        let storey = level
            .and_then(|l| levels.iter().position(|(n, _)| n == l))
            .unwrap_or_else(|| levels.iter().rposition(|(_, z)| (*z as f64) <= body.min_z + 0.01).unwrap_or(0));
        let (_, storey_placement, elevation) = storeys[storey];

        let mut items = Vec::new();
        for (colour, set) in &body.sets {
            let style = match styles.get(colour) {
                Some(&s) => s,
                None => {
                    let s = surface_style(&mut f, *colour)?;
                    styles.insert(*colour, s);
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
            coords.push_str("))");
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
        let representation = f.add(format!("IFCSHAPEREPRESENTATION(#{body_context},'Body','Tessellation',{})", refs(&items)))?;
        let shape = f.add(format!("IFCPRODUCTDEFINITIONSHAPE($,$,(#{representation}))"))?;
        let at = [body.origin[0], body.origin[1], body.origin[2] - elevation];
        let point = f.add(format!(
            "IFCCARTESIANPOINT(({},{},{}))",
            fixed(at[0], units.decimals),
            fixed(at[1], units.decimals),
            fixed(at[2], units.decimals)
        ))?;
        let axes = f.add(format!("IFCAXIS2PLACEMENT3D(#{point},$,$)"))?;
        let placement = f.add(format!("IFCLOCALPLACEMENT(#{storey_placement},#{axes})"))?;
        let head = format!(
            "{}({},$,{},$,{},#{placement},#{shape},{}",
            who.class,
            text(&ids.get(&key)),
            text(&who.name),
            opt(who.object_type.as_deref()),
            opt(who.tag.as_deref()),
        );
        let element = f.add(match who.class {
            "IFCDOOR" => format!("{head},$,$,.{}.,.NOTDEFINED.,$)", who.predefined),
            "IFCWINDOW" => format!("{head},$,$,.{}.,.NOTDEFINED.,$)", who.predefined),
            "IFCREINFORCINGBAR" => format!("{head},$,$,$,$,.{}.,$)", who.predefined),
            _ => format!("{head},.{}.)", who.predefined),
        })?;
        property_set(&mut f, &ids, &key, &model.format, &props, element)?;

        contained[storey].push(element);
        let mats = materials(&props);
        if !mats.is_empty() {
            match by_material.iter_mut().find(|(m, _)| *m == mats) {
                Some((_, list)) => list.push(element),
                None => by_material.push((mats, vec![element])),
            }
        }
        match by_layer.iter_mut().find(|(l, _)| *l == product.layer) {
            Some((_, list)) => list.push(representation),
            None => by_layer.push((product.layer.clone(), vec![representation])),
        }
    }

    // relations
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
                    let id = f.add(format!("IFCMATERIAL({},$,$)", text(m)))?;
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
    for (name, representations) in &by_layer {
        f.add(format!("IFCPRESENTATIONLAYERASSIGNMENT({},$,{},$)", text(name), refs(representations)))?;
    }

    writeln!(f.out, "ENDSEC;")?;
    writeln!(f.out, "END-ISO-10303-21;")?;
    Ok(())
}

/// A surface style for a colour (sRGB8, alpha 255 = opaque).
fn surface_style(f: &mut Step<impl Write>, c: [u8; 4]) -> io::Result<u64> {
    let ratio = |v: u8| real(v as f64 / 255.0);
    let colour = f.add(format!("IFCCOLOURRGB($,{},{},{})", ratio(c[0]), ratio(c[1]), ratio(c[2])))?;
    let shading = f.add(format!("IFCSURFACESTYLESHADING(#{colour},{})", real(1.0 - c[3] as f64 / 255.0)))?;
    let name = format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    f.add(format!("IFCSURFACESTYLE({},.BOTH.,(#{shading}))", text(&name)))
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
        write(model, "test.ifc", &mut out).unwrap();
        String::from_utf8(out).unwrap()
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
