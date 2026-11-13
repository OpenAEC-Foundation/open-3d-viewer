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
    let units = units(doc);
    // what the elements of a category store alike: read once per category, for all schedules
    let mut alike = Alike::default();
    for id in doc.object_ids_of_class("ScheduleInstance") {
        let Some(o) = doc.object(id) else { continue };
        let element = |name: &str| o.get(name).and_then(FieldValue::element).and_then(|e| u64::try_from(e).ok());
        let (Some(sheet), Some(schedule)) = (element("m_ownerDBViewId"), element("m_scheduleId")) else { continue };
        let origin = o.get("origin_XYZ").and_then(FieldValue::as_object).and_then(|p| super::rvt_drawings::vec3(p.get("m_xyz")));
        let (Some(origin), Some(mut table)) = (origin, table(doc, schedule, units, &mut alike)) else { continue };
        table.origin = [(origin[0] * FT) as f32, (origin[1] * FT) as f32];
        on.entry(sheet).or_default().push(table);
    }
    on
}

/// The table of a schedule, at the origin.
fn table(doc: &rvt_read::Document, schedule: u64, units: Option<u64>, alike: &mut Alike) -> Option<SheetTable> {
    let o = doc.object(schedule)?;
    let name = o.get("m_viewName").and_then(FieldValue::as_str).unwrap_or_default().to_string();
    let data = o.get("m_pData").and_then(|v| o.deref(v))?;
    let list = |of: &NestedObject, field: &str| -> Vec<&NestedObject> { of.get(field).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v)).collect() };
    let number = |of: &NestedObject, field: &str| of.get(field).and_then(FieldValue::as_i64).unwrap_or(0);
    let comma = units.and_then(|id| doc.object(id)).and_then(|o| o.get("m_units")?.as_object()?.get("m_decimalSymbol")?.as_i64()) == Some(1);
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
                        let value = alike.of(doc, category, id);
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

/// The element with the units of the project (`UnitsElem`). Finding it walks the file: look it
/// up once for all schedules.
pub(super) fn units(doc: &rvt_read::Document) -> Option<u64> {
    doc.object_ids_of_class("UnitsElem").first().copied()
}

