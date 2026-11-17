//! Analysis results in IFC (`.ifcfemresults`): an IFC4 file of the structural analysis view that
//! holds, beside the analysis model, what was computed for every load case and combination.
//!
//! Read here, from the entities of IFC's structural analysis domain:
//! - the nodes (`IfcStructuralPointConnection`, with their vertex point) and their supports
//!   (`IfcBoundaryNodeCondition`: a boolean or a stiffness per degree of freedom);
//! - the members (`IfcStructuralCurveMember`) between their nodes, the first and second
//!   `IfcRelConnectsStructuralMember` being the start and the end, with the condition given there
//!   as the release of that end, and the member's `Axis` as its local z;
//! - the load cases and combinations (`IfcStructuralLoadGroup`, with their factors);
//! - per `IfcStructuralResultGroup`: the displacements and reactions of the nodes
//!   (`IfcStructuralPointReaction`) and the internal forces along the members
//!   (`IfcStructuralCurveReaction` with a load configuration of stations in metres from the start).
//!
//! The file is in SI units (metres, newtons), as the model is. The
//! checks are not read. The program's own project in the property sets gives the profiles, the
//! grid and the levels (see `ifcfem_project`). A member's end forces
//! are left out where the file also gives its force lines, which hold them.
//!
//! Of the loads, the forces on nodes (`IfcStructuralPointAction`) and the constant line loads along
//! members (`IfcStructuralLinearAction`) are read, each in the load case whose group it is in.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::ProjectData;
use bim_structural::*;
use glam::DVec3;
use std::collections::HashMap;

pub struct IfcFemLoader;

impl Loader for IfcFemLoader {
    fn name(&self) -> &'static str {
        "Structural analysis results (IFC)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifcfemresults", "ifcfem"]
    }

    fn load(&self, source: &Source) -> Result<Vec<crate::model::Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let text = String::from_utf8_lossy(data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data));
        progress.set(Phase::Convert);
        let mut model = read(&text, &source.name()).map_err(|e| format!("{}: {e}", source.name()))?;
        // the profiles, grid and levels of the project the file carries along
        let carried = super::ifcfem_project::find(&text);
        if let Some(carried) = &carried {
            super::ifcfem_project::sections(&mut model, carried);
            let duplicates = model.rebuild_index();
            debug_assert!(duplicates.is_empty(), "{duplicates:?}");
        }
        let doc = SamDocument { origin: DVec3::ZERO, source: model.source.clone(), models: vec![model] };
        let origin = super::sam::anchor(&doc);
        let mut info = super::sam::info(&doc, origin);
        info[0].1 = "Structural analysis results (IFC, .ifcfemresults)".to_string();
        let mut project = ProjectData { info, ..ProjectData::default() };
        if let Some(carried) = &carried {
            super::ifcfem_project::grid(&doc.models[0], carried, origin, &mut project);
        }
        let layers = super::ifcfem_project::solids(&doc.models[0], origin);
        Ok(Loaded { project, layers, views: Vec::new(), origin: Some(origin), structural: doc.models })
    }
}

/// The entities read, by their number: the type and the text between its outer parentheses.
struct File<'a> {
    entities: HashMap<u64, (&'a str, &'a str)>,
    /// The numbers in file order, for what is read in the order it was written.
    order: Vec<u64>,
}

/// The types that are read; the rest (property sets of megabytes among them) is passed over.
const READ: [&str; 24] = [
    "IFCCARTESIANPOINT",
    "IFCVERTEXPOINT",
    "IFCEDGE",
    "IFCDIRECTION",
    "IFCTOPOLOGYREPRESENTATION",
    "IFCPRODUCTDEFINITIONSHAPE",
    "IFCSTRUCTURALPOINTCONNECTION",
    "IFCBOUNDARYNODECONDITION",
    "IFCSTRUCTURALCURVEMEMBER",
    "IFCRELCONNECTSSTRUCTURALMEMBER",
    "IFCSTRUCTURALLOADGROUP",
    "IFCRELASSIGNSTOGROUPBYFACTOR",
    "IFCSTRUCTURALRESULTGROUP",
    "IFCRELASSIGNSTOGROUP",
    "IFCSTRUCTURALPOINTREACTION",
    "IFCSTRUCTURALCURVEREACTION",
    "IFCRELCONNECTSSTRUCTURALACTIVITY",
    "IFCSTRUCTURALLOADSINGLEFORCE",
    "IFCSTRUCTURALLOADSINGLEDISPLACEMENT",
    "IFCSTRUCTURALLOADCONFIGURATION",
    "IFCPROJECT",
    "IFCSTRUCTURALPOINTACTION",
    "IFCSTRUCTURALLINEARACTION",
    "IFCSTRUCTURALLOADLINEARFORCE",
];

