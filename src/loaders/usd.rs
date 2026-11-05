//! Universal Scene Description: `.usda` (text), `.usdc` (binary crate) and `.usdz` (package),
//! through the pure-Rust `openusd` crate, which composes the stage (sublayers, references,
//! payloads, variants, instancing). Each `Mesh` prim is a pickable element with its name and
//! path, placed with its xform ops (`xformOpOrder`: translate, rotate in any axis order, orient,
//! scale, transform, inverted pivots, `!resetXformStack!`) and those of its parents; one layer
//! per bound material (`material:binding` to a `UsdPreviewSurface` diffuse colour), coloured by
//! `primvars:displayColor` when the mesh has one (constant, per face, per vertex or per face
//! vertex). Polygons are fan triangulated (holes left out), `leftHanded` meshes turned around,
//! flat shaded. `upAxis` and `metersPerUnit` (USD default: Y up, centimetres) convert to metres,
//! Z up. Textures, subdivision, implicit shapes (Cube, Sphere, ..), curves, points, skinning and
//! animation (the default time is drawn) are not read. In the browser only the file itself is
//! read: references to other files on disk are left out (a `.usdz` carries its own).

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{DMat3, DMat4, DQuat, DVec3};
use openusd::{ar, sdf, usd};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub struct UsdLoader;

impl Loader for UsdLoader {
    fn name(&self) -> &'static str {
        "USD"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["usd", "usda", "usdc", "usdz"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let stage = match source {
            Source::File(path) => {
                let path = path.to_str().ok_or("USD: the file path is not valid UTF-8")?;
                usd::Stage::open(path)
            }
            Source::Bytes { name, data } => {
                let resolver = MemoryResolver { name: name.clone(), data: data.clone() };
                usd::Stage::builder().resolver(resolver).open(name)
            }
        }
        .map_err(|e| format!("USD: {e}"))?;
        read(&stage)
    }
}

/// Serves the bytes of a file picked in the browser, and the entries of it when it is a `.usdz`
/// package; nothing else exists.
struct MemoryResolver {
    name: String,
    data: Arc<Vec<u8>>,
}

impl ar::Resolver for MemoryResolver {
    fn create_identifier(&self, asset_path: &str, anchor: Option<&ar::ResolvedPath>) -> String {
        let anchor = anchor.map(|a| a.to_string_lossy().into_owned());
        let Some(anchor) = anchor.filter(|_| !asset_path.contains('[') && !asset_path.starts_with('/')) else {
            return asset_path.to_string();
        };
        // relative to the layer inside the package, or to the package's root
        let (package, dir) = match ar::split_package_relative_path_outer(&anchor) {
            Some((package, inner)) => (package, inner.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_string())),
            None if anchor.to_ascii_lowercase().ends_with(".usdz") => (anchor.clone(), String::new()),
            None => return asset_path.to_string(),
        };
        let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
        for part in asset_path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                p => parts.push(p),
            }
        }
        ar::join_package_relative_path(&package, &parts.join("/"))
    }

    fn resolve(&self, asset_path: &str) -> Option<ar::ResolvedPath> {
        if asset_path == self.name {
            return Some(ar::ResolvedPath::new(asset_path));
        }
        let (package, inner) = ar::split_package_relative_path_outer(asset_path)?;
        (package == self.name && openusd::usdz::package_contains(self, &package, &inner)).then(|| ar::ResolvedPath::new(asset_path))
    }

    fn resolve_for_new_asset(&self, _asset_path: &str) -> Option<ar::ResolvedPath> {
        None
    }

    fn open_asset(&self, resolved_path: &ar::ResolvedPath) -> std::io::Result<Box<dyn ar::Asset>> {
        let path = resolved_path.to_string_lossy();
        if path == self.name {
            return Ok(Box::new(std::io::Cursor::new(self.data.to_vec())));
        }
        let not_found = || std::io::Error::new(std::io::ErrorKind::NotFound, path.to_string());
        let (package, inner) = ar::split_package_relative_path_outer(&path).ok_or_else(not_found)?;
        let entry = openusd::usdz::package_entry(self, &package, &inner).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Box::new(std::io::Cursor::new(entry)))
    }

    fn get_modification_timestamp(&self, _asset_path: &str, _resolved_path: &ar::ResolvedPath) -> Option<std::time::SystemTime> {
        None
    }
}

