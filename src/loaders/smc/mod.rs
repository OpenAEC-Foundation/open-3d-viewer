//! Coordination files of a model checker (`.smc`): the models of a project put together to be
//! checked, with their elements, properties and shapes.
//!
//! The file is a zip around one stream of serialized objects, often several gigabytes (see
//! [`stream`]). It holds one model per source file (an IFC of a discipline or a supplier); each
//! becomes a layer, its elements pickable with their properties. Shapes come from [`mesh`].
//!
//! Such a file can hold far more triangles than a window shows at once (a hospital or a town
//! hall with all its suppliers: over a hundred million). The models are therefore taken from the
//! lightest up until [`MOST_TRIANGLES`] is reached; the heaviest are left out and named on
//! standard error. `O3V_SMC_TRIANGLES` sets another limit, `O3V_SMC_MODELS` (names, comma
//! separated, a part of the source file's name) takes just those models.
//!
//! The slides of the file's presentations are its issues: what the checker's user wrote up to
//! send round, each with its title, state, camera, cut planes and picture. They are kept for
//! the collaborator to take in ([`take_issues`]). The checking results (tens of thousands of
//! found clashes) are not taken as issues; shapes cut by another are drawn uncut.

mod mesh;
mod stream;

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use bim_bcf::issues::{ClippingPlane, Comment, Issues, Lens, Topic, ViewCamera, Viewpoint};
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::PathBuf;
use std::sync::Mutex;
use stream::{Data, Elem, Rec, V};

/// The issues of the file read last, with its path, until they are taken.
static ISSUES: Mutex<Option<(PathBuf, Issues)>> = Mutex::new(None);

/// The issues a coordination file brought along, once: the file's path and its issues.
pub fn take_issues() -> Option<(PathBuf, Issues)> {
    ISSUES.lock().ok()?.take()
}

/// The most triangles taken from a file, unless `O3V_SMC_TRIANGLES` says otherwise.
const MOST_TRIANGLES: usize = 24_000_000;

pub struct SmcLoader;

