//! Connection types: how elements are joined where they meet, as items of a library.
//!
//! A connection type has a stable **key** (`steel.fin-plate.m20-3`), which is what a project
//! and its elements hold it by, a **family** (what is joined: steel, timber, a timber frame,
//! concrete, masonry, planar elements, finishes, services), what it stands **between**, and its
//! **recipe**: what the kernel does where the elements meet, in the kernel's own terms, with
//! its sizes. The recipes are the ones the kernel makes today (`bim_core::ConnectionKind`): a
//! butt, a mitre, a cut-out, a steel end plate and a steel fin plate. The families the kernel
//! has no recipe for yet are named, and empty.
//!
//! A type is named by rule from its recipe ("Fin plate 10 mm, 3 × M20"), in Dutch, English,
//! German and French, unless it has names of its own.
//!
//! The sizes of the plates are geometric defaults to start from, as the kernel's are. They are
//! not a checked structural design.
//!
//! In IFCX a connection type is a node with its English name and `openaec::connectiontype`
//! (all of it), and its NL-SfB code as `nlsfb::class` where its family has one. A library holds
//! its connection types ([`crate::Library::connection_types`]).

use crate::lod::Lod;
use crate::material::Names;
use crate::{millimetres, uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a type node with its connection type.
pub const CONNECTION_TYPE: &str = "openaec::connectiontype";

/// What a connection joins: its family in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    Steel,
    Timber,
    TimberFrame,
    Concrete,
    Masonry,
    /// The line joins of planar elements: wall to wall, wall to floor, roof edges.
    Planar,
    /// Skirting boards, cornices, corner beads.
    Finish,
    /// The fittings and the penetrations of ducts, pipes and cables.
    Services,
}

impl Family {
    pub const ALL: [Family; 8] = [Family::Steel, Family::Timber, Family::TimberFrame, Family::Concrete, Family::Masonry, Family::Planar, Family::Finish, Family::Services];

    /// Its name in Dutch and in English, as the library's tree lists it.
    pub fn name(self, lang: Lang) -> &'static str {
        let (nl, en) = match self {
            Family::Steel => ("staal", "steel"),
            Family::Timber => ("hout", "timber"),
            Family::TimberFrame => ("houtskeletbouw", "timber frame"),
            Family::Concrete => ("beton en prefab", "concrete and precast"),
            Family::Masonry => ("metselwerk", "masonry"),
            Family::Planar => ("wand, vloer en dak", "wall, floor and roof"),
            Family::Finish => ("afwerking", "finishes"),
            Family::Services => ("installaties", "services"),
        };
        if lang == Lang::Nl {
            nl
        } else {
            en
        }
    }

    /// The NL-SfB group its connections are of unless they say another code: the main
    /// structure for the structural families; none where the joined elements decide.
    pub fn code(self) -> Option<&'static str> {
        match self {
            Family::Steel | Family::Timber | Family::TimberFrame | Family::Concrete => Some("28"),
            _ => None,
        }
    }
}

/// What a connection stands between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Between {
    /// Two linear members: a beam and a column, two beams.
    Members,
    /// A linear member and the planar element or system it goes through.
    MemberAndPlanar,
    /// Two planar elements.
    Planars,
    /// Any two elements with a body.
    Any,
}

impl Between {
    pub fn name(self, lang: Lang) -> &'static str {
        let (nl, en) = match self {
            Between::Members => ("twee staven", "two members"),
            Between::MemberAndPlanar => ("een staaf en een vlak element", "a member and a planar element"),
            Between::Planars => ("twee vlakke elementen", "two planar elements"),
            Between::Any => ("twee elementen", "any two elements"),
        };
        if lang == Lang::Nl {
            nl
        } else {
            en
        }
    }
}

/// What the kernel does where the elements meet, with its sizes in metres. The fields are the
/// kernel's (`bim_core::join::EndPlate`, `bim_core::join::FinPlate`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recipe {
    /// The second member stops at the face of the first, which runs through.
    Butt,
    /// Both members are cut by the plane that halves the angle between them.
    Mitre,
    /// The first element cuts its body out of the second.
    Cut,
    /// A plate on the member's end, bolted to the face it stops short of. `margin`: how far the
    /// plate reaches past the profile; `rows`: bolts in each of its two rows.
    EndPlate { thickness: f64, margin: f64, bolt: f64, hole: f64, rows: u32 },
    /// A plate welded to the face, bolted through the member's web. `gap`: from the member's
    /// end to the face; `edge`: from a bolt to the plate's edge (bolts are twice that apart);
    /// `rows`: how many bolts.
    FinPlate { thickness: f64, gap: f64, edge: f64, bolt: f64, hole: f64, rows: u32 },
}

