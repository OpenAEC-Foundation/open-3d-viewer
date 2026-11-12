//! The properties of what a Revit file draws in a legend or drafting view, per element, for the
//! right pane ([`crate::app::drawn_props`]): what it is, its type and family, its line style
//! (name, line weight, colour, line pattern), a filled region's patterns and colours, a text's
//! font, and the parameter values the file stores on it and on its type.
//!
//! Read through the generic object view, named schema fields only:
//! - a detail line (`CurveElem`) names its line style in `m_categoryId`: a `GStyleElem`, whose
//!   `GStyle` holds the pen (`m_penNumber`), colour (`m_color`, blue-green-red) and line pattern
//!   (`m_linePatternId`, a `LinePatternElem`), and whose `m_categoryId` is the subcategory that
//!   names it (a `CategoryElem`, `m_pCategory.m_name`); without one it has the style of its
//!   category;
//! - a pen is a width per view scale in the project's `PenWidthTableElem` (`m_modelPenInfo` for
//!   what is drawn of the model and of details, `m_draftPenInfo` for annotations);
//! - a filled region's type (`m_attrId`, a `FilledRegionAttributes`) names its foreground and
//!   background patterns (`FillPatternElem`), the background colour, and through its
//!   `LineAndTextAttr` the category whose style gives the foreground colour and the pen;
//! - a text's type (`m_attrId`) and a dimension's (`m_styleSymbolId`) name a font (`FontElem`:
//!   name, size on paper, colour) in their `LineAndTextAttr`;
//! - a tag and a detail component name a type (`FamilySymbol`) of a `Family`;
//! - stored values are in `m_pParamValueSet*`, by parameter: a `ParameterElement` of the file
//!   (its caption and spec) or a built-in one (named here, see [`BUILT_IN`]).
//!
//! Lengths in the file are feet; sizes on paper are shown in millimetres.

use crate::app::drawn_props::{Drawn, Group, Word};
use crate::model::Draft;
use rvt_model::{FieldValue, NestedObject, ObjectView};
use std::collections::HashMap;
use std::sync::Arc;

const MM: f64 = 304.8;
/// The line pattern of a solid line.
const SOLID: i64 = -3000010;
const LINES: i64 = -2000051;
const SKETCH: i64 = -2000045;
const DETAIL_ITEMS: i64 = -2002000;

/// The properties of every element the drawings draw, by its element.
pub(super) fn drawn(doc: &rvt_read::Document, drafts: &[Draft]) -> Drawn {
    let mut reader = Reader { doc, names: HashMap::new(), styles: HashMap::new(), types: HashMap::new(), defaults: None, pens: None, defs: HashMap::new() };
    let mut out = Drawn::new();
    for draft in drafts {
        let elements = draft.lines.iter().map(|l| l.element).chain(draft.texts.iter().map(|t| t.element)).chain(draft.dimensions.iter().map(|d| d.element));
        for element in elements {
            if !out.contains_key(&element) {
                let groups = reader.element(element, draft.scale as f64);
                out.insert(element, groups);
            }
        }
    }
    out
}

struct Reader<'a> {
    doc: &'a rvt_read::Document,
    /// The name of an element: of a type, family, category, pattern or view.
    names: HashMap<i64, Option<String>>,
    /// The group of a line style at a view scale, drawn of the model or as an annotation.
    styles: HashMap<(i64, u64, bool), Option<Arc<Group>>>,
    /// The groups of a type at a view scale.
    types: HashMap<(u64, u64), Vec<Arc<Group>>>,
    /// The line style of a category without a style of its own: its projection style.
    defaults: Option<HashMap<i64, i64>>,
    /// The widths of the pens (feet on paper): per view scale for the model, and for annotations.
    pens: Option<(Vec<(f64, Vec<f64>)>, Vec<f64>)>,
    /// The caption and spec of a parameter of the file.
    defs: HashMap<i64, Option<(String, String)>>,
}

fn word(dutch: &'static str, english: &'static str) -> Word {
    Word::Both(dutch, english)
}

fn yes_no(yes: bool) -> Word {
    if yes {
        word("Ja", "Yes")
    } else {
        word("Nee", "No")
    }
}

