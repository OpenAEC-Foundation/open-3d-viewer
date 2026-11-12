//! Built-in model checks without an IDS and without geometry. Rule codes follow the buildingSMART
//! validation service (https://buildingsmart.github.io/ifc-gherkin-rules/) where a corresponding
//! rule exists; `IfcRoot.UR1` is the IFC schema uniqueness rule; `OBV-` codes are Open BIM
//! Validator industry-practice checks. Findings are in express-ID order per rule.
use crate::graph::{number, text, Graph, Node};
use crate::{
    ModelSummary, RuleSource, Severity, SpecificationResult, StoreySummary, ValidationStatus as S,
};
use ifc_lite_core::AttributeValue as A;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

pub fn tick(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Validation cancelled".into());
    }
    Ok(())
}

/// Shared model view for the built-in rule sets: class tests are cached per entity keyword.
pub struct Model<'a> {
    pub g: &'a Graph,
    pub cancel: &'a AtomicBool,
    classes: RefCell<HashMap<(&'a str, &'static str), bool>>,
    children: HashMap<u32, Vec<u32>>,
    /// Checked elements (see `ModelSummary`) in express-ID order.
    pub elements: Vec<&'a Node>,
}
impl<'a> Model<'a> {
    pub fn new(g: &'a Graph, cancel: &'a AtomicBool) -> Result<Self, String> {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for (child, parents) in &g.parents {
            for (relation, parent) in parents {
                if relation == "IFCRELAGGREGATES" {
                    children.entry(*parent).or_default().push(*child);
                }
            }
        }
        for list in children.values_mut() {
            list.sort_unstable();
        }
        let mut m = Self {
            g,
            cancel,
            classes: RefCell::default(),
            children,
            elements: vec![],
        };
        let mut elements = vec![];
        for n in g.nodes.values() {
            tick(cancel)?;
            if m.physical(n) && !m.part_of_element(n.id) {
                elements.push(n);
            }
        }
        m.elements = elements;
        Ok(m)
    }
    /// Schema-specific subtype test, cached per (keyword, class).
    pub fn is(&self, n: &'a Node, class: &'static str) -> bool {
        let g = self.g;
        *self
            .classes
            .borrow_mut()
            .entry((n.kind.as_str(), class))
            .or_insert_with(|| g.schema.is_subtype_of(&n.kind, class))
    }
    /// IfcElement except feature (opening) and virtual elements.
    pub fn physical(&self, n: &'a Node) -> bool {
        self.is(n, "IFCELEMENT")
            && !self.is(n, "IFCFEATUREELEMENT")
            && !self.is(n, "IFCVIRTUALELEMENT")
    }
    /// Relating objects of the given relationship kinds, in document order.
    pub fn related(&self, id: u32, relations: &[&str]) -> Vec<&'a Node> {
        let g: &'a Graph = self.g;
        g.parents
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|(r, _)| relations.contains(&r.as_str()))
            .filter_map(|(_, p)| g.nodes.get(p))
            .collect()
    }
    pub fn part_of_element(&self, id: u32) -> bool {
        self.related(id, &["IFCRELAGGREGATES"])
            .into_iter()
            .any(|p| self.is(p, "IFCELEMENT"))
    }
    pub fn has_type(&self, n: &Node) -> bool {
        self.g.types.get(&n.id).is_some_and(|t| !t.is_empty())
    }
    /// Material on the element, its type, or any aggregated part.
    pub fn has_material(&self, n: &Node) -> bool {
        let g = self.g;
        let assigned = |id: &u32| g.materials.get(id).is_some_and(|m| !m.is_empty());
        let mut stack = vec![n.id];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if seen.len() > 10_000 || !seen.insert(id) {
                continue;
            }
            if assigned(&id) || g.types.get(&id).into_iter().flatten().any(assigned) {
                return true;
            }
            stack.extend(self.children.get(&id).into_iter().flatten());
        }
        false
    }
    /// (classification system name, leaf reference code) for the element and its type.
    pub fn classifications(&self, n: &Node) -> Vec<(Option<String>, Option<String>)> {
        let g = self.g;
        let mut out = vec![];
        let owners = std::iter::once(n.id).chain(g.types.get(&n.id).into_iter().flatten().copied());
        for owner in owners {
            for start in g.classifications.get(&owner).into_iter().flatten() {
                let (mut current, mut code, mut system) = (*start, None, None);
                for _ in 0..64 {
                    let Some(obj) = g.nodes.get(&current) else {
                        break;
                    };
                    if obj.kind == "IFCCLASSIFICATION" {
                        system = g.name(obj);
                        break;
                    }
                    if obj.kind != "IFCCLASSIFICATIONREFERENCE" {
                        break;
                    }
                    if code.is_none() {
                        code = ["Identification", "ItemReference"]
                            .iter()
                            .find_map(|a| g.attr(obj, a).and_then(text))
                            .filter(|s| !s.trim().is_empty());
                    }
                    match g.refs(obj, "ReferencedSource").first() {
                        Some(next) => current = *next,
                        None => break,
                    }
                }
                out.push((system, code));
            }
        }
        out
    }
    /// A valid NL-SfB code, or why there is none.
    pub fn nlsfb(&self, n: &Node) -> Result<String, String> {
        let mut invalid = vec![];
        for (system, code) in self.classifications(n) {
            if !system.as_deref().is_some_and(is_nlsfb_system) {
                continue;
            }
            match code {
                Some(code) if is_nlsfb_code(&code) => return Ok(code),
                Some(code) => invalid.push(code),
                None => invalid.push(String::new()),
            }
        }
        if invalid.is_empty() {
            Err("No NL-SfB classification reference (element or type)".into())
        } else {
            Err(format!(
                "NL-SfB code '{}' is not of the form 21 or 21.12",
                invalid.join("', '")
            ))
        }
    }
    /// Property value from the first property set accepted by `set`, occurrence before type.
    pub fn property(
        &self,
        n: &Node,
        set: impl Fn(&str) -> bool,
        name: &str,
    ) -> Option<(String, &'a A)> {
        let g: &'a Graph = self.g;
        let mut owners = vec![(n.id, g.psets.get(&n.id).cloned().unwrap_or_default())];
        for t in g.types.get(&n.id).into_iter().flatten() {
            let mut sets = g.psets.get(t).cloned().unwrap_or_default();
            if let Some(node) = g.nodes.get(t) {
                sets.extend(g.refs(node, "HasPropertySets"));
            }
            owners.push((*t, sets));
        }
        for (_, sets) in owners {
            for set_id in sets {
                let Some(pset) = g.nodes.get(&set_id) else {
                    continue;
                };
                let set_name = g.name(pset).unwrap_or_default();
                if pset.kind != "IFCPROPERTYSET" || !set(&set_name) {
                    continue;
                }
                for prop_id in g.refs(pset, "HasProperties") {
                    let Some(prop) = g.nodes.get(&prop_id) else {
                        continue;
                    };
                    if g.name(prop).as_deref() != Some(name) {
                        continue;
                    }
                    let slot = match prop.kind.as_str() {
                        "IFCPROPERTYSINGLEVALUE" => "NominalValue",
                        "IFCPROPERTYENUMERATEDVALUE" => "EnumerationValues",
                        _ => continue,
                    };
                    if let Some(value) = g.attr(prop, slot) {
                        return Some((set_name, value));
                    }
                }
            }
        }
        None
    }
    /// Declared project length unit name and its scale in metres.
    pub fn length_unit(&self) -> (Option<String>, Option<f64>) {
        let g = self.g;
        let Some(id) = g.project_units.get("LENGTHUNIT") else {
            return (None, None);
        };
        let scale = g.units.get(id).map(|(_, s)| *s);
        let name = g.nodes.get(id).and_then(|n| match n.kind.as_str() {
            "IFCSIUNIT" => {
                let prefix = g.attr(n, "Prefix").and_then(text).unwrap_or_default();
                g.attr(n, "Name")
                    .and_then(text)
                    .map(|u| format!("{prefix}{u}"))
            }
            _ => g.name(n),
        });
        (name, scale)
    }
    pub fn summary(&self) -> Result<ModelSummary, String> {
        let g = self.g;
        let (length_unit, length_unit_scale) = self.length_unit();
        let mut summary = ModelSummary {
            project_name: g
                .nodes
                .values()
                .find(|n| n.kind == "IFCPROJECT")
                .and_then(|p| {
                    g.attr(p, "LongName")
                        .and_then(text)
                        .filter(|s| !s.is_empty())
                        .or_else(|| g.name(p))
                }),
            length_unit,
            length_unit_scale,
            entity_count: g.nodes.len(),
            element_count: self.elements.len(),
            ..Default::default()
        };
        let mut per_storey: HashMap<u32, usize> = HashMap::new();
        for n in &self.elements {
            tick(self.cancel)?;
            match g.storey_of(n.id) {
                Some(storey) => *per_storey.entry(storey).or_default() += 1,
                None => summary.without_storey += 1,
            }
            summary.proxy_count += usize::from(self.is(n, "IFCBUILDINGELEMENTPROXY"));
            summary.without_material += usize::from(!self.has_material(n));
            summary.without_nlsfb += usize::from(self.nlsfb(n).is_err());
            summary.without_type += usize::from(!self.has_type(n));
        }
        for n in g.nodes.values().filter(|n| n.kind == "IFCBUILDINGSTOREY") {
            summary.storeys.push(StoreySummary {
                express_id: n.id,
                global_id: g.attr(n, "GlobalId").and_then(text),
                name: g.name(n),
                elevation: g.attr(n, "Elevation").and_then(number),
                element_count: per_storey.get(&n.id).copied().unwrap_or(0),
            });
        }
        summary.storeys.sort_by(|a, b| {
            match (a.elevation, b.elevation) {
                (Some(x), Some(y)) => x.total_cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then(a.express_id.cmp(&b.express_id))
        });
        Ok(summary)
    }
}