impl Recipe {
    /// Whether it has parts of its own (a plate, bolts).
    pub fn has_parts(&self) -> bool {
        matches!(self, Recipe::EndPlate { .. } | Recipe::FinPlate { .. })
    }

    /// Its sizes as lines to show: what each is, and its value in millimetres or as a count.
    pub fn sizes(&self, lang: Lang) -> Vec<(&'static str, String)> {
        let nl = lang == Lang::Nl;
        let mm = |v: f64| format!("{} mm", millimetres(v, lang));
        let say = |dutch: &'static str, english: &'static str| if nl { dutch } else { english };
        match *self {
            Recipe::Butt | Recipe::Mitre | Recipe::Cut => Vec::new(),
            Recipe::EndPlate { thickness, margin, bolt, hole, rows } => vec![
                (say("Plaatdikte", "Plate thickness"), mm(thickness)),
                (say("Plaat buiten het profiel", "Plate past the profile"), mm(margin)),
                (say("Bouten", "Bolts"), format!("2 × {rows} M{}", millimetres(bolt, lang))),
                (say("Gaten", "Holes"), mm(hole)),
            ],
            Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows } => vec![
                (say("Plaatdikte", "Plate thickness"), mm(thickness)),
                (say("Ruimte tot het vlak", "Gap to the face"), mm(gap)),
                (say("Randafstand", "Edge distance"), mm(edge)),
                (say("Bouten", "Bolts"), format!("{rows} × M{}", millimetres(bolt, lang))),
                (say("Gaten", "Holes"), mm(hole)),
            ],
        }
    }

    /// What it is called by rule: "Fin plate 10 mm, 3 × M20".
    pub fn name(&self, lang: Lang) -> String {
        let pick = |nl: &str, en: &str, de: &str, fr: &str| match lang {
            Lang::Nl => nl.to_string(),
            Lang::En => en.to_string(),
            Lang::De => de.to_string(),
            Lang::Fr => fr.to_string(),
        };
        let mm = |v: f64| millimetres(v, lang);
        match *self {
            Recipe::Butt => pick("Stompe aansluiting", "Butt joint", "Stumpfer Anschluss", "Assemblage bout à bout"),
            Recipe::Mitre => pick("Verstek", "Mitre", "Gehrung", "Assemblage à onglet"),
            Recipe::Cut => pick("Sparing", "Cut-out", "Aussparung", "Réservation"),
            Recipe::EndPlate { thickness, bolt, rows, .. } => format!("{} {} mm, 2 × {rows} M{}", pick("Kopplaat", "End plate", "Stirnplatte", "Platine d'about"), mm(thickness), mm(bolt)),
            Recipe::FinPlate { thickness, bolt, rows, .. } => format!("{} {} mm, {rows} × M{}", pick("Lipplaat", "Fin plate", "Fahnenblech", "Gousset"), mm(thickness), mm(bolt)),
        }
    }

    /// Whether its sizes are ones a plate can have: a plate between 1 and 100 mm, bolts between
    /// M6 and M64 in holes no smaller than they are and at most 6 mm wider, one to twelve bolts
    /// in a row, margins and edges that hold a hole.
    fn check(&self) -> Result<(), &'static str> {
        let plate = |thickness: f64, bolt: f64, hole: f64, rows: u32| {
            if !(thickness.is_finite() && (0.001..=0.1).contains(&thickness)) {
                Err("a plate between 1 and 100 mm thick")
            } else if !(bolt.is_finite() && (0.006..=0.064).contains(&bolt)) {
                Err("bolts between M6 and M64")
            } else if !(hole.is_finite() && hole >= bolt && hole <= bolt + 0.006) {
                Err("holes as wide as the bolts, or up to 6 mm wider")
            } else if !(1..=12).contains(&rows) {
                Err("one to twelve bolts in a row")
            } else {
                Ok(())
            }
        };
        match *self {
            Recipe::Butt | Recipe::Mitre | Recipe::Cut => Ok(()),
            Recipe::EndPlate { thickness, margin, bolt, hole, rows } => {
                plate(thickness, bolt, hole, rows)?;
                if !(margin.is_finite() && margin >= hole && margin <= 0.5) {
                    return Err("a margin that holds a bolt hole, up to 500 mm");
                }
                Ok(())
            }
            Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows } => {
                plate(thickness, bolt, hole, rows)?;
                if !(gap.is_finite() && (0.0..=0.1).contains(&gap)) {
                    return Err("a gap of at most 100 mm");
                }
                if !(edge.is_finite() && edge >= hole && edge <= 0.2) {
                    return Err("an edge distance that holds a bolt hole, up to 200 mm");
                }
                Ok(())
            }
        }
    }
}

