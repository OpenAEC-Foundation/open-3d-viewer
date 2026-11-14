//! The tags of a Revit file, as they show in a view: the labels of their type with the values of
//! what they tag, the lines of their family and their leader.
//!
//! Read through the generic object view, named schema fields only:
//! - a tag is an `IndependentTag`: `m_symbolId` its type, `TagOrientationCell::m_headLocation`
//!   where its head stands and `m_rotationAngle` how it is turned; a `TagReferenceWithLeader`
//!   names what it tags (`m_referenceIdKey`) and whether a leader runs to it;
//! - its type, a `FamilySymbol`, holds the labels that show for it (`m_tagNoteData`, the family's
//!   labels whose visibility its type values switch on): where each stands from the head
//!   (`m_orig`, on paper), how it is aligned (`m_flags`), its text type (`m_attrId`) and the
//!   parameters it writes, each with a prefix and a suffix (`m_paramDataFormattingArr`);
//! - the lines of the family are in the type's saved graphics: the `GLine`s and `GArc`s that name
//!   a line style (those without one are the family's reference planes);
//! - a value is what the tagged element stores for the parameter, else what its type does
//!   (`ParamValueSet…` and, for a family type, `FamilyParams::m_params`); the height of its top
//!   is not stored: it is its level's (`DatumPlane::m_pSurface`) and its offset from that.
//!
//! A head and a leader's end are in the view's coordinates (feet); what hangs from the head is on
//! paper (metres), so a view's scale makes it that much larger on the drawing.

use super::rvt_drawings::{vec3, Types};
use rvt_model::{FieldValue, NestedObject, ObjectView};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const FT: f64 = 0.3048;
/// A line's height to its em, as the document sets its texts.
const LINE: f64 = 1.3;

/// A tag in a view.
pub(super) struct Tag {
    pub element: u64,
    pub view: u64,
    /// Where its head stands (feet).
    pub head: [f64; 3],
    pub texts: Vec<TagText>,
    /// The lines of its family, from the head (on paper, metres).
    pub lines: Vec<[[f64; 2]; 2]>,
    /// The points its leader runs through from the head, its end last (feet); empty without one.
    pub leader: Vec<[f64; 3]>,
    /// The mark at the leader's end.
    pub arrow: Option<Arrow>,
}

/// The mark at the end of a leader, as its style says (sizes on paper, metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Arrow {
    /// 0 a slash, 3 a dot, 8 an arrow.
    kind: i64,
    size: f64,
    /// The angle between an arrow's two sides.
    angle: f64,
}

impl Tag {
    /// Its leader on a drawing whose `scale` makes paper that much larger: the lines from the
    /// head to its end and the mark there, the points placed by `project`.
    pub fn leader_lines(&self, project: &dyn Fn([f64; 3]) -> [f64; 2], scale: f64) -> Vec<[[f64; 2]; 2]> {
        let points: Vec<[f64; 2]> = std::iter::once(self.head).chain(self.leader.iter().copied()).map(project).collect();
        let mut lines: Vec<[[f64; 2]; 2]> = points.windows(2).map(|w| [w[0], w[1]]).collect();
        let (Some(arrow), [.., before, end]) = (self.arrow, &points[..]) else { return lines };
        let run = (end[0] - before[0]).hypot(end[1] - before[1]);
        if run <= 0.0 {
            return lines;
        }
        // back along the leader, and across it
        let (u, size) = ([(before[0] - end[0]) / run, (before[1] - end[1]) / run], arrow.size * scale);
        let at = |along: f64, across: f64| [end[0] + (u[0] * along - u[1] * across) * size, end[1] + (u[1] * along + u[0] * across) * size];
        match arrow.kind {
            8 => {
                let half = (arrow.angle / 2.0).tan();
                let (a, b) = (at(1.0, half), at(1.0, -half));
                lines.extend([[*end, a], [a, b], [b, *end]]);
            }
            3 => lines.extend((0..12).map(|k| {
                let turn = |k: usize| (k as f64 * std::f64::consts::TAU / 12.0).sin_cos();
                let ((s0, c0), (s1, c1)) = (turn(k), turn(k + 1));
                [at(c0 / 2.0, s0 / 2.0), at(c1 / 2.0, s1 / 2.0)]
            })),
            _ => lines.push([at(0.5, 0.5), at(-0.5, -0.5)]),
        }
        lines
    }
}

/// A label of a tag with its wording.
pub(super) struct TagText {
    /// The foot of its box where its lines start, from the head (on paper, metres).
    pub at: [f64; 2],
    pub wording: String,
    /// The em of its letters on paper (metres).
    pub em: f64,
    /// Where its lines start: 0 left, 1 middle, 2 right.
    pub align: i64,
    /// The width its lines are set in, on paper (metres).
    pub width: f64,
    /// How its lines are turned (radians, counter-clockwise).
    pub angle: f64,
    /// The typeface of its letters, as its type says.
    pub face: crate::typeface::Face,
}

impl TagText {
    /// The height of its lines on paper (metres).
    pub fn height(&self) -> f64 {
        self.em * LINE * self.wording.lines().count().max(1) as f64
    }
}

/// A parameter a label writes.
struct Part {
    parameter: i64,
    prefix: String,
    suffix: String,
    /// The spaces before it, after the part before.
    spaces: usize,
    /// Whether it starts a new line.
    line_break: bool,
    /// The unit a length is written in, when the label says so.
    unit: Option<Unit>,
    plus: bool,
}

/// How a length is written: what a foot is in its unit, the decimals, the unit's symbol and
/// whether zeros at the end are left out.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Unit {
    per_foot: f64,
    decimals: usize,
    symbol: &'static str,
    trim: bool,
}

impl Unit {
    /// The unit format options name; none when they leave it to the project.
    fn of(format: &NestedObject) -> Option<Unit> {
        if format.get("m_bUseDefault") == Some(&FieldValue::Bool(true)) {
            return None;
        }
        let name = format.get("m_unitTypeId")?.as_object()?.get("m_typeId")?.as_str()?;
        let (per_foot, symbol) = [(":millimeters", 304.8, " mm"), (":centimeters", 30.48, " cm"), (":meters", 0.3048, " m")].iter().find(|(key, ..)| name.contains(key)).map(|(_, f, s)| (*f, *s))?;
        let decimals = format.get("m_accuracy").and_then(FieldValue::as_f64).filter(|a| *a > 0.0 && *a <= 1.0).map_or(0, |a| (-a.log10()).round().clamp(0.0, 6.0) as usize);
        let shown = format.get("m_symbolTypeId").and_then(FieldValue::as_object).and_then(|s| s.get("m_typeId")?.as_str()).is_some_and(|s| !s.is_empty());
        let trim = format.get("m_bSuppressTrailingZeros") == Some(&FieldValue::Bool(true));
        Some(Unit { per_foot, decimals, symbol: if shown { symbol } else { "" }, trim })
    }

