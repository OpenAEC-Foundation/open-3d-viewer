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
    }
}
