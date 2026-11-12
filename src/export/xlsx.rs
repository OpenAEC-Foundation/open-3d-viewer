//! Schedules as an Excel workbook (`.xlsx`): one sheet per schedule, its title above its
//! headings (bold), then its rows. A cell that is a plain number goes in as a number, so a
//! spreadsheet sums and sorts it; anything else (a value with its unit, a text) as text.
//!
//! An `.xlsx` is a zip of XML parts; only what a workbook of plain tables needs is written.

use crate::model::Schedule;
use std::io::{self, Write};

/// The workbook of `schedules`, in their order.
pub fn workbook(schedules: &[&Schedule]) -> io::Result<Vec<u8>> {
    let names = sheet_names(schedules);
    let mut out = io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let options = zip::write::SimpleFileOptions::default();
    let mut part = |name: &str, xml: String| -> io::Result<()> {
        zip.start_file(name, options).map_err(io::Error::other)?;
        zip.write_all(xml.as_bytes())
    };
    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
    let sheets = |each: &dyn Fn(usize) -> String| (1..=names.len()).map(each).collect::<String>();
    part(
        "[Content_Types].xml",
        format!(
            r#"{HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>{}</Types>"#,
            sheets(&|k| format!(r#"<Override PartName="/xl/worksheets/sheet{k}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#))
        ),
    )?;
    part(
        "_rels/.rels",
        format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#),
    )?;
    part(
        "xl/workbook.xml",
        format!(
            r#"{HEAD}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{}</sheets></workbook>"#,
            names.iter().enumerate().map(|(k, name)| format!(r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#, escape(name), k + 1, k + 1)).collect::<String>()
        ),
    )?;
    part(
        "xl/_rels/workbook.xml.rels",
        format!(
            r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
            sheets(&|k| format!(r#"<Relationship Id="rId{k}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{k}.xml"/>"#)),
            names.len() + 1
        ),
    )?;
    // two cell formats: plain, and bold (headings, the title and totals)
    part(
        "xl/styles.xml",
        format!(
            r#"{HEAD}<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs></styleSheet>"#
        ),
    )?;
    for (k, s) in schedules.iter().enumerate() {
        part(&format!("xl/worksheets/sheet{}.xml", k + 1), format!("{HEAD}{}", sheet(s)))?;
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(out.into_inner())
}

/// A name for each schedule's sheet: at most 31 letters, without the marks a sheet name cannot
/// have, no two alike.
fn sheet_names(schedules: &[&Schedule]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (k, s) in schedules.iter().enumerate() {
        let clean: String = s.name.chars().map(|c| if "[]:*?/\\".contains(c) { ' ' } else { c }).collect();
        let clean = clean.trim().trim_matches('\'').to_string();
        let base: String = if clean.is_empty() { format!("Schedule {}", k + 1) } else { clean.chars().take(31).collect() };
        let mut name = base.clone();
        let mut n = 2;
        while names.iter().any(|other| other.eq_ignore_ascii_case(&name)) {
            let tail = format!(" ({n})");
            name = base.chars().take(31 - tail.chars().count()).collect::<String>() + &tail;
            n += 1;
        }
        names.push(name);
    }
    names
}

/// A schedule's sheet: its title, its headings and its rows.
fn sheet(s: &Schedule) -> String {
    let mut rows = String::new();
    let mut row = |number: usize, cells: &mut dyn Iterator<Item = (Option<&str>, bool)>| {
        rows.push_str(&format!(r#"<row r="{number}">"#));
        for (column, (value, bold)) in cells.enumerate() {
            let Some(value) = value.filter(|v| !v.is_empty()) else { continue };
            let at = format!("{}{number}", column_letters(column));
            let style = if bold { r#" s="1""# } else { "" };
            match number_of(value) {
                Some(n) => rows.push_str(&format!(r#"<c r="{at}"{style}><v>{n}</v></c>"#)),
                None => rows.push_str(&format!(r#"<c r="{at}"{style} t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, escape(value))),
            }
        }
        rows.push_str("</row>");
    };
    row(1, &mut std::iter::once((Some(s.title.as_str()), true)));
    row(2, &mut s.columns.iter().map(|c| (Some(c.as_str()), true)));
    for (k, r) in s.rows.iter().enumerate() {
        row(k + 3, &mut r.cells.iter().map(|c| (c.as_deref(), r.total)));
    }
    // as wide as its longest value, within reason
    let width = |column: usize| {
        let longest = s.rows.iter().filter_map(|r| r.cells.get(column)?.as_deref()).chain(s.columns.get(column).map(String::as_str)).map(|v| v.chars().count()).max().unwrap_or(0);
        (longest + 2).clamp(8, 60)
    };
    let columns: String = (0..s.columns.len()).map(|k| format!(r#"<col min="{0}" max="{0}" width="{1}" customWidth="1"/>"#, k + 1, width(k))).collect();
    let columns = if columns.is_empty() { String::new() } else { format!("<cols>{columns}</cols>") };
    // the headings stay in sight
    format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="2" topLeftCell="A3" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>{columns}<sheetData>{rows}</sheetData></worksheet>"#
    )
}

/// A column's letters: A, B, … Z, AA, AB, …
fn column_letters(mut column: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (column % 26) as u8);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// The number a cell is, when it is a plain one: digits with a point or a comma for its
/// decimals. A number with leading zeros (a mark, `007`) or a unit stays a text.
fn number_of(value: &str) -> Option<f64> {
    let plain = value.trim();
    let digits = plain.strip_prefix('-').unwrap_or(plain);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') || digits.chars().filter(|c| !c.is_ascii_digit()).count() > 1 {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') && !digits[1..].starts_with(['.', ',']) {
        return None;
    }
    if !digits.starts_with(|c: char| c.is_ascii_digit()) || !digits.ends_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    plain.replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite())
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // what XML cannot hold
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScheduleRow;
    use std::io::Read;

    fn schedule(name: &str) -> Schedule {
        let row = |cells: &[&str], total: bool| ScheduleRow { images: Vec::new(), cells: cells.iter().map(|c| (!c.is_empty()).then(|| c.to_string())).collect(), elements: Vec::new(), total };
        Schedule { name: name.into(), title: "Doors <A & B>".into(), columns: vec!["Mark".into(), "Width".into(), "Area".into()], rows: vec![row(&["007", "930", "1,95"], false), row(&["D2", "", "2.10 m²"], false), row(&["", "", "4,05"], true)], computed: true }
    }

    #[test]
    fn numbers_are_numbers_and_marks_stay_texts() {
        assert_eq!(number_of("930"), Some(930.0));
        assert_eq!(number_of("-1,95"), Some(-1.95));
        assert_eq!(number_of("0.5"), Some(0.5));
        assert_eq!(number_of("0"), Some(0.0));
        for text in ["007", "2.10 m²", "1.234,5", "D2", "", "-", "12.", ".5", "1e5"] {
            assert_eq!(number_of(text), None, "{text:?}");
        }
        assert_eq!([0, 25, 26, 27, 701, 702].map(column_letters), ["A", "Z", "AA", "AB", "ZZ", "AAA"].map(String::from));
    }

    #[test]
    fn a_workbook_has_a_sheet_per_schedule() {
        let (a, b) = (schedule("Doors: all"), schedule("doors  all"));
        let bytes = workbook(&[&a, &b, &a]).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
        let mut read = |name: &str| {
            let mut text = String::new();
            zip.by_name(name).unwrap_or_else(|e| panic!("{name}: {e}")).read_to_string(&mut text).unwrap();
            text
        };
        let book = read("xl/workbook.xml");
        assert!(book.contains(r#"name="Doors  all""#) && book.contains(r#"name="doors  all (2)""#) && book.contains(r#"name="Doors  all (3)""#), "{book}");
        let sheet = read("xl/worksheets/sheet1.xml");
        assert!(sheet.contains("Doors &lt;A &amp; B&gt;"), "{sheet}");
        // the mark a text, the width and the area numbers, the area with its unit a text, the total bold
        assert!(sheet.contains(r#"<c r="A3" t="inlineStr"><is><t xml:space="preserve">007</t></is></c><c r="B3"><v>930</v></c><c r="C3"><v>1.95</v></c>"#), "{sheet}");
        assert!(sheet.contains(r#"<c r="C4" t="inlineStr"><is><t xml:space="preserve">2.10 m²</t>"#) && sheet.contains(r#"<c r="C5" s="1"><v>4.05</v></c>"#), "{sheet}");
        assert!(read("[Content_Types].xml").contains("sheet3.xml") && read("xl/_rels/workbook.xml.rels").contains(r#"Id="rId4""#));
        // every part is well-formed
        for name in ["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels", "xl/styles.xml", "xl/worksheets/sheet2.xml"] {
            let text = read(name);
            roxmltree::Document::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
