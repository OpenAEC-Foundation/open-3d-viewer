//! Object types: the things of a building that are placed as one piece: doors and windows,
//! stairs, sanitary and kitchen fittings, furniture, and the terminals and appliances of the
//! services (radiators, heat pumps, ventilation units and valves, luminaires, sockets, smoke
//! detectors, lifts, solar panels).
//!
//! An object type is of a **group** (what it is, and so its discipline, its IFC class and its
//! NL-SfB group), has a size (width, depth, height: the box it takes), the height it is
//! mounted at, what it is made of, properties of its own (a luminaire's power, a valve's air
//! flow) and its level of development ([`crate::lod`]).
//!
//! - LOD 200: a generic object of about the right size, to reserve its place ("toilet").
//! - LOD 300: a specific object: a standard size or a kind of its own ("wall-hung toilet 360 ×
//!   540", "inner door 880 × 2315").
//! - LOD 350 and 400 are for objects with their connections and their parts worked out; the
//!   library's own objects stop at LOD 300, a manufacturer's go further.
//!
//! The suite draws an object as a box of its size. A door or a window cuts an opening of its
//! width and height in the wall it stands in.
//!
//! In IFCX an object type is a node of its type class (`IfcDoorType`, `IfcLightFixtureType`,
//! ...) with its predefined type, its English name, its NL-SfB code as `nlsfb::class` and
//! `openaec::objecttype` (all of it). A library holds its object types
//! ([`crate::Library::object_types`]).

use crate::lod::Lod;
use crate::material::Names;
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a type node with its object type.
pub const OBJECT_TYPE: &str = "openaec::objecttype";

/// What an object is of: architecture, or one of the services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trade {
    Architecture,
    Interior,
    Heating,
    Ventilation,
    Plumbing,
    Electrical,
    Safety,
    Transport,
}

impl Trade {
    pub const ALL: [Trade; 8] = [Trade::Architecture, Trade::Interior, Trade::Heating, Trade::Ventilation, Trade::Plumbing, Trade::Electrical, Trade::Safety, Trade::Transport];

    /// Its name in Dutch and in English.
    pub fn name(self, lang: Lang) -> &'static str {
        let (nl, en) = match self {
            Trade::Architecture => ("bouwkundig", "architecture"),
            Trade::Interior => ("inrichting", "interior"),
            Trade::Heating => ("verwarming en koeling", "heating and cooling"),
            Trade::Ventilation => ("ventilatie", "ventilation"),
            Trade::Plumbing => ("water en afvoer", "plumbing and drainage"),
            Trade::Electrical => ("elektra en verlichting", "electrical and lighting"),
            Trade::Safety => ("brand en beveiliging", "fire and security"),
            Trade::Transport => ("transport", "transport"),
        };
        if lang == Lang::Nl {
            nl
        } else {
            en
        }
    }
}

/// What an object is: its group in the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObjectGroup {
    Door,
    Window,
    Stair,
    Railing,
    Sanitary,
    Kitchen,
    Furniture,
    Appliance,
    /// What gives off heat or cold in a room: radiators, convectors, floor heating manifolds.
    HeatEmitter,
    /// What makes heat or cold: boilers, heat pumps, chillers, buffer tanks.
    HeatSource,
    /// Units that move and treat air: heat recovery units, fans, air handling units.
    VentilationUnit,
    /// Where air enters or leaves a room: valves, grilles, diffusers.
    AirTerminal,
    /// Taps, water heaters, pumps, meters.
    WaterFitting,
    /// Floor gullies, roof outlets, inspection chambers.
    Drain,
    /// Distribution boards, inverters, charging points.
    ElectricalEquipment,
    /// Sockets, switches, data outlets.
    Outlet,
    Luminaire,
    SolarPanel,
    /// Detectors, call points, extinguishers, hose reels, emergency lighting.
    FireSafety,
    /// Cameras, access control, intercoms.
    Security,
    Lift,
}

