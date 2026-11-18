//! The analysis models of a Tekla Structures model (its `ID<guid>.db6` files, as `tekla-read`
//! reads them) in the structural analysis model of `bim-structural`, in SI units and world
//! metres.
//!
//! What maps:
//!
//! - nodes with their supports: fixed, free or a spring per degree of freedom, and the support
//!   of a bar end supported on a node that has none of its own (in the bar end's axes where it
//!   has them);
//! - the bars of the analysis parts as members, with the part's profile as their section (the
//!   European steel profiles by name, rectangles `h*b`, plates, round bars) and its grade as
//!   their material (stiffness, density and expansion from the material catalogue), their end
//!   releases (free, spring, partial) and the eccentricity of their ends;
//! - the area elements of slabs and walls as plates and shells, their thickness from the profile;
//! - one load case per load group of the analysis model, the self weight case with its weight;
//! - the loads the analysis model holds: a point load on the node it stands on, else on the
//!   member it lies on; a line or temperature load on the members along its line; an area load
//!   as a load on its own outline;
//! - the load combinations by name (their factors are not in what the reader decodes).
//!
//! Springs are taken in Tekla's own units, N/mm and N·mm/rad. What a Tekla model holds besides
//! (rigid links, results, `KeepAxis`, the part's offsets from its axis) is not read; a partial
//! release, a combination without its factors and a load without a member to carry it are noted
//! in the model notes.

use bim_structural::*;
use glam::DVec3;
use std::collections::HashMap;
use tekla_read as tk;

/// Tekla is in millimetres.
const MM: f64 = 0.001;

/// The analysis models of a model, its grades looked up in its material catalogue.
pub(super) fn models(structural: &tk::Structural, catalogue: &[tk::Material], file: &str) -> Vec<AnalysisModel> {
    structural.analysis_models.iter().enumerate().map(|(k, m)| model(structural, catalogue, m, k as u64 + 1, file)).collect()
}

/// A bar between two nodes, in millimetres, for placing the loads.
struct Bar {
    id: Id,
    part: u32,
    a: DVec3,
    b: DVec3,
}