pub fn is_nlsfb_system(name: &str) -> bool {
    let key: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    key.contains("NLSFB")
}
/// NL-SfB table-1 code: two digits, optionally `.` and one or two digits; parentheses allowed.
pub fn is_nlsfb_code(code: &str) -> bool {
    let c = code.trim();
    let c = c
        .strip_prefix('(')
        .and_then(|c| c.strip_suffix(')'))
        .unwrap_or(c);
    let digits = |s: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    match c.split_once('.') {
        Some((main, sub)) => digits(main, 2..=2) && digits(sub, 1..=2),
        None => digits(c, 2..=2),
    }
}

/// Accumulates one rule: each checked entity is counted once; each problem is one finding.
pub struct Rule<'a> {
    g: &'a Graph,
    r: SpecificationResult,
    failed: bool,
    unknown: bool,
}
impl<'a> Rule<'a> {
    pub fn new(
        g: &'a Graph,
        source: RuleSource,
        severity: Severity,
        code: &str,
        name: &str,
    ) -> Self {
        Self {
            g,
            r: SpecificationResult {
                name: name.into(),
                identifier: Some(code.into()),
                status: S::Passed,
                applicable_count: 0,
                passed_count: 0,
                failed_count: 0,
                findings: vec![],
                reasons: vec![],
                source,
                severity,
            },
            failed: false,
            unknown: false,
        }
    }
    pub fn check(&mut self, n: &Node, facet: &str, problems: Vec<String>) {
        self.r.applicable_count += 1;
        if problems.is_empty() {
            self.r.passed_count += 1;
            return;
        }
        self.r.failed_count += 1;
        for reason in problems {
            self.r
                .findings
                .push(self.g.finding(n, facet, S::Failed, reason));
        }
    }
    /// A rule-level failure without a single responsible entity.
    pub fn fail(&mut self, reason: String) {
        self.failed = true;
        self.r.reasons.push(reason);
    }
    pub fn note(&mut self, reason: String) {
        self.r.reasons.push(reason);
    }
    pub fn not_checkable(&mut self, reason: String) {
        self.unknown = true;
        self.r.reasons.push(reason);
    }
    pub fn severity(&mut self, severity: Severity) {
        self.r.severity = severity;
    }
    pub fn finish(mut self) -> SpecificationResult {
        self.r.status = if self.unknown {
            S::NotCheckable
        } else if self.failed || self.r.failed_count > 0 {
            S::Failed
        } else {
            S::Passed
        };
        self.r
    }
}