impl ObjectGroup {
    pub const ALL: [ObjectGroup; 21] = [
        ObjectGroup::Door,
        ObjectGroup::Window,
        ObjectGroup::Stair,
        ObjectGroup::Railing,
        ObjectGroup::Sanitary,
        ObjectGroup::Kitchen,
        ObjectGroup::Furniture,
        ObjectGroup::Appliance,
        ObjectGroup::HeatEmitter,
        ObjectGroup::HeatSource,
        ObjectGroup::VentilationUnit,
        ObjectGroup::AirTerminal,
        ObjectGroup::WaterFitting,
        ObjectGroup::Drain,
        ObjectGroup::ElectricalEquipment,
        ObjectGroup::Outlet,
        ObjectGroup::Luminaire,
        ObjectGroup::SolarPanel,
        ObjectGroup::FireSafety,
        ObjectGroup::Security,
        ObjectGroup::Lift,
    ];

    pub fn trade(self) -> Trade {
        use ObjectGroup::*;
        match self {
            Door | Window | Stair | Railing => Trade::Architecture,
            Sanitary | Kitchen | Furniture | Appliance => Trade::Interior,
            HeatEmitter | HeatSource => Trade::Heating,
            VentilationUnit | AirTerminal => Trade::Ventilation,
            WaterFitting | Drain => Trade::Plumbing,
            ElectricalEquipment | Outlet | Luminaire | SolarPanel => Trade::Electrical,
            FireSafety | Security => Trade::Safety,
            Lift => Trade::Transport,
        }
    }

    /// Its name in Dutch and in English, in the plural: what the library's tree lists.
    pub fn name(self, lang: Lang) -> &'static str {
        use ObjectGroup::*;
        let (nl, en) = match self {
            Door => ("deuren", "doors"),
            Window => ("ramen", "windows"),
            Stair => ("trappen", "stairs"),
            Railing => ("balustrades en leuningen", "railings"),
            Sanitary => ("sanitair", "sanitary fittings"),
            Kitchen => ("keukens", "kitchens"),
            Furniture => ("meubilair", "furniture"),
            Appliance => ("apparaten", "appliances"),
            HeatEmitter => ("radiatoren en afgifte", "heat emitters"),
            HeatSource => ("opwekkers", "heat and cold sources"),
            VentilationUnit => ("ventilatie-units", "ventilation units"),
            AirTerminal => ("ventielen en roosters", "air terminals"),
            WaterFitting => ("kranen en toestellen", "water fittings"),
            Drain => ("putten en afvoeren", "drains"),
            ElectricalEquipment => ("verdelers en apparatuur", "electrical equipment"),
            Outlet => ("wandcontactdozen en schakelaars", "sockets and switches"),
            Luminaire => ("verlichting", "luminaires"),
            SolarPanel => ("zonnepanelen", "solar panels"),
            FireSafety => ("brandveiligheid", "fire safety"),
            Security => ("beveiliging", "security"),
            Lift => ("liften", "lifts"),
        };
        if lang == Lang::Nl {
            nl
        } else {
            en
        }
    }

    /// The IFC class (IFC4.3) of its objects and of their type, and the predefined type an
    /// object has unless it says another.
    pub fn ifc(self) -> (&'static str, &'static str, &'static str) {
        use ObjectGroup::*;
        match self {
            Door => ("IfcDoor", "IfcDoorType", "DOOR"),
            Window => ("IfcWindow", "IfcWindowType", "WINDOW"),
            Stair => ("IfcStair", "IfcStairType", "STRAIGHT_RUN_STAIR"),
            Railing => ("IfcRailing", "IfcRailingType", "BALUSTRADE"),
            Sanitary => ("IfcSanitaryTerminal", "IfcSanitaryTerminalType", "NOTDEFINED"),
            Kitchen | Furniture => ("IfcFurniture", "IfcFurnitureType", "NOTDEFINED"),
            Appliance => ("IfcElectricAppliance", "IfcElectricApplianceType", "NOTDEFINED"),
            HeatEmitter => ("IfcSpaceHeater", "IfcSpaceHeaterType", "RADIATOR"),
            HeatSource => ("IfcBoiler", "IfcBoilerType", "NOTDEFINED"),
            VentilationUnit => ("IfcUnitaryEquipment", "IfcUnitaryEquipmentType", "AIRHANDLER"),
            AirTerminal => ("IfcAirTerminal", "IfcAirTerminalType", "DIFFUSER"),
            WaterFitting => ("IfcValve", "IfcValveType", "FAUCET"),
            Drain => ("IfcWasteTerminal", "IfcWasteTerminalType", "FLOORWASTE"),
            ElectricalEquipment => ("IfcElectricDistributionBoard", "IfcElectricDistributionBoardType", "DISTRIBUTIONBOARD"),
            Outlet => ("IfcOutlet", "IfcOutletType", "POWEROUTLET"),
            Luminaire => ("IfcLightFixture", "IfcLightFixtureType", "POINTSOURCE"),
            SolarPanel => ("IfcSolarDevice", "IfcSolarDeviceType", "SOLARPANEL"),
            FireSafety => ("IfcSensor", "IfcSensorType", "SMOKESENSOR"),
            Security => ("IfcAudioVisualAppliance", "IfcAudioVisualApplianceType", "CAMERA"),
            Lift => ("IfcTransportElement", "IfcTransportElementType", "ELEVATOR"),
        }
    }

    /// The NL-SfB group its objects are of unless they say another code.
    pub fn code(self) -> &'static str {
        use ObjectGroup::*;
        match self {
            Door => "32",
            Window => "31",
            Stair => "24",
            Railing => "34",
            Sanitary => "74",
            Kitchen => "73",
            Furniture => "82",
            Appliance => "83",
            HeatEmitter => "56",
            HeatSource => "51",
            VentilationUnit | AirTerminal => "57",
            WaterFitting => "53",
            Drain => "52",
            ElectricalEquipment => "61",
            Outlet => "62",
            Luminaire => "63",
            SolarPanel => "61",
            FireSafety | Security => "65",
            Lift => "66",
        }
    }

    /// Whether its objects cut an opening in the wall they stand in.
    pub fn opening(self) -> bool {
        matches!(self, ObjectGroup::Door | ObjectGroup::Window)
    }
}

