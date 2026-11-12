//! Remote control: a running viewer takes commands over a local socket (see `remote_link` for the
//! protocol), so an AI agent can do through the MCP server `open-3d-viewer-mcp` what a person does
//! in the window: open files, look around, show and hide layers, select, cut, measure and take
//! screenshots. Every command goes through [`App::remote`] on the UI thread, as a click would.

use crate::annotations::Measurement;
use crate::app::{face_at, pick_triangle, App, Message, Status};
use crate::explode::ExplodeMode;
use crate::model::{Bounds, Model, ViewKind};
use crate::remote_link::{self, Announcement};
use crate::selection::Pickable;
use crate::section::{SectionBox, SectionPlane};
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::{transform_bounds, RenderMode};
use glam::Vec3;
use iced::futures::{SinkExt, Stream, StreamExt};
use iced::{window, Task};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// `save_file` calls waiting until the file is written.
    pub pending_save: Vec<RemoteCall>,
    /// Size of the last viewport screenshot handed out: screen coordinates in commands are in its
    /// pixels.
    pub shot_image: Option<(u32, u32)>,
}

// ---- the socket -------------------------------------------------------------------------------

/// The socket listens (set once it is bound).
static LISTENING: AtomicBool = AtomicBool::new(false);
/// Clients connected now (the MCP server keeps one connection while it runs).
static CLIENTS: AtomicUsize = AtomicUsize::new(0);

/// What the status bar's lamp shows of the remote control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    /// Remote control is off or the socket could not be bound.
    Off,
    /// Listening, no client.
    Ready,
    /// An MCP server (or another client) is connected.
    Connected,
}

pub fn link_status() -> LinkStatus {
    if !LISTENING.load(Ordering::Relaxed) {
        LinkStatus::Off
    } else if CLIENTS.load(Ordering::Relaxed) > 0 {
        LinkStatus::Connected
    } else {
        LinkStatus::Ready
    }
}

/// An internal command that only makes the window redraw (the lamp changed).
const LINK_CHANGED: &str = "_link";

/// The subscription stream: starts the listener and yields its commands as messages.
pub fn serve() -> impl Stream<Item = Message> {
    iced::stream::channel(16, async |mut output| {
        let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<RemoteCall>();
        // the HTTP endpoint does not need the control port: it keeps its commands coming when
        // that cannot be bound or announced (no settings folder, port taken)
        let http = start_mcp_http(&tx);
        if let Err(e) = listen(tx) {
            eprintln!("remote control: {e}");
            if !http {
                return;
            }
        }
        while let Some(call) = rx.next().await {
            if output.send(Message::Remote(call)).await.is_err() {
                break;
            }
        }
    })
}

/// The MCP endpoint over HTTP to start with the control port (OpenAEC SuperCloud), if any.
#[cfg(not(target_arch = "wasm32"))]
static MCP_HTTP: Mutex<Option<crate::mcp::HttpConfig>> = Mutex::new(None);

/// Serves MCP over HTTP from this viewer, besides the control port: the same tools as
/// `open-3d-viewer-mcp`, with the SuperCloud's names, and every call goes through
/// [`App::remote`] as a call from the MCP server does.
#[cfg(not(target_arch = "wasm32"))]
pub fn serve_mcp_http(config: crate::mcp::HttpConfig) {
    *MCP_HTTP.lock().unwrap_or_else(|e| e.into_inner()) = Some(config);
}

/// Starts the MCP endpoint over HTTP, if one is set; whether it serves.
#[cfg(not(target_arch = "wasm32"))]
fn start_mcp_http(tx: &iced::futures::channel::mpsc::UnboundedSender<RemoteCall>) -> bool {
    let Some(config) = MCP_HTTP.lock().unwrap_or_else(|e| e.into_inner()).take() else { return false };
    let tx = Mutex::new(tx.clone());
    let served = config.serve(move |body| {
        let tx = tx.lock().unwrap_or_else(|e| e.into_inner()).clone();
        crate::mcp::handle_text(body, true, &mut |cmd: &str, args: &Value, timeout| {
            let (call, rx) = RemoteCall::new(cmd, args.clone());
            tx.unbounded_send(call).map_err(|_| "the viewer is closing".to_string())?;
            rx.recv_timeout(timeout).unwrap_or_else(|e| match e {
                mpsc::RecvTimeoutError::Timeout => Err(format!("the viewer did not answer within {} s", timeout.as_secs())),
                mpsc::RecvTimeoutError::Disconnected => Err("the viewer dropped the command".to_string()),
            })
        })
    });
    if let Err(e) = &served {
        eprintln!("MCP over HTTP: {e}");
    }
    served.is_ok()
}

