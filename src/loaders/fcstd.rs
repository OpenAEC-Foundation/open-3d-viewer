//! FreeCAD documents (.FCStd), read natively: the visible objects with a shape or a mesh, each a
//! layer in its colour.
//!
//! An `.FCStd` is a zip: `Document.xml` lists the objects and their properties, `GuiDocument.xml`
//! how each is shown, and beside them one file per shape (`*.brp`, the text form of an exact
//! boundary representation) or mesh (`*.bms`). Read here:
//! - an object's label, its shape or mesh file, and the containers it stands in (`App::Part` and
//!   the like: their `Group` and `Placement`); a shape already holds its own placement;
//! - whether it is visible, and its colour (`ShapeAppearance`, or `ShapeColor` in older files);
//! - a shape ([`brep`]): its faces are triangulated here, on planes, cylinders, cones, spheres,
//!   tori, extrusions, revolutions and Bézier and B-spline surfaces, trimmed by their edges;
//! - a mesh: its points and facets.
//!
//! Left out: shapes saved in binary form (a FreeCAD preference), wires and points without faces,
//! sketches, dimensions and drawings (TechDraw), textures, linked documents. Millimetres to
//! metres; FreeCAD's Z up is the model's.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{DMat4, DQuat, DVec3};
use std::collections::HashMap;
use std::io::Read;

#[path = "fcstd/brep.rs"]
mod brep;

pub struct FcstdLoader;

impl Loader for FcstdLoader {
    fn name(&self) -> &'static str {
        "FreeCAD"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["fcstd"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data[..])).map_err(|e| format!("FreeCAD: not an .FCStd file: {e}"))?;
        let document = entry(&mut zip, "Document.xml").ok_or("FreeCAD: the file has no Document.xml")?;
        let gui = entry(&mut zip, "GuiDocument.xml");
        let objects = objects(&String::from_utf8_lossy(&document))?;
        let looks = gui.map(|g| looks(&String::from_utf8_lossy(&g))).unwrap_or_default();
        let mut layers = Vec::new();
        let mut binary = 0;
        for o in objects.iter() {
            let look = looks.get(&o.name);
            // what holds other objects has their shapes as its own: they are drawn themselves
            if look.is_some_and(|l| !l.visible) || !o.group.is_empty() {
                continue;
            }
            let color = look.and_then(|l| l.color.or_else(|| rgb(&entry(&mut zip, l.appearance.as_deref()?)?))).unwrap_or(DEFAULT_COLOR);
            let place = placement_of(&objects, o);
            let mut layer = Layer { name: o.label.clone(), color, ..Layer::default() };
            if let Some(file) = &o.shape {
                if file.ends_with(".bin") {
                    binary += 1;
                    continue;
                }
                let Some(text) = entry(&mut zip, file) else { continue };
                brep::triangulate(&String::from_utf8_lossy(&text), &place, &mut layer);
            } else if let Some(file) = &o.mesh {
                let Some(raw) = entry(&mut zip, file) else { continue };
                mesh(&raw, &(place * o.placement), &mut layer);
            }
            if layer.indices.is_empty() {
                continue;
            }
            let id = layers.len() as u64 + 1;
            layer.elements = vec![(id, 0, layer.indices.len() as u32)];
            layer.props = vec![(id, serde_json::json!({"Name": o.label, "Object": o.name, "Type": o.kind}).to_string())];
            layers.push(layer);
        }
        if layers.is_empty() {
            return Err(if binary > 0 { "FreeCAD: the shapes of this file are saved in binary form, which is not read".into() } else { "FreeCAD: the file has no visible shapes or meshes".into() });
        }
        Ok(layers)
    }
}

/// A file of the document.
fn entry(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut file = zip.by_name(name).ok()?;
    let mut out = Vec::new();
    // a shape of a hundred megabytes is no shape
    file.by_ref().take(1 << 30).read_to_end(&mut out).ok()?;
    Some(out)
}

/// An object of the document.
#[derive(Debug, Default)]
struct Object {
    name: String,
    kind: String,
    label: String,
    shape: Option<String>,
    mesh: Option<String>,
    placement: DMat4,
    /// The objects it holds, when it places them (an `App::Part`).
    group: Vec<String>,
    places: bool,
}

