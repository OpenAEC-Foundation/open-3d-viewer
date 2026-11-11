use crate::graph::{text, Graph, Node};
use crate::values::{matches, supported, value_matches};
use crate::{combine, Finding, SpecificationResult, ValidationReport, ValidationStatus as S};
use ifc_lite_core::AttributeValue as A;
use openbim_ids::{Entity, Facet, Ids, Occurrence, Value};
use std::collections::{BTreeMap, HashSet};

// Presence is separate from matching: optional never means that a present wrong value passes.
struct Check {
    present: bool,
    matches: bool,
}
impl Check {
    fn new(present: bool, matches: bool) -> Self {
        Self { present, matches }
    }
}
pub fn validate(
    g: &Graph,
    ids: &Ids,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ValidationReport, String> {
    let mut specifications = Vec::new();
    for spec in &ids.specifications {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Validation cancelled".into());
        }
        let mut r = SpecificationResult {
            name: spec.name.clone(),
            identifier: spec.identifier.clone(),
            status: S::Passed,
            applicable_count: 0,
            passed_count: 0,
            failed_count: 0,
            findings: vec![],
            reasons: vec![],
            source: crate::RuleSource::Ids,
            severity: crate::Severity::Error,
        };
        if !spec.ifc_versions.iter().any(|v| v.as_str() == g.label) {
            r.status = S::NotCheckable;
            r.reasons.push(format!(
                "Specification does not target IFC schema {}",
                g.label
            ));
            specifications.push(r);
            continue;
        }
        let facets = spec.applicability.facets.iter().chain(
            spec.requirements
                .iter()
                .flat_map(|r| r.facets.iter().map(|r| &r.facet)),
        );
        for f in facets {
            if let Err(reason) = support(f) {
                r.status = S::NotCheckable;
                r.reasons.push(reason)
            }
        }
        if !r.reasons.is_empty() {
            r.reasons.sort();
            r.reasons.dedup();
            specifications.push(r);
            continue;
        }
        for n in g.nodes.values() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("Validation cancelled".into());
            }
            let mut applicable = true;
            let mut unknown = None;
            for f in &spec.applicability.facets {
                match check(g, n, f) {
                    Ok(c) if !c.matches => {
                        applicable = false;
                        break;
                    }
                    Err(e) => unknown = Some(e),
                    _ => (),
                }
            }
            if !applicable {
                continue;
            }
            if let Some(reason) = unknown {
                r.status = S::NotCheckable;
                r.findings
                    .push(finding(g, n, "applicability", S::NotCheckable, reason));
                continue;
            }
            r.applicable_count += 1;
            let mut entity_status = S::Passed;
            if spec.applicability.max_occurs == Some(0) {
                entity_status = S::Failed;
                r.findings.push(finding(
                    g,
                    n,
                    "applicability",
                    S::Failed,
                    "Prohibited applicable entity is present".into(),
                ));
            } else if let Some(requirements) = &spec.requirements {
                for req in &requirements.facets {
                    match check(g, n, &req.facet) {
                        Ok(c) => {
                            let pass = match req.occurrence {
                                Occurrence::Required => c.matches,
                                Occurrence::Optional => !c.present || c.matches,
                                Occurrence::Prohibited => !c.matches,
                            };
                            if !pass {
                                entity_status = combine(entity_status, S::Failed);
                                r.findings.push(finding(
                                    g,
                                    n,
                                    req.facet.kind(),
                                    S::Failed,
                                    format!(
                                        "{} requirement is not satisfied ({:?})",
                                        req.facet.kind(),
                                        req.occurrence
                                    ),
                                ));
                            }
                        }
                        Err(reason) => {
                            entity_status = combine(entity_status, S::NotCheckable);
                            r.findings.push(finding(
                                g,
                                n,
                                req.facet.kind(),
                                S::NotCheckable,
                                reason,
                            ));
                        }
                    }
                }
            }
            match entity_status {
                S::Passed => r.passed_count += 1,
                S::Failed => r.failed_count += 1,
                _ => (),
            }
            r.status = combine(r.status, entity_status);
        }
        if r.applicable_count < spec.applicability.min_occurs as usize
            || spec
                .applicability
                .max_occurs
                .is_some_and(|max| r.applicable_count > max as usize)
        {
            r.status = combine(r.status, S::Failed);
            r.reasons.push(format!(
                "Applicable entity count {} is outside {}..{}",
                r.applicable_count,
                spec.applicability.min_occurs,
                spec.applicability
                    .max_occurs
                    .map_or("unbounded".into(), |v| v.to_string())
            ));
        }
        specifications.push(r);
    }
    let status = specifications
        .iter()
        .fold(S::Passed, |s, r| combine(s, r.status));
    Ok(ValidationReport {
        schema: g.label.clone(),
        ids_title: ids.info.title.clone(),
        status,
        specifications,
        model: None,
    })
}
fn finding(g: &Graph, n: &Node, facet: &str, status: S, reason: String) -> Finding {
    g.finding(n, facet, status, reason)
}
fn support(f: &Facet) -> Result<(), String> {
    if let Facet::PartOf(p) = f {
        if p.relation == Some(openbim_ids::Relation::VoidsElementFillsElement) {
            return Err("Combined void/fill partOf traversal is not yet supported".into());
        }
    }
    let values: Vec<&Value> = match f {
        Facet::Entity(e) => std::iter::once(&e.name)
            .chain(e.predefined_type.iter())
            .collect(),
        Facet::Attribute(a) => std::iter::once(&a.name).chain(a.value.iter()).collect(),
        Facet::Property(p) => std::iter::once(&p.property_set)
            .chain(std::iter::once(&p.base_name))
            .chain(p.value.iter())
            .collect(),
        Facet::Classification(c) => std::iter::once(&c.system).chain(c.value.iter()).collect(),
        Facet::Material(m) => m.value.iter().collect(),
        Facet::PartOf(p) => std::iter::once(&p.entity.name)
            .chain(p.entity.predefined_type.iter())
            .collect(),
    };
    for v in values {
        supported(v)?;
    }
    Ok(())
}
fn entity(g: &Graph, n: &Node, e: &Entity) -> Result<bool, String> {
    let name_match = match &e.name {
        Value::Simple(name) => g.schema.is_subtype_of(&n.kind, name),
        v => matches(v, &n.kind)?,
    };
    let mut name_match = name_match;
    if !name_match && g.schema == ifc_lite_core::SchemaVersion::Ifc2x3 {
        if let Value::Simple(name) = &e.name {
            for id in g.inherited_ids(n.id)?.into_iter().skip(1) {
                let obj = g.nodes.get(&id).ok_or("Dangling type reference")?;
                if obj.kind.strip_suffix("TYPE") == Some(name.as_str())
                    && ifc_lite_core::SchemaVersion::Ifc4.is_subtype_of(name, &n.kind)
                {
                    name_match = true;
                }
            }
        }
    }
    if !name_match {
        return Ok(false);
    }
    if let Some(expected) = &e.predefined_type {
        for id in g.inherited_ids(n.id)?.into_iter().rev() {
            let obj = g.nodes.get(&id).ok_or("Dangling IFC type reference")?;
            if let Some(a) = g.attr(obj, "PredefinedType") {
                if let Some(mut s) = text(a) {
                    if s == "NOTDEFINED" {
                        continue;
                    }
                    if s == "USERDEFINED" {
                        if matches(expected, &s)? {
                            return Ok(true);
                        }
                        s = ["ObjectType", "ElementType", "ProcessType"]
                            .iter()
                            .find_map(|name| g.attr(obj, name).and_then(text))
                            .unwrap_or_default();
                    }
                    return matches(expected, &s);
                }
            }
        }
        return Ok(false);
    }
    Ok(true)
}
fn check(g: &Graph, n: &Node, f: &Facet) -> Result<Check, String> {
    match f {
        Facet::Entity(e) => Ok(Check::new(true, entity(g, n, e)?)),
        Facet::Attribute(a) => {
            let names = g
                .schema
                .attribute_names(&n.kind)
                .ok_or("Entity is unknown in declared IFC schema")?;
            let (mut present, mut ok) = (false, false);
            for (i, name) in names.iter().enumerate() {
                if matches(&a.name, name)? {
                    if let Some(v) = n.attrs.get(i) {
                        if !matches!(v, A::Null) {
                            present = true;
                            if let A::EntityRef(id) = v {
                                if !g.nodes.contains_key(id) {
                                    return Err("Dangling attribute reference".into());
                                }
                                if a.value.is_none() {
                                    ok = true;
                                } else {
                                    return Err(
                                        "Reference attribute value matching is unsupported".into(),
                                    );
                                }
                            } else {
                                ok |= value_matches(&a.value, v)?;
                            }
                        }
                    }
                }
            }
            Ok(Check::new(present, ok))
        }
        Facet::Property(p) => property(g, n, p),
        Facet::Material(m) => material(g, n, &m.value),
        Facet::Classification(c) => classification(g, n, c),
        Facet::PartOf(p) => {
            if p.relation == Some(openbim_ids::Relation::VoidsElementFillsElement) {
                return Err("Combined void/fill partOf traversal is not yet supported".into());
            }
            ensure_acyclic_parents(g, n.id)?;
            let mut queue = vec![n.id];
            let mut seen = HashSet::new();
            let mut present = false;
            while let Some(id) = queue.pop() {
                if !seen.insert(id) {
                    continue;
                }
                if seen.len() > 100_000 {
                    return Err("partOf traversal exceeds limit".into());
                }
                for (relation, parent) in g.parents.get(&id).into_iter().flatten() {
                    if p.relation.is_some_and(|r| r.as_str() != relation) {
                        continue;
                    }
                    present = true;
                    let obj = g.nodes.get(parent).ok_or("Dangling partOf reference")?;
                    if (relation != "IFCRELASSIGNSTOGROUP" || matches(&p.entity.name, &obj.kind)?)
                        && entity(g, obj, &p.entity)?
                    {
                        return Ok(Check::new(true, true));
                    }
                    queue.push(*parent);
                }
            }
            Ok(Check::new(present, false))
        }
    }
}
fn property(g: &Graph, n: &Node, p: &openbim_ids::Property) -> Result<Check, String> {
    let mut matched_sets: BTreeMap<String, bool> = BTreeMap::new();
    let mut props: BTreeMap<(String, String), &Node> = BTreeMap::new();
    for id in g.inherited_ids(n.id)?.into_iter().rev() {
        let owner = g.nodes.get(&id).ok_or("Dangling type reference")?;
        let mut owner_names = HashSet::new();
        let sets = g
            .psets
            .get(&id)
            .into_iter()
            .flatten()
            .copied()
            .chain(g.refs(owner, "HasPropertySets"));
        for set_id in sets {
            let set = g
                .nodes
                .get(&set_id)
                .ok_or("Dangling property set reference")?;
            let set_name = g.attr(set, "Name").and_then(text).unwrap_or_default();
            if !matches(&p.property_set, &set_name)? {
                continue;
            }
            if !matches!(
                set.kind.as_str(),
                "IFCPROPERTYSET"
                    | "IFCELEMENTQUANTITY"
                    | "IFCMATERIALPROPERTIES"
                    | "IFCEXTENDEDMATERIALPROPERTIES"
                    | "IFCPROFILEPROPERTIES"
            ) {
                return Err(format!(
                    "Predefined property set {} is not yet supported",
                    set.kind
                ));
            }
            matched_sets.insert(set_name.clone(), false);
            for prop_id in g
                .refs(set, "HasProperties")
                .into_iter()
                .chain(g.refs(set, "Quantities"))
                .chain(g.refs(set, "Properties"))
                .chain(g.refs(set, "ExtendedProperties"))
            {
                let prop = g.nodes.get(&prop_id).ok_or("Dangling property reference")?;
                let name = g.attr(prop, "Name").and_then(text).unwrap_or_default();
                if matches(&p.base_name, &name)? {
                    matched_sets.insert(set_name.clone(), true);
                    if !owner_names.insert((set_name.clone(), name.clone())) {
                        return Err(
                            "Ambiguous duplicate property names on the same IFC owner".into()
                        );
                    }
                    props.insert((set_name.clone(), name), prop);
                }
            }
        }
    }
    let mut present = false;
    let mut ok = matched_sets.values().all(|v| *v);
    for prop in props.values() {
        let (slot, inferred_type) = match prop.kind.as_str() {
            "IFCPROPERTYSINGLEVALUE" => ("NominalValue", None),
            "IFCPROPERTYENUMERATEDVALUE" => ("EnumerationValues", None),
            "IFCPROPERTYLISTVALUE" => ("ListValues", None),
            "IFCPROPERTYBOUNDEDVALUE" => ("UpperBoundValue", None),
            "IFCPROPERTYTABLEVALUE" => ("DefiningValues", None),
            "IFCQUANTITYLENGTH" => ("LengthValue", Some("IFCLENGTHMEASURE")),
            "IFCQUANTITYAREA" => ("AreaValue", Some("IFCAREAMEASURE")),
            "IFCQUANTITYVOLUME" => ("VolumeValue", Some("IFCVOLUMEMEASURE")),
            "IFCQUANTITYWEIGHT" => ("WeightValue", Some("IFCMASSMEASURE")),
            "IFCQUANTITYTIME" => ("TimeValue", Some("IFCTIMEMEASURE")),
            "IFCQUANTITYCOUNT" => ("CountValue", Some("IFCCOUNTMEASURE")),
            _ => {
                return Err(format!(
                    "Property representation {} is not yet supported",
                    prop.kind
                ))
            }
        };
        let a = g.attr(prop, slot).ok_or("Missing property value slot")?;
        if matches!(a, A::Null)
            && !matches!(
                prop.kind.as_str(),
                "IFCPROPERTYBOUNDEDVALUE" | "IFCPROPERTYTABLEVALUE"
            )
        {
            ok = false;
            continue;
        }
        let mut candidates = match a {
            A::List(v) => v.iter().collect::<Vec<_>>(),
            _ => vec![a],
        };
        if prop.kind == "IFCPROPERTYBOUNDEDVALUE" {
            for slot in ["LowerBoundValue", "SetPointValue"] {
                if let Some(a) = g.attr(prop, slot) {
                    candidates.push(a);
                }
            }
        }
        if prop.kind == "IFCPROPERTYTABLEVALUE" {
            if ["DefiningUnit", "DefinedUnit"]
                .iter()
                .any(|slot| g.attr(prop, slot).is_some_and(|v| !matches!(v, A::Null)))
            {
                return Err("Explicit table property units are not yet supported".into());
            }
            if let Some(A::List(v)) = g.attr(prop, "DefinedValues") {
                candidates.extend(v);
            }
        }
        candidates.retain(|a| !matches!(a, A::Null));
        present |= !candidates.is_empty();
        if candidates.is_empty() {
            ok = false;
            continue;
        }
        let mut any = false;
        for a in candidates {
            let dtype = if let A::Typed { name, .. } = a {
                Some(name.as_ref())
            } else {
                inferred_type
            };
            if let Some(required) = &p.data_type {
                if dtype != Some(required.as_str()) {
                    continue;
                }
            }
            if p.value.is_none() {
                any |= value_matches(&None, a)?;
                continue;
            }
            let normalized = if let Some(dtype) = dtype {
                normalize(g, prop, a, dtype)?
            } else {
                a.clone()
            };
            any |= value_matches(&p.value, &normalized)?;
        }
        ok &= any;
    }
    Ok(Check::new(present, present && ok))
}
fn material(g: &Graph, n: &Node, expected: &Option<Value>) -> Result<Check, String> {
    let mut ids = vec![];
    for id in g.inherited_ids(n.id)? {
        if let Some(v) = g.materials.get(&id) {
            ids.extend(v);
            break;
        }
    }
    let present = !ids.is_empty();
    let mut seen = HashSet::new();
    let mut ok = false;
    while let Some(id) = ids.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > 100_000 {
            return Err("Material traversal exceeds limit".into());
        }
        let obj = g.nodes.get(&id).ok_or("Dangling material reference")?;
        if !obj.kind.starts_with("IFCMATERIAL") {
            return Err("Unsupported material selection".into());
        }
        if expected.is_none() {
            ok = true
        }
        for name in ["Name", "Category", "LayerSetName"] {
            if let Some(a) = g.attr(obj, name) {
                ok |= value_matches(expected, a)?;
            }
        }
        for name in [
            "Material",
            "Materials",
            "MaterialLayers",
            "MaterialProfiles",
            "MaterialConstituents",
            "ForLayerSet",
            "ForProfileSet",
        ] {
            ids.extend(g.refs(obj, name));
        }
    }
    Ok(Check::new(present, ok))
}
fn classification(g: &Graph, n: &Node, c: &openbim_ids::Classification) -> Result<Check, String> {
    let mut present = false;
    let mut ok = false;
    for owner in g.inherited_ids(n.id)? {
        for id in g.classifications.get(&owner).into_iter().flatten() {
            present = true;
            let mut current = *id;
            let mut seen = HashSet::new();
            let mut codes: Vec<String> = vec![];
            loop {
                if !seen.insert(current) {
                    return Err("Cyclic classification reference".into());
                }
                if seen.len() > 1024 {
                    return Err("Classification depth exceeds limit".into());
                }
                let obj = g
                    .nodes
                    .get(&current)
                    .ok_or("Dangling classification reference")?;
                if obj.kind == "IFCCLASSIFICATION" {
                    let system = g.attr(obj, "Name").and_then(text).unwrap_or_default();
                    if matches(&c.system, &system)? {
                        present = true;
                        if let Some(expected) = &c.value {
                            for code in &codes {
                                ok |= matches(expected, code)?;
                            }
                        } else {
                            ok = true
                        }
                    }
                    break;
                }
                if obj.kind != "IFCCLASSIFICATIONREFERENCE" {
                    return Err("Unsupported classification selection".into());
                }
                if let Some(code) = g
                    .attr(obj, "Identification")
                    .or_else(|| g.attr(obj, "ItemReference"))
                    .and_then(text)
                {
                    codes.push(code)
                }
                current = *g
                    .refs(obj, "ReferencedSource")
                    .first()
                    .ok_or("Classification reference has no system")?;
            }
        }
    }
    Ok(Check::new(present, ok))
}

