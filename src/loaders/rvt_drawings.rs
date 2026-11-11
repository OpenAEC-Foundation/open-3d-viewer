//! The drawings of a Revit file: its plans, sections and elevations with the texts, dimensions and
//! tags ([`super::rvt_tags`]) drawn in them, as the `o3v::` nodes the project document takes over
//! ([`crate::document::Document::adopt_ifcx`]). So a view of the file opens as a drawing of the
//! project, in its own tab, with marks that can be picked and changed.
//!
//! Read through the generic object view, named schema fields only:
//! - an element drawn in one view only names it in its element record (`ElementInfo::view`); a
//!   view lists the elements hidden in it (`HiddenElementsViewSettings::m_hiddenElements`);
//! - a text is a `TextNote`: `m_orig` is the top of its box where its lines start (left, middle or
//!   right: the low two bits of `m_flags`), `m_upVec` its up, `m_height` the box's height and
//!   `m_lineWidth` the width its lines wrap at (both on paper), the wording RTF in
//!   `RtfData::m_arrRtfStream`. Its type (`m_attrId`, a `TextNoteAttributes`) names a `Font`
//!   element whose `m_size` is the text size;
//! - a linear dimension is a `LinearDimString` of the category dimensions (the same class in
//!   another category is a constraint, which shows nowhere): `m_refPnts` are the points it
//!   measures, `m_pDimLine` (a `GLine`: `m_origin`, `m_dirVec`) its dimension line. Its type
//!   (`m_styleSymbolId`, a `DimensionStyle`) has the font, the mark at the ends of the line
//!   (`m_arrowHeadStyleId`, a `LeaderStyle`: `m_tickType` and its size), the extension lines
//!   (type values) and the unit of the figure (`m_oLinFormatOptions`).
//!
//! Model coordinates are feet, Z up; a drawing's are metres: a plan's x and y, a section's u along
//! its line and v up.

use crate::document::uuid;
use rvt_export::views::{ViewInfo, ViewKind as K};
use rvt_model::{FieldValue, ObjectView};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const FT: f64 = 0.3048;
/// How far the document draws a dimension line off its two points, on paper (metres).
const LINE_OFFSET: f64 = 0.006;

/// The category of the dimensions that show (the others of their class are constraints).
const DIMENSIONS: i64 = -2000260;
/// What a letter's em is to the text size of a type: a text of 2.5 mm is set in 10 points.
const EM: f64 = 4.0 * 25.4 / 72.0;
/// A line's height to its em, as the document sets its texts.
const LINE: f64 = 1.3;

/// What the types of a file say of its texts and dimensions, read once each.
pub(super) struct Types<'a> {
    doc: &'a rvt_read::Document,
    /// Per text type: the em of its letters on paper (metres).
    text: HashMap<i64, Option<f64>>,
    /// Per dimension type: its name, once the document has its style.
    dimension: HashMap<i64, Option<String>>,
    /// The dimension styles in use, as the document's.
    styles: Vec<Value>,
    /// Per view: the elements hidden in it.
    hidden: HashMap<u64, HashSet<u64>>,
}

impl<'a> Types<'a> {
    pub(super) fn new(doc: &'a rvt_read::Document) -> Self {
        Self { doc, text: HashMap::new(), dimension: HashMap::new(), styles: Vec::new(), hidden: HashMap::new() }
    }

    /// The em of a font element's letters on paper (metres).
    fn em(&self, font: i64) -> Option<f64> {
        // a font element holds its font
        let o = self.doc.object(u64::try_from(font).ok()?)?;
        let size = o.path(&["m_pFont", "m_size"]).or_else(|| o.get("m_size"))?.as_f64()?;
        (size.is_finite() && size > 0.0).then_some(size * FT * EM)
    }

    /// The em of the letters of a text of this type, on paper (metres).
    pub(super) fn text_em(&mut self, kind: i64) -> Option<f64> {
        if let Some(known) = self.text.get(&kind) {
            return *known;
        }
        let font = u64::try_from(kind).ok().and_then(|id| self.doc.object(id)).and_then(|o| o.path(&["m_lineAndTextAttr", "m_fontId"])?.element());
        let em = font.and_then(|f| self.em(f));
        self.text.insert(kind, em);
        em
    }

