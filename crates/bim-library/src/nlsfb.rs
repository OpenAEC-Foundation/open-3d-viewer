//! NL-SfB codes: the Dutch classification of building elements (table 1 of NL-SfB, the element
//! method), which a Dutch delivery asks on every element (BIM basis ILS).
//!
//! A type has the code of its group by rule: a wall with a cavity or insulation is an external
//! wall (21), another an internal wall (22), a floor 23, a roof 27, a ceiling 45, a column or a
//! beam the main structure (28), a pile a pile foundation (17). The rule gives the two digits
//! of the group and no more: whether a wall bears, or what a floor is made of, it does not
//! guess. A type takes a code of its own in the rule's place (`code`), down to four digits
//! ("21.22"); it is checked for its form and for its group being one of the table.
//!
//! A library holds its codes once ([`Code`], [`crate::Library::codes`]): the groups of the table
//! to start with, and the deeper codes an office adds, each with its name. Many types have one
//! code; the library says which ([`crate::Library::coded`]).
//!
//! In IFCX a type's node has its code as `nlsfb::class` (`code`, and the `uri` of the class in
//! the buildingSMART Data Dictionary), as an element has it. A code of the library is a node
//! too, with `nlsfb::class`, its English name and `openaec::classification` (its system, code
//! and names).

use crate::buildup::{BuildUp, Role};
use crate::material::{HatchKind, Material, Names};
use crate::profile::{LinearRole, LinearType};
use crate::system::{SystemGroup, SystemType};
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a node with its NL-SfB class.
pub const CLASS: &str = "nlsfb::class";

/// The groups of table 1: their two digits and their names in Dutch and English.
pub const GROUPS: &[(&str, &str, &str)] = &[
    ("11", "bodemvoorzieningen", "ground works"),
    ("13", "vloeren op grondslag", "floors on grade"),
    ("16", "funderingsconstructies", "foundations"),
    ("17", "paalfunderingen", "pile foundations"),
    ("21", "buitenwanden", "external walls"),
    ("22", "binnenwanden", "internal walls"),
    ("23", "vloeren", "floors"),
    ("24", "trappen en hellingen", "stairs and ramps"),
    ("27", "daken", "roofs"),
    ("28", "hoofddraagconstructies", "main structure"),
    ("31", "buitenwandopeningen", "openings in external walls"),
    ("32", "binnenwandopeningen", "openings in internal walls"),
    ("33", "vloeropeningen", "openings in floors"),
    ("34", "balustrades en leuningen", "balustrades and handrails"),
    ("37", "dakopeningen", "openings in roofs"),
    ("38", "inbouwpakketten", "infill packages"),
    ("41", "buitenwandafwerkingen", "external wall finishes"),
    ("42", "binnenwandafwerkingen", "internal wall finishes"),
    ("43", "vloerafwerkingen", "floor finishes"),
    ("44", "trap- en hellingafwerkingen", "stair and ramp finishes"),
    ("45", "plafondafwerkingen", "ceiling finishes"),
    ("47", "dakafwerkingen", "roof finishes"),
    ("48", "afwerkingspakketten", "finish packages"),
    ("51", "warmteopwekking", "heat generation"),
    ("52", "afvoeren", "drainage"),
    ("53", "water", "water"),
    ("54", "gassen", "gases"),
    ("55", "koude-opwekking en distributie", "cooling"),
    ("56", "warmtedistributie", "heat distribution"),
    ("57", "luchtbehandeling", "air handling"),
    ("58", "regeling klimaat en sanitair", "climate and sanitary controls"),
    ("61", "centrale elektrotechnische voorzieningen", "central electrical services"),
    ("62", "krachtstroom", "power"),
    ("63", "verlichting", "lighting"),
    ("64", "communicatie", "communication"),
    ("65", "beveiliging", "security"),
    ("66", "transport", "transport"),
    ("67", "gebouwbeheervoorzieningen", "building management"),
    ("71", "vaste verkeersvoorzieningen", "fixed circulation fittings"),
    ("72", "vaste gebruikersvoorzieningen", "fixed user fittings"),
    ("73", "vaste keukenvoorzieningen", "fixed kitchen fittings"),
    ("74", "vaste sanitaire voorzieningen", "fixed sanitary fittings"),
    ("75", "vaste onderhoudsvoorzieningen", "fixed maintenance fittings"),
    ("76", "vaste opslagvoorzieningen", "fixed storage fittings"),
    ("81", "losse verkeersinventaris", "loose circulation furnishings"),
    ("82", "losse gebruikersinventaris", "loose user furnishings"),
    ("83", "losse keukeninventaris", "loose kitchen furnishings"),
    ("84", "losse sanitaire inventaris", "loose sanitary furnishings"),
    ("85", "losse schoonmaakinventaris", "loose cleaning furnishings"),
    ("86", "losse opberginventaris", "loose storage furnishings"),
    ("90", "terrein", "site"),
];

