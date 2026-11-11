//! The drawings of a Revit file that show no model: its legends and drafting views, with the
//! detail lines, texts and dimensions drawn in them ([`crate::model::Draft`]).
//!
//! Read through the generic object view, named schema fields only:
//! - a legend is a `DBViewLegend`, a drafting view a `DBViewAnonDrafting`; an element drawn in
//!   one names it in its element record (`ElementInfo::view`);
//! - a detail line is a `CurveElem` with `m_detail`: its curve is `CurveElemDriver::m_pCrv`, a
//!   `GLine` (`m_origin`, `m_dirVec`, between `m_endParams`);
//! - a text is a `TextNote` and a linear dimension a `LinearDimString`, with their types, as
//!   [`super::rvt_drawings`] reads them; an element hidden in its view is left out;
//! - a tag is drawn as [`super::rvt_tags`] reads it.
//!
//! A view's coordinates are feet in its own plane, at full size.

use super::rvt_drawings::{text_of, vec3, Types};
use crate::document::DimensionStyle;
use crate::model::{Draft, DraftDimension, DraftKind, DraftLine, DraftText};
use rvt_export::views::{ViewInfo, ViewKind as K};
use rvt_model::{FieldValue, ObjectView};
use std::collections::HashMap;
use std::sync::Arc;

const FT: f64 = 0.3048;
/// The category of the dimensions that show (the others of their class are constraints).
const DIMENSIONS: i64 = -2000260;

/// The legends and drafting views of the file, each kind by name.
pub(super) fn drafts(doc: &rvt_read::Document, tags: &[super::rvt_tags::Tag]) -> Vec<Draft> {
    let mut drafts: Vec<Draft> = Vec::new();
    let mut index: HashMap<u64, usize> = HashMap::new();
    for v in rvt_export::views::all_views(doc) {
        let Some(kind) = draft_kind(&v) else { continue };
        index.insert(v.id, drafts.len());
        let scale = v.scale.filter(|s| s.is_finite() && *s >= 1.0).unwrap_or(1.0) as f32;
        drafts.push(Draft { name: v.name, kind, scale, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() });
    }
    if drafts.is_empty() {
        return drafts;
    }
    let mut types = Types::new(doc);
    let mut styles: HashMap<i64, Arc<DimensionStyle>> = HashMap::new();
    let standard = Arc::new(DimensionStyle::standard());
    // the view an element is drawn in, when it is one of these: its id and its place
    let owner = |id: u64| doc.element(id).and_then(|e| e.view).and_then(|v| u64::try_from(v).ok()).and_then(|v| Some((v, *index.get(&v)?)));
    for id in doc.object_ids_of_class("CurveElem") {
        let Some((view, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let _ = view;
        let Some(o) = doc.object(id) else { continue };
        drafts[k].lines.extend(detail_line(&o));
    }
    for id in doc.object_ids_of_class("TextNote") {
        let Some((_, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let scale = drafts[k].scale as f64;
        if let Some(text) = doc.object(id).and_then(|o| text_note(&o, scale, &mut types)) {
            drafts[k].texts.push(text);
        }
    }
    for id in doc.object_ids_of_class("LinearConstraint") {
        let Some((_, k)) = owner(id).filter(|(v, _)| !types.hidden(*v, id)) else { continue };
        let Some(o) = doc.object(id).filter(|o| o.category == Some(DIMENSIONS)) else { continue };
        let style = o.get("m_styleSymbolId").and_then(FieldValue::element).map_or_else(
            || standard.clone(),
            |kind| styles.entry(kind).or_insert_with(|| types.style_of(kind).map_or_else(|| standard.clone(), Arc::new)).clone(),
        );
        drafts[k].dimensions.extend(dimensions(&o, &style));
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
            drafts[k].texts.push(DraftText { element: tag.element, at, width: width as f32, height: height as f32, size: (text.em * scale) as f32, align: align as u8, angle: text.angle as f32, text: text.wording.clone() });
        }
        drafts[k].lines.extend(tag.lines.iter().map(|l| DraftLine { element: tag.element, a: on(l[0]), b: on(l[1]) }));
        let leader = tag.leader_lines(&|p| [p[0] * FT, p[1] * FT], scale);
        drafts[k].lines.extend(leader.iter().map(|l| DraftLine { element: tag.element, a: l[0].map(|v| v as f32), b: l[1].map(|v| v as f32) }));
    }
    drafts.sort_by(|a, b| (a.kind as u8).cmp(&(b.kind as u8)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    drafts
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
fn detail_line(o: &ObjectView) -> Vec<DraftLine> {
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
    Some(DraftText { element: o.id, at: [left as f32, foot[1] as f32], width: width as f32, height: height as f32, size: size as f32, align: text.align.clamp(0, 2) as u8, angle: angle as f32, text: text.wording })
}

/// A dimension string as dimensions on its line: one between each two points it measures.
fn dimensions(o: &ObjectView, style: &Arc<DimensionStyle>) -> Vec<DraftDimension> {
    let measured: Vec<[f64; 3]> = o.get("m_refPnts").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|p| vec3(Some(p))).collect();
    let line = o.get("m_pDimLine").and_then(|v| o.deref(v));
    let (Some(origin), Some(dir)) = (line.and_then(|l| vec3(l.get("m_origin"))), line.and_then(|l| vec3(l.get("m_dirVec")))) else { return Vec::new() };
    let along = |p: [f64; 3]| (0..3).map(|k| (p[k] - origin[k]) * dir[k]).sum::<f64>();
    let on_line = |t: f64| point([origin[0] + t * dir[0], origin[1] + t * dir[1], origin[2] + t * dir[2]]);
    let mut stops: Vec<(f64, [f64; 3])> = measured.iter().map(|&p| (along(p), p)).collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    stops
        .windows(2)
        .filter(|w| (w[1].0 - w[0].0) * FT >= 0.0005)
        .map(|w| DraftDimension { element: o.id, from: on_line(w[0].0), to: on_line(w[1].0), measured: [point(w[0].1), point(w[1].1)], style: style.clone() })
        .collect()
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
        let drafts = drafts(&doc, &super::super::rvt_tags::tags(&doc, &mut Types::new(&doc)));
        eprintln!("{} drafts in {:?}", drafts.len(), t.elapsed());
        for d in &drafts {
            eprintln!("{:?} {:?} 1:{}: {} lines, {} texts, {} dimensions, extent {:?}", d.kind, d.name, d.scale, d.lines.len(), d.texts.len(), d.dimensions.len(), d.extent());
        }
        for t in drafts.iter().flat_map(|d| &d.texts).take(4) {
            eprintln!("  text {t:?}");
        }
        assert!(drafts.iter().any(|d| d.kind == DraftKind::Legend && !d.texts.is_empty()));
        assert!(drafts.iter().any(|d| !d.lines.is_empty()) && drafts.iter().any(|d| !d.dimensions.is_empty()));
        // the dimensions have the styles of their types, not only the standard one
        assert!(drafts.iter().flat_map(|d| &d.dimensions).any(|m| m.style.name != DimensionStyle::STANDARD));
    }
}
