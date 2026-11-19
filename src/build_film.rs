//! A film of how the model is built: its elements appear in the order they are made.
//!
//! Three parts:
//! - the **order** ([`sequence`]): the elements in steps, by the phase the file gives them
//!   (Revit) and within a phase from the ground up, in bands of [`BAND`] metres; elements a later
//!   phase demolishes go out at the first step of that phase;
//! - the **stage** ([`usd`]): the model as a USD file (`.usda`, text), one mesh per element, each
//!   with its visibility over time, so any program that reads USD plays the order;
//! - the **film** ([`render`], desktop): Blender, run without a window, loads the stage, puts a
//!   camera round it and writes an `.mp4`. Blender is a program beside this one (GPL): it is
//!   started, never linked. `O3V_BLENDER` names its executable where it is not found;
//! - a **still** ([`still`], desktop): the whole model on the same stage, rendered once by
//!   Blender's path tracer with a sky, a sun and ground under it, from a camera of the caller
//!   ([`Shot`]) or one Blender places itself.

use crate::model::Model;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// The height of a band of elements that appear together (metres).
pub const BAND: f32 = 1.0;
/// How long a film runs when nothing else is asked (seconds).
pub const SECONDS: f32 = 20.0;
/// Frames per second of the film and time codes per second of the stage.
pub const FPS: u32 = 24;

/// A step of the order: the elements that appear in it and those that go.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    /// What is said of it: its phase and how high its elements start.
    pub name: String,
    pub shown: Vec<u64>,
    pub removed: Vec<u64>,
}

/// The lowest point of every element (metres).
fn lowest(model: &Model) -> HashMap<u64, f32> {
    let mut out: HashMap<u64, f32> = HashMap::new();
    for layer in &model.layers {
        for &(id, first, count) in &layer.elements {
            let low = out.entry(id).or_insert(f32::INFINITY);
            for &i in layer.indices.iter().skip(first as usize).take(count as usize) {
                if let Some(&z) = layer.positions.get(i as usize * 3 + 2) {
                    *low = low.min(z);
                }
            }
        }
    }
    out.retain(|_, z| z.is_finite());
    out
}

/// The order the model is built in: per phase of the file (one phase where it has none), from
/// the ground up. A step without elements is left out.
pub fn sequence(model: &Model) -> Vec<Step> {
    let lows = lowest(model);
    let Some(ground) = lows.values().copied().reduce(f32::min) else { return Vec::new() };
    let phasing = &model.project.phasing.elements;
    let phase_of = |id: u64| phasing.get(&id).map_or(0, |(made, _)| *made);
    let mut steps: BTreeMap<(u16, i32), Step> = BTreeMap::new();
    let mut ids: Vec<u64> = lows.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let band = ((lows[&id] - ground) / BAND).floor() as i32;
        let phase = phase_of(id);
        let step = steps.entry((phase, band)).or_default();
        if step.name.is_empty() {
            let height = format!("{:+.0} m", ground + band as f32 * BAND);
            step.name = match model.project.phases.get(phase as usize) {
                Some(name) if !phasing.is_empty() => format!("{name}, {height}"),
                _ => height,
            };
        }
        step.shown.push(id);
    }
    // what a phase demolishes goes at its first step (or at a step of its own, where the phase
    // makes nothing)
    let mut gone: Vec<(u16, u64)> = phasing.iter().filter_map(|(id, (_, demolished))| Some(((*demolished)?, *id))).filter(|(_, id)| lows.contains_key(id)).collect();
    gone.sort_unstable();
    for (phase, id) in gone {
        let key = steps.range((phase, i32::MIN)..=(phase, i32::MAX)).next().map(|(k, _)| *k).unwrap_or((phase, i32::MIN));
        let step = steps.entry(key).or_default();
        if step.name.is_empty() {
            step.name = model.project.phases.get(phase as usize).cloned().unwrap_or_default();
        }
        step.removed.push(id);
    }
    steps.into_values().collect()
}

/// How long the film shows a step and how long it holds the finished model (frames).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub per_step: u32,
    pub hold: u32,
}

impl Timing {
    /// The timing of a film of `seconds` for `steps` steps: a fifth of it holds the end.
    pub fn of(seconds: f32, steps: usize) -> Self {
        let frames = (seconds.max(1.0) * FPS as f32) as u32;
        let hold = (frames / 5).max(FPS);
        Timing { per_step: (frames.saturating_sub(hold) / steps.max(1) as u32).max(1), hold }
    }

