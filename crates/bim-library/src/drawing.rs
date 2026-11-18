//! Drawing standards: the kinds of drawing an office makes and the rules each is drawn by, and
//! under them the settings those rules pick from: the scales, the line styles, the line widths
//! (pens) and the text styles.
//!
//! A standard belongs to a country. The suite starts with the Dutch one ([`Standard::netherlands`]):
//!
//! - The kinds, their views, scales and numbers are those of the drawings and sheets of the INB
//!   template of the OpenAEC Foundation (`INB-Template 0.6.ifc`). A kind the template has no
//!   drawing of is added from Dutch practice and says so ([`Origin::Practice`]); its scales are a
//!   proposal.
//! - The line styles are the basic line types of ISO 128-2, their dashes and gaps as multiples of
//!   the line width, as that standard gives them.
//! - The line widths and the text heights are the ISO series. What is cut is drawn twice as wide
//!   as what is seen beyond, the ratio of wide to narrow lines of ISO 128.
//! - The hatches are those of the country's standard ([`DrawingStandard`]).
//!
//! Nothing here is fixed: a standard is data an office changes, kept in IFCX ([`nodes`],
//! [`read`]), one node per setting, so a file can hold a whole standard or one changed pen.

use crate::material::{DrawingStandard, Names};
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The attribute of a standard's own node.
pub const STANDARD: &str = "openaec::drawing::standard";
/// The attribute of a kind of drawing.
pub const KIND: &str = "openaec::drawing::kind";
/// The attribute of a line style.
pub const LINE_STYLE: &str = "openaec::drawing::linestyle";
/// The attribute of a pen.
pub const PEN: &str = "openaec::drawing::pen";
/// The attribute of a text style.
pub const TEXT_STYLE: &str = "openaec::drawing::textstyle";

/// What a drawing is a view of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    /// Seen from above, cut at a height.
    Plan,
    /// A ceiling seen from below, drawn as a plan.
    CeilingPlan,
    Section,
    Elevation,
    /// The model in 3D.
    Model,
}

impl View {
    pub const ALL: [View; 5] = [View::Plan, View::CeilingPlan, View::Section, View::Elevation, View::Model];
}

/// Whose drawing it is. Its code stands in a drawing's name (`01_BWK_PLG_00_begane_grond`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Discipline {
    General,
    Architecture,
    Structure,
    Services,
}

impl Discipline {
    pub const ALL: [Discipline; 4] = [Discipline::General, Discipline::Architecture, Discipline::Structure, Discipline::Services];

    /// Its code in the Netherlands, as the INB template writes it; the template has no drawings
    /// of services, so `INS` is the suite's.
    pub fn code(self) -> &'static str {
        match self {
            Discipline::General => "ALG",
            Discipline::Architecture => "BWK",
            Discipline::Structure => "CON",
            Discipline::Services => "INS",
        }
    }
}

/// Where a kind of drawing comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    /// The country's template has drawings of this kind.
    Template,
    /// Usual in practice; the template has none, and its scales are a proposal.
    Practice,
    /// Added by the office.
    Own,
}

/// A line style: a continuous line, or dashes and gaps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStyle {
    pub key: String,
    pub names: Names,
    /// Dash, gap, dash, gap, ... as multiples of the line's width; empty for a continuous line.
    pub dashes: Vec<f64>,
}

/// A pen: a line width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pen {
    pub key: String,
    pub names: Names,
    /// Millimetres on paper.
    pub width: f64,
}

/// A text style: a text height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub key: String,
    pub names: Names,
    /// Millimetres on paper.
    pub height: f64,
}

/// A kind of drawing and the rules it is drawn by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingKind {
    pub key: String,
    pub names: Names,
    /// Its code in a drawing's name ("PLG", "aanzicht").
    pub code: String,
    pub discipline: Discipline,
    pub view: View,
    /// The scales it is drawn at, as 1 to this, the usual one first.
    pub scales: Vec<u32>,
    /// The series its drawings are numbered in ("01"), or empty.
    #[serde(default)]
    pub number: String,
    /// The pen of what is cut.
    pub cut: String,
    /// The pen of what is seen beyond the cut.
    pub beyond: String,
    /// The line style of what is hidden.
    pub hidden: String,
    /// The text style of its texts and dimensions.
    pub text: String,
    /// Whether what is cut is hatched by its material.
    pub hatched: bool,
    pub origin: Origin,
}

