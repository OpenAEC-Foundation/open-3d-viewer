//! The checks on the models as they are drawn, for every open model whatever its format: those
//! of the rooms ([`super::room_checks`]) and those of the elements' places: an element that
//! touches no other hangs loose, and an element far from all the rest was put in the wrong place.
//!
//! The checks that read an IFC file itself give a report per IFC model; a model of another
//! format (a coordination file, a native file) gets a report here, with these checks alone.

use super::collab::{field, Checked};
use super::App;
use crate::collab::rules::RuleSet;
use crate::model::Model;
use bim_validator_core::{Finding, RuleSource, Severity, SpecificationResult, ValidationReport, ValidationStatus as S};
use glam::Vec3;
use std::sync::Arc;

/// An element as these checks see it: its box and whether it is part of what is built.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub id: u64,
    pub min: Vec3,
    pub max: Vec3,
}

/// The classes that are no part of what is built, or stand loose by their nature.
fn loose_by_nature(class: &str) -> bool {
    let class = class.to_ascii_lowercase();
    ["ifcspace", "ifcopening", "ifcgrid", "ifcannotation", "ifcsite", "ifcgeographic", "ifcfurni", "ifcsystemfurni", "ifcvirtual", "ifczone", "ifcbuildingelementproxy", "ifctransportelement", "ifccivilelement"]
        .iter()
        .any(|c| class.starts_with(c))
}

/// The elements that touch no other: none comes within `gap` of their box.
pub fn loose(placed: &[Placed], gap: f32) -> Vec<usize> {
    let mut order: Vec<usize> = (0..placed.len()).collect();
    order.sort_by(|&a, &b| placed[a].min.x.total_cmp(&placed[b].min.x));
    let mut touched = vec![false; placed.len()];
    let reach = Vec3::splat(gap);
    for (at, &i) in order.iter().enumerate() {
        for &j in &order[at + 1..] {
            if placed[j].min.x > placed[i].max.x + gap {
                break;
            }
            if (touched[i] && touched[j]) || placed[i].id == placed[j].id {
                continue;
            }
            if (placed[i].min - reach).cmple(placed[j].max).all() && (placed[i].max + reach).cmpge(placed[j].min).all() {
                touched[i] = true;
                touched[j] = true;
            }
        }
    }
    (0..placed.len()).filter(|&k| !touched[k]).collect()
}

