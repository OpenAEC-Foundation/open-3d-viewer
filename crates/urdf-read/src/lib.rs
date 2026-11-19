//! URDF robot descriptions as placed shapes.
//!
//! The file is read by `urdf-rs`. Every link's visual geometry is placed in the robot's base
//! frame by the joints from the root link down, each joint in its zero position (its origin
//! alone). Boxes, cylinders, capsules and spheres become triangle meshes here; a mesh file is
//! handed back by its path, found relative to the URDF file, with its scale and placement, for
//! the caller to read with a reader of its format.
//!
//! Lengths are metres and Z is up, as URDF defines them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A placement: a 4 × 4 matrix by columns, metres.
pub type Matrix = [[f64; 4]; 4];

/// The identity placement.
pub const IDENTITY: Matrix = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A robot: its name and the visuals of its links.
#[derive(Debug, Clone, Default)]
pub struct Robot {
    pub name: String,
    pub visuals: Vec<Visual>,
    /// Links no joint reaches from the root (left out), by name.
    pub unreached: Vec<String>,
}

/// One visual element of a link, placed in the robot's base frame.
#[derive(Debug, Clone)]
pub struct Visual {
    /// The link's name.
    pub link: String,
    /// The visual's own name, if it has one.
    pub name: Option<String>,
    /// The joints from the root to the link, by name.
    pub joints: Vec<String>,
    /// The colour of its material (sRGB, 0..1, with alpha), when the file gives one.
    pub color: Option<[f32; 4]>,
    /// The name of its material, if any.
    pub material: Option<String>,
    pub shape: Shape,
}

