//! Remote control: a running viewer takes commands over a local socket (see `remote_link` for the
//! protocol), so an AI agent can do through the MCP server `open-3d-viewer-mcp` what a person does
//! in the window: open files, look around, show and hide layers, select, cut, measure and take
//! screenshots. Every command goes through [`App::remote`] on the UI thread, as a click would.

use crate::annotations::Measurement;
use crate::app::{face_at, pick_triangle, App, Message, Status};
use crate::explode::ExplodeMode;
use crate::model::{Bounds, Model, ViewKind};
use crate::remote_link::{self, Announcement};
use crate::section::{SectionBox, SectionPlane};
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::RenderMode;
use glam::Vec3;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{window, Task};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

type Answer = Result<Value, String>;

/// A command from a client and the way back for its answer.
#[derive(Clone)]
pub struct RemoteCall {
    pub cmd: String,
    pub args: Value,
    reply: Arc<Mutex<Option<mpsc::Sender<Answer>>>>,
}

impl RemoteCall {
    pub fn new(cmd: &str, args: Value) -> (Self, mpsc::Receiver<Answer>) {
        let (tx, rx) = mpsc::channel();
        (Self { cmd: cmd.to_string(), args, reply: Arc::new(Mutex::new(Some(tx))) }, rx)
    }

    /// Answers once; later answers are dropped.
    pub fn answer(&self, answer: Answer) {
        if let Some(tx) = self.reply.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(answer);
        }
    }
}

impl std::fmt::Debug for RemoteCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RemoteCall({} {})", self.cmd, self.args)
    }
}

/// Remote control state kept in the app.
#[derive(Debug, Default)]
pub struct RemoteState {
    /// Listen for commands (the window; not the headless paths).
    pub enabled: bool,
    /// `open_file` calls waiting for the model.
    pub pending_load: Vec<RemoteCall>,
    /// `screenshot` calls waiting for the capture.
    pub pending_shots: Vec<RemoteCall>,
    /// Size of the last viewport screenshot handed out: screen coordinates in commands are in its
    /// pixels.
    pub shot_image: Option<(u32, u32)>,
}

// ---- the socket -------------------------------------------------------------------------------

/// The subscription stream: starts the listener and yields its commands as messages.
pub fn serve() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async |mut output| {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<RemoteCall>();
        if let Err(e) = listen(tx) {
            eprintln!("remote control: {e}");
            return;
        }
        while let Some(call) = rx.next().await {
            if output.send(Message::Remote(call)).await.is_err() {
                break;
            }
        }
    })
}

fn listen(tx: iced::futures::channel::mpsc::UnboundedSender<RemoteCall>) -> std::io::Result<()> {
    let port = std::env::var("O3V_REMOTE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let announcement = Announcement { port: listener.local_addr()?.port(), token: new_token(), pid: std::process::id() };
    remote_link::announce(&announcement)?;
    let token = announcement.token;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, token) = (tx.clone(), token.clone());
            std::thread::spawn(move || connection(stream, &tx, &token));
        }
    });
    Ok(())
}

/// One client: a request per line, an answer per line.
fn connection(stream: TcpStream, tx: &iced::futures::channel::mpsc::UnboundedSender<RemoteCall>, token: &str) {
    let Ok(read) = stream.try_clone() else { return };
    let mut write = stream;
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let (answer, close) = match serde_json::from_str::<Value>(&line) {
            Err(e) => (Err(format!("bad request: {e}")), false),
            Ok(req) if req["token"].as_str() != Some(token) => (Err("wrong token".to_string()), true),
            Ok(req) => {
                let (call, rx) = RemoteCall::new(req["cmd"].as_str().unwrap_or(""), req["args"].clone());
                if tx.unbounded_send(call).is_err() {
                    (Err("the viewer is closing".to_string()), true)
                } else {
                    (rx.recv().unwrap_or_else(|_| Err("the viewer dropped the command".to_string())), false)
                }
            }
        };
        let out = match answer {
            Ok(result) => json!({ "ok": true, "result": result }),
            Err(error) => json!({ "ok": false, "error": error }),
        };
        if writeln!(write, "{out}").and_then(|_| write.flush()).is_err() || close {
            break;
        }
    }
}

/// 128 random bits from the standard library's hasher keys.
fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(nanos ^ ((std::process::id() as u128) << 64) ^ i as u128);
            format!("{:016x}", h.finish())
        })
        .collect()
}

// ---- argument helpers -------------------------------------------------------------------------

fn num(a: &Value, key: &str) -> Option<f32> {
    a.get(key)?.as_f64().map(|v| v as f32)
}

fn flag(a: &Value, key: &str) -> Option<bool> {
    a.get(key)?.as_bool()
}

fn vec3(v: &Value) -> Option<Vec3> {
    match v {
        Value::Array(a) if a.len() == 3 => Some(Vec3::new(a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32)),
        Value::Object(o) => Some(Vec3::new(o.get("x")?.as_f64()? as f32, o.get("y")?.as_f64()? as f32, o.get("z")?.as_f64()? as f32)),
        _ => None,
    }
}

/// [x, y, z] where a null or missing entry keeps `current`.
fn partial_vec3(v: &Value, current: Vec3) -> Option<Vec3> {
    let a = v.as_array().filter(|a| a.len() == 3)?;
    let at = |i: usize, c: f32| a[i].as_f64().map_or(c, |x| x as f32);
    Some(Vec3::new(at(0, current.x), at(1, current.y), at(2, current.z)))
}

fn id_of(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.trim().parse().ok())
}

fn round(v: f32) -> f64 {
    (v as f64 * 1e4).round() / 1e4
}

fn jv(v: Vec3) -> Value {
    json!([round(v.x), round(v.y), round(v.z)])
}

fn jbounds(b: &Bounds) -> Value {
    let (min, max) = (Vec3::from_array(b.min), Vec3::from_array(b.max));
    json!({ "min": jv(min), "max": jv(max), "size": jv(max - min), "center": jv((min + max) * 0.5) })
}

fn mode_name(m: RenderMode) -> &'static str {
    match m {
        RenderMode::Solid => "solid",
        RenderMode::Wireframe => "wireframe",
        RenderMode::SolidEdges => "edges",
    }
}

fn kind_name(k: ViewKind) -> &'static str {
    match k {
        ViewKind::ThreeD => "3d",
        ViewKind::Plan => "plan",
        ViewKind::Section => "section",
        ViewKind::Elevation => "elevation",
        ViewKind::Detail => "detail",
    }
}