/// xyz triples of a point, vector or colour array (float, double or half).
fn vec3s(value: &sdf::Value) -> Vec<DVec3> {
    match value {
        sdf::Value::Vec3fVec(v) => v.iter().map(|p| DVec3::new(p.x as f64, p.y as f64, p.z as f64)).collect(),
        sdf::Value::Vec3dVec(v) => v.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect(),
        sdf::Value::Vec3hVec(v) => v.iter().map(|p| DVec3::new(p.x.to_f64(), p.y.to_f64(), p.z.to_f64())).collect(),
        single => vec3(single).into_iter().collect(),
    }
}

fn vec3(value: &sdf::Value) -> Option<DVec3> {
    match value {
        sdf::Value::Vec3f(p) => Some(DVec3::new(p.x as f64, p.y as f64, p.z as f64)),
        sdf::Value::Vec3d(p) => Some(DVec3::new(p.x, p.y, p.z)),
        sdf::Value::Vec3h(p) => Some(DVec3::new(p.x.to_f64(), p.y.to_f64(), p.z.to_f64())),
        _ => None,
    }
}

fn scalar(value: &sdf::Value) -> Option<f64> {
    match value {
        sdf::Value::Float(v) => Some(*v as f64),
        sdf::Value::Double(v) => Some(*v),
        sdf::Value::Half(v) => Some(v.to_f64()),
        sdf::Value::Int(v) => Some(*v as f64),
        _ => None,
    }
}

fn ints(value: &sdf::Value) -> Vec<i64> {
    match value {
        sdf::Value::IntVec(v) => v.iter().map(|&i| i as i64).collect(),
        sdf::Value::Int64Vec(v) => v.clone(),
        sdf::Value::UintVec(v) => v.iter().map(|&i| i as i64).collect(),
        _ => Vec::new(),
    }
}

fn text(value: &sdf::Value) -> Option<String> {
    match value {
        sdf::Value::Token(t) => Some(t.as_str().to_string()),
        sdf::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn tokens(value: &sdf::Value) -> Vec<String> {
    match value {
        sdf::Value::TokenVec(v) => v.iter().map(|t| t.as_str().to_string()).collect(),
        sdf::Value::StringVec(v) => v.clone(),
        _ => Vec::new(),
    }
}

fn attribute(prim: &usd::Prim, name: &str) -> Option<sdf::Value> {
    prim.attribute(name).get::<sdf::Value>().ok().flatten()
}

/// A rotation of Euler angles in degrees, `order` naming the axis applied first (`XYZ` = X first).
fn euler(order: &str, angles: DVec3) -> DMat4 {
    let mut m = DMat4::IDENTITY;
    for (axis, angle) in order.chars().zip([angles.x, angles.y, angles.z]) {
        let r = angle.to_radians();
        let rotation = match axis {
            'X' => DMat4::from_rotation_x(r),
            'Y' => DMat4::from_rotation_y(r),
            _ => DMat4::from_rotation_z(r),
        };
        m = rotation * m;
    }
    m
}

/// The prim's local transform from its xform ops, and whether it resets the parents' transform.
fn local_transform(prim: &usd::Prim) -> (DMat4, bool) {
    let order = attribute(prim, "xformOpOrder").map(|v| tokens(&v)).unwrap_or_default();
    let mut m = DMat4::IDENTITY;
    let mut reset = false;
    for op in &order {
        if op == "!resetXformStack!" {
            reset = true;
            m = DMat4::IDENTITY;
            continue;
        }
        let (inverse, name) = match op.strip_prefix("!invert!") {
            Some(name) => (true, name),
            None => (false, op.as_str()),
        };
        let Some(value) = attribute(prim, name) else { continue };
        // xformOp:<kind>[:suffix]
        let kind = name.strip_prefix("xformOp:").unwrap_or(name).split(':').next().unwrap_or("");
        let op_matrix = match kind {
            "translate" => vec3(&value).map(DMat4::from_translation),
            "scale" => vec3(&value).map(DMat4::from_scale).or_else(|| scalar(&value).map(|s| DMat4::from_scale(DVec3::splat(s)))),
            "rotateX" => scalar(&value).map(|a| DMat4::from_rotation_x(a.to_radians())),
            "rotateY" => scalar(&value).map(|a| DMat4::from_rotation_y(a.to_radians())),
            "rotateZ" => scalar(&value).map(|a| DMat4::from_rotation_z(a.to_radians())),
            k if k.len() == 9 && k.starts_with("rotate") => vec3(&value).map(|a| euler(&k[6..], a)),
            "orient" => match value {
                sdf::Value::Quatf(q) => Some(DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64)),
                sdf::Value::Quatd(q) => Some(DQuat::from_xyzw(q.x, q.y, q.z, q.w)),
                sdf::Value::Quath(q) => Some(DQuat::from_xyzw(q.x.to_f64(), q.y.to_f64(), q.z.to_f64(), q.w.to_f64())),
                _ => None,
            }
            .map(|q| DMat4::from_quat(q.normalize())),
            // row-major with the translation in the last row: read as columns, it is ours
            "transform" => match value {
                sdf::Value::Matrix4d(t) => Some(DMat4::from_cols_array(&t.0)),
                _ => None,
            },
            _ => None,
        };
        let Some(op_matrix) = op_matrix else { continue };
        m *= if inverse { op_matrix.inverse() } else { op_matrix };
    }
    (m, reset)
}

struct Scene<'s> {
    stage: &'s usd::Stage,
    /// World transforms by prim path.
    worlds: HashMap<String, DMat4>,
    /// Material path to its name and diffuse colour.
    materials: HashMap<String, (String, [f32; 3])>,
    layers: BTreeMap<String, Layer>,
    root: DMat4,
}