/// The geometry of a visual.
#[derive(Debug, Clone)]
pub enum Shape {
    /// A box, cylinder, capsule or sphere, triangulated and placed.
    Solid { kind: &'static str, mesh: Mesh },
    /// A mesh file: the name as the URDF gives it, the file found for it (if any), and the
    /// placement of its vertices (scale included).
    File { filename: String, path: Option<PathBuf>, placement: Matrix },
}

/// Triangles in the base frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

/// Reads a URDF file's text; `base` is the folder of the file (for mesh files), if known.
pub fn read(text: &str, base: Option<&Path>) -> Result<Robot, String> {
    let urdf = urdf_rs::read_from_string(text).map_err(|e| format!("URDF: {e}"))?;
    Ok(place(&urdf, base))
}

/// Places the visuals of a parsed robot.
pub fn place(urdf: &urdf_rs::Robot, base: Option<&Path>) -> Robot {
    let materials: HashMap<&str, &urdf_rs::Material> = urdf.materials.iter().map(|m| (m.name.as_str(), m)).collect();
    let children: HashSet<&str> = urdf.joints.iter().map(|j| j.child.link.as_str()).collect();
    let mut by_parent: HashMap<&str, Vec<&urdf_rs::Joint>> = HashMap::new();
    for joint in &urdf.joints {
        by_parent.entry(joint.parent.link.as_str()).or_default().push(joint);
    }
    // the link poses, from every root (a link that is no joint's child) down
    let mut poses: HashMap<&str, (Matrix, Vec<String>)> = HashMap::new();
    let mut stack: Vec<(&str, Matrix, Vec<String>)> = urdf.links.iter().filter(|l| !children.contains(l.name.as_str())).map(|l| (l.name.as_str(), IDENTITY, Vec::new())).collect();
    stack.reverse();
    while let Some((link, pose, joints)) = stack.pop() {
        if poses.contains_key(link) {
            continue;
        }
        poses.insert(link, (pose, joints.clone()));
        for joint in by_parent.get(link).into_iter().flatten().rev() {
            let mut path = joints.clone();
            path.push(joint.name.clone());
            stack.push((joint.child.link.as_str(), multiply(&pose, &origin(&joint.origin)), path));
        }
    }
    let mut robot = Robot { name: urdf.name.clone(), ..Robot::default() };
    for link in &urdf.links {
        let Some((pose, joints)) = poses.get(link.name.as_str()) else {
            robot.unreached.push(link.name.clone());
            continue;
        };
        for visual in &link.visual {
            let placement = multiply(pose, &origin(&visual.origin));
            let named = visual.material.as_ref().and_then(|m| materials.get(m.name.as_str()).copied());
            let color = visual.material.as_ref().and_then(|m| m.color.as_ref()).or(named.and_then(|m| m.color.as_ref())).map(|c| c.rgba.0.map(|v| v as f32));
            let material = visual.material.as_ref().map(|m| m.name.clone()).filter(|n| !n.is_empty());
            let shape = match &visual.geometry {
                urdf_rs::Geometry::Box { size } => Shape::Solid { kind: "Box", mesh: transformed(cuboid([size[0], size[1], size[2]]), &placement) },
                urdf_rs::Geometry::Cylinder { radius, length } => Shape::Solid { kind: "Cylinder", mesh: transformed(capsule(*radius, *length, false), &placement) },
                urdf_rs::Geometry::Capsule { radius, length } => Shape::Solid { kind: "Capsule", mesh: transformed(capsule(*radius, *length, true), &placement) },
                urdf_rs::Geometry::Sphere { radius } => Shape::Solid { kind: "Sphere", mesh: transformed(sphere(*radius), &placement) },
                urdf_rs::Geometry::Mesh { filename, scale } => {
                    let s = scale.map(|s| s.0).unwrap_or([1.0; 3]);
                    let scaled = [[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0], [0.0, 0.0, 0.0, 1.0]];
                    let path = base.and_then(|b| resolve(filename, b));
                    Shape::File { filename: filename.clone(), path, placement: multiply(&placement, &scaled) }
                }
            };
            robot.visuals.push(Visual { link: link.name.clone(), name: visual.name.clone(), joints: joints.clone(), color, material, shape });
        }
    }
    robot
}

/// The file a mesh name points to, looked for from the URDF's folder `base`: a plain path
/// relative to it (or absolute), `file://` paths, and `package://<package>/<path>`, whose
/// package is taken to be `base` or a folder above it, or a folder of that name beside one.
pub fn resolve(filename: &str, base: &Path) -> Option<PathBuf> {
    let found = |p: PathBuf| p.is_file().then_some(p);
    if let Some(rest) = filename.strip_prefix("file://") {
        return found(PathBuf::from(rest)).or_else(|| found(base.join(rest.trim_start_matches('/'))));
    }
    let Some(rest) = filename.strip_prefix("package://").or_else(|| filename.strip_prefix("model://")) else {
        return found(base.join(filename));
    };
    let (package, inner) = rest.split_once('/').unwrap_or((rest, ""));
    let mut folder = Some(base);
    while let Some(dir) = folder {
        for candidate in [dir.join(inner), dir.join(package).join(inner)] {
            if dir.file_name().is_some_and(|n| n == package) || candidate.starts_with(dir.join(package)) {
                if let Some(path) = found(candidate) {
                    return Some(path);
                }
            }
        }
        folder = dir.parent();
    }
    // last: the path inside the package from the URDF's folder and the one above it
    found(base.join(inner)).or_else(|| base.parent().and_then(|p| found(p.join(inner))))
}

/// The placement of a URDF origin: translation `xyz`, then roll, pitch and yaw about the fixed
/// X, Y and Z axes.
fn origin(pose: &urdf_rs::Pose) -> Matrix {
    let [x, y, z] = pose.xyz.0;
    let [roll, pitch, yaw] = pose.rpy.0;
    let (sr, cr) = roll.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    [
        [cy * cp, sy * cp, -sp, 0.0],
        [cy * sp * sr - sy * cr, sy * sp * sr + cy * cr, cp * sr, 0.0],
        [cy * sp * cr + sy * sr, sy * sp * cr - cy * sr, cp * cr, 0.0],
        [x, y, z, 1.0],
    ]
}

/// `a` after `b`.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, value) in column.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// A point placed by `m`.
pub fn point(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

/// A normal placed by `m` (by the inverse transpose of its linear part), unit length.
pub fn normal(m: &Matrix, n: [f64; 3]) -> [f64; 3] {
    let col = |c: usize| [m[c][0], m[c][1], m[c][2]];
    let (a, b, c) = (col(0), col(1), col(2));
    let cross = |u: [f64; 3], v: [f64; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    // the rows of the inverse transpose are the cross products of the columns, over the
    // determinant; its sign keeps the normal outward when the placement mirrors
    let (bc, ca, ab) = (cross(b, c), cross(c, a), cross(a, b));
    let det = a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2];
    let v = [0, 1, 2].map(|r| (bc[r] * n[0] + ca[r] * n[1] + ab[r] * n[2]) * det.signum());
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 { v.map(|x| x / len) } else { [0.0, 0.0, 1.0] }
}

/// True when `m` mirrors (its triangles' winding must turn).
pub fn mirrors(m: &Matrix) -> bool {
    let col = |c: usize| [m[c][0], m[c][1], m[c][2]];
    let (a, b, c) = (col(0), col(1), col(2));
    let bc = [b[1] * c[2] - b[2] * c[1], b[2] * c[0] - b[0] * c[2], b[0] * c[1] - b[1] * c[0]];
    a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2] < 0.0
}

/// A mesh in local coordinates (f64), as triangles with a normal per vertex.
#[derive(Default)]
struct Local {
    points: Vec<[f64; 3]>,
    normals: Vec<[f64; 3]>,
    indices: Vec<u32>,
}

fn transformed(local: Local, m: &Matrix) -> Mesh {
    let mut mesh = Mesh::default();
    for (p, n) in local.points.iter().zip(&local.normals) {
        mesh.positions.extend(point(m, *p).map(|v| v as f32));
        mesh.normals.extend(normal(m, *n).map(|v| v as f32));
    }
    mesh.indices = local.indices;
    if mirrors(m) {
        for tri in mesh.indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
    mesh
}

/// A box of `size` about the origin.
fn cuboid(size: [f64; 3]) -> Local {
    let h = size.map(|s| s / 2.0);
    let mut out = Local::default();
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for sign in [-1.0, 1.0] {
            let base = out.points.len() as u32;
            let mut n = [0.0; 3];
            n[axis] = sign;
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let mut p = [0.0; 3];
                p[axis] = sign * h[axis];
                p[u] = a * h[u];
                p[v] = b * h[v];
                out.points.push(p);
                out.normals.push(n);
            }
            let quad: [u32; 6] = if sign > 0.0 { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
            out.indices.extend(quad.iter().map(|k| base + k));
        }
    }
    out
}

const SEGMENTS: usize = 32;

/// A cylinder (or with `rounded`, a capsule: a cylinder with half spheres on its ends) along
/// Z about the origin, `length` long between its ends' centres.
fn capsule(radius: f64, length: f64, rounded: bool) -> Local {
    let mut out = Local::default();
    let half = length / 2.0;
    let ring = |out: &mut Local, z: f64, nz: f64, r: f64, nr: f64| {
        for k in 0..=SEGMENTS {
            let a = k as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
            let (s, c) = a.sin_cos();
            out.points.push([r * c, r * s, z]);
            let n = [nr * c, nr * s, nz];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            out.normals.push(n.map(|v| v / len));
        }
    };
    let strip = |out: &mut Local, lower: u32, upper: u32| {
        for k in 0..SEGMENTS as u32 {
            let (a, b, c, d) = (lower + k, lower + k + 1, upper + k + 1, upper + k);
            out.indices.extend([a, b, c, a, c, d]);
        }
    };
    // the side
    let lower = out.points.len() as u32;
    ring(&mut out, -half, 0.0, radius, 1.0);
    let upper = out.points.len() as u32;
    ring(&mut out, half, 0.0, radius, 1.0);
    strip(&mut out, lower, upper);
    if rounded {
        // half spheres: rings from the equator to the pole on each end
        let steps = SEGMENTS / 4;
        for sign in [1.0, -1.0] {
            let mut previous = if sign > 0.0 { upper } else { lower };
            for k in 1..=steps {
                let t = k as f64 / steps as f64 * std::f64::consts::FRAC_PI_2;
                let (s, c) = t.sin_cos();
                let next = out.points.len() as u32;
                ring(&mut out, sign * (half + radius * s), sign * s, radius * c, c);
                if sign > 0.0 { strip(&mut out, previous, next) } else { strip(&mut out, next, previous) }
                previous = next;
            }
        }
    } else {
        for sign in [-1.0, 1.0] {
            let centre = out.points.len() as u32;
            out.points.push([0.0, 0.0, sign * half]);
            out.normals.push([0.0, 0.0, sign]);
            let rim = out.points.len() as u32;
            for k in 0..=SEGMENTS {
                let a = k as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
                out.points.push([radius * a.cos(), radius * a.sin(), sign * half]);
                out.normals.push([0.0, 0.0, sign]);
            }
            for k in 0..SEGMENTS as u32 {
                if sign > 0.0 {
                    out.indices.extend([centre, rim + k, rim + k + 1]);
                } else {
                    out.indices.extend([centre, rim + k + 1, rim + k]);
                }
            }
        }
    }
    out
}

/// A sphere about the origin.
fn sphere(radius: f64) -> Local {
    let mut out = Local::default();
    let rows = SEGMENTS / 2;
    for i in 0..=rows {
        let t = i as f64 / rows as f64 * std::f64::consts::PI;
        let (st, ct) = t.sin_cos();
        for k in 0..=SEGMENTS {
            let a = k as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
            let n = [st * a.cos(), st * a.sin(), -ct];
            out.points.push(n.map(|v| v * radius));
            out.normals.push(n);
        }
    }
    let width = SEGMENTS as u32 + 1;
    for i in 0..rows as u32 {
        for k in 0..SEGMENTS as u32 {
            let (a, b) = (i * width + k, i * width + k + 1);
            let (c, d) = ((i + 1) * width + k + 1, (i + 1) * width + k);
            out.indices.extend([a, b, c, a, c, d]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARM: &str = r#"<?xml version="1.0"?>
<robot name="arm">
  <material name="orange"><color rgba="1 0.5 0 1"/></material>
  <link name="base">
    <visual><origin xyz="0 0 0.05"/><geometry><box size="0.4 0.3 0.1"/></geometry><material name="orange"/></visual>
  </link>
  <link name="upper">
    <visual name="tube"><origin xyz="0 0 0.25"/><geometry><cylinder radius="0.05" length="0.5"/></geometry><material name="grey"><color rgba="0.5 0.5 0.5 1"/></material></visual>
  </link>
  <link name="hand">
    <visual><geometry><sphere radius="0.08"/></geometry></visual>
    <visual><origin xyz="0.1 0 0" rpy="0 1.5707963 0"/><geometry><mesh filename="package://arm_description/meshes/gripper.stl" scale="0.001 0.001 0.001"/></geometry></visual>
  </link>
  <link name="loose"/>
  <joint name="shoulder" type="revolute"><parent link="base"/><child link="upper"/><origin xyz="0 0 0.1"/><axis xyz="0 1 0"/><limit lower="-1" upper="1" effort="1" velocity="1"/></joint>
  <joint name="elbow" type="fixed"><parent link="upper"/><child link="hand"/><origin xyz="0 0 0.5" rpy="0 0 1.5707963"/></joint>
</robot>"#;

    fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in mesh.positions.chunks(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    #[test]
    fn places_the_links_by_their_joints_at_zero() {
        let robot = read(ARM, None).unwrap();
        assert_eq!(robot.name, "arm");
        assert_eq!(robot.visuals.len(), 4);
        // a link without a joint is a root of its own, without visuals
        assert!(robot.unreached.is_empty());
        let Shape::Solid { kind, mesh } = &robot.visuals[0].shape else { panic!() };
        assert_eq!(*kind, "Box");
        assert!(close(bounds(mesh).0, [-0.2, -0.15, 0.0]) && close(bounds(mesh).1, [0.2, 0.15, 0.1]));
        assert_eq!(robot.visuals[0].color, Some([1.0, 0.5, 0.0, 1.0]), "the colour of the named material");
        let Shape::Solid { kind, mesh } = &robot.visuals[1].shape else { panic!() };
        assert_eq!(*kind, "Cylinder");
        assert!(close(bounds(mesh).0, [-0.05, -0.05, 0.1]) && close(bounds(mesh).1, [0.05, 0.05, 0.6]));
        assert_eq!(robot.visuals[1].joints, ["shoulder"]);
        assert_eq!(robot.visuals[1].name.as_deref(), Some("tube"));
        let Shape::Solid { mesh, .. } = &robot.visuals[2].shape else { panic!() };
        assert!(close(bounds(mesh).0, [-0.08, -0.08, 0.52]) && close(bounds(mesh).1, [0.08, 0.08, 0.68]));
        assert_eq!(robot.visuals[2].joints, ["shoulder", "elbow"]);
    }

    #[test]
    fn a_mesh_file_keeps_its_name_scale_and_turns() {
        let robot = read(ARM, None).unwrap();
        let Shape::File { filename, path, placement } = &robot.visuals[3].shape else { panic!() };
        assert_eq!(filename, "package://arm_description/meshes/gripper.stl");
        assert!(path.is_none(), "no folder to look in");
        // the elbow turns a quarter about Z, so the visual's 0.1 along X lands along Y
        let at = point(placement, [0.0, 0.0, 0.0]);
        assert!((at[0]).abs() < 1e-6 && (at[1] - 0.1).abs() < 1e-6 && (at[2] - 0.6).abs() < 1e-6, "{at:?}");
        // millimetres: 1000 units along the mesh's X are 1 m, turned about Y onto -Z
        let tip = point(placement, [1000.0, 0.0, 0.0]);
        assert!((tip[2] - (0.6 - 1.0)).abs() < 1e-5, "{tip:?}");
    }

    #[test]
    fn finds_mesh_files_in_the_package() {
        let root = std::env::temp_dir().join(format!("urdf-read-{}", std::process::id()));
        let urdf_dir = root.join("arm_description").join("urdf");
        let meshes = root.join("arm_description").join("meshes");
        std::fs::create_dir_all(&urdf_dir).unwrap();
        std::fs::create_dir_all(&meshes).unwrap();
        std::fs::write(meshes.join("gripper.stl"), b"solid x\nendsolid x\n").unwrap();
        std::fs::write(urdf_dir.join("local.dae"), b"<COLLADA/>").unwrap();
        let found = resolve("package://arm_description/meshes/gripper.stl", &urdf_dir).unwrap();
        assert_eq!(found, meshes.join("gripper.stl"));
        assert_eq!(resolve("local.dae", &urdf_dir).unwrap(), urdf_dir.join("local.dae"));
        assert_eq!(resolve("../meshes/gripper.stl", &urdf_dir).unwrap(), urdf_dir.join("../meshes/gripper.stl"));
        assert!(resolve("package://other/meshes/none.stl", &urdf_dir).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn solids_are_closed_and_face_outward() {
        for local in [cuboid([1.0, 2.0, 3.0]), capsule(0.5, 2.0, false), capsule(0.5, 2.0, true), sphere(1.0)] {
            let mesh = transformed(local, &IDENTITY);
            let p = |k: u32| {
                let i = k as usize * 3;
                [mesh.positions[i] as f64, mesh.positions[i + 1] as f64, mesh.positions[i + 2] as f64]
            };
            // the signed volume of a closed, outward mesh is positive
            let volume: f64 = mesh.indices.chunks(3).map(|t| {
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
            }).sum::<f64>() / 6.0;
            assert!(volume > 0.0, "{volume}");
        }
        assert_eq!(transformed(cuboid([1.0, 2.0, 3.0]), &IDENTITY).indices.len(), 36);
    }

    #[test]
    fn mirrored_placements_keep_normals_outward() {
        let m: Matrix = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        assert!(mirrors(&m));
        assert_eq!(normal(&m, [1.0, 0.0, 0.0]), [-1.0, 0.0, 0.0]);
        assert!(read("<robot", None).is_err());
    }
}
