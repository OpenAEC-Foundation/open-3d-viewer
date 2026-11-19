//! A house style: how an office's drawings and documents look, as an object of its own that is
//! saved and taken to another project.
//!
//! A house style says how things look in general: who the office is, its logo, its colours and
//! its typefaces, the sheet formats it draws on, its title block and how it writes dimensions.
//! It names the drawing standard it follows ([`crate::drawing::Standard`]), which holds the
//! pens, the line styles, the text styles and the hatches; what a drawing of a kind shows is
//! that standard's, not the house style's.
//!
//! - A colour has a role (`primary`, `accent`, `text`, ...) that sheets and reports ask for; an
//!   office adds colours of its own under other keys.
//! - A sheet format is a paper size with its margins; the title block lies in the lower right
//!   corner of the drawing area, inside the margins.
//! - The title block is rows of cells, each a field of the sheet or the project (its title, its
//!   number, the scale, the date, who drew and who checked it, its status and its revision) or
//!   the office's own (its name, its logo, its address), each cell a share of the row's width.
//! - The logo is an image as a data URI, so a house style is one file.
//!
//! In IFCX a house style is a node with its English name and `openaec::housestyle` (all of it).
//! A project file holds the house style it follows the same way; a house style file is an IFCX
//! file of that node alone.

use crate::material::Names;
use crate::{uuid, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The attribute of a house style's node.
pub const HOUSE_STYLE: &str = "openaec::housestyle";

/// Who the office is, as its title block and its reports write it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Company {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub postcode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub city: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub country: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phone: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub website: String,
    /// Its number in the trade register (KvK).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub registration: String,
}

/// A colour of the house style, by its role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colour {
    /// Stable, lower case with hyphens: `primary`, `secondary`, `accent`, `text`, `muted`,
    /// `background`, or a key of the office's own.
    pub key: String,
    pub names: Names,
    pub rgb: [u8; 3],
}

/// The typefaces: of running text and of headings, by their family names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fonts {
    pub text: String,
    pub heading: String,
}

/// A sheet format: a paper with its margins, in millimetres. `width` is the long side: a sheet
/// lies in landscape unless a sheet says otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetFormat {
    /// "A1".
    pub key: String,
    pub width: f64,
    pub height: f64,
    /// Left (the binding edge), top, right, bottom.
    pub margins: [f64; 4],
}

impl SheetFormat {
    /// The area inside the margins: its width and height.
    pub fn drawing_area(&self) -> [f64; 2] {
        [self.width - self.margins[0] - self.margins[2], self.height - self.margins[1] - self.margins[3]]
    }
}

/// What a cell of the title block shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Field {
    Logo,
    Company,
    Address,
    Project,
    ProjectNumber,
    Client,
    /// The title of the sheet.
    Title,
    /// The number of the sheet.
    Number,
    Scale,
    Format,
    Date,
    Phase,
    Status,
    Revision,
    DrawnBy,
    CheckedBy,
}

impl Field {
    pub const ALL: [Field; 16] = [
        Field::Logo,
        Field::Company,
        Field::Address,
        Field::Project,
        Field::ProjectNumber,
        Field::Client,
        Field::Title,
        Field::Number,
        Field::Scale,
        Field::Format,
        Field::Date,
        Field::Phase,
        Field::Status,
        Field::Revision,
        Field::DrawnBy,
        Field::CheckedBy,
    ];

    /// The label a title block writes over the field, in Dutch and in English; none for the
    /// office's own (its logo, its name, its address).
    pub fn label(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Field::Logo | Field::Company | Field::Address => return None,
            Field::Project => ("project", "project"),
            Field::ProjectNumber => ("projectnummer", "project number"),
            Field::Client => ("opdrachtgever", "client"),
            Field::Title => ("onderwerp", "title"),
            Field::Number => ("bladnummer", "sheet"),
            Field::Scale => ("schaal", "scale"),
            Field::Format => ("formaat", "format"),
            Field::Date => ("datum", "date"),
            Field::Phase => ("fase", "phase"),
            Field::Status => ("status", "status"),
            Field::Revision => ("wijziging", "revision"),
            Field::DrawnBy => ("getekend", "drawn"),
            Field::CheckedBy => ("gecontroleerd", "checked"),
        })
    }
}

/// A cell of a row of the title block: a field and its share of the row's width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub field: Field,
    pub share: f64,
}

/// A row of the title block: its height in millimetres and its cells from the left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub height: f64,
    pub cells: Vec<Cell>,
}

/// The title block: its width in millimetres and its rows from the top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub width: f64,
    pub rows: Vec<Row>,
}