/// The attribute of a code's node with the library's own data.
pub const CLASSIFICATION: &str = "openaec::classification";

/// A code of the library: one code, the many types that have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Code {
    /// "21", "21.2" or "21.22".
    pub code: String,
    pub names: Names,
}

impl Code {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }
}

/// The codes a library starts with: the groups of table 1. Their names are Dutch and English;
/// German and French read the English one.
pub fn starter() -> Vec<Code> {
    GROUPS.iter().map(|&(code, nl, en)| Code { code: code.into(), names: Names::new(nl, en, en, en) }).collect()
}

/// The path of the node of `code`.
pub fn code_path(code: &str) -> String {
    uuid(&format!("openaec-library/classification/nlsfb/{code}"))
}

/// The node of a code of the library.
pub fn code_node(code: &Code) -> Value {
    json!({
        "path": code_path(&code.code),
        "attributes": {
            CLASS: class_value(&code.code),
            "bsi::ifc::prop::Name": code.name(Lang::En),
            CLASSIFICATION: {"system": "NL-SfB", "code": code.code, "names": code.names},
        }
    })
}

/// The name of the group of `code` ("21.22" is of group 21). Dutch for Dutch, else English.
pub fn group_name(code: &str, lang: Lang) -> Option<&'static str> {
    let group = code.get(..2)?;
    GROUPS.iter().find(|(digits, _, _)| *digits == group).map(|&(_, nl, en)| if lang == Lang::Nl { nl } else { en })
}

/// Whether `code` is a code of table 1: two digits of a group of the table, and after a point
/// one or two digits more ("21", "21.2", "21.22").
pub fn valid(code: &str) -> bool {
    let (group, rest) = code.split_once('.').map_or((code, None), |(group, rest)| (group, Some(rest)));
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    group.len() == 2 && digits(group) && GROUPS.iter().any(|(of, _, _)| *of == group) && rest.is_none_or(|rest| rest.len() <= 2 && digits(rest))
}

/// `code` as a type keeps it: trimmed, none when empty; refused when it is no code of table 1.
pub fn taken(code: &str) -> Result<Option<String>, LibraryError> {
    let code = code.trim();
    if code.is_empty() {
        Ok(None)
    } else if valid(code) {
        Ok(Some(code.to_string()))
    } else {
        Err(LibraryError::BadCode(code.to_string()))
    }
}

/// The class of `code` in the buildingSMART Data Dictionary.
pub fn uri(code: &str) -> String {
    format!("https://identifier.buildingsmart.org/uri/nlsfb/nlsfb2005/2.2/class/{code}")
}

/// `code` as the attribute of a node.
pub fn class_value(code: &str) -> Value {
    json!({"code": code, "uri": uri(code)})
}

/// The code a node has, when it has one of table 1.
pub fn class_of(attributes: &serde_json::Map<String, Value>) -> Option<&str> {
    attributes.get(CLASS).and_then(|class| class.get("code").or(Some(class))).and_then(Value::as_str).filter(|code| valid(code))
}

impl BuildUp {
    /// Its code by rule: a wall with a cavity, insulation or cladding is an external wall.
    pub fn rule_code(&self, materials: &[Material]) -> &'static str {
        match self.role {
            Role::Wall => {
                let outside = self.layers.iter().any(|layer| {
                    layer.material.contains("cladding") || materials.iter().find(|m| m.key == layer.material).is_some_and(|m| matches!(m.hatch, HatchKind::Insulation | HatchKind::Air))
                });
                if outside {
                    "21"
                } else {
                    "22"
                }
            }
            Role::Floor => "23",
            Role::Roof => "27",
            Role::Ceiling => "45",
        }
    }

    /// Its NL-SfB code: its own, else the rule's.
    pub fn nlsfb(&self, materials: &[Material]) -> String {
        self.code.clone().unwrap_or_else(|| self.rule_code(materials).to_string())
    }
}