/// A connection type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionType {
    /// Its stable key: what a project and its elements hold it by. Never shown as its name.
    pub id: String,
    pub family: Family,
    pub between: Between,
    pub recipe: Recipe,
    /// Names of its own, in place of the rule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Names>,
    /// The key of the material its parts are of, where it has parts and the library has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// An NL-SfB code of its own, in place of its family's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ConnectionType {
    pub fn new(id: &str, family: Family, between: Between, recipe: Recipe) -> Self {
        Self { id: id.into(), family, between, recipe, names: None, material: None, code: None }
    }

    /// Its name: its own, else the rule's for its recipe.
    pub fn name(&self, lang: Lang) -> String {
        match &self.names {
            Some(names) => names.get(lang).to_string(),
            None => self.recipe.name(lang),
        }
    }

    /// Its NL-SfB code: its own, else its family's, where the family has one.
    pub fn nlsfb(&self) -> Option<String> {
        self.code.clone().or_else(|| self.family.code().map(str::to_string))
    }

    /// Its level of development: a connection with its parts worked out is LOD 350; one that
    /// only says how the elements meet LOD 300.
    pub fn lod(&self) -> Lod {
        if self.recipe.has_parts() {
            Lod::L350
        } else {
            Lod::L300
        }
    }

    /// Whether it holds together: a key without spaces, names (where it has its own) in every
    /// language, a recipe with sizes a plate can have, a code that is one.
    pub fn check(&self) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        let bad = |what: &str| Err(LibraryError::BadConnection(format!("{}: {what}", self.id)));
        if self.id.chars().any(char::is_whitespace) {
            return bad("a key without spaces");
        }
        if self.names.as_ref().is_some_and(|names| Lang::ALL.into_iter().any(|lang| names.get(lang).trim().is_empty())) {
            return bad("a name in every language");
        }
        if let Err(what) = self.recipe.check() {
            return bad(what);
        }
        if let Some(code) = &self.code {
            if !crate::nlsfb::valid(code) {
                return Err(LibraryError::BadCode(code.clone()));
            }
        }
        Ok(())
    }
}

/// The path of the node of connection type `id`.
pub fn path(id: &str) -> String {
    uuid(&format!("openaec-library/connectiontype/{id}"))
}

/// The node of a connection type.
pub fn node(connection: &ConnectionType) -> Value {
    let mut attributes = json!({
        "bsi::ifc::prop::Name": connection.name(Lang::En),
        CONNECTION_TYPE: connection,
    });
    if connection.recipe.has_parts() {
        attributes["bsi::ifc::class"] = json!({"code": "IfcElementAssemblyType"});
        attributes["bsi::ifc::prop::PredefinedType"] = json!("USERDEFINED");
    }
    if let Some(code) = connection.nlsfb() {
        attributes[crate::nlsfb::CLASS] = crate::nlsfb::class_value(&code);
    }
    json!({"path": path(&connection.id), "attributes": attributes})
}