// buildingSMART ifc-gherkin-rules features/resources/spatial_CompositionTable.csv (SPS002):
// (applicable entity, permitted aggregating object).
const COMPOSITION: &[(&str, &str)] = &[
    ("IFCBRIDGEPART", "IFCBRIDGE"),
    ("IFCBRIDGEPART", "IFCBRIDGEPART"),
    ("IFCBRIDGE", "IFCPROJECT"),
    ("IFCBRIDGE", "IFCSITE"),
    ("IFCBRIDGE", "IFCBRIDGE"),
    ("IFCBUILDING", "IFCPROJECT"),
    ("IFCBUILDING", "IFCSITE"),
    ("IFCBUILDING", "IFCBUILDING"),
    ("IFCBUILDINGSTOREY", "IFCBUILDING"),
    ("IFCBUILDINGSTOREY", "IFCBUILDINGSTOREY"),
    ("IFCEXTERNALSPATIALELEMENT", "IFCPROJECT"),
    ("IFCEXTERNALSPATIALELEMENT", "IFCSITE"),
    ("IFCEXTERNALSPATIALELEMENT", "IFCEXTERNALSPATIALELEMENT"),
    ("IFCFACILITYPARTCOMMON", "IFCFACILITY"),
    ("IFCFACILITYPARTCOMMON", "IFCFACILITYPARTCOMMON"),
    ("IFCFACILITY", "IFCPROJECT"),
    ("IFCFACILITY", "IFCSITE"),
    ("IFCFACILITY", "IFCFACILITY"),
    ("IFCMARINEFACILITY", "IFCPROJECT"),
    ("IFCMARINEFACILITY", "IFCSITE"),
    ("IFCMARINEFACILITY", "IFCMARINEFACILITY"),
    ("IFCMARINEPART", "IFCMARINEFACILITY"),
    ("IFCMARINEPART", "IFCMARINEPART"),
    ("IFCRAILWAYPART", "IFCRAILWAY"),
    ("IFCRAILWAYPART", "IFCRAILWAYPART"),
    ("IFCRAILWAY", "IFCPROJECT"),
    ("IFCRAILWAY", "IFCSITE"),
    ("IFCRAILWAY", "IFCRAILWAY"),
    ("IFCROADPART", "IFCROAD"),
    ("IFCROADPART", "IFCROADPART"),
    ("IFCROAD", "IFCPROJECT"),
    ("IFCROAD", "IFCSITE"),
    ("IFCROAD", "IFCROAD"),
    ("IFCSITE", "IFCPROJECT"),
    ("IFCSITE", "IFCSITE"),
    ("IFCSPACE", "IFCPROJECT"),
    ("IFCSPACE", "IFCSITE"),
    ("IFCSPACE", "IFCBUILDING"),
    ("IFCSPACE", "IFCBUILDINGSTOREY"),
    ("IFCSPACE", "IFCSPACE"),
    ("IFCSPACE", "IFCFACILITY"),
    ("IFCSPACE", "IFCBRIDGE"),
    ("IFCSPACE", "IFCMARINEFACILITY"),
    ("IFCSPACE", "IFCRAILWAY"),
    ("IFCSPACE", "IFCROAD"),
    ("IFCSPACE", "IFCFACILITYPARTCOMMON"),
    ("IFCSPACE", "IFCBRIDGEPART"),
    ("IFCSPACE", "IFCMARINEPART"),
    ("IFCSPACE", "IFCRAILWAYPART"),
    ("IFCSPACE", "IFCROADPART"),
    ("IFCALIGNMENT", "IFCPROJECT"),
];