fn model(structural: &tk::Structural, catalogue: &[tk::Material], am: &tk::AnalysisModel, id: Id, file: &str) -> AnalysisModel {
    // the application, with the analysis method the model is set to
    let application = match am.analysis_method_name() {
        Some("FIRST_ORDER") => "Tekla Structures, first-order analysis",
        Some("SECOND_ORDER") => "Tekla Structures, second-order analysis",
        _ => "Tekla Structures",
    };
    let source = SourceInfo { format: "Tekla Structures analysis model".into(), file: file.to_string(), application: Some(application.into()), ..SourceInfo::default() };
    let name = if am.name.trim().is_empty() { format!("Analysis model {id}") } else { am.name.trim().to_string() };
    let mut model = AnalysisModel::new(id, name, source);
    let v = |p: [f64; 3]| DVec3::from_array(p);

    // nodes and their supports
    let mut nodes: HashMap<u32, (Id, DVec3)> = HashMap::new();
    for (k, n) in am.nodes.iter().enumerate() {
        let id = sam_id(IdKind::Node, k as u64 + 1);
        nodes.insert(n.id, (id, v(n.position_mm)));
        let support = support(&n.support, &n.support_springs);
        if n.support.iter().any(|c| *c > 2) {
            model.diagnostics.push(Diagnostic { object: Some(id), code: "support_not_read".into(), message: format!("support conditions {:?}: only free (0), fixed (1) and spring (2) are read", n.support) });
        }
        model.nodes.push(Node { id, number: n.number.to_string(), p: v(n.position_mm) * MM, frame: None, support, origin: NodeOrigin::Analytical });
    }

    // materials by grade, from the material catalogue
    // a member or area names its analysis part by the model's part it stands for (or by the
    // analysis part's own id)
    let mut parts: HashMap<u32, &tk::AnalysisPart> = am.parts.iter().map(|p| (p.id, p)).collect();
    parts.extend(am.parts.iter().map(|p| (p.part, p)));
    let mut materials: HashMap<String, Id> = HashMap::new();
    let mut material = |model: &mut AnalysisModel, grade: &str| -> Option<Id> {
        let grade = grade.trim();
        if grade.is_empty() {
            return None;
        }
        let next = sam_id(IdKind::Material, materials.len() as u64 + 1);
        Some(*materials.entry(grade.to_string()).or_insert_with(|| {
            let known = catalogue.iter().find(|m| m.name == grade || m.aliases.iter().any(|a| a == grade));
            let positive = |x: f64| (x.is_finite() && x > 0.0).then_some(x);
            model.materials.push(Material {
                id: next,
                name: grade.to_string(),
                e: known.and_then(|m| positive(m.modulus_of_elasticity)),
                g: None,
                nu: known.and_then(|m| positive(m.poissons_ratio)),
                density: known.and_then(|m| positive(m.profile_density).or(positive(m.plate_density))),
                alpha_t: known.and_then(|m| positive(m.thermal_dilatation)),
            });
            next
        }))
    };

    // sections by profile
    let mut sections: HashMap<String, Id> = HashMap::new();
    let mut bars: Vec<Bar> = Vec::new();
    for (k, m) in am.members.iter().enumerate() {
        let id = sam_id(IdKind::Member, k as u64 + 1);
        let (Some(&(a, pa)), Some(&(b, pb))) = (nodes.get(&m.start_node), nodes.get(&m.end_node)) else {
            model.diagnostics.push(Diagnostic { object: None, code: "member_without_ends".into(), message: format!("member {} has an end node that is not there", m.number) });
            continue;
        };
        let part = parts.get(&m.part).copied();
        let grade = part.and_then(|p| material(&mut model, &p.material));
        let section = part.map(|p| p.profile.trim()).filter(|p| !p.is_empty()).map(|profile| {
            let next = sam_id(IdKind::Section, sections.len() as u64 + 1);
            *sections.entry(profile.to_string()).or_insert_with(|| {
                let shape = shape(profile);
                let (props, props_source) = section::compute(&shape, None);
                model.sections.push(Section { id: next, name: profile.to_string(), shape, props, props_source, imported: None, material: grade });
                next
            })
        });
        bars.push(Bar { id, part: part.map_or(0, |p| p.part), a: pa, b: pb });
        let releases = part.map_or([None, None], |p| [p.start.as_ref(), p.end.as_ref()].map(|end| end.and_then(|e| release(e, id, &mut model.diagnostics))));
        model.members.push(Member {
            id,
            number: m.number.to_string(),
            kind: MemberKind::Beam,
            nodes: vec![a, b],
            geometry: Curve::Line,
            z_ref: frame::default_z_ref(pb - pa),
            orientation: Orientation::Default,
            section: section.map(|s| (s, None)),
            section_rotation: 0.0,
            insertion: insertion(part),
            releases,
            material: grade,
            links: part.map(|p| vec![u64::from(p.part)]).unwrap_or_default(),
        });
    }

    // the supports of a bar's supported ends where their node has none of its own
    for m in &am.members {
        let Some(part) = parts.get(&m.part) else { continue };
        for (node, end, held) in [(m.start_node, &part.start, part.start_support), (m.end_node, &part.end, part.end_support)] {
            let Some(&(id, _)) = nodes.get(&node) else { continue };
            let Some(n) = model.nodes.iter_mut().find(|n| n.id == id && n.support.is_none()) else { continue };
            match end {
                Some(end) if end.condition == 1 => {
                    n.support = support(&end.support, &end.support_springs);
                    // in the support's own axes where it has them
                    let (x, y) = (DVec3::from_array(end.support_axis_x).normalize_or_zero(), DVec3::from_array(end.support_axis_y).normalize_or_zero());
                    if let Some(s) = n.support.as_mut().filter(|_| x != DVec3::ZERO && y != DVec3::ZERO && (x != DVec3::X || y != DVec3::Y)) {
                        n.frame = Some(glam::DMat3::from_cols(x, y, x.cross(y)));
                        s.frame = DofFrame::NodeLocal;
                    }
                }
                Some(_) => {}
                None => n.support = held.and_then(|h| support(&h, &[0.0; 6])),
            }
        }
    }

    // slabs and walls
    for (k, area) in am.areas.iter().enumerate() {
        let id = sam_id(IdKind::Surface, k as u64 + 1);
        let boundary: Vec<Id> = area.nodes.iter().filter_map(|n| nodes.get(n).map(|x| x.0)).collect();
        if boundary.len() < 3 || boundary.len() != area.nodes.len() {
            model.diagnostics.push(Diagnostic { object: None, code: "surface_without_nodes".into(), message: format!("area element {} has nodes that are not there", area.id) });
            continue;
        }
        let part = parts.get(&area.part).copied();
        let grade = part.and_then(|p| material(&mut model, &p.material));
        let t = part.and_then(|p| thickness(&p.profile));
        model.surfaces.push(SurfaceMember {
            id,
            number: (k + 1).to_string(),
            kind: if part.is_some_and(|p| p.analysis_class == 4) { SurfaceKind::Plate } else { SurfaceKind::Shell },
            boundary,
            openings: Vec::new(),
            thickness: t.map_or(Thickness::Unknown, Thickness::Uniform),
            offset_z: 0.0,
            x_dir: None,
            material: grade,
            links: part.map(|p| vec![u64::from(p.part)]).unwrap_or_default(),
        });
    }

    // a load case per load group of the analysis
    let mut cases: HashMap<u32, Id> = HashMap::new();
    for (k, g) in am.load_groups.iter().enumerate() {
        let id = sam_id(IdKind::LoadCase, k as u64 + 1);
        cases.insert(g.id, id);
        let own_weight = g.group_type == 5000;
        model.load_cases.push(LoadCase {
            id,
            number: (k + 1).to_string(),
            name: g.name.clone(),
            action_type: match g.group_type {
                5000 | 1002 => ActionType::Permanent,
                1004 => ActionType::Variable,
                other => ActionType::User(other.to_string()),
            },
            action_source: g.group_type_name().map_or_else(|| g.group_type.to_string(), str::to_string),
            self_weight: own_weight.then_some(DVec3::NEG_Z),
            coefficient: 1.0,
        });
    }

    // the loads, each in its group's case: as the analysis model lists them, else (a model that
    // lists none) every load in the case of its load group
    let tolerance = am.node_position_tolerance_mm.max(1.0);
    let listed: Vec<(u32, u32)> = if am.loads.is_empty() {
        structural.loads.iter().filter_map(|l| Some((l.id, am.load_groups.iter().find(|g| g.load_group.is_some() && g.load_group == l.group)?.id))).collect()
    } else {
        am.loads.clone()
    };
    for &(load_id, group) in &listed {
        let (Some(load), Some(&case)) = (structural.loads.iter().find(|l| l.id == load_id), cases.get(&group)) else { continue };
        let placed = place(load, &nodes, &bars, tolerance);
        if placed.is_empty() {
            model.diagnostics.push(Diagnostic { object: None, code: "load_not_placed".into(), message: format!("{} {}: no node or member of the analysis model carries it", load.kind.as_deref().unwrap_or("load"), load.id) });
        }
        for (target, kind) in placed {
            let id = sam_id(IdKind::Action, model.actions.len() as u64 + 1);
            model.actions.push(Action { id, load: LoadRef::Case(case), target, frame: LoadFrame::Global, kind, destabilising: false });
        }
        if let Some(t) = load.temperatures.filter(|t| t[3] != 0.0) {
            model.diagnostics.push(Diagnostic { object: None, code: "load_not_read".into(), message: format!("temperature load {}: its initial axial elongation ({}) is not read", load.id, t[3]) });
        }
    }

    // the combinations by name: what they combine is not read
    for (k, c) in am.combinations.iter().enumerate() {
        let id = sam_id(IdKind::Combination, k as u64 + 1);
        model.combinations.push(Combination { id, number: (k + 1).to_string(), name: c.name.clone(), purpose: Purpose::User(format!("Tekla code {}", c.code)), terms: Vec::new() });
        model.diagnostics.push(Diagnostic { object: Some(id), code: "combination_without_factors".into(), message: format!("load combination {}: its load groups and factors are not read", c.name) });
    }

    let duplicates = model.rebuild_index();
    debug_assert!(duplicates.is_empty(), "{duplicates:?}");
    model
}

