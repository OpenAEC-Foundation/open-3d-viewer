//! The schedules placed on the sheets of a Revit file, as tables: the columns and rows the file
//! stores for each, with the texts of their cells.
//!
//! Read through the generic object view, named schema fields only:
//! - a placed schedule is a `ScheduleInstance`: `m_ownerDBViewId` the sheet it is on,
//!   `m_scheduleId` its schedule and `origin_XYZ` where the upper left corner of its table lies
//!   on the sheet (feet);
//! - the schedule, a `DBViewSchedule`, keeps its table in `m_pData`: a section for the title and
//!   one for the body (`m_SectionData`, left out when `m_hideSection`), each with its columns
//!   (`m_dWidth`), its rows (`m_dHeight`) and per row its cells (`m_tableCells`);
//! - a cell (`TableCellData`) has the text it showed when the file was saved (`m_text`), how many
//!   cells to its right it covers (`m_coverRight`; `m_coverLeft` below zero: covered itself) and
//!   its style: its text size in 1/96 inch and how its text is aligned.
//!
//! The text of a cell that shows a parameter of the elements (`m_cellType` 2) is not always
//! stored; such a cell is marked open, to be filled from the computed schedule.

use crate::model::{SheetTable, TableCell, TableRow};
use rvt_model::{FieldValue, NestedObject};
use std::collections::HashMap;

const FT: f64 = 0.3048;
/// A line's height to its em, as the sheet sets its texts.
const LINE: f64 = 1.3;
/// What a letter's em is to the text size a cell stores: a text of 2.5 mm is set in 10 points.
const EM: f64 = 4.0 * 25.4 / 72.0;
/// What a row keeps free above and below its text (metres).
const PAD: f64 = 0.0008;
/// The parameter a title cell shows: the name of the view.
const VIEW_NAME: i64 = -1005112;

/// The schedules placed on each sheet, by the sheet's element.
pub(super) fn placed(doc: &rvt_read::Document) -> HashMap<u64, Vec<SheetTable>> {
    let mut on: HashMap<u64, Vec<SheetTable>> = HashMap::new();
    for id in doc.object_ids_of_class("ScheduleInstance") {
        let Some(o) = doc.object(id) else { continue };
        let element = |name: &str| o.get(name).and_then(FieldValue::element).and_then(|e| u64::try_from(e).ok());
        let (Some(sheet), Some(schedule)) = (element("m_ownerDBViewId"), element("m_scheduleId")) else { continue };
        let origin = o.get("origin_XYZ").and_then(FieldValue::as_object).and_then(|p| super::rvt_drawings::vec3(p.get("m_xyz")));
        let (Some(origin), Some(mut table)) = (origin, table(doc, schedule)) else { continue };
        table.origin = [(origin[0] * FT) as f32, (origin[1] * FT) as f32];
        on.entry(sheet).or_default().push(table);
    }
    on
}

