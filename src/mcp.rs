//! The MCP side of the viewer, shared by both ways an AI agent reaches it:
//!
//! - `open-3d-viewer-mcp`, the MCP server over stdio that an MCP client starts on the desktop;
//! - the viewer's own MCP endpoint over streamable HTTP, for the OpenAEC SuperCloud
//!   (OpenAEC-Blueprint `docs/supercloud-koppeling.md`, route B, and OpenAEC-Blueprint#61):
//!   with `OPENAEC_MCP_POORT` and `OPENAEC_MCP_GEHEIM` the viewer serves
//!   `http://<OPENAEC_MCP_ADRES>:<port>/mcp` for the internal network of its container.
//!
//! One set of tools for both: the definitions, the JSON-RPC handling and the answers are here; how
//! a command reaches the viewer (its control port, or its own command queue) is the caller's.
//!
//! Tool names: the desktop clients know them without a prefix (`open_file`, `get_state`). In the
//! SuperCloud every tool carries the prefix of the tool, `viewer3d_`, so they do not clash with
//! those of other tools in the same conversation, and the three the contract names have its
//! names: `viewer3d_open` (open a file in the work folder), `viewer3d_opslaan` (save) and
//! `viewer3d_stand` (what is open). A call is accepted under every one of these names.
//!
//! This file has no dependencies beyond `serde_json`, so the MCP server binary includes it with
//! `#[path]`, as it does `remote_link.rs`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The prefix of this tool's MCP tools in the SuperCloud.
pub const PREFIX: &str = "viewer3d_";

/// The names the SuperCloud contract gives to opening, saving and the state: `<tool>_open`,
/// `<tool>_opslaan` and `<tool>_stand`.
const CONTRACT_NAMES: [(&str, &str); 3] = [("open_file", "viewer3d_open"), ("save_file", "viewer3d_opslaan"), ("get_state", "viewer3d_stand")];

const INSTRUCTIONS: &str = "Open 3D Viewer Studio shows 3D and BIM models (IFC, Revit, SketchUp, glTF, OBJ, STL, PLY). \
The scene is Z up, in metres for Revit, SketchUp and glTF. Start with open_file (or get_state to see what is open), \
then screenshot to look at the viewport. Pixel coordinates (pick, screen points in measure_* and section_plane) are \
pixels of the last viewport screenshot. Elements (BIM) have ids and properties: find them with find_elements, \
inspect with get_element (size, area, volume), highlight with select. Measurements are drawn in the viewport until \
clear_measurements. Camera and section commands land at once, so a screenshot right after shows the result.";

const PREFIXED_NOTE: &str = " Here every tool name starts with viewer3d_: open_file is viewer3d_open, save_file is \
viewer3d_opslaan, get_state is viewer3d_stand, and the others are viewer3d_ plus the name used above. Files are in \
the work folder; give paths relative to it.";

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