    /// A length (feet) as this writes it, with the project's decimal mark.
    fn write(&self, feet: f64, comma: bool) -> String {
        let mut figure = format!("{:.*}", self.decimals, feet * self.per_foot);
        if self.trim && figure.contains('.') {
            figure = figure.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if figure == "-0" {
            figure = "0".into();
        }
        if comma {
            figure = figure.replace('.', ",");
        }
        figure + self.symbol
    }
}

/// How the project writes its lengths, and whether its decimal mark is a comma.
struct Units {
    length: Unit,
    comma: bool,
}

impl Units {
    /// Of the element with the units of the project (`Types::units`: finding it walks the file).
    fn of(doc: &rvt_read::Document, element: Option<u64>) -> Units {
        let o = element.and_then(|id| doc.object(id));
        let units = o.as_ref().and_then(|o| o.get("m_units")?.as_object());
        let length = units.and_then(|u| {
            let all = u.get("m_formatOptionsMap")?.as_list()?.iter().filter_map(FieldValue::as_object);
            let mut lengths = all.filter(|p| p.get("first").and_then(FieldValue::as_object).and_then(|f| f.get("m_typeId")?.as_str()).is_some_and(|t| t.contains("aec:length-")));
            Unit::of(lengths.next()?.get("second")?.as_object()?)
        });
        Units {
            // millimetres without decimals, when the project does not say
            length: length.unwrap_or(Unit { per_foot: 304.8, decimals: 0, symbol: "", trim: false }),
            comma: units.and_then(|u| u.get("m_decimalSymbol")?.as_i64()) == Some(1),
        }
    }
}

/// A label of a tag type.
struct Label {
    /// Where it stands from the head (on paper, metres).
    at: [f64; 2],
    /// Which of its box's heights `at` is: 0 the top, 1 the middle, 2 the foot.
    vertical: u8,
    align: i64,
    em: f64,
    /// The typeface of its letters.
    face: crate::typeface::Face,
    width: f64,
    /// How its lines are turned in its family (radians, counter-clockwise).
    angle: f64,
    /// How far the box drawn around it stands off its lines (on paper, metres), when its text
    /// type shows one.
    boxed: Option<f64>,
    parts: Vec<Part>,
}

impl Label {
    /// The label with what `value` gives for each of its parameters, standing on the foot of its
    /// lines; none when it writes nothing. A parameter without a value writes nothing at all:
    /// not what stands before and after it either, nor the line it would start (a line of a
    /// title block for a revision that is not filled in).
    fn write(&self, value: &mut dyn FnMut(&Part) -> Option<String>) -> Option<TagText> {
        let mut wording = String::new();
        for (k, part) in self.parts.iter().enumerate() {
            let value = value(part).unwrap_or_default();
            if value.trim().is_empty() {
                continue;
            }
            if part.line_break && !wording.is_empty() {
                wording.push('\n');
            } else if k > 0 && !wording.is_empty() {
                wording.extend(std::iter::repeat_n(' ', part.spaces));
            }
            wording.push_str(&part.prefix);
            wording.push_str(&value);
            wording.push_str(&part.suffix);
        }
        if wording.trim().is_empty() {
            return None;
        }
        let mut text = TagText { at: self.at, wording, em: self.em, align: self.align, width: self.width, angle: self.angle, face: self.face.clone() };
        // from where the label stands down its own up to the foot of its lines
        let drop = [text.height(), text.height() / 2.0, 0.0][self.vertical as usize];
        text.at = [text.at[0] + self.angle.sin() * drop, text.at[1] - self.angle.cos() * drop];
        Some(text)
    }
}

/// The box around a text, `off` away from its lines (on paper, metres): its four sides, turned
/// with it.
fn frame_of(text: &TagText, off: f64) -> [[[f64; 2]; 2]; 4] {
    let (sin, cos) = text.angle.sin_cos();
    let left = -[0.0, text.width / 2.0, text.width][text.align.clamp(0, 2) as usize] - off;
    let (right, foot, top) = (left + text.width + 2.0 * off, -off, text.height() + off);
    let at = |x: f64, y: f64| [text.at[0] + x * cos - y * sin, text.at[1] + x * sin + y * cos];
    let corners = [at(left, foot), at(right, foot), at(right, top), at(left, top)];
    std::array::from_fn(|k| [corners[k], corners[(k + 1) % 4]])
}

/// Which of a text box's heights its origin is, as its flags say: 0 the top, 1 the middle, 2 the
/// foot.
fn vertical(flags: i64) -> u8 {
    if flags & 0x800 != 0 {
        2
    } else if flags & 0x400 != 0 {
        1
    } else {
        0
    }
}

/// The title block of a sheet, from its origin on the sheet (on paper, metres): the lines of its
/// family, its fixed texts (`m_textNoteData`) and its labels with what the sheet (its `number` and
/// `name` among it) and the project information store for them.
pub(super) fn title_block(doc: &rvt_read::Document, types: &mut Types, kind: u64, sheet: u64, number: &str, name: &str) -> (Vec<[[f64; 2]; 2]>, Vec<TagText>) {
    let Some(o) = doc.object(kind) else { return Default::default() };
    let (mut sources, mut specs, units) = (HashMap::new(), HashMap::new(), Units::of(doc, types.units()));
    let project = types.project_info();
    let issued = doc.object(sheet).and_then(|s| s.get("m_issueDate").and_then(FieldValue::as_str).map(str::to_string));
    let texts = family_texts(doc, types, &o, &mut |part| match part.parameter {
        -1007401 => Some(number.to_string()),
        -1007400 => Some(name.to_string()),
        // the date the sheet was issued
        -1006322 => issued.clone(),
        _ => value(doc, &mut sources, &mut specs, &units, sheet, part).or_else(|| value(doc, &mut sources, &mut specs, &units, project?, part)),
    });
    (family_lines(doc, kind, types, 1.0), texts)
}

/// The texts of a family type, in its own coordinates (metres): its labels with what `value`
/// gives for each of their parameters (`m_tagNoteData`), and its fixed texts (`m_textNoteData`).
fn family_texts(doc: &rvt_read::Document, types: &mut Types, o: &ObjectView, value: &mut dyn FnMut(&Part) -> Option<String>) -> Vec<TagText> {
    let _ = doc;
    let mut texts = Vec::new();
    let of = |field: &str| o.get(field).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v));
    for label in of("m_tagNoteData").filter_map(|l| label(l, types)) {
        texts.extend(label.write(value));
    }
    for fixed in of("m_textNoteData") {
        let stream = fixed.get("m_oRtfData").and_then(|v| o.deref(v)).and_then(|r| r.get("m_arrRtfStream")?.as_list());
        let bytes: Vec<u8> = stream.unwrap_or(&[]).iter().filter_map(|b| b.as_i64().and_then(|b| u8::try_from(b).ok())).collect();
        let wording = super::rvt_drawings::rtf_text(&bytes);
        let Some(orig) = vec3(fixed.get("m_orig")).filter(|_| !wording.trim().is_empty()) else { continue };
        let flags = fixed.get("m_flags").and_then(FieldValue::as_i64).unwrap_or(0);
        let em = fixed.get("m_attrId").and_then(FieldValue::element).and_then(|k| types.text_em(k)).unwrap_or(crate::document::NOTE_HEIGHT);
        let mut face = fixed.get("m_attrId").and_then(FieldValue::element).map(|k| types.text_face(k)).unwrap_or_default();
        let [bold, italic, underline] = super::rvt_fonts::rtf_marks(&bytes);
        (face.bold, face.italic, face.underline) = (face.bold || bold, face.italic || italic, face.underline || underline);
        let width = fixed.get("m_lineWidth").and_then(FieldValue::as_f64).filter(|w| w.is_finite() && *w > 0.0).map_or(0.0, |w| w * FT);
        let base = vec3(fixed.get("m_baseVec")).unwrap_or([1.0, 0.0, 0.0]);
        let angle = Some(base[1].atan2(base[0])).filter(|a| a.abs() > 1e-6).unwrap_or(0.0);
        let mut text = TagText { at: [orig[0] * FT, orig[1] * FT], wording, em, align: (flags & 3).clamp(0, 2), width, angle, face };
        // down its own up to the foot of its lines
        let drop = [text.height(), text.height() / 2.0, 0.0][vertical(flags) as usize];
        text.at = [text.at[0] + angle.sin() * drop, text.at[1] - angle.cos() * drop];
        texts.push(text);
    }
    texts
}

/// The regions of a title block filled in one colour, from its origin on the sheet (on paper,
/// metres): each its outlines and its colour.
pub(super) fn title_block_fills(doc: &rvt_read::Document, types: &mut Types, kind: u64) -> Vec<(Vec<Vec<[f64; 2]>>, [u8; 3])> {
    let fills = graphics_lines(doc, kind, types, 1.0, Seen::PLAIN).fills;
    fills.into_iter().map(|f| (f.loops.into_iter().map(|l| l.into_iter().map(|p| [p[0] * FT, p[1] * FT]).collect()).collect(), f.color)).collect()
}

/// Per line of a title block ([`title_block`] gives them in this order) its colour, when its line
/// style has one that is not black.
pub(super) fn title_block_line_colors(doc: &rvt_read::Document, types: &mut Types, kind: u64) -> Vec<Option<[u8; 3]>> {
    graphics_lines(doc, kind, types, 1.0, Seen::PLAIN).colors
}

/// The lines of an annotation symbol type, on paper (metres).
pub(super) fn symbol_lines(doc: &rvt_read::Document, types: &mut Types, kind: u64) -> Vec<[[f64; 2]; 2]> {
    family_lines(doc, kind, types, 1.0)
}

/// The title of a view placed on a sheet, as the type of its viewport writes it
/// (`ViewportAttributes`: `m_showLabel`, and `m_familyTagId`, the type of its title family, whose
/// labels write the view's name and scale): empty when its type shows no title, none when the
/// viewport tells nothing of it. With it the typeface of its first label (its type's, so a
/// title set in italic stays italic).
pub(super) fn viewport_title(doc: &rvt_read::Document, types: &mut Types, viewport: u64, view: u64, name: &str, scale: &str, detail: Option<&str>) -> Option<(String, crate::typeface::Face)> {
    /// The view's name, its scale and the number of the view on its sheet.
    const NAME: i64 = -1005203;
    const SCALE: i64 = -1005204;
    const DETAIL_NUMBER: i64 = -1005201;
    let kind = doc.object(viewport)?.get("m_attrId").and_then(FieldValue::element).and_then(|k| u64::try_from(k).ok())?;
    let kind = doc.object(kind)?;
    if kind.get("m_showLabel").and_then(FieldValue::as_i64) == Some(0) {
        return Some(Default::default());
    }
    let Some(family) = kind.get("m_familyTagId").and_then(FieldValue::element).and_then(|f| u64::try_from(f).ok()) else { return Some(Default::default()) };
    let o = doc.object(family)?;
    let (mut sources, mut specs, units) = (HashMap::new(), HashMap::new(), Units::of(doc, types.units()));
    let labels = o.get("m_tagNoteData").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v)).filter_map(|l| label(l, types));
    let mut face: Option<crate::typeface::Face> = None;
    let written: Vec<String> = labels
        .filter_map(|label| {
            label.write(&mut |part| match part.parameter {
                NAME => Some(name.to_string()),
                SCALE => Some(scale.to_string()),
                DETAIL_NUMBER => detail.map(str::to_string),
                _ => value(doc, &mut sources, &mut specs, &units, viewport, part).or_else(|| value(doc, &mut sources, &mut specs, &units, view, part)),
            })
        })
        .map(|text| {
            face.get_or_insert_with(|| text.face.clone());
            text.wording
        })
        .collect();
    Some((written.join("  "), face.unwrap_or_default()))
}

