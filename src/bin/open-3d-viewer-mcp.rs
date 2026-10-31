//! MCP server for Open 3D Viewer Studio: lets an AI agent (Claude Code, Claude Desktop or any MCP
//! client) drive the viewer as a person would: open files, look around, show and hide layers,
//! find and select elements, cut with a section box or plane, measure and take screenshots.
//!
//! It speaks MCP over stdio (JSON-RPC, one message per line) and passes every tool call on to the
//! running viewer through its local control port (`remote_link`). When no viewer runs it starts
//! one: `O3V_VIEWER`, else the viewer next to this program.
//!
//! Usage:
//!   open-3d-viewer-mcp                  (started by the MCP client)
//!   open-3d-viewer-mcp --print-config   (the JSON to add it to an MCP client)

#[path = "../remote_link.rs"]
mod remote_link;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Open 3D Viewer Studio shows 3D and BIM models (IFC, Revit, SketchUp, glTF, OBJ, STL, PLY). \
The scene is Z up, in metres for Revit, SketchUp and glTF. Start with open_file (or get_state to see what is open), \
then screenshot to look at the viewport. Pixel coordinates (pick, screen points in measure_* and section_plane) are \
pixels of the last viewport screenshot. Elements (BIM) have ids and properties: find them with find_elements, \
inspect with get_element (size, area, volume), highlight with select. Measurements are drawn in the viewport until \
clear_measurements. Camera and section commands land at once, so a screenshot right after shows the result.";

fn point_schema(what: &str) -> Value {
    json!({
        "description": format!("{what}: [x, y, z] in world coordinates, {{\"screen\": [x, y], \"snap\": true}} for what is under that pixel of the last viewport screenshot (snap = nearest corner of the face), or {{\"element\": id}} for the centre of an element"),
        "oneOf": [
            { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
            { "type": "object", "properties": { "screen": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }, "snap": { "type": "boolean" } }, "required": ["screen"] },
            { "type": "object", "properties": { "element": { "type": ["integer", "string"] } }, "required": ["element"] }
        ]
    })
}

fn vec3_schema(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": ["number", "null"] }, "minItems": 3, "maxItems": 3, "description": description })
}

fn target_schema(description: &str) -> Value {
    json!({
        "description": format!("{description}: \"model\", \"selection\", an element id or a list of element ids"),
        "oneOf": [
            { "type": "string" },
            { "type": "integer" },
            { "type": "array", "items": { "type": ["integer", "string"] } }
        ]
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false },
    })
}

