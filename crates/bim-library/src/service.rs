//! Service types: the ducts, pipes, cable trays, cable ladders and conduits a building's
//! services are routed with, as items of a library.
//!
//! A service type has a stable **key** (`pipe.copper`), what it **carries** (a duct, a pipe, a
//! cable tray, a cable ladder, a conduit), its **section** (round, rectangular, oval), the
//! material it is of, the standard its sizes follow, and its **sizes**: the series it is made
//! in, each with its nominal size as it is ordered ("DN 25", "125", "300x200"), its real
//! outside size and its wall. With them go its **fittings**: which bend, which tee, which
//! transition and which joint a run of it takes: its routing preferences.
//!
//! A size of a service type is a linear type ([`ServiceType::linear`]), so a run of that size
//! is drawn and placed as every other linear element is.
//!
//! **Cover types** are what goes round or inside a run: insulation and lining, each of a
//! material and a thickness.
//!
//! The sizes are those of the product standards as they are commonly made, from general
//! knowledge and of no manufacturer. The sheet thicknesses of ducts, trays and ladders differ
//! by maker: they are a start to change.
//!
//! In IFCX a service type is a node with its English name, the IFC class of its segments' type
//! and `openaec::servicetype` (all of it); a cover type an `IfcCoveringType` with
//! `openaec::covertype`. A library holds both ([`crate::Library::service_types`],
//! [`crate::Library::cover_types`]).

use crate::lod::Lod;
use crate::material::{Material, Names};
use crate::profile::{LinearRole, LinearType, ProfileShape};
use crate::{millimetres, uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a type node with its service type.
pub const SERVICE_TYPE: &str = "openaec::servicetype";
/// The attribute of a type node with its cover type.
pub const COVER_TYPE: &str = "openaec::covertype";

fn pick(lang: Lang, nl: &'static str, en: &'static str) -> &'static str {
    if lang == Lang::Nl {
        nl
    } else {
        en
    }
}

/// What a service type carries: its group in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Carrier {
    Duct,
    Pipe,
    CableTray,
    CableLadder,
    Conduit,
}

impl Carrier {
    pub const ALL: [Carrier; 5] = [Carrier::Duct, Carrier::Pipe, Carrier::CableTray, Carrier::CableLadder, Carrier::Conduit];

    /// Its name, as the library's tree lists its types.
    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Carrier::Duct => pick(lang, "luchtkanalen", "ducts"),
            Carrier::Pipe => pick(lang, "leidingen", "pipes"),
            Carrier::CableTray => pick(lang, "kabelgoten", "cable trays"),
            Carrier::CableLadder => pick(lang, "kabelladders", "cable ladders"),
            Carrier::Conduit => pick(lang, "installatiebuizen", "conduits"),
        }
    }

    /// The linear role a run of it has.
    pub fn role(self) -> LinearRole {
        match self {
            Carrier::Duct => LinearRole::Duct,
            Carrier::Pipe => LinearRole::Pipe,
            Carrier::CableTray | Carrier::CableLadder | Carrier::Conduit => LinearRole::CableTray,
        }
    }

    /// The IFC class (IFC4.3) of its segments' type, and the predefined type.
    pub fn ifc(self) -> (&'static str, &'static str) {
        match self {
            Carrier::Duct => ("IfcDuctSegmentType", "RIGIDSEGMENT"),
            Carrier::Pipe => ("IfcPipeSegmentType", "RIGIDSEGMENT"),
            Carrier::CableTray => ("IfcCableCarrierSegmentType", "CABLETRAYSEGMENT"),
            Carrier::CableLadder => ("IfcCableCarrierSegmentType", "CABLELADDERSEGMENT"),
            Carrier::Conduit => ("IfcCableCarrierSegmentType", "CONDUITSEGMENT"),
        }
    }

    /// The NL-SfB group its types are of unless they say another code: air handling for
    /// ducts, drainage for pipes (as a pipe's linear role), central electrical services for
    /// what carries cables.
    pub fn code(self) -> &'static str {
        match self {
            Carrier::Duct => "57",
            Carrier::Pipe => "52",
            Carrier::CableTray | Carrier::CableLadder | Carrier::Conduit => "61",
        }
    }

    /// Whether it is open at the top: a tray or a ladder, which has sides and no closed wall.
    pub fn open(self) -> bool {
        matches!(self, Carrier::CableTray | Carrier::CableLadder)
    }
}

/// The shape of a run's section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Section {
    Round,
    Rectangular,
    /// A flat oval: a rectangle with round ends.
    Oval,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Round, Section::Rectangular, Section::Oval];

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Section::Round => pick(lang, "rond", "round"),
            Section::Rectangular => pick(lang, "rechthoekig", "rectangular"),
            Section::Oval => pick(lang, "ovaal", "oval"),
        }
    }
}