impl<'a> File<'a> {
    fn parse(text: &'a str) -> Self {
        let (mut entities, mut order) = (HashMap::new(), Vec::new());
        for line in text.lines() {
            let Some(rest) = line.strip_prefix('#') else { continue };
            let Some((number, rest)) = rest.split_once('=') else { continue };
            let Some(open) = rest.find('(') else { continue };
            let name = rest[..open].trim();
            if !READ.contains(&name) {
                continue;
            }
            let Ok(number) = number.trim().parse::<u64>() else { continue };
            let Some(close) = rest.rfind(')') else { continue };
            if close > open {
                entities.insert(number, (name, &rest[open + 1..close]));
                order.push(number);
            }
        }
        File { entities, order }
    }

    fn of(&self, kind: &'a str) -> impl Iterator<Item = (u64, Vec<&'a str>)> + '_ {
        self.order.iter().filter_map(move |n| {
            let (name, args) = self.entities.get(n)?;
            (*name == kind).then(|| (*n, split(args)))
        })
    }

    fn args(&self, number: u64, kind: &str) -> Option<Vec<&'a str>> {
        let (name, args) = self.entities.get(&number)?;
        (*name == kind).then(|| split(args))
    }

    /// The point of a product's shape: shape → topology representation → vertex → point.
    fn vertex_point(&self, vertex: u64) -> Option<DVec3> {
        let point = reference(self.args(vertex, "IFCVERTEXPOINT")?.first()?)?;
        let c = numbers(self.args(point, "IFCCARTESIANPOINT")?.first()?);
        Some(DVec3::new(*c.first()?, *c.get(1)?, c.get(2).copied().unwrap_or(0.0)))
    }

    /// The items of a product definition shape's representations.
    fn shape_items(&self, shape: u64) -> Vec<u64> {
        let Some(shape) = self.args(shape, "IFCPRODUCTDEFINITIONSHAPE") else { return Vec::new() };
        references(shape.get(2).copied().unwrap_or_default())
            .into_iter()
            .filter_map(|r| self.args(r, "IFCTOPOLOGYREPRESENTATION"))
            .flat_map(|r| references(r.get(3).copied().unwrap_or_default()))
            .collect()
    }
}

/// The arguments at the top level: commas inside parentheses and strings do not split.
fn split(args: &str) -> Vec<&str> {
    let (mut out, mut depth, mut quoted, mut start) = (Vec::new(), 0usize, false, 0usize);
    for (k, c) in args.char_indices() {
        match c {
            '\'' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                out.push(args[start..k].trim());
                start = k + 1;
            }
            _ => {}
        }
    }
    out.push(args[start..].trim());
    out
}

fn reference(arg: &str) -> Option<u64> {
    arg.trim().strip_prefix('#')?.parse().ok()
}

/// Every reference in an argument, a list of them included.
fn references(arg: &str) -> Vec<u64> {
    arg.split(|c: char| !(c.is_ascii_digit() || c == '#')).filter_map(reference).collect()
}

/// A number, bare or wrapped in its measure type (`IFCFORCEMEASURE(2600.)`).
fn number(arg: &str) -> Option<f64> {
    let arg = arg.trim();
    let inner = match (arg.find('('), arg.rfind(')')) {
        (Some(open), Some(close)) if close > open => &arg[open + 1..close],
        _ => arg,
    };
    inner.trim().parse().ok()
}

/// The numbers of a (nested) list.
fn numbers(arg: &str) -> Vec<f64> {
    arg.split(|c: char| c == ',' || c == '(' || c == ')').filter_map(|s| s.trim().parse().ok()).collect()
}

/// A string without its quotes, with `''` and the `\X2\…\X0\` escapes resolved.
pub(super) fn string(arg: &str) -> Option<String> {
    let s = arg.trim().strip_prefix('\'')?.strip_suffix('\'')?.replace("''", "'");
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(at) = rest.find("\\X2\\") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 4..];
        let end = after.find("\\X0\\").unwrap_or(after.len());
        let units: Vec<u16> = after[..end].as_bytes().chunks(4).filter_map(|h| u16::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()).collect();
        out.push_str(&String::from_utf16_lossy(&units));
        rest = after.get(end + 4..).unwrap_or_default();
    }
    out.push_str(rest);
    Some(out)
}

/// The number a name ends in (`3D-knoop 12` → `12`), or the name.
fn tail_number(name: &str) -> String {
    name.rsplit(' ').next().filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())).map_or_else(|| name.to_string(), str::to_string)
}

/// A degree of freedom of a boundary condition: a boolean, a stiffness, or not given.
fn dof(arg: &str) -> Dof {
    let arg = arg.trim();
    if arg.contains(".T.") {
        Dof::Fixed
    } else if arg.contains(".F.") || arg == "$" {
        Dof::Free
    } else {
        match number(arg) {
            Some(k) if k != 0.0 => Dof::Spring(k),
            _ => Dof::Free,
        }
    }
}