impl Loader for SmcLoader {
    fn name(&self) -> &'static str {
        "Coordination model (SMC)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["smc"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let source = source.clone();
        // objects lie deep in each other (a model, its classes, their elements, their shapes):
        // the reader follows them on a stack of its own
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || {
                let started = std::time::Instant::now();
                let data = match &source {
                    Source::File(path) => read(std::io::BufReader::new(std::fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", source.name()))?)),
                    Source::Bytes { data, .. } => read(std::io::Cursor::new(data.as_slice())),
                }?;
                eprintln!("TIMING smc read {:?}", started.elapsed());
                let found = issues(&data);
                if !found.topics.is_empty() {
                    if let Ok(mut kept) = ISSUES.lock() {
                        *kept = Some((source.model_path(), found));
                    }
                }
                Ok(layers(&data))
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|_| "The file could not be read".to_string())?
    }
}

/// Reads the stream of a file: the largest entry of the zip, or the file itself when it is
/// the bare stream.
fn read<R: Read + Seek>(mut file: R) -> Result<Data, String> {
    let mut magic = [0u8; 2];
    file.read_exact(&mut magic).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    if magic != *b"PK" {
        return stream::Reader::new(file).run();
    }
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Not a coordination file: {e}"))?;
    let largest = (0..zip.len()).max_by_key(|&k| zip.by_index(k).map_or(0, |f| f.size())).ok_or("The file is empty")?;
    let entry = zip.by_index(largest).map_err(|e| e.to_string())?;
    stream::Reader::new(entry).run()
}

/// The file name of a model's source, without the folder it came from and its extension.
fn model_name(data: &Data, k: usize) -> String {
    let source = data.text(data.models[k].source).unwrap_or("");
    let name = source.split("##").next().unwrap_or("");
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if stem.is_empty() { format!("Model {k}") } else { stem.to_string() }
}

/// Elements that are no part of what is built: holes, and spaces (shown apart).
fn is_void(data: &Data, e: &Elem) -> bool {
    data.text(e.ifc_type).is_some_and(|t| t.eq_ignore_ascii_case("IfcOpeningElement"))
}

fn is_space(data: &Data, e: &Elem) -> bool {
    data.text(e.ifc_type).is_some_and(|t| t.eq_ignore_ascii_case("IfcSpace"))
}

/// One layer per model (and one, switched off, for its spaces), from the lightest model up
/// while the triangles fit. The models are made on all the processor's cores at once.
fn layers(data: &Data) -> Vec<Layer> {
    let started = std::time::Instant::now();
    let source = mesh::sources(data);
    // the elements of each model, and about how heavy each model is
    let mut of_model: Vec<Vec<u32>> = vec![Vec::new(); data.models.len()];
    let mut weight = vec![0usize; data.models.len()];
    let mut weigher = mesh::Shapes::new(data, &source);
    for (k, e) in data.elems.iter().enumerate().filter(|(_, e)| !is_void(data, e)) {
        let heavy = weigher.weight(e);
        if heavy > 0 {
            of_model[e.model as usize].push(k as u32);
            weight[e.model as usize] += heavy;
        }
    }
    let most = std::env::var("O3V_SMC_TRIANGLES").ok().and_then(|v| v.parse().ok()).unwrap_or(MOST_TRIANGLES);
    let wanted: Vec<String> = std::env::var("O3V_SMC_MODELS").map(|v| v.split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let mut order: Vec<usize> = (0..data.models.len()).filter(|&k| weight[k] > 0).collect();
    order.sort_by_key(|&k| weight[k]);
    let mut taken: Vec<usize> = Vec::new();
    let mut total = 0;
    for &k in &order {
        let named = wanted.iter().any(|w| model_name(data, k).to_lowercase().contains(w));
        if (wanted.is_empty() && total + weight[k] <= most) || named {
            taken.push(k);
            total += weight[k];
        } else if wanted.is_empty() {
            eprintln!("left out, too heavy to show with the rest: {} (about {} triangles)", model_name(data, k), weight[k]);
        }
    }
    eprintln!("TIMING smc weighed {:?}", started.elapsed());
    // the heaviest first, so the threads end together; each takes the next model
    taken.sort_by_key(|&k| std::cmp::Reverse(weight[k]));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let made: Mutex<Vec<(usize, Vec<Layer>)>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 12).min(taken.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut shapes = mesh::Shapes::new(data, &source);
                loop {
                    let Some(&model) = taken.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) else { break };
                    let layers = model_layers(data, &mut shapes, model, &of_model[model]);
                    made.lock().unwrap_or_else(|e| e.into_inner()).push((model, layers));
                }
            });
        }
    });
    let mut made = made.into_inner().unwrap_or_else(|e| e.into_inner());
    made.sort_by_key(|(model, _)| *model);
    eprintln!("TIMING smc shapes {:?}", started.elapsed());
    made.into_iter().flat_map(|(_, layers)| layers).collect()
}

/// The layer of a model and the one of its spaces, from its elements.
fn model_layers(data: &Data, shapes: &mut mesh::Shapes, model: usize, elements: &[u32]) -> Vec<Layer> {
    let grey = DEFAULT_COLOR.map(|c| (c * 255.0).round() as u8);
    let name = model_name(data, model);
    let mut out: Vec<Layer> = Vec::new();
    let mut place: [Option<usize>; 2] = [None; 2];
    let mut mesh = mesh::Mesh::default();
    for &k in elements {
        let e = &data.elems[k as usize];
        mesh.clear();
        shapes.element(e, &mut mesh);
        if mesh.triangles() == 0 {
            continue;
        }
        let space = is_space(data, e);
        let at = *place[space as usize].get_or_insert_with(|| {
            out.push(Layer { name: if space { format!("{name} (IfcSpace)") } else { name.clone() }, color: DEFAULT_COLOR, colors: Some(Vec::new()), hidden: space, ..Layer::default() });
            out.len() - 1
        });
        let layer = &mut out[at];
        let first = layer.indices.len() as u32;
        let base = layer.vertex_count() as u32;
        let count = mesh.indices.len() as u32;
        layer.positions.extend(&mesh.positions);
        layer.normals.extend(&mesh.normals);
        if let Some(colors) = &mut layer.colors {
            // a shape without a colour of its own takes the layer's
            colors.extend(mesh.colors.iter().map(|c| if c[..3] == [184, 184, 189] { [grey[0], grey[1], grey[2], c[3]] } else { *c }));
        }
        layer.indices.extend(mesh.indices.iter().map(|i| base + i));
        let id = k as u64 + 1;
        layer.elements.push((id, first, count));
        layer.props.push((id, props(data, e, &name)));
    }
    out
}