fn props_json(props: Vec<(String, String)>) -> Value {
    Value::Object(props.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
}

/// What a ray hit: layer, triangle, element, the point and the face normal (world, towards the
/// viewer).
struct Hit {
    layer: usize,
    triangle: usize,
    element: Option<u64>,
    point: Vec3,
    normal: Vec3,
}

// ---- commands ---------------------------------------------------------------------------------

impl App {
    /// Runs a remote command. Most answer at once; `open_file` answers when the model is in and
    /// `screenshot` when the window is captured.
    pub fn remote(&mut self, call: RemoteCall) -> Task<Message> {
        match call.cmd.as_str() {
            "open_file" => return self.remote_open(call),
            "get_schedule" => {
                let (answer, task) = self.remote_schedule(&call.args);
                call.answer(answer);
                return task;
            }
            "screenshot" => {
                self.finish_animation();
                self.remote.pending_shots.push(call);
                if self.remote.pending_shots.len() > 1 {
                    return Task::none(); // one capture answers all
                }
                // give the window a few frames to show the last changes, then capture it
                return Task::perform(async { std::thread::sleep(std::time::Duration::from_millis(200)) }, |_| ())
                    .then(|_| window::latest().and_then(window::screenshot))
                    .map(Message::RemoteShot);
            }
            _ => {}
        }
        let answer = self.command(&call.cmd, &call.args);
        // a remote command shows its end state at once: no camera transition
        self.finish_animation();
        call.answer(answer);
        Task::none()
    }

    fn finish_animation(&mut self) {
        if let Some((_, to, _)) = self.anim.take() {
            self.camera = to;
        }
    }

    fn command(&mut self, cmd: &str, a: &Value) -> Answer {
        match cmd {
            "get_state" => Ok(self.state_json()),
            "list_layers" => self.list_layers(a),
            "set_layer_visibility" => self.set_layer_visibility(a),
            "list_views" => self.list_views(),
            "apply_view" => self.remote_apply_view(a),
            "get_project_info" => self.project_info(),
            "list_schedules" => self.schedules_json(),
            "find_elements" => self.find_elements(a),
            "get_element" => {
                let id = a.get("id").and_then(id_of).ok_or("id is required")?;
                self.element_json(id)
            }
            "select" => self.remote_select(a),
            "pick" => self.remote_pick(a),
            "set_view" => self.remote_set_view(a),
            "set_camera" => self.set_camera(a),
            "navigate" => self.navigate(a),
            "zoom_to" => {
                let b = self.target_bounds(a.get("target").unwrap_or(&Value::Null))?;
                self.frame(&b);
                Ok(self.camera_json())
            }
            "set_display" => self.set_display(a),
            "explode" => self.remote_explode(a),
            "section_box" => self.remote_section_box(a),
            "section_plane" => self.remote_section_plane(a),
            "measure_distance" => self.measure_distance(a),
            "measure_angle" => self.measure_angle(a),
            "measure_between_elements" => self.measure_between_elements(a),
            "clear_measurements" => {
                let n = self.measurements.len();
                self.measurements.clear();
                Ok(json!({ "cleared": n }))
            }
            "cancel" => {
                let _ = self.update(Message::Escape);
                Ok(json!({ "placing_section_plane": self.plane_picking, "selection": self.selection_json() }))
            }
            _ => Err(format!("unknown command '{cmd}'")),
        }
    }

    fn require_model(&self) -> Result<Arc<Model>, String> {
        self.model.clone().ok_or_else(|| "no model is open: call open_file first".to_string())
    }

    // ---- files ----

    fn remote_open(&mut self, call: RemoteCall) -> Task<Message> {
        let Some(path) = call.args.get("path").and_then(Value::as_str).map(PathBuf::from) else {
            call.answer(Err("path is required".into()));
            return Task::none();
        };
        if !path.is_file() {
            call.answer(Err(format!("no such file: {}", path.display())));
            return Task::none();
        }
        if matches!(self.status, Status::Loading { .. }) {
            call.answer(Err("another file is still loading".into()));
            return Task::none();
        }
        let task = self.update(Message::Open(path));
        self.remote.pending_load.push(call);
        task
    }

    /// Answers the `open_file` calls once a load has finished.
    pub fn remote_loaded(&mut self) {
        let answer = match &self.status {
            Status::Error(e) => Err(e.clone()),
            _ => Ok(self.model_json()),
        };
        for call in self.remote.pending_load.drain(..) {
            call.answer(answer.clone());
        }
    }

    // ---- screenshots ----

    /// Answers the `screenshot` calls with the captured window (or its viewport).
    pub fn remote_shot(&mut self, shot: window::Screenshot) {
        for call in std::mem::take(&mut self.remote.pending_shots) {
            let answer = self.shot_answer(&shot, &call.args);
            call.answer(answer);
        }
    }

    fn shot_answer(&mut self, shot: &window::Screenshot, a: &Value) -> Answer {
        let viewport = a.get("area").and_then(Value::as_str).unwrap_or("viewport") != "window";
        let s = shot.scale_factor;
        let (mut w, mut h, mut rgba) = (shot.size.width, shot.size.height, shot.rgba.to_vec());
        if viewport {
            let x = ((self.viewport_origin.0 * s).round().max(0.0) as u32).min(w.saturating_sub(1));
            let y = ((self.viewport_origin.1 * s).round().max(0.0) as u32).min(h.saturating_sub(1));
            let cw = ((self.viewport_size.0 * s).round() as u32).clamp(1, w - x);
            let ch = ((self.viewport_size.1 * s).round() as u32).clamp(1, h - y);
            rgba = crop(&rgba, w, x, y, cw, ch);
            (w, h) = (cw, ch);
        }
        let max = a.get("max_size").and_then(Value::as_u64).unwrap_or(1600).clamp(64, 8192) as u32;
        if w.max(h) > max {
            let f = max as f32 / w.max(h) as f32;
            let (nw, nh) = (((w as f32 * f).round() as u32).max(1), ((h as f32 * f).round() as u32).max(1));
            rgba = downscale(&rgba, w, h, nw, nh);
            (w, h) = (nw, nh);
        }
        // the viewport shows through: make the PNG opaque
        for px in rgba.chunks_exact_mut(4) {
            px[3] = 255;
        }
        let png = encode_png(w, h, &rgba)?;
        let mut out = json!({ "area": if viewport { "viewport" } else { "window" }, "width": w, "height": h });
        if viewport {
            self.remote.shot_image = Some((w, h));
            out["note"] = json!("Screen coordinates for pick, select, section_plane and measure are pixels of this image.");
        }
        if let Some(path) = a.get("save_to").and_then(Value::as_str) {
            std::fs::write(path, &png).map_err(|e| format!("save_to {path}: {e}"))?;
            out["saved_to"] = json!(path);
        }
        out["png_base64"] = json!(base64(&png));
        Ok(out)
    }

    // ---- state ----

    fn model_json(&self) -> Value {
        let Some(m) = &self.model else { return Value::Null };
        let elements: usize = element_layers(m).len();
        json!({
            "file": m.path.display().to_string(),
            "format": m.format,
            "file_size_bytes": m.file_size,
            "load_time_s": round(m.load_time.as_secs_f32()),
            "layers": m.layers.len(),
            "vertices": m.vertex_count(),
            "triangles": m.triangle_count(),
            "elements": elements,
            "views": m.views.len(),
            "levels": m.project.levels.len(),
            "rooms": m.project.rooms.len(),
            "grids": m.project.grids.len(),
            "bounds": self.world_bounds().map(|b| jbounds(&b)),
            "units": "metres (Revit, SketchUp, glTF); OBJ, STL and PLY keep the file's own units",
            "up_axis": "Z",
        })
    }

    fn camera_json(&self) -> Value {
        let c = &self.camera;
        json!({
            "eye": jv(c.eye()),
            "target": jv(c.target),
            "view_direction": jv((c.target - c.eye()).normalize_or_zero()),
            "yaw_deg": round(c.yaw.to_degrees()),
            "pitch_deg": round(c.pitch.to_degrees()),
            "distance": round(c.distance),
            "fov_deg": round(c.fov_y.to_degrees()),
            "projection": if c.ortho { "orthographic" } else { "perspective" },
        })
    }

    fn section_json(&self) -> Value {
        match self.section {
            Some(s) => json!({
                "enabled": self.section_on,
                "min": jv(s.min),
                "max": jv(s.max),
                "turn_deg": round(s.turn.to_degrees()),
                "pivot": jv(s.pivot),
                "world_bounds": jbounds(&s.world_bounds()),
                "fill_cut": self.section_fill,
            }),
            None => Value::Null,
        }
    }

    fn plane_json(&self) -> Value {
        match (self.plane, self.world_bounds()) {
            (Some(p), Some(world)) => {
                let (lo, hi) = p.range(&world);
                json!({
                    "normal": jv(p.normal),
                    "offset": round(p.offset),
                    "cuts_away": "points with normal · p > offset",
                    "range": [round(lo), round(hi)],
                    "fill_cut": self.section_fill,
                })
            }
            _ => Value::Null,
        }
    }

    /// The selected elements (or layer) and the first one's properties.
    fn selection_json(&self) -> Value {
        let (Some(model), Some((layer, element))) = (&self.model, self.selected) else { return Value::Null };
        let mut ids: Vec<u64> = self
            .selection
            .iter()
            .filter_map(|&(l, first, _)| model.layers.get(l)?.element_at(first as usize / 3))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        json!({
            "layer": model.layers.get(layer).map(|l| l.name.clone()),
            "element": element,
            "elements": ids,
            "properties": element.map(|id| props_json(model.element_props(id))),
            "bounds": self.selection_bounds().map(|b| jbounds(&b)),
        })
    }

    fn state_json(&self) -> Value {
        let (status, message) = match &self.status {
            Status::Ready => ("ready", String::new()),
            Status::Loading { file, since, .. } => ("loading", format!("{file}, {:.0} s", since.elapsed().as_secs_f32())),
            Status::Saving { file, since } => ("saving", format!("{file}, {:.0} s", since.elapsed().as_secs_f32())),
            Status::Loaded(s) => ("loaded", s.clone()),
            Status::Error(e) => ("error", e.clone()),
        };
        json!({
            "status": status,
            "message": message,
            "model": self.model_json(),
            "camera": self.camera_json(),
            "display": {
                "mode": mode_name(self.mode),
                "grid": self.grid,
                "grid_lines": self.show_grids,
                "flip_up": self.flip_up,
            },
            "section_box": self.section_json(),
            "section_plane": self.plane_json(),
            "placing_section_plane": self.plane_picking,
            "selection": self.selection_json(),
            "active_view": self.active_view.and_then(|i| self.model.as_ref()?.views.get(i).map(|v| v.name.clone())),
            "viewport": { "width": round(self.viewport_size.0), "height": round(self.viewport_size.1) },
            "measurements": self.measurements.iter().map(|m| m.label.clone()).collect::<Vec<_>>(),
        })
    }

    // ---- layers, views, project ----

    fn list_layers(&self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let filter = a.get("filter").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let layers: Vec<Value> = model
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| filter.is_empty() || l.name.to_lowercase().contains(&filter))
            .map(|(i, l)| {
                let [r, g, b] = l.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                json!({
                    "index": i,
                    "name": l.name,
                    "visible": self.visible.get(i).copied().unwrap_or(true),
                    "triangles": l.triangle_count(),
                    "elements": l.elements.len(),
                    "color": format!("#{r:02x}{g:02x}{b:02x}"),
                })
            })
            .collect();
        Ok(json!({ "count": layers.len(), "layers": layers }))
    }

    /// Layer indices for a list of indices and/or names (exact name first, else name contains).
    fn layer_indices(&self, model: &Model, spec: &Value) -> Result<Vec<usize>, String> {
        if spec.as_str() == Some("all") {
            return Ok((0..model.layers.len()).collect());
        }
        let items: Vec<&Value> = match spec {
            Value::Array(a) => a.iter().collect(),
            Value::Null => return Err("layers is required (indices, names or \"all\")".into()),
            v => vec![v],
        };
        let mut out = Vec::new();
        for item in items {
            if let Some(i) = item.as_u64() {
                let i = i as usize;
                if i >= model.layers.len() {
                    return Err(format!("no layer {i}"));
                }
                out.push(i);
            } else if let Some(name) = item.as_str() {
                let lower = name.to_lowercase();
                let exact: Vec<usize> = (0..model.layers.len()).filter(|&i| model.layers[i].name.to_lowercase() == lower).collect();
                let found = if exact.is_empty() {
                    (0..model.layers.len()).filter(|&i| model.layers[i].name.to_lowercase().contains(&lower)).collect()
                } else {
                    exact
                };
                if found.is_empty() {
                    return Err(format!("no layer named '{name}'"));
                }
                out.extend(found);
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    fn set_layer_visibility(&mut self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let layers = self.layer_indices(&model, a.get("layers").unwrap_or(&Value::Null))?;
        if flag(a, "isolate").unwrap_or(false) {
            for (i, v) in self.visible.iter_mut().enumerate() {
                *v = layers.contains(&i);
            }
        } else {
            let visible = flag(a, "visible").ok_or("visible (true/false) or isolate is required")?;
            for &i in &layers {
                let _ = self.update(Message::Layer(i, visible));
            }
        }
        let shown: Vec<&str> = (0..model.layers.len()).filter(|&i| self.visible[i]).map(|i| model.layers[i].name.as_str()).collect();
        Ok(json!({ "changed": layers.len(), "visible_layers": shown.len(), "hidden_layers": model.layers.len() - shown.len() }))
    }

    fn list_views(&self) -> Answer {
        let model = self.require_model()?;
        let views: Vec<Value> = model
            .views
            .iter()
            .enumerate()
            .map(|(i, v)| json!({ "index": i, "name": v.name, "kind": kind_name(v.kind), "level": v.level, "active": self.active_view == Some(i) }))
            .collect();
        Ok(json!({ "count": views.len(), "views": views }))
    }

    fn remote_apply_view(&mut self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let spec = a.get("view").ok_or("view (index or name) is required")?;
        let i = match spec.as_u64() {
            Some(i) => i as usize,
            None => {
                let name = spec.as_str().ok_or("view must be an index or a name")?.to_lowercase();
                model
                    .views
                    .iter()
                    .position(|v| v.name.to_lowercase() == name)
                    .or_else(|| model.views.iter().position(|v| v.name.to_lowercase().contains(&name)))
                    .ok_or(format!("no view named '{name}'"))?
            }
        };
        if i >= model.views.len() {
            return Err(format!("no view {i}"));
        }
        self.apply_view(i);
        self.finish_animation();
        Ok(json!({ "view": model.views[i].name, "camera": self.camera_json(), "section_box": self.section_json() }))
    }

    fn project_info(&self) -> Answer {
        let model = self.require_model()?;
        let p = &model.project;
        Ok(json!({
            "info": Value::Object(p.info.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
            "levels": p.levels.iter().map(|(n, z)| json!({ "name": n, "elevation": round(*z) })).collect::<Vec<_>>(),
            "phases": p.phases,
            "grids": p.grids.iter().map(|g| json!({ "name": g.name, "a": g.a, "b": g.b })).collect::<Vec<_>>(),
            "rooms": p.rooms.iter().map(|r| json!({
                "number": r.number,
                "name": r.name,
                "level": r.level,
                "parameters": Value::Object(r.extra.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
            })).collect::<Vec<_>>(),
        }))
    }

    // ---- elements ----

    fn find_elements(&self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let text = a.get("text").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let layer = a.get("layer").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let conditions: Vec<(String, String)> = a
            .get("where")
            .and_then(Value::as_object)
            .map(|o| o.iter().map(|(k, v)| (k.to_lowercase(), v.as_str().map_or_else(|| v.to_string(), str::to_string).to_lowercase())).collect())
            .unwrap_or_default();
        let group_by = a.get("group_by").and_then(Value::as_str);
        let limit = a.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
        let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;

        let mut matches: Vec<(u64, Vec<usize>, Vec<(String, String)>)> = Vec::new();
        for (id, layers) in element_layers(&model) {
            if !layer.is_empty() && !layers.iter().any(|&l| model.layers[l].name.to_lowercase().contains(&layer)) {
                continue;
            }
            let mut props = model.element_props(id);
            if !props.iter().any(|(k, _)| k == "category") {
                props.push(("category".into(), model.layers[layers[0]].name.clone()));
            }
            let value = |key: &str| props.iter().find(|(k, _)| k.to_lowercase() == key).map(|(_, v)| v.to_lowercase());
            if !conditions.iter().all(|(k, v)| value(k).is_some_and(|x| x == *v || x.contains(v.as_str()))) {
                continue;
            }
            if !text.is_empty() && !props.iter().any(|(_, v)| v.to_lowercase().contains(&text)) && id.to_string() != text {
                continue;
            }
            matches.push((id, layers, props));
        }
        let total = matches.len();
        if let Some(key) = group_by {
            let mut groups: BTreeMap<String, usize> = BTreeMap::new();
            for (_, _, props) in &matches {
                let v = props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map_or("(none)".to_string(), |(_, v)| v.clone());
                *groups.entry(v).or_default() += 1;
            }
            return Ok(json!({ "total": total, "group_by": key, "groups": groups }));
        }
        let shown: Vec<Value> = matches
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(id, layers, props)| {
                let keep = ["category", "family", "type", "name", "level", "class"];
                let summary: Map<String, Value> =
                    props.into_iter().filter(|(k, _)| keep.contains(&k.as_str())).map(|(k, v)| (k, Value::String(v))).collect();
                json!({ "id": id, "layer": model.layers[layers[0]].name, "properties": summary })
            })
            .collect();
        Ok(json!({ "total": total, "offset": offset, "returned": shown.len(), "elements": shown }))
    }

    /// The triangles of an element in world space.
    fn element_triangles(&self, model: &Model, id: u64) -> Vec<[Vec3; 3]> {
        let mm = self.model_matrix();
        let mut out = Vec::new();
        for (l, first, count) in model.element_ranges(id) {
            let layer = &model.layers[l];
            let Some(tris) = layer.indices.get(first as usize..(first + count) as usize) else { continue };
            for t in tris.chunks_exact(3) {
                let at = |i: u32| layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| mm.transform_point3(Vec3::new(p[0], p[1], p[2])));
                if let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) {
                    out.push([a, b, c]);
                }
            }
        }
        out
    }

    fn element_json(&self, id: u64) -> Answer {
        let model = self.require_model()?;
        let ranges = model.element_ranges(id);
        if ranges.is_empty() {
            return Err(format!("no element {id}"));
        }
        let tris = self.element_triangles(&model, id);
        let mut b = Bounds::empty();
        let mut area = 0.0f64;
        for t in &tris {
            for p in t {
                b.extend(p.to_array());
            }
            area += 0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length() as f64;
        }
        // volume only for closed meshes (divergence theorem); the model matrix only turns
        let closed = ranges.iter().all(|&(l, first, count)| {
            let layer = &model.layers[l];
            layer.indices.get(first as usize..(first + count) as usize).is_some_and(|t| crate::model::encloses_volume(&layer.positions, t))
        });
        let volume = closed.then(|| tris.iter().map(|t| t[0].dot(t[1].cross(t[2])) as f64).sum::<f64>().abs() / 6.0);
        let mut layers: Vec<&str> = ranges.iter().map(|r| model.layers[r.0].name.as_str()).collect();
        layers.dedup();
        Ok(json!({
            "id": id,
            "layers": layers,
            "properties": props_json(model.element_props(id)),
            "triangles": tris.len(),
            "bounds": jbounds(&b),
            "surface_area_m2": (area * 1e4).round() / 1e4,
            "closed": closed,
            "volume_m3": volume.map(|v| (v * 1e6).round() / 1e6),
        }))
    }

    /// Bounds of "model", "selection", an element id or a list of element ids (world).
    fn target_bounds(&self, target: &Value) -> Result<Bounds, String> {
        let model = self.require_model()?;
        let ids: Vec<u64> = match target {
            Value::Null => return self.world_bounds().ok_or("no model".into()),
            Value::String(s) if s == "model" => return self.world_bounds().ok_or("no model".into()),
            Value::String(s) if s == "selection" => return self.selection_bounds().ok_or("nothing is selected".into()),
            Value::Array(a) => a.iter().filter_map(id_of).collect(),
            v => vec![id_of(v).ok_or("target: \"model\", \"selection\", an element id or a list of ids")?],
        };
        let mut b = Bounds::empty();
        for id in ids {
            for t in self.element_triangles(&model, id) {
                t.iter().for_each(|p| b.extend(p.to_array()));
            }
        }
        if b.is_valid() { Ok(b) } else { Err("no such elements".into()) }
    }

    fn remote_select(&mut self, a: &Value) -> Answer {
        let model = self.require_model()?;
        if flag(a, "clear").unwrap_or(false) {
            self.selected = None;
            self.selection.clear();
            return Ok(Value::Null);
        }
        let ids: Vec<u64> = match (a.get("ids"), a.get("id")) {
            (Some(Value::Array(list)), _) => list.iter().filter_map(id_of).collect(),
            (_, Some(v)) => vec![id_of(v).ok_or("id must be an element id")?],
            _ => return Err("ids (list of element ids), id or clear is required".into()),
        };
        let mut selection = Vec::new();
        for &id in &ids {
            let ranges = model.element_ranges(id);
            if ranges.is_empty() {
                return Err(format!("no element {id}"));
            }
            selection.extend(ranges);
        }
        let Some(&(layer, ..)) = selection.first() else { return Err("no elements given".into()) };
        self.selected = Some((layer, Some(ids[0])));
        self.selection = selection;
        if flag(a, "zoom").unwrap_or(false) {
            if let Some(b) = self.selection_bounds() {
                self.frame(&b);
            }
        }
        Ok(self.selection_json())
    }

    /// Screen pixels of the last viewport screenshot to viewport logical pixels.
    fn to_viewport(&self, x: f32, y: f32) -> (f32, f32) {
        match self.remote.shot_image {
            Some((w, h)) => (x / w as f32 * self.viewport_size.0, y / h as f32 * self.viewport_size.1),
            None => (x, y),
        }
    }

    fn screen_xy(v: &Value) -> Option<(f32, f32)> {
        let a = v.as_array().filter(|a| a.len() == 2)?;
        Some((a[0].as_f64()? as f32, a[1].as_f64()? as f32))
    }

    /// What is under viewport point (x, y) (logical pixels), as the click picks it.
    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let model = self.model.clone()?;
        let (width, height) = (self.viewport_size.0.max(1.0), self.viewport_size.1.max(1.0));
        let mm = self.model_matrix();
        let inv = (self.camera.view_proj(width / height) * mm).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let origin = inv.project_point3(Vec3::new(nx, ny, 0.0));
        let dir = inv.project_point3(Vec3::new(nx, ny, 1.0)) - origin;
        let (section, plane) = (self.active_section(), self.plane);
        let keep = |p: Vec3| {
            let w = mm.transform_point3(p);
            section.is_none_or(|s| s.contains(w)) && plane.is_none_or(|pl| !pl.cuts(w))
        };
        // the exploded view moves the triangles: their offsets back in model space
        let back = mm.inverse();
        let shift = |l: usize, i: u32| back.transform_vector3(self.explode_offset(l, i));
        let (layer, triangle) = pick_triangle(&model, &self.visible, origin, dir, &shift, &keep)?;
        let s = model.layers[layer].indices.get(3 * triangle).map_or(Vec3::ZERO, |&i| shift(layer, i));
        let (point, normal) = face_at(&model.layers[layer], triangle, origin - s, dir)?;
        let (point, mut normal) = (mm.transform_point3(point + s), mm.transform_vector3(normal).normalize_or_zero());
        if normal.dot(mm.transform_vector3(dir)) > 0.0 {
            normal = -normal;
        }
        Some(Hit { layer, triangle, element: model.layers[layer].element_at(triangle), point, normal })
    }

    /// The corner of the hit triangle nearest to the hit point (world).
    fn snap(&self, hit: &Hit) -> Vec3 {
        let Some(model) = &self.model else { return hit.point };
        let layer = &model.layers[hit.layer];
        let mm = self.model_matrix();
        let corners = layer.indices.get(3 * hit.triangle..3 * hit.triangle + 3).unwrap_or(&[]).iter().filter_map(|&i| {
            layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| mm.transform_point3(Vec3::new(p[0], p[1], p[2])) + self.explode_offset(hit.layer, i))
        });
        corners.min_by(|a, b| a.distance(hit.point).total_cmp(&b.distance(hit.point))).unwrap_or(hit.point)
    }

    fn hit_json(&self, hit: &Hit) -> Value {
        let model = self.model.as_ref();
        json!({
            "point": jv(hit.point),
            "normal": jv(hit.normal),
            "layer": model.map(|m| m.layers[hit.layer].name.clone()),
            "element": hit.element,
            "properties": hit.element.and_then(|id| Some(props_json(model?.element_props(id)))),
            "distance_from_camera": round(hit.point.distance(self.camera.eye())),
        })
    }

    fn remote_pick(&mut self, a: &Value) -> Answer {
        self.require_model()?;
        let (x, y) = (num(a, "x").ok_or("x is required")?, num(a, "y").ok_or("y is required")?);
        let (vx, vy) = self.to_viewport(x, y);
        let hit = self.hit_at(vx, vy);
        if flag(a, "select").unwrap_or(false) {
            self.pick(vx, vy, self.viewport_size.0, self.viewport_size.1);
        }
        Ok(match hit {
            Some(h) => {
                let mut out = self.hit_json(&h);
                out["snapped_corner"] = jv(self.snap(&h));
                out
            }
            None => json!({ "hit": false, "note": "nothing there (empty space, hidden layer or cut away)" }),
        })
    }

    // ---- camera ----

    /// Frames `b` from the current direction, keeping near and far for the whole model.
    fn frame(&mut self, b: &Bounds) {
        self.anim = None;
        self.camera.fit(b, self.aspect());
        if let Some(world) = self.world_bounds() {
            self.camera.scene_radius = self.camera.scene_radius.max(world.radius());
        }
    }

    fn remote_set_view(&mut self, a: &Value) -> Answer {
        let preset = match a.get("preset").and_then(Value::as_str).unwrap_or("") {
            "iso" => ViewPreset::Iso,
            "top" => ViewPreset::Top,
            "bottom" => ViewPreset::Bottom,
            "front" => ViewPreset::Front,
            "back" => ViewPreset::Back,
            "left" => ViewPreset::Left,
            "right" => ViewPreset::Right,
            p => return Err(format!("unknown preset '{p}': iso, top, bottom, front, back, left, right")),
        };
        let _ = self.update(Message::Preset(preset));
        Ok(self.camera_json())
    }

    fn set_camera(&mut self, a: &Value) -> Answer {
        self.anim = None;
        let max_pitch = 89.9f32.to_radians();
        let c = &mut self.camera;
        match (a.get("eye").and_then(vec3), a.get("target").and_then(vec3)) {
            (Some(eye), Some(target)) => {
                let f = (target - eye).try_normalize().ok_or("eye and target are the same point")?;
                c.target = target;
                c.distance = eye.distance(target).max(1e-3);
                c.yaw = (-f.y).atan2(-f.x);
                c.pitch = (-f.z).clamp(-1.0, 1.0).asin().clamp(-max_pitch, max_pitch);
            }
            (Some(eye), None) => {
                // keep looking the same way from the new eye
                let f = (c.target - c.eye()).normalize_or(Vec3::Y);
                c.target = eye + f * c.distance;
            }
            (None, Some(target)) => c.target = target,
            (None, None) => {}
        }
        if let Some(v) = num(a, "yaw_deg") {
            c.yaw = v.to_radians();
        }
        if let Some(v) = num(a, "pitch_deg") {
            c.pitch = v.to_radians().clamp(-max_pitch, max_pitch);
        }
        if let Some(v) = num(a, "distance") {
            c.distance = v.max(1e-3);
        }
        if let Some(v) = num(a, "fov_deg") {
            c.fov_y = v.clamp(10.0, 120.0).to_radians();
        }
        match a.get("projection").and_then(Value::as_str) {
            Some("orthographic") => c.ortho = true,
            Some("perspective") => c.ortho = false,
            Some(p) => return Err(format!("unknown projection '{p}'")),
            None => {}
        }
        // the far plane must reach the model from wherever the camera is now
        if let Some(world) = self.world_bounds() {
            let c = &mut self.camera;
            c.scene_radius = c.scene_radius.max(world.radius() + c.target.distance(Vec3::from_array(world.center())));
        }
        Ok(self.camera_json())
    }

    fn navigate(&mut self, a: &Value) -> Answer {
        self.anim = None;
        let max_pitch = 89.9f32.to_radians();
        let c = &mut self.camera;
        if let Some(d) = num(a, "orbit_yaw_deg") {
            c.yaw += d.to_radians();
        }
        if let Some(d) = num(a, "orbit_pitch_deg") {
            c.pitch = (c.pitch + d.to_radians()).clamp(-max_pitch, max_pitch);
        }
        let forward = (c.target - c.eye()).normalize_or(Vec3::Y);
        let right = forward.cross(Vec3::Z).normalize_or(Vec3::X);
        let up = right.cross(forward);
        c.target += right * num(a, "pan_right").unwrap_or(0.0) + up * num(a, "pan_up").unwrap_or(0.0) + forward * num(a, "move_forward").unwrap_or(0.0);
        if let Some(f) = num(a, "zoom") {
            if f <= 0.0 {
                return Err("zoom is a factor > 0 (2 = twice as close)".into());
            }
            c.distance = (c.distance / f).max(1e-3);
        }
        Ok(self.camera_json())
    }

    // ---- display ----

    fn set_display(&mut self, a: &Value) -> Answer {
        if let Some(m) = a.get("mode").and_then(Value::as_str) {
            self.mode = match m {
                "solid" => RenderMode::Solid,
                "wireframe" => RenderMode::Wireframe,
                "edges" => RenderMode::SolidEdges,
                _ => return Err(format!("unknown mode '{m}': solid, wireframe, edges")),
            };
        }
        if let Some(v) = flag(a, "grid") {
            self.grid = v;
        }
        if let Some(v) = flag(a, "grid_lines") {
            self.show_grids = v;
        }
        if let Some(v) = flag(a, "section_fill") {
            self.section_fill = v;
        }
        if let Some(v) = flag(a, "flip_up") {
            if v != self.flip_up {
                let _ = self.update(Message::ToggleFlipUp);
            }
        }
        Ok(json!({
            "mode": mode_name(self.mode),
            "grid": self.grid,
            "grid_lines": self.show_grids,
            "flip_up": self.flip_up,
            "section_fill": self.section_fill,
        }))
    }

    /// The exploded view: on or off, per storey or per element, and how far per axis (percent).
    fn remote_explode(&mut self, a: &Value) -> Answer {
        let index = self.explode_index.clone().ok_or("no model is open: call open_file first")?;
        if let Some(m) = a.get("mode").and_then(Value::as_str) {
            let mode = match m {
                "storeys" if index.storeys() == 0 => return Err("this model has no storeys: use mode elements".into()),
                "storeys" => ExplodeMode::Storeys,
                "elements" => ExplodeMode::Elements,
                _ => return Err(format!("unknown mode '{m}': storeys, elements")),
            };
            let _ = self.update(Message::ExplodeMode(mode));
        }
        if let Some(percent) = a.get("percent").and_then(Value::as_array) {
            for (axis, value) in percent.iter().enumerate().take(3) {
                if let Some(v) = value.as_f64() {
                    let _ = self.update(Message::Explode(axis, v as f32));
                }
            }
        }
        if let Some(on) = flag(a, "enabled") {
            if on != self.explode_on {
                let _ = self.update(Message::ToggleExplode);
            }
        }
        Ok(json!({
            "enabled": self.explode_on,
            "mode": match self.explode_mode { ExplodeMode::Storeys => "storeys", ExplodeMode::Elements => "elements" },
            "percent": self.explode,
        }))
    }

    // ---- sections ----

    fn remote_section_box(&mut self, a: &Value) -> Answer {
        let world = self.world_bounds().ok_or("no model is open: call open_file first")?;
        if flag(a, "reset").unwrap_or(false) {
            self.section = Some(SectionBox::around(&world));
            self.section_on = true;
        }
        if let Some(target) = a.get("fit") {
            let b = self.target_bounds(target)?;
            self.section = Some(SectionBox::around(&b));
            self.section_on = true;
        }
        if a.get("min").is_some() || a.get("max").is_some() {
            let current = self.section.unwrap_or_else(|| SectionBox::around(&world));
            let min = a.get("min").map_or(Some(current.min), |v| partial_vec3(v, current.min)).ok_or("min must be [x, y, z]")?;
            let max = a.get("max").map_or(Some(current.max), |v| partial_vec3(v, current.max)).ok_or("max must be [x, y, z]")?;
            if (0..3).any(|i| max[i] <= min[i]) {
                return Err("max must be above min on every axis".into());
            }
            self.section = Some(SectionBox { min, max, turn: current.turn, pivot: if current.turn == 0.0 { (min + max) * 0.5 } else { current.pivot } });
            self.section_on = true;
        }
        if let Some(deg) = num(a, "turn_deg") {
            let s = self.section.get_or_insert_with(|| SectionBox::around(&world));
            s.turn_by(deg.to_radians() - s.turn);
            self.section_on = true;
        }
        if let Some(on) = flag(a, "enabled") {
            if on && self.section.is_none() {
                self.section = Some(SectionBox::around(&world));
            }
            self.section_on = on;
        }
        Ok(self.section_json())
    }

    fn remote_section_plane(&mut self, a: &Value) -> Answer {
        let world = self.world_bounds().ok_or("no model is open: call open_file first")?;
        if flag(a, "remove").unwrap_or(false) {
            self.plane = None;
            self.plane_picking = false;
            return Ok(Value::Null);
        }
        if let Some(screen) = a.get("screen") {
            let (x, y) = Self::screen_xy(screen).ok_or("screen must be [x, y]")?;
            let (vx, vy) = self.to_viewport(x, y);
            let hit = self.hit_at(vx, vy).ok_or("no face at that screen point")?;
            self.plane = Some(SectionPlane::on_face(hit.point, hit.normal, &world).ok_or("the face has no usable normal")?);
        } else if let Some(axis) = a.get("axis").and_then(Value::as_str) {
            let normal = match axis {
                "x" => Vec3::X,
                "y" => Vec3::Y,
                "z" => Vec3::Z,
                _ => return Err("axis is x, y or z".into()),
            };
            let at = num(a, "at").ok_or("at (the coordinate along the axis) is required with axis")?;
            self.plane = Some(SectionPlane { normal, offset: at });
        } else if let Some(normal) = a.get("normal").and_then(vec3) {
            let normal = normal.try_normalize().ok_or("normal has no length")?;
            let point = a.get("point").and_then(vec3).ok_or("point is required with normal")?;
            self.plane = Some(SectionPlane { normal, offset: normal.dot(point) });
        }
        self.plane_picking = false;
        let plane = self.plane.as_mut().ok_or("no section plane: give screen, axis + at, or normal + point")?;
        if flag(a, "flip").unwrap_or(false) {
            plane.flip();
        }
        if let Some(offset) = num(a, "offset") {
            plane.offset = offset;
        }
        if let Some(d) = num(a, "move") {
            plane.move_by(d, &world);
        }
        Ok(self.plane_json())
    }

    // ---- measuring ----

    /// A point: [x, y, z] (world), {"screen": [x, y], "snap": bool} (what is under that pixel of
    /// the last viewport screenshot, optionally snapped to the nearest corner of the face) or
    /// {"element": id} (the centre of its bounds).
    fn point(&self, v: &Value) -> Result<(Vec3, Value), String> {
        if let Some(p) = vec3(v).filter(|_| v.get("screen").is_none() && v.get("element").is_none()) {
            return Ok((p, json!({ "point": jv(p) })));
        }
        if let Some(screen) = v.get("screen") {
            let (x, y) = Self::screen_xy(screen).ok_or("screen must be [x, y]")?;
            let (vx, vy) = self.to_viewport(x, y);
            let hit = self.hit_at(vx, vy).ok_or(format!("nothing under screen point ({x}, {y})"))?;
            let p = if flag(v, "snap").unwrap_or(false) { self.snap(&hit) } else { hit.point };
            let mut info = self.hit_json(&hit);
            info["point"] = jv(p);
            return Ok((p, info));
        }
        if let Some(id) = v.get("element").and_then(id_of) {
            let b = self.target_bounds(&json!(id))?;
            let c = Vec3::from_array(b.center());
            return Ok((c, json!({ "point": jv(c), "element": id, "at": "centre of its bounds" })));
        }
        Err("a point is [x, y, z], {\"screen\": [x, y]} or {\"element\": id}".into())
    }

    fn add_measurement(&mut self, a: &Value, points: Vec<Vec3>, label: String) {
        if flag(a, "show").unwrap_or(true) {
            self.measurements.push(Measurement { points, label });
        }
    }

    fn measure_distance(&mut self, a: &Value) -> Answer {
        self.require_model()?;
        let (pa, ia) = self.point(a.get("a").ok_or("a is required")?)?;
        let (pb, ib) = self.point(a.get("b").ok_or("b is required")?)?;
        let d = pb - pa;
        let distance = d.length();
        self.add_measurement(a, vec![pa, pb], format!("{distance:.3} m"));
        Ok(json!({
            "distance": round(distance),
            "delta": jv(d),
            "horizontal": round(d.truncate().length()),
            "vertical": round(d.z.abs()),
            "a": ia,
            "b": ib,
        }))
    }

    fn measure_angle(&mut self, a: &Value) -> Answer {
        self.require_model()?;
        let (pa, _) = self.point(a.get("a").ok_or("a is required")?)?;
        let (pv, _) = self.point(a.get("vertex").ok_or("vertex is required")?)?;
        let (pb, _) = self.point(a.get("b").ok_or("b is required")?)?;
        let (u, v) = ((pa - pv).try_normalize().ok_or("a is on the vertex")?, (pb - pv).try_normalize().ok_or("b is on the vertex")?);
        let degrees = u.dot(v).clamp(-1.0, 1.0).acos().to_degrees();
        self.add_measurement(a, vec![pa, pv, pb], format!("{degrees:.1}°"));
        Ok(json!({ "angle_deg": round(degrees), "a": jv(pa), "vertex": jv(pv), "b": jv(pb) }))
    }

    fn measure_between_elements(&mut self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let ida = a.get("a").and_then(id_of).ok_or("a (element id) is required")?;
        let idb = a.get("b").and_then(id_of).ok_or("b (element id) is required")?;
        let (ta, tb) = (self.element_triangles(&model, ida), self.element_triangles(&model, idb));
        if ta.is_empty() || tb.is_empty() {
            return Err(format!("no element {}", if ta.is_empty() { ida } else { idb }));
        }
        let (d, p, q) = closest_between(&ta, &tb);
        self.add_measurement(a, vec![p, q], format!("{d:.3} m"));
        Ok(json!({
            "distance": round(d),
            "closest_on_a": jv(p),
            "closest_on_b": jv(q),
            "delta": jv(q - p),
            "note": "shortest distance from the corners of one element to the faces of the other, both ways",
        }))
    }
}