// buildingSMART features/resources/valid_ConversionBasedUnits.csv (PJS001), as SI scales.
const CONVERSION_UNITS: &[(&str, &str, f64)] = &[
    ("inch", "LENGTHUNIT", 0.0254),
    ("foot", "LENGTHUNIT", 0.3048),
    ("US survey foot", "LENGTHUNIT", 0.30480060960122),
    ("yard", "LENGTHUNIT", 0.9144),
    ("mile", "LENGTHUNIT", 1609.344),
    ("square inch", "AREAUNIT", 0.00064516),
    ("square foot", "AREAUNIT", 0.09290304),
    ("square yard", "AREAUNIT", 0.83612736),
    ("acre", "AREAUNIT", 4046.873),
    ("square mile", "AREAUNIT", 2589988.),
    ("cubic inch", "VOLUMEUNIT", 0.00001638706),
    ("cubic foot", "VOLUMEUNIT", 0.028316846592),
    ("cubic yard", "VOLUMEUNIT", 0.7645549),
    ("litre", "VOLUMEUNIT", 0.001),
    ("fluid ounce UK", "VOLUMEUNIT", 0.0000284130625),
    ("fluid ounce US", "VOLUMEUNIT", 0.00002957353),
    ("pint UK", "VOLUMEUNIT", 0.000568),
    ("pint US", "VOLUMEUNIT", 0.0004731765),
    ("gallon UK", "VOLUMEUNIT", 0.004546),
    ("gallon US", "VOLUMEUNIT", 0.003785412),
    ("degree", "PLANEANGLEUNIT", 0.017453292519943295),
];
const SI_BASE: &[(&str, &str)] = &[
    ("LENGTHUNIT", "METRE"),
    ("AREAUNIT", "SQUARE_METRE"),
    ("VOLUMEUNIT", "CUBIC_METRE"),
    ("PLANEANGLEUNIT", "RADIAN"),
];

type RuleFn = fn(&Model) -> Result<SpecificationResult, String>;
pub fn run(m: &Model) -> Result<Vec<SpecificationResult>, String> {
    let rules: [RuleFn; 16] = [
        project_presence,
        building_presence,
        spatial_breakdown,
        spatial_containment,
        global_id_format,
        global_id_unique,
        predefined_type,
        conversion_units,
        length_unit,
        other_units,
        pset_prefix,
        acyclic_groups,
        acyclic_decomposition,
        proxies,
        materials,
        types,
    ];
    let mut out = vec![];
    for rule in rules {
        tick(m.cancel)?;
        out.push(rule(m)?);
    }
    Ok(out)
}
fn rule<'a>(m: &Model<'a>, severity: Severity, code: &str, name: &str) -> Rule<'a> {
    Rule::new(m.g, RuleSource::ModelChecks, severity, code, name)
}