    /// The frame step `k` starts at, and the last frame of the film.
    pub fn frame(&self, k: usize) -> u32 {
        1 + k as u32 * self.per_step
    }
    pub fn end(&self, steps: usize) -> u32 {
        self.frame(steps) + self.hold
    }
}

/// The name of an element's mesh on the stage.
pub fn prim(id: u64) -> String {
    format!("e{id}")
}

/// The model as a USD stage in text (`.usda`): one mesh per element, Z up, metres, each visible
/// from its step on and invisible from the step that removes it. What belongs to no element is
/// one mesh that always shows.
pub fn usd(model: &Model, steps: &[Step], timing: Timing) -> String {
    let mut shown_at: HashMap<u64, u32> = HashMap::new();
    let mut gone_at: HashMap<u64, u32> = HashMap::new();
    for (k, step) in steps.iter().enumerate() {
        for &id in &step.shown {
            shown_at.entry(id).or_insert(timing.frame(k));
        }
        for &id in &step.removed {
            gone_at.entry(id).or_insert(timing.frame(k));
        }
    }
    // the triangles of every element, over the layers, with the colour of its first layer
    struct Mesh {
        points: Vec<[f32; 3]>,
        faces: Vec<u32>,
        color: [f32; 3],
    }
    let mut meshes: BTreeMap<u64, Mesh> = BTreeMap::new();
    let mut rest = Mesh { points: Vec::new(), faces: Vec::new(), color: [0.8; 3] };
    for layer in model.layers.iter().filter(|l| !l.points && !l.hidden) {
        let point = |i: u32| -> Option<[f32; 3]> { layer.positions.get(i as usize * 3..i as usize * 3 + 3).map(|p| [p[0], p[1], p[2]]) };
        let add = |mesh: &mut Mesh, indices: &[u32]| {
            let mut at: HashMap<u32, u32> = HashMap::new();
            for tri in indices.chunks_exact(3) {
                let Some(corners) = tri.iter().map(|&i| point(i)).collect::<Option<Vec<_>>>() else { continue };
                for (&i, p) in tri.iter().zip(corners) {
                    let k = *at.entry(i).or_insert_with(|| {
                        mesh.points.push(p);
                        mesh.points.len() as u32 - 1
                    });
                    mesh.faces.push(k);
                }
            }
        };
        let mut taken = 0usize;
        for &(id, first, count) in &layer.elements {
            let (first, count) = (first as usize, count as usize);
            if first > taken {
                rest.color = layer.color;
                add(&mut rest, &layer.indices[taken.min(layer.indices.len())..first.min(layer.indices.len())]);
            }
            let end = (first + count).min(layer.indices.len());
            let mesh = meshes.entry(id).or_insert_with(|| Mesh { points: Vec::new(), faces: Vec::new(), color: layer.color });
            add(mesh, &layer.indices[first.min(end)..end]);
            taken = taken.max(end);
        }
        if taken < layer.indices.len() {
            rest.color = layer.color;
            add(&mut rest, &layer.indices[taken..]);
        }
    }
    let end = timing.end(steps.len());
    let mut out = String::new();
    let _ = writeln!(out, "#usda 1.0\n(\n    defaultPrim = \"Model\"\n    upAxis = \"Z\"\n    metersPerUnit = 1\n    startTimeCode = 1\n    endTimeCode = {end}\n    timeCodesPerSecond = {FPS}\n)\n");
    let _ = writeln!(out, "def Xform \"Model\"\n{{");
    let mut write = |name: &str, mesh: &Mesh, visibility: Option<String>| {
        if mesh.faces.is_empty() {
            return;
        }
        let points: Vec<String> = mesh.points.iter().map(|p| format!("({}, {}, {})", p[0], p[1], p[2])).collect();
        let faces: Vec<String> = mesh.faces.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "    def Mesh \"{name}\"\n    {{");
        let _ = writeln!(out, "        point3f[] points = [{}]", points.join(", "));
        let _ = writeln!(out, "        int[] faceVertexCounts = [{}]", vec!["3"; mesh.faces.len() / 3].join(", "));
        let _ = writeln!(out, "        int[] faceVertexIndices = [{}]", faces.join(", "));
        let _ = writeln!(out, "        color3f[] primvars:displayColor = [({}, {}, {})]", mesh.color[0], mesh.color[1], mesh.color[2]);
        let _ = writeln!(out, "        uniform token subdivisionScheme = \"none\"");
        if let Some(samples) = visibility {
            let _ = writeln!(out, "        token visibility.timeSamples = {{ {samples} }}");
        }
        let _ = writeln!(out, "    }}");
    };
    for (id, mesh) in &meshes {
        let shown = shown_at.get(id).copied().unwrap_or(1);
        let mut samples = Vec::new();
        if shown > 1 {
            samples.push("1: \"invisible\"".to_string());
        }
        samples.push(format!("{shown}: \"inherited\""));
        if let Some(gone) = gone_at.get(id).filter(|g| **g > shown) {
            samples.push(format!("{gone}: \"invisible\""));
        }
        write(&prim(*id), mesh, Some(samples.join(", ")));
    }
    write("rest", &rest, None);
    out.push_str("}\n");
    out
}