/// An object type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectType {
    /// Stable and never shown: what elements refer to.
    pub id: String,
    pub group: ObjectGroup,
    pub names: Names,
    /// Its width, its depth and its height, in metres: the box it takes.
    pub size: [f64; 3],
    /// How high above the floor its underside is mounted, in metres.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub base: f64,
    pub lod: Lod,
    /// The IFC predefined type, where it is not its group's ("WASHHANDBASIN").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predefined: Option<String>,
    /// The IFC class of its objects and of its type, where it is not its group's (a heat pump
    /// among the heat sources: "IfcHeatPump").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// The key of the material it is of, where the library has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// An NL-SfB code of its own, in place of its group's ("31.2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Properties of its own, by their name and with their unit ("Power", "40 W").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<(String, String)>,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl ObjectType {
    /// An object of `group` at LOD 200: generic, of about `size`.
    pub fn new(id: &str, group: ObjectGroup, names: Names, size: [f64; 3]) -> Self {
        Self { id: id.into(), group, names, size, base: 0.0, lod: Lod::L200, predefined: None, class: None, material: None, code: None, properties: Vec::new() }
    }

    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// The IFC class of its objects, the class of its type and its predefined type.
    pub fn ifc(&self) -> (String, String, String) {
        let (class, type_class, predefined) = self.group.ifc();
        let (class, type_class) = match &self.class {
            Some(own) => (own.clone(), format!("{own}Type")),
            None => (class.to_string(), type_class.to_string()),
        };
        (class, type_class, self.predefined.clone().unwrap_or_else(|| predefined.to_string()))
    }

    /// Its NL-SfB code: its own, else its group's.
    pub fn nlsfb(&self) -> String {
        self.code.clone().unwrap_or_else(|| self.group.code().to_string())
    }

    /// Whether it holds together: an id, a name in every language, a box of positive sizes no
    /// larger than a building part (50 m), a mounting height that is a number, a code that is
    /// one, an IFC class that is one.
    pub fn check(&self) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        let bad = |what: &str| Err(LibraryError::BadObject(format!("{}: {what}", self.id)));
        if Lang::ALL.into_iter().any(|lang| self.name(lang).trim().is_empty()) {
            return bad("a name in every language");
        }
        if self.size.iter().any(|v| !v.is_finite() || *v < crate::MIN_THICKNESS || *v > 50.0) {
            return bad("a width, a depth and a height between 0.1 mm and 50 m");
        }
        if !self.base.is_finite() || self.base.abs() > 500.0 {
            return bad("a mounting height");
        }
        if let Some(code) = &self.code {
            if !crate::nlsfb::valid(code) {
                return Err(LibraryError::BadCode(code.clone()));
            }
        }
        if self.class.as_ref().is_some_and(|class| !class.starts_with("Ifc") || class.len() < 5 || !class.chars().all(|c| c.is_ascii_alphanumeric())) {
            return bad("an IFC class");
        }
        Ok(())
    }
}