/// Element id → the layers that draw it, for every element in the model.
fn element_layers(model: &Model) -> BTreeMap<u64, Vec<usize>> {
    let mut out: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    for (l, layer) in model.layers.iter().enumerate() {
        for &(id, ..) in &layer.elements {
            let v = out.entry(id).or_default();
            if v.last() != Some(&l) {
                v.push(l);
            }
        }
    }
    out
}

/// The closest point to `p` on triangle abc (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Shortest distance between two triangle sets, corners of each against the faces of the other:
/// (distance, point on the first, point on the second). Big pairs take every n-th corner.
pub fn closest_between(ta: &[[Vec3; 3]], tb: &[[Vec3; 3]]) -> (f32, Vec3, Vec3) {
    const BUDGET: usize = 40_000_000;
    let mut best = (f32::MAX, Vec3::ZERO, Vec3::ZERO);
    for (from, to, swap) in [(ta, tb, false), (tb, ta, true)] {
        let mut corners: Vec<Vec3> = from.iter().flatten().copied().collect();
        corners.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap_or(std::cmp::Ordering::Equal));
        corners.dedup();
        let step = (corners.len() * to.len() / BUDGET).max(1);
        for &p in corners.iter().step_by(step) {
            for t in to {
                let q = closest_on_triangle(p, t[0], t[1], t[2]);
                let d = p.distance(q);
                if d < best.0 {
                    best = if swap { (d, q, p) } else { (d, p, q) };
                }
            }
        }
    }
    best
}