/// What a tag type draws: its labels and the lines of its family (on paper, metres).
#[derive(Default)]
struct Shape {
    labels: Vec<Label>,
    lines: Vec<[[f64; 2]; 2]>,
    /// The mark at the end of its leaders.
    arrow: Option<Arrow>,
}

/// A stored parameter value.
#[derive(Debug, Clone, Default)]
struct Stored {
    text: Option<String>,
    number: Option<f64>,
    integer: Option<i64>,
}

/// What an element stores: its values by parameter, its type and its name.
#[derive(Default)]
struct Source {
    values: HashMap<i64, Stored>,
    kind: Option<u64>,
    name: Option<String>,
    /// The level it stands on.
    level: Option<u64>,
}

/// The tags of the file that show in a view, read once per type and tagged element.
pub(super) fn tags(doc: &rvt_read::Document, types: &mut Types) -> Vec<Tag> {
    let mut shapes: HashMap<i64, Arc<Shape>> = HashMap::new();
    let mut sources: HashMap<u64, Arc<Source>> = HashMap::new();
    let mut specs: HashMap<i64, String> = HashMap::new();
    let units = Units::of(doc, types.units());
    let mut out = Vec::new();
    for id in doc.object_ids_of_class("IndependentTag") {
        let Some(view) = doc.element(id).and_then(|e| e.view).and_then(|v| u64::try_from(v).ok()).filter(|v| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id) else { continue };
        let Some(head) = vec3(o.path(&["m_oTagOrientationCell", "m_headLocation"])) else { continue };
        let Some(kind) = o.get("m_symbolId").and_then(FieldValue::element) else { continue };
        let shape = shapes.entry(kind).or_insert_with(|| Arc::new(shape(doc, kind, types).unwrap_or_default())).clone();
        // what it tags, and whether a leader runs to it
        let reference = o.objects.iter().find(|x| x.class == "TagReferenceWithLeader");
        let tagged = reference.and_then(|r| r.get("m_referenceIdKey")?.as_object()?.get("m_hostOrLinkElemId")?.as_object()?.get("m_linkInstOrHostId")?.element()).and_then(|e| u64::try_from(e).ok());
        // a tag set upright stands a quarter turn, another as it is turned
        let turned = o.path(&["m_oTagOrientationCell", "m_rotationAngle"]).and_then(FieldValue::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0);
        let upright = o.path(&["m_oTagOrientationCell", "m_orientation"]).and_then(FieldValue::as_i64) == Some(1);
        let angle = if upright { std::f64::consts::FRAC_PI_2 } else { turned };
        let (sin, cos) = angle.sin_cos();
        let turn = |p: [f64; 2]| [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos];
        let mut texts = Vec::new();
        let mut boxes: Vec<[[f64; 2]; 2]> = Vec::new();
        for label in &shape.labels {
            let Some(mut text) = label.write(&mut |part| tagged.and_then(|e| value(doc, &mut sources, &mut specs, &units, e, part))) else { continue };
            // the box its text type draws around it
            if let Some(off) = label.boxed {
                boxes.extend(frame_of(&text, off).map(|l| [turn(l[0]), turn(l[1])]));
            }
            text.at = turn(text.at);
            text.angle += angle;
            texts.push(text);
        }
        let lines = shape.lines.iter().map(|l| [turn(l[0]), turn(l[1])]).chain(boxes).collect();
        // a leader: through its elbow to its end, both stored from the head; an end that holds on
        // to what is tagged is not stored: the head stands off that by `m_relHeadLocation`
        let mut leader = Vec::new();
        if let Some(r) = reference.filter(|r| r.get("m_isLeaderEnabled") == Some(&FieldValue::Bool(true))) {
            let off = |name: &str| vec3(r.get(name).and_then(FieldValue::as_object).and_then(|x| x.get("m_xyz"))).filter(|v| v.iter().any(|x| x.abs() > 1e-9));
            let from_head = |v: [f64; 3]| [head[0] + v[0], head[1] + v[1], head[2] + v[2]];
            let end = off("m_endOffset").map(from_head).or_else(|| {
                let back = vec3(o.path(&["m_oTagOrientationCell", "m_relHeadLocation", "m_xyz"])).filter(|v| v.iter().any(|x| x.abs() > 1e-9))?;
                Some([head[0] - back[0], head[1] - back[1], head[2] - back[2]])
            });
            if let Some(end) = end {
                leader.extend(off("m_elbowOffset").map(from_head));
                leader.push(end);
            }
        }
        out.push(Tag { element: id, view, head, texts, lines, leader, arrow: shape.arrow });
    }
    out
}

/// A detail component in a view: the lines of its family where it is placed (feet, in the
/// view's coordinates).
pub(super) struct Detail {
    pub element: u64,
    pub view: u64,
    pub lines: Vec<[[f64; 3]; 2]>,
    /// Per line its colour, when its line style or its fill pattern has one that is not black.
    pub colors: Vec<Option<[u8; 3]>>,
    /// Its regions with a solid fill.
    pub fills: Vec<Fill>,
    /// Its masking regions, which cover what lies under them: the outlines of each.
    pub masks: Vec<Vec<Vec<[f64; 3]>>>,
    /// Its texts and labels: each where the foot of its lines starts (feet, in the view's
    /// coordinates), turned as it lies in the view; sizes on paper.
    pub texts: Vec<([f64; 3], TagText)>,
}

/// A region filled in one colour: its outlines (the first its outer one, the others its holes;
/// feet, in the view's coordinates) and its colour (red, green, blue).
#[derive(Debug, Clone)]
pub(super) struct Fill {
    pub loops: Vec<Vec<[f64; 3]>>,
    pub color: [u8; 3],
}

/// How graphics are looked at: at which level of detail (1 coarse, 2 medium, 3 fine), and whether
/// they are those of an element of the model, of which only the symbols nested in its family
/// are taken (its own solids are cut and drawn with the model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Seen {
    detail: i64,
    model: bool,
    /// The direction it is looked along, in the element's surroundings (millionths of a unit
    /// vector): z for a plan and for a family drawn on paper, a section's own for a section.
    along: [i32; 3],
}

impl Seen {
    /// Seen from above, as a plan and a family drawn on paper are.
    const DOWN: [i32; 3] = [0, 0, 1_000_000];
    /// A family drawn on paper or in a view without a level of detail of its own.
    const PLAIN: Seen = Seen { detail: 2, model: false, along: Seen::DOWN };

    fn towards(dir: [f64; 3]) -> [i32; 3] {
        dir.map(|v| (v * 1e6).round() as i32)
    }
}

/// What the saved graphics of an element draw: lines, each with its colour when it has one that
/// is not black, and regions filled in one colour.
#[derive(Debug, Default)]
struct Graphics {
    lines: Vec<[[f64; 3]; 2]>,
    colors: Vec<Option<[u8; 3]>>,
    fills: Vec<Fill>,
    masks: Vec<Vec<Vec<[f64; 3]>>>,
    /// The outlines of its regions that a fill pattern fills: what an opaque filled region
    /// covers.
    patterned: Vec<Vec<Vec<[f64; 3]>>>,
    /// Of an element of the model: that it has solids and none of them shows as it is looked at
    /// (its family switches them off for that direction or level of detail).
    solids_off: bool,
}

/// Where graphics are placed: the directions of their x, y and z and their origin.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    axes: [[f64; 3]; 3],
    origin: [f64; 3],
}

impl Placement {
    const NONE: Placement = Placement { axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], origin: [0.0; 3] };

    fn turn(&self, v: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|k| v[0] * self.axes[0][k] + v[1] * self.axes[1][k] + v[2] * self.axes[2][k])
    }

    fn put(&self, p: [f64; 3]) -> [f64; 3] {
        let v = self.turn(p);
        std::array::from_fn(|k| self.origin[k] + v[k])
    }

    /// `inner`, placed in graphics that are placed by this.
    fn of(&self, inner: &Placement) -> Placement {
        Placement { axes: inner.axes.map(|a| self.turn(a)), origin: self.put(inner.origin) }
    }
}

impl Graphics {
    /// Places what was added from `from` on (lines, fills, masks).
    fn place(&mut self, from: (usize, usize, usize), by: &Placement) {
        if *by == Placement::NONE {
            return;
        }
        for line in &mut self.lines[from.0..] {
            *line = line.map(|p| by.put(p));
        }
        for outline in self.fills[from.1..].iter_mut().flat_map(|f| &mut f.loops).chain(self.masks[from.2..].iter_mut().flatten()) {
            for p in outline.iter_mut() {
                *p = by.put(*p);
            }
        }
    }