/// A size a service type is made in, in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Size {
    /// As it is ordered and written on a drawing: "DN 25", "125", "300x200".
    pub nominal: String,
    /// The real outside width; of a round section its outside diameter.
    pub width: f64,
    /// The real outside height; of a round section its outside diameter.
    pub height: f64,
    /// The wall, or the sheet it is made of.
    pub wall: f64,
}

impl Size {
    /// A round size of outside diameter `d` and wall `t`, in millimetres.
    pub fn round(nominal: &str, d: f64, t: f64) -> Self {
        Self { nominal: nominal.into(), width: d / 1000.0, height: d / 1000.0, wall: t / 1000.0 }
    }

    /// A size `w` wide and `h` high of wall `t`, in millimetres, named "300x200".
    pub fn box_of(w: f64, h: f64, t: f64) -> Self {
        Self { nominal: format!("{w}x{h}"), width: w / 1000.0, height: h / 1000.0, wall: t / 1000.0 }
    }

    /// Its clear inside width and height.
    pub fn inside(&self) -> (f64, f64) {
        (self.width - 2.0 * self.wall, self.height - 2.0 * self.wall)
    }

    /// Its outside size as written: "Ø 21.3" for a round one, else "300 x 200".
    pub fn outside(&self, section: Section, lang: Lang) -> String {
        if section == Section::Round {
            format!("Ø {}", millimetres(self.width, lang))
        } else {
            format!("{} x {}", millimetres(self.width, lang), millimetres(self.height, lang))
        }
    }
}

/// The bend a run takes where it turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bend {
    /// A fitting with a smooth bend.
    Elbow,
    /// A bend made of straight segments.
    Segmented,
    /// A sharp corner.
    Mitre,
    /// The run itself is bent, without a fitting.
    Bent,
}

impl Bend {
    pub const ALL: [Bend; 4] = [Bend::Elbow, Bend::Segmented, Bend::Mitre, Bend::Bent];

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Bend::Elbow => pick(lang, "bochtstuk", "elbow"),
            Bend::Segmented => pick(lang, "segmentbocht", "segmented bend"),
            Bend::Mitre => pick(lang, "verstek", "mitre"),
            Bend::Bent => pick(lang, "gebogen, zonder hulpstuk", "bent, without a fitting"),
        }
    }
}

/// The fitting where a branch leaves a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tee {
    Tee,
    /// A branch set on the run.
    Saddle,
    /// A branch under 45 degrees.
    Wye,
}

impl Tee {
    pub const ALL: [Tee; 3] = [Tee::Tee, Tee::Saddle, Tee::Wye];

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Tee::Tee => pick(lang, "T-stuk", "tee"),
            Tee::Saddle => pick(lang, "zadelstuk", "saddle"),
            Tee::Wye => pick(lang, "Y-stuk 45°", "wye 45°"),
        }
    }
}

/// The fitting where a run changes size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transition {
    /// Both sizes on one centre line.
    Concentric,
    /// One side runs straight on (the top of a duct, the bottom of a drain).
    Eccentric,
}

impl Transition {
    pub const ALL: [Transition; 2] = [Transition::Concentric, Transition::Eccentric];

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Transition::Concentric => pick(lang, "concentrisch verloop", "concentric reducer"),
            Transition::Eccentric => pick(lang, "excentrisch verloop", "eccentric reducer"),
        }
    }
}

/// How two lengths, or a length and a fitting, are joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Joint {
    Slip,
    Flange,
    Press,
    Solder,
    Glue,
    Weld,
    Thread,
    Push,
    Coupler,
}

impl Joint {
    pub const ALL: [Joint; 9] = [Joint::Slip, Joint::Flange, Joint::Press, Joint::Solder, Joint::Glue, Joint::Weld, Joint::Thread, Joint::Push, Joint::Coupler];

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            Joint::Slip => pick(lang, "steekverbinding", "slip joint"),
            Joint::Flange => pick(lang, "flens", "flange"),
            Joint::Press => pick(lang, "persfitting", "press fitting"),
            Joint::Solder => pick(lang, "gesoldeerd", "soldered"),
            Joint::Glue => pick(lang, "lijmmof", "solvent cement socket"),
            Joint::Weld => pick(lang, "gelast", "welded"),
            Joint::Thread => pick(lang, "schroefdraad", "threaded"),
            Joint::Push => pick(lang, "manchetmof", "push-fit socket"),
            Joint::Coupler => pick(lang, "koppelplaat", "coupler"),
        }
    }
}

/// The fittings a run of a type takes: its routing preferences.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fittings {
    pub bend: Bend,
    /// The radius of a bend's centre line, as a multiple of the size it turns in (1.5 for a
    /// long radius elbow; 0 for a mitre).
    pub radius: f64,
    pub tee: Tee,
    pub transition: Transition,
    pub joint: Joint,
}

impl Fittings {
    pub fn new(bend: Bend, radius: f64, tee: Tee, transition: Transition, joint: Joint) -> Self {
        Self { bend, radius, tee, transition, joint }
    }
}