fn condition(file: &File, entity: u64) -> Option<Dofs> {
    let args = file.args(entity, "IFCBOUNDARYNODECONDITION")?;
    let mut dofs = dofs_all(Dof::Free);
    for (k, d) in dofs.iter_mut().enumerate() {
        *d = dof(args.get(k + 1).copied().unwrap_or("$"));
    }
    Some(dofs)
}

/// The six values of a single force or displacement (after its name).
fn six(file: &File, entity: u64) -> Option<[f64; 6]> {
    let (name, args) = file.entities.get(&entity)?;
    if !matches!(*name, "IFCSTRUCTURALLOADSINGLEFORCE" | "IFCSTRUCTURALLOADSINGLEDISPLACEMENT") {
        return None;
    }
    let args = split(args);
    let mut out = [0.0; 6];
    for (k, v) in out.iter_mut().enumerate() {
        *v = args.get(k + 1).and_then(|a| number(a)).unwrap_or(0.0);
    }
    Some(out)
}

/// Reads the file into an analysis model.
fn read(text: &str, file_name: &str) -> Result<AnalysisModel, String> {
    if !text.trim_start().starts_with("ISO-10303-21") {
        return Err("expected an IFC file (ISO-10303-21)".into());
    }
    let application = text.lines().take(12).find(|l| l.starts_with("FILE_NAME")).and_then(|l| {
        let args = split(l.strip_prefix("FILE_NAME(")?.trim_end_matches(';').strip_suffix(')')?);
        string(args.get(5)?).filter(|s| !s.is_empty())
    });
    let file = File::parse(text);
    let title = file.of("IFCPROJECT").next().and_then(|(_, a)| string(a.get(2)?)).filter(|t| !t.is_empty());
    let source = SourceInfo { format: "IFC4".into(), file: file_name.to_string(), application, ..SourceInfo::default() };
    let mut model = AnalysisModel::new(1, title.unwrap_or_else(|| file_name.to_string()), source);

    // nodes, by the number of their connection and of their vertex
    let mut node_of: HashMap<u64, Id> = HashMap::new();
    let mut at_vertex: HashMap<u64, Id> = HashMap::new();
    for (n, args) in file.of("IFCSTRUCTURALPOINTCONNECTION") {
        let Some(shape) = args.get(6).and_then(|a| reference(a)) else { continue };
        let Some((vertex, p)) = file.shape_items(shape).into_iter().find_map(|v| Some((v, file.vertex_point(v)?))) else { continue };
        let id = sam_id(IdKind::Node, n);
        let support = args.get(7).and_then(|a| reference(a)).and_then(|c| condition(&file, c)).filter(|dofs| dofs.iter().any(|d| *d != Dof::Free)).map(|dofs| {
            let preset = support::classify(&dofs, Dimension::Space3d);
            Support { dofs, frame: DofFrame::Global, named: None, preset }
        });
        let name = args.get(2).and_then(|a| string(a)).unwrap_or_default();
        model.nodes.push(Node { id, number: tail_number(&name), p, frame: None, support, origin: NodeOrigin::Analytical });
        node_of.insert(n, id);
        at_vertex.insert(vertex, id);
    }
    if model.nodes.is_empty() {
        return Err("no structural analysis model in this file".into());
    }

    // members: their nodes and end releases from the connections, in the order written
    let mut ends: HashMap<u64, Vec<(Id, Option<Hinge>)>> = HashMap::new();
    for (_, args) in file.of("IFCRELCONNECTSSTRUCTURALMEMBER") {
        let (Some(member), Some(node)) = (args.get(4).and_then(|a| reference(a)), args.get(5).and_then(|a| reference(a)).and_then(|c| node_of.get(&c))) else { continue };
        // a condition at a member's end says what is connected there: what is not is released
        let hinge = args.get(6).and_then(|a| reference(a)).and_then(|c| condition(&file, c)).filter(|dofs| dofs.iter().any(|d| *d != Dof::Fixed)).map(|dofs| Hinge { dofs, named: None });
        ends.entry(member).or_default().push((*node, hinge));
    }
    let mut member_of: HashMap<u64, Id> = HashMap::new();
    for (n, args) in file.of("IFCSTRUCTURALCURVEMEMBER") {
        let mut nodes: Vec<(Id, Option<Hinge>)> = ends.remove(&n).unwrap_or_default();
        if nodes.len() < 2 {
            // no connections: the ends of the member's edge
            let edge = args.get(6).and_then(|a| reference(a)).into_iter().flat_map(|s| file.shape_items(s)).find_map(|e| file.args(e, "IFCEDGE"));
            nodes = edge.into_iter().flat_map(|e| [e.first().copied(), e.get(1).copied()]).flatten().filter_map(reference).filter_map(|v| at_vertex.get(&v)).map(|id| (*id, None)).collect();
        }
        let (Some(a), Some(b)) = (nodes.first().cloned(), nodes.get(1).cloned()) else { continue };
        let (Some(pa), Some(pb)) = (model.nodes.iter().find(|x| x.id == a.0).map(|x| x.p), model.nodes.iter().find(|x| x.id == b.0).map(|x| x.p)) else { continue };
        let axis = args.get(8).and_then(|a| reference(a)).and_then(|d| file.args(d, "IFCDIRECTION")).map(|d| numbers(d.first().copied().unwrap_or_default())).filter(|c| c.len() >= 3).map(|c| DVec3::new(c[0], c[1], c[2])).filter(|v| v.length() > 1e-9);
        let id = sam_id(IdKind::Member, n);
        let name = args.get(2).and_then(|a| string(a)).unwrap_or_default();
        model.members.push(Member {
            id,
            number: tail_number(&name),
            kind: MemberKind::Beam,
            nodes: vec![a.0, b.0],
            geometry: Curve::Line,
            z_ref: axis.unwrap_or_else(|| frame::default_z_ref(pb - pa)),
            orientation: if axis.is_some() { Orientation::IfcAxis } else { Orientation::Default },
            section: None,
            section_rotation: 0.0,
            insertion: Insertion::default(),
            releases: [a.1, b.1],
            material: None,
            links: Vec::new(),
        });
        member_of.insert(n, id);
    }

    // load cases and combinations
    let mut load_of: HashMap<u64, LoadRef> = HashMap::new();
    for (n, args) in file.of("IFCSTRUCTURALLOADGROUP") {
        let name = args.get(2).and_then(|a| string(a)).unwrap_or_default();
        let word = |k: usize| args.get(k).map(|a| a.trim_matches('.').to_string()).unwrap_or_default();
        if word(5) == "LOAD_COMBINATION" {
            // a file may give its load groups once per analysis model: one of a name is one
            if let Some(same) = model.combinations.iter().find(|c| c.name == name) {
                load_of.insert(n, LoadRef::Combination(same.id));
                continue;
            }
            let id = sam_id(IdKind::Combination, n);
            let purpose = match args.get(9).and_then(|a| string(a)).unwrap_or_default().as_str() {
                "UGT" | "ULS" => Purpose::Uls,
                "BGT" | "SLS" => Purpose::Sls,
                other => Purpose::User(other.to_string()),
            };
            model.combinations.push(Combination { id, number: (model.combinations.len() + 1).to_string(), name, purpose, terms: Vec::new() });
            load_of.insert(n, LoadRef::Combination(id));
        } else {
            if let Some(same) = model.load_cases.iter().find(|c| c.name == name) {
                load_of.insert(n, LoadRef::Case(same.id));
                continue;
            }
            let id = sam_id(IdKind::LoadCase, n);
            let action_type = match word(6).as_str() {
                "PERMANENT_G" => ActionType::Permanent,
                "VARIABLE_Q" => ActionType::Variable,
                "EXTRAORDINARY_A" => ActionType::Accidental,
                other => ActionType::User(other.to_string()),
            };
            model.load_cases.push(LoadCase { id, number: (model.load_cases.len() + 1).to_string(), name, action_type, action_source: word(7), self_weight: None, coefficient: number(args.get(8).copied().unwrap_or("$")).unwrap_or(1.0) });
            load_of.insert(n, LoadRef::Case(id));
        }
    }
    for (_, args) in file.of("IFCRELASSIGNSTOGROUPBYFACTOR") {
        let (Some(LoadRef::Combination(into)), Some(factor)) = (args.get(6).and_then(|a| reference(a)).and_then(|g| load_of.get(&g)), args.get(7).and_then(|a| number(a))) else { continue };
        let Some(combination) = model.combinations.iter_mut().find(|c| c.id == *into) else { continue };
        combination.terms.extend(references(args.get(4).copied().unwrap_or_default()).into_iter().filter_map(|g| load_of.get(&g)).map(|l| (*l, factor)));
    }

    // what every result (a reaction) acts on, and the result group it is in
    let mut acts_on: HashMap<u64, u64> = HashMap::new();
    for (_, args) in file.of("IFCRELCONNECTSSTRUCTURALACTIVITY") {
        if let (Some(item), Some(activity)) = (args.get(4).and_then(|a| reference(a)), args.get(5).and_then(|a| reference(a))) {
            acts_on.insert(activity, item);
        }
    }
    let mut in_group: HashMap<u64, Vec<u64>> = HashMap::new();
    for (_, args) in file.of("IFCRELASSIGNSTOGROUP") {
        if let Some(group) = args.get(6).and_then(|a| reference(a)) {
            in_group.entry(group).or_default().extend(references(args.get(4).copied().unwrap_or_default()));
        }
    }
    // the loads: a force on a node, or a line load along a whole member, in the load case whose
    // group they are in
    for (kind, name) in [(false, "IFCSTRUCTURALPOINTACTION"), (true, "IFCSTRUCTURALLINEARACTION")] {
        for (n, args) in file.of(name) {
            let (Some(item), Some(load)) = (acts_on.get(&n), args.get(7).and_then(|a| reference(a))) else { continue };
            let Some(case) = in_group.iter().filter(|(_, members)| members.contains(&n)).find_map(|(group, _)| load_of.get(group)).copied() else { continue };
            let frame = if args.get(8).is_some_and(|f| f.contains("LOCAL")) { LoadFrame::Local } else { LoadFrame::Global };
            let id = sam_id(IdKind::Action, n);
            if kind {
                let (Some(member), Some(q)) = (member_of.get(item), file.args(load, "IFCSTRUCTURALLOADLINEARFORCE")) else { continue };
                let q = DVec3::new(number(q.get(1).copied().unwrap_or("$")).unwrap_or(0.0), number(q.get(2).copied().unwrap_or("$")).unwrap_or(0.0), number(q.get(3).copied().unwrap_or("$")).unwrap_or(0.0));
                if q.length() > 0.0 {
                    model.actions.push(Action { id, load: case, target: Target::Member { id: *member, span: None }, frame, kind: Load::MemberDistributed { dir: q.normalize(), moment: false, dist: Distribution::Uniform(q.length()) }, destabilising: false });
                }
            } else if let (Some(node), Some(v)) = (node_of.get(item), six(&file, load)) {
                model.actions.push(Action { id, load: case, target: Target::Node(*node), frame, kind: Load::NodalForce { f: DVec3::new(v[0], v[1], v[2]), m: DVec3::new(v[3], v[4], v[5]) }, destabilising: false });
            }
        }
    }
    model.actions.sort_by_key(|a| a.load);
    for (n, args) in file.of("IFCSTRUCTURALRESULTGROUP") {
        let name = args.get(2).and_then(|a| string(a)).unwrap_or_default();
        let (mut displacements, mut reactions) = (results::NodeTable::default(), results::NodeTable::default());
        // the force lines of a member replace its end forces
        let mut lines: HashMap<Id, (Vec<f64>, Vec<[f64; 6]>, bool)> = HashMap::new();
        for r in in_group.get(&n).into_iter().flatten() {
            let Some(item) = acts_on.get(r) else { continue };
            if let Some(a) = file.args(*r, "IFCSTRUCTURALPOINTREACTION") {
                let (Some(node), Some(load)) = (node_of.get(item), a.get(7).and_then(|l| reference(l))) else { continue };
                let (Some(values), Some((kind, _))) = (six(&file, load), file.entities.get(&load)) else { continue };
                let table = if *kind == "IFCSTRUCTURALLOADSINGLEDISPLACEMENT" { &mut displacements } else { &mut reactions };
                table.node.push(*node);
                for (k, v) in values.into_iter().enumerate() {
                    table.comps[k].push(v);
                }
            } else if let Some(a) = file.args(*r, "IFCSTRUCTURALCURVEREACTION") {
                let (Some(member), Some(config)) = (member_of.get(item), a.get(7).and_then(|l| reference(l)).and_then(|c| file.args(c, "IFCSTRUCTURALLOADCONFIGURATION"))) else { continue };
                let values: Vec<[f64; 6]> = references(config.get(1).copied().unwrap_or_default()).into_iter().filter_map(|f| six(&file, f)).collect();
                let x = numbers(config.get(2).copied().unwrap_or_default());
                if values.is_empty() || values.len() != x.len() {
                    continue;
                }
                let is_line = values.len() > 2 || a.get(4).and_then(|t| string(t)).is_some_and(|t| t.contains(":lijnen"));
                let slot = lines.entry(*member).or_insert_with(|| (Vec::new(), Vec::new(), false));
                if is_line || !slot.2 {
                    *slot = (x, values, is_line);
                }
            }
        }
        let mut forces = results::MemberTable { offsets: vec![0], ..Default::default() };
        for m in &model.members {
            let Some((x, values, _)) = lines.get(&m.id) else { continue };
            forces.member.push(m.id);
            forces.x.extend(x);
            for v in values {
                for (k, c) in v.iter().enumerate() {
                    forces.comps[k].push(*c);
                }
            }
            forces.offsets.push(forces.x.len() as u32);
        }
        let mut payload = results::ResultPayload::default();
        let mut available = Vec::new();
        if !forces.member.is_empty() {
            available.push(results::TableKind::MemberForces);
            payload.member_forces = Some(forces);
        }
        if !displacements.node.is_empty() {
            available.push(results::TableKind::NodeDisplacements);
            payload.node_displacements = Some(displacements);
        }
        if !reactions.node.is_empty() {
            available.push(results::TableKind::Reactions);
            payload.reactions = Some(reactions);
        }
        // the load group the results are of; a group without one goes by its name
        let named = name.strip_prefix("Resultaten ").unwrap_or(&name);
        let load = args.get(6).and_then(|a| reference(a)).and_then(|g| load_of.get(&g).copied()).or_else(|| {
            model.load_cases.iter().find(|c| c.name == named).map(|c| LoadRef::Case(c.id)).or_else(|| model.combinations.iter().find(|c| c.name == named).map(|c| LoadRef::Combination(c.id)))
        });
        let for_ = match load {
            Some(LoadRef::Case(id)) => ResultFor::Case(id),
            Some(LoadRef::Combination(id)) => ResultFor::Combination(id),
            None => ResultFor::Envelope(name.clone()),
        };
        let theory = match args.get(5).map(|t| t.trim_matches('.')) {
            Some("SECOND_ORDER_THEORY") => Theory::Second,
            Some("THIRD_ORDER_THEORY") => Theory::Third,
            Some("FULL_NONLINEAR_THEORY") => Theory::FullNonlinear,
            _ => Theory::First,
        };
        model.results.push(ResultSet {
            id: sam_id(IdKind::ResultSet, n),
            number: (model.results.len() + 1).to_string(),
            name: named.to_string(),
            for_,
            theory,
            linear: args.get(7).is_none_or(|l| l.contains('T')),
            available,
            inline: Some(payload),
        });
    }
    let duplicates = model.rebuild_index();
    debug_assert!(duplicates.is_empty(), "{duplicates:?}");
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A beam of 6 m along y on a fixed and a free node, with the results of one load case and
    /// of one combination.
    const BEAM: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [StructuralAnalysisView]'),'2;1');
FILE_NAME('ligger.ifcfemresults','',(''),(''),'Rekenprogramma','Rekenprogramma','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#11=IFCCARTESIANPOINT((0.,0.,0.));
#15=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,$,$);
#18=IFCPROJECT('3xXKKU3BlO1jIL0YIPD01g',$,'Ligger \\X2\\2014\\X0\\ proef',$,$,$,$,(#15),$);
#23=IFCSTRUCTURALLOADGROUP('a',$,'Eigen gewicht',$,$,.LOAD_CASE.,.PERMANENT_G.,.DEAD_LOAD_G.,$,$);
#28=IFCSTRUCTURALLOADGROUP('b',$,'UGT 6.10a',$,$,.LOAD_COMBINATION.,.USERDEFINED.,.USERDEFINED.,$,'UGT');
#29=IFCRELASSIGNSTOGROUPBYFACTOR('c',$,$,$,(#23),$,#28,1.35);
#94=IFCVERTEXPOINT(#11);
#95=IFCTOPOLOGYREPRESENTATION(#15,'Reference','Vertex',(#94));
#96=IFCPRODUCTDEFINITIONSHAPE($,$,(#95));
#97=IFCBOUNDARYNODECONDITION('oplegging',IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.));
#98=IFCSTRUCTURALPOINTCONNECTION('d',$,'3D-knoop 1',$,$,$,#96,#97,$);
#99=IFCCARTESIANPOINT((0.,6.,0.));
#100=IFCVERTEXPOINT(#99);
#101=IFCTOPOLOGYREPRESENTATION(#15,'Reference','Vertex',(#100));
#102=IFCPRODUCTDEFINITIONSHAPE($,$,(#101));
#109=IFCSTRUCTURALPOINTCONNECTION('e',$,'3D-knoop 2',$,$,$,#102,$,$);
#435=IFCEDGE(#94,#100);
#436=IFCTOPOLOGYREPRESENTATION(#15,'Reference','Edge',(#435));
#437=IFCPRODUCTDEFINITIONSHAPE($,$,(#436));
#438=IFCDIRECTION((0.,0.,1.));
#439=IFCSTRUCTURALCURVEMEMBER('f',$,'3D-staaf 7',$,$,$,#437,.RIGID_JOINED_MEMBER.,#438);
#440=IFCRELCONNECTSSTRUCTURALMEMBER('g',$,$,$,#439,#98,$,$,$,$);
#450=IFCBOUNDARYNODECONDITION('scharnier',IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.T.),IFCBOOLEAN(.F.),IFCBOOLEAN(.F.));
#441=IFCRELCONNECTSSTRUCTURALMEMBER('h',$,$,$,#439,#109,#450,$,$,$);
#1621=IFCSTRUCTURALLOADSINGLEFORCE('last 1',IFCFORCEMEASURE(2600.),IFCFORCEMEASURE(0.),IFCFORCEMEASURE(0.),IFCTORQUEMEASURE(0.),IFCTORQUEMEASURE(0.),IFCTORQUEMEASURE(0.));
#1622=IFCSTRUCTURALPOINTACTION('t',$,'knooplast 1',$,$,$,$,#1621,.GLOBAL_COORDS.,$);
#1623=IFCRELCONNECTSSTRUCTURALACTIVITY('u',$,$,$,#109,#1622);
#1648=IFCSTRUCTURALLOADLINEARFORCE('q 1',$,IFCLINEARFORCEMEASURE(0.),IFCLINEARFORCEMEASURE(-375.),$,$,$);
#1649=IFCSTRUCTURALLINEARACTION('v',$,'staaflast 1',$,$,$,$,#1648,.LOCAL_COORDS.,$,.TRUE_LENGTH.,.CONST.);
#1650=IFCRELCONNECTSSTRUCTURALACTIVITY('w',$,$,$,#439,#1649);
#87=IFCSTRUCTURALLOADGROUP('y',$,'Eigen gewicht',$,$,.LOAD_CASE.,.NOTDEFINED.,.NOTDEFINED.,$,$);
#2088=IFCRELASSIGNSTOGROUP('z',$,$,$,(#98,#109,#439,#1622,#1649),$,#91);
#2089=IFCRELASSIGNSTOGROUP('x',$,$,$,(#1622,#1649),$,#87);
#2096=IFCPROPERTYSINGLEVALUE('Inhoud',$,IFCTEXT('#1=IFCSTRUCTURALCURVEMEMBER(, (((('),$);
#2100=IFCSTRUCTURALRESULTGROUP('i',$,'Resultaten Eigen gewicht',$,'geval',.FIRST_ORDER_THEORY.,$,.T.);
#2101=IFCSTRUCTURALLOADSINGLEDISPLACEMENT('u 2',0.,0.,-0.012,0.,0.,0.);
#2102=IFCSTRUCTURALPOINTREACTION('j',$,'Verplaatsing knoop 2',$,'verplaatsing:2',$,$,#2101,.GLOBAL_COORDS.);
#2103=IFCRELCONNECTSSTRUCTURALACTIVITY('k',$,$,$,#109,#2102);
#2299=IFCSTRUCTURALLOADSINGLEFORCE('R 1',IFCFORCEMEASURE(0.),IFCFORCEMEASURE(0.),IFCFORCEMEASURE(6000.),IFCTORQUEMEASURE(0.),IFCTORQUEMEASURE(-18000.),IFCTORQUEMEASURE(0.));
#2300=IFCSTRUCTURALPOINTREACTION('l',$,'Reactie knoop 1',$,'reactie:1',$,$,#2299,.GLOBAL_COORDS.);
#2301=IFCRELCONNECTSSTRUCTURALACTIVITY('m',$,$,$,#98,#2300);
#2335=IFCSTRUCTURALLOADSINGLEFORCE('Start',0.,0.,6000.,0.,-18000.,0.);
#2336=IFCSTRUCTURALLOADSINGLEFORCE('End',0.,0.,0.,0.,0.,0.);
#2337=IFCSTRUCTURALLOADCONFIGURATION('Eindkrachten',(#2335,#2336),((0.),(6.E0)));
#2338=IFCSTRUCTURALCURVEREACTION('n',$,'Eindkrachten staaf 7',$,'staaf:7:eindkrachten',$,$,#2337,.LOCAL_COORDS.,.DISCRETE.);
#2339=IFCRELCONNECTSSTRUCTURALACTIVITY('o',$,$,$,#439,#2338);
#3060=IFCSTRUCTURALLOADSINGLEFORCE($,0.,0.,6000.,0.,-18000.,0.);
#3061=IFCSTRUCTURALLOADSINGLEFORCE($,0.,0.,3000.,0.,-4500.,0.);
#3062=IFCSTRUCTURALLOADSINGLEFORCE($,0.,0.,0.,0.,0.,0.);
#3081=IFCSTRUCTURALLOADCONFIGURATION('Snedekrachtlijnen',(#3060,#3061,#3062),((0.),(3.E0),(6.E0)));
#3082=IFCSTRUCTURALCURVEREACTION('p',$,'Snedekrachtlijnen staaf 7',$,'staaf:7:lijnen:axial',$,$,#3081,.LOCAL_COORDS.,.DISCRETE.);
#3083=IFCRELCONNECTSSTRUCTURALACTIVITY('q',$,$,$,#439,#3082);
#9225=IFCRELASSIGNSTOGROUP('r',$,$,$,(#2102,#2300,#2338,#3082),$,#2100);
#9226=IFCSTRUCTURALRESULTGROUP('s',$,'Resultaten UGT 6.10a',$,'combinatie',.SECOND_ORDER_THEORY.,#28,.F.);
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn reads_the_model_its_supports_and_releases() {
        let model = read(BEAM, "ligger.ifcfemresults").expect("a model");
        assert_eq!(model.name, "Ligger — proef");
        assert_eq!(model.source.application.as_deref(), Some("Rekenprogramma"));
        assert_eq!(model.nodes.iter().map(|n| (n.number.as_str(), n.p.y, n.support.is_some())).collect::<Vec<_>>(), [("1", 0.0, true), ("2", 6.0, false)]);
        assert_eq!(model.nodes[0].support.as_ref().map(|s| s.preset), Some(SupportPreset::Fixed));
        let member = &model.members[0];
        assert_eq!((member.number.as_str(), member.nodes.as_slice()), ("7", [model.nodes[0].id, model.nodes[1].id].as_slice()));
        assert_eq!((member.z_ref, &member.orientation), (DVec3::Z, &Orientation::IfcAxis));
        // rigid at the start; at the end the two bending moments are let go
        assert!(member.releases[0].is_none());
        let hinge = member.releases[1].as_ref().expect("a hinge");
        assert_eq!((&hinge.dofs[3], &hinge.dofs[4], &hinge.dofs[5]), (&Dof::Fixed, &Dof::Free, &Dof::Free));
    }

    #[test]
    fn reads_load_cases_combinations_and_what_was_computed() {
        let model = read(BEAM, "ligger.ifcfemresults").expect("a model");
        assert_eq!(model.load_cases.iter().map(|c| (c.name.as_str(), &c.action_type)).collect::<Vec<_>>(), [("Eigen gewicht", &ActionType::Permanent)]);
        let combination = &model.combinations[0];
        assert_eq!((combination.name.as_str(), &combination.purpose), ("UGT 6.10a", &Purpose::Uls));
        assert_eq!(combination.terms, [(LoadRef::Case(model.load_cases[0].id), 1.35)]);

        // the first group names no load group: it goes by its name
        let set = &model.results[0];
        assert_eq!((set.name.as_str(), &set.for_, set.theory, set.linear), ("Eigen gewicht", &ResultFor::Case(model.load_cases[0].id), Theory::First, true));
        let payload = set.inline.as_ref().expect("its tables");
        // the three stations of the force lines, not the two end forces
        let forces = payload.member_forces.as_ref().expect("member forces");
        assert_eq!((forces.member.as_slice(), forces.offsets.as_slice(), forces.x.as_slice()), ([model.members[0].id].as_slice(), [0, 3].as_slice(), [0.0, 3.0, 6.0].as_slice()));
        assert_eq!((forces.comps[2].as_slice(), forces.comps[4].as_slice()), ([6000.0, 3000.0, 0.0].as_slice(), [-18000.0, -4500.0, 0.0].as_slice()));
        let displacements = payload.node_displacements.as_ref().expect("displacements");
        assert_eq!((displacements.node.as_slice(), displacements.comps[2].as_slice()), ([model.nodes[1].id].as_slice(), [-0.012].as_slice()));
        let reactions = payload.reactions.as_ref().expect("reactions");
        assert_eq!((reactions.node.as_slice(), reactions.comps[2].as_slice(), reactions.comps[4].as_slice()), ([model.nodes[0].id].as_slice(), [6000.0].as_slice(), [-18000.0].as_slice()));

        // a force on node 2 and a line load along the member, both in the load case: the file names
        // it twice, and holds them under the second, but it is one case
        let case = LoadRef::Case(model.load_cases[0].id);
        assert_eq!(model.actions.len(), 2);
        assert!(matches!(&model.actions[0], Action { load, target: Target::Node(n), frame: LoadFrame::Global, kind: Load::NodalForce { f, .. }, .. } if *load == case && *n == model.nodes[1].id && *f == DVec3::new(2600.0, 0.0, 0.0)));
        assert!(matches!(&model.actions[1], Action { load, target: Target::Member { id, span: None }, frame: LoadFrame::Local, kind: Load::MemberDistributed { dir, dist: Distribution::Uniform(q), .. }, .. } if *load == case && *id == model.members[0].id && *dir == DVec3::NEG_Z && *q == 375.0));

        let second = &model.results[1];
        assert_eq!((&second.for_, second.theory, second.linear, second.available.is_empty()), (&ResultFor::Combination(combination.id), Theory::Second, false, true));
    }

    #[test]
    fn another_file_is_refused() {
        assert!(read("{}", "x.ifcfemresults").is_err());
        assert!(read("ISO-10303-21;\nDATA;\n#1=IFCWALL('a',$,'w',$,$,$,$,$,$);\nENDSEC;\n", "x.ifcfemresults").is_err());
    }
}