impl LinearType {
    /// Its code by rule, from its role.
    pub fn rule_code(&self) -> &'static str {
        match self.role {
            LinearRole::Column | LinearRole::Beam | LinearRole::Member => "28",
            LinearRole::Mullion => "31",
            LinearRole::Skirting => "42",
            LinearRole::Pipe => "52",
            LinearRole::Duct => "57",
            LinearRole::CableTray => "61",
            LinearRole::Pile => "17",
        }
    }

    /// Its NL-SfB code: its own, else the rule's.
    pub fn nlsfb(&self) -> String {
        self.code.clone().unwrap_or_else(|| self.rule_code().to_string())
    }
}

impl SystemType {
    /// Its code by rule, from its group.
    pub fn rule_code(&self) -> &'static str {
        match self.group {
            SystemGroup::CurtainWall | SystemGroup::TimberFrame | SystemGroup::Masonry => "21",
            SystemGroup::Frame => "31",
            SystemGroup::MetalStud => "22",
            SystemGroup::Cladding => "41",
            SystemGroup::Floor => "23",
            SystemGroup::Ceiling => "45",
        }
    }

    /// Its NL-SfB code: its own, else the rule's.
    pub fn nlsfb(&self) -> String {
        self.code.clone().unwrap_or_else(|| self.rule_code().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;
    use std::collections::HashSet;

    #[test]
    fn the_groups_are_two_digits_each_once_and_named() {
        let digits: HashSet<&str> = GROUPS.iter().map(|g| g.0).collect();
        assert_eq!(digits.len(), GROUPS.len());
        for &(group, nl, en) in GROUPS {
            assert!(valid(group) && !nl.is_empty() && !en.is_empty(), "{group}");
        }
        assert_eq!((group_name("21.22", Lang::Nl), group_name("21.22", Lang::De), group_name("99", Lang::Nl)), (Some("buitenwanden"), Some("external walls"), None));
    }

    #[test]
    fn a_code_is_a_group_and_at_most_two_digits_more() {
        for code in ["21", "21.2", "21.22", "90"] {
            assert!(valid(code), "{code}");
        }
        for code in ["", "2", "21.", "21.222", "21,2", "99", "2a", "21.2x", " 21"] {
            assert!(!valid(code), "{code}");
        }
        assert_eq!(taken(" 22.11 "), Ok(Some("22.11".to_string())));
        assert_eq!(taken("  "), Ok(None));
        assert_eq!(taken("wand"), Err(LibraryError::BadCode("wand".into())));
    }

    #[test]
    fn every_starter_type_has_a_code_by_rule() {
        let library = Library::starter();
        let code = |id: &str| library.buildup(id).unwrap().nlsfb(&library.materials);
        assert_eq!(code("wall.cavity-pir-360"), "21", "a cavity wall is an external wall");
        assert_eq!(code("wall.timber-frame-outer"), "21");
        assert_eq!(code("wall.metal-stud-75"), "21", "its studs are filled with insulation: the rule cannot tell, a code of its own can");
        assert_eq!(code("wall.sand-lime-100"), "22");
        assert_eq!((code("floor.hollow-core-200"), code("roof.flat-concrete-pir"), code("ceiling.gypsum-12.5")), ("23".to_string(), "27".to_string(), "45".to_string()));
        for b in &library.buildups {
            assert!(valid(&b.nlsfb(&library.materials)), "{}", b.id);
        }
        for t in &library.linear_types {
            assert!(valid(&t.nlsfb()), "{}", t.id);
        }
        for t in &library.system_types {
            assert!(valid(&t.nlsfb()), "{}", t.id);
        }
        let column = library.linear_types.iter().find(|t| t.role == LinearRole::Column).unwrap();
        let pile = library.linear_types.iter().find(|t| t.role == LinearRole::Pile);
        assert_eq!((column.nlsfb().as_str(), pile.map(|p| p.nlsfb())), ("28", pile.map(|_| "17".to_string())));
    }

    #[test]
    fn a_code_of_its_own_stands_in_the_rules_place() {
        let mut library = Library::starter();
        let materials = library.materials.clone();
        let partition = library.buildups.iter_mut().find(|b| b.id == "wall.metal-stud-75").unwrap();
        partition.code = taken("22.11").unwrap();
        assert_eq!((partition.nlsfb(&materials).as_str(), partition.rule_code(&materials)), ("22.11", "21"));
        assert_eq!(class_value("22.11"), json!({"code": "22.11", "uri": "https://identifier.buildingsmart.org/uri/nlsfb/nlsfb2005/2.2/class/22.11"}));
    }
}