/// The colour of every element: that of the first layer that draws it.
pub fn colours(model: &Model) -> BTreeMap<u64, [f32; 3]> {
    let mut out = BTreeMap::new();
    for layer in model.layers.iter().filter(|l| !l.points && !l.hidden) {
        for &(id, _, _) in &layer.elements {
            out.entry(id).or_insert(layer.color);
        }
    }
    out
}

/// What the script in Blender needs beside the stage: per step the frame it starts at, its name
/// and the meshes it shows and removes; the frames of the whole film; the colour of each mesh.
pub fn plan(steps: &[Step], timing: Timing, colours: &BTreeMap<u64, [f32; 3]>) -> String {
    let steps: Vec<serde_json::Value> = steps
        .iter()
        .enumerate()
        .map(|(k, s)| {
            serde_json::json!({
                "frame": timing.frame(k),
                "name": s.name,
                "show": s.shown.iter().map(|id| prim(*id)).collect::<Vec<_>>(),
                "hide": s.removed.iter().map(|id| prim(*id)).collect::<Vec<_>>(),
            })
        })
        .collect();
    let colours: serde_json::Map<String, serde_json::Value> = colours.iter().map(|(id, c)| (prim(*id), serde_json::json!(c))).collect();
    serde_json::json!({ "fps": FPS, "end": timing.end(steps.len()), "steps": steps, "colours": colours }).to_string()
}

/// Where a still is taken from: the eye, what it looks at (metres, Z up) and its vertical field
/// of view (degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
}

/// What the script needs for a still: that it is one, the colours, and the camera where the
/// caller gives one.
pub fn still_plan(colours: &BTreeMap<u64, [f32; 3]>, shot: Option<Shot>) -> String {
    let colours: serde_json::Map<String, serde_json::Value> = colours.iter().map(|(id, c)| (prim(*id), serde_json::json!(c))).collect();
    let mut plan = serde_json::json!({ "still": true, "fps": FPS, "end": 1, "steps": [], "colours": colours });
    if let Some(shot) = shot {
        plan["camera"] = serde_json::json!({ "eye": shot.eye, "target": shot.target, "fov": shot.fov });
    }
    plan.to_string()
}

/// The script Blender runs: it loads the stage, keys what shows when, puts a sun and a camera
/// that turns round the model, and writes the film; or renders one still.
#[cfg(not(target_arch = "wasm32"))]
const SCRIPT: &str = include_str!("build_film.py");

/// Where Blender is: `O3V_BLENDER`, else `blender` on the path, else the newest one installed
/// in the usual place.
#[cfg(not(target_arch = "wasm32"))]
pub fn blender() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(named) = std::env::var_os("O3V_BLENDER").map(PathBuf::from) {
        return named.is_file().then_some(named);
    }
    let name = if cfg!(windows) { "blender.exe" } else { "blender" };
    if let Some(found) = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(name)).find(|p| p.is_file())) {
        return Some(found);
    }
    let mut installed: Vec<PathBuf> = Vec::new();
    for root in ["C:\\Program Files\\Blender Foundation", "/Applications", "/usr/share", "/opt"] {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let dir = entry.path();
            if !dir.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("blender")) {
                continue;
            }
            for at in [dir.join(name), dir.join("Contents/MacOS/Blender")] {
                if at.is_file() {
                    installed.push(at);
                }
            }
        }
    }
    installed.sort();
    installed.pop()
}