impl TitleBlock {
    pub fn height(&self) -> f64 {
        self.rows.iter().map(|row| row.height).sum()
    }

    /// Every cell as a rectangle of the block: its field, and its left, top, width and height
    /// in millimetres from the block's upper left corner.
    pub fn cells(&self) -> Vec<(Field, [f64; 4])> {
        let mut out = Vec::new();
        let mut top = 0.0;
        for row in &self.rows {
            let shares: f64 = row.cells.iter().map(|cell| cell.share).sum();
            let mut left = 0.0;
            for cell in &row.cells {
                let width = self.width * cell.share / shares;
                out.push((cell.field, [left, top, width, row.height]));
                left += width;
            }
            top += row.height;
        }
        out
    }
}

/// How the end of a dimension line is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Terminator {
    /// A short oblique stroke, as Dutch building drawings have it.
    #[default]
    Tick,
    Arrow,
    Dot,
}

/// How dimensions are written, in millimetres on paper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionStyle {
    pub terminator: Terminator,
    /// The size of the tick, the arrow or the dot.
    pub terminator_size: f64,
    pub text_height: f64,
    /// How far an extension line starts from what is measured, and how far it runs past the
    /// dimension line.
    pub gap: f64,
    pub overshoot: f64,
}

/// A house style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HouseStyle {
    /// Stable, lower case with hyphens: what a project names.
    pub key: String,
    pub company: Company,
    /// An image as a data URI (`data:image/png;base64,...` or `data:image/svg+xml;...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub colours: Vec<Colour>,
    pub fonts: Fonts,
    /// The key of the drawing standard it follows ("nl").
    pub standard: String,
    pub sheets: Vec<SheetFormat>,
    pub title_block: TitleBlock,
    pub dimensions: DimensionStyle,
}

impl HouseStyle {
    /// The house style a project starts with: the suite's own, neutral, to be made an office's.
    /// Its colours are the suite's interface colours in the light theme; its sheets are the A
    /// formats of ISO 216 in landscape with the margins of ISO 5457 (20 mm at the binding edge,
    /// 10 mm elsewhere); its title block is 180 mm wide, as ISO 7200 has it at most.
    pub fn starter() -> Self {
        let colour = |key: &str, nl: &str, en: &str, de: &str, fr: &str, rgb| Colour { key: key.into(), names: Names::new(nl, en, de, fr), rgb };
        let sheet = |key: &str, width: f64, height: f64| SheetFormat { key: key.into(), width, height, margins: [20.0, 10.0, 10.0, 10.0] };
        let row = |height: f64, cells: &[(Field, f64)]| Row { height, cells: cells.iter().map(|&(field, share)| Cell { field, share }).collect() };
        Self {
            key: "openaec".into(),
            company: Company { name: "OpenAEC".into(), ..Company::default() },
            logo: None,
            colours: vec![
                colour("primary", "hoofdkleur", "primary", "Hauptfarbe", "couleur principale", [0, 112, 173]),
                colour("secondary", "tweede kleur", "secondary", "Zweitfarbe", "couleur secondaire", [45, 55, 72]),
                colour("accent", "accent", "accent", "Akzent", "accent", [232, 119, 34]),
                colour("text", "tekst", "text", "Text", "texte", [26, 32, 44]),
                colour("muted", "tweede tekst", "secondary text", "Zweittext", "texte secondaire", [113, 128, 150]),
                colour("background", "achtergrond", "background", "Hintergrund", "fond", [255, 255, 255]),
            ],
            fonts: Fonts { text: "Inter".into(), heading: "Inter".into() },
            standard: "nl".into(),
            sheets: vec![sheet("A0", 1189.0, 841.0), sheet("A1", 841.0, 594.0), sheet("A2", 594.0, 420.0), sheet("A3", 420.0, 297.0), sheet("A4", 297.0, 210.0)],
            title_block: TitleBlock {
                width: 180.0,
                rows: vec![
                    row(18.0, &[(Field::Logo, 1.0), (Field::Company, 2.0), (Field::Address, 3.0)]),
                    row(10.0, &[(Field::Project, 4.0), (Field::ProjectNumber, 2.0)]),
                    row(10.0, &[(Field::Client, 4.0), (Field::Phase, 2.0)]),
                    row(12.0, &[(Field::Title, 4.0), (Field::Number, 2.0)]),
                    row(8.0, &[(Field::Scale, 1.0), (Field::Format, 1.0), (Field::Date, 1.0), (Field::Status, 1.0), (Field::Revision, 1.0)]),
                    row(8.0, &[(Field::DrawnBy, 1.0), (Field::CheckedBy, 1.0)]),
                ],
            },
            dimensions: DimensionStyle { terminator: Terminator::Tick, terminator_size: 2.0, text_height: 2.5, gap: 1.5, overshoot: 1.5 },
        }
    }