impl DrawingKind {
    /// Its usual scale as it is written: "1:50".
    pub fn scale_text(&self) -> String {
        self.scales.first().map(|s| scale_text(*s)).unwrap_or_default()
    }
}

/// A scale as it is written: "1:50".
pub fn scale_text(scale: u32) -> String {
    format!("1:{scale}")
}

/// A drawing standard: its kinds of drawing and the settings they pick from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Standard {
    pub key: String,
    pub names: Names,
    /// The standard its hatches follow.
    pub hatches: DrawingStandard,
    /// The scales drawings are made at, as 1 to this.
    pub scales: Vec<u32>,
    /// The number of its first sheet; the next ones count on.
    pub first_sheet: u32,
    #[serde(default, skip_serializing)]
    pub line_styles: Vec<LineStyle>,
    #[serde(default, skip_serializing)]
    pub pens: Vec<Pen>,
    #[serde(default, skip_serializing)]
    pub text_styles: Vec<TextStyle>,
    #[serde(default, skip_serializing)]
    pub kinds: Vec<DrawingKind>,
}

impl Standard {
    /// The Dutch standard the suite starts with (see the module).
    pub fn netherlands() -> Self {
        let style = |key: &str, nl: &str, en: &str, de: &str, fr: &str, dashes: &[f64]| LineStyle { key: key.into(), names: Names::new(nl, en, de, fr), dashes: dashes.to_vec() };
        let pen = |width: f64| {
            let name = format!("{width} mm");
            Pen { key: format!("{width}"), names: Names::same(&name), width }
        };
        let text = |height: f64| {
            let name = format!("{height} mm");
            TextStyle { key: format!("{height}"), names: Names::same(&name), height }
        };
        // what is cut twice as wide as what is seen beyond, the wider the larger the scale
        let pens = |scale: u32| match scale {
            0..=20 => ("0.7", "0.35"),
            21..=50 => ("0.5", "0.25"),
            _ => ("0.35", "0.18"),
        };
        #[allow(clippy::too_many_arguments)]
        let kind = |key: &str, names: Names, code: &str, discipline, view, scales: &[u32], number: &str, origin| {
            let (cut, beyond) = pens(scales[0]);
            DrawingKind {
                key: key.into(),
                names,
                code: code.into(),
                discipline,
                view,
                scales: scales.to_vec(),
                number: number.into(),
                cut: cut.into(),
                beyond: beyond.into(),
                hidden: "dashed".into(),
                text: "2.5".into(),
                hatched: !matches!(view, View::Elevation | View::Model),
                origin,
            }
        };
        use Discipline::{Architecture, General, Services, Structure};
        use Origin::{Practice, Template};
        Standard {
            key: "nl".into(),
            names: Names::new("Nederland", "Netherlands", "Niederlande", "Pays-Bas"),
            hatches: DrawingStandard::Nen47,
            // the scales the template has a style sheet for, and the two of a site plan
            scales: vec![1, 2, 5, 10, 20, 50, 100, 200, 500, 1000],
            first_sheet: 300,
            // ISO 128-2: types 01, 02, 04, 05 and 07; dots 0.5, gaps 3, dashes 12 and long dashes
            // 24 times the line width
            line_styles: vec![
                style("continuous", "doorgetrokken", "continuous", "Volllinie", "continu", &[]),
                style("dashed", "streeplijn", "dashed", "Strichlinie", "interrompu", &[12.0, 3.0]),
                style("chain", "streep-stiplijn", "long-dashed dotted", "Strich-Punktlinie", "mixte", &[24.0, 3.0, 0.5, 3.0]),
                style("double-chain", "streep-stip-stiplijn", "long-dashed double-dotted", "Strich-Zweipunktlinie", "mixte à deux points", &[24.0, 3.0, 0.5, 3.0, 0.5, 3.0]),
                style("dotted", "stippellijn", "dotted", "Punktlinie", "pointillé", &[0.5, 3.0]),
            ],
            pens: [0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1.0].map(pen).to_vec(),
            text_styles: [1.8, 2.5, 3.5, 5.0, 7.0].map(text).to_vec(),
            kinds: vec![
                kind("overview", Names::new("overzicht", "overview", "Übersicht", "vue d'ensemble"), "totaaloverzicht", General, View::Plan, &[50], "", Template),
                kind("floor-plan", Names::new("plattegrond", "floor plan", "Grundriss", "plan d'étage"), "PLG", Architecture, View::Plan, &[50, 100], "01", Template),
                kind("foundation-plan", Names::new("funderingsplattegrond", "foundation plan", "Fundamentplan", "plan de fondation"), "PLG", Architecture, View::Plan, &[50], "01", Template),
                kind("area-plan", Names::new("BVO-plattegrond", "gross floor area plan", "BGF-Plan", "plan des surfaces brutes"), "BVO", Architecture, View::Plan, &[50], "02", Template),
                kind("elevation", Names::new("aanzicht", "elevation", "Ansicht", "élévation"), "aanzicht", Architecture, View::Elevation, &[50, 100], "", Template),
                kind("section", Names::new("doorsnede", "section", "Schnitt", "coupe"), "doorsnede", Architecture, View::Section, &[50, 100], "", Template),
                kind("detail", Names::new("detail", "detail", "Detail", "détail"), "detail", Architecture, View::Section, &[5, 10], "", Template),
                kind("structural-plan", Names::new("constructieve plattegrond", "structural plan", "Tragwerksplan", "plan de structure"), "PLG", Structure, View::Plan, &[50], "01", Template),
                kind("pile-plan", Names::new("palenplan", "pile plan", "Pfahlplan", "plan des pieux"), "PLG", Structure, View::Plan, &[100], "01", Template),
                kind("structural-detail", Names::new("constructief detail", "structural detail", "Tragwerksdetail", "détail de structure"), "details", Structure, View::Section, &[10], "", Template),
                kind("legend", Names::new("renvooi", "legend", "Legende", "légende"), "renvooi", Structure, View::Plan, &[50, 100], "", Template),
                kind("model", Names::new("3D-aanzicht", "3D view", "3D-Ansicht", "vue 3D"), "3D", General, View::Model, &[100], "", Template),
                kind("site-plan", Names::new("situatie", "site plan", "Lageplan", "plan de situation"), "situatie", General, View::Plan, &[500, 1000, 200], "", Practice),
                kind("roof-plan", Names::new("dakplan", "roof plan", "Dachaufsicht", "plan de toiture"), "dakplan", Architecture, View::Plan, &[100, 50], "", Practice),
                kind("ceiling-plan", Names::new("plafondplan", "ceiling plan", "Deckenspiegel", "plan de plafond"), "plafond", Architecture, View::CeilingPlan, &[50, 100], "", Practice),
                kind("fire-plan", Names::new("brandveiligheidsplattegrond", "fire safety plan", "Brandschutzplan", "plan de sécurité incendie"), "brand", Architecture, View::Plan, &[100, 200], "", Practice),
                kind("demolition-plan", Names::new("sloopplattegrond", "demolition plan", "Abbruchplan", "plan de démolition"), "sloop", Architecture, View::Plan, &[100, 50], "", Practice),
                kind("reinforcement", Names::new("wapeningstekening", "reinforcement drawing", "Bewehrungsplan", "plan de ferraillage"), "wapening", Structure, View::Plan, &[50, 20], "", Practice),
                kind("services-plan", Names::new("installatieplattegrond", "services plan", "Installationsplan", "plan des installations"), "PLG", Services, View::Plan, &[50, 100], "", Practice),
                kind("drainage-plan", Names::new("rioleringsplattegrond", "drainage plan", "Entwässerungsplan", "plan d'assainissement"), "riolering", Services, View::Plan, &[100, 50], "", Practice),
            ],
        }
    }