    fn line(&mut self, line: [[f64; 3]; 2], color: Option<[u8; 3]>) {
        self.lines.push(line);
        self.colors.push(color);
    }
}

/// A run of points as lines: in its line pattern when it has one, else from point to point.
fn drawn(points: &[[f64; 3]], pattern: Option<&[f64]>, scale: f64) -> Vec<[[f64; 3]; 2]> {
    match pattern {
        Some(pattern) => super::rvt_drawings::dashed(points, pattern, scale),
        None => points.windows(2).filter(|w| w[0] != w[1]).map(|w| [w[0], w[1]]).collect(),
    }
}

/// A colour as the file stores it (blue, green, red from the high byte down); none for black and
/// for a value that is no colour.
fn color_of(stored: i64) -> Option<[u8; 3]> {
    (stored > 0 && stored < 0x100_0000).then(|| [(stored & 0xFF) as u8, (stored >> 8 & 0xFF) as u8, (stored >> 16 & 0xFF) as u8])
}

/// The detail components of the file that show in a view: a `FamilyInstance` of the category
/// detail items that names its view. Its type's saved graphics hold the lines of its family, at
/// full size; `InstanceInfo::m_Trf` places them. And its filled regions (`FilledRegion`), each
/// with its outline and the lines of its fill pattern.
pub(super) fn details(doc: &rvt_read::Document, types: &mut Types) -> Vec<Detail> {
    const DETAIL_ITEMS: i32 = -2002000;
    // a symbol: drawn on paper, and placed as many times as large as the scale of its view
    const GENERIC_ANNOTATIONS: i32 = -2000150;
    // per type and scale of the view: a drafting pattern in it is sized on paper
    let mut families: HashMap<(u64, u64, Seen), Arc<Graphics>> = HashMap::new();
    let mut out = Vec::new();
    let units = Units::of(doc, types.units());
    // the values their labels show are looked up through these, shared by all of them: the
    // type of an instance and what a parameter measures were read again for every instance
    let (mut sources, mut specs) = (HashMap::new(), HashMap::new());
    let mut symbols: HashMap<u64, Option<rvt_model::ObjectView>> = HashMap::new();
    for id in doc.object_ids_of_class("FamilyInstance") {
        let Some(placed_in) = doc.element(id).filter(|e| e.category == DETAIL_ITEMS || e.category == GENERIC_ANNOTATIONS) else { continue };
        let symbol = placed_in.category == GENERIC_ANNOTATIONS;
        let Some(view) = placed_in.view.and_then(|v| u64::try_from(v).ok()).filter(|v| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id) else { continue };
        // an instance with values of its own (its length) has a type of its own, drawn with them
        let named = |v: Option<&FieldValue>| v.and_then(FieldValue::element).and_then(|k| u64::try_from(k).ok());
        let Some(kind) = named(o.path(&["m_pInstanceInfo", "m_symbolId"])).or_else(|| named(o.get("m_masterSymbolId"))) else { continue };
        let scale = if symbol { 1.0 } else { types.view_scale(view) };
        let seen_as = Seen { detail: types.view_detail(view), model: false, along: Seen::DOWN };
        let family = match families.get(&(kind, scale.to_bits(), seen_as)) {
            Some(known) => known.clone(),
            None => {
                let drawn = Arc::new(graphics_lines(doc, kind, types, scale, seen_as));
                families.insert((kind, scale.to_bits(), seen_as), drawn.clone());
                drawn
            }
        };
        if family.lines.is_empty() && family.fills.is_empty() && family.masks.is_empty() {
            continue;
        }
        // a detail component covers what lies under it: its masking regions, and its regions a
        // pattern fills (a filled region in a family is opaque, as a new one is)
        let covering = family.masks.iter().chain(family.patterned.iter().filter(|_| !symbol));
        // where it is placed: its origin and the directions of its family's x and y
        let placed = o.path(&["m_pInstanceInfo", "m_Trf"]).and_then(FieldValue::as_object);
        let Some(origin) = vec3(placed.and_then(|t| t.get("m_or"))).or_else(|| vec3(o.get("m_instOrigin"))) else { continue };
        let axes = placed.and_then(|t| t.get("m_3x3")?.as_list());
        // the matrix turns the family's coordinates into the view's: the direction of the
        // family's x is its first column
        let axis = |k: usize, or: [f64; 3]| axes.and_then(|a| Some([vec3(a.first())?[k], vec3(a.get(1))?[k], vec3(a.get(2))?[k]])).unwrap_or(or);
        let (x, y) = (axis(0, [1.0, 0.0, 0.0]), axis(1, [0.0, 1.0, 0.0]));
        // a mirrored instance says so apart from where it is placed: its family's y the other way
        // (a type of its own is saved mirrored already: only the family's shared type is turned)
        let mirrored = o.get("m_pGeomTable").and_then(|v| o.deref(v)).is_some_and(|t| t.get("m_refPntMirrored") == Some(&FieldValue::Bool(true)));
        let shared = named(o.get("m_masterSymbolId")) == Some(kind);
        let side = if mirrored && shared { -1.0 } else { 1.0 };
        let at = |p: [f64; 3]| -> [f64; 3] { std::array::from_fn(|k| origin[k] + p[0] * x[k] + side * p[1] * y[k]) };
        let fills = family.fills.iter().map(|f| Fill { loops: f.loops.iter().map(|l| l.iter().map(|&p| at(p)).collect()).collect(), color: f.color }).collect();
        let masks = covering.map(|m| m.iter().map(|l| l.iter().map(|&p| at(p)).collect()).collect()).collect();
        // its texts and labels, with the values of the instance and of its type
        let texts = symbols.entry(kind).or_insert_with(|| doc.object(kind)).as_ref().map_or(Vec::new(), |symbol| family_texts(doc, types, symbol, &mut |part| value(doc, &mut sources, &mut specs, &units, id, part)));
        let texts = texts
            .into_iter()
            .map(|mut t| {
                let foot = at([t.at[0] / FT, t.at[1] / FT, 0.0]);
                let (s, c) = t.angle.sin_cos();
                let run = std::array::from_fn::<f64, 3, _>(|k| c * x[k] + side * s * y[k]);
                t.angle = run[1].atan2(run[0]);
                (foot, t)
            })
            .collect();
        out.push(Detail { element: id, view, lines: family.lines.iter().map(|l| [at(l[0]), at(l[1])]).collect(), colors: family.colors.clone(), fills, masks, texts });
    }
    // a filled region: its outline and the lines of its fill pattern, in its own saved graphics
    for id in doc.object_ids_of_class("FilledRegion") {
        let Some(view) = doc.element(id).and_then(|e| e.view).and_then(|v| u64::try_from(v).ok()).filter(|v| !types.hidden(*v, id)) else { continue };
        let scale = types.view_scale(view);
        let seen_as = Seen { detail: types.view_detail(view), model: false, along: Seen::DOWN };
        let mut drawn = graphics_lines(doc, id, types, scale, seen_as);
        // its type opaque (`m_background` 0, as a new type is): it covers what lies under its
        // pattern too
        let opaque = doc.object(id).and_then(|o| o.get("m_attrId")?.element()).and_then(|t| doc.object(u64::try_from(t).ok()?)).and_then(|t| t.get("m_background")?.as_i64()) == Some(0);
        if opaque {
            let patterned = std::mem::take(&mut drawn.patterned);
            drawn.masks.extend(patterned);
        }
        if !drawn.lines.is_empty() || !drawn.fills.is_empty() || !drawn.masks.is_empty() {
            out.push(Detail { element: id, view, lines: drawn.lines, colors: drawn.colors, fills: drawn.fills, masks: drawn.masks, texts: Vec::new() });
        }
    }
    // an element of the model whose family has symbols nested in it (a pile with its mark): those,
    // in each section it lies in and each plan that cuts it, as that looks at it. Its saved graphics are its own,
    // where it stands. Where its family shows none of its solids as the view looks at it, the
    // view leaves the element itself out ([`Types::switch_off`]).
    const WITH_SYMBOLS: [i32; 1] = [-2001300];
    /// Where a view shows the model: between two heights, or in a box of its own.
    enum Range {
        Heights(f64, f64),
        Box { origin: [f64; 3], basis: [[f64; 3]; 3], min: [f64; 3], max: [f64; 3] },
    }
    // with the height of a plan's cut: a family's symbols show where the plan cuts the element
    let views: Vec<(u64, Range, [f64; 3], Option<f64>)> = rvt_export::views::views(doc)
        .into_iter()
        .filter(|v| !v.is_template)
        .filter_map(|v| match v.class.as_str() {
            "DBViewPlan" => {
                let range = v.view_range?;
                let (top, low) = (range.top.or(range.cut)?, range.depth.or(range.bottom)?);
                Some((v.id, Range::Heights(low.min(top), low.max(top)), [0.0, 0.0, 1.0], range.cut))
            }
            "DBViewSection" => {
                let b = v.crop_box?;
                Some((v.id, Range::Box { origin: b.origin, basis: b.basis, min: b.min, max: b.max }, b.basis[2], None))
            }
            _ => None,
        })
        .collect();
    let within = |range: &Range, bbox: &[f64; 6]| match range {
        Range::Heights(low, top) => bbox[2] <= *top + 1e-6 && bbox[5] >= *low - 1e-6,
        Range::Box { origin, basis, min, max } => (0..3).all(|k| {
            // the element's box along this direction of the view's
            let along = (0..8).map(|c| (0..3).map(|n| (bbox[n + if c >> n & 1 == 1 { 3 } else { 0 }] - origin[n]) * basis[k][n]).sum::<f64>());
            let (lo, hi) = along.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            lo <= max[k] + 1e-6 && hi >= min[k] - 1e-6
        }),
    };
    let mut drawn: HashMap<(u64, u64, Seen), Arc<Graphics>> = HashMap::new();
    for id in doc.object_ids_of_class("FamilyInstance") {
        let Some(e) = doc.element(id).filter(|e| WITH_SYMBOLS.contains(&e.category) && e.view.is_none_or(|v| v < 0)) else { continue };
        let Some(bbox) = e.bbox else { continue };
        for (view, _, along, cut) in views.iter().filter(|(_, range, ..)| within(range, &bbox)) {
            let view = *view;
            if types.hidden(view, id) {
                continue;
            }
            // a plan that does not cut the element looks at it from outside: it shows the element
            // itself (dashed where something covers it), not the symbols of its cut
            if cut.is_some_and(|cut| cut < bbox[2] - 1e-6 || cut > bbox[5] + 1e-6) {
                continue;
            }
            let (scale, seen_as) = (types.view_scale(view), Seen { detail: types.view_detail(view), model: true, along: Seen::towards(*along) });
            let symbols = match drawn.get(&(id, scale.to_bits(), seen_as)) {
                Some(known) => known.clone(),
                None => {
                    let symbols = Arc::new(graphics_lines(doc, id, types, scale, seen_as));
                    drawn.insert((id, scale.to_bits(), seen_as), symbols.clone());
                    symbols
                }
            };
            if symbols.solids_off {
                types.switch_off(view, id);
            }
            if !symbols.lines.is_empty() || !symbols.fills.is_empty() || !symbols.masks.is_empty() {
                out.push(Detail { element: id, view, lines: symbols.lines.clone(), colors: symbols.colors.clone(), fills: symbols.fills.clone(), masks: symbols.masks.clone(), texts: Vec::new() });
            }
        }
    }
    out
}