/// A service type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceType {
    /// Its stable key: what a project and its elements hold it by. Never shown as its name.
    pub id: String,
    pub carrier: Carrier,
    pub section: Section,
    /// The key of the material it is of.
    pub material: String,
    pub names: Names,
    /// The standard its sizes follow ("EN 1506"), where it follows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standard: Option<String>,
    /// The series it is made in, smallest first.
    pub sizes: Vec<Size>,
    pub fittings: Fittings,
    /// An NL-SfB code of its own, in place of its carrier's ("53" for a water pipe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ServiceType {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// Its NL-SfB code: its own, else its carrier's.
    pub fn nlsfb(&self) -> String {
        self.code.clone().unwrap_or_else(|| self.carrier.code().to_string())
    }

    /// Its level of development: a specific element, its sizes and its material settled.
    pub fn lod(&self) -> Lod {
        Lod::L300
    }

    /// The size with this nominal size.
    pub fn size(&self, nominal: &str) -> Option<&Size> {
        self.sizes.iter().find(|s| s.nominal == nominal)
    }

    /// The smallest size of the series whose clear inside is at least `width` by `height`:
    /// the size a run takes that has to pass that much.
    pub fn fitting(&self, width: f64, height: f64) -> Option<&Size> {
        self.sizes.iter().find(|s| {
            let (w, h) = s.inside();
            w + 1e-9 >= width && h + 1e-9 >= height
        })
    }

    /// The profile of a run of `size`: a hollow section, or for a tray or a ladder its
    /// outline.
    pub fn profile(&self, size: &Size) -> ProfileShape {
        match self.section {
            Section::Round => ProfileShape::Chs { d: size.width, t: size.wall },
            _ if self.carrier.open() => ProfileShape::Rectangle { width: size.width, depth: size.height },
            // (a flat oval is drawn by its outline until the kernel has that profile)
            Section::Rectangular | Section::Oval => ProfileShape::Rhs { h: size.height, b: size.width, t: size.wall, r_out: 0.0, r_in: 0.0 },
        }
    }

    /// The linear type of a run of its size `nominal`: its key is the type's with the size
    /// ("pipe.copper.22"), its name the type's with the size.
    pub fn linear(&self, nominal: &str, lang: Lang) -> Option<LinearType> {
        let size = self.size(nominal)?;
        let role = self.carrier.role();
        let mut linear = LinearType::new(&format!("{}.{}", self.id, crate::library::slug(nominal)), role, &self.material, self.profile(size));
        linear.name = Some(format!("{} {}", self.name(lang), size.nominal));
        linear.code = Some(self.nlsfb()).filter(|code| code != linear.rule_code());
        Some(linear)
    }

    /// Whether it holds together: a key without spaces, a name in every language, a material
    /// of `materials`, sizes that are sizes (each named once, round ones as wide as high, a
    /// wall that leaves it open), a bend radius of at most ten sizes, a code that is one.
    pub fn check(&self, materials: &[Material]) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        let bad = |what: String| Err(LibraryError::BadService(format!("{}: {what}", self.id)));
        if self.id.chars().any(char::is_whitespace) {
            return bad("a key without spaces".into());
        }
        if Lang::ALL.into_iter().any(|lang| self.names.get(lang).trim().is_empty()) {
            return bad("a name in every language".into());
        }
        if !materials.iter().any(|m| m.key == self.material) {
            return Err(LibraryError::UnknownMaterial(self.material.clone()));
        }
        if self.sizes.is_empty() {
            return bad("at least one size".into());
        }
        for (k, size) in self.sizes.iter().enumerate() {
            let nominal = size.nominal.trim();
            if nominal.is_empty() {
                return bad("a nominal size for every size".into());
            }
            if self.sizes[..k].iter().any(|s| s.nominal.trim() == nominal) {
                return bad(format!("{nominal} once"));
            }
            let real = |v: f64| v.is_finite() && v > 0.0 && v <= 5.0;
            if !(real(size.width) && real(size.height)) {
                return bad(format!("{nominal}: an outside size of up to 5 m"));
            }
            if self.section == Section::Round && (size.width - size.height).abs() > 1e-9 {
                return bad(format!("{nominal}: a round size as wide as it is high"));
            }
            if !(size.wall.is_finite() && size.wall > 0.0 && 2.0 * size.wall < size.width.min(size.height)) {
                return bad(format!("{nominal}: a wall that leaves it open"));
            }
            self.profile(size).check()?;
        }
        if !(self.fittings.radius.is_finite() && (0.0..=10.0).contains(&self.fittings.radius)) {
            return bad("a bend radius of at most ten times the size".into());
        }
        if let Some(code) = &self.code {
            if !crate::nlsfb::valid(code) {
                return Err(LibraryError::BadCode(code.clone()));
            }
        }
        Ok(())
    }
}

/// Where a cover is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Place {
    /// Round the run.
    Insulation,
    /// Inside the run.
    Lining,
}

impl Place {
    pub const ALL: [Place; 2] = [Place::Insulation, Place::Lining];