/// A spring constant of degree of freedom `k` (Ux Uy Uz Rx Ry Rz) in SI units: Tekla's N/mm is
/// 1000 N/m, its N·mm/rad 0.001 N·m/rad.
fn spring(k: usize, value: f64) -> f64 {
    if k < 3 { value * 1000.0 } else { value / 1000.0 }
}

/// Six support conditions (0 free, 1 fixed, 2 spring) with their springs; `None` when all are
/// free.
fn support(codes: &[u32; 6], springs: &[f64; 6]) -> Option<Support> {
    let dofs: Dofs = std::array::from_fn(|k| match codes[k] {
        1 => Dof::Fixed,
        2 => Dof::Spring(spring(k, springs[k])),
        _ => Dof::Free,
    });
    if dofs.iter().all(|d| *d == Dof::Free) {
        return None;
    }
    let preset = support::classify(&dofs, Dimension::Space3d);
    Some(Support { dofs, frame: DofFrame::Global, named: None, preset })
}

/// The release of a bar end (in the member's axes): free, a spring, or a partial release (taken
/// as free, and noted); `None` when it is connected in all six.
fn release(end: &tk::AnalysisBarEnd, member: Id, notes: &mut Vec<Diagnostic>) -> Option<Hinge> {
    let mut partial = Vec::new();
    let dofs: Dofs = std::array::from_fn(|k| match end.connectivity[k] {
        1 => Dof::Free,
        2 => Dof::Spring(spring(k, end.connectivity_springs[k])),
        3 => {
            partial.push(format!("{} {}", ["ux", "uy", "uz", "rx", "ry", "rz"][k], end.connectivity_springs[k]));
            Dof::Free
        }
        _ => Dof::Fixed,
    });
    if !partial.is_empty() {
        notes.push(Diagnostic { object: Some(member), code: "partial_release".into(), message: format!("partial release ({}): taken as released", partial.join(", ")) });
    }
    (!dofs.iter().all(|d| *d == Dof::Fixed)).then(|| Hinge { dofs, named: (!partial.is_empty()).then(|| format!("partial release {}", partial.join(", "))) })
}

/// The eccentricity of a bar's ends, as rigid offsets from their nodes in the world's axes.
fn insertion(part: Option<&tk::AnalysisPart>) -> Insertion {
    let offset = |end: &Option<tk::AnalysisBarEnd>| end.as_ref().map_or(DVec3::ZERO, |e| DVec3::from_array(e.eccentricity_mm) / 1000.0);
    let ends = part.map_or([DVec3::ZERO; 2], |p| [offset(&p.start), offset(&p.end)]);
    if ends == [DVec3::ZERO; 2] {
        return Insertion::default();
    }
    Insertion { end_offsets: ends, end_offset_frame: OffsetFrame::Global, ..Insertion::default() }
}

/// The numbers of a profile name after its letters, split at `*`, `X` or `x`: "400*600" is
/// [400, 600], "PL20*200" [20, 200], "HEA200" [200].
fn numbers(profile: &str) -> Vec<f64> {
    let Some(start) = profile.find(|c: char| c.is_ascii_digit()) else { return Vec::new() };
    profile[start..]
        .split(['*', 'X', 'x'])
        .map_while(|s| {
            let s = s.trim();
            let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
            s[..end].parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
        })
        .collect()
}

/// The section shape of a Tekla profile name, in metres: a European steel profile of the steel
/// table ("HEA200", "IPE300", "L80*8", "SHS100*5", "CHS114.3*5"), a rectangle height × width
/// ("400*600"), a plate thickness × width ("PL20*200"), a round bar ("D20", "ROD20"); else
/// unknown, kept by name.
pub(super) fn shape(profile: &str) -> Shape {
    let profile = profile.trim();
    let letters: String = profile.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_uppercase();
    let n = numbers(profile);
    let mm = |v: f64| v * MM;
    let size = |v: f64| if v.fract() == 0.0 { format!("{v}") } else { format!("{v:.1}") };
    // the steel table names a profile "HEA 200", "L 80x80x8", "SHS 100x100x5", "CHS 114.3x5"
    let series = match letters.as_str() {
        // "HE200A" is the HEA 200
        "HE" => profile.chars().last().filter(|c| matches!(c, 'A' | 'B' | 'M')).map(|c| format!("HE{c}")),
        "UNP" => Some("UPN".to_string()),
        "" => None,
        other => Some(other.to_string()),
    };
    if let Some(series) = &series {
        let names: Vec<String> = match (series.as_str(), &n[..]) {
            (_, [h]) => vec![format!("{series} {}", size(*h))],
            ("L" | "SHS", [a, t]) => vec![format!("{series} {}x{}x{}", size(*a), size(*a), size(*t))],
            (_, [a, b]) => vec![format!("{series} {}x{}", size(*a), size(*b))],
            (_, [a, b, t]) => vec![format!("{series} {}x{}x{}", size(*a), size(*b), size(*t))],
            _ => Vec::new(),
        };
        if let Some(p) = names.iter().find_map(|name| bim_library::profile::steel(name)) {
            return p.shape.structural();
        }
    }
    match (letters.as_str(), &n[..]) {
        ("", [h, b]) => Shape::Rect { b: mm(*b), h: mm(*h) },
        // a plate: the larger number is its height, whichever comes first
        ("PL" | "PLT" | "FL" | "BL", [a, b]) => Shape::Rect { b: mm(a.min(*b)), h: mm(a.max(*b)) },
        ("D" | "ROD" | "RD", [d]) => Shape::Circle { d: mm(*d) },
        ("CHS" | "PD" | "TUBE", [d, t]) => Shape::Chs { d: mm(*d), t: mm(*t) },
        _ => Shape::Unknown,
    }
}