/// A size on paper (feet) in millimetres.
fn paper(feet: f64) -> String {
    let number = format!("{:.2}", feet * MM);
    format!("{} mm", number.trim_end_matches('0').trim_end_matches('.'))
}

/// A colour as the file stores it (blue, green, red from the high byte down).
fn colour(stored: i64) -> Word {
    let (r, g, b) = (stored & 255, (stored >> 8) & 255, (stored >> 16) & 255);
    match (r, g, b) {
        (0, 0, 0) => word("Zwart (RGB 0, 0, 0)", "Black (RGB 0, 0, 0)"),
        (255, 255, 255) => word("Wit (RGB 255, 255, 255)", "White (RGB 255, 255, 255)"),
        _ => format!("RGB {r}, {g}, {b}").into(),
    }
}

fn element_of(v: Option<&FieldValue>) -> Option<i64> {
    v.and_then(FieldValue::element).filter(|e| *e != -1)
}

fn flag(v: Option<&FieldValue>) -> Option<bool> {
    match v {
        Some(FieldValue::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// The stored values of an element: its four value sets, by parameter.
fn stored(o: &ObjectView) -> Vec<(i64, Stored)> {
    let mut out = Vec::new();
    for (field, kind) in [("m_pParamValueSetAString", 2), ("m_pParamValueSetDouble", 0), ("m_pParamValueSetInt", 1), ("m_pParamValueSetElementId", 3)] {
        let Some(list) = o.get(field).and_then(|v| o.deref(v)).and_then(|set| set.get("m_paramSet")).and_then(FieldValue::as_list) else { continue };
        for entry in list.iter().filter_map(FieldValue::as_object) {
            let (Some(id), Some(v)) = (entry.get("m_paramId").and_then(FieldValue::element), entry.get("m_value")) else { continue };
            let value = match kind {
                0 => v.as_f64().map(Stored::Number),
                1 => v.as_i64().map(Stored::Integer),
                2 => v.as_str().map(|s| Stored::Text(s.to_string())),
                _ => v.element().map(Stored::Element),
            };
            out.extend(value.map(|value| (id, value)));
        }
    }
    out
}

enum Stored {
    Number(f64),
    Integer(i64),
    Text(String),
    Element(i64),
}

/// How a built-in parameter's number reads.
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    /// A size on paper (feet).
    Paper,
    /// A length in the drawing (feet).
    Length,
    Angle,
    Number,
    /// A text: a number stored for it says nothing.
    Text,
    YesNo,
}

/// The built-in parameters met on what a drawing draws and on its types: number, Dutch and
/// English name, and how its value reads. A built-in parameter not listed here is left out: the
/// file stores its number only.
const BUILT_IN: &[(i64, &str, &str, Unit)] = &[
    (-1010105, "Typeopmerking", "Type Comments", Unit::Text),
    (-1010106, "Opmerkingen", "Comments", Unit::Text),
    (-1010103, "Merk", "Mark", Unit::Text),
    (-1010104, "Typemerk", "Type Mark", Unit::Text),
    (-1010108, "Omschrijving", "Description", Unit::Text),
    (-1010109, "Fabrikant", "Manufacturer", Unit::Text),
    (-1010107, "URL", "URL", Unit::Text),
    (-1010102, "Model", "Model", Unit::Text),
    (-1010101, "Kosten", "Cost", Unit::Number),
    (-1002001, "Typenaam", "Type Name", Unit::Text),
    (-1002002, "Familienaam", "Family Name", Unit::Text),
    (-1002500, "Samenstellingscode", "Assembly Code", Unit::Text),
    (-1002501, "Keynote", "Keynote", Unit::Text),
    (-1006327, "Breedtefactor", "Width Factor", Unit::Number),
    (-1006326, "Tabgrootte", "Tab Size", Unit::Paper),
    (-1006315, "Pijlpunt", "Leader Arrowhead", Unit::Number),
    (-1006316, "Projectnummer", "Project Number", Unit::Number),
    (-1006414, "Maat pijlpunt", "Tick Size", Unit::Paper),
    (-1006426, "Hoek pijlpunt", "Arrow Width Angle", Unit::Angle),
    (-1006401, "Afstand tekst tot maatlijn", "Text Offset", Unit::Paper),
    (-1006404, "Verlenging maatlijn", "Dimension Line Extension", Unit::Paper),
    (-1006405, "Afstand hulplijn tot element", "Witness Line Gap to Element", Unit::Paper),
    (-1006407, "Lengte hulplijn", "Witness Line Length", Unit::Paper),
    (-1006431, "Verlenging hulplijn", "Witness Line Extension", Unit::Paper),
    (-1006433, "Verlenging omgekeerde maatlijn", "Flipped Dimension Line Extension", Unit::Paper),
    (-1006465, "Afstand maatlijnen", "Dimension Line Snap Distance", Unit::Paper),
    (-1006430, "Maatstreepje hartlijn", "Centerline Tick Mark", Unit::Number),
    (-1005501, "Volgorde", "Draw Order", Unit::Number),
    (-1004005, "Lengte", "Length", Unit::Length),
    (-1005003, "Hoek", "Angle", Unit::Angle),
    (-1005120, "Zichtbaar", "Visible", Unit::YesNo),
];

/// The built-in categories of what a drawing draws: number, Dutch and English name.
const CATEGORIES: &[(i64, &str, &str)] = &[
    (LINES, "Lijnen", "Lines"),
    (SKETCH, "Schetslijnen", "Sketch Lines"),
    (DETAIL_ITEMS, "Detailonderdelen", "Detail Items"),
    (-2000300, "Tekstnotities", "Text Notes"),
    (-2000260, "Maatvoering", "Dimensions"),
    (-2005028, "Tags van detailonderdelen", "Detail Item Tags"),
    (-2000150, "Generieke annotaties", "Generic Annotations"),
    (-2005020, "Generieke-modeltags", "Generic Model Tags"),
    (-2005030, "Materiaaltags", "Material Tags"),
    (-2005031, "Keynote-tags", "Keynote Tags"),
    (-2005014, "Multicategorie-tags", "Multi-Category Tags"),
    (-2000263, "Peilmaten", "Spot Elevations"),
    (-2000264, "Coördinaatmaten", "Spot Coordinates"),
    (-2000265, "Hellingmaten", "Spot Slopes"),
    (-2000064, "Lijnen in zicht", "Lines"),
    (-2000059, "Annotatie", "Annotation"),
];

impl Reader<'_> {
    fn object(&self, id: i64) -> Option<ObjectView> {
        self.doc.object(u64::try_from(id).ok()?)
    }

    /// The name of an element, wherever its class keeps it.
    fn name(&mut self, id: i64) -> Option<String> {
        if let Some(known) = self.names.get(&id) {
            return known.clone();
        }
        let name = self.object(id).and_then(|o| {
            let paths: [&[&str]; 7] = [&["m_symbolInfo", "m_name"], &["m_pCategory", "m_name"], &["m_pFillPattern", "m_name"], &["m_pLinePattern", "m_name"], &["m_pFont", "m_name"], &["m_name"], &["m_text"]];
            paths.iter().find_map(|p| o.path(p)?.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty())
        });
        self.names.insert(id, name.clone());
        name
    }

    /// The name of a category: a built-in one, or a subcategory of the file.
    fn category(&mut self, id: i64) -> Option<Word> {
        if id < 0 {
            return CATEGORIES.iter().find(|c| c.0 == id).map(|c| word(c.1, c.2));
        }
        self.name(id).map(Word::Own)
    }

    /// The width of a pen at a view scale (feet on paper).
    fn pen(&mut self, pen: i64, scale: f64, model: bool) -> Option<f64> {
        if self.pens.is_none() {
            // the project's table is the first one; a family brings its own
            let table = self.doc.object_ids_of_class("PenWidthTableElem").into_iter().min().and_then(|id| self.doc.object(id));
            let widths = |o: &NestedObject| -> Vec<f64> { o.get("m_pens").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_f64).collect() };
            let read = table.as_ref().and_then(|o| {
                let t = o.get("m_pPenWidthTable").and_then(|v| o.deref(v))?;
                let per_scale = t.get("m_modelPenInfo").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
                let mut model: Vec<(f64, Vec<f64>)> = per_scale.filter_map(|i| Some((i.get("m_invertedScale")?.as_f64()?, widths(i)))).collect();
                model.sort_by(|a, b| a.0.total_cmp(&b.0));
                let draft = t.get("m_draftPenInfo").and_then(FieldValue::as_object).map(widths).unwrap_or_default();
                Some((model, draft))
            });
            self.pens = Some(read.unwrap_or_default());
        }
        let (model_pens, draft) = self.pens.as_ref()?;
        let index = usize::try_from(pen - 1).ok()?;
        let widths = if model {
            // the column of the view's scale, else of the next coarser one
            &model_pens.iter().find(|(s, _)| *s >= scale - 1e-6).or(model_pens.last())?.1
        } else {
            draft
        };
        widths.get(index).copied().filter(|w| w.is_finite() && *w > 0.0)
    }

    /// The rows of a graphics style (a `GStyleElem`): line weight, colour and line pattern.
    fn style_rows(&mut self, style: i64, scale: f64, model: bool, colour_label: Word) -> Vec<(Word, Word)> {
        let mut rows = Vec::new();
        let Some(o) = self.object(style) else { return rows };
        let Some(g) = o.get("m_pGStyle").and_then(|v| o.deref(v)) else { return rows };
        if let Some(pen) = g.get("m_penNumber").and_then(FieldValue::as_i64).filter(|p| *p > 0) {
            let value = match self.pen(pen, scale, model) {
                Some(width) => format!("{pen} ({})", paper(width)),
                None => pen.to_string(),
            };
            rows.push((word("Lijndikte", "Line weight"), value.into()));
        }
        if let Some(c) = g.get("m_color").and_then(FieldValue::as_i64) {
            rows.push((colour_label, colour(c)));
        }
        match g.get("m_linePatternId").and_then(FieldValue::element) {
            Some(SOLID) => rows.push((word("Lijnpatroon", "Line pattern"), word("Doorgetrokken", "Solid"))),
            Some(pattern) if pattern > 0 => rows.extend(self.name(pattern).map(|n| (word("Lijnpatroon", "Line pattern"), n.into()))),
            _ => {}
        }
        rows
    }

    /// The line style group of a `GStyleElem`: its name and what it draws with.
    fn line_style(&mut self, style: i64, scale: f64, model: bool) -> Option<Arc<Group>> {
        let key = (style, scale.to_bits(), model);
        if let Some(known) = self.styles.get(&key) {
            return known.clone();
        }
        let category = self.object(style).and_then(|o| o.get("m_categoryId")?.element());
        let mut rows = Vec::new();
        if let Some(name) = category.and_then(|c| self.category(c)) {
            rows.push((word("Naam", "Name"), name));
        }
        rows.extend(self.style_rows(style, scale, model, word("Kleur", "Colour")));
        let group = (!rows.is_empty()).then(|| Arc::new(Group { title: word("Lijnstijl", "Line style"), rows }));
        self.styles.insert(key, group.clone());
        group
    }

    /// The projection style of a category that has no style of its own.
    fn default_style(&mut self, category: i64) -> Option<i64> {
        if self.defaults.is_none() {
            let mut defaults = HashMap::new();
            for id in self.doc.object_ids_of_class("GStyleElem") {
                let Some(o) = self.doc.object_root(id) else { continue };
                if o.get("m_gstyleType").and_then(FieldValue::as_i64) == Some(1) {
                    if let Some(c) = o.get("m_categoryId").and_then(FieldValue::element).filter(|c| *c < 0) {
                        defaults.entry(c).or_insert(id as i64);
                    }
                }
            }
            self.defaults = Some(defaults);
        }
        self.defaults.as_ref()?.get(&category).copied()
    }

    /// The caption and spec of a parameter of the file.
    fn definition(&mut self, id: i64) -> Option<(String, String)> {
        if let Some(known) = self.defs.get(&id) {
            return known.clone();
        }
        let def = self.object(id).and_then(|o| {
            let caption = o.path(&["m_pParamDef", "m_caption"])?.as_str()?.trim().to_string();
            let spec = o.path(&["m_pParamDef", "m_specTypeId", "m_typeId"]).and_then(FieldValue::as_str).unwrap_or_default().to_string();
            (!caption.is_empty()).then_some((caption, spec))
        });
        self.defs.insert(id, def.clone());
        def
    }

    /// A stored value as a row: its parameter's name and the value in the unit of its spec.
    /// `None` for a parameter without a name here, and for an empty value.
    fn parameter(&mut self, id: i64, value: &Stored) -> Option<(Word, Word)> {
        let (label, unit, spec) = if id > 0 {
            let (caption, spec) = self.definition(id)?;
            let kind = spec.split(':').nth(1).and_then(|k| k.split('-').next()).unwrap_or_default();
            let unit = match kind {
                "length" | "distance" => Unit::Length,
                "angle" | "rotationAngle" => Unit::Angle,
                _ if spec.contains("spec.bool") || spec.contains("spec:bool") => Unit::YesNo,
                _ => Unit::Number,
            };
            (Word::Own(caption), unit, kind.to_string())
        } else {
            let known = BUILT_IN.iter().find(|b| b.0 == id)?;
            (word(known.1, known.2), known.3, String::new())
        };
        let shown: Word = match value {
            Stored::Text(t) if t.trim().is_empty() => return None,
            Stored::Text(t) => t.trim().into(),
            _ if unit == Unit::Text => return None,
            Stored::Number(v) => match (unit, spec.as_str()) {
                (Unit::Paper, _) => paper(*v).into(),
                (Unit::Length, _) => format!("{:.0} mm", v * MM).into(),
                (Unit::Angle, _) => format!("{:.2}°", v.to_degrees()).into(),
                (_, "area") => format!("{:.2} m²", v * 0.092_903_04).into(),
                (_, "volume") => format!("{:.3} m³", v * 0.028_316_846_592).into(),
                _ => format!("{}", (v * 1e6).round() / 1e6).into(),
            },
            Stored::Integer(v) if unit == Unit::YesNo => yes_no(*v != 0),
            Stored::Integer(v) => v.to_string().into(),
            Stored::Element(e) if *e <= 0 => return None,
            Stored::Element(e) => self.name(*e).unwrap_or_else(|| format!("#{e}")).into(),
        };
        Some((label, shown))
    }

    /// The stored values of an element as a group, by name.
    fn parameters(&mut self, o: &ObjectView, title: Word) -> Option<Arc<Group>> {
        let mut rows: Vec<(Word, Word)> = stored(o).iter().filter_map(|(id, v)| self.parameter(*id, v)).collect();
        // the values a family's instance or type holds for its own parameters
        for field in ["m_pInstParams", "m_pParams"] {
            let Some(list) = o.get(field).and_then(|v| o.deref(v)).and_then(|p| p.get("m_params")).and_then(FieldValue::as_list) else { continue };
            for p in list.iter().filter_map(FieldValue::as_object) {
                let Some(id) = element_of(p.get("m_paramId")) else { continue };
                let text = p.get("m_str").and_then(FieldValue::as_str).filter(|s| !s.trim().is_empty());
                let value = match (text, element_of(p.get("m_elemId")), p.get("m_value").and_then(FieldValue::as_f64)) {
                    (Some(t), _, _) => Stored::Text(t.to_string()),
                    (None, Some(e), _) => Stored::Element(e),
                    // a label of a family holds no number of its own
                    (None, None, Some(v)) if v != 0.0 => Stored::Number(v),
                    _ => continue,
                };
                rows.extend(self.parameter(id, &value));
            }
        }
        rows.dedup();
        (!rows.is_empty()).then(|| Arc::new(Group { title, rows }))
    }

    /// The rows of a type's letters and lines (its `LineAndTextAttr`): font, size, colour,
    /// bold, italic, underlined, and the line weight of its category.
    fn letters(&mut self, attr: &NestedObject, scale: f64) -> Vec<(Word, Word)> {
        let mut rows = Vec::new();
        if let Some(o) = element_of(attr.get("m_fontId")).and_then(|f| self.object(f)) {
            let font = o.get("m_pFont").and_then(|v| o.deref(v));
            if let Some(name) = font.and_then(|f| f.get("m_name")?.as_str()).filter(|n| !n.is_empty()) {
                rows.push((word("Lettertype", "Font"), name.into()));
            }
            if let Some(size) = font.and_then(|f| f.get("m_size")?.as_f64()).filter(|s| s.is_finite() && *s > 0.0) {
                rows.push((word("Tekstgrootte", "Text size"), paper(size).into()));
            }
            if let Some(c) = font.and_then(|f| f.get("m_color")?.as_i64()) {
                rows.push((word("Kleur", "Colour"), colour(c & 0xff_ffff)));
            }
        }
        for (field, dutch, english) in [("m_bBold", "Vet", "Bold"), ("m_bItalic", "Cursief", "Italic"), ("m_bUnderline", "Onderstreept", "Underline")] {
            rows.extend(flag(attr.get(field)).map(|on| (word(dutch, english), yes_no(on))));
        }
        match attr.get("m_background").and_then(FieldValue::as_i64) {
            Some(0) => rows.push((word("Achtergrond", "Background"), word("Dekkend", "Opaque"))),
            Some(1) => rows.push((word("Achtergrond", "Background"), word("Transparant", "Transparent"))),
            _ => {}
        }
        if let Some(style) = element_of(attr.get("m_categoryId")).and_then(|c| self.own_style(c)) {
            let lines = self.style_rows(style, scale, false, word("Lijnkleur", "Line colour"));
            rows.extend(lines.into_iter().filter(|(label, _)| *label == word("Lijndikte", "Line weight")));
        }
        rows
    }

    /// The style of a category of the file (a `CategoryElem`): its first one, for projection.
    fn own_style(&self, category: i64) -> Option<i64> {
        let o = self.object(category)?;
        element_of(o.get("m_gstyleIds")?.as_list()?.first())
    }

    /// The groups of a type: what it is called, how it draws, and its stored values.
    fn kind(&mut self, id: u64, scale: f64) -> Vec<Arc<Group>> {
        let key = (id, scale.to_bits());
        if let Some(known) = self.types.get(&key) {
            return known.clone();
        }
        let mut groups = Vec::new();
        if let Some(o) = self.doc.object(id) {
            match o.class.as_str() {
                "FilledRegionAttributes" => {
                    let mut rows = Vec::new();
                    let attr = o.get("m_lineAndTextAttr").and_then(FieldValue::as_object);
                    let style = attr.and_then(|a| element_of(a.get("m_categoryId"))).and_then(|c| self.own_style(c));
                    let pattern = |r: &mut Self, field: &str| element_of(o.get(field)).and_then(|p| r.name(p)).map_or(word("Geen", "None"), Word::Own);
                    rows.push((word("Patroon voor", "Foreground pattern"), pattern(self, "m_fillPatternElemId")));
                    let lines = style.map(|s| self.style_rows(s, scale, true, word("Kleur voor", "Foreground colour"))).unwrap_or_default();
                    rows.extend(lines.iter().filter(|(label, _)| *label == word("Kleur voor", "Foreground colour")).cloned());
                    let background = element_of(o.get("m_backgroundPatternElemId")).is_some();
                    rows.push((word("Patroon achter", "Background pattern"), pattern(self, "m_backgroundPatternElemId")));
                    if background {
                        rows.extend(o.get("m_backgroundPatternColor").and_then(FieldValue::as_i64).map(|c| (word("Kleur achter", "Background colour"), colour(c))));
                    }
                    match o.get("m_background").and_then(FieldValue::as_i64) {
                        Some(0) => rows.push((word("Achtergrond", "Background"), word("Dekkend", "Opaque"))),
                        Some(1) => rows.push((word("Achtergrond", "Background"), word("Transparant", "Transparent"))),
                        _ => {}
                    }
                    rows.extend(lines.into_iter().filter(|(label, _)| *label == word("Lijndikte", "Line weight")));
                    groups.push(Arc::new(Group { title: word("Arcering", "Filled region"), rows }));
                }
                "TextNoteAttributes" => {
                    let rows = o.get("m_lineAndTextAttr").and_then(FieldValue::as_object).map(|a| self.letters(a, scale)).unwrap_or_default();
                    if !rows.is_empty() {
                        groups.push(Arc::new(Group { title: word("Teksttype", "Text type"), rows }));
                    }
                }
                "DimensionStyle" => {
                    let mut rows = o.get("m_pLineAndTextAttr").and_then(|v| o.deref(v)).map(|a| self.letters(a, scale)).unwrap_or_default();
                    rows.retain(|(label, _)| *label != word("Achtergrond", "Background"));
                    if let Some(mark) = element_of(o.get("m_arrowHeadStyleId")).and_then(|m| self.name(m)) {
                        rows.push((word("Maatstreepje", "Tick mark"), mark.into()));
                    }
                    if !rows.is_empty() {
                        groups.push(Arc::new(Group { title: word("Maatstijl", "Dimension style"), rows }));
                    }
                }
                _ => {}
            }
            groups.extend(self.parameters(&o, word("Typeparameters", "Type parameters")));
        }
        self.types.insert(key, groups.clone());
        groups
    }

    /// The groups of an element drawn in a view of `scale`.
    fn element(&mut self, id: u64, scale: f64) -> Vec<Arc<Group>> {
        let Some(o) = self.doc.object(id) else { return Vec::new() };
        let category = o.category.filter(|c| *c != -1);
        let sort = match o.class.as_str() {
            "CurveElem" if category == Some(SKETCH) => word("Schetslijn", "Sketch line"),
            "CurveElem" => word("Detaillijn", "Detail line"),
            "FilledRegion" if flag(o.get("m_bMasking")) == Some(true) => word("Maskering", "Masking region"),
            "FilledRegion" => word("Arcering", "Filled region"),
            "FamilyInstance" if category == Some(DETAIL_ITEMS) => word("Detailcomponent", "Detail component"),
            "FamilyInstance" => word("Component", "Component"),
            "TextNote" => word("Tekst", "Text"),
            "IndependentTag" => word("Tag", "Tag"),
            _ if o.is_a("Dimension") => word("Maat", "Dimension"),
            class => class.into(),
        };
        let mut rows = vec![(word("Soort", "Kind"), sort)];
        if let Some(name) = category.and_then(|c| self.category(c)) {
            rows.push((word("Categorie", "Category"), name));
        }

        // its type, and the family of that
        let named = |v: Option<&FieldValue>| element_of(v).and_then(|k| u64::try_from(k).ok()).filter(|k| *k != id);
        let kind = ["m_masterSymbolId", "m_symbolId", "m_attrId", "m_styleSymbolId"].into_iter().find_map(|f| named(o.get(f))).or_else(|| named(o.path(&["m_pInstanceInfo", "m_symbolId"])));
        if let Some(kind) = kind {
            let of_type = self.doc.object(kind);
            let family = match of_type.as_ref().map(|t| t.class.as_str()) {
                Some("TextNoteAttributes") => Some(word("Tekst", "Text")),
                Some("FilledRegionAttributes") => Some(word("Arcering", "Filled region")),
                Some("DimensionStyle") => Some(word("Lineaire maatstijl", "Linear dimension style")),
                _ => of_type.as_ref().and_then(|t| element_of(t.get("m_familyId"))).and_then(|f| self.name(f)).map(Word::Own),
            };
            rows.extend(family.map(|f| (word("Familie", "Family"), f)));
            rows.extend(self.name(kind as i64).map(|n| (word("Type", "Type"), n.into())));
        }

        // what its class tells of it
        match o.class.as_str() {
            "CurveElem" => {
                let curve = o.get("m_pCurveDriver").and_then(|v| o.deref(v)).and_then(|d| d.get("m_pCrv")).and_then(|v| o.deref(v));
                if let Some(curve) = curve {
                    let ends: Vec<f64> = curve.get("m_endParams").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_f64).collect();
                    let run = ends.get(1).zip(ends.first()).map(|(to, from)| (to - from).abs()).filter(|r| r.is_finite() && *r < 1e6);
                    let radius = curve.get("m_radius").and_then(FieldValue::as_f64).filter(|r| r.is_finite() && *r > 0.0);
                    let shape = match curve.class.as_str() {
                        "GLine" => word("Recht", "Straight"),
                        "GArc" if run.is_some_and(|r| r < 1e-9) => word("Cirkel", "Circle"),
                        "GArc" => word("Boog", "Arc"),
                        "GEllipse" => word("Ellips", "Ellipse"),
                        "GNurbSpline" | "GHermiteSpline" => word("Spline", "Spline"),
                        other => other.trim_start_matches('G').into(),
                    };
                    rows.push((word("Vorm", "Shape"), shape));
                    match (curve.class.as_str(), run, radius) {
                        ("GLine", Some(run), _) => rows.push((word("Lengte", "Length"), format!("{:.1} mm", run * MM).into())),
                        ("GArc", Some(run), Some(r)) => {
                            let sweep = if run < 1e-9 { std::f64::consts::TAU } else { run };
                            rows.push((word("Straal", "Radius"), format!("{:.1} mm", r * MM).into()));
                            rows.push((word("Lengte", "Length"), format!("{:.1} mm", r * sweep * MM).into()));
                        }
                        _ => {}
                    }
                }
            }
            "TextNote" => {
                let width = o.get("m_lineWidth").and_then(FieldValue::as_f64).filter(|w| w.is_finite() && *w > 0.0);
                rows.extend(width.map(|w| (word("Breedte", "Width"), paper(w).into())));
                let leaders = o.get("m_leaders").and_then(FieldValue::as_list).map_or(0, <[FieldValue]>::len);
                rows.push((word("Aanhaallijnen", "Leaders"), leaders.to_string().into()));
            }
            "IndependentTag" => {
                // what it tags: the host of its first reference
                let host = o.objects.iter().filter(|n| n.class == "TagReferenceWithLeader").find_map(|n| {
                    let key = n.get("m_referenceIdKey").and_then(FieldValue::as_object)?;
                    element_of(key.get("m_hostOrLinkElemId").and_then(FieldValue::as_object)?.get("m_linkInstOrHostId"))
                });
                if let Some(host) = host {
                    let of = self.object(host).map(|h| h.class).unwrap_or_default();
                    let what = match of.as_str() {
                        "FilledRegion" => word("Arcering", "Filled region"),
                        "FamilyInstance" => word("Component", "Component"),
                        "CurveElem" => word("Lijn", "Line"),
                        _ => Word::Own(of),
                    };
                    rows.push((word("Getagd element", "Tagged element"), what));
                    rows.push((word("Id getagd element", "Tagged element id"), host.to_string().into()));
                }
            }
            _ if o.is_a("Dimension") => {
                // the lengths it measures, in the drawing
                let segments = o.get("m_ArrSegInfo").and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
                let values: Vec<String> = segments
                    .filter_map(|s| s.get("m_values")?.as_list()?.first()?.as_object()?.get("m_value")?.as_f64())
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .map(|v| format!("{:.0} mm", v * MM))
                    .collect();
                if !values.is_empty() {
                    rows.push((word("Gemeten", "Measured"), values.join(" + ").into()));
                }
            }
            _ => {}
        }
        if let Some(view) = element_of(o.get("m_ownerDBViewId")).and_then(|v| self.name(v)) {
            rows.push((word("Weergave", "View"), view.into()));
        }
        rows.push((word("Element-id", "Element id"), id.to_string().into()));
        let mut groups = vec![Arc::new(Group { title: word("Selectie", "Selection"), rows })];

        // the line style of a line: its own, else that of its category
        if o.class == "CurveElem" {
            let style = element_of(o.get("m_categoryId")).or_else(|| category.and_then(|c| self.default_style(c)));
            groups.extend(style.and_then(|s| self.line_style(s, scale, true)));
        }
        groups.extend(self.parameters(&o, word("Parameters", "Parameters")));
        if let Some(kind) = kind {
            groups.extend(self.kind(kind, scale));
        }
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_colours_read_as_the_user_knows_them() {
        assert_eq!(paper(0.00426509186351706), "1.3 mm");
        assert_eq!(paper(0.0005905511811023622), "0.18 mm");
        assert_eq!(paper(0.00984251968503937), "3 mm");
        assert_eq!(colour(6388941), Word::Own("RGB 205, 124, 97".into()));
        assert_eq!(colour(0), word("Zwart (RGB 0, 0, 0)", "Black (RGB 0, 0, 0)"));
    }
}