fn project_presence(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, Severity::Error, "PJS101", "Project presence");
    let projects: Vec<_> =
        m.g.nodes
            .values()
            .filter(|n| n.kind == "IFCPROJECT")
            .collect();
    for p in &projects {
        let problems = if projects.len() > 1 {
            vec![format!(
                "{} IfcProject instances; exactly one is required",
                projects.len()
            )]
        } else {
            vec![]
        };
        r.check(p, "project", problems);
    }
    if projects.is_empty() {
        r.fail("The model contains no IfcProject; exactly one is required".into());
    }
    Ok(r.finish())
}

// SPS001 is an IFC2X3 Coordination View rule at buildingSMART; for later schemas the building
// presence check is applied as an industry-practice warning.
fn building_presence(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let ifc2x3 = g.schema == ifc_lite_core::SchemaVersion::Ifc2x3;
    let mut r = rule(
        m,
        Severity::Error,
        "SPS001",
        "Basic spatial structure for buildings",
    );
    if !ifc2x3 {
        r.severity(Severity::Warning);
    }
    let mut buildings = 0;
    let sites: Vec<_> = g.nodes.values().filter(|n| m.is(n, "IFCSITE")).collect();
    for n in g.nodes.values() {
        tick(m.cancel)?;
        if m.is(n, "IFCBUILDING") || m.is(n, "IFCFACILITY") {
            buildings += 1;
            r.check(n, "spatial structure", vec![]);
        }
    }
    if ifc2x3 && sites.len() > 1 {
        for site in &sites {
            r.check(
                site,
                "spatial structure",
                vec![format!(
                    "{} IfcSite instances; at most one is allowed",
                    sites.len()
                )],
            );
        }
    }
    if buildings == 0 {
        r.fail("The model contains no IfcBuilding (or other facility)".into());
    }
    Ok(r.finish())
}

fn spatial_breakdown(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, Severity::Error, "SPS002", "Correct spatial breakdown");
    for n in m.g.nodes.values() {
        tick(m.cancel)?;
        let allowed: Vec<&'static str> = COMPOSITION
            .iter()
            .filter(|(child, _)| m.is(n, child))
            .map(|(_, parent)| *parent)
            .collect();
        if allowed.is_empty() {
            continue;
        }
        let mut names: Vec<_> = allowed.clone();
        names.sort_unstable();
        names.dedup();
        let parents = m.related(n.id, &["IFCRELAGGREGATES"]);
        let problems = match parents.as_slice() {
            [] => vec![format!(
                "{} is not aggregated into the spatial breakdown (allowed: {})",
                n.kind,
                names.join(", ")
            )],
            [p] if allowed.iter().any(|a| m.is(p, a)) => vec![],
            [p] => vec![format!(
                "{} is aggregated by {} #{}; allowed: {}",
                n.kind,
                p.kind,
                p.id,
                names.join(", ")
            )],
            many => vec![format!(
                "{} is aggregated by {} objects",
                n.kind,
                many.len()
            )],
        };
        r.check(n, "decomposition", problems);
    }
    Ok(r.finish())
}

fn spatial_containment(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(m, Severity::Error, "SPS007", "Spatial containment");
    let decomposition: &[&str] = if g.schema == ifc_lite_core::SchemaVersion::Ifc2x3 {
        &["IFCRELAGGREGATES", "IFCRELNESTS"]
    } else {
        &["IFCRELAGGREGATES"]
    };
    for n in g.nodes.values() {
        tick(m.cancel)?;
        if !m.is(n, "IFCPRODUCT") {
            continue;
        }
        let containers = m.related(n.id, &["IFCRELCONTAINEDINSPATIALSTRUCTURE"]);
        let mut problems = vec![];
        if containers.len() > 1 {
            problems.push(format!(
                "Contained in {} spatial structures; at most one is allowed",
                containers.len()
            ));
        }
        if m.is(n, "IFCELEMENT") {
            if m.is(n, "IFCFEATUREELEMENT") {
                // Openings and other features follow their host element.
            } else if m.part_of_element(n.id) {
                if !containers.is_empty() {
                    problems.push(
                        "Aggregated into another element and must not also be spatially contained"
                            .into(),
                    );
                }
            } else if containers.is_empty() && m.related(n.id, decomposition).is_empty() {
                problems.push("Not contained in a spatial structure element (no storey)".into());
            }
        } else if m.is(n, "IFCGRID") || m.is(n, "IFCANNOTATION") {
            let nested_annotation = m.is(n, "IFCANNOTATION")
                && m.related(n.id, &["IFCRELNESTS"])
                    .into_iter()
                    .any(|p| m.is(p, "IFCANNOTATION"));
            if containers.is_empty() && !nested_annotation {
                problems.push(format!(
                    "{} is not contained in a spatial structure element",
                    n.kind
                ));
            }
        } else if !containers.is_empty() {
            problems.push(format!(
                "{} must not be contained through IfcRelContainedInSpatialStructure",
                n.kind
            ));
        }
        r.check(n, "containment", problems);
    }
    Ok(r.finish())
}