/// The thickness of a slab or wall from its profile ("200", "200*1000"): its first number, m.
fn thickness(profile: &str) -> Option<f64> {
    numbers(profile.trim()).first().map(|t| t * MM)
}

/// Where a load acts in the analysis model and how: a point load on the node at it, else on the
/// nearest member it lies on (within its box); a line or temperature load on each member along
/// its line; an area load on its outline. Empty when nothing carries it.
fn place(load: &tk::Load, nodes: &HashMap<u32, (Id, DVec3)>, bars: &[Bar], tolerance: f64) -> Vec<(Target, Load)> {
    let v = |p: [f64; 3]| DVec3::from_array(p);
    let force = |k: usize| load.forces.get(k).map_or(DVec3::ZERO, |f| v(*f));
    let positions: Vec<DVec3> = load.positions_mm.iter().map(|p| v(*p)).collect();
    match load.kind_code {
        // a point load: N and N·mm
        50 => {
            let Some(&p) = positions.first() else { return Vec::new() };
            let (f, m) = (force(0), load.moment.map_or(DVec3::ZERO, |m| v(m) * MM));
            if let Some((id, _)) = nodes.values().filter(|(_, q)| q.distance(p) <= tolerance).min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p))) {
                return vec![(Target::Node(*id), Load::NodalForce { f, m })];
            }
            // the member it lies on, else the nearest within its box; one of its own part first
            let reach = load.bounding_box_mm.iter().copied().fold(tolerance, f64::max);
            let nearest = bars
                .iter()
                .filter_map(|bar| {
                    let (t, d) = along(bar, p);
                    (d <= reach).then_some((bar, t, d, Some(bar.part) != load.attached_to))
                })
                .min_by(|a, b| (a.3, a.2).partial_cmp(&(b.3, b.2)).unwrap_or(std::cmp::Ordering::Equal));
            nearest.map(|(bar, t, _, _)| (Target::Member { id: bar.id, span: None }, Load::MemberPoint { x: t * MM, f, m })).into_iter().collect()
        }
        // a line load, N/mm, from its start plus distance A to its end less distance B; a
        // temperature load along its line
        51 | 72 => {
            let (Some(&a), Some(&b)) = (positions.first(), positions.get(1)) else { return Vec::new() };
            let length = a.distance(b);
            if length <= tolerance {
                return Vec::new();
            }
            let [from, to] = if load.kind_code == 51 { load.distances_mm.map_or([0.0, length], |d| [d[0].max(0.0), length - d[1].max(0.0)]) } else { [0.0, length] };
            let (p1, p2) = (force(0), if load.forces.len() > 1 && load.form_code != Some(1001) { force(1) } else { force(0) });
            let dir = if p1.length() > 0.0 { p1 } else { p2 }.normalize_or_zero();
            let q = |s: f64| {
                let w = if to > from { ((s - from) / (to - from)).clamp(0.0, 1.0) } else { 0.0 };
                p1.lerp(p2, w).dot(dir) / MM
            };
            let u = (b - a) / length;
            let mut out = Vec::new();
            for bar in bars {
                // a member along the load's line: both its ends on it
                let off = |p: DVec3| (p - a - u * (p - a).dot(u)).length();
                if off(bar.a) > tolerance || off(bar.b) > tolerance {
                    continue;
                }
                let (sa, sb) = ((bar.a - a).dot(u), (bar.b - a).dot(u));
                let (lo, hi) = (sa.min(sb).max(from), sa.max(sb).min(to));
                if hi - lo <= tolerance {
                    continue;
                }
                let span = (bar.b - bar.a).length() * MM;
                // the member's own x of a point of the load's line
                let x = |s: f64| ((s - sa).abs() * MM).clamp(0.0, span);
                let (mut x1, mut x2, mut q1, mut q2) = (x(lo), x(hi), q(lo), q(hi));
                if x1 > x2 {
                    (x1, x2, q1, q2) = (x2, x1, q2, q1);
                }
                let whole = x1 <= tolerance * MM && x2 >= span - tolerance * MM;
                if load.kind_code == 72 {
                    // the change along, top to bottom (local z), side to side (local y)
                    let t = load.temperatures.unwrap_or_default();
                    out.push((Target::Member { id: bar.id, span: (!whole).then_some((x1, x2)) }, Load::Temperature { dt: t[0], dt_y: t[2], dt_z: t[1] }));
                    continue;
                }
                if dir == DVec3::ZERO {
                    continue;
                }
                let dist = if whole && (q1 - q2).abs() < 1e-9 {
                    Distribution::Uniform(q1)
                } else if whole {
                    Distribution::Linear { q1, q2 }
                } else {
                    Distribution::Trapezoidal { a: x1, b: x2, q1, q2 }
                };
                out.push((Target::Member { id: bar.id, span: None }, Load::MemberDistributed { dir, moment: false, dist }));
            }
            out
        }
        // an area load, N/mm², on its parallelogram (or its triangle)
        52 => {
            if positions.len() < 3 {
                return Vec::new();
            }
            let mut corners = positions[..3].to_vec();
            if load.form_name() == Some("LOAD_FORM_AREA_PARALLELOGRAM") {
                corners.push(positions[0] + positions[2] - positions[1]);
            }
            let dir = (0..3).map(force).find(|f| f.length() > 0.0).unwrap_or_default().normalize_or_zero();
            if dir == DVec3::ZERO {
                return Vec::new();
            }
            let q: [f64; 3] = std::array::from_fn(|k| force(k).dot(dir) / (MM * MM));
            let dist = if q.iter().all(|x| (x - q[0]).abs() < 1e-9) { SurfaceDistribution::Uniform(q[0]) } else { SurfaceDistribution::Bilinear(q) };
            vec![(Target::Free(corners.into_iter().map(|p| p * MM).collect()), Load::Surface { dir, dist })]
        }
        _ => Vec::new(),
    }
}