fn normalize(g: &Graph, prop: &Node, a: &A, dtype: &str) -> Result<A, String> {
    use ifc_lite_core::{measure_unit, MeasureUnit};
    match measure_unit(dtype) {
        Some(MeasureUnit::Typed { unit_type, .. }) => {
            if unit_type == "THERMODYNAMICTEMPERATUREUNIT" {
                return Err("Temperature offset normalization is not yet supported".into());
            }
            let explicit = g.refs(prop, "Unit");
            let id = explicit
                .first()
                .or_else(|| g.project_units.get(unit_type))
                .ok_or_else(|| format!("No unambiguous declared {unit_type} for normalization"))?;
            let (kind, scale) = g.units.get(id).ok_or("Unresolved IFC unit")?;
            if kind != unit_type {
                return Err("Property unit type does not match its value type".into());
            }
            let v = a.as_float().ok_or("Measure value is not numeric")? * scale;
            if !v.is_finite() {
                return Err("Normalized measure is not finite".into());
            }
            Ok(A::Float(v))
        }
        Some(MeasureUnit::Monetary) => Err("Monetary conversion is not supported".into()),
        None if dtype.ends_with("MEASURE") => Err(format!("Unknown IFC measure type {dtype}")),
        _ => Ok(a.clone()),
    }
}

fn ensure_acyclic_parents(g: &Graph, start: u32) -> Result<(), String> {
    let mut active = HashSet::new();
    let mut done = HashSet::new();
    let mut stack = vec![(start, false)];
    while let Some((id, exit)) = stack.pop() {
        if exit {
            active.remove(&id);
            done.insert(id);
            continue;
        }
        if done.contains(&id) {
            continue;
        }
        if !active.insert(id) {
            return Err("Cyclic partOf graph cannot be evaluated".into());
        }
        if active.len() + done.len() > 100_000 {
            return Err("partOf traversal exceeds limit".into());
        }
        stack.push((id, true));
        for (_, parent) in g.parents.get(&id).into_iter().flatten() {
            stack.push((*parent, false));
        }
    }
    Ok(())
}