    /// The name of the document's style for a dimension of this type.
    pub(super) fn dimension_style(&mut self, kind: i64) -> Option<String> {
        if let Some(known) = self.dimension.get(&kind) {
            return known.clone();
        }
        let style = u64::try_from(kind).ok().and_then(|id| self.doc.object(id)).and_then(|o| self.style(&o));
        let name = style.as_ref().and_then(|s| s["name"].as_str()).map(str::to_string);
        self.styles.extend(style);
        self.dimension.insert(kind, name.clone());
        name
    }

    /// The document's style for a dimension of this type.
    pub(super) fn style_of(&mut self, kind: i64) -> Option<crate::document::DimensionStyle> {
        let name = self.dimension_style(kind)?;
        self.styles.iter().find(|s| s["name"] == name.as_str()).and_then(crate::document::DimensionStyle::from_ifcx)
    }

    /// A dimension type as a style of the document.
    fn style(&self, o: &ObjectView) -> Option<Value> {
        let name = o.path(&["m_symbolInfo", "m_name"])?.as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        // type values by parameter: sizes on paper (feet)
        let values: HashMap<i64, f64> = o
            .path(&["m_pParamValueSetDouble", "m_paramSet"])
            .and_then(FieldValue::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| p.as_object())
            .filter_map(|p| Some((p.get("m_paramId")?.element()?, p.get("m_value")?.as_f64()?)))
            .collect();
        let paper = |parameter: i64, or: f64| values.get(&parameter).filter(|v| v.is_finite() && (0.0..0.1).contains(*v)).map_or(or, |v| v * FT);
        let mut style = json!({"name": name, "extension": paper(-1006404, 0.0015), "gap": paper(-1006405, 0.0015)});
        if let Some(em) = o.path(&["m_pLineAndTextAttr", "m_fontId"]).and_then(FieldValue::element).and_then(|f| self.em(f)) {
            style["size"] = em.into();
        }
        // the mark at the ends of the line: a leader style
        let mark = o.get("m_arrowHeadStyleId").and_then(FieldValue::element).and_then(|id| self.doc.object(u64::try_from(id).ok()?));
        match &mark {
            None => style["tick"] = "none".into(),
            Some(mark) => {
                let size = mark
                    .path(&["m_pParamValueSetDouble", "m_paramSet"])
                    .and_then(FieldValue::as_list)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(|p| p.as_object())
                    .find(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(-1006414))
                    .and_then(|p| p.get("m_value")?.as_f64())
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .map_or(0.0024, |s| s * FT);
                // a slash is as long as the size, half to each side of the line
                (style["tick"], style["tick_size"]) = match mark.get("m_tickType").and_then(FieldValue::as_i64) {
                    Some(3) => ("dot".into(), size.into()),
                    Some(8) => ("arrow".into(), size.into()),
                    _ => ("slash".into(), (size / 2.0).into()),
                };
            }
        }
        // the figure: millimetres unless the type says metres
        let format = |name: &str| o.path(&["m_oLinFormatOptions", name]);
        if format("m_bUseDefault") == Some(&FieldValue::Bool(false)) {
            let unit = o.path(&["m_oLinFormatOptions", "m_unitTypeId", "m_typeId"]).and_then(FieldValue::as_str).unwrap_or_default();
            let decimals = format("m_accuracy").and_then(FieldValue::as_f64).filter(|a| *a > 0.0 && *a <= 1.0).map_or(0, |a| (-a.log10()).round().clamp(0.0, 6.0) as u8);
            if unit.contains(":meters") {
                style["unit"] = "m".into();
                style["decimals"] = decimals.into();
            } else if unit.contains(":millimeters") {
                style["decimals"] = decimals.into();
            }
        }
        Some(style)
    }

    /// Whether an element is hidden in a view.
    pub(super) fn hidden(&mut self, view: u64, element: u64) -> bool {
        let doc = self.doc;
        let hidden = self.hidden.entry(view).or_insert_with(|| {
            let Some(o) = doc.object(view) else { return HashSet::new() };
            let list = o.path(&["m_oHiddenElementsViewSettings", "m_hiddenElements"]).and_then(FieldValue::as_list).unwrap_or(&[]);
            list.iter().filter_map(|e| u64::try_from(e.element()?).ok()).collect()
        });
        hidden.contains(&element)
    }