/// The tools, under their names without a prefix (the viewer's command names).
pub fn tools() -> Vec<Value> {
    let mut save = tool("save_file", "Save the open model as IFC (the format the viewer writes), as File > Save as does, and wait until it is written. Without a path it goes next to the open file, with the same name and .ifc; an open .ifc is not replaced but saved beside it as <name>-viewer.ifc, as the viewer's IFC is a re-export (triangles and the properties it shows). A file that exists is only replaced with overwrite: true. Returns the path written.",
        json!({
            "path": { "type": "string", "description": "Where to save: absolute, or relative to the work folder (OPENAEC_MAP) when there is one. Default: next to the open file, as .ifc (<name>-viewer.ifc when the open file is an IFC)" },
            "overwrite": { "type": "boolean", "description": "Replace the file when it exists (default false)" }
        }), &[], false);
    save["annotations"]["destructiveHint"] = json!(true);
    // writes a file with save_to, and replaces one with overwrite
    let mut screenshot = tool("screenshot", "Capture what the viewer shows as a PNG image. 'viewport' (default) is the 3D view only; its pixels are the coordinates for pick and screen points. 'window' is the whole window with ribbon and panels.",
        json!({
            "area": { "type": "string", "enum": ["viewport", "window"] },
            "max_size": { "type": "integer", "description": "Longest side in pixels (default 1600)" },
            "save_to": { "type": "string", "description": "Also write the PNG to this .png path (absolute, or relative to the work folder)" },
            "overwrite": { "type": "boolean", "description": "Replace the save_to file when it exists (default false)" }
        }), &[], false);
    screenshot["annotations"]["destructiveHint"] = json!(true);
    let paging = |mut properties: Value, default: u32, max: u32| {
        properties["offset"] = json!({ "type": "integer", "description": "Skip this many (default 0)" });
        properties["limit"] = json!({ "type": "integer", "description": format!("At most this many (default {default}, at most {max})") });
        properties
    };
    vec![
        tool("get_state", "What the viewer shows now: the open model (file, counts, bounds), camera, render mode, section box and plane, selection, active saved view, viewport size and measurements.", json!({}), &[], true),
        tool("open_file", "Open a model file in the viewer (as File > Open) and wait until it is loaded. Formats: IFC (.ifc .ifczip), IFC5 (.ifcx), STEP (.stp .step .stpz), Revit (.rvt .rfa), SketchUp (.skp), Archicad (.pln .pla), Allplan (.ndw .prj), Tekla (.db1), Vertex BD (.vxz .vxm) and Hundegger BTL (.btl), DWG/DXF/DGN (.dwg .dxf .dgn), CityJSON (.json .cityjson .jsonl .cityjsonl), CityGML (.gml .citygml), GeoJSON (.geojson), Shapefile (.shp), LandXML (.xml .landxml), glTF (.gltf .glb), USD (.usd .usda .usdc .usdz), Rhino (.3dm, Rhino 4 to 8), structural analysis models in sam-json (.samjson), COLLADA (.dae), VRML/X3D (.wrl .wrz .x3d .x3dv .x3dz .x3dvz), 3MF (.3mf), AMF (.amf), FBX (.fbx), 3DS (.3ds), .obj, .off, .stl, .ply, point clouds LAS/LAZ (.las .laz), E57 (.e57), Leica PTS/PTX (.pts .ptx), PCD (.pcd), ASCII XYZ (.xyz .txt .csv .asc), also ESRI ASCII elevation grids (.asc), and OBJ/PLY/glTF point clouds. Returns the model summary.",
            json!({ "path": { "type": "string", "description": "Path of the file: absolute, or relative to the work folder (OPENAEC_MAP) when there is one" } }), &["path"], false),
        save,
        screenshot,
        tool("list_layers", "The model's layers (Revit categories, SketchUp tags, glTF materials, OBJ groups) with index, visibility, triangle and element counts and colour, a page at a time (total and truncated tell whether there are more).",
            paging(json!({ "filter": { "type": "string", "description": "Only layers whose name contains this" } }), 100, 200), &[], true),
        tool("set_layer_visibility", "Show or hide layers, or isolate them (show only these).",
            json!({
                "layers": { "description": "Layer indices and/or names (exact, else contains), or \"all\"", "oneOf": [{ "type": "string" }, { "type": "integer" }, { "type": "array", "items": { "type": ["integer", "string"] } }] },
                "visible": { "type": "boolean" },
                "isolate": { "type": "boolean", "description": "Show only these layers" }
            }), &["layers"], false),
        tool("list_views", "Views saved in the file (Revit: 3D views, floor plans, sections, elevations) with index, kind and level, a page at a time.", paging(json!({}), 100, 200), &[], true),
        tool("apply_view", "Show a saved view: its camera and what it shows (section box, plan range).",
            json!({ "view": { "type": ["integer", "string"], "description": "Index or name of the view" } }), &["view"], false),
        tool("get_project_info", "Project data from the file: project information, levels with elevations, phases, grid lines (up to 200 each) and rooms with their parameters, a page of rooms at a time (offset and limit page the rooms; room_count is the total).", paging(json!({}), 20, 50), &[], true),
        tool("list_schedules", "Schedules in the file (Revit): index, name, column headings and row count. They are computed a few seconds after the model is in ('computing': true until then).", json!({}), &[], true),
        tool("get_schedule", "The rows of a schedule (Revit) as Revit shows them, computed from the elements, with the element ids of each row (usable with select). With show, the table opens under the 3D view.",
            paging(json!({
                "schedule": { "type": ["integer", "string"], "description": "Index or name of the schedule" },
                "show": { "type": "boolean", "description": "Open the schedule's table in the window" }
            }), 100, 200), &["schedule"], false),
        tool("list_sheets", "Sheets in the file (Revit, beta): index, number, name and the names of the views placed on each.", json!({}), &[], true),
        tool("get_sheet", "A sheet (Revit, beta): its paper and the views placed on it, as boxes in sheet millimetres (x right, y up) with title, detail number, scale and the view index for apply_view. With show, the sheet opens in its tab in the window.",
            json!({
                "sheet": { "type": ["integer", "string"], "description": "Index, sheet number or name" },
                "show": { "type": "boolean", "description": "Open the sheet in the window" }
            }), &["sheet"], false),
        tool("find_elements", "Find BIM elements by their properties (category, family, type, level, name, ... as the properties panel shows them). Matching is case-insensitive and on 'contains'. With group_by, returns counts per value instead (e.g. walls per level).",
            paging(json!({
                "text": { "type": "string", "description": "Any property value contains this (or the element id)" },
                "where": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Property conditions, e.g. {\"category\": \"Doors\", \"level\": \"Level 1\"}" },
                "layer": { "type": "string", "description": "Only elements on layers whose name contains this" },
                "group_by": { "type": "string", "description": "Property to count by, e.g. \"level\" or \"type\"" }
            }), 50, 100), &[], true),
        tool("get_element", "All properties of an element plus its measurements: bounding box (min, max, size, centre), surface area, and volume when its mesh is closed. Structural nodes, members and the other analysis objects are elements too (properties built from the analysis model, bounds from their nodes).",
            json!({ "id": { "type": ["integer", "string"] } }), &["id"], true),
        tool("list_structural", "The structural analysis models in the open model (from .samjson files): per model its name, kind (analytical or physical axes), source, origin (world metres) and counts of nodes, members, surfaces, line supports, sections, load cases, combinations, actions, result sets and supports, and how many model notes (diagnostics) it has.",
            json!({}), &[], true),
        tool("get_structural_issues", "The model notes (diagnostics) of a structural analysis model: everything the importer skipped, guessed or converted (named supports expanded, default local axes, unknown profiles, ...), with the element id, kind and number of the object each is about, a page at a time.",
            paging(json!({
                "model": { "type": ["integer", "string"], "description": "Index from list_structural or (part of) the model's name; default all" },
                "code": { "type": "string", "description": "Only notes whose code contains this, e.g. \"named-support\"" },
                "object": { "type": ["integer", "string"], "description": "Only notes about this element id" }
            }), 100, 1000), &[], true),
        tool("get_structural_element", "A structural object (node, member, surface, line support, section, material, load case, combination, load or result set) as the analysis model holds it: world coordinates in f64 metres, member frame, section, insertion (cardinal point, eccentricity, end offsets), releases and support DOF codes (ux uy uz rx ry rz: 1 restrained or connected, 0 free or released, s spring, n nonlinear) with their names.",
            json!({ "id": { "type": ["integer", "string"], "description": "Element id (from find_structural, find_elements or get_structural_issues), as given; source ids are strings and are not element ids" } }), &["id"], true),
        tool("find_structural", "Find structural objects: element ids of one kind that pass the filters, a page at a time. With within, the nodes, members and surfaces inside a box (scene coordinates, as get_element's bounds): box selection. Element ids are small integers; pass them back as given (a decimal string is accepted too).",
            paging(json!({
                "kind": { "type": "string", "enum": ["node", "member", "surface", "line_support", "section", "material", "load_case", "combination", "action", "result_set"] },
                "model": { "type": ["integer", "string"], "description": "Index from list_structural or (part of) the model's name" },
                "support": { "type": "string", "description": "Nodes with this support: fixed, pinned, roller, sliding, spring, free, custom, a support name from the file, or any" },
                "section": { "type": "string", "description": "Members (or sections) whose section name contains this" },
                "member_kind": { "type": "string", "enum": ["beam", "truss", "cable", "tension_only", "compression_only", "rigid", "spring"] },
                "number": { "type": "string", "description": "Exact source number" },
                "number_from": { "type": "number", "description": "Source numbers from this (numeric)" },
                "number_to": { "type": "number", "description": "Source numbers up to this (numeric)" },
                "within": {
                    "type": "object",
                    "properties": {
                        "min": vec3_schema("Lower corner [x, y, z]"),
                        "max": vec3_schema("Upper corner [x, y, z]"),
                        "crossing": { "type": "boolean", "description": "Also objects that only cross the box (default: wholly inside)" }
                    },
                    "required": ["min", "max"]
                }
            }), 1000, 10000), &[], true),
        tool("select", "Select (highlight) elements by id as a click would, and show their properties in the properties panel; or clear the selection. Structural nodes and members are selected too; a load or line support lights up the member or node it sits on (listed under `structural` in the answer).",
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

/// The name of a tool in the SuperCloud: the contract's name, else the prefix plus the name.
pub fn prefixed_name(name: &str) -> String {
    match CONTRACT_NAMES.iter().find(|(plain, _)| *plain == name) {
        Some((_, contract)) => contract.to_string(),
        None => format!("{PREFIX}{name}"),
    }
}

/// The viewer command behind a tool name: the plain name, the prefixed one or the contract's.
pub fn command_of(name: &str) -> Option<String> {
    let plain = CONTRACT_NAMES
        .iter()
        .find(|(_, contract)| *contract == name)
        .map(|(plain, _)| plain.to_string())
        .unwrap_or_else(|| name.strip_prefix(PREFIX).unwrap_or(name).to_string());
    tools().iter().any(|t| t["name"] == plain.as_str()).then_some(plain)
}

/// The tools as `tools/list` gives them: with the SuperCloud names when `prefixed`.
pub fn tool_list(prefixed: bool) -> Vec<Value> {
    let mut tools = tools();
    if prefixed {
        for t in &mut tools {
            let name = prefixed_name(t["name"].as_str().unwrap_or_default());
            t["name"] = json!(name);
        }
    }
    tools
}

/// How long a command may take: opening and saving a large model takes minutes.
pub fn timeout_of(command: &str) -> Duration {
    Duration::from_secs(if matches!(command, "open_file" | "save_file") { 900 } else { 60 })
}

/// How a tool call reaches the viewer: command, arguments, time-out; the viewer's answer.
pub type Call<'a> = dyn FnMut(&str, &Value, Duration) -> Result<Value, String> + 'a;

fn call_tool(call: &mut Call, name: &str, args: &Value) -> Value {
    let Some(command) = command_of(name) else {
        return json!({ "content": [{ "type": "text", "text": format!("unknown tool '{name}'") }], "isError": true });
    };
    match call(&command, args, timeout_of(&command)) {
        Ok(mut result) => {
            let image = result.as_object_mut().and_then(|o| o.remove("png_base64"));
            // compact: answers stay small for the assistant
            let text = serde_json::to_string(&result).unwrap_or_default();
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

/// Answers one JSON-RPC message; `None` for a notification.
pub fn handle(msg: &Value, prefixed: bool, call: &mut Call) -> Option<Value> {
    let id = msg.get("id")?.clone(); // notifications get no answer
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOL_VERSIONS[0]);
            let instructions = if prefixed { format!("{INSTRUCTIONS}{PREFIXED_NOTE}") } else { INSTRUCTIONS.to_string() };
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "open-3d-viewer", "title": "Open 3D Viewer Studio", "version": env!("CARGO_PKG_VERSION") },
                "instructions": instructions,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tool_list(prefixed) }),
        "tools/call" => call_tool(call, params["name"].as_str().unwrap_or(""), params.get("arguments").unwrap_or(&json!({}))),
        method => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Answers a message or a batch as text; `None` when it held only notifications.
pub fn handle_text(text: &str, prefixed: bool, call: &mut Call) -> Option<String> {
    let answer = match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(batch)) => {
            let answers: Vec<Value> = batch.iter().filter_map(|m| handle(m, prefixed, call)).collect();
            (!answers.is_empty()).then(|| Value::Array(answers))
        }
        Ok(msg) => handle(&msg, prefixed, call),
        Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
    };
    answer.map(|a| a.to_string())
}

// ---- the HTTP endpoint (OpenAEC SuperCloud) -----------------------------------------------------

/// The shortest secret accepted.
const MIN_SECRET: usize = 16;
/// The largest request body (a JSON-RPC message).
const MAX_BODY: usize = 16 << 20;

/// Where the viewer serves MCP over HTTP: `OPENAEC_MCP_POORT`, `OPENAEC_MCP_GEHEIM` (every
/// request sends `Authorization: Bearer <secret>`) and `OPENAEC_MCP_ADRES` (default `0.0.0.0`,
/// the container's internal network, in managed mode; else `127.0.0.1`).
pub struct HttpConfig {
    pub addr: IpAddr,
    pub port: u16,
    secret: String,
}

impl std::fmt::Debug for HttpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never the secret
        write!(f, "HttpConfig({}:{})", self.addr, self.port)
    }
}