/// What a tag type draws.
fn shape(doc: &rvt_read::Document, kind: i64, types: &mut Types) -> Option<Shape> {
    let id = u64::try_from(kind).ok()?;
    let o = doc.object(id)?;
    let labels = o.get("m_tagNoteData").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v)).filter_map(|l| label(l, types)).collect();
    // the mark at the end of its leaders: a leader style
    let style = o.path(&["m_pParamValueSetElementId", "m_paramSet"]).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object).find(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(-1006315));
    let arrow = style.and_then(|p| doc.object(u64::try_from(p.get("m_value")?.element()?).ok()?)).map(|mark| {
        let number = |parameter: i64| {
            let all = mark.path(&["m_pParamValueSetDouble", "m_paramSet"]).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
            all.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).find_map(|p| p.get("m_value")?.as_f64()).filter(|v| v.is_finite() && *v > 0.0)
        };
        Arrow { kind: mark.get("m_tickType").and_then(FieldValue::as_i64).unwrap_or(8), size: number(-1006414).map_or(0.0024, |s| s * FT), angle: number(-1006426).unwrap_or(0.35) }
    });
    Some(Shape { labels, lines: family_lines(doc, id, types, 1.0), arrow })
}

/// A label of a tag type; none for one that writes nothing.
fn label(l: &NestedObject, types: &mut Types) -> Option<Label> {
    let orig = vec3(l.get("m_orig"))?;
    let flags = l.get("m_flags").and_then(FieldValue::as_i64).unwrap_or(0);
    let em = l.get("m_attrId").and_then(FieldValue::element).and_then(|k| types.text_em(k)).unwrap_or(crate::document::NOTE_HEIGHT);
    let face = l.get("m_attrId").and_then(FieldValue::element).map(|k| types.text_face(k)).unwrap_or_default();
    let width = l.get("m_lineWidth").and_then(FieldValue::as_f64).filter(|w| w.is_finite() && *w > 0.0).map_or(0.0, |w| w * FT);
    let parts: Vec<Part> = l
        .get("m_paramDataFormattingArr")
        .and_then(FieldValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(FieldValue::as_object)
        .filter_map(|p| {
            let text = |name: &str| p.get(name).and_then(FieldValue::as_str).unwrap_or_default().to_string();
            let format = p.get("m_formatOptions").and_then(FieldValue::as_object);
            let unit = format.and_then(Unit::of);
            Some(Part {
                parameter: p.get("m_paramId")?.element()?,
                prefix: text("m_prefix"),
                suffix: text("m_suffix"),
                spaces: p.get("m_leadingSpaces").and_then(FieldValue::as_i64).map_or(1, |s| s.clamp(0, 40) as usize),
                line_break: p.get("m_bLeadingLineBreak") == Some(&FieldValue::Bool(true)),
                unit,
                plus: format.is_some_and(|f| f.get("m_bUsePlusPrefix") == Some(&FieldValue::Bool(true))),
            })
        })
        .collect();
    if parts.is_empty() {
        return None;
    }
    let base = vec3(l.get("m_baseVec")).unwrap_or([1.0, 0.0, 0.0]);
    let angle = Some(base[1].atan2(base[0])).filter(|a| a.abs() > 1e-6).unwrap_or(0.0);
    let boxed = l.get("m_attrId").and_then(FieldValue::element).and_then(|k| types.text_box(k));
    Some(Label { at: [orig[0] * FT, orig[1] * FT], vertical: vertical(flags), align: (flags & 3).clamp(0, 2), em, face, width, angle, boxed, parts })
}

/// The lines of a family type, from its saved graphics (on paper, metres): see [`graphics_lines`].
/// `scale` is that of the view it is drawn in (1 on paper).
fn family_lines(doc: &rvt_read::Document, kind: u64, types: &mut Types, scale: f64) -> Vec<[[f64; 2]; 2]> {
    graphics_lines(doc, kind, types, scale, Seen::PLAIN).lines.into_iter().map(|l| l.map(|p| [p[0] * FT, p[1] * FT])).collect()
}

/// The most lines one fill pattern is drawn with in one region: a denser one is left out.
const MOST_HATCH_LINES: usize = 4000;

/// The lines of the saved graphics of an element (a family type, a filled region), in its own
/// coordinates (feet): its straight lines as they are, its arcs and circles in pieces of at most
/// 7.5 degrees, the edges of its regions and the lines of their fill patterns
/// (`Face::m_pGFilling` and `m_oBackgroundFilling`: a `FillPatternData` of `FillGrid`s, placed by
/// `m_placer`; a drafting pattern is sized on paper, so `view_scale` times as large in a view),
/// cut to the region; a solid fill as the region in its colour. Left out: what has no line style (a family's reference
/// planes), what its style does not print (invisible lines, reference lines), what a visibility
/// parameter of the type switches off and what shows from another side than the front.
fn graphics_lines(doc: &rvt_read::Document, id: u64, types: &mut Types, view_scale: f64, seen_as: Seen) -> Graphics {
    /// The flags of a piece of graphics that shows at no level of detail (coarse, medium, fine).
    const SWITCHED_OFF: i64 = 0xE0;
    use rvt_core::graph::{Object, Value};
    let Some(saved) = doc.saved_graphics() else { return Graphics::default() };
    let Some(graph) = saved.bodies.get(&id).and_then(|body| rvt_core::graph::Graph::decode(&saved.schema, body).ok()) else { return Graphics::default() };
    let class = |o: &Object| saved.schema.class(o.class).map_or("", |c| c.name.as_str());
    let style = |o: &Object| o.get("m_GInfo").and_then(|g| g.as_obj()).and_then(|g| g.get("m_categoryId")?.element_id()).filter(|c| *c != -1);
    let floats = |o: &Object, name: &str| o.get(name).and_then(|v| v.floats()).filter(|v| v.len() >= 2 && v.iter().all(|x| x.is_finite()));
    let vector = |o: &Object, name: &str| floats(o, name).map(|v| [v[0], v[1], v.get(2).copied().unwrap_or(0.0)]);
    let target = |v: Option<&Value>| v.and_then(|p| p.as_ptr()).and_then(|p| Some((p, graph.objects.get(p)?)));
    // a family is drawn in its own plane, seen from the front: what a filter asks of the
    // direction it is seen from is asked of z
    // the direction it is looked along, and the level of detail as one of three marks
    let along: [f64; 3] = seen_as.along.map(|v| v as f64 / 1e6);
    let level = 1i64 << (seen_as.detail.clamp(1, 3) - 1);
    let holds = |condition: &Object, placed: &Placement| {
        // of the view's level of detail (`m_param` 3): `m_value` marks levels (1 coarse, 2
        // medium, 4 fine), and it shows at one of them (`m_comp` 0) or at none of them (3)
        if class(condition) == "GConditionInt" {
            let field = |name: &str| condition.get(name).and_then(|v| v.as_i64());
            return match (field("m_param"), field("m_comp"), field("m_value")) {
                (Some(3), Some(0), Some(levels)) => levels & level != 0,
                (Some(3), Some(3), Some(levels)) => levels & level == 0,
                _ => false,
            };
        }
        let Some(dir) = floats(condition, "m_dir").filter(|d| d.len() >= 3 && class(condition) == "GConditionDir") else { return false };
        // how far the view looks along `m_dir`, which is in the family's own directions
        let to = placed.turn([dir[0], dir[1], dir[2]]);
        let dot = to[0] * along[0] + to[1] * along[1] + to[2] * along[2];
        let met = match condition.get("m_comp").and_then(|c| c.as_i64()) {
            Some(10) => dot > 0.999,
            Some(0) => dot.abs() < 1e-6,
            // seen from the side `m_dir` points to or from the other (6, 7), or along it either
            // way (8): from above or from below is a plan either way, from the front or the back
            // an elevation
            Some(6..=8) => dot.abs() > 0.999,
            _ => false,
        };
        met != matches!(condition.get("m_negateDirCondition"), Some(Value::Bool(true)))
    };
    // a solid: a region with a face that has no fill, with a face that is not flat, or with
    // four faces or more (a solid of a material has a fill on every face too; a filled region
    // is one flat face, or a few)
    let solid = |o: &Object| {
        let faces: Vec<&Object> = o.get("m_pFaces").and_then(|f| f.as_list()).unwrap_or(&[]).iter().filter_map(|p| graph.objects.get(p.as_ptr()?)).collect();
        let flat = |f: &&Object| target(f.get("m_pSurf")).is_some_and(|(_, s)| class(s) == "Plane");
        !faces.is_empty() && (faces.len() >= 4 || !faces.iter().all(|f| target(f.get("m_pGFilling")).is_some() && flat(f)))
    };
    // how many it has of its own (not of a family nested in it), whatever shows
    let mut solids = 0usize;
    if seen_as.model {
        let (mut todo, mut met) = (vec![(0usize, false)], HashSet::new());
        while let Some((i, nested)) = todo.pop() {
            let Some(o) = graph.objects.get(i).filter(|_| met.insert(i)) else { continue };
            let name = class(o);
            let nested = nested || name == "GGroup" && style(o).is_some();
            solids += (!nested && name == "Geometry" && solid(o)) as usize;
            todo.extend(target(o.get("m_oEmbeddedSymbolGRep")).map(|(at, _)| (at, nested)));
            todo.extend(o.get("m_subNodes").and_then(|s| s.as_list()).unwrap_or(&[]).iter().filter_map(|p| Some((p.as_ptr()?, nested))));
        }
    }
    let mut shown_solids = 0usize;
    let mut out = Graphics::default();
    let mut seen = HashSet::new();
    // each with whether it lies in a family nested in this one
    let mut open = vec![(0usize, false, Placement::NONE)];
    // what the piece before this one added, and where that is placed
    let mut added = ((0, 0, 0), Placement::NONE);
    while let Some((i, nested, placed)) = open.pop() {
        out.place(added.0, &added.1);
        added = ((out.lines.len(), out.fills.len(), out.masks.len()), placed);
        let Some(o) = graph.objects.get(i).filter(|_| seen.insert(i)) else { continue };
        let name = class(o);
        // a family placed in these graphics: its own graphics, and where they lie
        if name == "GInstance" {
            let trf = target(o.get("m_instanceInfo")).and_then(|(_, info)| info.get("m_Trf")?.as_obj());
            let rows = trf.and_then(|t| t.get("m_3x3")?.as_list()).map(|r| r.iter().filter_map(|v| v.floats()).filter(|v| v.len() >= 3).map(|v| [v[0], v[1], v[2]]).collect::<Vec<_>>());
            let inner = match (rows.as_deref(), trf.and_then(|t| vector(t, "m_or"))) {
                // the directions of its x, y and z are the columns of the matrix
                (Some([a, b, c]), Some(origin)) => Placement { axes: std::array::from_fn(|k| [a[k], b[k], c[k]]), origin },
                _ => Placement::NONE,
            };
            if let Some((at, _)) = target(o.get("m_oEmbeddedSymbolGRep")) {
                open.push((at, nested, placed.of(&inner)));
            }
        }
        // a group with a category is a nested family: its regions and lines lie where it is
        // placed; its reference lines are in its own coordinates and never show
        let nested = nested || name == "GGroup" && style(o).is_some();
        // what a parameter of the type switches off stays in its graphics, marked as showing at
        // none of the three levels of detail
        if matches!(name, "GGroup" | "Geometry" | "GLine" | "GArc") && o.get("m_GInfo").and_then(|g| g.as_obj()).and_then(|g| g.get("m_flags")?.as_i64()).is_some_and(|f| f & SWITCHED_OFF == SWITCHED_OFF) {
            continue;
        }
        if name == "GFilter" {
            let conditions = o.get("m_oConditions").and_then(|c| c.as_list()).unwrap_or(&[]);
            if !conditions.iter().all(|c| c.as_ptr().and_then(|p| graph.objects.get(p)).is_some_and(|c| holds(c, &placed))) {
                continue;
            }
        }
        open.extend(o.get("m_subNodes").and_then(|s| s.as_list()).unwrap_or(&[]).iter().filter_map(|p| Some((p.as_ptr()?, nested, placed))));
        // of an element of the model only what its symbols draw: what lies in a family nested in
        // it, and a region of its own that is filled (its solids have faces without a fill)
        if seen_as.model && !nested {
            let own_solid = name == "Geometry" && solid(o);
            shown_solids += own_solid as usize;
            let filled = name == "Geometry" && !own_solid && o.get("m_pFaces").and_then(|f| f.as_list()).is_some_and(|f| !f.is_empty());
            if !filled {
                continue;
            }
        }
        // regions: their edges, each a run of points on the plane of its face, and their fills
        if name == "Geometry" {
            // per face: its plane and the pieces of its outline on it
            let mut faces: HashMap<usize, Vec<[[f64; 2]; 2]>> = HashMap::new();
            for edge in o.get("m_pEdges").and_then(|e| e.as_list()).unwrap_or(&[]).iter().filter_map(|p| graph.objects.get(p.as_ptr()?)) {
                let Some((at, face)) = edge.get("m_pFace").and_then(|f| f.as_list()).and_then(|f| f.iter().find_map(|p| target(Some(p)))) else { continue };
                let Some((_, plane)) = target(face.get("m_pSurf")).filter(|(_, s)| class(s) == "Plane") else { continue };
                let (Some(c), Some(x), Some(y)) = (vector(plane, "m_origin"), vector(plane, "m_xVec"), vector(plane, "m_yVec")) else { continue };
                let run = |name: &str| edge.get(name).and_then(|l| l.as_list()).unwrap_or(&[]).iter().filter_map(|p| p.as_obj()?.get("uv")?.as_list()?.first()?.floats()).filter(|uv| uv.len() >= 2 && uv.iter().all(|v| v.is_finite())).map(|uv| [uv[0], uv[1]]).collect::<Vec<_>>();
                let (ends, between) = (run("m_firstAndLastEdgePnts"), run("m_interiorEdgePnts"));
                let [first, last] = ends.as_slice() else { continue };
                let points: Vec<[f64; 2]> = std::iter::once(*first).chain(between).chain(std::iter::once(*last)).collect();
                let pieces = points.windows(2).filter(|w| w[0] != w[1]).map(|w| [w[0], w[1]]);
                faces.entry(at).or_default().extend(pieces.clone());
                if let Some(s) = style(edge).filter(|s| types.line_shows(*s)) {
                    let on = |uv: [f64; 2]| std::array::from_fn(|k| c[k] + uv[0] * x[k] + uv[1] * y[k]);
                    let color = types.line_color(s);
                    let run: Vec<[f64; 3]> = points.iter().map(|&uv| on(uv)).collect();
                    for line in drawn(&run, types.line_dashes(s).as_deref().map(|d| d.as_slice()), view_scale) {
                        out.line(line, color);
                    }
                }
            }
            for (at, outline) in &faces {
                        let Some(face) = graph.objects.get(*at) else { continue };
                let Some((_, plane)) = target(face.get("m_pSurf")) else { continue };
                let (Some(c), Some(x), Some(y)) = (vector(plane, "m_origin"), vector(plane, "m_xVec"), vector(plane, "m_yVec")) else { continue };
                let on = |uv: [f64; 2]| -> [f64; 3] { std::array::from_fn(|k| c[k] + uv[0] * x[k] + uv[1] * y[k]) };
                // the outlines of the face, the outer one first
                let loops_of = || -> Vec<Vec<[f64; 3]>> {
                    let mut loops: Vec<Vec<[f64; 3]>> = Vec::new();
                    let mut next = target(face.get("m_pFirstLoop"));
                    while let Some((_, edges)) = next.filter(|_| loops.len() < 256) {
                        let mut points: Vec<[f64; 2]> = Vec::new();
                        let first_of = |v: Option<&Value>| v.and_then(|n| n.as_ptr().or_else(|| n.as_list()?.iter().find_map(|p| p.as_ptr()))).and_then(|p| graph.objects.get(p));
                        let mut edge = first_of(edges.get("m_next"));
                        let mut steps = 0;
                        while let Some(e) = edge.filter(|e| class(e) == "Edge" && steps < 4096) {
                            steps += 1;
                            let run = |name: &str| e.get(name).and_then(|l| l.as_list()).unwrap_or(&[]).iter().filter_map(|p| p.as_obj()?.get("uv")?.as_list()?.first()?.floats()).filter(|uv| uv.len() >= 2 && uv.iter().all(|v| v.is_finite())).map(|uv| [uv[0], uv[1]]).collect::<Vec<_>>();
                            let (ends, between) = (run("m_firstAndLastEdgePnts"), run("m_interiorEdgePnts"));
                            if let [first, last] = ends.as_slice() {
                                let mut piece: Vec<[f64; 2]> = std::iter::once(*first).chain(between).chain(std::iter::once(*last)).collect();
                                // each starts where the one before it ended
                                let far = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
                                if points.last().is_some_and(|&end| far(end, piece[0]) > far(end, piece[piece.len() - 1])) {
                                    piece.reverse();
                                }
                                points.extend(piece);
                            }
                            edge = first_of(e.get("m_next"));
                        }
                        points.dedup();
                        if points.len() >= 3 {
                            loops.push(points.into_iter().map(on).collect());
                        }
                        next = target(edges.get("m_nextLoop"));
                    }
                    loops
                };
                // a region a pattern fills: its outlines, for a filled region that covers
                if ["m_oBackgroundFilling", "m_pGFilling"].into_iter().filter_map(|f| target(face.get(f))).any(|f| target(f.1.get("m_data")).is_some()) {
                    let loops = loops_of();
                    if !loops.is_empty() {
                        out.patterned.push(loops);
                    }
                }
                for filling in ["m_oBackgroundFilling", "m_pGFilling"].into_iter().filter_map(|f| target(face.get(f))) {
                    let pattern_id = filling.1.get("m_patternId").and_then(|p| p.element_id());
                    // its own colour, else that of the style of the face (the colour of the type of a
                    // filled region)
                    let ink = filling.1.get("m_fillColor").and_then(|c| c.as_i64()).and_then(color_of).or_else(|| style(face).and_then(|s| types.line_color(s)));
                    let Some((_, pattern)) = target(filling.1.get("m_data")) else {
                        // no lines: a solid fill, or without a pattern a mask over what lies
                        // under it; of the outlines of the face in turn
                        let mask = pattern_id.is_none_or(|p| p < 0);
                        if !mask && !pattern_id.is_some_and(|p| types.pattern_solid(p)) {
                            continue;
                        }
                        let loops = loops_of();
                        if loops.is_empty() {
                            continue;
                        }
                        if mask {
                            out.masks.push(loops);
                        } else {
                            out.fills.push(Fill { loops, color: ink.unwrap_or([0, 0, 0]) });
                        }
                        continue;
                    };
                    let placer = filling.1.get("m_placer").and_then(|p| p.as_obj());
                    let on_paper = pattern_id.is_some_and(|p| types.pattern_on_paper(p));
                    let scale = placer.and_then(|p| p.get("m_scale")?.as_f64()).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0) * if on_paper { view_scale } else { 1.0 };
                    let origin = placer.and_then(|p| floats(p, "m_origin")).map_or([0.0, 0.0], |v| [v[0], v[1]]);
                    let dir = placer.and_then(|p| floats(p, "m_dir")).map_or([1.0, 0.0], |v| [v[0], v[1]]);
                    let mirror = if placer.is_some_and(|p| matches!(p.get("m_isMirrored"), Some(Value::Bool(true)))) { -1.0 } else { 1.0 };
                    // a point of the pattern on the plane of the face
                    let placed = |p: [f64; 2]| [origin[0] + scale * (p[0] * dir[0] - mirror * p[1] * dir[1]), origin[1] + scale * (p[0] * dir[1] + mirror * p[1] * dir[0])];
                    for grid in pattern.get("m_fillGrids").and_then(|g| g.as_list()).unwrap_or(&[]).iter().filter_map(|g| graph.objects.get(g.as_ptr()?)) {
                        let (Some(angle), Some(from), Some(deltas)) = (grid.get("m_angle").and_then(|a| a.as_f64()), floats(grid, "m_origin"), floats(grid, "m_deltas")) else { continue };
                        let segments: Vec<f64> = grid.get("m_segs").and_then(|s| s.floats()).unwrap_or_default();
                        let (sin, cos) = angle.sin_cos();
                        // line k of the grid: through this point, along `u`
                        let base = |k: f64| placed([from[0] + k * (deltas[0] * cos - deltas[1] * sin), from[1] + k * (deltas[0] * sin + deltas[1] * cos)]);
                        let (b0, b1) = (base(0.0), base(1.0));
                        let ahead = placed([from[0] + cos, from[1] + sin]);
                        let u = [(ahead[0] - b0[0]) / scale, (ahead[1] - b0[1]) / scale];
                        let n = [-u[1], u[0]];
                        let step = (b1[0] - b0[0]) * n[0] + (b1[1] - b0[1]) * n[1];
                        if step.abs() < 1e-7 {
                            continue;
                        }
                        let across = outline.iter().flatten().map(|p| ((p[0] - b0[0]) * n[0] + (p[1] - b0[1]) * n[1]) / step);
                        let (low, high) = across.fold((f64::MAX, f64::MIN), |(lo, hi), k| (lo.min(k), hi.max(k)));
                        let (low, high) = (low.ceil() as i64, high.floor() as i64);
                        if high < low || (high - low) as usize > MOST_HATCH_LINES {
                            continue;
                        }
                        for k in low..=high {
                            let b = base(k as f64);
                            // where the line crosses the outline: inside between each two
                            let mut cuts: Vec<f64> = outline
                                .iter()
                                .filter_map(|[p, q]| {
                                    let (sp, sq) = ((p[0] - b[0]) * n[0] + (p[1] - b[1]) * n[1], (q[0] - b[0]) * n[0] + (q[1] - b[1]) * n[1]);
                                    ((sp < 0.0) != (sq < 0.0)).then(|| {
                                        let f = sp / (sp - sq);
                                        (p[0] + f * (q[0] - p[0]) - b[0]) * u[0] + (p[1] + f * (q[1] - p[1]) - b[1]) * u[1]
                                    })
                                })
                                .collect();
                            cuts.sort_by(f64::total_cmp);
                            let mut dash = |from: f64, to: f64| {
                                if to - from > 1e-9 {
                                    out.line([on([b[0] + from * u[0], b[1] + from * u[1]]), on([b[0] + to * u[0], b[1] + to * u[1]])], ink);
                                }
                            };
                            let period: f64 = segments.iter().map(|s| s.abs() * scale).sum();
                            for inside in cuts.chunks_exact(2) {
                                if segments.is_empty() || period < 1e-7 || (inside[1] - inside[0]) / period > MOST_HATCH_LINES as f64 {
                                    dash(inside[0], inside[1]);
                                    continue;
                                }
                                // its strokes and gaps in turn, from the line's own start
                                let mut at = (inside[0] / period).floor() * period;
                                while at < inside[1] {
                                    for (i, s) in segments.iter().enumerate() {
                                        let length = s.abs() * scale;
                                        let stroke = if segments.iter().any(|s| *s < 0.0) { *s >= 0.0 } else { i % 2 == 0 };
                                        if stroke {
                                            dash(at.max(inside[0]), (at + length.max(1e-4)).min(inside[1]));
                                        }
                                        at += length;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            continue;
        }
        // the lines of a nested family lie where it is placed, as its regions do (the leader of a
        // row of bars); what it has in its own coordinates are reference lines, which never show
        let Some(shown) = style(o).filter(|s| types.line_shows(*s)) else { continue };
        let color = types.line_color(shown);
        let Some(ends) = floats(o, "m_endParams") else { continue };
        let (from, to) = (ends[0], ends[1]);
        let points: Vec<[f64; 3]> = match name {
            "GLine" => {
                let (Some(p), Some(d)) = (vector(o, "m_origin"), vector(o, "m_dirVec")) else { continue };
                [from, to].iter().map(|t| std::array::from_fn(|k| p[k] + t * d[k])).collect()
            }
            "GArc" => {
                let (Some(c), Some(x), Some(y)) = (vector(o, "m_center"), vector(o, "m_xVec"), vector(o, "m_yVec")) else { continue };
                let Some(r) = o.get("m_radius").and_then(|r| r.as_f64()).filter(|r| r.is_finite() && *r > 0.0) else { continue };
                // equal ends: a full circle
                let sweep = if (to - from).abs() < 1e-9 { std::f64::consts::TAU } else { to - from };
                let pieces = ((sweep.abs() / (std::f64::consts::TAU / 48.0)).ceil() as usize).clamp(2, 96);
                (0..=pieces)
                    .map(|i| {
                        let (s, c0) = (from + sweep * i as f64 / pieces as f64).sin_cos();
                        std::array::from_fn(|k| c[k] + r * (c0 * x[k] + s * y[k]))
                    })
                    .collect()
            }
            _ => continue,
        };
        for line in drawn(&points, types.line_dashes(shown).as_deref().map(|d| d.as_slice()), view_scale) {
            out.line(line, color);
        }
    }
    out.place(added.0, &added.1);
    out.solids_off = seen_as.model && solids > 0 && shown_solids == 0;
    out
}

/// What an element stores, read once.
fn source(doc: &rvt_read::Document, sources: &mut HashMap<u64, Arc<Source>>, id: u64) -> Arc<Source> {
    if let Some(known) = sources.get(&id) {
        return known.clone();
    }
    let read = doc.object(id).map(|o| read_source(&o)).unwrap_or_default();
    let read = Arc::new(read);
    sources.insert(id, read.clone());
    read
}

fn read_source(o: &ObjectView) -> Source {
    let mut values: HashMap<i64, Stored> = HashMap::new();
    let entries = |set: &'static str| o.get(set).and_then(|v| o.deref(v)).and_then(|s| s.get("m_paramSet")).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
    let pair = |p: &NestedObject| Some((p.get("m_paramId")?.element()?, p.get("m_value")?.clone()));
    for (id, v) in entries("m_pParamValueSetDouble").filter_map(pair) {
        values.entry(id).or_default().number = v.as_f64();
    }
    for (id, v) in entries("m_pParamValueSetInt").filter_map(pair) {
        values.entry(id).or_default().integer = v.as_i64();
    }
    for (id, v) in entries("m_pParamValueSetAString").filter_map(pair) {
        values.entry(id).or_default().text = v.as_str().map(str::to_string);
    }
    // a family type: the values of its family's parameters
    // a family type: the values of its family's parameters; a placed family: those of its own
    let params = ["m_pParams", "m_pInstParams"].into_iter().filter_map(|f| o.path(&[f, "m_params"])?.as_list()).flatten();
    for p in params.filter_map(FieldValue::as_object) {
        let Some(id) = p.get("m_paramId").and_then(FieldValue::element) else { continue };
        let text = p.get("m_str").and_then(FieldValue::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let stored = values.entry(id).or_default();
        if text.is_some() {
            stored.text = text;
        } else {
            stored.number = p.get("m_value").and_then(FieldValue::as_f64);
            stored.integer = p.get("m_int").and_then(FieldValue::as_i64);
        }
    }
    // its type: the field that names one, whatever the class calls it
    let named = |name: &str| o.get(name).and_then(FieldValue::element).and_then(|e| u64::try_from(e).ok()).filter(|e| *e != o.id);
    let kind = ["m_masterSymbolId", "m_symbolId", "m_attrId", "m_typeId"].into_iter().find_map(named).or_else(|| {
        let ends = |f: &str| ["AttributesId", "AttrId", "TypeId", "SymbolId"].iter().any(|end| f.ends_with(end));
        o.fields.iter().filter(|f| ends(&f.name)).find_map(|f| named(&f.name))
    });
    let name = o.path(&["m_symbolInfo", "m_name"]).and_then(FieldValue::as_str).map(str::to_string);
    Source { values, kind, name, level: named("m_assocLevelId").or_else(|| named("m_levelId")) }
}

/// What a label's part writes for an element: its own value, else its type's; none when neither
/// stores one.
fn value(doc: &rvt_read::Document, sources: &mut HashMap<u64, Arc<Source>>, specs: &mut HashMap<i64, String>, units: &Units, element: u64, part: &Part) -> Option<String> {
    let own = source(doc, sources, element);
    let kind = own.kind.map(|k| source(doc, sources, k));
    // the type's name is no stored value
    if part.parameter == -1002001 {
        return kind.as_ref().and_then(|k| k.name.clone()).or_else(|| own.name.clone());
    }
    // the height of its top: its level's and its offset from that
    let top = || {
        let offset = own.values.get(&-1001951)?.number?;
        let level = doc.object(own.level?)?;
        let height = vec3(level.get("m_pSurface").and_then(|v| level.deref(v))?.get("m_origin"))?[2];
        Some(Stored { number: Some(height + offset), ..Stored::default() })
    };
    let computed;
    let stored = match own.values.get(&part.parameter).or_else(|| kind.as_ref()?.values.get(&part.parameter)) {
        Some(stored) => stored,
        None if part.parameter == -1001598 => {
            computed = top()?;
            &computed
        }
        None => return None,
    };
    if let Some(text) = &stored.text {
        return Some(text.clone());
    }
    // what the parameter measures: a project, shared or family parameter says so
    let spec = specs.entry(part.parameter).or_insert_with(|| {
        let def = u64::try_from(part.parameter).ok().and_then(|id| doc.object(id));
        def.and_then(|o| Some(o.path(&["m_pParamDef", "m_specTypeId", "m_typeId"])?.as_str()?.to_string())).unwrap_or_default()
    });
    let counts = spec.contains("spec.int") || spec.contains("spec.bool") || (spec.is_empty() && stored.number.is_none_or(|n| n == 0.0) && stored.integer.is_some_and(|i| i != 0));
    if counts {
        return stored.integer.map(|i| i.to_string());
    }
    let number = stored.number.filter(|n| n.is_finite())?;
    let length = spec.contains(":length") || spec.is_empty();
    let written = if length {
        // in the label's own unit, else as the project writes its lengths
        part.unit.unwrap_or(units.length).write(number, units.comma)
    } else {
        let figure = format!("{}", (number * 1000.0).round() / 1000.0);
        if units.comma { figure.replace('.', ",") } else { figure }
    };
    Some(if part.plus && number > 0.0 { format!("+{written}") } else { written })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project with tags (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn tags_have_the_labels_and_lines_of_their_types() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with tags");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let mut types = Types::new(&doc);
        let t = std::time::Instant::now();
        let tags = tags(&doc, &mut types);
        eprintln!("{} tags in {:?}", tags.len(), t.elapsed());
        let mut views: HashMap<u64, usize> = HashMap::new();
        for tag in &tags {
            *views.entry(tag.view).or_default() += 1;
        }
        let names: HashMap<u64, String> = rvt_export::views::all_views(&doc).into_iter().map(|v| (v.id, format!("{:?} {:?} 1:{:?}", v.kind, v.name, v.scale))).collect();
        for (view, n) in &views {
            eprintln!("  view {view} {:?}: {n} tags", names.get(view));
        }
        for tag in tags.iter().step_by(tags.len() / 12 + 1) {
            let texts: Vec<_> = tag.texts.iter().map(|x| (x.wording.as_str(), x.at.map(|v| (v * 1e5).round() / 100.0), (x.em * 1e5).round() / 100.0, x.align)).collect();
            eprintln!("  tag {} view {} head {:?} leader {:?} lines {} texts {texts:?}", tag.element, tag.view, tag.head.map(|v| (v * 100.0).round() / 100.0), tag.leader, tag.lines.len());
        }
        // which parameters the labels ask for that nothing stores
        let (mut sources, mut specs, units) = (HashMap::new(), HashMap::new(), Units::of(&doc, doc.object_ids_of_class("UnitsElem").first().copied()));
        let mut missing: HashMap<i64, (usize, usize)> = HashMap::new();
        for id in doc.object_ids_of_class("IndependentTag") {
            let Some(o) = doc.object(id) else { continue };
            let Some(shape) = o.get("m_symbolId").and_then(FieldValue::element).and_then(|k| shape(&doc, k, &mut types)) else { continue };
            let reference = o.objects.iter().find(|x| x.class == "TagReferenceWithLeader");
            let Some(tagged) = reference.and_then(|r| r.get("m_referenceIdKey")?.as_object()?.get("m_hostOrLinkElemId")?.as_object()?.get("m_linkInstOrHostId")?.element()).and_then(|e| u64::try_from(e).ok()) else { continue };
            for part in shape.labels.iter().flat_map(|l| &l.parts) {
                let count = missing.entry(part.parameter).or_default();
                count.1 += 1;
                if value(&doc, &mut sources, &mut specs, &units, tagged, part).is_none() {
                    count.0 += 1;
                }
            }
        }
        eprintln!("parameter: missing of asked {:?}", missing);
        eprintln!("turned: {:?}", tags.iter().flat_map(|t| &t.texts).filter(|x| x.angle != 0.0).map(|x| (&x.wording, x.angle)).collect::<Vec<_>>());
        let written = tags.iter().filter(|t| !t.texts.is_empty()).count();
        eprintln!("{written} of {} tags write a label", tags.len());
        assert!(written * 2 > tags.len(), "most tags write a label");
        assert!(tags.iter().any(|t| !t.lines.is_empty()), "a tag with the lines of its family");
    }
}