/// Writes the film of `model` to `out`: an `.mp4` through Blender, or with the extension
/// `.usda` the stage alone. `seconds` is how long the film runs. Says what it wrote.
#[cfg(not(target_arch = "wasm32"))]
pub fn render(model: &Model, out: &std::path::Path, seconds: f32) -> Result<String, String> {
    let steps = sequence(model);
    if steps.is_empty() {
        return Err("the model has no elements to build a film of".into());
    }
    let timing = Timing::of(seconds, steps.len());
    let elements: std::collections::HashSet<u64> = steps.iter().flat_map(|s| s.shown.iter().copied()).collect();
    let said = format!("{} elements in {} steps, {:.1} s", elements.len(), steps.len(), timing.end(steps.len()) as f32 / FPS as f32);
    let stage = usd(model, &steps, timing);
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("usda")) {
        std::fs::write(out, stage).map_err(|e| format!("{}: {e}", out.display()))?;
        return Ok(said);
    }
    run_blender(&stage, &plan(&steps, timing, &colours(model)), out)?;
    Ok(said)
}

/// Writes a still of the whole `model` to `out` (a `.png`), rendered by Blender: from `shot`, or
/// from where Blender finds the model shows well. Says what it wrote.
#[cfg(not(target_arch = "wasm32"))]
pub fn still(model: &Model, out: &std::path::Path, shot: Option<Shot>) -> Result<String, String> {
    let colours = colours(model);
    if colours.is_empty() && model.layers.iter().all(|l| l.indices.is_empty()) {
        return Err("the model has nothing to render".into());
    }
    let stage = usd(model, &[], Timing { per_step: 1, hold: 0 });
    run_blender(&stage, &still_plan(&colours, shot), out)?;
    Ok(format!("{} elements, 1920 x 1080", colours.len()))
}