/// The table of a schedule, at the origin.
fn table(doc: &rvt_read::Document, schedule: u64) -> Option<SheetTable> {
    let o = doc.object(schedule)?;
    let name = o.get("m_viewName").and_then(FieldValue::as_str).unwrap_or_default().to_string();
    let data = o.get("m_pData").and_then(|v| o.deref(v))?;
    let list = |of: &NestedObject, field: &str| -> Vec<&NestedObject> { of.get(field).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v)).collect() };
    let number = |of: &NestedObject, field: &str| of.get(field).and_then(FieldValue::as_i64).unwrap_or(0);
    // the values all elements of a category share, by category and parameter
    let mut alike: HashMap<(i64, i64), Option<f64>> = HashMap::new();
    let comma = doc.object_ids_of_class("UnitsElem").first().and_then(|&id| doc.object(id)).and_then(|o| o.get("m_units")?.as_object()?.get("m_decimalSymbol")?.as_i64()) == Some(1);
    let mut columns: Vec<f32> = Vec::new();
    let mut rows = Vec::new();
    // the rows of the elements are as high as the schedule sets for rows with an image
    let image_row = if o.get("m_rowHeightOverride").and_then(FieldValue::as_i64).unwrap_or(0) != 0 { o.get("m_imageRowHeight").and_then(FieldValue::as_f64).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(0.0) * FT } else { 0.0 };
    for (k, section) in list(data, "m_SectionData").into_iter().enumerate() {
        if section.get("m_hideSection").and_then(FieldValue::as_i64) == Some(1) {
            continue;
        }
        let widths: Vec<f32> = list(section, "m_cols").iter().map(|c| (c.get("m_dWidth").and_then(FieldValue::as_f64).unwrap_or(0.0) * FT) as f32).collect();
        // the body gives the columns; the title runs over all of them
        if k > 0 || columns.is_empty() {
            columns.clone_from(&widths);
        }
        let heights = list(section, "m_rows");
        for (r, row) in list(section, "m_tableCells").into_iter().enumerate() {
            let mut cells = Vec::new();
            let mut tall = 0.0f64;
            // a row of the elements has a cell that shows a parameter or a computed value
            let mut of_elements = false;
            for (c, cell) in list(row, "m_arrCells").into_iter().enumerate() {
                if number(cell, "m_coverLeft") < 0 || number(cell, "m_coverTop") < 0 {
                    continue;
                }
                let style = cell.get("m_oTableCellStyle").and_then(|v| o.deref(v));
                let em = style.and_then(|s| s.get("m_textSize")?.as_f64()).filter(|s| s.is_finite() && *s > 0.0).map_or(crate::document::NOTE_HEIGHT, |s| s / 96.0 * 0.0254 * EM);
                let mut text = cell.get("m_text").and_then(FieldValue::as_str).unwrap_or_default().replace("\r\n", "\n");
                let parameter = number(cell, "m_cellType") == 2;
                of_elements |= k > 0 && matches!(number(cell, "m_cellType"), 2 | 6);
                if text.is_empty() && parameter && cell.get("m_paramId").and_then(FieldValue::element) == Some(VIEW_NAME) {
                    text.clone_from(&name);
                }
                // one line fits a row of its own height; more lines need their room
                let lines = text.lines().count().max(1) as f64;
                // a length every element of the category has alike is what each row shows
                if text.is_empty() && parameter && k > 0 {
                    let ids = (cell.get("m_categoryId").and_then(FieldValue::element), cell.get("m_paramId").and_then(FieldValue::element));
                    if let (Some(category), Some(id), Some(format)) = (ids.0, ids.1, cell.get("m_formatOptions").and_then(FieldValue::as_object)) {
                        let key = (category, id);
                        let value = *alike.entry(key).or_insert_with(|| uniform(doc, category, id));
                        if let Some(written) = value.and_then(|feet| written(format, feet, comma)) {
                            text = written;
                        }
                    }
                }
                tall = tall.max(if lines > 1.0 { lines * em * LINE + 2.0 * PAD } else { em * 1.2 });
                cells.push(TableCell {
                    column: c as u16,
                    span: (number(cell, "m_coverRight").max(0) + 1) as u16,
                    open: k > 0 && parameter && text.is_empty(),
                    image: None,
                    text,
                    size: em as f32,
                    align: style.map_or(0, |s| number(s, "m_horizontalAlignment").clamp(0, 2)) as u8,
                    vertical: style.map_or(0, |s| [0, 1, 2][(number(s, "m_verticalAlignment") / 4).clamp(0, 2) as usize]),
                });
            }
            // a row is as high as the file says, or as its text needs
            let stored = heights.get(r).and_then(|h| h.get("m_dHeight")?.as_f64()).unwrap_or(0.0) * FT;
            let height = stored.max(tall).max(if of_elements { image_row } else { 0.0 });
            // a blank line and a footer draw no borders between their cells
            rows.push(TableRow { height: height as f32, cells, title: k == 0, plain: k > 0 && r > 0 && !of_elements });
        }
    }
    (!rows.is_empty()).then_some(SheetTable { schedule: name, origin: [0.0; 2], columns, rows })
}

