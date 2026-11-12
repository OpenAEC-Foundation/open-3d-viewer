//! The drawings of a Revit file that show no model: its legends and drafting views, with the
//! detail lines, texts and dimensions drawn in them ([`crate::model::Draft`]).
//!
//! Read through the generic object view, named schema fields only:
//! - a legend is a `DBViewLegend`, a drafting view a `DBViewAnonDrafting`; an element drawn in
//!   one names it in its element record (`ElementInfo::view`);
//! - a detail line is a `CurveElem` with `m_detail`: its curve is `CurveElemDriver::m_pCrv`, a
//!   `GLine` (`m_origin`, `m_dirVec`, between `m_endParams`);
//! - a text is a `TextNote` and a linear dimension a `LinearDimString`, with their types, as
//!   [`super::rvt_drawings`] reads them; an element hidden in its view is left out. A dimension
//!   is drawn as the file has it: its lines and its figures, as lines and texts of the drawing;
//! - a tag, a detail component and a filled region are drawn as [`super::rvt_tags`] reads them.
//!
//! A view's coordinates are feet in its own plane, at full size.

use super::rvt_drawings::{text_of, vec3, Types, DIMENSION_MAKERS};
use crate::model::{Draft, DraftKind, DraftLine, DraftText};
use rvt_export::views::{ViewInfo, ViewKind as K};
use rvt_model::{FieldValue, ObjectView};
use std::collections::HashMap;

const FT: f64 = 0.3048;
/// The category of the dimensions that show (the others of their class are constraints).
const DIMENSIONS: i64 = -2000260;