    pub fn kind(&self, key: &str) -> Option<&DrawingKind> {
        self.kinds.iter().find(|k| k.key == key)
    }

    pub fn pen(&self, key: &str) -> Option<&Pen> {
        self.pens.iter().find(|p| p.key == key)
    }

    pub fn line_style(&self, key: &str) -> Option<&LineStyle> {
        self.line_styles.iter().find(|s| s.key == key)
    }

    pub fn text_style(&self, key: &str) -> Option<&TextStyle> {
        self.text_styles.iter().find(|t| t.key == key)
    }

    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// Whether the standard holds together: no key empty or twice, every width, height, dash
    /// and scale positive, and every kind's scales, pens and styles those of the standard.
    pub fn check(&self) -> Result<(), LibraryError> {
        fn keys<'a>(keys: impl Iterator<Item = &'a str>) -> Result<(), LibraryError> {
            let mut seen: Vec<&str> = Vec::new();
            for key in keys {
                if key.trim().is_empty() {
                    return Err(LibraryError::NoKey);
                }
                if seen.contains(&key) {
                    return Err(LibraryError::Duplicate(key.to_string()));
                }
                seen.push(key);
            }
            Ok(())
        }
        let positive = |x: f64| x.is_finite() && x > 0.0;
        keys(std::iter::once(self.key.as_str()))?;
        keys(self.line_styles.iter().map(|s| s.key.as_str()))?;
        keys(self.pens.iter().map(|p| p.key.as_str()))?;
        keys(self.text_styles.iter().map(|t| t.key.as_str()))?;
        keys(self.kinds.iter().map(|k| k.key.as_str()))?;
        if self.scales.contains(&0) || !self.pens.iter().all(|p| positive(p.width)) || !self.text_styles.iter().all(|t| positive(t.height)) || !self.line_styles.iter().all(|s| s.dashes.iter().all(|d| positive(*d))) {
            return Err(LibraryError::NotPositive);
        }
        // dashes come with their gaps
        if let Some(style) = self.line_styles.iter().find(|s| s.dashes.len() % 2 == 1) {
            return Err(LibraryError::UnknownSetting(style.key.clone()));
        }
        for kind in &self.kinds {
            if kind.scales.is_empty() {
                return Err(LibraryError::UnknownSetting(format!("{}: scale", kind.key)));
            }
            if let Some(scale) = kind.scales.iter().find(|s| !self.scales.contains(s)) {
                return Err(LibraryError::UnknownSetting(scale_text(*scale)));
            }
            for pen in [&kind.cut, &kind.beyond] {
                if self.pen(pen).is_none() {
                    return Err(LibraryError::UnknownSetting(pen.clone()));
                }
            }
            if self.line_style(&kind.hidden).is_none() {
                return Err(LibraryError::UnknownSetting(kind.hidden.clone()));
            }
            if self.text_style(&kind.text).is_none() {
                return Err(LibraryError::UnknownSetting(kind.text.clone()));
            }
        }
        Ok(())
    }
}