/// The objects of `Document.xml`, in the order of the file.
fn objects(xml: &str) -> Result<Vec<Object>, String> {
    let doc = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("FreeCAD: cannot read Document.xml: {e}"))?;
    let root = doc.root_element();
    fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
        n.children().find(|c| c.has_tag_name(tag))
    }
    let kinds: HashMap<&str, &str> = child(root, "Objects").into_iter().flat_map(|o| o.children()).filter(|o| o.has_tag_name("Object")).filter_map(|o| Some((o.attribute("name")?, o.attribute("type")?))).collect();
    let mut out = Vec::new();
    for o in child(root, "ObjectData").into_iter().flat_map(|d| d.children()).filter(|o| o.has_tag_name("Object")) {
        let Some(name) = o.attribute("name") else { continue };
        let kind = kinds.get(name).copied().unwrap_or_default();
        let mut object = Object { name: name.into(), kind: kind.into(), label: name.into(), placement: DMat4::IDENTITY, ..Object::default() };
        // only a container with a frame of its own places what it holds
        object.places = matches!(kind, "App::Part" | "PartDesign::Body" | "Assembly::AssemblyObject" | "App::LinkGroup");
        for p in child(o, "Properties").into_iter().flat_map(|p| p.children()).filter(|p| p.has_tag_name("Property")) {
            let Some(value) = p.first_element_child() else { continue };
            match p.attribute("name") {
                Some("Label") => object.label = value.attribute("value").filter(|l| !l.is_empty()).unwrap_or(name).into(),
                Some("Shape") => object.shape = value.attribute("file").filter(|f| !f.is_empty()).map(str::to_string),
                Some("Mesh") => object.mesh = value.attribute("file").filter(|f| !f.is_empty()).map(str::to_string),
                Some("Placement") => object.placement = placement(value),
                Some("Group") => object.group = value.children().filter_map(|l| l.attribute("value")).filter(|l| !l.is_empty()).map(str::to_string).collect(),
                _ => {}
            }
        }
        out.push(object);
    }
    Ok(out)
}

/// A placement: a position (millimetres) and a rotation (a quaternion x, y, z, w).
fn placement(p: roxmltree::Node) -> DMat4 {
    let n = |name: &str, or: f64| p.attribute(name).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(or);
    let q = DQuat::from_xyzw(n("Q0", 0.0), n("Q1", 0.0), n("Q2", 0.0), n("Q3", 1.0));
    let q = if q.length_squared() > 1e-18 { q.normalize() } else { DQuat::IDENTITY };
    DMat4::from_rotation_translation(q, DVec3::new(n("Px", 0.0), n("Py", 0.0), n("Pz", 0.0)))
}

/// Where the containers of an object place it: theirs, outermost first.
fn placement_of(objects: &[Object], object: &Object) -> DMat4 {
    let mut place = DMat4::IDENTITY;
    let mut name = object.name.as_str();
    for _ in 0..32 {
        let Some(holder) = objects.iter().find(|o| o.places && o.group.iter().any(|g| g == name)) else { break };
        place = holder.placement * place;
        name = &holder.name;
    }
    place
}

/// How an object is shown.
#[derive(Debug, Default)]
struct Look {
    visible: bool,
    color: Option<[f32; 3]>,
    /// The file of its materials.
    appearance: Option<String>,
}

/// The looks of `GuiDocument.xml`, by object.
fn looks(xml: &str) -> HashMap<String, Look> {
    let Ok(doc) = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) else { return HashMap::new() };
    let mut out = HashMap::new();
    for v in doc.descendants().filter(|n| n.has_tag_name("ViewProvider")) {
        let Some(name) = v.attribute("name") else { continue };
        let mut look = Look { visible: true, ..Look::default() };
        for p in v.descendants().filter(|p| p.has_tag_name("Property")) {
            let Some(value) = p.first_element_child() else { continue };
            match p.attribute("name") {
                Some("Visibility") => look.visible = value.attribute("value") != Some("false"),
                // red, green, blue and alpha packed in one number
                Some("ShapeColor") => look.color = value.attribute("value").and_then(|c| c.parse::<u32>().ok()).map(|c| [(c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8].map(|x| x as f32 / 255.0)),
                Some("ShapeAppearance") => look.appearance = value.attribute("file").filter(|f| !f.is_empty()).map(str::to_string),
                _ => {}
            }
        }
        out.insert(name.to_string(), look);
    }
    out
}

/// The diffuse colour of the first material of a material list: a count, then per material its
/// ambient, diffuse, specular and emissive colours, each packed in one number.
fn rgb(materials: &[u8]) -> Option<[f32; 3]> {
    let c = materials.get(8..12)?;
    Some([c[3], c[2], c[1]].map(|x| x as f32 / 255.0))
}