    pub fn name(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Place::Insulation, Lang::Nl) => "Isolatie",
            (Place::Insulation, Lang::En) => "Insulation",
            (Place::Insulation, Lang::De) => "Dämmung",
            (Place::Insulation, Lang::Fr) => "Isolation",
            (Place::Lining, Lang::Nl) => "Binnenbekleding",
            (Place::Lining, Lang::En) => "Lining",
            (Place::Lining, Lang::De) => "Innenauskleidung",
            (Place::Lining, Lang::Fr) => "Revêtement intérieur",
        }
    }
}

/// An insulation or a lining of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverType {
    /// Its stable key.
    pub id: String,
    pub place: Place,
    /// The key of the material it is of.
    pub material: String,
    /// In metres.
    pub thickness: f64,
    /// Names of its own, in place of the rule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Names>,
}

impl CoverType {
    pub fn new(id: &str, place: Place, material: &str, millimetres: f64) -> Self {
        Self { id: id.into(), place, material: material.into(), thickness: millimetres / 1000.0, names: None }
    }

    /// Its name: its own, else by rule where it is, what of and how thick: "Insulation
    /// mineral wool 30 mm".
    pub fn name(&self, materials: &[Material], lang: Lang) -> String {
        if let Some(names) = &self.names {
            return names.get(lang).to_string();
        }
        let material = materials.iter().find(|m| m.key == self.material).map_or(self.material.as_str(), |m| m.name(lang));
        format!("{} {material} {} mm", self.place.name(lang), millimetres(self.thickness, lang))
    }

    /// The outside size of a run of `size` with this insulation round it; a lining leaves the
    /// outside as it is.
    pub fn outside(&self, size: &Size) -> (f64, f64) {
        match self.place {
            Place::Insulation => (size.width + 2.0 * self.thickness, size.height + 2.0 * self.thickness),
            Place::Lining => (size.width, size.height),
        }
    }

    /// Whether it holds together: a key without spaces, a material of `materials`, between
    /// 1 and 500 mm thick.
    pub fn check(&self, materials: &[Material]) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        let bad = |what: &str| Err(LibraryError::BadService(format!("{}: {what}", self.id)));
        if self.id.chars().any(char::is_whitespace) {
            return bad("a key without spaces");
        }
        if self.names.as_ref().is_some_and(|names| Lang::ALL.into_iter().any(|lang| names.get(lang).trim().is_empty())) {
            return bad("a name in every language");
        }
        if !materials.iter().any(|m| m.key == self.material) {
            return Err(LibraryError::UnknownMaterial(self.material.clone()));
        }
        if !(self.thickness.is_finite() && (0.001..=0.5).contains(&self.thickness)) {
            return bad("a thickness between 1 and 500 mm");
        }
        Ok(())
    }
}

/// The path of the node of service type `id`.
pub fn path(id: &str) -> String {
    uuid(&format!("openaec-library/servicetype/{id}"))
}

/// The path of the node of cover type `id`.
pub fn cover_path(id: &str) -> String {
    uuid(&format!("openaec-library/covertype/{id}"))
}

/// The node of a service type.
pub fn node(service: &ServiceType) -> Value {
    let (class, predefined) = service.carrier.ifc();
    json!({"path": path(&service.id), "attributes": {
        "bsi::ifc::class": {"code": class},
        "bsi::ifc::prop::Name": service.name(Lang::En),
        "bsi::ifc::prop::PredefinedType": predefined,
        SERVICE_TYPE: service,
        crate::nlsfb::CLASS: crate::nlsfb::class_value(&service.nlsfb()),
    }})
}

/// The node of a cover type, named with `materials`.
pub fn cover_node(cover: &CoverType, materials: &[Material]) -> Value {
    json!({"path": cover_path(&cover.id), "attributes": {
        "bsi::ifc::class": {"code": "IfcCoveringType"},
        "bsi::ifc::prop::Name": cover.name(materials, Lang::En),
        "bsi::ifc::prop::PredefinedType": "INSULATION",
        COVER_TYPE: cover,
    }})
}