/// Starts Blender without a window on the stage and the plan; it writes `out`.
#[cfg(not(target_arch = "wasm32"))]
fn run_blender(stage: &str, plan: &str, out: &std::path::Path) -> Result<(), String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static RUN: AtomicU32 = AtomicU32::new(0);
    let blender = blender().ok_or("Blender is not found: install it, or set O3V_BLENDER to its executable")?;
    let dir = std::env::temp_dir().join(format!("o3v-build-film-{}-{}", std::process::id(), RUN.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (stage_at, plan_at, script_at) = (dir.join("stage.usda"), dir.join("plan.json"), dir.join("build_film.py"));
    let written = std::fs::write(&stage_at, stage).and_then(|()| std::fs::write(&plan_at, plan)).and_then(|()| std::fs::write(&script_at, SCRIPT));
    written.map_err(|e| format!("{}: {e}", dir.display()))?;
    let run = std::process::Command::new(&blender)
        .args(["--background", "--factory-startup", "--python"])
        .arg(&script_at)
        .arg("--")
        .args([&stage_at, &plan_at])
        .arg(out)
        .output()
        .map_err(|e| format!("{}: {e}", blender.display()))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !run.status.success() || !out.is_file() {
        let log = String::from_utf8_lossy(&run.stdout);
        let last: Vec<&str> = log.lines().rev().filter(|l| !l.trim().is_empty()).take(6).collect();
        return Err(format!("Blender wrote nothing ({}): {}", run.status, last.into_iter().rev().collect::<Vec<_>>().join(" | ")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;

    /// Two boxes, the second one storey up; each an element.
    fn model() -> Model {
        let mut layer = Layer { name: "walls".into(), color: [0.5, 0.25, 0.125], ..Default::default() };
        for (id, z) in [(7u64, 0.0f32), (9, 3.0)] {
            let first = layer.indices.len() as u32;
            let base = layer.positions.len() as u32 / 3;
            layer.positions.extend([0.0, 0.0, z, 1.0, 0.0, z, 0.0, 1.0, z, 0.0, 0.0, z + 1.0]);
            layer.normals.extend([0.0, 0.0, 1.0].repeat(4));
            layer.indices.extend([0, 1, 2, 0, 1, 3].map(|i| base + i));
            layer.elements.push((id, first, 6));
        }
        Model::new("two.ifc".into(), "IFC", vec![layer])
    }

    #[test]
    fn a_model_is_built_from_the_ground_up() {
        let steps = sequence(&model());
        assert_eq!(steps.iter().map(|s| s.shown.clone()).collect::<Vec<_>>(), [vec![7], vec![9]]);
        assert_eq!(steps[0].name, "+0 m");
        assert_eq!(steps[1].name, "+3 m");
    }

    #[test]
    fn phases_come_before_height_and_a_demolished_element_goes() {
        let mut m = model();
        m.project.phases = vec!["Existing".into(), "New".into()];
        // the upper box is existing; the lower one is new, and the new phase demolishes the upper
        m.project.phasing.elements.insert(9, (0, Some(1)));
        m.project.phasing.elements.insert(7, (1, None));
        let steps = sequence(&m);
        assert_eq!(steps.len(), 2);
        assert_eq!((steps[0].name.as_str(), &steps[0].shown), ("Existing, +3 m", &vec![9]));
        assert_eq!((steps[1].name.as_str(), &steps[1].shown, &steps[1].removed), ("New, +0 m", &vec![7], &vec![9]));
    }

    #[test]
    fn the_stage_gives_every_element_a_mesh_with_its_time() {
        let m = model();
        let steps = sequence(&m);
        let timing = Timing { per_step: 10, hold: 24 };
        let stage = usd(&m, &steps, timing);
        assert!(stage.starts_with("#usda 1.0"));
        assert!(stage.contains("upAxis = \"Z\"") && stage.contains("endTimeCode = 45"), "{stage}");
        // the first element shows from the start, the second from its step on
        assert!(stage.contains("def Mesh \"e7\"") && stage.contains("token visibility.timeSamples = { 1: \"inherited\" }"));
        assert!(stage.contains("token visibility.timeSamples = { 1: \"invisible\", 11: \"inherited\" }"), "{stage}");
        // two triangles of four points each, in the layer's colour
        assert_eq!(stage.matches("int[] faceVertexCounts = [3, 3]").count(), 2);
        assert!(stage.contains("int[] faceVertexIndices = [0, 1, 2, 0, 1, 3]"));
        assert!(stage.contains("(0.5, 0.25, 0.125)"));
        // and the plan names the same meshes at the same frames
        let plan: serde_json::Value = serde_json::from_str(&plan(&steps, timing, &colours(&m))).unwrap();
        assert_eq!(plan["colours"]["e9"], serde_json::json!([0.5, 0.25, 0.125]));
        assert_eq!(plan["end"], 45);
        assert_eq!(plan["steps"][1]["frame"], 11);
        assert_eq!(plan["steps"][1]["show"][0], "e9");
    }

    #[test]
    fn a_film_of_some_seconds_shares_them_over_its_steps() {
        let t = Timing::of(10.0, 8);
        assert_eq!((t.hold, t.per_step), (48, 24));
        assert_eq!(t.end(8), 1 + 8 * 24 + 48);
        // more steps than frames: a frame each
        assert_eq!(Timing::of(1.0, 1000).per_step, 1);
    }

    #[test]
    fn a_still_shows_everything_from_the_camera_it_is_given() {
        let m = model();
        // no steps: every element shows from the first frame
        let stage = usd(&m, &[], Timing { per_step: 1, hold: 0 });
        assert_eq!(stage.matches("token visibility.timeSamples = { 1: \"inherited\" }").count(), 2, "{stage}");
        let shot = Shot { eye: [10.0, -10.0, 5.0], target: [0.0, 0.0, 2.0], fov: 40.0 };
        let plan: serde_json::Value = serde_json::from_str(&still_plan(&colours(&m), Some(shot))).unwrap();
        assert_eq!(plan["still"], true);
        assert_eq!(plan["camera"]["eye"], serde_json::json!([10.0, -10.0, 5.0]));
        assert_eq!(plan["camera"]["fov"], 40.0);
        // without one, Blender places the camera itself
        let plan: serde_json::Value = serde_json::from_str(&still_plan(&colours(&m), None)).unwrap();
        assert!(plan.get("camera").is_none());
    }

    /// Blender renders a still of the two boxes (needs Blender; writes a file in the temp folder).
    #[test]
    #[ignore = "needs Blender and writes an image file"]
    fn blender_renders_a_still() {
        let out = std::env::temp_dir().join(format!("o3v-still-test-{}.png", std::process::id()));
        let said = still(&model(), &out, None).expect("a still");
        let size = std::fs::metadata(&out).expect("the file").len();
        eprintln!("{said}; {} bytes at {}", size, out.display());
        assert!(size > 1000);
    }

    /// Blender writes a film of the two boxes (needs Blender; writes a file in the temp folder).
    #[test]
    #[ignore = "needs Blender and writes a video file"]
    fn blender_writes_the_film() {
        let out = std::env::temp_dir().join(format!("o3v-build-film-test-{}.mp4", std::process::id()));
        let said = render(&model(), &out, 2.0).expect("a film");
        let size = std::fs::metadata(&out).expect("the file").len();
        eprintln!("{said}; {} bytes at {}", size, out.display());
        assert!(size > 1000);
    }
}