/// How far along a bar (mm from its start) the foot of `p` lies, and how far `p` is from it.
fn along(bar: &Bar, p: DVec3) -> (f64, f64) {
    let axis = bar.b - bar.a;
    let length = axis.length();
    if length <= 0.0 {
        return (0.0, p.distance(bar.a));
    }
    let t = ((p - bar.a).dot(axis) / length).clamp(0.0, length);
    (t, p.distance(bar.a + axis / length * t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, number: u32, p: [f64; 3], fixed: bool) -> tk::AnalysisNode {
        tk::AnalysisNode { id, number, position_mm: p, support: [u32::from(fixed); 6], support_springs: [0.0; 6], user_support: false }
    }

    fn part(id: u32, part: u32, name: &str, profile: &str, material: &str, class: u32) -> tk::AnalysisPart {
        tk::AnalysisPart {
            id,
            part,
            name: name.into(),
            profile: profile.into(),
            material: material.into(),
            analysis_class: class,
            analysis_type: if class == 4 { 6 } else { 8 },
            builtup_mode: 0,
            axis_location: 1,
            self_weight: true,
            part_names_include: true,
            snap_distance_mm: -1.0,
            area_element_size_mm: [1000.0; 3],
            minimum_area_hole_size_mm: 300.0,
            start_support: None,
            end_support: None,
            start: None,
            end: None,
            keep_axis: 2,
        }
    }

    fn load(id: u32, kind_code: u32, positions_mm: Vec<[f64; 3]>, forces: Vec<[f64; 3]>) -> tk::Load {
        tk::Load {
            id,
            guid: None,
            kind_code,
            kind: tk::load_kind_name(kind_code).map(str::to_string),
            group: None,
            positions_mm,
            forces,
            moment: None,
            torsions: None,
            distances_mm: None,
            form_code: None,
            temperatures: None,
            spanning_double: true,
            automatic_primary_axis_weight: true,
            weight: 0.5,
            load_dispersion_angle: 0.0,
            spanning_values: Vec::new(),
            bounding_box_mm: [1000.0; 3],
            attached_to: None,
            part_filter: String::new(),
            primary_axis_direction: [1.0, 0.0, 0.0],
            create_fixed_supports: false,
            part_names_include: true,
        }
    }

    fn grade(name: &str, e: f64, density: f64) -> tk::Material {
        tk::Material { name: name.into(), kind: 1, aliases: Vec::new(), profile_density: density, plate_density: density, modulus_of_elasticity: e, poissons_ratio: 0.3, thermal_dilatation: 1.2e-5, flags: 0, properties: Vec::new() }
    }

    /// Two columns, a beam between their heads, a slab over them and a precast beam, as the
    /// analysis model of a Tekla model holds them, with a load of each kind.
    fn frame() -> tk::Structural {
        let am = tk::AnalysisModel {
            id: 8196,
            name: "AM_Frame".into(),
            node_position_tolerance_mm: 10.0,
            nodes: vec![
                node(8316, 1, [0.0, 0.0, 0.0], true),
                node(8326, 2, [0.0, 0.0, 3000.0], false),
                node(8384, 3, [6000.0, 0.0, 0.0], true),
                node(8394, 4, [6000.0, 0.0, 3000.0], false),
                node(8502, 5, [-1000.0, 4000.0, 3000.0], false),
                node(8513, 6, [-1000.0, -1000.0, 3000.0], false),
                node(8524, 7, [7000.0, -1000.0, 3000.0], false),
                node(8534, 8, [7000.0, 4000.0, 3000.0], false),
                node(8596, 9, [0.0, 8000.0, 0.0], false),
                node(8606, 10, [8000.0, 8000.0, 0.0], false),
            ],
            parts: vec![
                part(8279, 7754, "COLUMN", "HEA200", "S235JR", 1),
                part(8266, 7790, "COLUMN", "HEA200", "S235JR", 1),
                part(8256, 7815, "BEAM", "IPE300", "S235JR", 2),
                part(8243, 7857, "SLAB", "200", "C30/37", 4),
                part(8224, 7890, "PRECAST_BEAM", "400*600", "C45/55", 2),
            ],
            members: vec![
                tk::AnalysisMember { id: 1, part: 7754, number: 1, start_node: 8316, end_node: 8326 },
                tk::AnalysisMember { id: 2, part: 7790, number: 2, start_node: 8384, end_node: 8394 },
                tk::AnalysisMember { id: 3, part: 7815, number: 3, start_node: 8326, end_node: 8394 },
                tk::AnalysisMember { id: 4, part: 7890, number: 4, start_node: 8596, end_node: 8606 },
            ],
            areas: vec![tk::AnalysisArea { id: 5, part: 7857, nodes: vec![8502, 8513, 8524, 8534] }],
            load_groups: vec![
                tk::AnalysisLoadGroup { id: 8184, load_group: None, name: "albl_SelfWeight".into(), group_type: 5000, direction: 3 },
                tk::AnalysisLoadGroup { id: 7956, load_group: Some(7956), name: "LG_Variable".into(), group_type: 1004, direction: 6 },
                tk::AnalysisLoadGroup { id: 7947, load_group: Some(7947), name: "LG_Permanent".into(), group_type: 1002, direction: 3 },
            ],
            loads: vec![(7963, 7956), (7985, 7956), (8002, 7947), (8020, 7947), (9000, 7947)],
            ..tk::AnalysisModel::default()
        };
        let mut point = load(7963, 50, vec![[3000.0, 0.0, 3000.0]], vec![[0.0, 0.0, -10_000.0]]);
        point.moment = Some([0.0, 500_000.0, 0.0]);
        point.attached_to = Some(7815);
        let mut line = load(7985, 51, vec![[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]], vec![[0.0, 0.0, -5.0], [0.0, 0.0, -8.0]]);
        line.distances_mm = Some([250.0, 500.0]);
        line.form_code = Some(1002);
        let mut area = load(8002, 52, vec![[-1000.0, -1000.0, 3200.0], [7000.0, -1000.0, 3200.0], [7000.0, 4000.0, 3200.0]], vec![[0.0, 0.0, -0.002], [0.0, 0.0, -0.003], [0.0, 0.0, -0.004]]);
        area.form_code = Some(1002);
        let mut temperature = load(8020, 72, vec![[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]], Vec::new());
        temperature.temperatures = Some([25.0, 10.0, 5.0, 1.5]);
        // a point load far from every member
        let lost = load(9000, 50, vec![[50_000.0, 0.0, 0.0]], vec![[0.0, 0.0, -1.0]]);
        tk::Structural { loads: vec![point, line, area, temperature, lost], strands: Vec::new(), analysis_models: vec![am] }
    }

    fn catalogue() -> Vec<tk::Material> {
        vec![grade("S235JR", 2.1e11, 7850.0), grade("C30/37", 3.3e10, 2500.0)]
    }

    #[test]
    fn an_analysis_model_has_its_nodes_supports_members_and_slab_in_metres() {
        let models = models(&frame(), &catalogue(), "frame");
        assert_eq!(models.len(), 1);
        let m = &models[0];
        assert_eq!((m.name.as_str(), m.nodes.len(), m.members.len(), m.surfaces.len()), ("AM_Frame", 10, 4, 1));
        let node = |number: &str| m.nodes.iter().find(|n| n.number == number).unwrap();
        assert_eq!(node("4").p, DVec3::new(6.0, 0.0, 3.0));
        assert_eq!(node("1").support.as_ref().map(|s| s.preset), Some(SupportPreset::Fixed));
        assert!(node("2").support.is_none());
        // a column from its foot up, a beam between the heads
        let ends = |number: &str| m.members.iter().find(|x| x.number == number).and_then(|x| m.member_ends(x)).unwrap();
        assert_eq!(ends("1"), (DVec3::ZERO, DVec3::new(0.0, 0.0, 3.0)));
        assert_eq!(ends("3"), (DVec3::new(0.0, 0.0, 3.0), DVec3::new(6.0, 0.0, 3.0)));
        // sections by profile, materials by grade
        let section = |number: &str| m.members.iter().find(|x| x.number == number).and_then(|x| m.section(x.section?.0)).unwrap();
        assert_eq!(section("1").name, "HEA200");
        assert!(matches!(section("1").shape, Shape::I { h, b, .. } if (h - 0.19).abs() < 1e-9 && (b - 0.2).abs() < 1e-9));
        assert_eq!(section("4").shape, Shape::Rect { b: 0.6, h: 0.4 });
        assert_eq!(m.sections.len(), 3);
        let steel = m.material(m.members[0].material.unwrap()).unwrap();
        assert_eq!((steel.name.as_str(), steel.e, steel.density), ("S235JR", Some(2.1e11), Some(7850.0)));
        // a grade the catalogue lacks keeps its name
        let precast = m.material(m.members[3].material.unwrap()).unwrap();
        assert_eq!((precast.name.as_str(), precast.e), ("C45/55", None));
        let slab = &m.surfaces[0];
        assert_eq!((slab.kind, &slab.thickness, slab.boundary.len()), (SurfaceKind::Plate, &Thickness::Uniform(0.2), 4));
        assert_eq!(m.members[2].links, [7815]);
    }

    #[test]
    fn each_load_group_is_a_load_case_with_its_loads() {
        let m = &models(&frame(), &catalogue(), "frame")[0];
        let cases: Vec<(&str, &ActionType, bool)> = m.load_cases.iter().map(|c| (c.name.as_str(), &c.action_type, c.self_weight.is_some())).collect();
        assert_eq!(cases, [("albl_SelfWeight", &ActionType::Permanent, true), ("LG_Variable", &ActionType::Variable, false), ("LG_Permanent", &ActionType::Permanent, false)]);
        let beam = m.members[2].id;
        assert_eq!(m.actions.len(), 4);
        let case = |k: usize| LoadRef::Case(m.load_cases[k].id);
        // the point load in the middle of the beam, N and N·m
        let a = &m.actions[0];
        assert_eq!((a.load, &a.target), (case(1), &Target::Member { id: beam, span: None }));
        assert_eq!(a.kind, Load::MemberPoint { x: 3.0, f: DVec3::new(0.0, 0.0, -10_000.0), m: DVec3::new(0.0, 500.0, 0.0) });
        // the line load from 0.25 m to 5.5 m, 5 to 8 kN/m downwards
        let a = &m.actions[1];
        assert_eq!(a.target, Target::Member { id: beam, span: None });
        let Load::MemberDistributed { dir, dist: Distribution::Trapezoidal { a: x1, b: x2, q1, q2 }, .. } = a.kind else { panic!("{:?}", a.kind) };
        assert_eq!(dir, DVec3::NEG_Z);
        assert!((x1 - 0.25).abs() < 1e-9 && (x2 - 5.5).abs() < 1e-9 && (q1 - 5000.0).abs() < 1e-6 && (q2 - 8000.0).abs() < 1e-6, "{:?}", a.kind);
        // the area load on its parallelogram, 2 to 4 kN/m² at its first three corners
        let a = &m.actions[2];
        assert_eq!(a.load, case(2));
        let Target::Free(corners) = &a.target else { panic!("{:?}", a.target) };
        assert_eq!(corners.len(), 4);
        assert!((corners[3] - DVec3::new(-1.0, 4.0, 3.2)).length() < 1e-9);
        let Load::Surface { dir, dist: SurfaceDistribution::Bilinear(q) } = &a.kind else { panic!("{:?}", a.kind) };
        assert_eq!(*dir, DVec3::NEG_Z);
        assert!(q.iter().zip([2000.0, 3000.0, 4000.0]).all(|(a, b)| (a - b).abs() < 1e-6), "{q:?}");
        // the temperature load on the beam
        let a = &m.actions[3];
        assert_eq!((&a.target, &a.kind), (&Target::Member { id: beam, span: None }, &Load::Temperature { dt: 25.0, dt_y: 5.0, dt_z: 10.0 }));
        // the load nothing carries, and the elongation that is not read, are noted
        assert!(m.diagnostics.iter().any(|d| d.code == "load_not_placed" && d.message.contains("9000")), "{:?}", m.diagnostics);
        assert!(m.diagnostics.iter().any(|d| d.code == "load_not_read" && d.message.contains("8020")));
    }

    #[test]
    fn a_point_load_on_a_node_acts_on_the_node() {
        let mut s = frame();
        s.loads[0].positions_mm = vec![[6000.0, 0.0, 3005.0]];
        let m = &models(&s, &[], "frame")[0];
        let node = m.nodes.iter().find(|n| n.number == "4").unwrap().id;
        assert_eq!(m.actions[0].target, Target::Node(node));
        assert!(matches!(m.actions[0].kind, Load::NodalForce { f, .. } if f == DVec3::new(0.0, 0.0, -10_000.0)));
    }

    #[test]
    fn a_model_that_lists_no_loads_takes_them_by_their_load_group() {
        let mut s = frame();
        s.analysis_models[0].loads.clear();
        s.loads[0].group = Some(7956);
        s.loads[1].group = Some(4444);
        let m = &models(&s, &[], "frame")[0];
        assert_eq!(m.actions.len(), 1);
        assert_eq!(m.actions[0].load, LoadRef::Case(m.load_cases[1].id));
    }

    /// A bar end as Tekla's dialog leaves it: connected in all six, no support.
    fn bar_end() -> tk::AnalysisBarEnd {
        tk::AnalysisBarEnd::default()
    }

    #[test]
    fn releases_springs_eccentricity_and_combinations_are_read() {
        let mut s = frame();
        let am = &mut s.analysis_models[0];
        // node 1: a support of its own with a spring about x of 777 N·mm/rad
        am.nodes[0].support = [1, 1, 1, 2, 0, 0];
        am.nodes[0].support_springs = [0.0, 0.0, 0.0, 777.0, 0.0, 0.0];
        am.nodes[0].user_support = true;
        // the beam: released at its start about x, a spring about y, partly about z
        am.parts[2].start = Some(tk::AnalysisBarEnd { connectivity: [0, 0, 0, 1, 2, 3], connectivity_springs: [0.0, 0.0, 0.0, 0.0, 1234.5, 0.5], ..bar_end() });
        am.parts[2].end = Some(bar_end());
        // the precast beam: its end supported on node 10 with springs, eccentric by (10, 20, 30)
        am.parts[4].end = Some(tk::AnalysisBarEnd { condition: 1, connectivity: [0, 0, 0, 0, 1, 1], support: [1, 1, 2, 0, 2, 0], support_springs: [0.0, 0.0, 5000.0, 0.0, 250.0, 0.0], eccentricity_mm: [10.0, 20.0, 30.0], ..bar_end() });
        am.combinations = vec![tk::AnalysisLoadCombination { id: 1, name: "LC_ULS".into(), code: 2 }];
        let m = &models(&s, &[], "frame")[0];
        let node = |number: &str| m.nodes.iter().find(|n| n.number == number).unwrap();
        let held = &node("1").support.as_ref().unwrap().dofs;
        assert_eq!(held[..4], [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Spring(0.777)]);
        // the supported end's support on its node: 5000 N/mm is 5 MN/m
        let s10 = node("10").support.as_ref().unwrap();
        assert_eq!(s10.dofs, [Dof::Fixed, Dof::Fixed, Dof::Spring(5.0e6), Dof::Free, Dof::Spring(0.25), Dof::Free]);
        assert_eq!(s10.frame, DofFrame::Global);
        let beam = &m.members[2];
        let start = beam.releases[0].as_ref().unwrap();
        assert_eq!(start.dofs, [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Free, Dof::Spring(1.2345), Dof::Free]);
        assert!(start.named.as_deref().is_some_and(|n| n.contains("rz 0.5")));
        assert!(beam.releases[1].is_none());
        assert!(m.diagnostics.iter().any(|d| d.code == "partial_release" && d.object == Some(beam.id)));
        let precast = &m.members[3];
        assert_eq!(precast.releases[1].as_ref().map(|h| hinge::classify(&h.dofs)), Some(hinge::HingePreset::Hinge));
        assert_eq!(precast.insertion.end_offsets[1], DVec3::new(0.01, 0.02, 0.03));
        assert_eq!(precast.insertion.end_offset_frame, OffsetFrame::Global);
        assert_eq!(m.members[0].insertion, Insertion::default());
        assert_eq!((m.combinations.len(), m.combinations[0].name.as_str(), m.combinations[0].terms.len()), (1, "LC_ULS", 0));
        assert!(m.diagnostics.iter().any(|d| d.code == "combination_without_factors"));
    }

    /// allestest_r7 with its edited analysis model: set TEKLA_ANALYSIS_R7 to a folder holding
    /// allestest_r7.db1/.db2 and the `ID<GUID>.db6` files of AM_AllesTest (after the edit) and
    /// AM_Second.
    #[test]
    fn the_releases_and_springs_of_allestest_r7_are_read() {
        let Some(folder) = std::env::var_os("TEKLA_ANALYSIS_R7") else {
            eprintln!("skipped: set TEKLA_ANALYSIS_R7 to the allestest_r7 model folder");
            return;
        };
        let model = crate::loaders::load(std::path::Path::new(&folder)).unwrap();
        let m = model.structural.iter().map(|p| &p.doc).find(|m| m.name == "AM_AllesTest").expect("AM_AllesTest");
        let node = |number: &str| m.nodes.iter().find(|n| n.number == number).unwrap();
        let dofs = |number: &str| node(number).support.as_ref().map(|s| s.dofs.clone());
        assert_eq!(dofs("1"), Some([Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Spring(0.777), Dof::Free, Dof::Free]));
        assert_eq!(dofs("10"), Some([Dof::Fixed, Dof::Fixed, Dof::Spring(5.0e6), Dof::Free, Dof::Spring(0.25), Dof::Free]));
        assert_eq!(dofs("3").map(|d| d.iter().all(|x| *x == Dof::Fixed)), Some(true));
        let section = |x: &Member| m.section(x.section.unwrap().0).unwrap().name.clone();
        let beam = m.members.iter().find(|x| section(x) == "IPE300").unwrap();
        assert_eq!(beam.releases[0].as_ref().unwrap().dofs[..5], [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Free, Dof::Spring(1.2345)]);
        assert_eq!(beam.releases[1].as_ref().map(|h| hinge::classify(&h.dofs)), Some(hinge::HingePreset::Hinge));
        let precast = m.members.iter().find(|x| section(x) == "400*600").unwrap();
        assert!((precast.insertion.end_offsets[1] - DVec3::new(0.01, 0.02, 0.03)).length() < 1e-12, "{:?}", precast.insertion);
        let columns: Vec<&Member> = m.members.iter().filter(|x| section(x) == "HEA200").collect();
        assert!(columns.len() == 2 && columns.iter().all(|c| c.releases == [None, None]));
        assert_eq!(m.source.application.as_deref(), Some("Tekla Structures, first-order analysis"));
        for d in &m.diagnostics {
            eprintln!("note {}: {}", d.code, d.message);
        }
        eprintln!("combinations {:?}", m.combinations.iter().map(|c| &c.name).collect::<Vec<_>>());
    }

    #[test]
    fn tekla_profile_names_give_their_shapes() {
        let i = |name: &str| match shape(name) {
            Shape::I { h, b, .. } => Some(((h * 1000.0).round(), (b * 1000.0).round())),
            _ => None,
        };
        assert_eq!((i("HEA200"), i("HE200A"), i("IPE300"), i("HEB300")), (Some((190.0, 200.0)), Some((190.0, 200.0)), Some((300.0, 150.0)), Some((300.0, 300.0))));
        assert_eq!(shape("400*600"), Shape::Rect { b: 0.6, h: 0.4 });
        assert_eq!(shape("PL20*200"), Shape::Rect { b: 0.02, h: 0.2 });
        assert_eq!(shape("D20"), Shape::Circle { d: 0.02 });
        assert!(matches!(shape("L80*8"), Shape::L { .. }));
        assert!(matches!(shape("CHS114.3*5"), Shape::Chs { .. }));
        assert_eq!(shape("HI400-15-20*300"), Shape::Unknown);
    }

    /// The test model O3V_AllesTest: set TEKLA_ANALYSIS_FOLDER to its model folder.
    #[test]
    fn a_tekla_model_folder_opens_with_its_analysis_model() {
        let Some(folder) = std::env::var_os("TEKLA_ANALYSIS_FOLDER") else {
            eprintln!("skipped: set TEKLA_ANALYSIS_FOLDER to the model folder of O3V_AllesTest");
            return;
        };
        let model = crate::loaders::load(std::path::Path::new(&folder)).unwrap();
        assert_eq!(model.structural.len(), 1);
        let m = &model.structural[0].doc;
        assert_eq!((m.name.as_str(), m.nodes.len(), m.members.len(), m.surfaces.len(), m.load_cases.len()), ("AM_AllesTest", 10, 4, 1, 4));
        let node = |number: &str| m.nodes.iter().find(|n| n.number == number).unwrap();
        let fixed = |number: &str| node(number).support.as_ref().map(|s| s.preset) == Some(SupportPreset::Fixed);
        for (number, p) in [("1", [0.0, 0.0, 0.0]), ("2", [0.0, 0.0, 3.0]), ("3", [6.0, 0.0, 0.0]), ("5", [-1.0, 4.0, 3.0]), ("10", [8.0, 8.0, 0.0])] {
            assert!((node(number).p - DVec3::from_array(p)).length() < 1e-6, "node {number}: {:?}", node(number).p);
        }
        assert!(fixed("1") && fixed("3") && (4..=10).all(|k| node(&k.to_string()).support.is_none()) && node("2").support.is_none());
        let mut profiles: Vec<(String, String)> = m.members.iter().map(|x| (m.section(x.section.unwrap().0).unwrap().name.clone(), m.material(x.material.unwrap()).unwrap().name.clone())).collect();
        profiles.sort();
        assert_eq!(profiles, [("400*600", "C45/55"), ("HEA200", "S235JR"), ("HEA200", "S235JR"), ("IPE300", "S235JR")].map(|(a, b)| (a.to_string(), b.to_string())));
        assert!(m.sections.iter().all(|s| s.shape != Shape::Unknown), "{:?}", m.sections);
        let mut cases: Vec<&str> = m.load_cases.iter().map(|c| c.name.as_str()).collect();
        cases.sort();
        assert_eq!(cases, ["DefaultGroep", "LG_Permanent", "LG_Variable", "albl_SelfWeight"]);
        for a in &m.actions {
            eprintln!("{:?} {:?} {:?}", m.load_case(match a.load { LoadRef::Case(c) => c, LoadRef::Combination(c) => c }).map(|c| &c.name), a.target, a.kind);
        }
        for d in &m.diagnostics {
            eprintln!("note {}: {}", d.code, d.message);
        }
        let kinds = |f: fn(&Load) -> bool| m.actions.iter().filter(|a| f(&a.kind)).count();
        assert_eq!(kinds(|k| matches!(k, Load::MemberPoint { .. } | Load::NodalForce { .. })), 1);
        assert_eq!(kinds(|k| matches!(k, Load::MemberDistributed { .. })), 1);
        assert_eq!(kinds(|k| matches!(k, Load::Surface { .. })), 1);
        assert_eq!(kinds(|k| matches!(k, Load::Temperature { .. })), 1);
    }
}