    /// The dimension styles in use, as the node the document takes its styles from.
    pub(super) fn styles_node(&self) -> Option<Value> {
        (!self.styles.is_empty()).then(|| json!({"o3v::drawings": {"version": 1, "dimension_styles": self.styles}}))
    }
}

/// A text of a view, as its type and box say: its wording with the lines its box breaks it into,
/// the em of its letters on paper (metres), the height of its box on paper (metres) and where its
/// lines start (0 left, 1 middle, 2 right).
pub(super) struct Text {
    pub wording: String,
    pub em: f64,
    pub height: f64,
    pub align: i64,
}

/// Reads a text; none for one without wording.
pub(super) fn text_of(o: &ObjectView, types: &mut Types) -> Option<Text> {
    let rtf = o.get("m_oSavedRtfData").and_then(|v| o.deref(v))?.get("m_arrRtfStream")?.as_list()?;
    let bytes: Vec<u8> = rtf.iter().filter_map(|b| b.as_i64().and_then(|b| u8::try_from(b).ok())).collect();
    let mut wording = rtf_text(&bytes);
    if wording.trim().is_empty() {
        return None;
    }
    let kind = o.get("m_attrId").and_then(FieldValue::element);
    let em = kind.and_then(|k| types.text_em(k)).or_else(|| super::rvt_drafts::rtf_points(&bytes).map(|pt| pt * 0.0254 / 72.0)).unwrap_or(crate::document::NOTE_HEIGHT);
    let paper = |name: &str| o.get(name).and_then(FieldValue::as_f64).filter(|v| v.is_finite() && *v > 0.0).map(|v| v * FT);
    // a box higher than its paragraphs: its lines break at its width
    let lines = paper("m_height").map_or(0, |h| (h / (em * LINE)).round() as usize);
    if let Some(width) = paper("m_lineWidth").filter(|_| lines > wording.lines().count()) {
        wording = wrap(&wording, ((width / (em * 0.5)).floor() as usize).max(1));
    }
    let height = paper("m_height").unwrap_or(em * LINE * wording.lines().count().max(1) as f64);
    let align = o.get("m_flags").and_then(FieldValue::as_i64).map_or(0, |f| f & 3);
    Some(Text { wording, em, height, align })
}