/// A mesh kernel: a header of 264 bytes, the counts of points and facets, the points and per
/// facet its three points and three neighbours.
fn mesh(raw: &[u8], place: &DMat4, layer: &mut Layer) {
    let u32_at = |at: usize| raw.get(at..at + 4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize);
    let f32_at = |at: usize| raw.get(at..at + 4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64);
    let (Some(points), Some(facets)) = (u32_at(264), u32_at(268)) else { return };
    let point = |k: usize| Some(place.transform_point3(DVec3::new(f32_at(272 + k * 12)?, f32_at(276 + k * 12)?, f32_at(280 + k * 12)?)) * 0.001);
    let first = 272 + points * 12;
    for f in 0..facets {
        let corner = |k: usize| u32_at(first + f * 24 + k * 4).filter(|v| *v < points).and_then(point);
        let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else { continue };
        let Some(normal) = (b - a).cross(c - a).try_normalize() else { continue };
        let base = (layer.positions.len() / 3) as u32;
        for p in [a, b, c] {
            layer.positions.extend(p.to_array().map(|x| x as f32));
            layer.normals.extend(normal.to_array().map(|x| x as f32));
        }
        layer.indices.extend([base, base + 1, base + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The volume a layer's triangles enclose (cubic metres).
    fn volume(layer: &Layer) -> f64 {
        let p = |i: u32| DVec3::new(layer.positions[i as usize * 3] as f64, layer.positions[i as usize * 3 + 1] as f64, layer.positions[i as usize * 3 + 2] as f64);
        layer.indices.chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum()
    }

    /// FreeCAD documents (`FCSTD_FILES`: a folder of `.FCStd` files, read-only; skipped when
    /// absent), with `expected.txt` beside them when the folder has one: per line an object's
    /// name, type, label, volume (mm³) and bounding box (mm).
    #[test]
    fn documents_give_their_visible_shapes_with_their_volumes() {
        let Some(folder) = std::env::var_os("FCSTD_FILES").map(std::path::PathBuf::from).filter(|p| p.is_dir()) else {
            eprintln!("skipped: set FCSTD_FILES to a folder of .FCStd files");
            return;
        };
        // label -> volume (m³), lower and upper corner (m)
        let mut expected: HashMap<String, Vec<(f64, [f64; 3], [f64; 3])>> = HashMap::new();
        for line in std::fs::read_to_string(folder.join("expected.txt")).unwrap_or_default().lines() {
            let (Some(label), Some(volume), Some(bbox)) = (line.split('\'').nth(1), line.split("volume=").nth(1), line.split("bbox=").nth(1)) else { continue };
            let volume: f64 = volume.split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let n: Vec<f64> = bbox.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter_map(|x| x.parse().ok()).collect();
            if n.len() == 6 {
                expected.entry(label.to_string()).or_default().push((volume * 1e-9, [n[0], n[1], n[2]].map(|x| x * 1e-3), [n[3], n[4], n[5]].map(|x| x * 1e-3)));
            }
        }
        let mut read = 0;
        for file in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("fcstd")) {
                continue;
            }
            let layers = match FcstdLoader.load(&Source::from(path.as_path())) {
                Ok(layers) => layers,
                Err(e) => {
                    eprintln!("{:?}: {e}", path.file_name().unwrap());
                    assert!(e.contains("binary"), "{path:?}: {e}");
                    continue;
                }
            };
            eprintln!("{:?}: {} layers", path.file_name().unwrap(), layers.len());
            for l in &layers {
                let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
                for p in l.positions.chunks_exact(3) {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k] as f64);
                        hi[k] = hi[k].max(p[k] as f64);
                    }
                }
                let v = volume(l);
                eprintln!("  {:?}: {} triangles, volume {v:.4} m³, {lo:.3?} to {hi:.3?}, colour {:?}", l.name, l.triangle_count(), l.color);
                // two documents may name an object alike: one of them is this one
                let Some(wanted) = expected.get(&l.name) else { continue };
                let fits = |(want, want_lo, want_hi): &(f64, [f64; 3], [f64; 3])| (0..3).all(|k| (lo[k] - want_lo[k]).abs() < 0.005 && (hi[k] - want_hi[k]).abs() < 0.005) && (*want <= 0.0 || (v - want).abs() < want * 0.03);
                assert!(wanted.iter().any(fits), "{:?}: volume {v}, box {lo:?} to {hi:?}, not {wanted:?}", l.name);
            }
            read += 1;
        }
        assert!(read > 0, "no documents read in {folder:?}");
    }
}