/// The service types a library starts with: round and rectangular ducts of galvanised sheet,
/// pipes of copper, steel, PVC, PE and multilayer pipe, a cable tray, a cable ladder and a
/// conduit, each with the series it is commonly made in.
pub fn starter() -> Vec<ServiceType> {
    use Carrier::*;
    let rounds = |sizes: &[(f64, f64)]| sizes.iter().map(|&(d, t)| Size::round(&format!("{d}"), d, t)).collect::<Vec<_>>();
    let boxes = |sizes: &[(f64, f64)], t: f64| sizes.iter().map(|&(w, h)| Size::box_of(w, h, t)).collect::<Vec<_>>();
    let of = |id: &str, carrier, section, material: &str, names: [&str; 4], standard: &str, code: Option<&str>, sizes: Vec<Size>, fittings| ServiceType {
        id: id.into(),
        carrier,
        section,
        material: material.into(),
        names: Names::new(names[0], names[1], names[2], names[3]),
        standard: Some(standard.to_string()),
        sizes,
        fittings,
        code: code.map(str::to_string),
    };
    let f = Fittings::new;
    vec![
        // round ducts of spirally wound sheet: the diameters of EN 1506
        of(
            "duct.spiral-galvanised",
            Duct,
            Section::Round,
            "galvanised-steel",
            ["Spirokanaal, verzinkt staal", "Spiral duct, galvanised steel", "Wickelfalzrohr, verzinkter Stahl", "Conduit spiralé, acier galvanisé"],
            "EN 1506",
            None,
            [63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0].iter().map(|&d: &f64| Size::round(&format!("{d}"), d, if d <= 315.0 { 0.5 } else if d < 1000.0 { 0.8 } else { 1.0 })).collect(),
            f(Bend::Elbow, 1.0, Tee::Tee, Transition::Concentric, Joint::Slip),
        ),
        // rectangular ducts: sides of the series of EN 1505
        of(
            "duct.rectangular-galvanised",
            Duct,
            Section::Rectangular,
            "galvanised-steel",
            ["Rechthoekig kanaal, verzinkt staal", "Rectangular duct, galvanised steel", "Rechteckkanal, verzinkter Stahl", "Conduit rectangulaire, acier galvanisé"],
            "EN 1505",
            None,
            boxes(&[(200.0, 100.0), (300.0, 100.0), (300.0, 150.0), (300.0, 200.0), (400.0, 200.0), (400.0, 300.0), (500.0, 250.0), (500.0, 300.0), (600.0, 300.0), (600.0, 400.0), (800.0, 400.0), (1000.0, 400.0), (1000.0, 500.0), (1200.0, 600.0)], 0.8),
            f(Bend::Elbow, 1.0, Tee::Tee, Transition::Eccentric, Joint::Flange),
        ),
        // copper tube for water and heating: outside diameter by wall, EN 1057
        of(
            "pipe.copper",
            Pipe,
            Section::Round,
            "copper",
            ["Koperen buis", "Copper tube", "Kupferrohr", "Tube en cuivre"],
            "EN 1057",
            Some("53"),
            rounds(&[(12.0, 1.0), (15.0, 1.0), (18.0, 1.0), (22.0, 1.0), (28.0, 1.0), (35.0, 1.2), (42.0, 1.2), (54.0, 1.5)]),
            f(Bend::Elbow, 1.0, Tee::Tee, Transition::Concentric, Joint::Press),
        ),
        // threaded steel tube, medium series: nominal size, outside diameter, wall, EN 10255
        of(
            "pipe.steel-threaded",
            Pipe,
            Section::Round,
            "steel",
            ["Stalen draadpijp, middelzwaar", "Steel tube, threaded, medium series", "Gewinderohr aus Stahl, mittlere Reihe", "Tube en acier fileté, série moyenne"],
            "EN 10255",
            Some("56"),
            [(15, 21.3, 2.6), (20, 26.9, 2.6), (25, 33.7, 3.2), (32, 42.4, 3.2), (40, 48.3, 3.2), (50, 60.3, 3.6), (65, 76.1, 3.6), (80, 88.9, 4.0), (100, 114.3, 4.5), (125, 139.7, 5.0), (150, 165.1, 5.0)].iter().map(|&(dn, d, t): &(u32, f64, f64)| Size::round(&format!("DN {dn}"), d, t)).collect(),
            f(Bend::Elbow, 1.5, Tee::Tee, Transition::Concentric, Joint::Thread),
        ),
        // PVC soil and waste pipe inside a building, EN 1329
        of(
            "pipe.pvc-drain",
            Pipe,
            Section::Round,
            "pvc",
            ["PVC afvoerbuis", "PVC drain pipe", "PVC-Abwasserrohr", "Tuyau d'évacuation en PVC"],
            "EN 1329",
            None,
            rounds(&[(32.0, 3.0), (40.0, 3.0), (50.0, 3.0), (75.0, 3.0), (90.0, 3.0), (110.0, 3.2), (125.0, 3.2), (160.0, 4.0)]),
            f(Bend::Elbow, 1.0, Tee::Wye, Transition::Eccentric, Joint::Glue),
        ),
        // PE pressure pipe for water, SDR 11, EN 12201
        of(
            "pipe.pe-water",
            Pipe,
            Section::Round,
            "polyethylene",
            ["PE drukbuis SDR 11", "PE pressure pipe SDR 11", "PE-Druckrohr SDR 11", "Tube PE pression SDR 11"],
            "EN 12201",
            Some("53"),
            rounds(&[(20.0, 2.0), (25.0, 2.3), (32.0, 3.0), (40.0, 3.7), (50.0, 4.6), (63.0, 5.8), (75.0, 6.8), (90.0, 8.2), (110.0, 10.0)]),
            f(Bend::Bent, 5.0, Tee::Tee, Transition::Concentric, Joint::Weld),
        ),
        // multilayer pipe (PE-RT, aluminium, PE-RT), EN ISO 21003: the walls differ by maker
        of(
            "pipe.multilayer",
            Pipe,
            Section::Round,
            "polyethylene",
            ["Meerlagenbuis PE-RT/Al/PE-RT", "Multilayer pipe PE-RT/Al/PE-RT", "Mehrschichtverbundrohr PE-RT/Al/PE-RT", "Tube multicouche PE-RT/Al/PE-RT"],
            "EN ISO 21003",
            Some("53"),
            rounds(&[(16.0, 2.0), (20.0, 2.25), (26.0, 3.0), (32.0, 3.0), (40.0, 3.5), (50.0, 4.0), (63.0, 4.5)]),
            f(Bend::Bent, 5.0, Tee::Tee, Transition::Concentric, Joint::Press),
        ),
        // cable tray and cable ladder of sheet steel, EN 61537
        of(
            "tray.cable-tray-60",
            CableTray,
            Section::Rectangular,
            "galvanised-steel",
            ["Kabelgoot 60 hoog, verzinkt staal", "Cable tray 60 high, galvanised steel", "Kabelrinne 60 hoch, verzinkter Stahl", "Chemin de câbles hauteur 60, acier galvanisé"],
            "EN 61537",
            None,
            boxes(&[(60.0, 60.0), (100.0, 60.0), (150.0, 60.0), (200.0, 60.0), (300.0, 60.0), (400.0, 60.0), (500.0, 60.0), (600.0, 60.0)], 1.0),
            f(Bend::Elbow, 1.0, Tee::Tee, Transition::Concentric, Joint::Coupler),
        ),
        of(
            "tray.cable-ladder-100",
            CableLadder,
            Section::Rectangular,
            "galvanised-steel",
            ["Kabelladder 100 hoog, verzinkt staal", "Cable ladder 100 high, galvanised steel", "Kabelleiter 100 hoch, verzinkter Stahl", "Échelle à câbles hauteur 100, acier galvanisé"],
            "EN 61537",
            None,
            boxes(&[(200.0, 100.0), (300.0, 100.0), (400.0, 100.0), (500.0, 100.0), (600.0, 100.0)], 1.5),
            f(Bend::Elbow, 1.0, Tee::Tee, Transition::Concentric, Joint::Coupler),
        ),
        // rigid PVC conduit, EN 61386: the Dutch 16 and 19 mm, and 25
        of(
            "conduit.pvc",
            Conduit,
            Section::Round,
            "pvc",
            ["Installatiebuis PVC", "PVC conduit", "PVC-Installationsrohr", "Conduit PVC"],
            "EN 61386",
            None,
            rounds(&[(16.0, 1.5), (19.0, 1.5), (25.0, 1.8)]),
            f(Bend::Bent, 4.0, Tee::Tee, Transition::Concentric, Joint::Slip),
        ),
    ]
}

