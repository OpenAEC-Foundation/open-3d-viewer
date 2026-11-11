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
    fn of(doc: &rvt_read::Document) -> Units {
        let o = doc.object_ids_of_class("UnitsElem").first().and_then(|&id| doc.object(id));
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
    width: f64,
    parts: Vec<Part>,
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
    let units = Units::of(doc);
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
        for label in &shape.labels {
            let mut wording = String::new();
            for (k, part) in label.parts.iter().enumerate() {
                let value = tagged.and_then(|e| value(doc, &mut sources, &mut specs, &units, e, part)).unwrap_or_default();
                if part.line_break {
                    wording.push('\n');
                } else if k > 0 {
                    wording.extend(std::iter::repeat_n(' ', part.spaces));
                }
                wording.push_str(&part.prefix);
                wording.push_str(&value);
                wording.push_str(&part.suffix);
            }
            if wording.trim().is_empty() {
                continue;
            }
            let mut text = TagText { at: label.at, wording, em: label.em, align: label.align, width: label.width, angle };
            // from where the label stands to the foot of its lines
            text.at[1] -= [text.height(), text.height() / 2.0, 0.0][label.vertical as usize];
            text.at = turn(text.at);
            texts.push(text);
        }
        let lines = shape.lines.iter().map(|l| [turn(l[0]), turn(l[1])]).collect();
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
    Some(Shape { labels, lines: family_lines(doc, id), arrow })
}

/// A label of a tag type; none for one that writes nothing.
fn label(l: &NestedObject, types: &mut Types) -> Option<Label> {
    let orig = vec3(l.get("m_orig"))?;
    let flags = l.get("m_flags").and_then(FieldValue::as_i64).unwrap_or(0);
    let em = l.get("m_attrId").and_then(FieldValue::element).and_then(|k| types.text_em(k)).unwrap_or(crate::document::NOTE_HEIGHT);
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
    // where its box hangs from its origin: bits of its flags
    let vertical = if flags & 0x800 != 0 { 2 } else if flags & 0x400 != 0 { 1 } else { 0 };
    Some(Label { at: [orig[0] * FT, orig[1] * FT], vertical, align: (flags & 3).clamp(0, 2), em, width, parts })
}

/// The lines of a tag type's family, from its saved graphics (on paper, metres): straight ones as
/// they are, arcs and circles in pieces of at most 7.5 degrees.
fn family_lines(doc: &rvt_read::Document, kind: u64) -> Vec<[[f64; 2]; 2]> {
    let Some(saved) = doc.saved_graphics() else { return Vec::new() };
    let Some(graph) = saved.bodies.get(&kind).and_then(|body| rvt_core::graph::Graph::decode(&saved.schema, body).ok()) else { return Vec::new() };
    let class = |o: &rvt_core::graph::Object| saved.schema.class(o.class).map_or("", |c| c.name.as_str());
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut open = vec![0usize];
    while let Some(i) = open.pop() {
        let Some(o) = graph.objects.get(i).filter(|_| seen.insert(i)) else { continue };
        let name = class(o);
        // what shows from one side only: the family's reference planes
        if name == "GFilter" && o.get("m_oConditions").and_then(|c| c.as_list()).is_some_and(|c| !c.is_empty()) {
            continue;
        }
        open.extend(o.get("m_subNodes").and_then(|s| s.as_list()).unwrap_or(&[]).iter().filter_map(|p| p.as_ptr()));
        let styled = o.get("m_GInfo").and_then(|g| g.as_obj()).and_then(|g| g.get("m_categoryId")?.element_id()).is_some_and(|c| c != -1);
        if !styled {
            continue;
        }
        let floats = |name: &str| o.get(name).and_then(|v| v.floats()).filter(|v| v.len() >= 2 && v.iter().all(|x| x.is_finite()));
        let Some(ends) = floats("m_endParams") else { continue };
        let (from, to) = (ends[0], ends[1]);
        let points: Vec<[f64; 2]> = match name {
            "GLine" => {
                let (Some(p), Some(d)) = (floats("m_origin"), floats("m_dirVec")) else { continue };
                [from, to].iter().map(|t| [(p[0] + t * d[0]) * FT, (p[1] + t * d[1]) * FT]).collect()
            }
            "GArc" => {
                let (Some(c), Some(x), Some(y)) = (floats("m_center"), floats("m_xVec"), floats("m_yVec")) else { continue };
                let Some(r) = o.get("m_radius").and_then(|r| r.as_f64()).filter(|r| r.is_finite() && *r > 0.0) else { continue };
                // equal ends: a full circle
                let sweep = if (to - from).abs() < 1e-9 { std::f64::consts::TAU } else { to - from };
                let pieces = ((sweep.abs() / (std::f64::consts::TAU / 48.0)).ceil() as usize).clamp(2, 96);
                (0..=pieces)
                    .map(|k| {
                        let (s, c0) = (from + sweep * k as f64 / pieces as f64).sin_cos();
                        [(c[0] + r * (c0 * x[0] + s * y[0])) * FT, (c[1] + r * (c0 * x[1] + s * y[1])) * FT]
                    })
                    .collect()
            }
            _ => continue,
        };
        out.extend(points.windows(2).filter(|w| w[0] != w[1]).map(|w| [w[0], w[1]]));
    }
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
    for p in o.path(&["m_pParams", "m_params"]).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object) {
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
        let (mut sources, mut specs, units) = (HashMap::new(), HashMap::new(), Units::of(&doc));
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