/// A moment in milliseconds since 1970, as BCF writes its dates.
fn date(ms: i64) -> String {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000) / 1000);
    // the civil date of a day number (days since 1970-01-01)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let year = yoe + era * 400 + (month <= 2) as i64;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The issues of a file: the slides of its presentations, in their order.
fn issues(data: &Data) -> Issues {
    let class = |rec: &Rec| &*data.classes[rec.class as usize].short;
    let rec = |v: V| if let V::Rec(k) = v { data.recs.get(k as usize) } else { None };
    let text = |v: V| data.text(v).unwrap_or("").trim().to_string();
    let point = |v: V| if let V::Pt(k) = v { Some(data.points[k as usize].map(f64::from)) } else { None };
    // which presentation a slide is of, and the elements it is about
    let mut shown_in: HashMap<u32, String> = HashMap::new();
    let mut about: HashMap<u32, Vec<String>> = HashMap::new();
    for r in &data.recs {
        match class(r) {
            "Presentation" => {
                let name = text(data.field(r, "Category", "name"));
                for slide in r.extras.iter().flat_map(|v| data.list(*v)) {
                    if let V::Rec(k) = slide {
                        shown_in.insert(*k, name.clone());
                    }
                }
            }
            "InvolvesWithState" => {
                let V::Rec(k) = data.field(r, "Item", "parent") else { continue };
                let guids = data.list(data.field(r, "Property", "value")).iter().filter_map(|v| if let V::Elem(e) = v { data.text(data.elems[*e as usize].guid) } else { None });
                about.entry(k).or_default().extend(guids.map(str::to_string));
            }
            _ => {}
        }
    }
    let mut out = Issues::new();
    for (k, slide) in data.recs.iter().enumerate().filter(|(_, r)| class(r) == "Slide") {
        let id = if let V::Int(id) = data.field(slide, "Issue", "id") { id } else { k as i32 };
        let number = if let V::Int(n) = data.field(slide, "Problem", "trackingNumber") { n as i64 } else { 0 };
        let told = rec(data.field(slide, "Problem", "coordination"));
        let said = |field: &str| told.map(|c| text(data.field(c, "IssueCoordination", field))).unwrap_or_default();
        let when = |field: &str| told.and_then(|c| if let V::Long(n) = data.field(c, "IssueCoordination", field) { Some(date(data.longs[n as usize])) } else { None }).unwrap_or_default();
        let title = [said("title"), text(data.field(slide, "Issue", "name"))].into_iter().find(|t| !t.is_empty()).unwrap_or_else(|| format!("Issue {number}"));
        let mut topic = Topic::new(&title, &said("author"));
        // the same issue has the same identifier each time the file is read
        topic.guid = format!("534d4300-0000-4000-8000-{:012x}", id as u32);
        topic.index = Some(number);
        topic.description = said("description");
        topic.modified_author = said("modifiedBy");
        topic.priority = said("priority");
        topic.stage = said("stage");
        topic.due_date = said("dueDate");
        for (value, into) in [(said("resolution"), &mut topic.status), (said("topicType"), &mut topic.kind), (when("timeStamp"), &mut topic.creation_date), (when("modDate"), &mut topic.modified_date)] {
            if !value.is_empty() {
                *into = value;
            }
        }
        let strings = |v: V| data.list(v).iter().map(|v| text(*v)).filter(|s| !s.is_empty()).collect::<Vec<_>>();
        topic.labels = told.map(|c| strings(data.field(c, "IssueCoordination", "labels"))).unwrap_or_default();
        topic.labels.extend(shown_in.get(&(k as u32)).filter(|name| !name.is_empty()).cloned());
        topic.assigned_to = told.map(|c| strings(data.field(c, "IssueCoordination", "responsibilities")).join(", ")).unwrap_or_default();
        for (n, seen) in data.list(data.field(slide, "Issue", "issueViewpoints")).iter().filter_map(|v| rec(*v)).enumerate() {
            let mut view = Viewpoint::new();
            view.guid = format!("534d4301-{n:04x}-4000-8000-{:012x}", id as u32);
            view.selection = about.get(&(k as u32)).cloned().unwrap_or_default();
            if let Some(frame) = rec(data.field(seen, "IssueViewpoint", "animationKeyFrame")) {
                let at = |field: &str| point(data.field(frame, "AnimationKeyFrame", field));
                let number = |field: &str| if let V::Num(x) = data.field(frame, "AnimationKeyFrame", field) { x as f64 } else { 0.0 };
                if let (Some(position), Some(direction), Some(up)) = (at("targetLocation"), at("targetDirection"), at("targetUpDirection")) {
                    let lens = match data.field(frame, "AnimationKeyFrame", "isOrthogonalMode") {
                        V::Bool(true) => Lens::Orthogonal { view_to_world_scale: number("orthoScale") * 0.001 },
                        _ => Lens::Perspective { field_of_view: number("fieldOfView").to_degrees().clamp(1.0, 170.0) },
                    };
                    view.camera = Some(ViewCamera { position: position.map(|x| x * 0.001), direction, up, lens });
                }
            }
            if let Some(shown) = rec(data.field(seen, "IssueViewpoint", "visualization")) {
                for plane in data.list(data.field(shown, "Visualization", "clipPlanes")).iter().filter_map(|v| rec(*v)) {
                    let (Some(location), Some(side)) = (point(data.field(plane, "ClipPlane", "point")), rec(data.field(plane, "ClipPlane", "plane"))) else { continue };
                    let part = |field: &str| if let V::Num(x) = data.field(side, "Tuple4d", field) { x as f64 } else { 0.0 };
                    if data.field(plane, "ClipPlane", "enabled") != V::Bool(false) {
                        view.clipping_planes.push(ClippingPlane { location: location.map(|x| x * 0.001), direction: [part("x"), part("y"), part("z")] });
                    }
                }
            }
            if let Some(V::Bytes(b)) = rec(data.field(seen, "IssueViewpoint", "snapshot")).map(|s| data.field(s, "Snapshot", "pngBytes")) {
                view.snapshot = Some((if n == 0 { "snapshot.png".to_string() } else { format!("snapshot-{n}.png") }, data.blobs[b as usize].clone()));
            }
            let remark = text(data.field(seen, "IssueViewpoint", "comment"));
            if !remark.is_empty() {
                topic.comments.push(Comment { guid: format!("534d4302-{n:04x}-4000-8000-{:012x}", id as u32), date: topic.creation_date.clone(), author: topic.creation_author.clone(), text: remark, viewpoint: Some(view.guid.clone()), ..Comment::default() });
            }
            topic.viewpoints.push(view);
        }
        out.topics.push(topic);
    }
    out.topics.sort_by_key(|t| t.index);
    out
}