/// The path of a node of standard `standard`: its own (`what` "standard", `key` empty) or one of
/// its settings.
pub fn path(standard: &str, what: &str, key: &str) -> String {
    uuid(&format!("openaec-library/drawing/{standard}/{what}/{key}"))
}

/// The nodes of a standard: its own, then one per line style, pen, text style and kind, each
/// saying which standard it is of.
pub fn nodes(standard: &Standard) -> Vec<Value> {
    fn setting<T: Serialize>(standard: &Standard, attribute: &str, what: &str, key: &str, name: &str, item: &T) -> Value {
        let mut data = serde_json::to_value(item).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut data {
            map.insert("standard".into(), json!(standard.key));
        }
        json!({"path": path(&standard.key, what, key), "attributes": {"bsi::ifc::prop::Name": name, attribute: data}})
    }
    let mut nodes = vec![json!({"path": path(&standard.key, "standard", ""), "attributes": {"bsi::ifc::prop::Name": standard.name(Lang::En), STANDARD: standard}})];
    nodes.extend(standard.line_styles.iter().map(|s| setting(standard, LINE_STYLE, "linestyle", &s.key, s.names.get(Lang::En), s)));
    nodes.extend(standard.pens.iter().map(|p| setting(standard, PEN, "pen", &p.key, p.names.get(Lang::En), p)));
    nodes.extend(standard.text_styles.iter().map(|t| setting(standard, TEXT_STYLE, "textstyle", &t.key, t.names.get(Lang::En), t)));
    nodes.extend(standard.kinds.iter().map(|k| setting(standard, KIND, "kind", &k.key, k.names.get(Lang::En), k)));
    nodes
}