// ---- images -----------------------------------------------------------------------------------

fn crop(rgba: &[u8], width: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (w * 4) as usize]);
    }
    out
}

/// Box-filter downscale of RGBA8.
fn downscale(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for oy in 0..nh {
        let (y0, y1) = (oy * h / nh, ((oy + 1) * h / nh).max(oy * h / nh + 1).min(h));
        for ox in 0..nw {
            let (x0, x1) = (ox * w / nw, ((ox + 1) * w / nw).max(ox * w / nw + 1).min(w));
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += rgba[i + c] as u32;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            let o = ((oy * nw + ox) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    out
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(png)
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;

    /// A 2 × 1 × 3 m box (element 7, closed) and a flat 1 × 1 m sheet 4 m to the right (element 8).
    fn app() -> App {
        let (x0, x1, y0, y1, z0, z1) = (0.0, 2.0, 0.0, 1.0, 0.0, 3.0);
        let mut positions = vec![
            x0, y0, z0, x1, y0, z0, x1, y1, z0, x0, y1, z0, x0, y0, z1, x1, y0, z1, x1, y1, z1, x0, y1, z1,
        ];
        let mut indices: Vec<u32> = vec![
            0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7,
        ];
        positions.extend_from_slice(&[6.0, 0.0, 0.0, 7.0, 0.0, 0.0, 7.0, 1.0, 0.0, 6.0, 1.0, 0.0]);
        indices.extend_from_slice(&[8, 9, 10, 8, 10, 11]);
        let normals = crate::model::smooth_normals(&positions, &indices);
        let layer = Layer {
            name: "Walls".into(),
            positions,
            normals,
            indices,
            elements: vec![(7, 0, 36), (8, 36, 6)],
            props: vec![
                (7, r#"{"category":"Walls","type":"Basic Wall 200","level":"Level 1"}"#.into()),
                (8, r#"{"category":"Floors","level":"Level 2"}"#.into()),
            ],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("test.rvt"), "Revit", vec![layer])));
        app
    }

    fn run(app: &mut App, cmd: &str, args: Value) -> Answer {
        let (call, rx) = RemoteCall::new(cmd, args);
        let _ = app.remote(call);
        rx.try_recv().expect("answered at once")
    }

    #[test]
    fn pulls_the_model_apart() {
        let mut app = app();
        let on = run(&mut app, "explode", json!({ "enabled": true, "mode": "elements", "percent": [50, null, 20] })).expect("explode");
        assert_eq!(on, json!({ "enabled": true, "mode": "elements", "percent": [50.0, 0.0, 20.0] }));
        assert!(run(&mut app, "explode", json!({ "mode": "sideways" })).is_err());
        let off = run(&mut app, "explode", json!({ "enabled": false })).expect("explode");
        assert_eq!(off["enabled"], false);
    }

    #[test]
    fn measures_elements_and_distances() {
        let mut app = app();
        let e = run(&mut app, "get_element", json!({ "id": 7 })).unwrap();
        assert_eq!(e["closed"], true);
        assert!((e["volume_m3"].as_f64().unwrap() - 6.0).abs() < 1e-4, "{e}");
        assert!((e["surface_area_m2"].as_f64().unwrap() - 22.0).abs() < 1e-3, "{e}");
        assert_eq!(e["bounds"]["size"], json!([2.0, 1.0, 3.0]));
        let d = run(&mut app, "measure_distance", json!({ "a": [0, 0, 0], "b": [3, 4, 0] })).unwrap();
        assert_eq!(d["distance"], json!(5.0));
        let between = run(&mut app, "measure_between_elements", json!({ "a": 7, "b": "8" })).unwrap();
        assert!((between["distance"].as_f64().unwrap() - 4.0).abs() < 1e-4, "{between}");
        let angle = run(&mut app, "measure_angle", json!({ "a": [1, 0, 0], "vertex": [0, 0, 0], "b": [0, 2, 0] })).unwrap();
        assert_eq!(angle["angle_deg"], json!(90.0));
        assert_eq!(app.measurements.len(), 3);
        run(&mut app, "clear_measurements", json!({})).unwrap();
        assert!(app.measurements.is_empty());
    }

    #[test]
    fn finds_selects_and_cuts() {
        let mut app = app();
        let found = run(&mut app, "find_elements", json!({ "where": { "category": "walls" } })).unwrap();
        assert_eq!(found["total"], json!(1));
        assert_eq!(found["elements"][0]["id"], json!(7));
        let groups = run(&mut app, "find_elements", json!({ "group_by": "level" })).unwrap();
        assert_eq!(groups["groups"]["Level 2"], json!(1));
        let sel = run(&mut app, "select", json!({ "ids": [7, 8] })).unwrap();
        assert_eq!(sel["elements"], json!([7, 8]));
        assert!(run(&mut app, "select", json!({ "id": 99 })).is_err());

        let plane = run(&mut app, "section_plane", json!({ "axis": "z", "at": 1.5 })).unwrap();
        assert_eq!(plane["offset"], json!(1.5));
        assert!(app.plane.unwrap().cuts(Vec3::new(1.0, 0.5, 2.0)));
        run(&mut app, "section_plane", json!({ "flip": true })).unwrap();
        assert!(!app.plane.unwrap().cuts(Vec3::new(1.0, 0.5, 2.0)));
        let b = run(&mut app, "section_box", json!({ "min": [null, null, 0.5], "max": [1, null, null] })).unwrap();
        assert_eq!(b["enabled"], true);
        assert_eq!(app.section.unwrap().max.x, 1.0);
        assert_eq!(app.section.unwrap().min.z, 0.5);

        let layers = run(&mut app, "set_layer_visibility", json!({ "layers": ["walls"], "visible": false })).unwrap();
        assert_eq!(layers["hidden_layers"], json!(1));
        assert!(run(&mut app, "set_layer_visibility", json!({ "layers": ["roofs"], "visible": false })).is_err());
    }

    #[test]
    fn camera_commands_land_at_once() {
        let mut app = app();
        run(&mut app, "set_view", json!({ "preset": "top" })).unwrap();
        assert!(app.anim.is_none());
        assert!(app.camera.pitch.to_degrees() > 89.0);
        let cam = run(&mut app, "set_camera", json!({ "eye": [10, 0, 0], "target": [0, 0, 0] })).unwrap();
        assert_eq!(cam["eye"], json!([10.0, 0.0, 0.0]));
        assert_eq!(cam["view_direction"], json!([-1.0, 0.0, 0.0]));
        let zoomed = run(&mut app, "navigate", json!({ "zoom": 2 })).unwrap();
        assert_eq!(zoomed["distance"], json!(5.0));
        // looking along −X at the box from x = 10: the middle of the viewport hits its +X face
        app.viewport_size = (800.0, 600.0);
        app.camera.distance = 10.0;
        app.camera.target = Vec3::new(0.0, 0.5, 1.5);
        let hit = run(&mut app, "pick", json!({ "x": 400, "y": 300 })).unwrap();
        assert_eq!(hit["element"], json!(7));
        assert_eq!(hit["normal"], json!([1.0, 0.0, 0.0]));
        assert!((hit["point"][0].as_f64().unwrap() - 2.0).abs() < 1e-3, "{hit}");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn downscale_averages() {
        let img = [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        assert_eq!(downscale(&img, 2, 2, 1, 1), vec![127, 127, 127, 255]);
    }
}