    /// The colour of role `key`.
    pub fn colour(&self, key: &str) -> Option<[u8; 3]> {
        self.colours.iter().find(|c| c.key == key).map(|c| c.rgb)
    }

    /// The sheet format called `key` ("A1").
    pub fn sheet(&self, key: &str) -> Option<&SheetFormat> {
        self.sheets.iter().find(|s| s.key == key)
    }

    /// Whether it holds together: a key and a company name, colours and sheet formats each once,
    /// sheets larger than their margins and than the title block, rows with cells that have a
    /// share, positive sizes. The text says what does not.
    pub fn check(&self) -> Result<(), LibraryError> {
        let bad = |what: String| Err(LibraryError::BadHouseStyle(what));
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if self.key.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if self.company.name.trim().is_empty() {
            return bad("a company has a name".into());
        }
        for (k, colour) in self.colours.iter().enumerate() {
            if colour.key.trim().is_empty() || self.colours[..k].iter().any(|c| c.key == colour.key) {
                return Err(LibraryError::Duplicate(colour.key.clone()));
            }
        }
        if self.fonts.text.trim().is_empty() || self.fonts.heading.trim().is_empty() {
            return bad("a typeface has a name".into());
        }
        let block = &self.title_block;
        if !positive(block.width) || block.rows.is_empty() {
            return bad("a title block has a width and rows".into());
        }
        for row in &block.rows {
            if !positive(row.height) || row.cells.is_empty() || row.cells.iter().any(|cell| !positive(cell.share)) {
                return bad("a row of the title block has a height and cells with a share".into());
            }
        }
        for (k, sheet) in self.sheets.iter().enumerate() {
            if sheet.key.trim().is_empty() || self.sheets[..k].iter().any(|s| s.key == sheet.key) {
                return Err(LibraryError::Duplicate(sheet.key.clone()));
            }
            let [width, height] = sheet.drawing_area();
            if !positive(sheet.width) || !positive(sheet.height) || sheet.margins.iter().any(|m| !m.is_finite() || *m < 0.0) || !positive(width) || !positive(height) {
                return bad(format!("sheet {} is larger than its margins", sheet.key));
            }
            if block.width > width + 1e-9 || block.height() > height + 1e-9 {
                return bad(format!("the title block fits on sheet {}", sheet.key));
            }
        }
        let d = &self.dimensions;
        if !positive(d.terminator_size) || !positive(d.text_height) || !d.gap.is_finite() || d.gap < 0.0 || !d.overshoot.is_finite() || d.overshoot < 0.0 {
            return bad("dimensions have a text height and a terminator size".into());
        }
        Ok(())
    }
}

/// The path of the node of house style `key`.
pub fn path(key: &str) -> String {
    uuid(&format!("openaec-library/housestyle/{key}"))
}

/// The node of a house style.
pub fn node(style: &HouseStyle) -> Value {
    json!({"path": path(&style.key), "attributes": {"bsi::ifc::prop::Name": style.company.name, HOUSE_STYLE: style}})
}

/// A house style file: an IFCX file of the style's node alone, with `id`, `author` and
/// `timestamp` (`YYYY-MM-DDTHH:MM:SSZ`) in its header.
pub fn file(style: &HouseStyle, id: &str, author: &str, timestamp: &str) -> Value {
    json!({
        "header": {"id": id, "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": author, "timestamp": timestamp},
        "imports": [],
        "schemas": {},
        "data": [node(style)],
    })
}