/// Completes a computed schedule where it left cells open (`units`: [`units`]):
/// - a parameter the elements of a row do not store themselves is read from their types
///   (`m_pParams.m_params` of the symbol: its text, or its number as the schedule's cell for
///   that parameter formats it), when all of the row have it alike;
/// - what is still open takes the text the file stores for the cell (`m_text` of the rows of
///   the elements, as they showed when the file was saved): a computed row takes the first
///   stored row not taken yet that agrees with every text they both have, or the row in its
///   place when there are as many of each;
/// - a total row shows nothing where it totals nothing.
pub(super) fn complete(doc: &rvt_read::Document, s: &mut rvt_export::schedules::Schedule, units: Option<u64>) {
    let Some(o) = doc.object(s.id) else { return };
    let Some(data) = o.get("m_pData").and_then(|v| o.deref(v)) else { return };
    let list = |of: &NestedObject, field: &str| -> Vec<&NestedObject> { of.get(field).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(|v| o.deref(v)).collect() };
    let number = |of: &NestedObject, field: &str| of.get(field).and_then(FieldValue::as_i64).unwrap_or(0);
    let comma = units.and_then(|id| doc.object(id)).and_then(|o| o.get("m_units")?.as_object()?.get("m_decimalSymbol")?.as_i64()) == Some(1);
    // the rows of the elements as the file stores them, and how a parameter's cells are formatted
    let mut stored: Vec<Vec<String>> = Vec::new();
    let mut formats: HashMap<i64, &NestedObject> = HashMap::new();
    for section in list(data, "m_SectionData").into_iter().skip(1) {
        if section.get("m_hideSection").and_then(FieldValue::as_i64) == Some(1) {
            continue;
        }
        for row in list(section, "m_tableCells") {
            let cells = list(row, "m_arrCells");
            for cell in cells.iter().filter(|c| number(c, "m_cellType") == 2) {
                if let (Some(parameter), Some(format)) = (cell.get("m_paramId").and_then(FieldValue::element), cell.get("m_formatOptions").and_then(FieldValue::as_object)) {
                    formats.entry(parameter).or_insert(format);
                }
            }
            if cells.iter().any(|c| matches!(number(c, "m_cellType"), 2 | 6)) {
                stored.push(cells.iter().map(|c| c.get("m_text").and_then(FieldValue::as_str).unwrap_or_default().replace("\r\n", "\n")).collect());
            }
        }
    }
    let open = |cell: &Option<String>| cell.as_deref().is_none_or(|t| t.trim().is_empty());
    // how the project writes what a parameter measures (`UnitsElem`, by the parameter's spec),
    // for a cell that leaves its format to the project
    let units = units.and_then(|id| doc.object(id));
    let of_project = |parameter: i64| -> Option<&NestedObject> {
        let p = doc.object(u64::try_from(parameter).ok()?)?;
        let spec = p.get("m_pParamDef").and_then(|v| p.deref(v))?.get("m_specTypeId")?.as_object()?.get("m_typeId")?.as_str()?.rsplit_once('-')?.0.to_string();
        let all = units.as_ref()?.get("m_units")?.as_object()?.get("m_formatOptionsMap")?.as_list()?;
        all.iter().filter_map(FieldValue::as_object).find(|pair| pair.get("first").and_then(FieldValue::as_object).and_then(|t| t.get("m_typeId")?.as_str()).and_then(|t| t.rsplit_once('-')).is_some_and(|t| t.0 == spec))?.get("second")?.as_object()
    };
    // what the type of an element says for a parameter: a text, or a number
    let mut types: HashMap<u64, Vec<(i64, String, Option<f64>)>> = HashMap::new();
    let mut of_type = |element: u64, parameter: i64| -> Option<(String, Option<f64>)> {
        let e = doc.object(element)?;
        let own = e.path(&["m_pInstanceInfo", "m_symbolId"]).and_then(FieldValue::element);
        let kind = own.or_else(|| ["m_masterSymbolId", "m_symbolId"].into_iter().find_map(|f| e.get(f)?.element())).and_then(|k| u64::try_from(k).ok())?;
        let all = types.entry(kind).or_insert_with(|| {
            let Some(t) = doc.object(kind) else { return Vec::new() };
            let params = t.path(&["m_pParams", "m_params"]).and_then(FieldValue::as_list).unwrap_or(&[]);
            params.iter().filter_map(FieldValue::as_object).filter_map(|p| Some((p.get("m_paramId")?.element()?, p.get("m_str").and_then(FieldValue::as_str).unwrap_or_default().to_string(), p.get("m_value").and_then(FieldValue::as_f64).filter(|v| v.is_finite())))).collect()
        });
        all.iter().find(|p| p.0 == parameter).map(|p| (p.1.clone(), p.2))
    };
    for (i, column) in s.columns.iter().enumerate() {
        if column.parameter == -1 {
            continue;
        }
        // the date a sheet was issued: a field of the sheet itself
        if column.parameter == -1006322 {
            for row in s.rows.iter_mut().filter(|r| !r.total && r.cells.get(i).is_some_and(&open)) {
                let mut dates = row.elements.iter().map(|&e| doc.object(e).and_then(|o| o.get("m_issueDate").and_then(FieldValue::as_str).map(str::to_string)));
                if let Some(Some(first)) = dates.next() {
                    if dates.all(|d| d.as_deref() == Some(first.as_str())) {
                        row.cells[i] = Some(first);
                    }
                }
            }
            continue;
        }
        for row in s.rows.iter_mut().filter(|r| !r.total && r.cells.get(i).is_some_and(&open)) {
            let mut said = row.elements.iter().map(|&e| of_type(e, column.parameter));
            let Some(Some(first)) = said.next() else { continue };
            if !said.all(|other| other.as_ref() == Some(&first)) {
                continue;
            }
            let text = match first {
                (text, _) if !text.is_empty() => Some(text),
                (_, Some(value)) => formats.get(&column.parameter).and_then(|format| written(format, value, comma)).or_else(|| written(of_project(column.parameter)?, value, comma)),
                _ => None,
            };
            if let Some(text) = text {
                row.cells[i] = Some(text);
            }
        }
    }
    // the texts the file stores, by the columns that are shown
    let shown: Vec<usize> = (0..s.columns.len()).filter(|&i| !s.columns[i].hidden).collect();
    let rows: Vec<usize> = (0..s.rows.len()).filter(|&r| !s.rows[r].total).collect();
    // the stored row each computed row took
    let mut took: Vec<Option<usize>> = vec![None; rows.len()];
    let mut taken = vec![false; stored.len()];
    for (n, &r) in rows.iter().enumerate() {
        let agrees = |(_, texts): &(usize, &Vec<String>)| {
            let both = shown.iter().enumerate().filter_map(|(k, &i)| Some((texts.get(k)?.trim(), s.rows[r].cells.get(i)?.as_deref()?.trim()))).filter(|(a, b)| !a.is_empty() && !b.is_empty());
            let (mut compared, mut same) = (0, true);
            for (a, b) in both {
                compared += 1;
                same &= a == b;
            }
            compared > 0 && same
        };
        // the first that agrees and is not taken yet: rows that read alike come in their order
        let at = stored.iter().enumerate().filter(|(k, _)| !taken[*k]).find(agrees).map(|(k, _)| k).or((stored.len() == rows.len() && !taken[n]).then_some(n));
        if let Some(k) = at {
            taken[k] = true;
        }
        took[n] = at;
        let Some(texts) = at.map(|k| stored[k].clone()) else { continue };
        for (k, &i) in shown.iter().enumerate() {
            if let (Some(cell), Some(text)) = (s.rows[r].cells.get_mut(i), texts.get(k).filter(|t| !t.trim().is_empty())) {
                if open(cell) {
                    *cell = Some(text.clone());
                }
            }
        }
    }
    // rows the file did not show: where it stores fewer rows than were computed and each of
    // those was taken by one computed row, the others were left out by a filter that could not
    // be computed (one on a parameter of the type)
    let mut used: Vec<usize> = took.iter().flatten().copied().collect();
    used.sort_unstable();
    used.dedup();
    if !stored.is_empty() && stored.len() < rows.len() && used.len() == stored.len() && took.iter().flatten().count() == stored.len() {
        let left_out: std::collections::HashSet<usize> = rows.iter().zip(&took).filter(|(_, t)| t.is_none()).map(|(&r, _)| r).collect();
        let mut k = 0;
        s.rows.retain(|_| {
            k += 1;
            !left_out.contains(&(k - 1))
        });
    }
    for row in s.rows.iter_mut().filter(|r| r.total) {
        for cell in row.cells.iter_mut().filter(|c| c.is_none()) {
            *cell = Some(String::new());
        }
    }
}