fn tools() -> Vec<Value> {
    vec![
        tool("get_state", "What the viewer shows now: the open model (file, counts, bounds), camera, render mode, section box and plane, selection, active saved view, viewport size and measurements.", json!({}), &[], true),
        tool("open_file", "Open a 3D model file in the viewer (as File > Open) and wait until it is loaded. Formats: .ifc .rvt .rfa .skp .pln .pla .gltf .glb .obj .stl .ply. Returns the model summary.",
            json!({ "path": { "type": "string", "description": "Absolute path of the file" } }), &["path"], false),
        tool("screenshot", "Capture what the viewer shows as a PNG image. 'viewport' (default) is the 3D view only; its pixels are the coordinates for pick and screen points. 'window' is the whole window with ribbon and panels.",
            json!({
                "area": { "type": "string", "enum": ["viewport", "window"] },
                "max_size": { "type": "integer", "description": "Longest side in pixels (default 1600)" },
                "save_to": { "type": "string", "description": "Also write the PNG to this path" }
            }), &[], true),
        tool("list_layers", "The model's layers (Revit categories, SketchUp tags, glTF materials, OBJ groups) with index, visibility, triangle and element counts and colour.",
            json!({ "filter": { "type": "string", "description": "Only layers whose name contains this" } }), &[], true),
        tool("set_layer_visibility", "Show or hide layers, or isolate them (show only these).",
            json!({
                "layers": { "description": "Layer indices and/or names (exact, else contains), or \"all\"", "oneOf": [{ "type": "string" }, { "type": "integer" }, { "type": "array", "items": { "type": ["integer", "string"] } }] },
                "visible": { "type": "boolean" },
                "isolate": { "type": "boolean", "description": "Show only these layers" }
            }), &["layers"], false),
        tool("list_views", "Views saved in the file (Revit: 3D views, floor plans, sections, elevations) with index, kind and level.", json!({}), &[], true),
        tool("apply_view", "Show a saved view: its camera and what it shows (section box, plan range).",
            json!({ "view": { "type": ["integer", "string"], "description": "Index or name of the view" } }), &["view"], false),
        tool("get_project_info", "Project data from the file: project information, levels with elevations, phases, grid lines and rooms.", json!({}), &[], true),
        tool("list_schedules", "Schedules in the file (Revit): index, name, column headings and row count. They are computed a few seconds after the model is in ('computing': true until then).", json!({}), &[], true),
        tool("get_schedule", "The rows of a schedule (Revit) as Revit shows them, computed from the elements, with the element ids of each row (usable with select). With show, the table opens under the 3D view.",
            json!({
                "schedule": { "type": ["integer", "string"], "description": "Index or name of the schedule" },
                "offset": { "type": "integer" },
                "limit": { "type": "integer", "description": "Default 100" },
                "show": { "type": "boolean", "description": "Open the schedule's table in the window" }
            }), &["schedule"], false),
        tool("find_elements", "Find BIM elements by their properties (category, family, type, level, name, ... as the properties panel shows them). Matching is case-insensitive and on 'contains'. With group_by, returns counts per value instead (e.g. walls per level).",
            json!({
                "text": { "type": "string", "description": "Any property value contains this (or the element id)" },
                "where": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Property conditions, e.g. {\"category\": \"Doors\", \"level\": \"Level 1\"}" },
                "layer": { "type": "string", "description": "Only elements on layers whose name contains this" },
                "group_by": { "type": "string", "description": "Property to count by, e.g. \"level\" or \"type\"" },
                "limit": { "type": "integer", "description": "Default 50" },
                "offset": { "type": "integer" }
            }), &[], true),
        tool("get_element", "All properties of an element plus its measurements: bounding box (min, max, size, centre), surface area, and volume when its mesh is closed.",
            json!({ "id": { "type": ["integer", "string"] } }), &["id"], true),
        tool("select", "Select (highlight) elements by id as a click would, and show their properties in the properties panel; or clear the selection.",
            json!({
                "ids": { "type": "array", "items": { "type": ["integer", "string"] } },
                "id": { "type": ["integer", "string"] },
                "zoom": { "type": "boolean", "description": "Also frame the selection" },
                "clear": { "type": "boolean" }
            }), &[], false),
        tool("pick", "What is under a pixel of the last viewport screenshot: the 3D point, face normal, layer, element and its properties. With select: true it also selects it, as a click.",
            json!({
                "x": { "type": "number" }, "y": { "type": "number" },
                "select": { "type": "boolean" }
            }), &["x", "y"], false),
        tool("set_view", "Turn the camera to a standard view and frame the model (as the view cube).",
            json!({ "preset": { "type": "string", "enum": ["iso", "top", "bottom", "front", "back", "left", "right"] } }), &["preset"], false),
        tool("set_camera", "Place the camera: eye and target points, or yaw/pitch (degrees; yaw 0 looks from +X, pitch 90 from above), distance to the target, field of view and projection. Omitted values stay.",
            json!({
                "eye": vec3_schema("Camera position [x, y, z]"),
                "target": vec3_schema("Point looked at [x, y, z]"),
                "yaw_deg": { "type": "number" }, "pitch_deg": { "type": "number" },
                "distance": { "type": "number" }, "fov_deg": { "type": "number" },
                "projection": { "type": "string", "enum": ["perspective", "orthographic"] }
            }), &[], false),
        tool("navigate", "Move the camera relative to where it is, as dragging and the wheel do: orbit around the target (degrees), pan (metres, screen right/up), move forward (metres) and zoom (factor, 2 = twice as close).",
            json!({
                "orbit_yaw_deg": { "type": "number" }, "orbit_pitch_deg": { "type": "number" },
                "pan_right": { "type": "number" }, "pan_up": { "type": "number" },
                "move_forward": { "type": "number" }, "zoom": { "type": "number" }
            }), &[], false),
        tool("zoom_to", "Frame the model, the selection or elements from the current direction.",
            json!({ "target": target_schema("What to frame") }), &[], false),
        tool("set_display", "Display settings: render mode (solid, wireframe, edges), ground grid, the model's grid lines, flip up axis (for files with Y up), fill of cut faces.",
            json!({
                "mode": { "type": "string", "enum": ["solid", "wireframe", "edges"] },
                "grid": { "type": "boolean" }, "grid_lines": { "type": "boolean" },
                "flip_up": { "type": "boolean" }, "section_fill": { "type": "boolean" }
            }), &[], false),
        tool("explode", "The exploded view: pull the model apart per storey (Revit levels) or per element, by a percentage per axis, and switch it on or off. Frame it afterwards with zoom_to.",
            json!({
                "enabled": { "type": "boolean" },
                "mode": { "type": "string", "enum": ["storeys", "elements"] },
                "percent": { "type": "array", "items": { "type": ["number", "null"] }, "minItems": 3, "maxItems": 3, "description": "[x, y, z] from 0 to 100; null keeps a value" }
            }), &[], false),
        tool("section_box", "The section box: switch it on or off, fit it around the model, selection or elements, set its limits (min/max [x, y, z] in the box's frame; null keeps a value) or turn it about the vertical. Everything outside the box is cut away.",
            json!({
                "enabled": { "type": "boolean" },
                "fit": target_schema("Put the box around"),
                "min": vec3_schema("Lower limits [x, y, z]; null keeps a value"),
                "max": vec3_schema("Upper limits [x, y, z]; null keeps a value"),
                "turn_deg": { "type": "number" },
                "reset": { "type": "boolean", "description": "Back around the whole model" }
            }), &[], false),
        tool("section_plane", "A single cutting plane; everything on the side its normal points to is cut away. Place it on an axis (axis z, at 1.2 cuts away everything above 1.2 m: a floor plan cut), by a point and normal, or on the face under a screenshot pixel (as clicking a face). Then flip, move it along its normal, or remove it.",
            json!({
                "axis": { "type": "string", "enum": ["x", "y", "z"] },
                "at": { "type": "number", "description": "Coordinate along the axis" },
                "point": vec3_schema("A point on the plane"),
                "normal": vec3_schema("Normal, towards the side that is cut away"),
                "screen": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "Pixel [x, y] of the last viewport screenshot" },
                "flip": { "type": "boolean", "description": "Cut away the other side" },
                "move": { "type": "number", "description": "Move along the normal (metres)" },
                "offset": { "type": "number", "description": "Set the position along the normal" },
                "remove": { "type": "boolean" }
            }), &[], false),
        tool("measure_distance", "Measure the distance between two points (also horizontal and vertical parts) and draw it in the viewport.",
            json!({ "a": point_schema("First point"), "b": point_schema("Second point"), "show": { "type": "boolean", "description": "Draw it (default true)" } }), &["a", "b"], false),
        tool("measure_angle", "Measure the angle at a vertex between two points and draw it in the viewport.",
            json!({ "a": point_schema("First point"), "vertex": point_schema("The vertex"), "b": point_schema("Second point"), "show": { "type": "boolean" } }), &["a", "vertex", "b"], false),
        tool("measure_between_elements", "Shortest distance between two elements (clearance), with the closest points, drawn in the viewport.",
            json!({ "a": { "type": ["integer", "string"] }, "b": { "type": ["integer", "string"] }, "show": { "type": "boolean" } }), &["a", "b"], false),
        tool("clear_measurements", "Remove the measurements drawn in the viewport.", json!({}), &[], false),
        tool("cancel", "Press Esc: stop placing a section plane, else clear the selection.", json!({}), &[], false),
    ]
}