#[cfg(target_arch = "wasm32")]
fn start_mcp_http(_: &iced::futures::channel::mpsc::UnboundedSender<RemoteCall>) -> bool {
    false
}

fn listen(tx: iced::futures::channel::mpsc::UnboundedSender<RemoteCall>) -> std::io::Result<()> {
    let port = std::env::var("O3V_REMOTE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let announcement = Announcement { port: listener.local_addr()?.port(), token: new_token(), pid: std::process::id() };
    remote_link::announce(&announcement)?;
    LISTENING.store(true, Ordering::Relaxed);
    let _ = tx.unbounded_send(RemoteCall::new(LINK_CHANGED, Value::Null).0);
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
    CLIENTS.fetch_add(1, Ordering::Relaxed);
    let _ = tx.unbounded_send(RemoteCall::new(LINK_CHANGED, Value::Null).0);
    serve_client(read, stream, tx, token);
    CLIENTS.fetch_sub(1, Ordering::Relaxed);
    let _ = tx.unbounded_send(RemoteCall::new(LINK_CHANGED, Value::Null).0);
}

fn serve_client(read: TcpStream, stream: TcpStream, tx: &iced::futures::channel::mpsc::UnboundedSender<RemoteCall>, token: &str) {
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

/// The most entries a listing gives in one answer.
const MAX_LIST: usize = 200;

/// `offset` and `limit` of a listing: `limit` defaults to `default` and is at most `max`, so an
/// answer stays small for the assistant (the SuperCloud contract asks for at most about 30,000
/// characters); the rest comes with a next `offset`.
pub(crate) fn page(a: &Value, default: usize, max: usize) -> (usize, usize) {
    let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = a.get("limit").and_then(Value::as_u64).map_or(default, |l| l as usize).min(max);
    (offset, limit)
}

/// One page of `items` as `{total, offset, returned, truncated, <key>: [...]}`.
pub(crate) fn paged(key: &str, items: Vec<Value>, (offset, limit): (usize, usize)) -> Value {
    let total = items.len();
    let shown: Vec<Value> = items.into_iter().skip(offset).take(limit).collect();
    let mut out = json!({ "total": total, "offset": offset, "returned": shown.len(), "truncated": offset.saturating_add(shown.len()) < total });
    out[key] = Value::Array(shown);
    out
}

/// A path from a remote client: in the work folder of the SuperCloud (`managed::confine`).
fn confine(path: &str) -> Result<PathBuf, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return crate::managed::confine(std::path::Path::new(path));
    #[cfg(target_arch = "wasm32")]
    Ok(PathBuf::from(path))
}

/// Where a screenshot's `save_to` writes: a `.png` in the work folder, and an existing file only
/// with `overwrite`, as `save_file` (so a screenshot never replaces a model).
fn shot_target(path: &Value, a: &Value) -> Result<PathBuf, String> {
    let path = confine(path.as_str().ok_or("save_to must be a path")?)?;
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return Err(format!("save_to {}: a screenshot is saved as .png", path.display()));
    }
    if path.symlink_metadata().is_ok() && !flag(a, "overwrite").unwrap_or(false) {
        return Err(format!("{} exists; pass overwrite: true to replace it, or another path", path.display()));
    }
    Ok(path)
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
            "save_file" => return self.remote_save(call),
            "get_schedule" => {
                let (answer, task) = self.remote_schedule(&call.args);
                call.answer(answer);
                return task;
            }
            "get_sheet" => {
                let (answer, task) = self.remote_sheet(&call.args);
                call.answer(answer);
                return task;
            }
            "screenshot" => {
                // a wrong save_to is refused before the capture
                if let Some(Err(e)) = call.args.get("save_to").map(|p| shot_target(p, &call.args)) {
                    call.answer(Err(e));
                    return Task::none();
                }
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

    pub(crate) fn finish_animation(&mut self) {
        if let Some((_, to, _)) = self.anim.take() {
            self.camera = to;
        }
    }

    fn command(&mut self, cmd: &str, a: &Value) -> Answer {
        match cmd {
            // the lamp changed: nobody waits for the answer, the window just redraws
            LINK_CHANGED => Ok(Value::Null),
            "get_state" => Ok(self.state_json()),
            "list_layers" => self.list_layers(a),
            "set_layer_visibility" => self.set_layer_visibility(a),
            "list_views" => self.list_views(a),
            "apply_view" => self.remote_apply_view(a),
            "get_project_info" => self.project_info(a),
            "list_schedules" => self.schedules_json(),
            "list_sheets" => self.sheets_json(),
            "find_elements" => self.find_elements(a),
            "list_structural" => crate::structural::remote::list(&*self.require_model()?),
            "get_structural_issues" => crate::structural::remote::issues(&*self.require_model()?, a),
            "get_structural_element" => crate::structural::remote::element(&*self.require_model()?, a, &self.model_matrix()),
            "find_structural" => crate::structural::remote::find(&*self.require_model()?, a, &self.model_matrix()),
            "get_element" => {
                let id = a.get("id").and_then(id_of).ok_or("id is required")?;
                self.element_json(id)
            }
            "select" => self.remote_select(a),
            "hide_elements" => self.remote_hide(a, Message::HideSelection),
            "isolate_elements" => self.remote_hide(a, Message::IsolateSelection),
            "show_all_elements" => {
                let _ = self.update(Message::ShowAllElements);
                Ok(self.visibility_json())
            }
            "undo" => {
                let _ = self.update(Message::Undo);
                Ok(self.visibility_json())
            }
            "redo" => {
                let _ = self.update(Message::Redo);
                Ok(self.visibility_json())
            }
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
            #[cfg(all(feature = "collaborator", not(target_arch = "wasm32")))]
            cmd if cmd.starts_with("collab_") => self.collab_command(cmd, a),
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
        let Some(path) = call.args.get("path").and_then(Value::as_str) else {
            call.answer(Err("path is required".into()));
            return Task::none();
        };
        let path = match confine(path) {
            Ok(p) => p,
            Err(e) => {
                call.answer(Err(e));
                return Task::none();
            }
        };
        if !path.is_file() {
            call.answer(Err(format!("no such file: {}", path.display())));
            return Task::none();
        }
        if matches!(self.status, Status::Loading { .. }) {
            call.answer(Err("another file is still loading".into()));
            return Task::none();
        }
        // beside the models open, as Add does, or in their place
        let add = call.args.get("add").and_then(Value::as_bool).unwrap_or(false) && self.model.is_some();
        let task = self.update(if add { Message::AddPicked(Some(crate::loaders::Source::File(path))) } else { Message::Open(path) });
        self.remote.pending_load.push(call);
        task
    }

    /// Where `save_file` writes: `path` (in the work folder), else next to the open file as IFCX.
    /// Only formats the viewer writes, and an existing file only with `overwrite`. The default is
    /// never the open file itself: the viewer's IFCX is a re-export (triangles, the properties it
    /// shows), so an open `a.ifcx` is saved as `a-viewer.ifcx` beside it.
    pub(crate) fn save_target(&self, a: &Value) -> Result<PathBuf, String> {
        let model = self.require_model()?;
        if matches!(self.status, Status::Loading { .. } | Status::Saving { .. }) {
            return Err("a file is still loading or saving".into());
        }
        let default_ext = crate::export::registry()[0].extensions()[0];
        let mut path = match a.get("path").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) {
            Some(p) => confine(p)?,
            None if model.path.as_os_str().is_empty() => return Err("the model has no file; give a path".into()),
            None => {
                let mut path = model.path.with_extension(default_ext);
                if model.path.extension().is_some_and(|e| e.eq_ignore_ascii_case(default_ext)) {
                    let stem = model.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    path.set_file_name(format!("{stem}-viewer.{default_ext}"));
                }
                confine(&path.to_string_lossy())?
            }
        };
        if path.extension().is_none() {
            path.set_extension(default_ext);
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        crate::export::writer_for(&name).map_err(|e| format!("{e}: the viewer saves models as IFCX (.ifcx) or IFC (.ifc)"))?;
        if path.parent().is_some_and(|d| !d.as_os_str().is_empty() && !d.is_dir()) {
            return Err(format!("the folder of {} does not exist", path.display()));
        }
        if path.exists() && !flag(a, "overwrite").unwrap_or(false) {
            return Err(format!("{} exists; pass overwrite: true to replace it, or another path", path.display()));
        }
        Ok(path)
    }

    fn remote_save(&mut self, call: RemoteCall) -> Task<Message> {
        match self.save_target(&call.args) {
            Ok(path) => {
                let task = self.start_save(path, flag(&call.args, "overwrite").unwrap_or(false));
                self.remote.pending_save.push(call);
                task
            }
            Err(e) => {
                call.answer(Err(e));
                Task::none()
            }
        }
    }

    /// Answers the `save_file` calls once the file is written (one save runs at a time, see
    /// [`App::start_save`], so this result is theirs).
    pub fn remote_saved(&mut self, result: &Result<(PathBuf, f32), String>) {
        let answer = result.clone().map(|(path, secs)| {
            let bytes = std::fs::metadata(&path).map(|m| m.len()).ok();
            json!({ "saved": path.display().to_string(), "bytes": bytes, "seconds": round(secs) })
        });
        for call in self.remote.pending_save.drain(..) {
            call.answer(answer.clone());
        }
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
        if let Some(path) = a.get("save_to") {
            let path = shot_target(path, a)?;
            crate::export::write_file(&path, flag(a, "overwrite").unwrap_or(false), |out| out.write_all(&png))
                .map_err(|e| format!("save_to {}: {e}", path.display()))?;
            out["saved_to"] = json!(path.display().to_string());
        }
        out["png_base64"] = json!(base64(&png));
        Ok(out)
    }

    // ---- state ----

    fn model_json(&self) -> Value {
        let Some(m) = &self.model else { return Value::Null };
        let elements: usize = element_layers(m).len();
        let mut out = json!({
            "file": m.path.display().to_string(),
            "format": m.format,
            "file_size_bytes": m.file_size,
            "load_time_s": round(m.load_time.as_secs_f32()),
            "layers": m.layers.len(),
            "vertices": m.vertex_count(),
            "triangles": m.triangle_count(),
            "points": m.point_count(),
            "elements": elements,
            "views": m.views.len(),
            "levels": m.project.levels.len(),
            "rooms": m.project.rooms.len(),
            "grids": m.project.grids.len(),
            "bounds": self.world_bounds().map(|b| jbounds(&b)),
            "units": "metres (Revit, SketchUp, glTF); OBJ, STL and PLY keep the file's own units",
            "up_axis": "Z",
        });
        if !m.structural.is_empty() {
            let count = |f: fn(&bim_structural::AnalysisModel) -> usize| m.structural.iter().map(|p| f(&p.doc)).sum::<usize>();
            out["structural"] = json!({
                "models": m.structural.len(),
                "nodes": count(|d| d.nodes.len()),
                "members": count(|d| d.members.len()),
                "surfaces": count(|d| d.surfaces.len()),
                "objects": m.structural_index.len(),
                "note": "structural objects are elements: get_element, find_elements and the *_structural tools take their ids",
            });
        }
        out
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

    /// The selected elements (or layer) and the first one's properties; structural objects listed
    /// apart (`structural`), their first one's properties shown when no mesh element is
    /// selected.
    fn selection_json(&self) -> Value {
        let Some(model) = &self.model else { return Value::Null };
        // a picked grid line, picked alone
        if let Some(grid) = self.picked_grid.and_then(|g| model.project.grids.get(g)) {
            return grid_json(grid);
        }
        let structural: Vec<Value> = self.structural.selected.iter().map(|&id| crate::structural::id_json(id)).collect();
        let Some((layer, element)) = self.selected else {
            let Some(&first) = self.structural.selected.first() else { return Value::Null };
            return json!({
                "layer": Value::Null,
                "element": crate::structural::id_json(first),
                "elements": [],
                "structural": structural,
                "properties": props_json(model.element_props(first)),
                "bounds": self.selection_bounds().map(|b| jbounds(&b)),
            });
        };
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
            "structural": structural,
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
            "selection_count": self.sel.len(),
            "hidden_count": self.hidden.len(),
            "active_view": self.active_view.and_then(|i| self.model.as_ref()?.views.get(i).map(|v| v.name.clone())),
            "shown_sheet": self.active_sheet.and_then(|i| self.model.as_ref()?.project.sheets.get(i).map(|s| s.title.clone())),
            "shown_schedule": self.active_schedule.and_then(|i| self.schedules.get(i).map(|s| s.name.clone())),
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
                    "points": l.point_count(),
                    "elements": l.elements.len(),
                    "color": format!("#{r:02x}{g:02x}{b:02x}"),
                })
            })
            .collect();
        Ok(paged("layers", layers, page(a, 100, MAX_LIST)))
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

    fn list_views(&self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let views: Vec<Value> = model
            .views
            .iter()
            .enumerate()
            .map(|(i, v)| json!({ "index": i, "name": v.name, "kind": kind_name(v.kind), "level": v.level, "active": self.active_view == Some(i) }))
            .collect();
        Ok(paged("views", views, page(a, 100, MAX_LIST)))
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

    fn project_info(&self, a: &Value) -> Answer {
        let model = self.require_model()?;
        let p = &model.project;
        // levels and grids up to MAX_LIST; rooms (with all their parameters) a page at a time
        let (offset, limit) = page(a, 20, 50);
        let rooms: Vec<Value> = p.rooms.iter().skip(offset).take(limit).map(|r| json!({
            "number": r.number,
            "name": r.name,
            "level": r.level,
            "parameters": Value::Object(r.extra.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
        })).collect();
        let truncated = p.levels.len() > MAX_LIST || p.grids.len() > MAX_LIST || offset.saturating_add(rooms.len()) < p.rooms.len();
        Ok(json!({
            "info": Value::Object(p.info.iter().map(|(k, v)| (k.clone(), json!(v))).collect()),
            "levels": p.levels.iter().take(MAX_LIST).map(|(n, z)| json!({ "name": n, "elevation": round(*z) })).collect::<Vec<_>>(),
            "level_count": p.levels.len(),
            "phases": p.phases,
            "grids": p.grids.iter().take(MAX_LIST).map(|g| json!({ "name": g.name, "a": g.a, "b": g.b })).collect::<Vec<_>>(),
            "grid_count": p.grids.len(),
            "rooms": rooms,
            "room_count": p.rooms.len(),
            "rooms_offset": offset,
            "truncated": truncated,
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
        let (offset, limit) = page(a, 50, 100);

        // elements with triangles (listed under their first layer), then structural objects
        // drawn from their document (listed under their analysis model)
        let drawn = element_layers(&model);
        let structural: Vec<crate::structural::StructuralItem> = if model.structural.is_empty() {
            Vec::new()
        } else {
            model.structural_index.all(&model.structural).into_iter().filter(|item| !drawn.contains_key(&item.element)).collect()
        };
        let passes = |id: u64, props: &[(String, String)]| {
            let value = |key: &str| props.iter().find(|(k, _)| k.to_lowercase() == key).map(|(_, v)| v.to_lowercase());
            conditions.iter().all(|(k, v)| value(k).is_some_and(|x| x == *v || x.contains(v.as_str())))
                && (text.is_empty() || props.iter().any(|(_, v)| v.to_lowercase().contains(&text)) || id.to_string() == text)
        };
        let mut matches: Vec<(u64, Listed, Vec<(String, String)>)> = Vec::new();
        for (id, layers) in drawn {
            if !layer.is_empty() && !layers.iter().any(|&l| model.layers[l].name.to_lowercase().contains(&layer)) {
                continue;
            }
            let mut props = model.element_props(id);
            if !props.iter().any(|(k, _)| k == "category") {
                props.push(("category".into(), model.layers[layers[0]].name.clone()));
            }
            if passes(id, &props) {
                matches.push((id, Listed::Layer(layers[0]), props));
            }
        }
        for item in structural {
            if !layer.is_empty() && !format!("structural: {}", item.doc().name.to_lowercase()).contains(&layer) {
                continue;
            }
            // a structural object's category is its class: test it before building properties
            let class = crate::structural::class_name(item.kind());
            if conditions.iter().any(|(k, v)| (k == "category" || k == "class") && !class.to_lowercase().contains(v.as_str())) {
                continue;
            }
            let mut props = item.props();
            props.push(("category".into(), class.into()));
            if passes(item.element, &props) {
                matches.push((item.element, Listed::Structural(item.part_index as usize), props));
            }
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
            .map(|(id, listed, props)| {
                let (layer, structural) = match listed {
                    Listed::Layer(l) => (model.layers[l].name.clone(), false),
                    Listed::Structural(p) => (format!("Structural: {}", model.structural[p].doc.name), true),
                };
                let keep = ["category", "family", "type", "name", "level", "class"];
                let keep_structural = ["Class", "Number", "Name", "Kind", "Section", "Support"];
                let summary: Map<String, Value> = props
                    .into_iter()
                    .filter(|(k, _)| keep.contains(&k.as_str()) || (structural && keep_structural.contains(&k.as_str())))
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                json!({ "id": crate::structural::id_json(id), "layer": layer, "properties": summary })
            })
            .collect();
        Ok(json!({ "total": total, "offset": offset, "returned": shown.len(), "truncated": offset.saturating_add(shown.len()) < total, "elements": shown }))
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
            // a structural object: drawn from its document, not from triangles
            return crate::structural::remote::as_element(&model, id, &self.model_matrix()).ok_or_else(|| format!("no element {id}"));
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
            "id": crate::structural::id_json(id),
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
        let mm = self.model_matrix();
        for id in ids {
            let tris = self.element_triangles(&model, id);
            for t in &tris {
                t.iter().for_each(|p| b.extend(p.to_array()));
            }
            // a structural object: its point, segment or target box
            if let Some(s) = model.structural_item(id).filter(|_| tris.is_empty()).and_then(|item| item.bounds(&model.origins)) {
                let w = transform_bounds(&s, &mm);
                b.extend(w.min);
                b.extend(w.max);
            }
        }
        if b.is_valid() { Ok(b) } else { Err("no such elements".into()) }
    }

    /// `hide_elements` and `isolate_elements`: the elements in `ids`, or else the selection, as
    /// Hide (which clears the selection) and Isolate (which keeps it) do.
    fn remote_hide(&mut self, a: &Value, message: Message) -> Answer {
        let model = self.require_model()?;
        if let Some(Value::Array(list)) = a.get("ids") {
            let ids: Vec<u64> = list.iter().filter_map(id_of).collect();
            hideable(&model, self.explode_index.as_deref(), &ids)?;
            self.sel.clear();
            self.sel.extend(ids.into_iter().map(Pickable::Element));
            self.selection_changed();
        }
        if self.sel.is_empty() {
            return Err("ids (list of element ids) is required when nothing is selected".into());
        }
        let _ = self.update(message);
        Ok(self.visibility_json())
    }

    /// What is hidden and what undo can do, after a visibility command.
    fn visibility_json(&self) -> Value {
        json!({
            "hidden_count": self.hidden.len(),
            "selection_count": self.sel.len(),
            "can_undo": self.history.can_undo(),
            "can_redo": self.history.can_redo(),
        })
    }

    fn remote_select(&mut self, a: &Value) -> Answer {
        let model = self.require_model()?;
        if flag(a, "clear").unwrap_or(false) {
            self.sel.clear();
            self.selection_changed();
            self.set_structural_selection(Vec::new());
            return Ok(Value::Null);
        }
        let ids: Vec<u64> = match (a.get("ids"), a.get("id")) {
            (Some(Value::Array(list)), _) => list.iter().filter_map(id_of).collect(),
            (_, Some(v)) => vec![id_of(v).ok_or("id must be an element id")?],
            _ => return Err("ids (list of element ids), id or clear is required".into()),
        };
        if ids.is_empty() {
            return Err("no elements given".into());
        }
        let drawn: std::collections::HashSet<u64> = model.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        // a structural object is drawn from its document: a node or member itself, a load or line
        // support through the item it sits on
        let mut structural = Vec::new();
        for &id in ids.iter().filter(|id| !drawn.contains(id)) {
            match model.structural_item(id) {
                Some(item) if item.unit_key().is_some() => structural.push(id),
                Some(item) => {
                    return Err(format!(
                        "{id} is a structural {} without a place in the model (nothing to select); get_element and get_structural_element take its id",
                        crate::structural::kind_name(item.kind())
                    ))
                }
                None => return Err(not_drawn(&model, id, "selecting")),
            }
        }
        let add = flag(a, "add").unwrap_or(false);
        if !add {
            self.sel.clear();
        }
        self.sel.extend(ids.iter().filter(|id| drawn.contains(id)).map(|&id| Pickable::Element(id)));
        self.selection_changed();
        let mut chosen = if add { self.structural.selected.clone() } else { Vec::new() };
        let mut seen: std::collections::HashSet<u64> = chosen.iter().copied().collect();
        chosen.extend(structural.into_iter().filter(|id| seen.insert(*id)));
        self.set_structural_selection(chosen);
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
        let (layer, triangle) = pick_triangle(&model, &self.visible, origin, dir, &shift, &keep, &self.hidden_test(), None)?;
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
        let (w, h) = self.viewport_size;
        // a grid line drawn there (its bubble, or within 5 px of its line where it is seen), as a
        // click takes it first (unless it measures or places a plane)
        let clicks = self.measure.tool.is_none() && !self.plane_picking;
        let grid = self.grid_under(vx, vy, w, h, hit.as_ref().map(|h| h.point)).filter(|_| clicks);
        if let (Some(g), Some(model)) = (grid, self.model.clone()) {
            if flag(a, "select").unwrap_or(false) {
                self.pick_grid(Some(g));
            }
            let mut out = grid_json(&model.project.grids[g]);
            out["hit"] = json!(true);
            return Ok(out);
        }
        let structural = self.pick_structural(vx, vy, w, h, hit.as_ref().map(|h| h.point));
        if flag(a, "select").unwrap_or(false) {
            match &structural {
                // what was found is selected, as a click would (no second pick)
                Some(s) if self.measure.tool.is_none() && !self.plane_picking => self.click_structural(s.element, false),
                _ => self.pick(vx, vy, w, h, false),
            }
        }
        // a structural node or member drawn there (within 8 px of a node, 6 px of a member)
        if let (Some(s), Some(model)) = (structural, self.model.as_ref()) {
            return Ok(json!({
                "hit": true,
                "structural": if s.node { "node" } else { "member" },
                "element": crate::structural::id_json(s.element),
                "point": jv(s.world),
                "pixels_away": round(s.px),
                "properties": props_json(model.element_props(s.element)),
                "distance_from_camera": round(s.world.distance(self.camera.eye())),
            }));
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
            return Err(not_drawn(&model, if ta.is_empty() { ida } else { idb }, "measuring between"));
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
/// Where `find_elements` lists a match: the first layer of its triangles, or the structural part
/// it is in (names resolved for the page shown only).
enum Listed {
    Layer(usize),
    Structural(usize),
}

/// Whether every one of `ids` can be hidden: an element with triangles, or a structural node,
/// member or surface (a unit of its own in `index`, which Hide and Isolate reach through the
/// units texture). Else why not.
fn hideable(model: &Model, index: Option<&crate::explode::ExplodeIndex>, ids: &[u64]) -> Result<(), String> {
    let drawn: std::collections::HashSet<u64> = model.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
    for &id in ids.iter().filter(|id| !drawn.contains(id)) {
        if index.is_some_and(|index| index.structural_unit(id).is_some()) {
            continue;
        }
        return Err(match model.structural_item(id) {
            Some(item) => format!(
                "{id} is a structural {}: only nodes, members and surfaces can be hidden; get_element and get_structural_element take its id",
                crate::structural::kind_name(item.kind())
            ),
            None => format!("no element {id}"),
        });
    }
    Ok(())
}

/// Why an element without triangles cannot be measured: a structural object is drawn from its
/// document (measuring between those is not supported yet), anything else does not exist.
fn not_drawn(model: &Model, id: u64, what: &str) -> String {
    match model.structural_item(id) {
        Some(item) => format!(
            "{id} is a structural object ({}): {what} structural objects is not supported yet; get_element and get_structural_element take its id",
            crate::structural::kind_name(item.kind())
        ),
        None => format!("no element {id}"),
    }
}

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

pub(crate) fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(png)
}

pub(crate) fn base64(bytes: &[u8]) -> String {
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

/// A grid line: its name and the ends of its line (world, metres).
fn grid_json(grid: &crate::model::GridLine) -> Value {
    json!({ "grid": grid.name, "start": jv(Vec3::from_array(grid.a)), "end": jv(Vec3::from_array(grid.b)) })
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
    fn remote_hide_and_show_all() {
        let mut app = app();
        run(&mut app, "hide_elements", json!({ "ids": [7] })).expect("hide");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["hidden_count"], 1);
        run(&mut app, "select", json!({ "id": 8 })).expect("select");
        run(&mut app, "isolate_elements", json!({})).expect("isolate the selection");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["hidden_count"], 1, "only 7 is hidden");
        run(&mut app, "show_all_elements", json!({})).expect("show all");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["hidden_count"], 0);
        assert!(run(&mut app, "hide_elements", json!({ "ids": [99] })).is_err());
    }

    #[test]
    fn remote_select_add() {
        let mut app = app();
        run(&mut app, "select", json!({ "id": 7 })).expect("select");
        run(&mut app, "select", json!({ "id": 8, "add": true })).expect("add");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["selection_count"], 2);
        run(&mut app, "select", json!({ "id": 8 })).expect("replace");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["selection_count"], 1);
    }

    #[test]
    fn remote_undo_restores_hidden() {
        let mut app = app();
        run(&mut app, "hide_elements", json!({ "ids": [7, 8] })).expect("hide");
        run(&mut app, "undo", json!({})).expect("undo");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["hidden_count"], 0);
        run(&mut app, "redo", json!({})).expect("redo");
        assert_eq!(run(&mut app, "get_state", json!({})).unwrap()["hidden_count"], 2);
    }

    #[test]
    fn saves_only_what_the_viewer_writes() {
        let app = app();
        // next to the open file, as IFCX
        assert_eq!(app.save_target(&json!({})), Ok(PathBuf::from("test.ifcx")));
        let dir = std::env::temp_dir().join(format!("o3v-save-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
        assert_eq!(app.save_target(&json!({ "path": at("out") })), Ok(dir.join("out.ifcx")));
        assert!(app.save_target(&json!({ "path": at("out.glb") })).unwrap_err().contains("glb"));
        assert!(app.save_target(&json!({ "path": at("missing/out.ifc") })).is_err());
        // an existing file only with overwrite
        std::fs::write(dir.join("old.ifc"), b"x").unwrap();
        assert!(app.save_target(&json!({ "path": at("old.ifc") })).unwrap_err().contains("overwrite"));
        assert_eq!(app.save_target(&json!({ "path": at("old.ifc"), "overwrite": true })), Ok(dir.join("old.ifc")));
        // an open IFC gets its IFCX beside it; an open IFCX is not the default target: the
        // viewer's IFCX goes beside it
        let mut ifc = App::default();
        ifc.set_model(Arc::new(Model::new(dir.join("a.IFC"), "IFC", vec![])));
        assert_eq!(ifc.save_target(&json!({})), Ok(dir.join("a.ifcx")));
        assert_eq!(ifc.save_target(&json!({ "path": at("a.IFC"), "overwrite": true })), Ok(dir.join("a.IFC")));
        let mut ifcx = App::default();
        ifcx.set_model(Arc::new(Model::new(dir.join("b.IFCX"), "IFCX", vec![])));
        assert_eq!(ifcx.save_target(&json!({})), Ok(dir.join("b-viewer.ifcx")));
        let _ = std::fs::remove_dir_all(&dir);
        // nothing open, nothing to save
        assert!(App::default().save_target(&json!({})).is_err());
    }

    #[test]
    fn screenshots_are_saved_as_new_pngs() {
        let dir = std::env::temp_dir().join(format!("o3v-shot-target-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = |name: &str| json!(dir.join(name).to_string_lossy());
        let none = json!({});
        assert_eq!(shot_target(&at("view.png"), &none), Ok(dir.join("view.png")));
        assert_eq!(shot_target(&at("view.PNG"), &none), Ok(dir.join("view.PNG")));
        // never a model, nor a file that is there without overwrite
        assert!(shot_target(&at("model.ifc"), &none).unwrap_err().contains(".png"));
        assert!(shot_target(&at("model"), &none).is_err());
        std::fs::write(dir.join("old.png"), b"x").unwrap();
        assert!(shot_target(&at("old.png"), &none).unwrap_err().contains("overwrite"));
        assert!(shot_target(&at("old.png"), &json!({ "overwrite": true })).is_ok());
        assert!(shot_target(&json!(1), &none).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The open_file tool names every format the viewer opens, so an agent does not turn one down.
    #[test]
    fn open_file_lists_every_format() {
        let tools = crate::mcp::tools();
        let open = tools.iter().find(|t| t["name"] == "open_file").unwrap();
        let text = open["description"].as_str().unwrap();
        for ext in crate::loaders::supported_extensions() {
            assert!(text.contains(&format!(".{ext} ")) || text.contains(&format!(".{ext})")) || text.contains(&format!(".{ext},")), ".{ext} is not in open_file");
        }
    }

    #[test]
    fn listings_are_paged_and_capped() {
        let mut app = app();
        let layers = run(&mut app, "list_layers", json!({ "limit": 100000 })).unwrap();
        assert_eq!((layers["total"].clone(), layers["truncated"].clone()), (json!(1), json!(false)));
        let none = run(&mut app, "list_layers", json!({ "offset": 1 })).unwrap();
        assert_eq!(none["returned"], json!(0));
        assert_eq!(page(&json!({ "limit": 100000 }), 50, 100), (0, 100));
        assert_eq!(page(&json!({}), 50, 100), (0, 50));
        let found = run(&mut app, "find_elements", json!({ "limit": 1 })).unwrap();
        assert_eq!((found["returned"].clone(), found["truncated"].clone()), (json!(1), json!(true)));
        let info = run(&mut app, "get_project_info", json!({})).unwrap();
        assert_eq!(info["room_count"], json!(0));
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

    /// `pick` names a grid line under the pixel, as a click takes it before what lies behind it,
    /// and with select picks it; the state shows it then.
    #[test]
    fn pick_names_a_grid_line_and_selects_it() {
        let mut model = Model::new(PathBuf::from("grids.rvt"), "Revit", Vec::new());
        model.project.grids = vec![crate::model::GridLine { name: "A".into(), a: [-5.0, 0.0, 0.0], b: [5.0, 0.0, 0.0] }];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app.viewport_size = (800.0, 600.0);
        app.camera.set_preset(ViewPreset::Top);
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 20.0;
        let hit = run(&mut app, "pick", json!({ "x": 400, "y": 302 })).unwrap();
        assert_eq!(hit["grid"], json!("A"), "{hit}");
        assert_eq!((hit["start"].clone(), hit["end"].clone()), (json!([-5.0, 0.0, 0.0]), json!([5.0, 0.0, 0.0])));
        assert_eq!(app.picked_grid, None);
        run(&mut app, "pick", json!({ "x": 400, "y": 302, "select": true })).unwrap();
        assert_eq!(app.picked_grid, Some(0));
        let state = run(&mut app, "get_state", json!({})).unwrap();
        assert_eq!(state["selection"]["grid"], json!("A"), "{state}");
        assert!(run(&mut app, "pick", json!({ "x": 400, "y": 380 })).unwrap()["grid"].is_null());
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