/// A file of drawing standards alone, as [`crate::ifcx::file`] is of a library.
pub fn file(standards: &[Standard], id: &str, author: &str, timestamp: &str) -> Value {
    json!({
        "header": {"id": id, "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": author, "timestamp": timestamp},
        "imports": [],
        "schemas": {},
        "data": standards.iter().flat_map(nodes).collect::<Vec<_>>(),
    })
}

/// The standards in an IFCX file, in the order of their own nodes, each with the settings that
/// name it. The attributes of nodes with the same path are composed, a later one winning, as
/// IFCX layers them; a setting of a standard the file does not hold is left out. Refused when a
/// standard does not hold together ([`Standard::check`]).
pub fn read(file: &Value) -> Result<Vec<Standard>, LibraryError> {
    let data = file.get("data").and_then(Value::as_array).ok_or_else(|| LibraryError::Unreadable("no data".into()))?;
    // compose the nodes by path, keeping the order of first appearance
    let mut order: Vec<&str> = Vec::new();
    let mut composed: std::collections::HashMap<&str, Map<String, Value>> = std::collections::HashMap::new();
    for node in data {
        let (Some(path), Some(attributes)) = (node.get("path").and_then(Value::as_str), node.get("attributes").and_then(Value::as_object)) else { continue };
        let entry = composed.entry(path).or_insert_with(|| {
            order.push(path);
            Map::new()
        });
        for (key, value) in attributes {
            entry.insert(key.clone(), value.clone());
        }
    }
    fn of<T: for<'a> Deserialize<'a>>(attributes: &Map<String, Value>, attribute: &str) -> Result<Option<(String, T)>, LibraryError> {
        let Some(value) = attributes.get(attribute) else { return Ok(None) };
        let standard = value.get("standard").and_then(Value::as_str).unwrap_or_default().to_string();
        serde_json::from_value(value.clone()).map(|item| Some((standard, item))).map_err(|e| LibraryError::Unreadable(format!("{attribute}: {e}")))
    }
    let mut standards: Vec<Standard> = Vec::new();
    for path in &order {
        if let Some(value) = composed[path].get(STANDARD) {
            standards.push(serde_json::from_value(value.clone()).map_err(|e| LibraryError::Unreadable(format!("{STANDARD}: {e}")))?);
        }
    }
    fn find<'a>(standards: &'a mut [Standard], key: &str) -> Option<&'a mut Standard> {
        standards.iter_mut().find(|s| s.key == key)
    }
    for path in &order {
        let attributes = &composed[path];
        if let Some((key, item)) = of::<LineStyle>(attributes, LINE_STYLE)? {
            if let Some(standard) = find(&mut standards, &key) {
                standard.line_styles.push(item);
            }
        }
        if let Some((key, item)) = of::<Pen>(attributes, PEN)? {
            if let Some(standard) = find(&mut standards, &key) {
                standard.pens.push(item);
            }
        }
        if let Some((key, item)) = of::<TextStyle>(attributes, TEXT_STYLE)? {
            if let Some(standard) = find(&mut standards, &key) {
                standard.text_styles.push(item);
            }
        }
        if let Some((key, item)) = of::<DrawingKind>(attributes, KIND)? {
            if let Some(standard) = find(&mut standards, &key) {
                standard.kinds.push(item);
            }
        }
    }
    for standard in &standards {
        standard.check()?;
    }
    Ok(standards)
}