/// The legends and drafting views of the file, each kind by name.
pub(super) fn drafts(doc: &rvt_read::Document, tags: &[super::rvt_tags::Tag], details: &[super::rvt_tags::Detail]) -> Vec<Draft> {
    let mut drafts: Vec<Draft> = Vec::new();
    let mut index: HashMap<u64, usize> = HashMap::new();
    for v in rvt_export::views::all_views(doc) {
        let Some(kind) = draft_kind(&v) else { continue };
        index.insert(v.id, drafts.len());
        let scale = v.scale.filter(|s| s.is_finite() && *s >= 1.0).unwrap_or(1.0) as f32;
        drafts.push(Draft { id: v.id, name: v.name, kind, scale, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new(), line_colors: Vec::new(), fills: Vec::new() });
    }
    if drafts.is_empty() {
        return drafts;
    }
    let mut types = Types::new(doc);
    // the view an element is drawn in, when it is one of these: its id and its place
    let owner = |id: u64| doc.element(id).and_then(|e| e.view).and_then(|v| u64::try_from(v).ok()).and_then(|v| Some((v, *index.get(&v)?)));
    for id in doc.object_ids_of_class("CurveElem") {
        let Some((view, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let _ = view;
        let Some(o) = doc.object(id) else { continue };
        let scale = drafts[k].scale as f64;
        let (lines, color) = detail_line(&o, &mut types, scale);
        let draft = &mut drafts[k];
        draft.line_colors.resize(draft.lines.len(), None);
        draft.line_colors.extend(std::iter::repeat_n(color, lines.len()));
        draft.lines.extend(lines);
    }
    for id in doc.object_ids_of_class("TextNote") {
        let Some((_, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let scale = drafts[k].scale as f64;
        if let Some(text) = doc.object(id).and_then(|o| text_note(&o, scale, &mut types)) {
            drafts[k].texts.push(text);
        }
    }
    let mut dimensioned: Vec<u64> = Vec::new();
    for (id, make) in DIMENSION_MAKERS.iter().flat_map(|(class, make)| doc.object_ids_of_class(class).into_iter().map(move |id| (id, *make))) {
        let Some((_, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id).filter(|o| o.category == Some(DIMENSIONS)) else { continue };
        // as the file draws it: its lines and its figures
        let scale = drafts[k].scale as f64;
        let Some(drawn) = make(&o, &|p| [p[0] * FT, p[1] * FT], scale, &mut types) else { continue };
        dimensioned.push(id);
        drafts[k].lines.extend(drawn.lines.iter().map(|l| DraftLine { element: id, a: l[0].map(|v| v as f32), b: l[1].map(|v| v as f32) }));
        drafts[k].texts.extend(drawn.texts.into_iter().map(|t| {
            let height = t.em * 1.3 * scale * t.wording.lines().count().max(1) as f64;
            DraftText { element: id, at: t.at.map(|v| v as f32), width: 0.0, height: height as f32, size: (t.em * scale) as f32, align: 1, angle: t.angle as f32, face: t.face, text: t.wording }
        }));
    }
    // a tag: its labels as texts, the lines of its family and its leader, all hanging from its head
    for tag in tags {
        let Some(&k) = index.get(&tag.view) else { continue };
        let scale = drafts[k].scale as f64;
        let on = |p: [f64; 2]| [(tag.head[0] * FT + p[0] * scale) as f32, (tag.head[1] * FT + p[1] * scale) as f32];
        for text in &tag.texts {
            let (width, height) = (text.width * scale, text.height() * scale);
            let align = text.align.clamp(0, 2) as usize;
            let at = on([text.at[0] - [0.0, text.width / 2.0, text.width][align], text.at[1]]);
            drafts[k].texts.push(DraftText { element: tag.element, at, width: width as f32, height: height as f32, size: (text.em * scale) as f32, align: align as u8, angle: text.angle as f32, text: text.wording.clone(), face: text.face.clone() });
        }
        drafts[k].lines.extend(tag.lines.iter().map(|l| DraftLine { element: tag.element, a: on(l[0]), b: on(l[1]) }));
        let leader = tag.leader_lines(&|p| [p[0] * FT, p[1] * FT], scale);
        drafts[k].lines.extend(leader.iter().map(|l| DraftLine { element: tag.element, a: l[0].map(|v| v as f32), b: l[1].map(|v| v as f32) }));
    }
    // what is always drawn over the rest: the tags and the dimensions
    let above: std::collections::HashSet<u64> = tags.iter().map(|t| t.element).chain(dimensioned.iter().copied()).collect();
    let mut covering: HashMap<usize, Vec<(u64, Vec<Vec<[f32; 2]>>)>> = HashMap::new();
    // per drawing: where its view's draw order has each element, from the back to the front
    let orders: Vec<HashMap<u64, usize>> = drafts
        .iter()
        .map(|d| {
            let view = doc.object(d.id);
            let listed = view.as_ref().and_then(|o| o.get("m_pDetailDrawOrderMgr").and_then(|v| o.deref(v))?.get("m_aDrawOrder")?.as_list());
            listed.unwrap_or(&[]).iter().filter_map(|e| u64::try_from(e.element()?).ok()).enumerate().map(|(k, e)| (e, k)).collect()
        })
        .collect();
    // a detail component: the lines of its family
    for detail in details {
        let Some(&k) = index.get(&detail.view) else { continue };
        // the lines before these have no colour of their own
        let draft = &mut drafts[k];
        draft.line_colors.resize(draft.lines.len(), None);
        draft.lines.extend(detail.lines.iter().map(|l| DraftLine { element: detail.element, a: point(l[0]), b: point(l[1]) }));
        draft.line_colors.extend(detail.colors.iter().copied());
        let scale = draft.scale as f64;
        // its texts and labels: from the foot of their lines, along them to where the box starts
        for (foot, text) in &detail.texts {
            let (width, height) = (text.width * scale, text.height() * scale);
            let align = text.align.clamp(0, 2) as usize;
            let back = [0.0, width / 2.0, width][align];
            let (s, c) = text.angle.sin_cos();
            let at = [(foot[0] * FT - c * back) as f32, (foot[1] * FT - s * back) as f32];
            draft.texts.push(DraftText { element: detail.element, at, width: width as f32, height: height as f32, size: (text.em * scale) as f32, align: align as u8, angle: text.angle as f32, text: text.wording.clone(), face: text.face.clone() });
        }
        draft.fills.extend(detail.fills.iter().map(|f| crate::model::DraftFill { element: detail.element, loops: f.loops.iter().map(|l| l.iter().map(|&p| point(p)).collect()).collect(), color: f.color }));
        // what covers what lies under it: a masking region and a region filled in one colour
        let covers = detail.masks.iter().chain(detail.fills.iter().map(|f| &f.loops));
        covering.entry(k).or_default().extend(covers.map(|loops| (detail.element, loops.iter().map(|l| l.iter().map(|&p| point(p)).collect()).collect())));
    }
    for (k, draft) in drafts.iter_mut().enumerate() {
        draft.line_colors.resize(draft.lines.len(), None);
        if let Some(covers) = covering.get(&k) {
            cover(draft, covers, &above, &orders[k]);
        }
    }
    drafts.sort_by(|a, b| (a.kind as u8).cmp(&(b.kind as u8)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    drafts
}

/// Takes away what the covering regions of a drawing hide: the pieces of its lines inside a
/// region of an element that lies over theirs in the view's draw order
/// (`DrawOrderMgr::m_aDrawOrder`, from the back to the front). The lines of `above` (tags,
/// dimensions) are never hidden.
fn cover(draft: &mut Draft, covers: &[(u64, Vec<Vec<[f32; 2]>>)], above: &std::collections::HashSet<u64>, order: &HashMap<u64, usize>) {
    // where an element lies: as the view's draw order says, from the back to the front; one it
    // does not name lies under those, in the order they were made
    let rank = |element: u64| order.get(&element).map_or((0, element), |k| (1, *k as u64));
    // each with its box, to pass over quickly
    let boxed: Vec<(u64, &Vec<Vec<[f32; 2]>>, [f32; 4])> = covers
        .iter()
        .map(|(element, loops)| {
            let all = loops.iter().flatten();
            let b = all.fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
            (*element, loops, b)
        })
        .collect();
    let inside = |loops: &Vec<Vec<[f32; 2]>>, p: [f32; 2]| {
        let mut odd = false;
        for outline in loops {
            for (a, b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                    odd = !odd;
                }
            }
        }
        odd
    };
    let (lines, colors) = (std::mem::take(&mut draft.lines), std::mem::take(&mut draft.line_colors));
    for (k, line) in lines.into_iter().enumerate() {
        let color = colors.get(k).copied().flatten();
        // the pieces of the line that still show
        let mut pieces = vec![(line.a, line.b)];
        if !above.contains(&line.element) {
            for (_, loops, b) in boxed.iter().filter(|(element, ..)| *element != line.element && rank(*element) > rank(line.element)) {
                if line.a[0].max(line.b[0]) < b[0] || line.a[0].min(line.b[0]) > b[2] || line.a[1].max(line.b[1]) < b[1] || line.a[1].min(line.b[1]) > b[3] {
                    continue;
                }
                let mut kept = Vec::new();
                for (p, q) in pieces {
                    let d = [q[0] - p[0], q[1] - p[1]];
                    // where it crosses the outlines, along it
                    let mut cuts = vec![0.0f32, 1.0];
                    for outline in loops.iter() {
                        for (a, c) in outline.iter().zip(outline.iter().cycle().skip(1)) {
                            let e = [c[0] - a[0], c[1] - a[1]];
                            let det = d[0] * e[1] - d[1] * e[0];
                            if det.abs() < 1e-12 {
                                continue;
                            }
                            let t = ((a[0] - p[0]) * e[1] - (a[1] - p[1]) * e[0]) / det;
                            let s = ((a[0] - p[0]) * d[1] - (a[1] - p[1]) * d[0]) / det;
                            if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&s) {
                                cuts.push(t);
                            }
                        }
                    }
                    cuts.sort_by(f32::total_cmp);
                    for w in cuts.windows(2).filter(|w| w[1] - w[0] > 1e-6) {
                        let at = |t: f32| [p[0] + t * d[0], p[1] + t * d[1]];
                        if !inside(loops, at((w[0] + w[1]) / 2.0)) {
                            kept.push((at(w[0]), at(w[1])));
                        }
                    }
                }
                pieces = kept;
                if pieces.is_empty() {
                    break;
                }
            }
        }
        for (a, b) in pieces {
            draft.lines.push(DraftLine { element: line.element, a, b });
            draft.line_colors.push(color);
        }
    }
}

/// Whether a view is a legend or a drafting view of the project (not a template, not a view of
/// a family or a group).
fn draft_kind(v: &ViewInfo) -> Option<DraftKind> {
    if v.is_template || v.owned {
        return None;
    }
    match &v.kind {
        K::Other(k) if k == "Legend" => Some(DraftKind::Legend),
        K::Other(k) if k == "Drafting" => Some(DraftKind::Drafting),
        _ => None,
    }
}

fn point(p: [f64; 3]) -> [f32; 2] {
    [(p[0] * FT) as f32, (p[1] * FT) as f32]
}

/// A detail line as lines of the drawing: a straight one as it is, an arc or circle in pieces of
/// at most 7.5 degrees; none for another curve.
fn detail_line(o: &ObjectView, types: &mut Types, scale: f64) -> (Vec<DraftLine>, Option<[u8; 3]>) {
    let (lines, color, pattern) = detail_curve(o, types);
    let Some(pattern) = pattern else { return (lines, color) };
    // in the line pattern of its style: along its pieces in turn
    let mut points: Vec<[f64; 2]> = lines.first().map(|l| l.a.map(f64::from)).into_iter().collect();
    points.extend(lines.iter().map(|l| l.b.map(f64::from)));
    let strokes = super::rvt_drawings::dashed(&points, &pattern, scale * FT);
    (strokes.into_iter().map(|s| DraftLine { element: o.id, a: s[0].map(|v| v as f32), b: s[1].map(|v| v as f32) }).collect(), color)
}

/// A detail line as it runs, with the colour and the line pattern of its line style
/// (`GInfo::m_categoryId` of its curve); nothing for a style that is not drawn.
fn detail_curve(o: &ObjectView, types: &mut Types) -> (Vec<DraftLine>, Option<[u8; 3]>, Option<std::sync::Arc<Vec<f64>>>) {
    let none = || (Vec::new(), None, None);
    let lines = detail_points(o);
    let curve = o.get("m_pCurveDriver").and_then(|v| o.deref(v)).and_then(|d| d.get("m_pCrv")).and_then(|v| o.deref(v));
    let style = curve.and_then(|c| c.get("m_GInfo")?.as_object()?.get("m_categoryId")?.element()).filter(|s| *s != -1);
    match style {
        Some(s) if !types.line_shows(s) => none(),
        Some(s) => (lines, types.line_color(s), types.line_dashes(s)),
        None => (lines, None, None),
    }
}

fn detail_points(o: &ObjectView) -> Vec<DraftLine> {
    let curve = || {
        matches!(o.get("m_detail"), Some(FieldValue::Bool(true))).then_some(())?;
        o.get("m_pCurveDriver").and_then(|v| o.deref(v))?.get("m_pCrv").and_then(|v| o.deref(v))
    };
    let Some(curve) = curve() else { return Vec::new() };
    let ends = curve.get("m_endParams").and_then(FieldValue::as_list).unwrap_or(&[]);
    let end = |i: usize| ends.get(i).and_then(FieldValue::as_f64).filter(|t| t.is_finite() && t.abs() < 1e6).unwrap_or(0.0);
    let (from, to) = (end(0), end(1));
    let points: Vec<[f32; 2]> = match curve.class.as_str() {
        "GLine" => {
            let (Some(origin), Some(dir)) = (vec3(curve.get("m_origin")), vec3(curve.get("m_dirVec"))) else { return Vec::new() };
            [from, to].iter().map(|t| point([origin[0] + t * dir[0], origin[1] + t * dir[1], origin[2] + t * dir[2]])).collect()
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
                    point([c[0] + r * (k * x[0] + s * y[0]), c[1] + r * (k * x[1] + s * y[1]), 0.0])
                })
                .collect()
        }
        _ => return Vec::new(),
    };
    points.windows(2).filter(|w| w[0] != w[1]).map(|w| DraftLine { element: o.id, a: w[0], b: w[1] }).collect()
}

/// The font size an RTF text starts with: its first `\fs` (half points), in points.
pub(super) fn rtf_points(rtf: &[u8]) -> Option<f64> {
    let at = rtf.windows(3).enumerate().find(|(i, w)| *w == b"\\fs" && rtf.get(i + 3).is_some_and(u8::is_ascii_digit))?.0 + 3;
    let digits = rtf[at..].iter().take_while(|b| b.is_ascii_digit()).count();
    let half: f64 = std::str::from_utf8(&rtf[at..at + digits]).ok()?.parse().ok()?;
    Some(half / 2.0)
}

/// A text with its box and the em of its letters, at full size in a view of `scale`.
fn text_note(o: &ObjectView, scale: f64, types: &mut Types) -> Option<DraftText> {
    let text = text_of(o, types)?;
    let orig = vec3(o.get("m_orig"))?;
    let number = |name: &str| o.get(name).and_then(FieldValue::as_f64).filter(|v| v.is_finite() && *v > 0.0);
    let (size, height) = (text.em * scale, text.height * scale);
    // the width its lines are set in, on paper
    let width = number("m_lineWidth").or(number("m_trueWidth")).map_or(0.0, |w| w * FT * scale);
    // the box hangs from its origin, its left, middle or right, along its up; it is turned about
    // its foot under that
    let up = vec3(o.get("m_upVec")).unwrap_or([0.0, 1.0, 0.0]);
    let base = vec3(o.get("m_baseVec")).unwrap_or([1.0, 0.0, 0.0]);
    let foot = [orig[0] * FT - up[0] * height, orig[1] * FT - up[1] * height];
    let left = foot[0] - [0.0, width / 2.0, width][text.align.clamp(0, 2) as usize];
    let angle = Some(base[1].atan2(base[0])).filter(|a| a.abs() > 1e-6).unwrap_or(0.0);
    Some(DraftText { element: o.id, at: [left as f32, foot[1] as f32], width: width as f32, height: height as f32, size: size as f32, align: text.align.clamp(0, 2) as u8, angle: angle as f32, text: text.wording, face: text.face })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_tells_its_font_size() {
        assert_eq!(rtf_points(b"{\\rtf1\\pard\\cf1\\f0\\fs14 text\\par}"), Some(7.0));
        assert_eq!(rtf_points(b"{\\rtf1 text}"), None);
    }

    /// A project with legends and drafting views (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn legends_and_drafting_views_have_lines_texts_and_dimensions() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with legends");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let t = std::time::Instant::now();
        let mut types = Types::new(&doc);
        let (tags, details) = (super::super::rvt_tags::tags(&doc, &mut types), super::super::rvt_tags::details(&doc, &mut types));
        let drafts = drafts(&doc, &tags, &details);
        eprintln!("{} drafts in {:?}", drafts.len(), t.elapsed());
        for d in &drafts {
            eprintln!("{:?} {:?} 1:{}: {} lines, {} texts, {} dimensions, extent {:?}", d.kind, d.name, d.scale, d.lines.len(), d.texts.len(), d.dimensions.len(), d.extent());
        }
        for t in drafts.iter().flat_map(|d| &d.texts).take(4) {
            eprintln!("  text {t:?}");
        }
        assert!(drafts.iter().any(|d| d.kind == DraftKind::Legend && !d.texts.is_empty()));
        assert!(drafts.iter().any(|d| !d.lines.is_empty()));
        // a dimension's figure the file writes a text instead of
        assert!(drafts.iter().flat_map(|d| &d.texts).any(|t| t.text == "2h"));
    }
}