/// IFC base64 GlobalId: 22 characters of `0-9A-Za-z_$`, first character 0-3 (128-bit range).
pub fn valid_global_id(id: &str) -> bool {
    id.len() == 22
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        && matches!(id.as_bytes()[0], b'0'..=b'3')
}

fn global_id_format(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(
        m,
        Severity::Error,
        "PJS003",
        "Globally unique identifier format",
    );
    for n in g.nodes.values() {
        tick(m.cancel)?;
        if !m.is(n, "IFCROOT") {
            continue;
        }
        let problems = match g.attr(n, "GlobalId").and_then(text) {
            None => vec!["GlobalId is missing".into()],
            Some(id) if !valid_global_id(&id) => vec![format!(
                "GlobalId '{id}' is not 22 characters of 0-9A-Za-z_$ starting with 0-3"
            )],
            Some(_) => vec![],
        };
        r.check(n, "GlobalId", problems);
    }
    Ok(r.finish())
}

fn global_id_unique(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(m, Severity::Error, "IfcRoot.UR1", "Unique GlobalId");
    let mut owners: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for n in g.nodes.values() {
        tick(m.cancel)?;
        if m.is(n, "IFCROOT") {
            if let Some(id) = g.attr(n, "GlobalId").and_then(text) {
                owners.entry(id).or_default().push(n.id);
            }
        }
    }
    for n in g.nodes.values() {
        tick(m.cancel)?;
        let Some(id) = g
            .attr(n, "GlobalId")
            .and_then(text)
            .filter(|_| m.is(n, "IFCROOT"))
        else {
            continue;
        };
        let others: Vec<_> = owners[&id]
            .iter()
            .filter(|o| **o != n.id)
            .map(|o| format!("#{o}"))
            .collect();
        let problems = if others.is_empty() {
            vec![]
        } else {
            vec![format!(
                "GlobalId '{id}' is also used by {}",
                others.join(", ")
            )]
        };
        r.check(n, "GlobalId", problems);
    }
    Ok(r.finish())
}

fn predefined_type(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(m, Severity::Error, "OJT001", "Object predefined type");
    let filled = |n: &Node, names: &[&str]| -> Option<bool> {
        let present: Vec<_> = names.iter().filter_map(|a| g.attr(n, a)).collect();
        (!present.is_empty()).then(|| {
            present
                .iter()
                .any(|a| text(a).is_some_and(|s| !s.trim().is_empty()))
        })
    };
    for n in g.nodes.values() {
        tick(m.cancel)?;
        let Some(own) = g.attr(n, "PredefinedType") else {
            continue;
        };
        let own = text(own);
        let mut problems = vec![];
        if m.is(n, "IFCTYPEOBJECT") {
            if own.as_deref() == Some("USERDEFINED")
                && filled(n, &["ElementType", "ProcessType", "ResourceType"]) == Some(false)
            {
                problems.push("USERDEFINED PredefinedType without ElementType".into());
            }
        } else if m.is(n, "IFCOBJECT") {
            let mut type_value = None;
            for t in g.types.get(&n.id).into_iter().flatten() {
                let Some(t) = g.nodes.get(t) else { continue };
                let value = g.attr(t, "PredefinedType").and_then(text);
                if let Some(value) = value.filter(|v| v != "NOTDEFINED") {
                    if let Some(o) = own.as_deref().filter(|o| *o != "NOTDEFINED" && *o != value) {
                        problems.push(format!(
                            "PredefinedType {o} overrides {} #{} PredefinedType {value}",
                            t.kind, t.id
                        ));
                    }
                    type_value = Some(value);
                }
            }
            if own.as_deref() == Some("USERDEFINED")
                && type_value.is_none()
                && filled(n, &["ObjectType"]) == Some(false)
            {
                problems.push("USERDEFINED PredefinedType without ObjectType".into());
            }
        } else {
            continue;
        }
        r.check(n, "PredefinedType", problems);
    }
    Ok(r.finish())
}