/// The connection types a library starts with: the recipes the kernel makes, for steel and
/// timber members and for what goes through a wall or a floor, with the usual plate sizes.
pub fn starter() -> Vec<ConnectionType> {
    let steel = |key: &str, recipe: Recipe| ConnectionType { material: recipe.has_parts().then(|| "steel".to_string()), ..ConnectionType::new(&format!("steel.{key}"), Family::Steel, Between::Members, recipe) };
    let fin = |bolt: u32, rows: u32, thickness: f64| {
        let d = f64::from(bolt) / 1000.0;
        steel(&format!("fin-plate.m{bolt}-{rows}"), Recipe::FinPlate { thickness, gap: 0.01, edge: f64::from(2 * bolt) / 1000.0, bolt: d, hole: f64::from(bolt + 2) / 1000.0, rows })
    };
    let end = |bolt: u32, rows: u32, thickness: f64, margin: f64| {
        let d = f64::from(bolt) / 1000.0;
        steel(&format!("end-plate.m{bolt}-2x{rows}"), Recipe::EndPlate { thickness, margin, bolt: d, hole: f64::from(bolt + 2) / 1000.0, rows })
    };
    let mut types = vec![steel("butt", Recipe::Butt), steel("mitre", Recipe::Mitre)];
    // fin plates: M16 on an 8 mm plate, M20 on 10 mm (the kernel's own default is M20 × 3),
    // M24 on 12 mm
    for rows in 2..=4 {
        types.push(fin(16, rows, 0.008));
    }
    for rows in 2..=6 {
        types.push(fin(20, rows, 0.01));
    }
    for rows in 3..=6 {
        types.push(fin(24, rows, 0.012));
    }
    // end plates: two rows of bolts (the kernel's own default is 15 mm with 2 × 2 M20)
    types.extend([end(16, 2, 0.012, 0.07), end(20, 2, 0.015, 0.08), end(20, 3, 0.015, 0.08), end(20, 4, 0.02, 0.08), end(24, 2, 0.02, 0.09), end(24, 3, 0.02, 0.09), end(24, 4, 0.025, 0.09)]);
    let of = |id: &str, family: Family, between: Between, recipe: Recipe| ConnectionType::new(id, family, between, recipe);
    types.extend([
        of("timber.butt", Family::Timber, Between::Members, Recipe::Butt),
        of("timber.mitre", Family::Timber, Between::Members, Recipe::Mitre),
        of("concrete.butt", Family::Concrete, Between::Members, Recipe::Butt),
        of("services.penetration", Family::Services, Between::Any, Recipe::Cut),
    ]);
    let cut = &mut types.last_mut().expect("the penetration").names;
    *cut = Some(Names::new("Doorvoer", "Penetration", "Durchführung", "Traversée"));
    types
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_starter_types_hold_together_under_stable_keys() {
        let types = starter();
        assert_eq!(types.len(), 25);
        let keys: HashSet<&str> = types.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(keys.len(), types.len(), "every key once");
        for t in &types {
            t.check().unwrap_or_else(|e| panic!("{}: {e}", t.id));
            for lang in Lang::ALL {
                assert!(!t.name(lang).trim().is_empty(), "{} {lang:?}", t.id);
            }
            assert_eq!(t.lod(), if t.recipe.has_parts() { Lod::L350 } else { Lod::L300 }, "{}", t.id);
            assert_eq!(t.material.is_some(), t.recipe.has_parts(), "{}", t.id);
        }
        // the kernel's own defaults are types of the catalogue, by their keys
        let by = |key: &str| types.iter().find(|t| t.id == key).unwrap_or_else(|| panic!("{key}"));
        assert_eq!(by("steel.fin-plate.m20-3").recipe, Recipe::FinPlate { thickness: 0.01, gap: 0.01, edge: 0.04, bolt: 0.02, hole: 0.022, rows: 3 });
        assert_eq!(by("steel.end-plate.m20-2x2").recipe, Recipe::EndPlate { thickness: 0.015, margin: 0.08, bolt: 0.02, hole: 0.022, rows: 2 });
        // named by rule in four languages, or by names of their own
        let fin = by("steel.fin-plate.m20-3");
        assert_eq!((fin.name(Lang::Nl), fin.name(Lang::En), fin.name(Lang::De), fin.name(Lang::Fr)), ("Lipplaat 10 mm, 3 × M20".into(), "Fin plate 10 mm, 3 × M20".into(), "Fahnenblech 10 mm, 3 × M20".into(), "Gousset 10 mm, 3 × M20".into()));
        assert_eq!(by("steel.end-plate.m24-2x4").name(Lang::Nl), "Kopplaat 25 mm, 2 × 4 M24");
        assert_eq!((by("services.penetration").name(Lang::Nl), by("timber.mitre").name(Lang::En)), ("Doorvoer".into(), "Mitre".into()));
        assert_eq!(fin.recipe.sizes(Lang::En), [("Plate thickness", "10 mm".to_string()), ("Gap to the face", "10 mm".to_string()), ("Edge distance", "40 mm".to_string()), ("Bolts", "3 × M20".to_string()), ("Holes", "22 mm".to_string())]);
        // steel and timber are of the main structure; a penetration takes its elements' code
        assert_eq!((fin.nlsfb().as_deref(), by("services.penetration").nlsfb()), (Some("28"), None));
        for family in Family::ALL {
            assert!(!family.name(Lang::Nl).is_empty() && !family.name(Lang::En).is_empty());
        }
    }

    #[test]
    fn a_connection_is_found_by_its_key_after_a_save_and_a_reload() {
        let mut library = crate::Library::starter();
        // an office's own fin plate beside the catalogue's, with a code and names of its own
        let mut own = ConnectionType::new("office.fin-plate.m20-3-12", Family::Steel, Between::Members, Recipe::FinPlate { thickness: 0.012, gap: 0.015, edge: 0.045, bolt: 0.02, hole: 0.022, rows: 3 });
        own.material = Some("steel".into());
        own.code = Some("28.1".into());
        own.names = Some(Names::new("Lipplaat kantoor", "Office fin plate", "Fahnenblech Büro", "Gousset bureau"));
        library.add_connection_type(own.clone()).unwrap();
        assert_eq!(library.add_connection_type(own.clone()), Err(LibraryError::Duplicate(own.id.clone())), "a key once");
        let mut of_air = own.clone();
        (of_air.id, of_air.material) = ("office.fin-2".into(), Some("no-such-material".into()));
        assert_eq!(library.add_connection_type(of_air), Err(LibraryError::UnknownMaterial("no-such-material".into())));
        let file = crate::ifcx::file(&library, "test", "test", "2026-11-20T00:00:00Z");
        let node = file["data"].as_array().unwrap().iter().find(|n| n["path"] == path("steel.fin-plate.m20-3")).expect("its node, by its key");
        assert_eq!(node["attributes"]["bsi::ifc::prop::Name"], "Fin plate 10 mm, 3 × M20");
        assert_eq!(node["attributes"][CONNECTION_TYPE]["recipe"], json!({"kind": "fin_plate", "thickness": 0.01, "gap": 0.01, "edge": 0.04, "bolt": 0.02, "hole": 0.022, "rows": 3}));
        assert_eq!(node["attributes"]["bsi::ifc::class"]["code"], "IfcElementAssemblyType");
        assert_eq!(node["attributes"][crate::nlsfb::CLASS]["code"], "28");
        // read back: the same types under the same keys, in the same order
        let read = crate::ifcx::read(&file).expect("the library reads");
        assert_eq!(read.connection_types, library.connection_types);
        assert_eq!(read.connection_type("steel.fin-plate.m20-3"), library.connection_type("steel.fin-plate.m20-3"));
        assert_eq!(read.connection_type("office.fin-plate.m20-3-12"), Some(&own));
        assert_eq!(read.connection_types_of(Family::Steel).count(), 22);
        assert!(read.connection_types_of(Family::Masonry).next().is_none() && read.connection_type("steel.fin-plate").is_none());
        // the code's types count it; a library that has a key does not take it again
        assert_eq!(read.coded("28.1", false), ["office.fin-plate.m20-3-12"]);
        let mut other = crate::Library::starter();
        let before = other.connection_types.len();
        other.merge(&read);
        assert_eq!(other.connection_types.len(), before + 1);
    }

    #[test]
    fn what_is_no_connection_is_refused() {
        let good = (0.01, 0.01, 0.04, 0.02, 0.022, 3);
        let fin = |(thickness, gap, edge, bolt, hole, rows): (f64, f64, f64, f64, f64, u32)| ConnectionType::new("steel.fin", Family::Steel, Between::Members, Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows }).check();
        assert!(fin(good).is_ok());
        assert!(fin((0.0, good.1, good.2, good.3, good.4, good.5)).is_err(), "no plate");
        assert!(fin((good.0, good.1, good.2, good.3, 0.018, good.5)).is_err(), "a hole smaller than its bolt");
        assert!(fin((good.0, good.1, good.2, good.3, good.4, 0)).is_err(), "no bolts");
        assert!(fin((good.0, good.1, 0.01, good.3, good.4, good.5)).is_err(), "an edge that holds no hole");
        assert!(fin((good.0, f64::NAN, good.2, good.3, good.4, good.5)).is_err());
        let mut spaced = ConnectionType::new("steel butt", Family::Steel, Between::Members, Recipe::Butt);
        assert!(spaced.check().is_err(), "a key has no spaces");
        spaced.id = "steel.butt-2".into();
        spaced.code = Some("wand".into());
        assert_eq!(spaced.check(), Err(LibraryError::BadCode("wand".into())));
        spaced.code = Some("28.1".into());
        spaced.names = Some(Names::new("stomp", "butt", "", "bout"));
        assert!(spaced.check().is_err(), "a name in every language");
    }
}