/// What an element says of itself, as the flat object the properties panel shows.
fn props(data: &Data, e: &Elem, model: &str) -> String {
    let mut map = serde_json::Map::new();
    let plain = |v: V| -> Option<serde_json::Value> {
        match v {
            V::Str(_) => data.text(v).filter(|s| !s.is_empty()).map(|s| s.into()),
            V::Num(x) if x.is_finite() => Some((((x as f64) * 1e4).round() / 1e4).into()),
            V::Int(n) => Some(n.into()),
            V::Bool(b) => Some(b.into()),
            _ => None,
        }
    };
    for (key, v) in [("IfcType", e.ifc_type), ("Name", e.name), ("GlobalId", e.guid), ("ObjectType", e.object_type), ("Description", e.description), ("Tag", e.tag)] {
        if let Some(value) = plain(v) {
            map.insert(key.into(), value);
        }
    }
    map.insert("Model".into(), model.into());
    for &(class, field, v) in e.own.iter() {
        if let (Some(value), Some(f)) = (plain(v), data.classes[class as usize].fields.get(field as usize)) {
            map.entry(f.name.to_string()).or_insert(value);
        }
    }
    // the sets of properties, each property as `Set.Property`
    let V::Props(all) = e.psets else { return serde_json::Value::Object(map).to_string() };
    for set in data.list(data.props[all as usize].1) {
        let V::Props(k) = *set else { continue };
        let (name, list) = data.props[k as usize];
        let set_name = data.text(name).unwrap_or("");
        for item in data.list(list) {
            let V::Named(n) = *item else { continue };
            let (key, v) = data.named[n as usize];
            if let (Some(key), Some(value)) = (data.text(key), plain(v)) {
                map.entry(if set_name.is_empty() { key.to_string() } else { format!("{set_name}.{key}") }).or_insert(value);
            }
        }
    }
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream by hand: one object of a class `Tuple3d` with three doubles, which the reader
    /// takes for a point.
    #[test]
    fn a_stream_reads_into_its_values() {
        let mut bytes: Vec<u8> = vec![0xac, 0xed, 0, 5, 0x73, 0x72];
        let name = b"javax.vecmath.Tuple3d";
        bytes.extend((name.len() as u16).to_be_bytes());
        bytes.extend(name);
        bytes.extend([0u8; 8]);
        bytes.extend([0x02, 0, 3]);
        for field in [b"x", b"y", b"z"] {
            bytes.extend([b'D', 0, 1]);
            bytes.extend(field);
        }
        bytes.extend([0x78, 0x70]);
        for value in [1.5f64, -2.0, 4000.0] {
            bytes.extend(value.to_be_bytes());
        }
        // the same point once more, by reference
        bytes.extend([0x71, 0, 0x7e, 0, 1]);
        let data = stream::Reader::new(&bytes[..]).run().unwrap();
        assert_eq!(data.tops, [V::Pt(0), V::Pt(0)]);
        assert_eq!(data.points, [[1.5, -2.0, 4000.0]]);
        assert!(!data.cut_short && data.objects == 1);
        // a stream that stops inside an object gives what was whole
        let data = stream::Reader::new(&bytes[..bytes.len() - 3]).run().unwrap();
        assert!(data.cut_short && data.tops == [V::Pt(0)]);
        assert!(stream::Reader::new(&b"PK\x03\x04"[..]).run().is_err());
    }

    #[test]
    fn a_moment_reads_as_a_date() {
        assert_eq!(date(0), "1970-01-01T00:00:00Z");
        assert_eq!(date(1_757_331_058_000), "2025-09-08T11:30:58Z");
        assert_eq!(date(951_782_400_000), "2000-02-29T00:00:00Z");
    }

    /// The project's own file, when it is there: `O3V_SMC` names it.
    #[test]
    #[ignore]
    fn a_coordination_file_opens() {
        let Ok(path) = std::env::var("O3V_SMC") else { return };
        let started = std::time::Instant::now();
        let layers = SmcLoader.load(&Source::File(path.into())).unwrap();
        let triangles: usize = layers.iter().map(Layer::triangle_count).sum();
        let elements: usize = layers.iter().map(|l| l.elements.len()).sum();
        println!("{} layers, {elements} elements, {triangles} triangles in {:.1} s", layers.len(), started.elapsed().as_secs_f32());
        assert!(triangles > 0 && elements > 0);
        let (_, issues) = take_issues().expect("the file's issues");
        println!("{} issues, {} with a picture, {} with a camera", issues.topics.len(), issues.topics.iter().filter(|t| t.viewpoints.iter().any(|v| v.snapshot.is_some())).count(), issues.topics.iter().filter(|t| t.viewpoints.iter().any(|v| v.camera.is_some())).count());
        assert!(!issues.topics.is_empty());
    }
    /// A compound shape comes with faces of its own and with its parts: where the parts are put
    /// must be where those faces are. On the project's own file (`O3V_SMC`), which has thousands.
    #[test]
    #[ignore]
    fn the_parts_of_compound_shapes_lie_where_their_own_faces_do() {
        let Ok(path) = std::env::var("O3V_SMC") else { return };
        let run = move || {
            let data = read(std::io::BufReader::new(std::fs::File::open(path).unwrap())).unwrap();
            let source = mesh::sources(&data);
            let mut shapes = mesh::Shapes::new(&data, &source);
            let bounds = |m: &mesh::Mesh| m.positions.chunks_exact(3).fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| (std::array::from_fn(|a| lo[a].min(p[a])), std::array::from_fn(|a| hi[a].max(p[a]))));
            let (mut all, mut agree) = (0, 0);
            for k in 0..data.geos.len() as u32 {
                let Some((own, parts)) = shapes.own_and_parts(k).filter(|(_, parts)| parts.triangles() > 0) else { continue };
                let (a, b) = (bounds(&own), bounds(&parts));
                all += 1;
                agree += ((0..3).all(|i| (a.0[i] - b.0[i]).abs() < 0.003 && (a.1[i] - b.1[i]).abs() < 0.003)) as usize;
            }
            println!("{agree} of {all} compound shapes agree");
            // the rest are profiles drawn as the rectangle around them
            assert!(all > 1000 && agree * 10 > all * 9, "{agree} of {all}");
        };
        std::thread::Builder::new().stack_size(1 << 30).spawn(run).unwrap().join().unwrap();
    }
    /// What a cut through one slab of the project's file gives: `O3V_SMC` names the file,
    /// `O3V_SMC_MODELS` the slab's model, `O3V_SMC_ELEMENT` its number and
    /// `O3V_SMC_CUT` the x of the cut.
    #[test]
    #[ignore]
    fn a_cut_through_a_slab_of_the_file() {
        let (Ok(path), Ok(guid), Ok(x)) = (std::env::var("O3V_SMC"), std::env::var("O3V_SMC_ELEMENT"), std::env::var("O3V_SMC_CUT")) else { return };
        let x: f32 = x.parse().unwrap();
        let layers = SmcLoader.load(&Source::File(path.clone().into())).unwrap();
        let model = crate::model::Model::new(path.into(), "SMC", layers);
        let (l, id) = model.layers.iter().enumerate().find_map(|(l, layer)| layer.elements.iter().find(|e| e.0.to_string() == guid).map(|e| (l, e.0))).expect("the element");
        let layer = &model.layers[l];
        let (_, first, count) = *layer.elements.iter().find(|e| e.0 == id).unwrap();
        let indices = &layer.indices[first as usize..(first + count) as usize];
        println!("element {id}: {} triangles, encloses volume {}, openness {:.4}", indices.len() / 3, crate::model::encloses_volume(&layer.positions, indices), crate::model::openness(&layer.positions, indices));
        let fills = crate::drawing::faces_at(&model, crate::drawing::Shown::all(), |p| [p[1], p[2], p[0] - x]);
        for fill in fills.iter().filter(|f| f.element == Some(id)) {
            for outline in &fill.loops {
                let area: f32 = (0..outline.len()).map(|k| { let (a, b) = (outline[k], outline[(k + 1) % outline.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f32>() / 2.0;
                let (lo, hi) = outline.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
                println!("   outline of {} points, {area:.4} m2, y {:.3}..{:.3} z {:.3}..{:.3}", outline.len(), lo[0], hi[0], lo[1], hi[1]);
            }
        }
        // what else is cut where this element is
        for fill in &fills {
            let Some(other) = fill.element.filter(|e| *e != id) else { continue };
            for outline in fill.loops.iter().filter(|o| o.iter().all(|p| p[0] > -2.75 && p[0] < -1.95 && p[1] > 14.55 && p[1] < 14.9)) {
                let area: f32 = (0..outline.len()).map(|k| { let (a, b) = (outline[k], outline[(k + 1) % outline.len()]); a[0] * b[1] - b[0] * a[1] }).sum::<f32>() / 2.0;
                let (lo, hi) = outline.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
                println!("   also cut: {other} in {} {:.4} m2 y {:.3}..{:.3} z {:.3}..{:.3} {}", model.layers[fill.layer].name, area, lo[0], hi[0], lo[1], hi[1], model.props.get(&other).map_or("", |p| &p[..p.len().min(150)]));
            }
        }
        let plane = crate::section::SectionPlane { normal: glam::Vec3::X, offset: x };
        let faces = crate::caps::cut_faces(&model, crate::drawing::Shown::all(), None, Some(&plane), bim_library::DrawingStandard::Nen47);
        let area: f32 = faces.triangles.iter().map(|(t, _)| (glam::Vec3::from(t[1]) - glam::Vec3::from(t[0])).cross(glam::Vec3::from(t[2]) - glam::Vec3::from(t[0])).length() / 2.0).sum();
        let here: f32 = faces.triangles.iter().filter(|(t, _)| t.iter().all(|p| p[1] > -2.65 && p[1] < -2.19 && p[2] > 14.63 && p[2] < 14.84)).map(|(t, _)| (glam::Vec3::from(t[1]) - glam::Vec3::from(t[0])).cross(glam::Vec3::from(t[2]) - glam::Vec3::from(t[0])).length() / 2.0).sum();
        let colours: std::collections::BTreeSet<[u8; 4]> = faces.triangles.iter().filter(|(t, _)| t.iter().all(|p| p[1] > -2.65 && p[1] < -2.19 && p[2] > 14.63 && p[2] < 14.84)).map(|(_, c)| *c).collect();
        let in_core = faces.triangles.iter().filter(|(t, _)| { let c = (glam::Vec3::from(t[0]) + glam::Vec3::from(t[1]) + glam::Vec3::from(t[2])) / 3.0; c.y > -2.52 && c.y < -2.45 && c.z > 14.70 && c.z < 14.77 }).count();
        println!("cut faces where the slab is: {here:.4} m2 (outline less cores is 0.0551), colours {colours:?}, triangles in the middle of a core: {in_core}");
        println!("cut faces of the whole model: {} triangles, {area:.3} m2", faces.triangles.len());
    }
}