fn conversion_units(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(
        m,
        Severity::Error,
        "PJS001",
        "Correct conversion based units",
    );
    let mut units: Vec<_> = g.project_units.iter().collect();
    units.sort_by_key(|(_, id)| **id);
    for (unit_type, id) in units {
        let Some(n) = g.nodes.get(id) else { continue };
        let Some((_, si_name)) = SI_BASE.iter().find(|(t, _)| t == unit_type) else {
            continue;
        };
        if n.kind != "IFCCONVERSIONBASEDUNIT" {
            continue;
        }
        let mut problems = vec![];
        let name = g.name(n).unwrap_or_default();
        let known = CONVERSION_UNITS
            .iter()
            .find(|(u, t, _)| t == unit_type && u.eq_ignore_ascii_case(name.trim()));
        if known.is_none() {
            let names: Vec<_> = CONVERSION_UNITS
                .iter()
                .filter(|(_, t, _)| t == unit_type)
                .map(|(u, _, _)| *u)
                .collect();
            problems.push(format!(
                "{unit_type} name '{name}' is not a recognised conversion-based unit ({})",
                names.join(", ")
            ));
        }
        let component = g
            .refs(n, "ConversionFactor")
            .first()
            .and_then(|f| g.nodes.get(f))
            .and_then(|f| g.refs(f, "UnitComponent").first().copied())
            .and_then(|c| g.nodes.get(&c));
        match component {
            Some(c) if c.kind == "IFCSIUNIT" => {
                let base = g.attr(c, "Name").and_then(text).unwrap_or_default();
                if base != *si_name {
                    problems.push(format!(
                        "Conversion factor is based on {base}; expected {si_name}"
                    ));
                }
            }
            _ => problems.push(format!(
                "Conversion factor is not based on the SI unit {si_name}"
            )),
        }
        match (g.units.get(id), known) {
            (Some((_, scale)), Some((u, _, expected)))
                if ((scale - expected) / expected).abs() > 1e-4 =>
            {
                problems.push(format!(
                    "Conversion factor gives {scale} {si_name} per {u}; expected {expected}"
                ))
            }
            (None, _) => problems.push("Conversion factor cannot be resolved".into()),
            _ => {}
        }
        r.check(n, "units", problems);
    }
    Ok(r.finish())
}

fn project<'a>(m: &Model<'a>) -> Option<&'a Node> {
    let g: &'a Graph = m.g;
    let mut projects = g.nodes.values().filter(|n| n.kind == "IFCPROJECT");
    let first = projects.next();
    if projects.next().is_some() {
        return None;
    }
    first
}
fn length_unit(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, Severity::Error, "OBV-UNIT01", "Project length unit");
    let Some(p) = project(m) else {
        r.not_checkable("Project units require exactly one IfcProject".into());
        return Ok(r.finish());
    };
    let (name, scale) = m.length_unit();
    let problems = match (&name, scale) {
        (None, _) => vec!["IfcUnitAssignment declares no LENGTHUNIT".into()],
        (Some(name), None) => vec![format!("Length unit {name} cannot be resolved")],
        (Some(name), Some(scale)) => {
            r.note(format!("Length unit: {name} ({scale} m)"));
            vec![]
        }
    };
    r.check(p, "units", problems);
    Ok(r.finish())
}
fn other_units(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(
        m,
        Severity::Warning,
        "OBV-UNIT02",
        "Project area, volume and angle units",
    );
    let Some(p) = project(m) else {
        r.not_checkable("Project units require exactly one IfcProject".into());
        return Ok(r.finish());
    };
    let problems = ["AREAUNIT", "VOLUMEUNIT", "PLANEANGLEUNIT"]
        .iter()
        .filter(|u| !m.g.project_units.contains_key(**u))
        .map(|u| format!("IfcUnitAssignment declares no {u}"))
        .collect();
    r.check(p, "units", problems);
    Ok(r.finish())
}

fn pset_prefix(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(
        m,
        Severity::Warning,
        "PSE002",
        "Custom property set prefixes",
    );
    let reserved = |name: &str, prefix: &str| {
        let p = prefix.trim_end_matches('_');
        !name.starts_with(prefix)
            && name
                .as_bytes()
                .get(..p.len())
                .is_some_and(|b| b.eq_ignore_ascii_case(p.as_bytes()))
    };
    for n in g.nodes.values() {
        tick(m.cancel)?;
        let prefix = match n.kind.as_str() {
            "IFCPROPERTYSET" => "Pset_",
            "IFCELEMENTQUANTITY" => "Qto_",
            _ => continue,
        };
        let name = g.name(n).unwrap_or_default();
        let problems = if reserved(&name, prefix) {
            vec![format!(
                "'{name}' uses the reserved {prefix} prefix with different casing; standard sets start with exactly {prefix}, custom sets need another prefix"
            )]
        } else {
            vec![]
        };
        r.check(n, "property set", problems);
    }
    Ok(r.finish())
}