/// The nodes of drawing standards among `nodes`, when they read as standards ([`read`]); none
/// when there are none or they do not hold together.
pub fn held(nodes: &[Value]) -> Option<Vec<Value>> {
    let own = [STANDARD, KIND, LINE_STYLE, PEN, TEXT_STYLE];
    let ours: Vec<Value> = nodes.iter().filter(|node| node.get("attributes").and_then(Value::as_object).is_some_and(|a| own.iter().any(|key| a.contains_key(*key)))).cloned().collect();
    let standards = read(&json!({ "data": ours })).ok()?;
    (!standards.is_empty()).then_some(ours)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dutch_standard_holds_together() {
        let nl = Standard::netherlands();
        assert_eq!(nl.check(), Ok(()));
        // the kinds of the template's drawings, with their views, scales and numbers
        let plan = nl.kind("floor-plan").expect("a floor plan");
        assert_eq!((plan.code.as_str(), plan.discipline.code(), plan.view, plan.scale_text().as_str(), plan.number.as_str()), ("PLG", "BWK", View::Plan, "1:50", "01"));
        assert_eq!(nl.kind("pile-plan").map(|k| (k.discipline, k.scales.clone())), Some((Discipline::Structure, vec![100])));
        assert_eq!(nl.kind("elevation").map(|k| (k.view, k.hatched)), Some((View::Elevation, false)));
        assert_eq!(nl.first_sheet, 300);
        // what is cut is twice as wide as what is seen beyond
        for kind in &nl.kinds {
            let (cut, beyond) = (nl.pen(&kind.cut).unwrap().width, nl.pen(&kind.beyond).unwrap().width);
            assert!((cut / beyond - 2.0).abs() < 0.06, "{}: {cut} and {beyond}", kind.key);
        }
        // a kind the template has no drawing of says so
        assert_eq!(nl.kind("site-plan").map(|k| k.origin), Some(Origin::Practice));
        assert!(nl.kinds.iter().filter(|k| k.origin == Origin::Template).count() >= 12);
        // a continuous line has no dashes; the others pair each dash with a gap
        assert!(nl.line_style("continuous").unwrap().dashes.is_empty());
        assert_eq!(nl.line_style("chain").unwrap().dashes, [24.0, 3.0, 0.5, 3.0]);
    }

    #[test]
    fn a_standard_is_checked() {
        let nl = Standard::netherlands();
        let with = |change: &dyn Fn(&mut Standard)| {
            let mut s = nl.clone();
            change(&mut s);
            s.check()
        };
        assert_eq!(with(&|s| s.kinds[1].cut = "2.0".into()), Err(LibraryError::UnknownSetting("2.0".into())));
        assert_eq!(with(&|s| s.kinds[1].scales = vec![75]), Err(LibraryError::UnknownSetting("1:75".into())));
        assert_eq!(with(&|s| s.kinds[1].hidden = "wavy".into()), Err(LibraryError::UnknownSetting("wavy".into())));
        assert_eq!(with(&|s| s.pens[0].width = 0.0), Err(LibraryError::NotPositive));
        assert_eq!(with(&|s| s.pens[1].key = s.pens[0].key.clone()), Err(LibraryError::Duplicate("0.13".into())));
        assert_eq!(with(&|s| s.text_styles[0].key.clear()), Err(LibraryError::NoKey));
        assert_eq!(with(&|s| s.line_styles[1].dashes = vec![12.0]), Err(LibraryError::UnknownSetting("dashed".into())));
    }

    #[test]
    fn a_standard_reads_back_from_ifcx_setting_by_setting() {
        let nl = Standard::netherlands();
        let file = file(&[nl.clone()], "standards.ifcx", "test", "2026-11-18T00:00:00Z");
        assert_eq!(file["data"].as_array().map(Vec::len), Some(1 + nl.line_styles.len() + nl.pens.len() + nl.text_styles.len() + nl.kinds.len()));
        assert_eq!(read(&file), Ok(vec![nl.clone()]));
        // written twice, a node has the same path
        assert_eq!(nodes(&nl)[3]["path"], nodes(&nl)[3]["path"]);
        // a later layer changes one setting: the floor plan drawn at 1:100
        let mut layered = file.clone();
        let mut plan = nl.kind("floor-plan").unwrap().clone();
        plan.scales = vec![100];
        let mut change = serde_json::to_value(&plan).unwrap();
        change["standard"] = json!("nl");
        layered["data"].as_array_mut().unwrap().push(json!({"path": path("nl", "kind", "floor-plan"), "attributes": {KIND: change}}));
        let read_back = read(&layered).unwrap();
        assert_eq!(read_back[0].kind("floor-plan").unwrap().scales, [100]);
        assert_eq!(read_back[0].kinds.len(), nl.kinds.len());
        // a setting that does not fit its standard refuses the file
        plan.cut = "9".into();
        let mut bad = serde_json::to_value(&plan).unwrap();
        bad["standard"] = json!("nl");
        layered["data"].as_array_mut().unwrap().push(json!({"path": path("nl", "kind", "floor-plan"), "attributes": {KIND: bad}}));
        assert_eq!(read(&layered), Err(LibraryError::UnknownSetting("9".into())));
        // a file without data is no file of standards
        assert!(matches!(read(&json!({})), Err(LibraryError::Unreadable(_))));
    }
}