impl HttpConfig {
    /// From the variables `var` gives: `None` when neither port nor secret is set, an error when
    /// they do not make a working endpoint.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>, managed: bool) -> Option<Result<Self, String>> {
        let port = var("OPENAEC_MCP_POORT").filter(|v| !v.trim().is_empty());
        let secret = var("OPENAEC_MCP_GEHEIM").filter(|v| !v.is_empty());
        if port.is_none() && secret.is_none() {
            return None;
        }
        Some((|| {
            let port = port.ok_or("OPENAEC_MCP_GEHEIM is set but OPENAEC_MCP_POORT is not")?;
            let port: u16 = port.trim().parse().ok().filter(|p| *p > 0).ok_or(format!("OPENAEC_MCP_POORT={port} is not a port (1-65535)"))?;
            let secret = secret.ok_or("OPENAEC_MCP_POORT is set but OPENAEC_MCP_GEHEIM is not")?;
            if secret.chars().count() < MIN_SECRET {
                return Err(format!("OPENAEC_MCP_GEHEIM is shorter than {MIN_SECRET} characters"));
            }
            let addr = match var("OPENAEC_MCP_ADRES").filter(|v| !v.trim().is_empty()) {
                Some(a) => a.trim().parse().map_err(|_| format!("OPENAEC_MCP_ADRES={a} is not an IP address"))?,
                None if managed => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                None => IpAddr::V4(Ipv4Addr::LOCALHOST),
            };
            Ok(Self { addr, port, secret })
        })())
    }

    /// From the environment of this process.
    pub fn from_env(managed: bool) -> Option<Result<Self, String>> {
        Self::from_vars(|name| std::env::var(name).ok(), managed)
    }

    /// Binds the endpoint and serves it on a thread of its own; `handler` answers a request body
    /// (JSON-RPC) with a response body, or `None` for notifications only.
    pub fn serve(self, handler: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> std::io::Result<()> {
        let listener = TcpListener::bind((self.addr, self.port))?;
        eprintln!("MCP server listening on http://{}:{}/mcp", self.addr, self.port);
        let secret: Arc<str> = self.secret.into();
        let handler = Arc::new(handler);
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new().name("mcp-http".into()).spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    // out of file descriptors and the like: wait for some to come free, not spin
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                };
                // a thread per connection, but not more than MAX_CONNECTIONS of them, taken
                // before the secret is checked: no flood of (slow) clients uses up the threads
                if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::AcqRel);
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                    let _ = respond(&mut stream, 503, Some("text/plain; charset=utf-8"), b"too many connections");
                    continue;
                }
                let slot = Slot(active.clone());
                let (secret, handler) = (secret.clone(), handler.clone());
                let spawned = std::thread::Builder::new().spawn(move || {
                    let _slot = slot;
                    let _ = http_connection(stream, &secret, &*handler);
                });
                // no thread to be had: the connection closes (and its slot comes free)
                if spawned.is_err() {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        })?;
        Ok(())
    }
}

