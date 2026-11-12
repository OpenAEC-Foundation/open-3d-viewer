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
//!   measures, `m_pDimLine` (a `GLine`: `m_origin`, `m_dirVec`) its dimension line. It is drawn
//!   as the file has it ([`dimension_marks`]): lines and figures of the view, not dimensions of
//!   the document, which measure between two points only. Its type
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
    /// Per line style: whether its lines are drawn.
    lines: HashMap<i64, bool>,
    /// Per fill pattern: whether it is a drafting pattern, sized on paper.
    patterns: HashMap<i64, bool>,
    /// Per fill pattern: whether it is the solid fill.
    solids: HashMap<i64, bool>,
    /// Per line style: its colour, when it is not black.
    colors: HashMap<i64, Option<[u8; 3]>>,
    /// Per line style: the strokes and gaps of its line pattern, when it has one.
    dashes: HashMap<i64, Option<std::sync::Arc<Vec<f64>>>>,
    /// Per view: its scale (1 : this).
    scales: HashMap<u64, f64>,
    /// Per view: its level of detail.
    details: HashMap<u64, i64>,
    /// Per type of texts (a text type, a dimension type): the typeface of its letters.
    faces: HashMap<i64, crate::typeface::Face>,
}

impl<'a> Types<'a> {
    pub(super) fn new(doc: &'a rvt_read::Document) -> Self {
        Self { doc, text: HashMap::new(), dimension: HashMap::new(), styles: Vec::new(), hidden: HashMap::new(), lines: HashMap::new(), patterns: HashMap::new(), solids: HashMap::new(), colors: HashMap::new(), dashes: HashMap::new(), scales: HashMap::new(), details: HashMap::new(), faces: HashMap::new() }
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

    /// The typeface of the letters of a text of this type (a text type, the type of a label, a
    /// dimension type): the app's own for a type that names no font.
    pub(super) fn text_face(&mut self, kind: i64) -> crate::typeface::Face {
        let doc = self.doc;
        self.faces.entry(kind).or_insert_with(|| super::rvt_fonts::face(doc, kind).unwrap_or_default()).clone()
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

    /// Whether the lines of a line style (a `GStyleElem`) are drawn: not those of the invisible
    /// lines and of the reference lines.
    pub(super) fn line_shows(&mut self, style: i64) -> bool {
        const NOT_DRAWN: [i64; 2] = [-2000064, -2000083];
        let doc = self.doc;
        *self.lines.entry(style).or_insert_with(|| {
            let category = u64::try_from(style).ok().and_then(|id| doc.object(id)).and_then(|o| o.get("m_categoryId")?.element());
            !category.is_some_and(|c| NOT_DRAWN.contains(&c))
        })
    }

    /// Whether a fill pattern (a `FillPatternElem`) is a drafting pattern, sized on paper, and not
    /// a model pattern, sized in the model (`FillPattern::m_fpTarget`).
    pub(super) fn pattern_on_paper(&mut self, pattern: i64) -> bool {
        let doc = self.doc;
        *self.patterns.entry(pattern).or_insert_with(|| {
            let o = u64::try_from(pattern).ok().and_then(|id| doc.object(id));
            o.and_then(|o| o.get("m_pFillPattern").and_then(|v| o.deref(v))?.get("m_fpTarget")?.as_i64()) == Some(0)
        })
    }

    /// Whether a fill pattern is the solid fill: one without lines.
    pub(super) fn pattern_solid(&mut self, pattern: i64) -> bool {
        let doc = self.doc;
        *self.solids.entry(pattern).or_insert_with(|| {
            let o = u64::try_from(pattern).ok().and_then(|id| doc.object(id));
            o.and_then(|o| Some(o.get("m_pFillPattern").and_then(|v| o.deref(v))?.get("m_gridsArr")?.as_list()?.is_empty())) == Some(true)
        })
    }

    /// The colour of the lines of a line style (`GStyle::m_color`: blue, green, red from the high
    /// byte down), when it is not black.
    pub(super) fn line_color(&mut self, style: i64) -> Option<[u8; 3]> {
        let doc = self.doc;
        *self.colors.entry(style).or_insert_with(|| {
            let o = u64::try_from(style).ok().and_then(|id| doc.object(id))?;
            let stored = o.get("m_pGStyle").and_then(|v| o.deref(v))?.get("m_color")?.as_i64()?;
            (stored > 0 && stored < 0x100_0000).then(|| [(stored & 0xFF) as u8, (stored >> 8 & 0xFF) as u8, (stored >> 16 & 0xFF) as u8])
        })
    }

    /// The line pattern of a line style (`GStyle::m_linePatternId`, a `LinePatternElem`): its
    /// pieces in turn, on paper (feet): a stroke as its length, a gap as minus its length, a dot
    /// as 0. None for a style that draws a solid line.
    pub(super) fn line_dashes(&mut self, style: i64) -> Option<std::sync::Arc<Vec<f64>>> {
        let doc = self.doc;
        self.dashes
            .entry(style)
            .or_insert_with(|| {
                let o = u64::try_from(style).ok().and_then(|id| doc.object(id))?;
                let pattern = o.get("m_pGStyle").and_then(|v| o.deref(v))?.get("m_linePatternId")?.element()?;
                let p = doc.object(u64::try_from(pattern).ok()?)?;
                let pieces: Vec<f64> = p
                    .get("m_pLinePattern")
                    .and_then(|v| p.deref(v))?
                    .get("m_segs")?
                    .as_list()?
                    .iter()
                    .filter_map(FieldValue::as_object)
                    .filter_map(|s| {
                        let length = s.get("m_data")?.as_f64().filter(|l| l.is_finite() && *l >= 0.0)?;
                        Some(match s.get("m_type")?.as_i64()? {
                            0 => length,
                            1 => -length,
                            _ => 0.0,
                        })
                    })
                    .collect();
                // strokes only, or nothing to step along: a solid line
                let steps: f64 = pieces.iter().map(|p| p.abs()).sum();
                (steps > 1e-6 && pieces.iter().any(|p| *p < 0.0)).then(|| std::sync::Arc::new(pieces))
            })
            .clone()
    }

    /// The level of detail of a view (its parameter -1011002: 1 coarse, 2 medium, 3 fine); medium
    /// for a view that stores none.
    pub(super) fn view_detail(&mut self, view: u64) -> i64 {
        const DETAIL_LEVEL: i64 = -1011002;
        let doc = self.doc;
        *self.details.entry(view).or_insert_with(|| {
            let o = doc.object(view);
            let stored = o.as_ref().and_then(|o| o.path(&["m_pParamValueSetInt", "m_paramSet"])?.as_list()).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
            let level = stored.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(DETAIL_LEVEL)).find_map(|p| p.get("m_value")?.as_i64());
            level.filter(|l| (1..=3).contains(l)).unwrap_or(2)
        })
    }

    /// The scale of a view (1 : this); 1 for a view without one.
    pub(super) fn view_scale(&mut self, view: u64) -> f64 {
        let doc = self.doc;
        // a view stores what a length of the model is on paper: 0.02 at 1:50
        *self.scales.entry(view).or_insert_with(|| doc.object(view).and_then(|o| o.get("m_scale")?.as_f64()).filter(|s| s.is_finite() && *s > 1e-6 && *s <= 1.0).map_or(1.0, |s| 1.0 / s))
    }

    /// The dimension styles in use, as the node the document takes its styles from.
    pub(super) fn styles_node(&self) -> Option<Value> {
        (!self.styles.is_empty()).then(|| json!({"o3v::drawings": {"version": 1, "dimension_styles": self.styles}}))
    }
}

/// A run of points drawn in a line pattern (`pattern` as [`Types::line_dashes`] gives it, `scale`
/// times as large): its strokes, each cut where the run turns. A dot is a stroke of a tenth of a
/// millimetre on paper.
pub(super) fn dashed<const N: usize>(points: &[[f64; N]], pattern: &[f64], scale: f64) -> Vec<[[f64; N]; 2]> {
    const DOT: f64 = 0.0001 / FT;
    let mut out = Vec::new();
    let period: f64 = pattern.iter().map(|p| p.abs().max(if *p == 0.0 { DOT } else { 0.0 })).sum::<f64>() * scale;
    let length: f64 = points.windows(2).map(|w| (0..N).map(|k| (w[1][k] - w[0][k]).powi(2)).sum::<f64>().sqrt()).sum();
    if period < 1e-9 || length / period > 20_000.0 {
        return points.windows(2).filter(|w| w[0] != w[1]).map(|w| [w[0], w[1]]).collect();
    }
    // where in the pattern the run is, and how much of that piece is left
    let mut piece = 0usize;
    let size = |p: f64| (if p == 0.0 { DOT } else { p.abs() }) * scale;
    let mut left = pattern.first().map_or(0.0, |p| size(*p));
    for w in points.windows(2) {
        let run = (0..N).map(|k| (w[1][k] - w[0][k]).powi(2)).sum::<f64>().sqrt();
        if run < 1e-12 {
            continue;
        }
        let at = |t: f64| -> [f64; N] { std::array::from_fn(|k| w[0][k] + (w[1][k] - w[0][k]) * t / run) };
        let mut done = 0.0;
        while done < run {
            let step = left.min(run - done);
            if pattern[piece] >= 0.0 && step > 0.0 {
                out.push([at(done), at(done + step)]);
            }
            done += step;
            left -= step;
            if left <= 1e-12 {
                piece = (piece + 1) % pattern.len();
                left = size(pattern[piece]);
            }
        }
    }
    out
}

/// A text of a view, as its type and box say: its wording with the lines its box breaks it into,
/// the em of its letters on paper (metres), the height of its box on paper (metres) and where its
/// lines start (0 left, 1 middle, 2 right).
pub(super) struct Text {
    pub wording: String,
    pub em: f64,
    pub height: f64,
    pub align: i64,
    /// The typeface of its letters, as its type and its own wording say.
    pub face: crate::typeface::Face,
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
    let mut face = kind.map(|k| types.text_face(k)).unwrap_or_default();
    let [bold, italic, underline] = super::rvt_fonts::rtf_marks(&bytes);
    (face.bold, face.italic, face.underline) = (face.bold || bold, face.italic || italic, face.underline || underline);
    let paper = |name: &str| o.get(name).and_then(FieldValue::as_f64).filter(|v| v.is_finite() && *v > 0.0).map(|v| v * FT);
    // a box higher than its paragraphs: its lines break at its width
    let lines = paper("m_height").map_or(0, |h| (h / (em * LINE)).round() as usize);
    if let Some(width) = paper("m_lineWidth").filter(|_| lines > wording.lines().count()) {
        // measured in its own letters when the system has them, else by their number
        wording = if face.installed() { face.wrap(&wording, 100.0, (width / em * 100.0) as f32) } else { wrap(&wording, ((width / (em * 0.5)).floor() as usize).max(1)) };
    }
    let height = paper("m_height").unwrap_or(em * LINE * wording.lines().count().max(1) as f64);
    let align = o.get("m_flags").and_then(FieldValue::as_i64).map_or(0, |f| f & 3);
    Some(Text { wording, em, height, align, face })
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
pub(super) fn drawings(doc: &rvt_read::Document, views: &[ViewInfo], file: &str, tags: &[super::rvt_tags::Tag], details: &[super::rvt_tags::Detail]) -> Vec<(String, Value)> {
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
        let frame = frames[&view].0;
        let Some(drawn) = dimension_marks(&o, &|p| frame.project(p), scales[&view], &mut types) else { continue };
        let marks = marks.entry(view).or_default();
        marks.lines.extend(drawn.lines.iter().map(|l| json!(l)));
        marks.notes.extend(drawn.texts.iter().map(dimension_note));
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
            if let Some(font) = text.face.to_json() {
                note["font"] = font;
            }
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
    // a detail component: the lines of its family, within the view's crop as the model is
    for detail in details {
        let Some((frame, node, _)) = frames.get(&detail.view) else { continue };
        let crop = crop_of(node);
        let lines = detail.lines.iter().filter_map(|l| within(frame.project(l[0]), frame.project(l[1]), crop));
        marks.entry(detail.view).or_default().lines.extend(lines.map(|l| json!(l)));
    }
    // a detail line: a straight one as it is, an arc or circle in pieces, within the view's crop
    for id in doc.object_ids_of_class("CurveElem") {
        let Some(view) = owner(id).filter(|v| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id).filter(|o| o.category == Some(LINES)) else { continue };
        if o.get("m_categoryId").and_then(FieldValue::element).is_some_and(|style| style != -1 && !types.line_shows(style)) {
            continue;
        }
        let (frame, node, _) = &frames[&view];
        let crop = crop_of(node);
        let points = curve_points(&o);
        let lines = points.windows(2).filter_map(|w| within(frame.project(w[0]), frame.project(w[1]), crop));
        marks.entry(view).or_default().lines.extend(lines.map(|l| json!(l)));
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

/// The category of detail lines.
const LINES: i64 = -2000051;

/// The crop of a view's node (lower left, upper right; metres), when it has one.
fn crop_of(node: &Value) -> Option<([f64; 2], [f64; 2])> {
    let corner = |k: usize| Some([node["crop"][k][0].as_f64()?, node["crop"][k][1].as_f64()?]);
    Some((corner(0)?, corner(1)?))
}

/// The part of the line `a`–`b` within a crop (all of it without one); none when it lies outside.
fn within(a: [f64; 2], b: [f64; 2], crop: Option<([f64; 2], [f64; 2])>) -> Option<[[f64; 2]; 2]> {
    let Some((lo, hi)) = crop else { return Some([a, b]) };
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d[0], a[0] - lo[0]), (d[0], hi[0] - a[0]), (-d[1], a[1] - lo[1]), (d[1], hi[1] - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    (t0 <= t1).then(|| [[a[0] + t0 * d[0], a[1] + t0 * d[1]], [a[0] + t1 * d[0], a[1] + t1 * d[1]]])
}

/// The points of a detail line in the model (feet): the ends of a straight one, an arc or circle
/// in pieces of at most 7.5 degrees; none for another curve.
fn curve_points(o: &ObjectView) -> Vec<[f64; 3]> {
    if !matches!(o.get("m_detail"), Some(FieldValue::Bool(true))) {
        return Vec::new();
    }
    let Some(curve) = o.get("m_pCurveDriver").and_then(|v| o.deref(v)).and_then(|d| d.get("m_pCrv")).and_then(|v| o.deref(v)) else { return Vec::new() };
    let ends = curve.get("m_endParams").and_then(FieldValue::as_list).unwrap_or(&[]);
    let end = |i: usize| ends.get(i).and_then(FieldValue::as_f64).filter(|t| t.is_finite() && t.abs() < 1e6).unwrap_or(0.0);
    let (from, to) = (end(0), end(1));
    match curve.class.as_str() {
        "GLine" => {
            let (Some(origin), Some(dir)) = (vec3(curve.get("m_origin")), vec3(curve.get("m_dirVec"))) else { return Vec::new() };
            [from, to].iter().map(|t| std::array::from_fn(|k| origin[k] + t * dir[k])).collect()
        }
        "GArc" => {
            let (Some(c), Some(x), Some(y)) = (vec3(curve.get("m_center")), vec3(curve.get("m_xVec")), vec3(curve.get("m_yVec"))) else { return Vec::new() };
            let Some(r) = curve.get("m_radius").and_then(FieldValue::as_f64).filter(|r| r.is_finite() && *r > 0.0) else { return Vec::new() };
            // equal ends: a full circle
            let sweep = if (to - from).abs() < 1e-9 { std::f64::consts::TAU } else { to - from };
            let pieces = ((sweep.abs() / (std::f64::consts::TAU / 48.0)).ceil() as usize).clamp(2, 96);
            (0..=pieces)
                .map(|i| {
                    let (s, k) = (from + sweep * i as f64 / pieces as f64).sin_cos();
                    std::array::from_fn(|n| c[n] + r * (k * x[n] + s * y[n]))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// A figure of a dimension as a note: standing on the middle of its foot, turned along its line.
fn dimension_note(text: &DimensionText) -> Value {
    let mut note = json!({"at": text.at, "text": text.wording, "size": text.em, "align": "center"});
    if let Some(font) = text.face.to_json() {
        note["font"] = font;
    }
    if text.angle.abs() > 1e-9 {
        note["angle"] = text.angle.into();
    }
    note
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

/// The origin of a view's own frame on its drawing (metres), the point a sheet places, and the
/// direction of the frame's x on the drawing: a plan's crop may be turned (its `basis`), and the
/// sheet shows the view along its crop, not along the model's x and y.
pub(super) fn view_origin(v: &ViewInfo) -> Option<([f64; 2], [f64; 2])> {
    let (frame, ..) = view_node(v)?;
    let b = v.crop_box.as_ref()?;
    let turn = match frame {
        Frame::Plan => {
            let [x, y, _] = b.basis[0];
            let run = x.hypot(y);
            if run > 1e-9 { [x / run, y / run] } else { [1.0, 0.0] }
        }
        Frame::Section { .. } => [1.0, 0.0],
    };
    Some((frame.project(b.origin), turn))
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
    if let Some(font) = text.face.to_json() {
        note["font"] = font;
    }
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

/// A dimension as the file draws it: its lines (dimension line, witness lines, the marks of its
/// style) and its figures, on a drawing (metres).
#[derive(Debug, Default)]
pub(super) struct DimensionMarks {
    pub lines: Vec<[[f64; 2]; 2]>,
    pub texts: Vec<DimensionText>,
}

/// A figure of a dimension: the middle of the foot of its lines, how it is turned (radians,
/// counter-clockwise), its wording and the em of its letters on paper (metres).
#[derive(Debug)]
pub(super) struct DimensionText {
    pub at: [f64; 2],
    pub angle: f64,
    pub wording: String,
    pub em: f64,
    /// The typeface of its letters, as its type says.
    pub face: crate::typeface::Face,
}

/// How far a figure stands off its dimension line, on paper (metres).
const FIGURE_OFF: f64 = 0.0008;

/// A dimension string as it is drawn in a view of `scale`, where `project` puts a point of the
/// model (feet) on the drawing (metres):
/// - the dimension line through the feet of the points it measures (`m_refPnts`) on `m_pDimLine`;
/// - a witness line from just off each point to just past the line, and the mark of its style;
/// - per piece its figure over the middle, moved as the file says (`DimSegInfo::m_offset`: along
///   the line and off it), in the unit of its style, with what the file writes around or instead
///   of it (`DimSegValueTextFields`: prefix, suffix, a text instead, a line above and below). It
///   reads from below or from the right.
pub(super) fn dimension_marks(o: &ObjectView, project: &dyn Fn([f64; 3]) -> [f64; 2], scale: f64, types: &mut Types) -> Option<DimensionMarks> {
    let measured: Vec<[f64; 3]> = o.get("m_refPnts").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|p| vec3(Some(p))).collect();
    let line = o.get("m_pDimLine").and_then(|v| o.deref(v))?;
    let (origin, dir) = (vec3(line.get("m_origin"))?, vec3(line.get("m_dirVec"))?);
    let style = o.get("m_styleSymbolId").and_then(FieldValue::element).and_then(|kind| types.style_of(kind)).unwrap_or_else(crate::document::DimensionStyle::standard);
    let face = o.get("m_styleSymbolId").and_then(FieldValue::element).map(|kind| types.text_face(kind)).unwrap_or_default();
    // the line on the drawing: a point of it and its direction
    let from = project(origin);
    let to = project([origin[0] + dir[0], origin[1] + dir[1], origin[2] + dir[2]]);
    let run = (to[0] - from[0]).hypot(to[1] - from[1]);
    if run < 1e-9 {
        return None;
    }
    let d = [(to[0] - from[0]) / run, (to[1] - from[1]) / run];
    let along = |q: [f64; 2]| (q[0] - from[0]) * d[0] + (q[1] - from[1]) * d[1];
    let on_line = |t: f64| [from[0] + t * d[0], from[1] + t * d[1]];
    // the direction its figures read in, and their up
    let t = if d[0] > 1e-9 || d[0].abs() <= 1e-9 && d[1] > 0.0 { d } else { [-d[0], -d[1]] };
    let up = [-t[1], t[0]];
    let mut marks = DimensionMarks::default();
    let points: Vec<[f64; 2]> = measured.iter().map(|&p| project(p)).collect();
    let mut stops: Vec<f64> = points.iter().map(|&q| along(q)).collect();
    stops.sort_by(f64::total_cmp);
    stops.dedup_by(|a, b| (*a - *b).abs() < 0.0005);
    let (Some(&first), Some(&last)) = (stops.first(), stops.last()) else { return None };
    if stops.len() < 2 {
        return None;
    }
    marks.lines.push([on_line(first), on_line(last)]);
    // the witness lines
    let (gap, past) = (style.gap * scale, style.extension * scale);
    for &q in &points {
        let foot = on_line(along(q));
        let length = (foot[0] - q[0]).hypot(foot[1] - q[1]);
        if length < 1e-9 {
            marks.lines.push([foot, [foot[0] + up[0] * past, foot[1] + up[1] * past]]);
            continue;
        }
        let w = [(foot[0] - q[0]) / length, (foot[1] - q[1]) / length];
        let start = gap.min(length);
        marks.lines.push([[q[0] + w[0] * start, q[1] + w[1] * start], [foot[0] + w[0] * past, foot[1] + w[1] * past]]);
    }
    // the mark at each point
    let size = style.tick_size * scale;
    for (k, &stop) in stops.iter().enumerate() {
        let q = on_line(stop);
        match style.tick {
            crate::document::Tick::Slash => {
                let s = [(t[0] + up[0]) * size, (t[1] + up[1]) * size];
                marks.lines.push([[q[0] - s[0], q[1] - s[1]], [q[0] + s[0], q[1] + s[1]]]);
            }
            crate::document::Tick::Dot => {
                let r = size / 2.0;
                let ring: Vec<[f64; 2]> = (0..=12).map(|i| (std::f64::consts::TAU * i as f64 / 12.0).sin_cos()).map(|(s, c)| [q[0] + r * c, q[1] + r * s]).collect();
                marks.lines.extend(ring.windows(2).map(|w| [w[0], w[1]]));
                marks.lines.push([[q[0] - r * d[0], q[1] - r * d[1]], [q[0] + r * d[0], q[1] + r * d[1]]]);
            }
            crate::document::Tick::Arrow => {
                // a head towards the point from each piece that ends on it
                for inward in [(k > 0).then_some(-1.0), (k + 1 < stops.len()).then_some(1.0)].into_iter().flatten() {
                    let back = [q[0] + d[0] * size * inward, q[1] + d[1] * size * inward];
                    let wing = [-d[1] * size * 0.27, d[0] * size * 0.27];
                    let (l, r) = ([back[0] + wing[0], back[1] + wing[1]], [back[0] - wing[0], back[1] - wing[1]]);
                    marks.lines.extend([[q, l], [l, r], [r, q]]);
                }
            }
            crate::document::Tick::None => {}
        }
    }
    // what the file says of each piece, by where its middle lies on the line
    let pieces: Vec<&rvt_model::NestedObject> = o.get("m_ArrSegInfo").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object).collect();
    let said = |middle: f64, length: f64| {
        let near = |s: &&&rvt_model::NestedObject| vec3(s.get("m_origin")).map(|p| (along(project(p)) - middle).abs());
        pieces.iter().filter(|s| near(s).is_some_and(|off| off < length / 2.0)).min_by(|a, b| near(a).unwrap_or(f64::MAX).total_cmp(&near(b).unwrap_or(f64::MAX))).copied()
    };
    let lead = style.size * LINE * scale;
    for w in stops.windows(2) {
        let (length, middle) = (w[1] - w[0], (w[0] + w[1]) / 2.0);
        let piece = said(middle, length);
        let fields = piece.and_then(|s| s.get("m_values")?.as_list()?.first()?.as_object()?.get("m_oTextFields")).and_then(|v| o.deref(v));
        let field = |name: &str| fields.and_then(|f| f.get(name)?.as_str()).unwrap_or_default().to_string();
        let moved = piece.and_then(|s| s.get("m_offset")?.as_list()).and_then(|l| Some([l.first()?.as_f64()? * FT, l.get(1)?.as_f64()? * FT])).filter(|m| m.iter().all(|v| v.is_finite())).unwrap_or([0.0, 0.0]);
        let centre = on_line(middle + moved[0]);
        let centre = [centre[0] + up[0] * moved[1], centre[1] + up[1] * moved[1]];
        let instead = field("m_valueOverride");
        let mut wording = if instead.is_empty() { format!("{}{}{}", field("m_prefix"), style.figure(length), field("m_suffix")) } else { instead };
        let above = field("m_above");
        if !above.is_empty() {
            wording = format!("{above}\n{wording}");
        }
        let angle = t[1].atan2(t[0]);
        let off = FIGURE_OFF * scale;
        marks.texts.push(DimensionText { at: [centre[0] + up[0] * off, centre[1] + up[1] * off], angle, wording, em: style.size, face: face.clone() });
        let below = field("m_below");
        if !below.is_empty() {
            let down = off + lead;
            marks.texts.push(DimensionText { at: [centre[0] - up[0] * down, centre[1] - up[1] * down], angle, wording: below, em: style.size, face: face.clone() });
        }
    }
    Some(marks)
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
                        // to the next stop, eight letters on: the letters are not as wide as a stop
                        // is, so columns line up roughly
                        b"tab" if !skipping => {
                            let column = out.chars().rev().take_while(|c| *c != '\n').count();
                            out.extend(std::iter::repeat_n(' ', 8 - column % 8));
                        }
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