// ---- the viewer -------------------------------------------------------------------------------

struct Viewer {
    conn: Option<(BufReader<TcpStream>, TcpStream, String)>,
}

impl Viewer {
    /// Connects to the announced viewer.
    fn connect_announced() -> Option<(BufReader<TcpStream>, TcpStream, String)> {
        let a = remote_link::read_announcement()?;
        let stream = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, a.port)), Duration::from_secs(2)).ok()?;
        let _ = stream.set_nodelay(true);
        Some((BufReader::new(stream.try_clone().ok()?), stream, a.token))
    }

    /// Connects, starting the viewer first when none runs.
    fn connect(&mut self) -> Result<(), String> {
        if self.conn.is_some() {
            return Ok(());
        }
        if let Some(c) = Self::connect_announced() {
            self.conn = Some(c);
            return Ok(());
        }
        let exe = viewer_exe().ok_or(
            "Open 3D Viewer Studio is not running and was not found next to this MCP server. Start the viewer, or set O3V_VIEWER to its .exe.",
        )?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // its own process group, so it stays open when the MCP client stops this server
            cmd.creation_flags(0x0000_0200 | 0x0000_0008); // CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS
        }
        cmd.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_millis(300));
            if let Some(c) = Self::connect_announced() {
                self.conn = Some(c);
                return Ok(());
            }
        }
        Err(format!("started {} but it did not answer within 30 s", exe.display()))
    }

    /// Sends a command and waits for its answer; reconnects once when the viewer was restarted.
    fn call(&mut self, cmd: &str, args: &Value, timeout: Duration) -> Result<Value, String> {
        for attempt in 0..2 {
            self.connect()?;
            let (reader, writer, token) = self.conn.as_mut().expect("connected");
            let request = json!({ "token": token, "cmd": cmd, "args": args });
            let _ = writer.set_read_timeout(Some(timeout));
            let mut line = String::new();
            let sent = writeln!(writer, "{request}").and_then(|_| writer.flush());
            let read = sent.and_then(|_| reader.read_line(&mut line));
            match read {
                Ok(n) if n > 0 => {
                    let answer: Value = serde_json::from_str(&line).map_err(|e| format!("bad answer from the viewer: {e}"))?;
                    if answer["ok"] == true {
                        return Ok(answer["result"].clone());
                    }
                    let error = answer["error"].as_str().unwrap_or("unknown error").to_string();
                    if error == "wrong token" && attempt == 0 {
                        self.conn = None; // another viewer took over the announcement
                        continue;
                    }
                    return Err(error);
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    self.conn = None;
                    return Err(format!("the viewer did not answer within {} s", timeout.as_secs()));
                }
                _ => self.conn = None, // closed: the viewer was restarted
            }
        }
        Err("lost the connection to the viewer".into())
    }
}