/// The numbers every element of a category stores alike (internal units), by parameter: on the
/// element itself or on its type. A category is read once, its elements one after the other, for
/// all its parameters: reading it again for every column of every schedule took minutes on a
/// large project.
#[derive(Default)]
struct Alike {
    categories: HashMap<i32, HashMap<i64, Option<f64>>>,
}

impl Alike {
    /// The number every element of `category` has for `parameter`; none when they differ or none
    /// stores one.
    fn of(&mut self, doc: &rvt_read::Document, category: i64, parameter: i64) -> Option<f64> {
        let category = i32::try_from(category).ok()?;
        self.categories.entry(category).or_insert_with(|| uniform(doc, category)).get(&parameter).copied().flatten()
    }
}

/// See [`Alike`]: per parameter the number all elements of the category have alike (`None`:
/// they differ). An element's own value goes before that of its type.
fn uniform(doc: &rvt_read::Document, category: i32) -> HashMap<i64, Option<f64>> {
    // the numbers a type stores, by parameter
    let mut types: HashMap<u64, HashMap<i64, f64>> = HashMap::new();
    let of_type = |kind: u64| -> HashMap<i64, f64> {
        let Some(o) = doc.object(kind) else { return HashMap::new() };
        let all = o.path(&["m_pParams", "m_params"]).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
        let mut numbers = HashMap::new();
        for p in all {
            if let (Some(id), Some(value)) = (p.get("m_paramId").and_then(FieldValue::element), p.get("m_value").and_then(FieldValue::as_f64)) {
                // the first of a parameter counts
                numbers.entry(id).or_insert(value);
            }
        }
        numbers
    };
    let mut found: HashMap<i64, Option<f64>> = HashMap::new();
    // the numbers each element stores itself and its type, read on all cores
    let ids: Vec<u64> = doc.elements_in_category(category).map(|e| e.id).collect();
    let read = rvt_core::par::map(&ids, |&id| {
        let o = doc.object(id)?;
        let own = o.get("m_pParamValueSetDouble").and_then(|v| o.deref(v)).and_then(|s| s.get("m_paramSet")?.as_list());
        let mut numbers: HashMap<i64, f64> = HashMap::new();
        for p in own.unwrap_or(&[]).iter().filter_map(FieldValue::as_object) {
            if let (Some(id), Some(value)) = (p.get("m_paramId").and_then(FieldValue::element), p.get("m_value").and_then(FieldValue::as_f64)) {
                numbers.entry(id).or_insert(value);
            }
        }
        let kind = ["m_masterSymbolId", "m_symbolId"].into_iter().find_map(|f| u64::try_from(o.get(f)?.element()?).ok());
        Some((numbers, kind))
    });
    for (mut numbers, kind) in read.into_iter().flatten() {
        if let Some(kind) = kind {
            for (&id, &value) in types.entry(kind).or_insert_with(|| of_type(kind)).iter() {
                numbers.entry(id).or_insert(value);
            }
        }
        for (id, value) in numbers.into_iter().filter(|(_, v)| v.is_finite()) {
            match found.get(&id) {
                // they differ: stays so
                Some(None) => {}
                Some(Some(known)) if (known - value).abs() > 1e-9 => {
                    found.insert(id, None);
                }
                _ => {
                    found.insert(id, Some(value));
                }
            }
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
    if decimals > 0 && format.get("m_bSuppressTrailingZeros").and_then(FieldValue::as_i64) == Some(1) {
        figure = figure.trim_end_matches('0').trim_end_matches('.').to_string();
    }
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
    /// The computed schedules of a project, completed: fewer open cells than before, none in a
    /// total row (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn completing_a_schedule_fills_open_cells() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with schedules");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let open = |s: &rvt_export::schedules::Schedule| s.rows.iter().flat_map(|r| r.cells.iter().enumerate()).filter(|(i, c)| !s.columns[*i].hidden && c.is_none()).count();
        let (mut before, mut after) = (0, 0);
        for mut s in rvt_export::schedules::schedules(&doc).into_iter().filter(|s| !s.is_template) {
            let was = open(&s);
            super::complete(&doc, &mut s, super::units(&doc));
            eprintln!("{:?}: {} open cells, {} after", s.name, was, open(&s));
            for r in s.rows.iter().take(7) {
                eprintln!("    {:?}", r.cells.iter().enumerate().filter(|(i, _)| !s.columns[*i].hidden).map(|(_, c)| c.as_deref().unwrap_or("?")).collect::<Vec<_>>());
            }
            assert!(s.rows.iter().filter(|r| r.total).all(|r| r.cells.iter().all(Option::is_some)));
            before += was;
            after += open(&s);
        }
        assert!(after <= before);
    }
}
