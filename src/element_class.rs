//! What kind of building element an element is, from what its loader kept of it: the IFC class
//! of an IFC or IFCX element, else the class the IFC export gives its category in the source
//! format (Revit, Tekla, Archicad). A structural plan shows the elements that carry load.

/// IFC classes of the building's structure: what carries load, down to the foundations.
const STRUCTURAL: [&str; 18] = [
    "IFCWALL",
    "IFCSLAB",
    "IFCCOLUMN",
    "IFCBEAM",
    "IFCMEMBER",
    "IFCPLATE",
    "IFCFOOTING",
    "IFCPILE",
    "IFCDEEPFOUNDATION",
    "IFCCAISSONFOUNDATION",
    "IFCROOF",
    "IFCSTAIR",
    "IFCSTAIRFLIGHT",
    "IFCRAMP",
    "IFCRAMPFLIGHT",
    "IFCBEARING",
    "IFCTENDON",
    "IFCCHIMNEY",
];

/// Source categories whose IFC class is structural but which carry nothing: the parts of a
/// curtain wall.
const NOT_CARRYING: [&str; 2] = ["Curtain panels", "Curtain wall mullions"];

/// The IFC class of an element in upper case (`IFCWALL`, the standard and elemented cases
/// folded in), from its properties, or from its category in `format` as the IFC export maps it;
/// `None` when nothing says (a proxy).
pub fn ifc_class(format: &str, layer: &str, props: &[(String, String)]) -> Option<String> {
    let get = |k: &str| props.iter().find(|(p, _)| p == k).map(|(_, v)| v.trim()).filter(|v| !v.is_empty());
    let class = match get("IfcType").or_else(|| get("Class")) {
        Some(class) => class.to_uppercase(),
        None => crate::export::ifc::export_class(format, layer, props).to_string(),
    };
    let class = class.strip_suffix("STANDARDCASE").or_else(|| class.strip_suffix("ELEMENTEDCASE")).unwrap_or(&class).to_string();
    (class.starts_with("IFC") && class != "IFCBUILDINGELEMENTPROXY").then_some(class)
}

/// Whether an element carries load: its `LoadBearing` property where it has one, else its
/// class; `None` when nothing says (no class).
pub fn load_bearing(format: &str, layer: &str, props: &[(String, String)]) -> Option<bool> {
    let flag = props.iter().find(|(k, _)| k == "LoadBearing" || k.ends_with("::LoadBearing")).map(|(_, v)| v.trim().to_lowercase());
    match flag.as_deref() {
        Some("true" | ".t.") => return Some(true),
        Some("false" | ".f.") => return Some(false),
        _ => {}
    }
    let category = props.iter().find(|(k, _)| k == "category").map_or(layer, |(_, v)| v.as_str());
    if NOT_CARRYING.contains(&category) {
        return Some(false);
    }
    ifc_class(format, layer, props).map(|class| STRUCTURAL.contains(&class.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_structure_is_told_by_class_category_and_load_bearing() {
        // IFC and IFCX keep the class
        assert_eq!(load_bearing("IFC", "", &props(&[("IfcType", "IfcWallStandardCase")])), Some(true));
        assert_eq!(load_bearing("IFC", "", &props(&[("IfcType", "IfcFurnishingElement")])), Some(false));
        assert_eq!(load_bearing("IFCX", "", &props(&[("Class", "IfcBeam")])), Some(true));
        assert_eq!(load_bearing("IFCX", "", &props(&[("Class", "IfcDoor")])), Some(false));
        // a wall that says it carries nothing
        assert_eq!(load_bearing("IFCX", "", &props(&[("Class", "IfcWall"), ("LoadBearing", "false")])), Some(false));
        // Revit: by category, as the IFC export has it; a curtain wall's parts carry nothing
        assert_eq!(load_bearing("Revit", "Walls", &props(&[("category", "Structural foundations")])), Some(true));
        assert_eq!(load_bearing("Revit", "", &props(&[("category", "Furniture and casework")])), Some(false));
        assert_eq!(load_bearing("Revit", "", &props(&[("category", "Curtain wall mullions")])), Some(false));
        // nothing says: a proxy, or a format without classes
        assert_eq!(load_bearing("IFC", "", &props(&[("IfcType", "IfcBuildingElementProxy")])), None);
        assert_eq!(load_bearing("SketchUp", "Layer0", &props(&[("Name", "Sofa")])), None);
    }
}