/// The cover types a library starts with: mineral wool round ducts and pipes, elastomeric
/// foam round cold pipes, and a mineral wool lining inside a duct.
pub fn covers() -> Vec<CoverType> {
    let mut covers: Vec<CoverType> = [25.0, 30.0, 40.0, 50.0, 60.0].iter().map(|&mm: &f64| CoverType::new(&format!("insulation.mineral-wool-{mm}"), Place::Insulation, "mineral-wool", mm)).collect();
    covers.extend([9.0, 13.0, 19.0, 25.0, 32.0].iter().map(|&mm: &f64| CoverType::new(&format!("insulation.elastomeric-foam-{mm}"), Place::Insulation, "elastomeric-foam", mm)));
    covers.extend([25.0, 50.0].iter().map(|&mm: &f64| CoverType::new(&format!("lining.mineral-wool-{mm}"), Place::Lining, "mineral-wool", mm)));
    covers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;
    use std::collections::HashSet;

    #[test]
    fn the_starter_types_hold_together_and_their_sizes_run_up() {
        let materials = crate::material::starter();
        let types = starter();
        assert_eq!(types.iter().map(|t| t.id.as_str()).collect::<HashSet<_>>().len(), types.len(), "keys are unique");
        for t in &types {
            t.check(&materials).unwrap_or_else(|e| panic!("{}: {e}", t.id));
            // smallest first, and each a little open
            assert!(t.sizes.windows(2).all(|pair| pair[0].width * pair[0].height <= pair[1].width * pair[1].height), "{}", t.id);
            assert!(t.sizes.iter().all(|s| s.inside().0 > 0.0 && s.inside().1 > 0.0), "{}", t.id);
            assert!(crate::nlsfb::valid(&t.nlsfb()), "{}", t.id);
            assert_eq!(t.lod(), Lod::L300);
        }
        // every carrier has a type to start from
        assert!(Carrier::ALL.iter().all(|c| types.iter().any(|t| t.carrier == *c)));
        let of = |id: &str| types.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("{id}"));
        // the round duct series of the standard
        let spiral = of("duct.spiral-galvanised");
        assert_eq!(spiral.sizes.iter().map(|s| s.nominal.as_str()).collect::<Vec<_>>(), ["63", "80", "100", "125", "160", "200", "250", "315", "400", "500", "630", "800", "1000", "1250"]);
        assert_eq!((spiral.nlsfb().as_str(), spiral.carrier.ifc()), ("57", ("IfcDuctSegmentType", "RIGIDSEGMENT")));
        // a steel tube: its nominal size is not its diameter
        let steel = of("pipe.steel-threaded");
        let dn25 = steel.size("DN 25").expect("DN 25");
        assert_eq!((dn25.outside(steel.section, Lang::En), dn25.outside(steel.section, Lang::De), millimetres(dn25.wall, Lang::En)), ("Ø 33.7".to_string(), "Ø 33,7".to_string(), "3.2".to_string()));
        assert!((dn25.inside().0 - 0.0273).abs() < 1e-9);
        assert_eq!((steel.nlsfb().as_str(), of("pipe.copper").nlsfb().as_str(), of("pipe.pvc-drain").nlsfb().as_str()), ("56", "53", "52"));
        // its fittings: a drain branches under 45 degrees and is glued; a ladder is coupled
        let drain = of("pipe.pvc-drain").fittings;
        assert_eq!((drain.tee, drain.joint, drain.transition), (Tee::Wye, Joint::Glue, Transition::Eccentric));
        assert_eq!((of("tray.cable-ladder-100").fittings.joint, of("tray.cable-ladder-100").carrier.ifc().1), (Joint::Coupler, "CABLELADDERSEGMENT"));
        assert_eq!((Bend::Segmented.name(Lang::Nl), Tee::Saddle.name(Lang::En), Joint::Press.name(Lang::Nl), Section::Oval.name(Lang::Nl)), ("segmentbocht", "saddle", "persfitting", "ovaal"));
    }

    #[test]
    fn a_size_is_found_and_becomes_a_linear_type() {
        let library = Library::starter();
        let rect = library.service_type("duct.rectangular-galvanised").expect("the duct");
        // what has to pass 350 by 180 takes the first size that is that open inside
        assert_eq!(rect.fitting(0.35, 0.18).map(|s| s.nominal.as_str()), Some("400x200"));
        assert_eq!(rect.fitting(0.3, 0.2).map(|s| s.nominal.as_str()), Some("400x300"), "the sheet takes room");
        assert!(rect.fitting(2.0, 0.1).is_none());
        // a size as a linear type: its key, its name, its role, a hollow profile, its code
        let copper = library.service_type("pipe.copper").expect("copper");
        let run = copper.linear("22", Lang::En).expect("22");
        assert_eq!((run.id.as_str(), run.name.as_deref(), run.role, run.code.as_deref()), ("pipe.copper.22", Some("Copper tube 22"), LinearRole::Pipe, Some("53")));
        assert_eq!(run.profile, ProfileShape::Chs { d: 0.022, t: 0.001 });
        run.check(&library.materials).expect("a linear type of the library's materials");
        let duct = rect.linear("400x200", Lang::Nl).expect("400x200");
        assert_eq!((duct.id.as_str(), duct.name.as_deref(), duct.code.clone()), ("duct.rectangular-galvanised.400x200", Some("Rechthoekig kanaal, verzinkt staal 400x200"), None));
        assert_eq!(duct.profile, ProfileShape::Rhs { h: 0.2, b: 0.4, t: 0.0008, r_out: 0.0, r_in: 0.0 });
        // a tray is drawn by its outline
        let tray = library.service_type("tray.cable-tray-60").expect("the tray");
        assert_eq!(tray.linear("300x60", Lang::En).map(|t| (t.role, t.profile)), Some((LinearRole::CableTray, ProfileShape::Rectangle { width: 0.3, depth: 0.06 })));
        assert!(copper.linear("23", Lang::En).is_none());
    }

    #[test]
    fn a_type_that_does_not_hold_together_is_refused() {
        let mut library = Library::starter();
        let copper = library.service_type("pipe.copper").cloned().expect("copper");
        let with = |change: &dyn Fn(&mut ServiceType)| {
            let mut t = ServiceType { id: "office.pipe".into(), ..copper.clone() };
            change(&mut t);
            t
        };
        assert_eq!(library.add_service_type(copper.clone()), Err(LibraryError::Duplicate("pipe.copper".into())));
        assert_eq!(library.add_service_type(with(&|t| t.material = "unobtainium".into())), Err(LibraryError::UnknownMaterial("unobtainium".into())));
        for (why, change) in [
            ("no size", &(|t: &mut ServiceType| t.sizes.clear()) as &dyn Fn(&mut ServiceType)),
            ("a size twice", &|t: &mut ServiceType| t.sizes.push(t.sizes[0].clone())),
            ("a wall that closes it", &|t: &mut ServiceType| t.sizes[0].wall = 0.006),
            ("round and not as wide as high", &|t: &mut ServiceType| t.sizes[0].height = 0.02),
            ("a name missing", &|t: &mut ServiceType| t.names.de = " ".into()),
            ("a key with a space", &|t: &mut ServiceType| t.id = "office pipe".into()),
            ("a bend of a hundred sizes", &|t: &mut ServiceType| t.fittings.radius = 100.0),
        ] {
            assert!(matches!(library.add_service_type(with(change)), Err(LibraryError::BadService(_))), "{why}");
        }
        assert_eq!(library.add_service_type(with(&|t| t.code = Some("5x".into()))), Err(LibraryError::BadCode("5x".into())));
        library.add_service_type(with(&|t| t.section = Section::Round)).expect("the copy under a key of its own");
        // covers: named by rule, and what they add to a run
        let wool = library.cover_type("insulation.mineral-wool-30").cloned().expect("30 mm of wool");
        assert_eq!((wool.name(&library.materials, Lang::En), wool.name(&library.materials, Lang::Nl)), ("Insulation mineral wool 30 mm".to_string(), "Isolatie minerale wol 30 mm".to_string()));
        let size = copper.size("22").expect("22");
        let (w, h) = wool.outside(size);
        assert!((w - 0.082).abs() < 1e-9 && (h - 0.082).abs() < 1e-9);
        let lining = library.cover_type("lining.mineral-wool-25").expect("a lining");
        assert_eq!((lining.outside(size), lining.name(&library.materials, Lang::De)), ((0.022, 0.022), "Innenauskleidung Mineralwolle 25 mm".to_string()));
        assert!(matches!(library.add_cover_type(CoverType::new("office.cover", Place::Insulation, "mineral-wool", 0.5)), Err(LibraryError::BadService(_))));
        assert_eq!(library.add_cover_type(CoverType::new("office.cover", Place::Insulation, "wool", 30.0)), Err(LibraryError::UnknownMaterial("wool".into())));
        assert_eq!(library.add_cover_type(wool), Err(LibraryError::Duplicate("insulation.mineral-wool-30".into())));
    }

    #[test]
    fn service_and_cover_types_are_found_by_their_keys_after_a_save_and_a_reload() {
        let mut library = Library::starter();
        let mut own = library.service_type("pipe.steel-threaded").cloned().expect("steel");
        own.id = "office.pipe.steel".into();
        own.code = Some("55".into());
        own.sizes.truncate(3);
        own.fittings = Fittings::new(Bend::Segmented, 1.5, Tee::Saddle, Transition::Eccentric, Joint::Weld);
        library.add_service_type(own.clone()).unwrap();
        library.add_cover_type(CoverType { names: Some(Names::same("Armaflex 19")), ..CoverType::new("office.cover.19", Place::Insulation, "elastomeric-foam", 19.0) }).unwrap();
        let file = crate::ifcx::file(&library, "test", "test", "2026-11-20T00:00:00Z");
        let nodes = crate::ifcx::nodes(&library);
        let node = nodes.iter().find(|n| n["path"] == path("pipe.copper")).expect("the node");
        assert_eq!((&node["attributes"]["bsi::ifc::class"]["code"], &node["attributes"]["bsi::ifc::prop::Name"], &node["attributes"][crate::nlsfb::CLASS]["code"]), (&json!("IfcPipeSegmentType"), &json!("Copper tube"), &json!("53")));
        assert_eq!(node["attributes"][SERVICE_TYPE]["sizes"][3], json!({"nominal": "22", "width": 0.022, "height": 0.022, "wall": 0.001}));
        assert_eq!(node["attributes"][SERVICE_TYPE]["fittings"], json!({"bend": "elbow", "radius": 1.0, "tee": "tee", "transition": "concentric", "joint": "press"}));
        let cover = nodes.iter().find(|n| n["path"] == cover_path("lining.mineral-wool-25")).expect("the cover's node");
        assert_eq!((&cover["attributes"]["bsi::ifc::class"]["code"], &cover["attributes"]["bsi::ifc::prop::Name"]), (&json!("IfcCoveringType"), &json!("Lining mineral wool 25 mm")));
        let read = crate::ifcx::read(&file).expect("the library reads");
        assert_eq!(read.service_types, library.service_types);
        assert_eq!(read.cover_types, library.cover_types);
        assert_eq!(read.service_type("office.pipe.steel"), Some(&own));
        assert_eq!(read.cover_type("office.cover.19").map(|c| c.name(&read.materials, Lang::Fr)), Some("Armaflex 19".to_string()));
        // by carrier, by code, and counted at their level
        assert_eq!(Carrier::ALL.map(|c| Library::starter().service_types_of(c).count()), [2, 5, 1, 1, 1]);
        assert!(read.coded("55", false).contains(&"office.pipe.steel") && read.coded("5", true).is_empty());
        assert!(read.coded("57", false).contains(&"duct.spiral-galvanised"));
        // a library merged into another brings the types that one lacks, once
        let mut other = Library::starter();
        assert_eq!(other.merge(&read), 2);
        assert_eq!(other.merge(&read), 0);
    }
}