/// The elements far from all the rest: more than `far` metres from the middle most elements
/// lie around (the median of their middles, so the strays do not move it).
pub fn strays(placed: &[Placed], far: f32) -> Vec<usize> {
    if placed.len() < 8 {
        return Vec::new();
    }
    let middle = |k: usize| (placed[k].min + placed[k].max) / 2.0;
    let median = |axis: usize| {
        let mut v: Vec<f32> = (0..placed.len()).map(|k| middle(k)[axis]).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    let centre = Vec3::new(median(0), median(1), median(2));
    // how far most of the model reaches from there: nine in ten elements
    let mut reach: Vec<f32> = (0..placed.len()).map(|k| middle(k).distance(centre)).collect();
    reach.sort_by(f32::total_cmp);
    let most = reach[reach.len() * 9 / 10];
    (0..placed.len()).filter(|&k| middle(k).distance(centre) > (3.0 * most).max(far)).collect()
}

fn result(code: &str, name: &str) -> SpecificationResult {
    SpecificationResult {
        name: name.into(),
        identifier: Some(code.into()),
        status: S::Passed,
        applicable_count: 0,
        passed_count: 0,
        failed_count: 0,
        findings: Vec::new(),
        reasons: Vec::new(),
        source: RuleSource::ModelChecks,
        severity: Severity::Warning,
    }
}

impl App {
    /// The elements of open model `which` with a shape and a class that is built.
    fn placed_elements(model: &Model, which: u32) -> Vec<Placed> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for layer in model.layers.iter().filter(|l| !l.points) {
            for &(id, ..) in &layer.elements {
                if model.element_key(id).model != which || !seen.insert(id) {
                    continue;
                }
                let class = model.props.get(&id).and_then(|p| field(p, "IfcType")).unwrap_or("");
                if class.is_empty() || loose_by_nature(class) {
                    continue;
                }
                if let Some(b) = model.element_bounds(id) {
                    out.push(Placed { id, min: Vec3::from_array(b.min.map(|v| v as f32)), max: Vec3::from_array(b.max.map(|v| v as f32)) });
                }
            }
        }
        out
    }

    /// Adds the checks on the drawn models to `checked`: a report for each open model that has
    /// none, the checks of the rooms when the rule set asks them, and those of the elements'
    /// places with the model checks. Returns the walks of the rooms to their way out.
    pub(super) fn shape_checks(&self, checked: &mut Vec<Checked>, rules: &RuleSet) -> std::collections::HashMap<u64, Vec<[f32; 3]>> {
        let Some(model) = self.model.clone() else { return Default::default() };
        if !rules.rooms.on && !rules.model_checks {
            return Default::default();
        }
        for (k, entry) in self.models.iter().enumerate() {
            if checked.iter().any(|c| c.model == k) || entry.model.layers.iter().all(|l| l.points) {
                continue;
            }
            let name = entry.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let report = ValidationReport { schema: entry.model.format.to_string(), ids_title: String::new(), status: S::Passed, specifications: Vec::new(), model: None };
            checked.push(Checked { model: k, name, report: Ok(Arc::new(report)) });
        }
        checked.sort_by_key(|c| c.model);
        let routes = if rules.rooms.on { self.room_shape_checks(checked, &rules.rooms) } else { Default::default() };
        if rules.model_checks {
            for c in checked.iter_mut() {
                let Ok(report) = &mut c.report else { continue };
                let placed = Self::placed_elements(&model, c.model as u32);
                let finding = |p: &Placed, facet: &str, reason: String| {
                    let props = model.props.get(&p.id).map(String::as_str).unwrap_or("");
                    Finding {
                        express_id: model.element_key(p.id).source as u32,
                        global_id: field(props, "GlobalId").map(str::to_string),
                        entity_type: field(props, "IfcType").unwrap_or("").to_ascii_uppercase(),
                        facet: facet.into(),
                        status: S::Failed,
                        reason,
                        name: field(props, "Name").map(str::to_string),
                        storey: None,
                    }
                };
                let mut hang = result("GEO-LOOSE", "Elements that touch no other");
                let mut stray = result("GEO-STRAY", "Elements far from the model");
                if placed.is_empty() {
                    for r in [&mut hang, &mut stray] {
                        r.status = S::NotCheckable;
                        r.reasons.push("The model has no elements with a class and a shape".into());
                    }
                } else {
                    let away = strays(&placed, 250.0);
                    let alone = loose(&placed, 0.05);
                    for (r, found, facet, why) in [(&mut hang, &alone, "loose", "No other element comes within 50 mm of it"), (&mut stray, &away, "stray", "It lies far from where the rest of the model is")] {
                        r.applicable_count = placed.len();
                        r.failed_count = found.len();
                        r.passed_count = placed.len() - found.len();
                        // the first of them by name; the count says how many there are
                        r.findings = found.iter().take(500).map(|&k| finding(&placed[k], facet, why.to_string())).collect();
                        if !found.is_empty() {
                            r.status = S::Failed;
                        }
                    }
                }
                let report = Arc::make_mut(report);
                report.specifications.push(hang);
                report.specifications.push(stray);
            }
        }
        routes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(id: u64, lo: [f32; 3], hi: [f32; 3]) -> Placed {
        Placed { id, min: Vec3::from_array(lo), max: Vec3::from_array(hi) }
    }

    #[test]
    fn an_element_that_touches_nothing_hangs_loose() {
        let slab = at(1, [0.0, 0.0, 0.0], [10.0, 10.0, 0.2]);
        let column = at(2, [1.0, 1.0, 0.2], [1.3, 1.3, 3.0]);
        // a beam 40 mm above the column still joins it; a lamp a metre under the slab hangs loose
        let beam = at(3, [1.0, 1.0, 3.04], [8.0, 1.3, 3.4]);
        let lamp = at(4, [5.0, 5.0, -1.4], [5.2, 5.2, -1.0]);
        assert_eq!(loose(&[slab, column, beam, lamp], 0.05), [3]);
        assert!(loose(&[slab], 0.05) == [0]);
    }

    #[test]
    fn an_element_far_from_the_rest_is_a_stray() {
        let mut all: Vec<Placed> = (0..20).map(|k| at(k, [k as f32, 0.0, 0.0], [k as f32 + 1.0, 1.0, 3.0])).collect();
        assert!(strays(&all, 250.0).is_empty());
        all.push(at(99, [5000.0, 0.0, 0.0], [5001.0, 1.0, 1.0]));
        assert_eq!(strays(&all, 250.0), [20]);
    }
}