/// The path of the node of object type `id`.
pub fn path(id: &str) -> String {
    uuid(&format!("openaec-library/objecttype/{id}"))
}

/// The node of an object type.
pub fn node(object: &ObjectType) -> Value {
    let (_, type_class, predefined) = object.ifc();
    json!({
        "path": path(&object.id),
        "attributes": {
            "bsi::ifc::class": {"code": type_class},
            "bsi::ifc::prop::Name": object.name(Lang::En),
            "bsi::ifc::prop::PredefinedType": predefined,
            crate::nlsfb::CLASS: crate::nlsfb::class_value(&object.nlsfb()),
            OBJECT_TYPE: object,
        }
    })
}

/// The object types a library starts with: one generic object of each group at LOD 200, to
/// reserve a place with. The catalogues of specific objects are library files of their own.
pub fn starter() -> Vec<ObjectType> {
    use ObjectGroup::*;
    let of = |group: ObjectGroup, key: &str, nl: &str, en: &str, de: &str, fr: &str, size: [f64; 3], base: f64| ObjectType { base, ..ObjectType::new(&format!("object.{key}"), group, Names::new(nl, en, de, fr), size) };
    vec![
        of(Door, "door", "deur", "door", "Tür", "porte", [0.93, 0.1, 2.315], 0.0),
        of(Window, "window", "raam", "window", "Fenster", "fenêtre", [1.2, 0.1, 1.5], 0.9),
        of(Stair, "stair", "trap", "stair", "Treppe", "escalier", [1.0, 3.6, 2.8], 0.0),
        of(Railing, "railing", "balustrade", "railing", "Geländer", "garde-corps", [2.0, 0.05, 1.0], 0.0),
        of(Sanitary, "toilet", "toilet", "toilet", "WC", "WC", [0.4, 0.55, 0.4], 0.0),
        of(Kitchen, "kitchen-unit", "keukenkast", "kitchen unit", "Küchenschrank", "meuble de cuisine", [0.6, 0.6, 0.9], 0.0),
        of(Furniture, "table", "tafel", "table", "Tisch", "table", [1.6, 0.9, 0.75], 0.0),
        of(Appliance, "appliance", "apparaat", "appliance", "Gerät", "appareil", [0.6, 0.6, 0.85], 0.0),
        of(HeatEmitter, "radiator", "radiator", "radiator", "Heizkörper", "radiateur", [1.0, 0.1, 0.6], 0.15),
        of(HeatSource, "heat-source", "opwekker", "heat source", "Wärmeerzeuger", "générateur de chaleur", [0.6, 0.6, 1.8], 0.0),
        of(VentilationUnit, "ventilation-unit", "ventilatie-unit", "ventilation unit", "Lüftungsgerät", "centrale de ventilation", [0.75, 0.55, 0.85], 0.0),
        of(AirTerminal, "air-terminal", "ventiel", "air terminal", "Luftdurchlass", "bouche d'air", [0.15, 0.15, 0.05], 2.55),
        of(WaterFitting, "tap", "kraan", "tap", "Armatur", "robinet", [0.15, 0.2, 0.2], 0.9),
        of(Drain, "floor-gully", "vloerput", "floor gully", "Bodenablauf", "siphon de sol", [0.15, 0.15, 0.1], 0.0),
        of(ElectricalEquipment, "distribution-board", "groepenkast", "distribution board", "Verteiler", "tableau électrique", [0.4, 0.15, 0.6], 1.2),
        of(Outlet, "socket", "wandcontactdoos", "socket", "Steckdose", "prise", [0.08, 0.04, 0.08], 0.3),
        of(Luminaire, "luminaire", "armatuur", "luminaire", "Leuchte", "luminaire", [0.6, 0.6, 0.08], 2.52),
        of(SolarPanel, "solar-panel", "zonnepaneel", "solar panel", "Solarmodul", "panneau solaire", [1.1, 1.75, 0.035], 0.0),
        of(FireSafety, "smoke-detector", "rookmelder", "smoke detector", "Rauchmelder", "détecteur de fumée", [0.1, 0.1, 0.05], 2.55),
        of(Security, "camera", "camera", "camera", "Kamera", "caméra", [0.1, 0.15, 0.1], 2.4),
        of(Lift, "lift", "lift", "lift", "Aufzug", "ascenseur", [1.6, 1.8, 2.3], 0.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_group_has_a_trade_a_class_a_code_and_a_generic_object() {
        let objects = starter();
        assert_eq!(objects.len(), ObjectGroup::ALL.len());
        let ids: HashSet<&str> = objects.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids.len(), objects.len());
        for group in ObjectGroup::ALL {
            let (class, type_class, predefined) = group.ifc();
            assert!(class.starts_with("Ifc") && type_class == format!("{class}Type") && !predefined.is_empty(), "{group:?}");
            assert!(crate::nlsfb::valid(group.code()) && !group.name(Lang::Nl).is_empty() && !group.name(Lang::De).is_empty(), "{group:?}");
            let generic = objects.iter().find(|o| o.group == group).unwrap_or_else(|| panic!("{group:?}"));
            generic.check().unwrap_or_else(|e| panic!("{}: {e}", generic.id));
            assert_eq!(generic.lod, Lod::L200, "{}", generic.id);
        }
        // every trade has groups; doors and windows cut openings
        for trade in Trade::ALL {
            assert!(ObjectGroup::ALL.iter().any(|g| g.trade() == trade), "{trade:?}");
        }
        assert_eq!(ObjectGroup::ALL.iter().filter(|g| g.opening()).count(), 2);
        assert_eq!((ObjectGroup::Luminaire.trade(), ObjectGroup::AirTerminal.trade(), ObjectGroup::Luminaire.name(Lang::Nl)), (Trade::Electrical, Trade::Ventilation, "verlichting"));
    }

    #[test]
    fn an_object_says_its_own_class_code_and_properties() {
        let mut pump = ObjectType::new("nl.heat-pump-air-water", ObjectGroup::HeatSource, Names::new("lucht-waterwarmtepomp", "air-to-water heat pump", "Luft-Wasser-Wärmepumpe", "pompe à chaleur air-eau"), [1.1, 0.45, 0.9]);
        assert_eq!(pump.ifc(), ("IfcBoiler".to_string(), "IfcBoilerType".to_string(), "NOTDEFINED".to_string()));
        pump.class = Some("IfcHeatPump".into());
        pump.lod = Lod::L300;
        pump.code = Some("51.5".into());
        pump.properties = vec![("Power".into(), "6 kW".into())];
        pump.check().unwrap();
        assert_eq!((pump.ifc(), pump.nlsfb().as_str()), (("IfcHeatPump".to_string(), "IfcHeatPumpType".to_string(), "NOTDEFINED".to_string()), "51.5"));
        let written = node(&pump);
        assert_eq!((written["attributes"]["bsi::ifc::class"]["code"].as_str(), written["attributes"]["nlsfb::class"]["code"].as_str(), written["attributes"][OBJECT_TYPE]["lod"].as_str()), (Some("IfcHeatPumpType"), Some("51.5"), Some("300")));
        assert_eq!(serde_json::from_value::<ObjectType>(written["attributes"][OBJECT_TYPE].clone()).unwrap(), pump);
        // what is left open is not written
        let generic = &starter()[0];
        let own = node(generic)["attributes"][OBJECT_TYPE].clone();
        assert!(own.get("class").is_none() && own.get("code").is_none() && own.get("properties").is_none() && own.get("base").is_none());
    }

    #[test]
    fn what_does_not_hold_together_is_refused() {
        let ok = starter().remove(0);
        let refused = |change: &dyn Fn(&mut ObjectType)| {
            let mut object = ok.clone();
            change(&mut object);
            object.check().is_err()
        };
        assert!(refused(&|o| o.id.clear()));
        assert!(refused(&|o| o.names.fr.clear()));
        assert!(refused(&|o| o.size[1] = 0.0) && refused(&|o| o.size[2] = 80.0) && refused(&|o| o.size[0] = f64::NAN));
        assert!(refused(&|o| o.code = Some("deur".into())));
        assert!(refused(&|o| o.class = Some("Door".into())) && refused(&|o| o.class = Some("Ifc Door".into())));
        assert!(refused(&|o| o.base = f64::INFINITY));
    }
}