impl Scene<'_> {
    fn world(&mut self, prim: &usd::Prim, depth: usize) -> DMat4 {
        let key = prim.path().to_string();
        if let Some(m) = self.worlds.get(&key) {
            return *m;
        }
        let (local, reset) = local_transform(prim);
        let parent = match prim.parent() {
            Some(parent) if !reset && depth < 512 && !parent.path().is_abs_root() => self.world(&parent, depth + 1),
            _ => self.root,
        };
        let m = parent * local;
        self.worlds.insert(key, m);
        m
    }

    /// The material bound to the prim or its nearest ancestor: name and diffuse colour.
    fn material(&mut self, prim: &usd::Prim) -> Option<(String, [f32; 3])> {
        let mut at = Some(prim.clone());
        let mut target = None;
        while let Some(p) = at {
            if let Some(t) = p.relationship("material:binding").targets().ok().and_then(|t| t.into_iter().next()) {
                target = Some(t);
                break;
            }
            at = p.parent().filter(|p| !p.path().is_abs_root());
        }
        let target = target?;
        let key = target.to_string();
        if let Some(m) = self.materials.get(&key) {
            return Some(m.clone());
        }
        let name = target.name().unwrap_or("Material").to_string();
        let material = self.stage.prim(target.clone()).ok()?;
        // the UsdPreviewSurface (or any shader) under the material with a diffuse colour
        let mut color = None;
        let mut stack = material.children().unwrap_or_default();
        while let Some(child) = stack.pop() {
            if let Some(c) = attribute(&child, "inputs:diffuseColor").as_ref().and_then(vec3) {
                color = Some(c.to_array().map(|v| v.clamp(0.0, 1.0) as f32));
                break;
            }
            stack.extend(child.children().unwrap_or_default());
        }
        let entry = (name, color.unwrap_or(DEFAULT_COLOR));
        self.materials.insert(key, entry.clone());
        Some(entry)
    }

    fn mesh(&mut self, prim: &usd::Prim, id: u64) {
        let points = attribute(prim, "points").map(|v| vec3s(&v)).unwrap_or_default();
        let counts = attribute(prim, "faceVertexCounts").map(|v| ints(&v)).unwrap_or_default();
        let indices = attribute(prim, "faceVertexIndices").map(|v| ints(&v)).unwrap_or_default();
        if points.is_empty() || counts.is_empty() {
            return;
        }
        let left_handed = attribute(prim, "orientation").and_then(|v| text(&v)).as_deref() == Some("leftHanded");
        let world = self.world(prim, 0);
        let flip = left_handed != (world.determinant() < 0.0);
        let colors = attribute(prim, "primvars:displayColor").map(|v| vec3s(&v)).unwrap_or_default();
        let interpolation = prim.attribute("primvars:displayColor").get_metadata::<sdf::Value>("interpolation").ok().flatten().and_then(|v| text(&v));
        let material = self.material(prim);
        let (layer_name, base) = material.unwrap_or_else(|| ("Default".into(), DEFAULT_COLOR));
        let base_rgba = {
            let c = colors.first().map(|c| c.to_array().map(|v| v as f32)).unwrap_or(base).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            [c[0], c[1], c[2], 255]
        };
        let to_rgba = |c: DVec3| {
            let c = c.to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            [c[0], c[1], c[2], 255]
        };
        // per face, per vertex or per face vertex (from the metadata, else from the count)
        let kind = interpolation.unwrap_or_else(|| {
            match colors.len() {
                n if n == counts.len() && n > 1 => "uniform",
                n if n == points.len() && n > 1 => "vertex",
                n if n == indices.len() && n > 1 => "faceVarying",
                _ => "constant",
            }
            .to_string()
        });
        let layer = self.layers.entry(layer_name.clone()).or_insert_with(|| Layer {
            name: layer_name.clone(),
            color: if colors.is_empty() { base } else { [base_rgba[0], base_rgba[1], base_rgba[2]].map(|v| v as f32 / 255.0) },
            colors: Some(Vec::new()),
            ..Layer::default()
        });
        let first = layer.indices.len() as u32;
        let mut start = 0usize;
        for (face, &count) in counts.iter().enumerate() {
            let count = count.max(0) as usize;
            let corners = start..start + count;
            start += count;
            let Some(ids) = indices.get(corners.clone()) else { break };
            let Some(p) = ids.iter().map(|&i| points.get(usize::try_from(i).ok()?).map(|p| world.transform_point3(*p))).collect::<Option<Vec<_>>>() else { continue };
            if count < 3 {
                continue;
            }
            let mut normal = DVec3::ZERO;
            for i in 0..count {
                let (a, b) = (p[i], p[(i + 1) % count]);
                normal += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
            }
            let normal = if flip { -normal } else { normal }.normalize_or_zero();
            let color = |k: usize| -> [u8; 4] {
                let c = match kind.as_str() {
                    "uniform" => colors.get(face),
                    "vertex" | "varying" => usize::try_from(ids[k]).ok().and_then(|i| colors.get(i)),
                    "faceVarying" => colors.get(corners.start + k),
                    _ => None,
                };
                c.copied().map(to_rgba).unwrap_or(base_rgba)
            };
            for k in 1..count - 1 {
                let tri = if flip { [0, k + 1, k] } else { [0, k, k + 1] };
                for i in tri {
                    layer.indices.push(layer.vertex_count() as u32);
                    layer.positions.extend(p[i].to_array().map(|v| v as f32));
                    layer.normals.extend(normal.to_array().map(|v| v as f32));
                    layer.colors.as_mut().unwrap().push(color(i));
                }
            }
        }
        let count = layer.indices.len() as u32 - first;
        if count > 0 {
            layer.elements.push((id, first, count));
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), prim.path().name().unwrap_or("Mesh").into());
            props.insert("Path".into(), prim.path().to_string().into());
            if layer_name != "Default" {
                props.insert("Material".into(), layer_name.into());
            }
            layer.props.push((id, serde_json::Value::Object(props).to_string()));
        }
    }
}