/// The number every element of a category stores alike for a parameter (internal units), on the
/// element itself or on its type; none when they differ or none stores one.
fn uniform(doc: &rvt_read::Document, category: i64, parameter: i64) -> Option<f64> {
    let category = i32::try_from(category).ok()?;
    let mut types: HashMap<u64, Option<f64>> = HashMap::new();
    let of_type = |kind: u64| -> Option<f64> {
        let o = doc.object(kind)?;
        let all = o.path(&["m_pParams", "m_params"])?.as_list()?.iter().filter_map(FieldValue::as_object);
        all.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).find_map(|p| p.get("m_value")?.as_f64())
    };
    let mut found: Option<f64> = None;
    for element in doc.elements_in_category(category) {
        let Some(o) = doc.object(element.id) else { continue };
        let own = o.get("m_pParamValueSetDouble").and_then(|v| o.deref(v)).and_then(|s| s.get("m_paramSet")?.as_list());
        let own = own.unwrap_or(&[]).iter().filter_map(FieldValue::as_object).find(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).and_then(|p| p.get("m_value")?.as_f64());
        let value = own.or_else(|| {
            let kind = ["m_masterSymbolId", "m_symbolId"].into_iter().find_map(|f| u64::try_from(o.get(f)?.element()?).ok())?;
            *types.entry(kind).or_insert_with(|| of_type(kind))
        });
        let Some(value) = value.filter(|v| v.is_finite()) else { continue };
        match found {
            Some(known) if (known - value).abs() > 1e-9 => return None,
            _ => found = Some(value),
        }
    }
    found
}

/// A length (feet) as the format of a cell writes it: its unit, its decimals, a plus before what
/// lies above zero and the unit's sign after it. None for a format that leaves it to the project
/// or names a unit that is no length.
fn written(format: &NestedObject, feet: f64, comma: bool) -> Option<String> {
    if format.get("m_bUseDefault").and_then(FieldValue::as_i64) == Some(1) {
        return None;
    }
    let named = |field: &str| format.get(field).and_then(FieldValue::as_object).and_then(|t| t.get("m_typeId")?.as_str()).unwrap_or_default();
    let (per_foot, sign) = match named("m_unitTypeId").split(':').nth(1)?.split('-').next()? {
        "meters" => (FT, "m"),
        "centimeters" => (FT * 100.0, "cm"),
        "millimeters" => (FT * 1000.0, "mm"),
        "feet" => (1.0, "ft"),
        _ => return None,
    };
    let accuracy = format.get("m_accuracy").and_then(FieldValue::as_f64).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(1.0);
    let decimals = (-accuracy.log10()).round().clamp(0.0, 9.0) as usize;
    let value = feet * per_foot;
    let mut figure = format!("{value:.decimals$}");
    if comma {
        figure = figure.replace('.', ",");
    }
    if value > 0.0 && format.get("m_bUsePlusPrefix").and_then(FieldValue::as_i64) == Some(1) {
        figure.insert(0, '+');
    }
    if !named("m_symbolTypeId").is_empty() {
        figure.push(' ');
        figure.push_str(sign);
    }
    Some(figure)
}

#[cfg(test)]
mod tests {

    /// A project with schedules on its sheets (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn placed_schedules_become_tables_of_their_sheets() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with schedules on its sheets");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let placed = super::placed(&doc);
        let tables: Vec<_> = placed.values().flatten().collect();
        for t in &tables {
            eprintln!("{:?} at {:?}: {} columns, {} rows, {} texts", t.schedule, t.origin, t.columns.len(), t.rows.len(), t.rows.iter().flat_map(|r| &r.cells).filter(|c| !c.text.is_empty()).count());
        }
        assert!(!tables.is_empty(), "no schedule on any sheet");
        assert!(tables.iter().all(|t| t.rows.iter().all(|r| r.height > 0.0)));
        assert!(tables.iter().any(|t| t.rows.iter().flat_map(|r| &r.cells).any(|c| !c.text.is_empty())));
        // a length every pile has alike, written as its cell formats it
        let texts: Vec<&str> = tables.iter().flat_map(|t| &t.rows).flat_map(|r| &r.cells).map(|c| c.text.as_str()).collect();
        eprintln!("signed lengths: {:?}", texts.iter().filter(|t| t.starts_with('+')).collect::<Vec<_>>());
    }
}