/// Nodes on a directed cycle (strongly connected component of size > 1, or a self-loop).
/// Iterative Tarjan; deterministic over BTreeMap order.
fn cyclic(edges: &BTreeMap<u32, Vec<u32>>, cancel: &AtomicBool) -> Result<BTreeSet<u32>, String> {
    let (mut index, mut low) = (HashMap::new(), HashMap::new());
    let (mut on_stack, mut stack, mut out, mut next) = (HashSet::new(), vec![], BTreeSet::new(), 0);
    for &root in edges.keys() {
        if index.contains_key(&root) {
            continue;
        }
        let mut work: Vec<(u32, usize)> = vec![(root, 0)];
        index.insert(root, next);
        low.insert(root, next);
        next += 1;
        stack.push(root);
        on_stack.insert(root);
        while let Some(&(v, i)) = work.last() {
            tick(cancel)?;
            let successors = edges.get(&v).map(Vec::as_slice).unwrap_or(&[]);
            if let Some(&w) = successors.get(i) {
                work.last_mut().expect("frame").1 += 1;
                if let std::collections::hash_map::Entry::Vacant(e) = index.entry(w) {
                    e.insert(next);
                    low.insert(w, next);
                    next += 1;
                    stack.push(w);
                    on_stack.insert(w);
                    work.push((w, 0));
                } else if on_stack.contains(&w) {
                    let lw = index[&w];
                    let lv = low.get_mut(&v).expect("visited");
                    *lv = (*lv).min(lw);
                }
                continue;
            }
            work.pop();
            if let Some(&(parent, _)) = work.last() {
                let lv = low[&v];
                let lp = low.get_mut(&parent).expect("visited");
                *lp = (*lp).min(lv);
            }
            if low[&v] == index[&v] {
                let mut component = vec![];
                while let Some(w) = stack.pop() {
                    on_stack.remove(&w);
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                if component.len() > 1 || successors.contains(&v) {
                    out.extend(component);
                }
            }
        }
    }
    Ok(out)
}
fn acyclic(
    m: &Model,
    relations: &[&str],
    mut r: Rule,
    facet: &str,
) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut edges: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (child, parents) in &g.parents {
        for (relation, parent) in parents {
            if relations.contains(&relation.as_str()) {
                edges.entry(*child).or_default().push(*parent);
            }
        }
    }
    let cycles = cyclic(&edges, m.cancel)?;
    for id in edges.keys() {
        let Some(n) = g.nodes.get(id) else { continue };
        let problems = if cycles.contains(id) {
            vec![format!(
                "Part of a cyclic {} relationship",
                relations.join("/")
            )]
        } else {
            vec![]
        };
        r.check(n, facet, problems);
    }
    Ok(r.finish())
}
fn acyclic_groups(m: &Model) -> Result<SpecificationResult, String> {
    let r = rule(m, Severity::Error, "GRP001", "Acyclic groups");
    acyclic(m, &["IFCRELASSIGNSTOGROUP"], r, "group")
}
fn acyclic_decomposition(m: &Model) -> Result<SpecificationResult, String> {
    let r = rule(
        m,
        Severity::Error,
        "OBV-CYCLE",
        "Acyclic aggregation and nesting",
    );
    acyclic(m, &["IFCRELAGGREGATES", "IFCRELNESTS"], r, "decomposition")
}

/// Shared by the model checks and the ILS profile.
pub fn proxy_rule<'a>(m: &Model<'a>, mut r: Rule<'a>) -> Result<SpecificationResult, String> {
    let mut count = 0;
    for n in &m.elements {
        tick(m.cancel)?;
        let proxy = m.is(n, "IFCBUILDINGELEMENTPROXY");
        count += usize::from(proxy);
        let problems = if proxy {
            vec!["IfcBuildingElementProxy; use a specific IFC class where one exists".into()]
        } else {
            vec![]
        };
        r.check(n, "entity", problems);
    }
    if !m.elements.is_empty() {
        r.note(format!(
            "{count} of {} elements ({:.1}%) are IfcBuildingElementProxy",
            m.elements.len(),
            100.0 * count as f64 / m.elements.len() as f64
        ));
    }
    Ok(r.finish())
}
fn proxies(m: &Model) -> Result<SpecificationResult, String> {
    let r = rule(m, Severity::Warning, "OBV-PROXY", "Proxy elements");
    proxy_rule(m, r)
}
pub fn material_rule<'a>(m: &Model<'a>, mut r: Rule<'a>) -> Result<SpecificationResult, String> {
    for n in &m.elements {
        tick(m.cancel)?;
        let problems = if m.has_material(n) {
            vec![]
        } else {
            vec!["No material association (element, type or parts)".into()]
        };
        r.check(n, "material", problems);
    }
    Ok(r.finish())
}
fn materials(m: &Model) -> Result<SpecificationResult, String> {
    let r = rule(
        m,
        Severity::Warning,
        "OBV-MATERIAL",
        "Elements with material",
    );
    material_rule(m, r)
}
fn types(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, Severity::Warning, "OBV-TYPE", "Elements with type");
    for n in &m.elements {
        tick(m.cancel)?;
        let problems = if m.has_type(n) {
            vec![]
        } else {
            vec!["No type object (IfcRelDefinesByType)".into()]
        };
        r.check(n, "type", problems);
    }
    Ok(r.finish())
}