/// The viewer program: `O3V_VIEWER`, else `open-3d-viewer` or the newest
/// `open-3d-viewer-*` build next to this program.
fn viewer_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("O3V_VIEWER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let plain = dir.join(format!("open-3d-viewer{}", std::env::consts::EXE_SUFFIX));
    if plain.is_file() {
        return Some(plain);
    }
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            name.starts_with("open-3d-viewer-") && name.ends_with(std::env::consts::EXE_SUFFIX) && !name.contains("mcp")
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

// ---- MCP --------------------------------------------------------------------------------------

fn call_tool(viewer: &mut Viewer, name: &str, args: &Value) -> Value {
    if !tools().iter().any(|t| t["name"] == name) {
        return json!({ "content": [{ "type": "text", "text": format!("unknown tool '{name}'") }], "isError": true });
    }
    let timeout = Duration::from_secs(if name == "open_file" { 900 } else { 60 });
    match viewer.call(name, args, timeout) {
        Ok(mut result) => {
            let image = result.as_object_mut().and_then(|o| o.remove("png_base64"));
            let text = serde_json::to_string_pretty(&result).unwrap_or_default();
            let mut content = vec![];
            if let Some(Value::String(data)) = image {
                content.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
            }
            content.push(json!({ "type": "text", "text": text }));
            json!({ "content": content, "isError": false })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn handle(viewer: &mut Viewer, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone(); // notifications get no answer
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "open-3d-viewer", "title": "Open 3D Viewer Studio", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => call_tool(viewer, params["name"].as_str().unwrap_or(""), params.get("arguments").unwrap_or(&json!({}))),
        method => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn main() {
    if std::env::args().any(|a| a == "--print-config") {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let config = json!({ "mcpServers": { "open-3d-viewer": { "command": exe, "args": [] } } });
        println!("{}", serde_json::to_string_pretty(&config).unwrap_or_default());
        println!("\nClaude Code:  claude mcp add open-3d-viewer -- \"{exe}\"");
        return;
    }
    let mut viewer = Viewer { conn: None };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let answers: Vec<Value> = batch.iter().filter_map(|m| handle(&mut viewer, m)).collect();
                (!answers.is_empty()).then(|| Value::Array(answers))
            }
            Ok(msg) => handle(&mut viewer, &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(answer) = answer {
            if writeln!(stdout, "{answer}").and_then(|_| stdout.flush()).is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_unique_names_and_object_schemas() {
        let tools = tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[test]
    fn answers_initialize_and_lists_tools() {
        let mut viewer = Viewer { conn: None };
        let init = handle(&mut viewer, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&mut viewer, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let list = handle(&mut viewer, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() > 20);
        let bad = handle(&mut viewer, &json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }
}