/// A text with lines of at most `most` letters: longer ones break between words.
fn wrap(text: &str, most: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > most {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// The scale of a view (1 : this); 100 for a view without one.
fn scale_of(v: &ViewInfo) -> f64 {
    v.scale.filter(|s| s.is_finite() && *s >= 1.0).unwrap_or(100.0)
}

/// Where a view's drawing lies in the model.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    /// x and y of the model.
    Plan,
    /// u along the line from `from` (feet) in the direction `along` (unit), v up.
    Section { from: [f64; 2], along: [f64; 2] },
}

impl Frame {
    /// A point of the model (feet) on the drawing (metres).
    fn project(&self, p: [f64; 3]) -> [f64; 2] {
        match self {
            Frame::Plan => [p[0] * FT, p[1] * FT],
            Frame::Section { from, along } => [((p[0] - from[0]) * along[0] + (p[1] - from[1]) * along[1]) * FT, p[2] * FT],
        }
    }
}

/// The texts, dimensions and tag lines of one view, on its drawing (metres).
#[derive(Debug, Default)]
struct Marks {
    notes: Vec<Value>,
    dimensions: Vec<Value>,
    lines: Vec<Value>,
}

/// The views of `views` the document can hold, as its nodes (path, attributes), and before them
/// the dimension styles they use; `file` tells the views of two files apart.
pub(super) fn drawings(doc: &rvt_read::Document, views: &[ViewInfo], file: &str, tags: &[super::rvt_tags::Tag]) -> Vec<(String, Value)> {
    let frames: HashMap<u64, (Frame, Value, &'static str)> = views.iter().filter_map(|v| Some((v.id, view_node(v)?))).collect();
    let scales: HashMap<u64, f64> = views.iter().map(|v| (v.id, scale_of(v))).collect();
    let mut marks: HashMap<u64, Marks> = HashMap::new();
    let mut types = Types::new(doc);
    let owner = |id: u64| doc.element(id).and_then(|e| e.view).and_then(|v| u64::try_from(v).ok()).filter(|v| frames.contains_key(v));
    for id in doc.object_ids_of_class("TextNote") {
        let Some(view) = owner(id).filter(|v| !types.hidden(*v, id)) else { continue };
        let Some(note) = doc.object(id).and_then(|o| text_note(&o, &frames[&view].0, scales[&view], &mut types)) else { continue };
        marks.entry(view).or_default().notes.push(note);
    }
    for id in doc.object_ids_of_class("LinearConstraint") {
        let Some(view) = owner(id).filter(|v| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id).filter(|o| o.category == Some(DIMENSIONS)) else { continue };
        marks.entry(view).or_default().dimensions.extend(dimensions(&o, &frames[&view].0, scales[&view], &mut types));
    }
    // a tag: its labels as texts, the lines of its family and its leader, all hanging from its head
    for tag in tags {
        let Some((frame, ..)) = frames.get(&tag.view) else { continue };
        let scale = scales[&tag.view];
        let head = frame.project(tag.head);
        let on = |p: [f64; 2]| [head[0] + p[0] * scale, head[1] + p[1] * scale];
        let marks = marks.entry(tag.view).or_default();
        for text in &tag.texts {
            let mut note = json!({"at": on(text.at), "text": text.wording, "size": text.em});
            match text.align {
                1 => note["align"] = "center".into(),
                2 => note["align"] = "right".into(),
                _ => {}
            }
            if text.angle != 0.0 {
                note["angle"] = text.angle.into();
            }
            marks.notes.push(note);
        }
        marks.lines.extend(tag.lines.iter().map(|l| json!([on(l[0]), on(l[1])])));
        marks.lines.extend(tag.leader_lines(&|p| frame.project(p), scale).into_iter().map(|l| json!(l)));
    }
    let styles = types.styles_node().map(|node| (uuid(&format!("rvt/{file}/drawings")), node));
    // in the order of the views, so the document lists them as the file does
    let views = views.iter().filter_map(|v| {
        let (_, mut node, key) = frames.get(&v.id)?.clone();
        node["scale"] = scale_of(v).into();
        if let Some(m) = marks.remove(&v.id) {
            node["notes"] = m.notes.into();
            node["dimensions"] = m.dimensions.into();
            node["lines"] = m.lines.into();
        }
        Some((uuid(&format!("rvt/{file}/view/{}", v.id)), json!({ key: node })))
    });
    styles.into_iter().chain(views).collect()
}

/// A view's frame, its node without marks and the node's attribute; none for a 3D view, a plan
/// without a level and a section without a box.
fn view_node(v: &ViewInfo) -> Option<(Frame, Value, &'static str)> {
    let rect = |lo: [f64; 2], hi: [f64; 2]| json!([[lo[0] * FT, lo[1] * FT], [hi[0] * FT, hi[1] * FT]]);
    if v.class == "DBViewPlan" {
        let (_, level) = v.level?;
        let name = v.level_name.as_ref()?;
        let ceiling = v.kind == K::CeilingPlan;
        let kind = match v.kind {
            K::CeilingPlan => "ceiling",
            K::StructuralPlan => "structural",
            _ => "floor",
        };
        let mut node = json!({"name": v.name, "kind": kind, "level": name});
        let range = v.view_range.as_ref();
        if let Some(cut) = v.cut_offset {
            node["cut"] = (cut * FT).into();
            // a plan looks down from its level to its view depth, a ceiling plan up from its cut
            let far = if ceiling { range.and_then(|r| r.top).map(|top| top - level - cut) } else { range.and_then(|r| r.depth.or(r.bottom)).map(|low| level - low) };
            if let Some(far) = far.filter(|f| f.is_finite()) {
                // just past the plane, so what lies on it shows
                node["depth"] = (far.max(0.0) * FT + 0.01).into();
            }
        }
        if let Some(b) = v.crop_box.as_ref().filter(|b| b.active) {
            let (lo, hi) = b.world_bounds();
            node["crop"] = rect([lo[0], lo[1]], [hi[0], hi[1]]);
        }
        return Some((Frame::Plan, node, "o3v::view::plan"));
    }
    if v.class != "DBViewSection" || !matches!(v.kind, K::Section | K::Elevation | K::Detail) {
        return None;
    }
    let b = v.crop_box.as_ref()?;
    let [right, up, forward] = b.basis;
    // level views only: the drawing's v is the model's height
    let run = right[0].hypot(right[1]);
    if up[2] < 0.999 || run < 0.999 || !(b.max[0] > b.min[0]) {
        return None;
    }
    let along = [right[0] / run, right[1] / run];
    // the document looks to the left of its line: Revit's right × up
    if -along[1] * forward[0] + along[0] * forward[1] < 0.0 {
        return None;
    }
    let at = |x: f64| [b.origin[0] + x * right[0] + b.min[2] * forward[0], b.origin[1] + x * right[1] + b.min[2] * forward[1]];
    let (from, to) = (at(b.min[0]), at(b.max[0]));
    let depth = Some(b.max[2] - b.min[2]).filter(|d| d.is_finite() && *d > 0.0 && *d < 1e6).unwrap_or(100.0 / FT);
    let kind = if v.kind == K::Elevation { "elevation" } else { "section" };
    let mut node = json!({
        "name": v.name, "kind": kind, "from": [from[0] * FT, from[1] * FT], "to": [to[0] * FT, to[1] * FT], "depth": depth * FT,
    });
    if b.active && b.max[1] > b.min[1] {
        node["crop"] = rect([0.0, b.origin[2] + b.min[1]], [b.max[0] - b.min[0], b.origin[2] + b.max[1]]);
    }
    Some((Frame::Section { from, along }, node, "o3v::view::section"))
}

pub(super) fn vec3(v: Option<&FieldValue>) -> Option<[f64; 3]> {
    let l = v?.as_list()?;
    let p = [l.first()?.as_f64()?, l.get(1)?.as_f64()?, l.get(2)?.as_f64()?];
    p.iter().all(|x| x.is_finite()).then_some(p)
}

/// A text as a note: its wording, standing on the foot of its box where its lines start.
fn text_note(o: &ObjectView, frame: &Frame, scale: f64, types: &mut Types) -> Option<Value> {
    let text = text_of(o, types)?;
    let orig = vec3(o.get("m_orig"))?;
    let up = vec3(o.get("m_upVec")).unwrap_or([0.0, 1.0, 0.0]);
    // the box's height in the model (feet)
    let height = text.height * scale / FT;
    let at = frame.project([orig[0] - up[0] * height, orig[1] - up[1] * height, orig[2] - up[2] * height]);
    let mut note = json!({"at": at, "text": text.wording, "size": text.em});
    match text.align {
        1 => note["align"] = "center".into(),
        2 => note["align"] = "right".into(),
        _ => {}
    }
    // turned as its base lies on the drawing
    if let Some(base) = vec3(o.get("m_baseVec")) {
        let (a, b) = (frame.project(orig), frame.project([orig[0] + base[0], orig[1] + base[1], orig[2] + base[2]]));
        let angle = (b[1] - a[1]).atan2(b[0] - a[0]);
        if angle.abs() > 1e-6 && (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-9 {
            note["angle"] = angle.into();
        }
    }
    Some(note)
}

/// A dimension string as the document's dimensions: one between each two of its measured points,
/// set on its dimension line, so a length is the one along that line, each in the style of its
/// type. The document draws a dimension to the left of its two points: they run so that what is
/// measured lies to the right.
fn dimensions(o: &ObjectView, frame: &Frame, scale: f64, types: &mut Types) -> Vec<Value> {
    let measured: Vec<[f64; 3]> = o.get("m_refPnts").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|p| vec3(Some(p))).collect();
    let line = o.get("m_pDimLine").and_then(|v| o.deref(v));
    let (Some(origin), Some(dir)) = (line.and_then(|l| vec3(l.get("m_origin"))), line.and_then(|l| vec3(l.get("m_dirVec")))) else { return Vec::new() };
    let style = o.get("m_styleSymbolId").and_then(FieldValue::element).and_then(|kind| types.dimension_style(kind));
    let along = |p: [f64; 3]| (0..3).map(|k| (p[k] - origin[k]) * dir[k]).sum::<f64>();
    let on_line = |t: f64| frame.project([origin[0] + t * dir[0], origin[1] + t * dir[1], origin[2] + t * dir[2]]);
    let mut stops: Vec<f64> = measured.iter().map(|&p| along(p)).collect();
    stops.sort_by(f64::total_cmp);
    // on the drawing: which side of the line the measured points lie
    let (a, b) = (on_line(0.0), on_line(1.0));
    let side: f64 = measured.iter().map(|&p| frame.project(p)).map(|q| (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0])).sum();
    stops
        .windows(2)
        .filter(|w| (w[1] - w[0]) * FT >= 0.001)
        .map(|w| if side > 0.0 { (on_line(w[1]), on_line(w[0])) } else { (on_line(w[0]), on_line(w[1])) })
        .map(|(from, to)| {
            // the document's line stands off its points: they step back, so it is where the file has it
            let run = (to[0] - from[0]).hypot(to[1] - from[1]).max(1e-9);
            let back = [(to[1] - from[1]) / run * LINE_OFFSET * scale, -(to[0] - from[0]) / run * LINE_OFFSET * scale];
            let (from, to) = ([from[0] + back[0], from[1] + back[1]], [to[0] + back[0], to[1] + back[1]]);
            match &style {
                Some(style) => json!([from, to, style]),
                None => json!([from, to]),
            }
        })
        .collect()
}

/// The wording of an RTF text: its paragraphs as lines, without its tables and formatting.
pub(super) fn rtf_text(rtf: &[u8]) -> String {
    let mut out = String::new();
    // per open group: whether its text is left out (font and colour tables, `\*` destinations)
    let mut skip = vec![false];
    let mut i = 0;
    while i < rtf.len() {
        let c = rtf[i];
        i += 1;
        let skipping = *skip.last().unwrap_or(&false);
        match c {
            b'{' => skip.push(skipping),
            b'}' => {
                if skip.len() > 1 {
                    skip.pop();
                }
            }
            b'\\' => {
                let Some(&next) = rtf.get(i) else { break };
                if next.is_ascii_alphabetic() {
                    let start = i;
                    while rtf.get(i).is_some_and(u8::is_ascii_alphabetic) {
                        i += 1;
                    }
                    let word = &rtf[start..i];
                    let digits = i;
                    if rtf.get(i) == Some(&b'-') {
                        i += 1;
                    }
                    while rtf.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    let number: Option<i32> = std::str::from_utf8(&rtf[digits..i]).ok().and_then(|n| n.parse().ok());
                    if rtf.get(i) == Some(&b' ') {
                        i += 1;
                    }
                    match word {
                        b"fonttbl" | b"colortbl" | b"stylesheet" | b"info" | b"pict" => *skip.last_mut().unwrap() = true,
                        b"par" | b"line" if !skipping => out.push('\n'),
                        b"tab" if !skipping => out.push('\t'),
                        b"u" => {
                            // a UTF-16 unit (negative above 32767), then one character for readers without Unicode
                            if let Some(ch) = number.and_then(|n| char::from_u32(n.rem_euclid(65536) as u32)).filter(|_| !skipping) {
                                out.push(ch);
                            }
                            match rtf.get(i) {
                                Some(b'\\') if rtf.get(i + 1) == Some(&b'\'') => i += 4,
                                Some(b'\\') | Some(b'{') | Some(b'}') | None => {}
                                Some(_) => i += 1,
                            }
                        }
                        _ => {}
                    }
                } else {
                    i += 1;
                    match next {
                        b'\'' => {
                            let byte = rtf.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                            i += 2;
                            if let Some(b) = byte.filter(|_| !skipping) {
                                out.push(b as char);
                            }
                        }
                        b'*' => *skip.last_mut().unwrap() = true,
                        b'~' if !skipping => out.push(' '),
                        b'\\' | b'{' | b'}' if !skipping => out.push(next as char),
                        _ => {}
                    }
                }
            }
            b'\r' | b'\n' | 0 => {}
            _ if !skipping => out.push(c as char),
            _ => {}
        }
    }
    out.trim_matches(|c: char| c == '\n' || c == ' ').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvt_export::views::OrientedBox;

    #[test]
    fn rtf_gives_its_paragraphs_as_lines() {
        let rtf = b"{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fnil\\fcharset0 Swis721 Cn BT;}}\r\n{\\colortbl ;\\red0\\green0\\blue0;}\r\n{\\*\\generator Riched20 10.0.10240}\\viewkind4\\uc1 \r\n\\pard\\cf1\\f0\\fs10 text style 1.3mm\\par\r\nvloer \\'e9\\'e9n \\u8804?\\{2\\}\\par\r\n}\r\n\0";
        assert_eq!(rtf_text(rtf), "text style 1.3mm\nvloer \u{e9}\u{e9}n \u{2264}{2}");
        assert_eq!(rtf_text(b""), "");
    }

    fn view(class: &str, kind: K) -> ViewInfo {
        ViewInfo {
            id: 7,
            name: "A".into(),
            kind,
            class: class.into(),
            camera: None,
            section_box: None,
            crop_box: None,
            level: None,
            level_name: None,
            cut_offset: None,
            view_range: None,
            scale: Some(50.0),
            is_template: false,
            owned: false,
        }
    }

    #[test]
    fn a_plan_keeps_its_level_range_and_crop() {
        let ft = |m: f64| m / FT;
        let mut v = view("DBViewPlan", K::FloorPlan);
        assert!(view_node(&v).is_none(), "no level");
        v.level = Some((1, ft(3.0)));
        v.level_name = Some("01".into());
        v.cut_offset = Some(ft(1.2));
        v.view_range = Some(rvt_export::views::ViewRange { top: None, cut: Some(ft(4.2)), bottom: Some(ft(3.0)), depth: Some(ft(2.5)) });
        v.crop_box = Some(OrientedBox { min: [ft(-1.0), ft(-2.0), 0.0], max: [ft(4.0), ft(3.0), 1.0], origin: [ft(10.0), 0.0, 0.0], basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]], active: true });
        let (frame, node, key) = view_node(&v).unwrap();
        assert_eq!((frame, key), (Frame::Plan, "o3v::view::plan"));
        assert_eq!((node["level"].as_str(), node["kind"].as_str()), (Some("01"), Some("floor")));
        let close = |v: &Value, to: f64| (v.as_f64().unwrap() - to).abs() < 1e-9;
        assert!(close(&node["cut"], 1.2) && close(&node["depth"], 0.51), "{node}");
        assert!(close(&node["crop"][0][0], 9.0) && close(&node["crop"][1][1], 3.0), "{node}");
    }

    #[test]
    fn a_section_lies_along_its_right_and_looks_to_its_left() {
        let ft = |m: f64| m / FT;
        let mut v = view("DBViewSection", K::Section);
        // looking north (+y): right is east (+x)
        v.crop_box = Some(OrientedBox { min: [ft(-2.0), ft(-1.0), 0.0], max: [ft(6.0), ft(3.0), ft(5.0)], origin: [ft(10.0), ft(20.0), ft(1.0)], basis: [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]], active: true });
        let (frame, node, key) = view_node(&v).unwrap();
        assert_eq!(key, "o3v::view::section");
        let close = |v: &Value, to: f64| (v.as_f64().unwrap() - to).abs() < 1e-9;
        assert!(close(&node["from"][0], 8.0) && close(&node["from"][1], 20.0) && close(&node["to"][0], 16.0), "{node}");
        assert!(close(&node["depth"], 5.0) && close(&node["crop"][0][1], 0.0) && close(&node["crop"][1][0], 8.0) && close(&node["crop"][1][1], 4.0), "{node}");
        // a point 1 m east of the box's origin, 2 m up
        let p = frame.project([ft(11.0), ft(23.0), ft(2.0)]);
        assert!((p[0] - 3.0).abs() < 1e-9 && (p[1] - 2.0).abs() < 1e-9, "{p:?}");
    }
}
