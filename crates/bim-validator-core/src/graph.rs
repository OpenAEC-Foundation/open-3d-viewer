use ifc_lite_core::{
    declared_schema_bounded, parse_entity, AttributeValue as A, EntityScanner, SchemaVersion,
};
use std::collections::{BTreeMap, HashMap};

pub struct Node {
    pub id: u32,
    pub kind: String,
    pub attrs: Vec<A>,
}
pub struct Graph {
    pub schema: SchemaVersion,
    pub label: String,
    pub nodes: BTreeMap<u32, Node>,
    pub types: HashMap<u32, Vec<u32>>,
    pub psets: HashMap<u32, Vec<u32>>,
    pub materials: HashMap<u32, Vec<u32>>,
    pub classifications: HashMap<u32, Vec<u32>>,
    pub units: HashMap<u32, (String, f64)>,
    pub project_units: HashMap<String, u32>,
    pub parents: HashMap<u32, Vec<(String, u32)>>,
}
impl Graph {
    pub fn parse(bytes: &[u8], cancel: &std::sync::atomic::AtomicBool) -> Result<Self, String> {
        if bytes.len() > 2 * 1024 * 1024 * 1024usize {
            return Err("IFC exceeds the 2 GiB input limit".into());
        }
        bounded_nesting(bytes, cancel)?;
        let label = declared_schema_bounded(bytes)
            .ok_or("Missing IFC FILE_SCHEMA")?
            .to_ascii_uppercase();
        let schema = SchemaVersion::from_file_schema(&label)
            .ok_or_else(|| format!("Unsupported IFC schema: {label}"))?;
        let mut g = Self {
            schema,
            label,
            nodes: BTreeMap::new(),
            types: HashMap::new(),
            psets: HashMap::new(),
            materials: HashMap::new(),
            classifications: HashMap::new(),
            parents: HashMap::new(),
            units: HashMap::new(),
            project_units: HashMap::new(),
        };
        let mut scanner = EntityScanner::new(bytes);
        while let Some((id, kind, start, end)) = scanner.next_entity() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("Validation cancelled".into());
            }
            if g.nodes.len() >= 5_000_000 {
                return Err("IFC exceeds the entity limit".into());
            }
            let (_, _, tokens) = parse_entity(&bytes[start..end])
                .map_err(|e| format!("Invalid STEP entity #{id}: {e}"))?;
            let layout = schema.attribute_names(kind).ok_or_else(|| {
                format!("Entity {kind} is unknown in declared schema {}", g.label)
            })?;
            if tokens.len() != layout.len() {
                return Err(format!(
                    "Entity #{id} ({kind}) has {} attributes; schema requires {}",
                    tokens.len(),
                    layout.len()
                ));
            }
            let attrs = tokens.iter().map(A::from_token).collect();
            if g.nodes
                .insert(
                    id,
                    Node {
                        id,
                        kind: kind.to_ascii_uppercase(),
                        attrs,
                    },
                )
                .is_some()
            {
                return Err(format!("Duplicate STEP entity #{id}"));
            }
        }
        if scanner.malformed_record_start().is_some() || scanner.skipped_oversized_ids() > 0 {
            return Err("Malformed or oversized STEP entity identifiers/records".into());
        }
        if g.nodes.is_empty() {
            return Err("IFC contains no STEP entities".into());
        }
        for n in g.nodes.values() {
            if matches!(
                n.kind.as_str(),
                "IFCMATERIALPROPERTIES" | "IFCEXTENDEDMATERIALPROPERTIES" | "IFCPROFILEPROPERTIES"
            ) {
                for owner in g
                    .refs(n, "Material")
                    .into_iter()
                    .chain(g.refs(n, "ProfileDefinition"))
                {
                    g.psets.entry(owner).or_default().push(n.id);
                }
                continue;
            }
            if n.kind == "IFCEXTERNALREFERENCERELATIONSHIP" {
                let targets = g.refs(n, "RelatingReference");
                for owner in g.refs(n, "RelatedResourceObjects") {
                    g.classifications.entry(owner).or_default().extend(&targets);
                }
                continue;
            }
            let related = g.refs(n, "RelatedObjects");
            let (map, target) = match n.kind.as_str() {
                "IFCRELDEFINESBYTYPE" => (&mut g.types, "RelatingType"),
                "IFCRELDEFINESBYPROPERTIES" => (&mut g.psets, "RelatingPropertyDefinition"),
                "IFCRELASSOCIATESMATERIAL" => (&mut g.materials, "RelatingMaterial"),
                "IFCRELASSOCIATESCLASSIFICATION" => {
                    (&mut g.classifications, "RelatingClassification")
                }
                _ => continue,
            };
            let targets = attr(schema, n, target).map(refs).unwrap_or_default();
            for id in related {
                map.entry(id).or_default().extend(&targets);
            }
        }
        for n in g.nodes.values() {
            let (children, parent) = match n.kind.as_str() {
                "IFCRELAGGREGATES" | "IFCRELNESTS" => ("RelatedObjects", "RelatingObject"),
                "IFCRELASSIGNSTOGROUP" => ("RelatedObjects", "RelatingGroup"),
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => ("RelatedElements", "RelatingStructure"),
                "IFCRELVOIDSELEMENT" => ("RelatedOpeningElement", "RelatingBuildingElement"),
                "IFCRELFILLSELEMENT" => ("RelatedBuildingElement", "RelatingOpeningElement"),
                _ => continue,
            };
            for child in g.refs(n, children) {
                for p in g.refs(n, parent) {
                    g.parents
                        .entry(child)
                        .or_default()
                        .push((n.kind.clone(), p));
                }
            }
        }
        let mut decoder = ifc_lite_core::EntityDecoder::new(bytes);
        for n in g.nodes.values() {
            if matches!(
                n.kind.as_str(),
                "IFCSIUNIT" | "IFCCONVERSIONBASEDUNIT" | "IFCDERIVEDUNIT"
            ) {
                if let Some((Some(kind), unit, false)) =
                    ifc_lite_core::resolve_unit_by_ref(&mut decoder, n.id)
                {
                    g.units.insert(n.id, (kind, unit.si_scale));
                }
            }
        }
        let projects: Vec<_> = g
            .nodes
            .values()
            .filter(|n| n.kind == "IFCPROJECT")
            .collect();
        if projects.len() == 1 {
            for assignment in g.refs(projects[0], "UnitsInContext") {
                if let Some(a) = g.nodes.get(&assignment) {
                    for id in g.refs(a, "Units") {
                        if let Some(unit) = g.nodes.get(&id) {
                            if let Some(kind) = g.attr(unit, "UnitType").and_then(text) {
                                if g.project_units.insert(kind, id).is_some() {
                                    return Err("Duplicate project unit types".into());
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(g)
    }
    pub fn attr<'a>(&self, n: &'a Node, name: &str) -> Option<&'a A> {
        attr(self.schema, n, name)
    }
    pub fn refs(&self, n: &Node, name: &str) -> Vec<u32> {
        self.attr(n, name).map(refs).unwrap_or_default()
    }
    pub fn inherited_ids(&self, id: u32) -> Result<Vec<u32>, String> {
        let mut result = vec![id];
        if let Some(types) = self.types.get(&id) {
            if types.len() > 1 {
                return Err("Multiple type assignments are ambiguous".into());
            }
            result.extend(types);
        }
        Ok(result)
    }
}
fn attr<'a>(schema: SchemaVersion, n: &'a Node, name: &str) -> Option<&'a A> {
    let names = schema.attribute_names(&n.kind)?;
    n.attrs.get(names.iter().position(|v| *v == name)?)
}
pub fn refs(a: &A) -> Vec<u32> {
    match a {
        A::EntityRef(id) => vec![*id],
        A::List(v) => v.iter().flat_map(refs).collect(),
        A::Typed { value, .. } => refs(value),
        _ => vec![],
    }
}
pub fn text(a: &A) -> Option<String> {
    match a {
        A::String(s) | A::Enum(s) => Some(s.clone()),
        A::Integer(v) => Some(v.to_string()),
        A::Float(v) if v.is_finite() => Some(v.to_string()),
        A::Typed { value, .. } => text(value),
        _ => None,
    }
}
// Bound recursive tokenization before entering the dependency parser. STEP strings and comments
// are opaque; doubled apostrophes are handled without allocating a second input copy.
fn bounded_nesting(bytes: &[u8], cancel: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    let (mut i, mut depth) = (0, 0usize);
    while i < bytes.len() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("Validation cancelled".into());
        }
        match bytes[i] {
            b'\'' => {
                i += 1;
                loop {
                    if i >= bytes.len() {
                        return Err("Unterminated STEP string".into());
                    }
                    if bytes[i] == b'\'' {
                        if bytes.get(i + 1) == Some(&b'\'') {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                    i += 1
                }
                if i + 1 >= bytes.len() {
                    return Err("Unterminated STEP comment".into());
                }
                i += 2;
            }
            b'(' => {
                depth += 1;
                if depth > 64 {
                    return Err("STEP nesting exceeds 64".into());
                }
                i += 1
            }
            b')' => {
                depth = depth.checked_sub(1).ok_or("Unbalanced STEP parentheses")?;
                i += 1
            }
            _ => i += 1,
        }
    }
    if depth != 0 {
        return Err("Unbalanced STEP parentheses".into());
    }
    Ok(())
}