/// Connections served at the same time; the next ones are refused at once.
const MAX_CONNECTIONS: usize = 32;
/// The time a client has for its whole request, head and body.
const REQUEST_TIME: Duration = Duration::from_secs(30);
/// What is read of a refused request's rest before closing, at most (and for a second at most).
const DRAIN: u64 = 64 << 10;

/// A connection's place among the [`MAX_CONNECTIONS`]; given back when the connection ends.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A stream whose reads stop at a deadline, not only after a pause: a client that sends a byte
/// now and then cannot hold its connection open.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the request took too long"));
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

/// The parts of an HTTP request the endpoint looks at.
#[derive(Debug, Default)]
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// An answer without a JSON-RPC body: status and text.
type Refusal = (u16, &'static str);

/// Whether the endpoint takes a request, before its body is read: no browser (a request with an
/// `Origin` header), the right secret, the path `/mcp` and the method POST.
fn admit(req: &Request, secret: &str) -> Result<(), Refusal> {
    if req.header("origin").is_some() {
        return Err((403, "requests from a browser (with an Origin header) are not accepted"));
    }
    let token = req.header("authorization").and_then(|v| v.split_once(' ')).filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer")).map(|(_, t)| t.trim());
    if !token.is_some_and(|t| same(t.as_bytes(), secret.as_bytes())) {
        return Err((401, "missing or wrong Authorization: Bearer <OPENAEC_MCP_GEHEIM>"));
    }
    if req.path.split('?').next() != Some("/mcp") {
        return Err((404, "the MCP endpoint is /mcp"));
    }
    if req.method != "POST" {
        return Err((405, "only POST: this server sends no event stream"));
    }
    Ok(())
}

/// Compares in constant time (for equal lengths).
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn read_request(reader: &mut impl BufRead) -> std::io::Result<Option<Request>> {
    let mut lines = vec![];
    loop {
        let mut line = String::new();
        if reader.by_ref().take(8192).read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            if lines.is_empty() {
                continue; // a stray empty line before the request
            }
            break;
        }
        if lines.len() > 100 {
            return Err(std::io::Error::other("too many headers"));
        }
        lines.push(line);
    }
    let mut parts = lines[0].split_whitespace();
    let mut req = Request { method: parts.next().unwrap_or("").to_string(), path: parts.next().unwrap_or("").to_string(), headers: vec![] };
    for line in &lines[1..] {
        if let Some((name, value)) = line.split_once(':') {
            req.headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(Some(req))
}

fn respond(stream: &mut TcpStream, status: u16, content_type: Option<&str>, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n", body.len());
    if let Some(t) = content_type {
        head.push_str(&format!("Content-Type: {t}\r\n"));
    }
    match status {
        401 => head.push_str("WWW-Authenticate: Bearer\r\n"),
        405 => head.push_str("Allow: POST\r\n"),
        _ => {}
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// One request per connection: admit it, read the body, answer, close.
fn http_connection(stream: TcpStream, secret: &str, handler: &dyn Fn(&str) -> Option<String>) -> std::io::Result<()> {
    let mut write = stream.try_clone()?;
    // a client that does not take its answer does not hold the connection either
    write.set_write_timeout(Some(REQUEST_TIME))?;
    let mut reader = BufReader::new(Deadline { stream, until: Instant::now() + REQUEST_TIME });
    let answered = answer_request(&mut reader, &mut write, secret, handler);
    // Close without a reset: a socket closed with unread data in it (a refused request's body)
    // sends RST, and the client may lose the answer. Only a little, briefly.
    let _ = write.shutdown(std::net::Shutdown::Write);
    reader.get_mut().until = Instant::now() + Duration::from_secs(1);
    let _ = std::io::copy(&mut reader.by_ref().take(DRAIN), &mut std::io::sink());
    answered
}

fn answer_request(reader: &mut impl BufRead, write: &mut TcpStream, secret: &str, handler: &dyn Fn(&str) -> Option<String>) -> std::io::Result<()> {
    let Some(req) = read_request(reader)? else { return Ok(()) };
    if let Err((status, text)) = admit(&req, secret) {
        return respond(write, status, Some("text/plain; charset=utf-8"), text.as_bytes());
    }
    if req.header("transfer-encoding").is_some() {
        return respond(write, 411, Some("text/plain; charset=utf-8"), b"send Content-Length, not chunks");
    }
    let Some(length) = req.header("content-length").and_then(|v| v.parse::<usize>().ok()) else {
        return respond(write, 411, Some("text/plain; charset=utf-8"), b"Content-Length is required");
    };
    if length > MAX_BODY {
        return respond(write, 413, Some("text/plain; charset=utf-8"), b"the request is too large");
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let Ok(text) = String::from_utf8(body) else {
        return respond(write, 400, Some("text/plain; charset=utf-8"), b"the body is not UTF-8");
    };
    match handler(&text) {
        Some(answer) => respond(write, 200, Some("application/json"), answer.as_bytes()),
        None => respond(write, 202, None, b""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_viewer() -> impl FnMut(&str, &Value, Duration) -> Result<Value, String> {
        |cmd: &str, _: &Value, _: Duration| Err(format!("no viewer for {cmd}"))
    }

    #[test]
    fn tools_have_unique_names_and_object_schemas() {
        for prefixed in [false, true] {
            let tools = tool_list(prefixed);
            let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
            let n = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), n);
            assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
            assert_eq!(names.iter().all(|n| n.starts_with(PREFIX)), prefixed);
        }
    }

    #[test]
    fn supercloud_names_lead_to_the_same_commands() {
        let names: Vec<String> = tool_list(true).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for contract in ["viewer3d_open", "viewer3d_opslaan", "viewer3d_stand", "viewer3d_screenshot", "viewer3d_find_elements"] {
            assert!(names.iter().any(|n| n == contract), "{contract} is listed");
        }
        assert_eq!(command_of("viewer3d_open").as_deref(), Some("open_file"));
        assert_eq!(command_of("viewer3d_opslaan").as_deref(), Some("save_file"));
        assert_eq!(command_of("viewer3d_stand").as_deref(), Some("get_state"));
        assert_eq!(command_of("viewer3d_select").as_deref(), Some("select"));
        // the names the desktop clients use keep working, also the prefixed form of them
        assert_eq!(command_of("open_file").as_deref(), Some("open_file"));
        assert_eq!(command_of("viewer3d_open_file").as_deref(), Some("open_file"));
        assert_eq!(command_of("viewer3d_nope"), None);
        assert_eq!(command_of("calc_stand"), None);
    }

    #[test]
    fn answers_initialize_and_lists_tools() {
        let mut call = no_viewer();
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), false, &mut call).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), false, &mut call).is_none());
        let list = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }), false, &mut call).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() > 20);
        let bad = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" }), false, &mut call).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }

    #[test]
    fn calls_pass_the_command_and_return_images() {
        let mut seen = vec![];
        let mut call = |cmd: &str, args: &Value, timeout: Duration| {
            seen.push((cmd.to_string(), args.clone(), timeout));
            Ok(json!({ "width": 2, "png_base64": "AAAA" }))
        };
        let msg = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "viewer3d_open", "arguments": { "path": "a.ifc" } } });
        let answer = handle(&msg, true, &mut call).unwrap();
        let content = answer["result"]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[1]["type"], "text");
        // unknown tools are an error result, not a protocol error, and never reach the viewer
        let unknown = handle(&json!({ "jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": { "name": "rm_rf" } }), true, &mut call).unwrap();
        assert_eq!(unknown["result"]["isError"], true);
        // a batch with only a notification has no answer
        assert!(handle_text(r#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#, true, &mut call).is_none());
        assert!(handle_text("{not json", true, &mut call).unwrap().contains("-32700"));
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "open_file");
        assert_eq!(seen[0].1["path"], "a.ifc");
        assert_eq!(seen[0].2, Duration::from_secs(900));
    }

    #[test]
    fn http_config_comes_from_the_environment() {
        let vars = |list: &'static [(&'static str, &'static str)]| move |name: &str| list.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string());
        assert!(HttpConfig::from_vars(vars(&[]), true).is_none());
        let ok = HttpConfig::from_vars(vars(&[("OPENAEC_MCP_POORT", "3001"), ("OPENAEC_MCP_GEHEIM", "0123456789abcdef")]), true).unwrap().unwrap();
        assert_eq!((ok.addr, ok.port), (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 3001));
        assert!(!format!("{ok:?}").contains("0123"), "the secret stays out of logs");
        let local = HttpConfig::from_vars(vars(&[("OPENAEC_MCP_POORT", "3001"), ("OPENAEC_MCP_GEHEIM", "0123456789abcdef")]), false).unwrap().unwrap();
        assert_eq!(local.addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        let set = HttpConfig::from_vars(vars(&[("OPENAEC_MCP_POORT", "3001"), ("OPENAEC_MCP_GEHEIM", "0123456789abcdef"), ("OPENAEC_MCP_ADRES", "10.0.0.5")]), true).unwrap().unwrap();
        assert_eq!(set.addr.to_string(), "10.0.0.5");
        for bad in [
            &[("OPENAEC_MCP_POORT", "3001")][..],
            &[("OPENAEC_MCP_GEHEIM", "0123456789abcdef")][..],
            &[("OPENAEC_MCP_POORT", "3001"), ("OPENAEC_MCP_GEHEIM", "short")][..],
            &[("OPENAEC_MCP_POORT", "0"), ("OPENAEC_MCP_GEHEIM", "0123456789abcdef")][..],
            &[("OPENAEC_MCP_POORT", "70000"), ("OPENAEC_MCP_GEHEIM", "0123456789abcdef")][..],
        ] {
            let list: Vec<(String, String)> = bad.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
            let var = |name: &str| list.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
            assert!(HttpConfig::from_vars(var, true).unwrap().is_err(), "{bad:?} is refused");
        }
    }

    #[test]
    fn the_endpoint_refuses_browsers_and_wrong_secrets() {
        let secret = "0123456789abcdef";
        let req = |method: &str, path: &str, headers: &[(&str, &str)]| Request {
            method: method.into(),
            path: path.into(),
            headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        };
        let auth = ("Authorization", "Bearer 0123456789abcdef");
        assert_eq!(admit(&req("POST", "/mcp", &[auth]), secret), Ok(()));
        assert_eq!(admit(&req("POST", "/mcp?x=1", &[("authorization", "bearer 0123456789abcdef")]), secret), Ok(()));
        assert_eq!(admit(&req("POST", "/mcp", &[auth, ("Origin", "https://evil.example")]), secret).unwrap_err().0, 403);
        assert_eq!(admit(&req("POST", "/mcp", &[]), secret).unwrap_err().0, 401);
        assert_eq!(admit(&req("POST", "/mcp", &[("Authorization", "Bearer 0123456789abcdeX")]), secret).unwrap_err().0, 401);
        assert_eq!(admit(&req("POST", "/mcp", &[("Authorization", "Basic 0123456789abcdef")]), secret).unwrap_err().0, 401);
        assert_eq!(admit(&req("POST", "/", &[auth]), secret).unwrap_err().0, 404);
        assert_eq!(admit(&req("GET", "/mcp", &[auth]), secret).unwrap_err().0, 405);
    }

    #[test]
    fn a_trickling_client_is_cut_off_at_the_deadline() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // a byte every 50 ms, which no per-read time-out would ever stop
        let client = std::thread::spawn(move || {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            while s.write_all(b"P").is_ok() && s.flush().is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let (stream, _) = listener.accept().unwrap();
        let start = Instant::now();
        let mut reader = BufReader::new(Deadline { stream, until: start + Duration::from_millis(400) });
        let err = read_request(&mut reader).unwrap_err();
        assert!(matches!(err.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock), "{err}");
        assert!(start.elapsed() < Duration::from_secs(5));
        drop(reader);
        client.join().unwrap();
    }

    #[test]
    fn serves_mcp_over_http() {
        let config = HttpConfig { addr: IpAddr::V4(Ipv4Addr::LOCALHOST), port: 0, secret: "0123456789abcdef".into() };
        // bind on a free port ourselves, then serve it as `serve` does
        let listener = TcpListener::bind((config.addr, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let secret: Arc<str> = config.secret.clone().into();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let secret = secret.clone();
                std::thread::spawn(move || {
                    let _ = http_connection(stream, &secret, &|body| handle_text(body, true, &mut no_viewer()));
                });
            }
        });
        let post = |body: &str, extra: &str| {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            write!(s, "POST /mcp HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer 0123456789abcdef\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut answer = String::new();
            s.read_to_string(&mut answer).unwrap();
            answer
        };
        let list = post(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "");
        assert!(list.starts_with("HTTP/1.1 200"), "{list}");
        assert!(list.contains("viewer3d_open") && list.contains("viewer3d_opslaan"));
        assert!(post(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "").starts_with("HTTP/1.1 202"));
        assert!(post("{}", "Origin: http://localhost\r\n").starts_with("HTTP/1.1 403"));
    }
}
