//! A project's issues on a BCF server: the open interface of buildingSMART (BCF API 2.1 and
//! 3.0), which the issue servers of the trade speak. The issues, their comments and their
//! viewpoints (camera, selection, visibility and picture) are brought to the same here and
//! there; of an issue both changed, the one changed last stands, with the comments of both.
//!
//! The server is named by its address and a token of the person (the interface asks OAuth2; the
//! token is what a sign-in at the server gives). Nothing goes to a server unless the person
//! names one.

use super::share::Synced;
use bim_bcf::issues::{Comment, Issues, Lens, Topic, ViewCamera, Viewpoint};
use serde_json::{json, Value};

/// A server and the project on it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Server {
    /// Its address, up to and without `/bcf`.
    pub base: String,
    pub token: String,
    /// The project's identifier on the server, and its name there.
    pub project: String,
    pub project_name: String,
    /// The version of the interface the server speaks (`3.0` or `2.1`); found at the first call.
    pub version: String,
}

impl Server {
    pub fn ready(&self) -> bool {
        !self.base.trim().is_empty() && !self.project.trim().is_empty()
    }

    fn url(&self, path: &str) -> String {
        format!("{}/bcf/{}/{path}", self.base.trim().trim_end_matches('/'), if self.version.is_empty() { "3.0" } else { &self.version })
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn point(v: Option<&Value>) -> Option<[f64; 3]> {
    let v = v?;
    Some([v.get("x")?.as_f64()?, v.get("y")?.as_f64()?, v.get("z")?.as_f64()?])
}

fn xyz(p: [f64; 3]) -> Value {
    json!({ "x": p[0], "y": p[1], "z": p[2] })
}

/// A topic of the server's answer (without its comments and viewpoints).
pub fn topic_of(v: &Value) -> Topic {
    let mut t = Topic::new(&text(v, "title"), &text(v, "creation_author"));
    t.guid = text(v, "guid");
    t.kind = text(v, "topic_type");
    t.status = text(v, "topic_status");
    t.priority = text(v, "priority");
    t.labels = v.get("labels").and_then(|l| l.as_array()).map(|l| l.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    t.description = text(v, "description");
    t.creation_date = text(v, "creation_date");
    t.modified_date = text(v, "modified_date");
    t.modified_author = text(v, "modified_author");
    t.due_date = text(v, "due_date");
    t.assigned_to = text(v, "assigned_to");
    t.stage = text(v, "stage");
    t.index = v.get("index").and_then(|x| x.as_i64());
    t
}

/// A topic as the server takes it.
pub fn topic_json(t: &Topic) -> Value {
    let mut v = json!({ "guid": t.guid, "title": t.title, "description": t.description, "labels": t.labels });
    for (key, value) in [("topic_type", &t.kind), ("topic_status", &t.status), ("priority", &t.priority), ("due_date", &t.due_date), ("assigned_to", &t.assigned_to), ("stage", &t.stage)] {
        if !value.is_empty() {
            v[key] = json!(value);
        }
    }
    v
}

pub fn comment_of(v: &Value) -> Comment {
    Comment {
        guid: text(v, "guid"),
        date: text(v, "date"),
        author: text(v, "author"),
        text: text(v, "comment"),
        viewpoint: v.get("viewpoint_guid").and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_string),
        modified_date: text(v, "modified_date"),
        modified_author: text(v, "modified_author"),
    }
}

/// A viewpoint of the server's answer, without its picture.
pub fn viewpoint_of(v: &Value) -> Viewpoint {
    let mut view = Viewpoint::new();
    view.guid = text(v, "guid");
    let camera = |c: &Value, lens: Lens| Some(ViewCamera { position: point(c.get("camera_view_point"))?, direction: point(c.get("camera_direction"))?, up: point(c.get("camera_up_vector")).unwrap_or([0.0, 0.0, 1.0]), lens });
    view.camera = match (v.get("perspective_camera").filter(|c| c.is_object()), v.get("orthogonal_camera").filter(|c| c.is_object())) {
        (Some(c), _) => camera(c, Lens::Perspective { field_of_view: c.get("field_of_view").and_then(|x| x.as_f64()).unwrap_or(60.0) }),
        (_, Some(c)) => camera(c, Lens::Orthogonal { view_to_world_scale: c.get("view_to_world_scale").and_then(|x| x.as_f64()).unwrap_or(10.0) }),
        _ => None,
    };
    let guids = |list: Option<&Value>| -> Vec<String> { list.and_then(|l| l.as_array()).map(|l| l.iter().map(|c| text(c, "ifc_guid")).filter(|g| !g.is_empty()).collect()).unwrap_or_default() };
    if let Some(components) = v.get("components") {
        view.selection = guids(components.get("selection"));
        if let Some(visibility) = components.get("visibility") {
            view.default_visibility = visibility.get("default_visibility").and_then(|x| x.as_bool()).unwrap_or(true);
            view.exceptions = guids(visibility.get("exceptions"));
        }
    }
    view
}

/// A viewpoint as the server takes it, its picture along.
pub fn viewpoint_json(view: &Viewpoint) -> Value {
    let list = |guids: &[String]| Value::Array(guids.iter().map(|g| json!({ "ifc_guid": g })).collect());
    let mut v = json!({ "guid": view.guid, "components": { "selection": list(&view.selection), "visibility": { "default_visibility": view.default_visibility, "exceptions": list(&view.exceptions) } } });
    if let Some(c) = &view.camera {
        let mut camera = json!({ "camera_view_point": xyz(c.position), "camera_direction": xyz(c.direction), "camera_up_vector": xyz(c.up), "aspect_ratio": 1.6 });
        match c.lens {
            Lens::Perspective { field_of_view } => {
                camera["field_of_view"] = json!(field_of_view);
                v["perspective_camera"] = camera;
            }
            Lens::Orthogonal { view_to_world_scale } => {
                camera["view_to_world_scale"] = json!(view_to_world_scale);
                v["orthogonal_camera"] = camera;
            }
        }
    }
    if let Some((name, bytes)) = &view.snapshot {
        let kind = if name.to_ascii_lowercase().ends_with(".png") { "png" } else { "jpg" };
        v["snapshot"] = json!({ "snapshot_type": kind, "snapshot_data": crate::remote::base64(bytes) });
    }
    v
}

/// When a topic last changed (see the shared folder: ISO dates compare as text).
fn stamp(t: &Topic) -> &str {
    let mut latest = t.creation_date.as_str().max(t.modified_date.as_str());
    for c in &t.comments {
        latest = latest.max(c.date.as_str()).max(c.modified_date.as_str());
    }
    latest
}

#[cfg(not(target_arch = "wasm32"))]
mod wire {
    use super::*;

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build()
    }

    fn said(e: ureq::Error) -> String {
        match e {
            ureq::Error::Status(401 | 403, _) => "the server does not accept the token".into(),
            ureq::Error::Status(404, _) => "the server does not know this address or project".into(),
            ureq::Error::Status(code, answer) => format!("{code}: {}", answer.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
            other => other.to_string(),
        }
    }

    fn with_token(request: ureq::Request, token: &str) -> ureq::Request {
        if token.trim().is_empty() { request } else { request.set("Authorization", &format!("Bearer {}", token.trim())) }
    }

    pub fn get(url: &str, token: &str) -> Result<Value, String> {
        let answer = with_token(agent().get(url), token).call().map_err(said)?;
        serde_json::from_str(&answer.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    pub fn bytes(url: &str, token: &str) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let answer = with_token(agent().get(url), token).call().map_err(said)?;
        let mut out = Vec::new();
        answer.into_reader().take(32 << 20).read_to_end(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    }

    pub fn send(method: &str, url: &str, token: &str, body: &Value) -> Result<Value, String> {
        let answer = with_token(agent().request(method, url), token).set("Content-Type", "application/json").send_string(&body.to_string()).map_err(said)?;
        Ok(answer.into_string().ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null))
    }

    pub fn delete(url: &str, token: &str) -> Result<(), String> {
        with_token(agent().delete(url), token).call().map(|_| ()).map_err(said)
    }
}

/// The newest version of the interface the server at `base` speaks that is read here.
#[cfg(not(target_arch = "wasm32"))]
pub fn version(base: &str, token: &str) -> Result<String, String> {
    let said = wire::get(&format!("{}/bcf/versions", base.trim().trim_end_matches('/')), token)?;
    let known: Vec<String> = said.get("versions").and_then(|v| v.as_array()).map(|v| v.iter().map(|x| text(x, "version_id")).collect()).unwrap_or_default();
    ["3.0", "2.1"].iter().find(|v| known.iter().any(|k| k == *v)).map(|v| v.to_string()).ok_or_else(|| format!("the server speaks no version read here ({})", known.join(", ")))
}

/// The projects of the server the token reaches: their identifiers and names.
#[cfg(not(target_arch = "wasm32"))]
pub fn projects(server: &Server) -> Result<Vec<(String, String)>, String> {
    let said = wire::get(&server.url("projects"), &server.token)?;
    Ok(said.as_array().map(|list| list.iter().map(|p| (text(p, "project_id"), text(p, "name"))).filter(|(id, _)| !id.is_empty()).collect()).unwrap_or_default())
}

/// What each issue said at the last time, on the server and here (see [`stamp`]): a server sets
/// its own dates, so what changed is told by what differs from then, not by whose clock is later.
pub type Seen = std::collections::HashMap<String, (String, String)>;

/// Brings the issues here and those of the project on the server to the same; `removed` are the
/// issues taken away here since the last time, `seen` what both sides said then.
#[cfg(not(target_arch = "wasm32"))]
pub fn sync(server: &Server, issues: &mut Issues, removed: &[String], seen: &mut Seen) -> Result<Synced, String> {
    let (token, topics) = (server.token.as_str(), server.url(&format!("projects/{}/topics", server.project)));
    let mut done = Synced::default();
    for guid in removed {
        // a server that keeps its topics says so; the issue is gone here either way
        let _ = wire::delete(&format!("{topics}/{guid}"), token);
    }
    let there: Vec<Topic> = wire::get(&topics, token)?.as_array().map(|list| list.iter().map(topic_of).filter(|t| !t.guid.is_empty() && !removed.contains(&t.guid)).collect()).unwrap_or_default();
    let comments_of = |guid: &str| -> Result<Vec<Comment>, String> { Ok(wire::get(&format!("{topics}/{guid}/comments"), token)?.as_array().map(|l| l.iter().map(comment_of).collect()).unwrap_or_default()) };
    let views_of = |guid: &str| -> Result<Vec<Viewpoint>, String> {
        let list = wire::get(&format!("{topics}/{guid}/viewpoints"), token)?;
        let mut views: Vec<Viewpoint> = list.as_array().map(|l| l.iter().map(viewpoint_of).collect()).unwrap_or_default();
        for (view, said) in views.iter_mut().zip(list.as_array().into_iter().flatten()) {
            if let Some(kind) = said.get("snapshot").map(|s| text(s, "snapshot_type")).filter(|k| !k.is_empty()) {
                if let Ok(bytes) = wire::bytes(&format!("{topics}/{guid}/viewpoints/{}/snapshot", view.guid), token) {
                    view.snapshot = Some((format!("{}.{}", view.guid, if kind.eq_ignore_ascii_case("png") { "png" } else { "jpg" }), bytes));
                }
            }
        }
        Ok(views)
    };
    let push_views = |topic: &Topic, known: &[String]| -> Result<(), String> {
        for view in topic.viewpoints.iter().filter(|v| !known.contains(&v.guid)) {
            wire::send("POST", &format!("{topics}/{}/viewpoints", topic.guid), token, &viewpoint_json(view))?;
        }
        Ok(())
    };
    let push_comments = |topic: &Topic, known: &[String]| -> Result<(), String> {
        for c in topic.comments.iter().filter(|c| !known.contains(&c.guid)) {
            let mut body = json!({ "guid": c.guid, "comment": c.text });
            if let Some(view) = &c.viewpoint {
                body["viewpoint_guid"] = json!(view);
            }
            wire::send("POST", &format!("{topics}/{}/comments", topic.guid), token, &body)?;
        }
        Ok(())
    };
    // what the server says of an issue after something was written to it
    let said_now = |guid: &str| -> Result<String, String> {
        let mut topic = topic_of(&wire::get(&format!("{topics}/{guid}"), token)?);
        topic.comments = comments_of(guid)?;
        Ok(stamp(&topic).to_string())
    };
    for mut theirs in there.iter().cloned() {
        theirs.comments = comments_of(&theirs.guid)?;
        let said = stamp(&theirs).to_string();
        match issues.topics.iter_mut().find(|t| t.guid == theirs.guid) {
            None => {
                theirs.viewpoints = views_of(&theirs.guid)?;
                seen.insert(theirs.guid.clone(), (said.clone(), said));
                issues.topics.push(theirs);
                done.pulled += 1;
            }
            Some(mine) => {
                let from_them: Vec<Comment> = theirs.comments.iter().filter(|c| !mine.comments.iter().any(|d| d.guid == c.guid)).cloned().collect();
                let lacking: Vec<String> = theirs.comments.iter().map(|c| c.guid.clone()).collect();
                let (mut newer_there, mut newer_here) = match seen.get(&mine.guid) {
                    Some((there, here)) => (said != *there, stamp(mine) != here),
                    None => (stamp(&theirs) > stamp(mine), stamp(&theirs) < stamp(mine)),
                };
                // both changed: the one changed last stands
                if newer_there && newer_here {
                    (newer_there, newer_here) = (stamp(&theirs) >= stamp(mine), stamp(&theirs) < stamp(mine));
                }
                let mut wrote = false;
                let has_new_comments = mine.comments.iter().any(|c| !lacking.contains(&c.guid));
                if newer_here || has_new_comments {
                    if newer_here {
                        wire::send("PUT", &format!("{topics}/{}", mine.guid), token, &topic_json(mine))?;
                    }
                    // the views first: a comment may stand at one
                    let views: Vec<String> = wire::get(&format!("{topics}/{}/viewpoints", mine.guid), token)?.as_array().map(|l| l.iter().map(|v| text(v, "guid")).collect()).unwrap_or_default();
                    push_views(mine, &views)?;
                    push_comments(mine, &lacking)?;
                    done.pushed += 1;
                    wrote = true;
                }
                if newer_there {
                    let (comments, views) = (std::mem::take(&mut mine.comments), std::mem::take(&mut mine.viewpoints));
                    *mine = theirs;
                    mine.comments.extend(comments.into_iter().filter(|c| !lacking.contains(&c.guid)));
                    let known: Vec<String> = views.iter().map(|v| v.guid.clone()).collect();
                    mine.viewpoints = views;
                    mine.viewpoints.extend(views_of(&mine.guid)?.into_iter().filter(|v| !known.contains(&v.guid)));
                } else {
                    mine.comments.extend(from_them.iter().cloned());
                }
                mine.comments.sort_by(|a, b| a.date.cmp(&b.date));
                if newer_there || !from_them.is_empty() {
                    done.pulled += 1;
                }
                let there_now = if wrote { said_now(&mine.guid)? } else { said };
                seen.insert(mine.guid.clone(), (there_now, stamp(mine).to_string()));
            }
        }
    }
    for topic in issues.topics.iter().filter(|t| !there.iter().any(|x| x.guid == t.guid)) {
        wire::send("POST", &topics, token, &topic_json(topic))?;
        push_views(topic, &[])?;
        push_comments(topic, &[])?;
        seen.insert(topic.guid.clone(), (said_now(&topic.guid)?, stamp(topic).to_string()));
        done.pushed += 1;
    }
    seen.retain(|guid, _| issues.topics.iter().any(|t| &t.guid == guid));
    Ok(done)
}

#[cfg(target_arch = "wasm32")]
pub fn version(_: &str, _: &str) -> Result<String, String> {
    Err("not in the browser".into())
}

#[cfg(target_arch = "wasm32")]
pub fn projects(_: &Server) -> Result<Vec<(String, String)>, String> {
    Err("not in the browser".into())
}

#[cfg(target_arch = "wasm32")]
pub fn sync(_: &Server, _: &mut Issues, _: &[String], _: &mut Seen) -> Result<Synced, String> {
    Err("not in the browser".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::{Arc, Mutex};

    /// A server that keeps topics, comments and viewpoints as the interface says, for one test.
    fn serve() -> (String, Arc<Mutex<Value>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let kept = Arc::new(Mutex::new(json!({ "topics": [], "comments": {}, "viewpoints": {} })));
        let state = kept.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
                let (mut length, mut token) = (0usize, false);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    let lower = header.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    token |= lower.starts_with("authorization: bearer geheim");
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let mut s = state.lock().unwrap();
                let rest: Vec<&str> = path.trim_start_matches("/bcf/").split('/').collect();
                let (code, answer): (u16, Value) = match (method.as_str(), rest.as_slice()) {
                    (_, ["versions"]) => (200, json!({ "versions": [{ "version_id": "2.1" }, { "version_id": "3.0" }] })),
                    _ if !token => (401, Value::Null),
                    ("GET", ["3.0", "projects"]) => (200, json!([{ "project_id": "p1", "name": "School" }])),
                    ("GET", ["3.0", "projects", "p1", "topics"]) => (200, s["topics"].clone()),
                    ("POST", ["3.0", "projects", "p1", "topics"]) => {
                        let mut t = body.clone();
                        t["creation_date"] = json!("2026-11-16T10:00:00Z");
                        s["topics"].as_array_mut().unwrap().push(t.clone());
                        (201, t)
                    }
                    ("GET", ["3.0", "projects", "p1", "topics", guid]) => match s["topics"].as_array().unwrap().iter().find(|t| t["guid"] == *guid) {
                        Some(t) => (200, t.clone()),
                        None => (404, Value::Null),
                    },
                    ("PUT", ["3.0", "projects", "p1", "topics", guid]) => {
                        let list = s["topics"].as_array_mut().unwrap();
                        match list.iter_mut().find(|t| t["guid"] == *guid) {
                            Some(t) => {
                                let created = t["creation_date"].clone();
                                *t = body.clone();
                                t["creation_date"] = created;
                                // the server's own clock: a minute on with each change
                                let tick = s["clock"].as_u64().unwrap_or(0) + 1;
                                s["clock"] = json!(tick);
                                let list = s["topics"].as_array_mut().unwrap();
                                let t = list.iter_mut().find(|t| t["guid"] == *guid).unwrap();
                                t["modified_date"] = json!(format!("2026-11-16T12:{tick:02}:00Z"));
                                (200, t.clone())
                            }
                            None => (404, Value::Null),
                        }
                    }
                    ("GET", ["3.0", "projects", "p1", "topics", guid, kind @ ("comments" | "viewpoints")]) => (200, s[*kind].get(*guid).cloned().unwrap_or(json!([]))),
                    ("POST", ["3.0", "projects", "p1", "topics", guid, kind @ ("comments" | "viewpoints")]) => {
                        let mut item = body.clone();
                        if *kind == "comments" {
                            item["date"] = json!("2026-11-16T11:00:00Z");
                            item["author"] = json!("an");
                        }
                        let all = s[*kind].as_object_mut().unwrap();
                        all.entry(guid.to_string()).or_insert(json!([])).as_array_mut().unwrap().push(item.clone());
                        (201, item)
                    }
                    _ => (404, Value::Null),
                };
                let text = answer.to_string();
                let _ = write!(stream, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
            }
        });
        (base, kept)
    }

    #[test]
    fn issues_go_to_a_server_and_come_from_it() {
        let (base, kept) = serve();
        assert_eq!(version(&base, "").unwrap(), "3.0");
        let mut server = Server { base: base.clone(), token: "fout".into(), version: "3.0".into(), ..Server::default() };
        assert!(projects(&server).unwrap_err().contains("token"));
        server.token = "geheim".into();
        assert_eq!(projects(&server).unwrap(), [("p1".to_string(), "School".to_string())]);
        server.project = "p1".into();
        // an issue with a comment and a view goes to the server
        let mut an = Issues::new();
        let mut topic = Topic::new("Leiding door wand", "an");
        topic.priority = "High".into();
        topic.comment("Wie pakt dit op?", "an", None);
        let mut view = Viewpoint::new();
        view.camera = Some(ViewCamera { position: [1.0, 2.0, 3.0], direction: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], lens: Lens::Perspective { field_of_view: 50.0 } });
        view.selection = vec!["0abc".into()];
        topic.viewpoints.push(view);
        an.topics.push(topic);
        let (mut an_seen, mut bo_seen) = (Seen::new(), Seen::new());
        assert_eq!(sync(&server, &mut an, &[], &mut an_seen).unwrap().pushed, 1);
        {
            let s = kept.lock().unwrap();
            assert_eq!(s["topics"][0]["title"], "Leiding door wand");
            assert_eq!(s["topics"][0]["priority"], "High");
        }
        // another takes it over, with the comment and the view
        let mut bo = Issues::new();
        assert_eq!(sync(&server, &mut bo, &[], &mut bo_seen).unwrap().pulled, 1);
        let got = &bo.topics[0];
        assert_eq!((got.title.as_str(), got.comments.len(), got.viewpoints.len()), ("Leiding door wand", 1, 1));
        assert_eq!(got.viewpoints[0].selection, ["0abc"]);
        assert_eq!(got.viewpoints[0].camera.map(|c| c.position), Some([1.0, 2.0, 3.0]));
        // bo comments: it goes to the server, and an gets it
        bo.topics[0].comment("Ik", "bo", None);
        assert_eq!(sync(&server, &mut bo, &[], &mut bo_seen).unwrap().pushed, 1);
        let done = sync(&server, &mut an, &[], &mut an_seen).unwrap();
        assert_eq!((done.pulled, an.topics[0].comments.len()), (1, 2));
        // nothing new: nothing goes
        assert!(sync(&server, &mut an, &[], &mut an_seen).unwrap().nothing());
        assert!(sync(&server, &mut bo, &[], &mut bo_seen).unwrap().nothing());
        // an closes it: it goes to the server, though the server's clock is behind an's
        an.topics[0].status = "Closed".into();
        an.topics[0].modified_date = "2026-12-01T00:00:00Z".into();
        assert_eq!(sync(&server, &mut an, &[], &mut an_seen).unwrap().pushed, 1);
        let got = sync(&server, &mut bo, &[], &mut bo_seen).unwrap();
        assert_eq!(got.pulled, 1, "{got:?} {bo_seen:?} {}", kept.lock().unwrap()["topics"]);
        assert_eq!(bo.topics[0].status, "Closed");
    }
}