fn read(stage: &usd::Stage) -> Result<Vec<Layer>, String> {
    let meta = |field: &str| stage.stage_metadata(field).ok().flatten();
    let up = meta("upAxis").and_then(|v| text(&v)).unwrap_or_else(|| "Y".into());
    let metres = meta("metersPerUnit").as_ref().and_then(scalar).filter(|m| m.is_finite() && *m > 0.0).unwrap_or(0.01);
    // Y up: (x, y, z) becomes (x, -z, y)
    let axis = if up == "Z" { DMat4::IDENTITY } else { DMat4::from_mat3(DMat3::from_rotation_x(std::f64::consts::FRAC_PI_2)) };
    let root = axis * DMat4::from_scale(DVec3::splat(metres));
    let mut meshes = Vec::new();
    stage
        .traverse(usd::PrimPredicate::DEFAULT_PROXIES, |path| meshes.push(path.clone()))
        .map_err(|e| format!("USD: {e}"))?;
    let mut scene = Scene { stage, worlds: HashMap::new(), materials: HashMap::new(), layers: BTreeMap::new(), root };
    let mut next_id = 1;
    for path in meshes {
        let Ok(prim) = stage.prim(path) else { continue };
        if prim.type_name().ok().flatten().is_none_or(|t| t.as_str() != "Mesh") {
            continue;
        }
        if attribute(&prim, "visibility").and_then(|v| text(&v)).as_deref() == Some("invisible") {
            continue;
        }
        scene.mesh(&prim, next_id);
        next_id += 1;
    }
    let mut layers: Vec<Layer> = scene.layers.into_values().filter(|l| !l.indices.is_empty()).collect();
    for layer in &mut layers {
        // one colour for the whole layer: no vertex colours
        if layer.colors.as_ref().is_some_and(|c| c.windows(2).all(|w| w[0] == w[1])) {
            if let Some(c) = layer.colors.take().and_then(|c| c.first().copied()) {
                layer.color = [c[0], c[1], c[2]].map(|v| v as f32 / 255.0);
            }
        }
    }
    if layers.is_empty() {
        return Err("USD: no Mesh prims found".into());
    }
    Ok(layers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1 x 1 m quad (Z up, metres) moved up 2 m and rotated 90° about z, bound to a red
    /// preview surface; and a triangle referenced from an abstract class prim, with a display colour.
    const USDA: &str = r#"#usda 1.0
(
    upAxis = "Z"
    metersPerUnit = 1
    defaultPrim = "World"
)

def Xform "World"
{
    double3 xformOp:translate = (0, 0, 2)
    float xformOp:rotateZ = 90
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:rotateZ"]

    def Mesh "Floor"
    {
        int[] faceVertexCounts = [4]
        int[] faceVertexIndices = [0, 1, 2, 3]
        point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
        rel material:binding = </World/Looks/Red>
    }

    def Mesh "Tri" (
        references = </Proto>
    )
    {
        color3f[] primvars:displayColor = [(0, 1, 0)] (
            interpolation = "constant"
        )
    }

    def Scope "Looks"
    {
        def Material "Red"
        {
            token outputs:surface.connect = </World/Looks/Red/Surface.outputs:surface>
            def Shader "Surface"
            {
                uniform token info:id = "UsdPreviewSurface"
                color3f inputs:diffuseColor = (1, 0, 0)
                token outputs:surface
            }
        }
    }
}

class Mesh "Proto"
{
    int[] faceVertexCounts = [3]
    int[] faceVertexIndices = [0, 1, 2]
    point3f[] points = [(0, 0, 0), (0, 1, 0), (0, 0, 1)]
    uniform token orientation = "leftHanded"
}
"#;

    fn load(name: &str, data: Vec<u8>) -> crate::model::Model {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("USD")
    }

    #[test]
    fn composes_transforms_materials_and_references() {
        let model = load("scene.usda", USDA.as_bytes().to_vec());
        assert_eq!(model.format, "USD");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Default", "Red"]);
        let red = &model.layers[1];
        assert_eq!(red.color, [1.0, 0.0, 0.0]);
        assert_eq!(red.triangle_count(), 2);
        // rotated 90° about z: x in the file becomes y
        for p in red.positions.chunks_exact(3) {
            assert!((p[2] - 2.0).abs() < 1e-6 && (-1.0 - 1e-6..=1e-6).contains(&p[0]) && (-1e-6..=1.0 + 1e-6).contains(&p[1]), "{p:?}");
        }
        assert!(red.normals.chunks_exact(3).all(|n| n[2] > 0.99));
        // the referenced triangle (also under World, so also rotated), green, left handed
        let tri = &model.layers[0];
        assert_eq!(tri.triangle_count(), 1);
        assert_eq!(tri.color, [0.0, 1.0, 0.0]);
        // in the y-z plane, facing +x when right handed; left handed it faces -x, which the 90°
        // turn makes -y
        assert!(tri.normals.chunks_exact(3).all(|n| n[1] < -0.99), "{:?}", tri.normals);
        assert!(model.props.values().any(|p| p.contains("/World/Floor") && p.contains("\"Material\":\"Red\"")));
    }

    #[test]
    fn reads_binary_crate_files_and_usdz_packages() {
        // the same scene written as a binary crate, and that crate in a package
        let data = openusd::usda::parse(USDA).expect("usda");
        let mut usdc = std::io::Cursor::new(Vec::new());
        openusd::usdc::CrateWriter::write(&data, &mut usdc).expect("usdc");
        let usdc = usdc.into_inner();
        let text = load("scene.usda", USDA.as_bytes().to_vec());
        let crate_file = load("scene.usdc", usdc.clone());
        assert_eq!(crate_file.triangle_count(), text.triangle_count());
        assert_eq!(crate_file.bounds, text.bounds);
        let mut package = openusd::usdz::ArchiveWriter::new(std::io::Cursor::new(Vec::new()));
        package.add_layer("scene.usdc", &usdc).expect("usdz entry");
        let usdz = package.finish().expect("usdz").into_inner();
        let packaged = load("scene.USDZ", usdz);
        assert_eq!(packaged.triangle_count(), text.triangle_count());
        assert_eq!(packaged.bounds, text.bounds);
    }

    #[test]
    fn y_up_centimetres_by_default() {
        let text = "#usda 1.0\ndef Mesh \"M\"\n{\n    int[] faceVertexCounts = [3]\n    int[] faceVertexIndices = [0, 1, 2]\n    point3f[] points = [(0, 0, 0), (100, 0, 0), (0, 100, 0)]\n}\n";
        let model = load("y.usda", text.as_bytes().to_vec());
        // 100 cm along file y is 1 m up
        assert!((model.bounds.max[2] - 1.0).abs() < 1e-6, "{:?}", model.bounds);
        assert!((model.bounds.max[0] - 1.0).abs() < 1e-6);
    }
}