/// The house styles in an IFCX file, in the order of their nodes. The attributes of nodes with
/// the same path are composed, a later one winning, as IFCX layers them. Refused when the file
/// has no data, or a house style does not hold together.
pub fn read(file: &Value) -> Result<Vec<HouseStyle>, LibraryError> {
    let data = file.get("data").and_then(Value::as_array).ok_or_else(|| LibraryError::Unreadable("no data".into()))?;
    let mut order: Vec<&str> = Vec::new();
    let mut composed: std::collections::HashMap<&str, Map<String, Value>> = std::collections::HashMap::new();
    for node in data {
        let (Some(path), Some(attributes)) = (node.get("path").and_then(Value::as_str), node.get("attributes").and_then(Value::as_object)) else { continue };
        let entry = composed.entry(path).or_insert_with(|| {
            order.push(path);
            Map::new()
        });
        for (key, value) in attributes {
            if value.is_null() {
                entry.remove(key);
            } else {
                entry.insert(key.clone(), value.clone());
            }
        }
    }
    let mut styles = Vec::new();
    for path in order {
        if let Some(own) = composed[path].get(HOUSE_STYLE) {
            let style: HouseStyle = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("house style {path}: {e}")))?;
            style.check()?;
            styles.push(style);
        }
    }
    Ok(styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_starter_house_style_holds_together() {
        let style = HouseStyle::starter();
        style.check().unwrap();
        assert_eq!(style.colour("primary"), Some([0, 112, 173]));
        assert_eq!(style.sheets.iter().map(|s| s.key.as_str()).collect::<Vec<_>>(), ["A0", "A1", "A2", "A3", "A4"]);
        // ISO 216: each format half the one before; ISO 5457: 20 mm to bind, 10 mm elsewhere
        let a1 = style.sheet("A1").unwrap();
        assert_eq!((a1.width, a1.height, a1.drawing_area()), (841.0, 594.0, [811.0, 574.0]));
        // every field once, the block as wide as its rows and fitting the smallest sheet
        let cells = style.title_block.cells();
        let fields: HashSet<Field> = cells.iter().map(|c| c.0).collect();
        assert_eq!((fields.len(), cells.len()), (Field::ALL.len(), Field::ALL.len()));
        assert_eq!((style.title_block.width, style.title_block.height()), (180.0, 66.0));
        for row_top in [0.0, 18.0, 28.0] {
            let width: f64 = cells.iter().filter(|c| c.1[1] == row_top).map(|c| c.1[2]).sum();
            assert!((width - 180.0).abs() < 1e-9, "{row_top}: {width}");
        }
        let title = cells.iter().find(|c| c.0 == Field::Title).unwrap().1;
        assert_eq!(title, [0.0, 38.0, 120.0, 12.0]);
        assert_eq!((Field::Title.label(), Field::Logo.label()), (Some(("onderwerp", "title")), None));
    }

    #[test]
    fn a_house_style_reads_back_from_its_file() {
        let mut style = HouseStyle::starter();
        style.key = "3bm".into();
        style.company = Company { name: "3BM Bouwkunde".into(), city: "Dordrecht".into(), registration: "12345678".into(), ..Company::default() };
        style.logo = Some("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg'/>".into());
        style.colours.push(Colour { key: "site".into(), names: Names::same("terrein"), rgb: [120, 150, 90] });
        let written = file(&style, "3bm.ifcx", "test", "2026-11-19T00:00:00Z");
        let nodes = written["data"].as_array().unwrap();
        assert_eq!((nodes.len(), nodes[0]["path"].as_str(), nodes[0]["attributes"]["bsi::ifc::prop::Name"].as_str()), (1, Some(path("3bm").as_str()), Some("3BM Bouwkunde")));
        // what is empty is left out of the file
        assert!(nodes[0]["attributes"][HOUSE_STYLE]["company"].get("phone").is_none());
        let text = serde_json::to_string(&written).unwrap();
        assert_eq!(read(&serde_json::from_str(&text).unwrap()).unwrap(), vec![style.clone()]);
        // a later node of the same path changes it, as IFCX layers do
        let mut layered = written.clone();
        let mut renamed = style.clone();
        renamed.company.name = "3BM".into();
        layered["data"].as_array_mut().unwrap().push(node(&renamed));
        assert_eq!(read(&layered).unwrap(), vec![renamed]);
        assert_eq!(read(&json!({"data": []})).unwrap(), Vec::new());
        assert!(read(&json!({})).is_err());
    }

    #[test]
    fn what_does_not_hold_together_is_refused() {
        let ok = HouseStyle::starter();
        let refused = |change: &dyn Fn(&mut HouseStyle)| {
            let mut style = ok.clone();
            change(&mut style);
            style.check().is_err()
        };
        assert!(refused(&|s| s.key = " ".into()));
        assert!(refused(&|s| s.company.name.clear()));
        assert!(refused(&|s| s.colours.push(s.colours[0].clone())), "a colour twice");
        assert!(refused(&|s| s.fonts.heading.clear()));
        assert!(refused(&|s| s.sheets.push(s.sheets[0].clone())), "a sheet format twice");
        assert!(refused(&|s| s.sheets[4].margins = [150.0, 10.0, 150.0, 10.0]), "margins wider than the sheet");
        assert!(refused(&|s| s.title_block.width = 290.0), "a title block wider than A4's drawing area");
        assert!(refused(&|s| s.title_block.rows[0].cells.clear()));
        assert!(refused(&|s| s.title_block.rows[0].cells[0].share = 0.0));
        assert!(refused(&|s| s.dimensions.text_height = 0.0));
        // a file with such a style is refused too
        let mut bad = ok.clone();
        bad.fonts.text.clear();
        assert!(read(&file(&bad, "x", "t", "2026-11-19T00:00:00Z")).is_err());
    }
}
