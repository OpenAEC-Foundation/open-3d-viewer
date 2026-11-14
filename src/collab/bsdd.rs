//! The properties a class has in the buildingSMART Data Dictionary (bSDD), to make rules of:
//! asking the dictionary of IFC 4.3 for a class gives its property sets and properties. Only the
//! name of the class goes out; nothing of a model.

/// A property of a class: its property set, its name as IFC writes it, and what it is called.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub set: String,
    pub code: String,
    pub name: String,
}

/// The properties in a class's answer of the dictionary.
pub fn parse(json: &str) -> Result<Vec<Property>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let text = |p: &serde_json::Value, key: &str| p.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let list = value.get("classProperties").and_then(|v| v.as_array()).ok_or("the dictionary gave no properties for this class")?;
    Ok(list.iter().map(|p| Property { set: text(p, "propertySet"), code: text(p, "propertyCode"), name: text(p, "name") }).filter(|p| !p.code.is_empty()).collect())
}

/// Asks the dictionary for the properties of IFC class `class` (`IfcWall`).
#[cfg(not(target_arch = "wasm32"))]
pub fn class_properties(class: &str) -> Result<Vec<Property>, String> {
    let class: String = class.trim().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if class.is_empty() {
        return Err("give a class".into());
    }
    let uri = format!("https://identifier.buildingsmart.org/uri/buildingsmart/ifc/4.3/class/{class}");
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    let answer = agent.get("https://api.bsdd.buildingsmart.org/api/Class/v1").query("Uri", &uri).query("IncludeClassProperties", "true").call().map_err(|e| match e {
        ureq::Error::Status(404, _) => format!("the dictionary has no class {class}"),
        other => other.to_string(),
    })?;
    parse(&answer.into_string().map_err(|e| e.to_string())?)
}

#[cfg(target_arch = "wasm32")]
pub fn class_properties(_: &str) -> Result<Vec<Property>, String> {
    Err("not in the browser".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_properties_of_a_class_read_from_the_dictionarys_answer() {
        let json = r#"{"referenceCode":"IfcWall","classProperties":[
            {"name":"Acoustic Rating","propertyCode":"AcousticRating","propertySet":"Pset_WallCommon"},
            {"name":"Fire Rating","propertyCode":"FireRating","propertySet":"Pset_WallCommon"},
            {"name":"No code"}]}"#;
        let found = parse(json).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[1], Property { set: "Pset_WallCommon".into(), code: "FireRating".into(), name: "Fire Rating".into() });
        assert!(parse("{}").is_err());
    }
}
