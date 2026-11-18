//! Ground investigation: cone penetration tests as their files give them.
//!
//! A sounding is read from two kinds of text:
//!
//! - a GEF file, the exchange format Dutch soil investigation firms deliver (a header of
//!   `#KEY= values` lines up to `#EOH=`, then a row of numbers per measurement);
//! - the XML the Dutch register of the subsurface (BRO) gives for one test.
//!
//! What is read is what was measured: the depth, the cone resistance, the local friction and
//! the friction ratio. Nothing is derived but a missing friction ratio, which is the friction
//! over the cone resistance. No soil is named here.

/// One measurement of a sounding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Below the ground (m).
    pub depth: f64,
    /// The cone resistance (MPa).
    pub cone: f64,
    /// The local friction (MPa).
    pub friction: Option<f64>,
    /// The friction ratio (%).
    pub ratio: Option<f64>,
}

/// A cone penetration test.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sounding {
    pub name: String,
    /// Where it was made, in the Dutch national grid (m east, m north), when the file says so.
    pub place: Option<[f64; 2]>,
    /// The height of the ground there above NAP (m), when the file says so.
    pub ground: Option<f64>,
    /// The day it was made, as the file writes it.
    pub date: Option<String>,
    /// The measurements, from the ground down.
    pub rows: Vec<Row>,
}

impl Sounding {
    /// How deep it went (m).
    pub fn depth(&self) -> f64 {
        self.rows.last().map_or(0.0, |row| row.depth)
    }

    /// The highest cone resistance measured (MPa).
    pub fn highest(&self) -> f64 {
        self.rows.iter().map(|row| row.cone).fold(0.0, f64::max)
    }

    /// The mean cone resistance per step of `step` metres of depth: (from, to, mean), for the
    /// steps that hold a measurement.
    pub fn steps(&self, step: f64) -> Vec<(f64, f64, f64)> {
        let mut out: Vec<(f64, f64, f64, usize)> = Vec::new();
        for row in &self.rows {
            let at = (row.depth / step).floor().max(0.0);
            match out.last_mut() {
                Some(last) if last.0 == at * step => {
                    last.2 += row.cone;
                    last.3 += 1;
                }
                _ => out.push((at * step, (at + 1.0) * step, row.cone, 1)),
            }
        }
        out.into_iter().map(|(from, to, sum, count)| (from, to, sum / count as f64)).collect()
    }

    /// Sorted by depth, the friction ratio filled in where the friction is known and it is not.
    fn finished(mut self) -> Result<Self, String> {
        self.rows.retain(|row| row.depth.is_finite() && row.cone.is_finite() && row.depth >= 0.0);
        if self.rows.is_empty() {
            return Err(format!("{}: no measurements found", self.name));
        }
        self.rows.sort_by(|a, b| a.depth.total_cmp(&b.depth));
        for row in &mut self.rows {
            if row.ratio.is_none() && row.cone > 0.0 {
                row.ratio = row.friction.map(|friction| friction / row.cone * 100.0);
            }
        }
        Ok(self)
    }
}

/// The national grid holds the Netherlands in these limits: a place outside them is not in it.
fn in_grid(x: f64, y: f64) -> bool {
    (-7_000.0..=300_000.0).contains(&x) && (289_000.0..=629_000.0).contains(&y)
}

/// A sounding of a GEF file. `file` names it where the file itself does not.
pub fn gef(text: &str, file: &str) -> Result<Sounding, String> {
    let mut sounding = Sounding { name: file.to_string(), ..Sounding::default() };
    // the column of each quantity, and the number that stands for "not measured" per column
    let (mut length, mut corrected, mut cone, mut friction, mut ratio) = (None, None, None, None, None);
    let mut voids: Vec<(usize, f64)> = Vec::new();
    let (mut column_mark, mut record_mark) = (None, None);
    let mut report = String::new();
    let mut lines = text.lines();
    let mut ended = false;
    for line in lines.by_ref() {
        let Some(rest) = line.trim().strip_prefix('#') else { continue };
        let (key, value) = rest.split_once('=').unwrap_or((rest, ""));
        let values: Vec<&str> = value.split(',').map(str::trim).collect();
        let number = |at: usize| values.get(at).and_then(|v| v.parse::<f64>().ok());
        match key.trim().to_ascii_uppercase().as_str() {
            "EOH" => {
                ended = true;
                break;
            }
            "TESTID" if !value.trim().is_empty() => sounding.name = value.trim().to_string(),
            "REPORTCODE" | "PROCEDURECODE" => report.push_str(&value.to_ascii_uppercase()),
            "XYID" => sounding.place = number(1).zip(number(2)).filter(|(x, y)| in_grid(*x, *y)).map(|(x, y)| [x, y]),
            "ZID" => sounding.ground = number(1),
            "STARTDATE" => {
                if let (Some(y), Some(m), Some(d)) = (number(0), number(1), number(2)) {
                    sounding.date = Some(format!("{y:04.0}-{m:02.0}-{d:02.0}"));
                }
            }
            "COLUMNINFO" => {
                // the column, its unit, its name, and the number of the quantity
                let (Some(column), Some(quantity)) = (number(0), number(3)) else { continue };
                let slot = match quantity as i64 {
                    1 => &mut length,
                    2 => &mut cone,
                    3 => &mut friction,
                    4 => &mut ratio,
                    11 => &mut corrected,
                    _ => continue,
                };
                *slot = Some(column as usize);
            }
            "COLUMNVOID" => voids.extend(number(0).zip(number(1)).map(|(column, value)| (column as usize, value))),
            "COLUMNSEPARATOR" => column_mark = value.trim().chars().next(),
            "RECORDSEPARATOR" => record_mark = value.trim().chars().next(),
            _ => {}
        }
    }
    if !ended {
        return Err(format!("{file}: not a GEF file (no end of the header)"));
    }
    if report.contains("BORE") {
        return Err(format!("{file}: a boring, not a sounding"));
    }
    let (Some(depth), Some(cone)) = (corrected.or(length), cone) else { return Err(format!("{file}: no column with the depth and one with the cone resistance")) };
    for line in lines {
        let numbers: Vec<f64> = line
            .split(|c: char| c.is_whitespace() || Some(c) == column_mark || Some(c) == record_mark)
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect();
        // a column counts from 1; a number that stands for "not measured" is none
        let at = |column: usize| numbers.get(column.checked_sub(1)?).copied().filter(|value| !voids.iter().any(|(c, void)| *c == column && (value - void).abs() < 1e-9));
        let (Some(depth), Some(cone)) = (at(depth), at(cone)) else { continue };
        sounding.rows.push(Row { depth: depth.abs(), cone, friction: friction.and_then(at), ratio: ratio.and_then(at) });
    }
    sounding.finished()
}

/// What stands between `<…name …>` and its closing tag, whatever the prefix of the name; the
/// search starts at `from`.
fn inner<'a>(xml: &'a str, name: &str, from: usize) -> Option<(&'a str, usize)> {
    let mut at = from;
    loop {
        let found = at + xml.get(at..)?.find(name)?;
        let after = found + name.len();
        let opens = found > 0 && matches!(xml.as_bytes()[found - 1], b'<' | b':') && matches!(xml.as_bytes().get(after), Some(b'>' | b' '));
        // not the closing tag: `</name>` or `</prefix:name>`
        let closing = xml[..found].rfind('<').is_some_and(|open| xml.as_bytes().get(open + 1) == Some(&b'/'));
        if opens && !closing {
            let start = after + xml[after..].find('>')? + 1;
            let end = start + xml[start..].find('<')?;
            return Some((&xml[start..end], end));
        }
        at = after;
    }
}

/// The value the register writes where nothing was measured.
const VOID: f64 = -999_999.0;

/// A sounding of the XML the register of the subsurface gives for one test.
pub fn bro(xml: &str) -> Result<Sounding, String> {
    let name = inner(xml, "broId", 0).map(|(id, _)| id.trim().to_string()).ok_or("the register answered without a test")?;
    let mut sounding = Sounding { name, ..Sounding::default() };
    if let Some((_, at)) = inner(xml, "deliveredLocation", 0).or_else(|| xml.find("deliveredLocation").map(|at| ("", at))) {
        sounding.place = inner(xml, "pos", at).and_then(|(pos, _)| {
            let mut numbers = pos.split_whitespace().map(str::parse::<f64>);
            Some([numbers.next()?.ok()?, numbers.next()?.ok()?])
        }).filter(|[x, y]| in_grid(*x, *y));
    }
    sounding.ground = inner(xml, "offset", 0).and_then(|(offset, _)| offset.trim().parse().ok());
    if let Some(at) = xml.find("conePenetrationTest") {
        sounding.date = inner(xml, "timePosition", at).map(|(day, _)| day.trim().to_string());
    }
    let (values, _) = inner(xml, "values", 0).ok_or_else(|| format!("{}: no measurements in the answer of the register", sounding.name))?;
    // a record: penetration length, depth, time, cone resistance, …, local friction (19th), …,
    // friction ratio (25th), as the register's catalogue of the test orders them
    for record in values.split(';') {
        let numbers: Vec<Option<f64>> = record.split(',').map(|part| part.trim().parse::<f64>().ok().filter(|value| *value != VOID)).collect();
        let at = |column: usize| numbers.get(column).copied().flatten();
        let (Some(depth), Some(cone)) = (at(1).or(at(0)), at(3)) else { continue };
        sounding.rows.push(Row { depth, cone, friction: at(18), ratio: at(24) });
    }
    sounding.finished()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEF: &str = "#GEFID= 1, 1, 0\n#COLUMN= 4\n#COLUMNINFO= 1, m, Sondeerlengte, 1\n#COLUMNINFO= 2, MPa, Conusweerstand, 2\n#COLUMNINFO= 3, MPa, Wrijvingsweerstand, 3\n#COLUMNINFO= 4, %, Wrijvingsgetal, 4\n#COLUMNVOID= 3, -9999.99\n#COLUMNVOID= 4, -9999.99\n#REPORTCODE= GEF-CPT-Report, 1, 1, 2\n#STARTDATE= 2020, 3, 9\n#TESTID= S01\n#XYID= 31000, 154652.00, 463125.00, 0.01, 0.01\n#ZID= 31000, 4.07, 0.01\n#EOH=\n0.02 0.50 0.010 2.0\n0.04 1.00 0.020 2.0\n0.06 2.00 -9999.99 -9999.99\n0.50 12.00 0.060 -9999.99\n";

    #[test]
    fn a_gef_file_gives_its_place_ground_day_and_measurements() {
        let sounding = gef(GEF, "s01.gef").unwrap();
        assert_eq!(sounding.name, "S01");
        assert_eq!(sounding.place, Some([154_652.0, 463_125.0]));
        assert_eq!(sounding.ground, Some(4.07));
        assert_eq!(sounding.date.as_deref(), Some("2020-03-09"));
        assert_eq!(sounding.rows.len(), 4);
        assert_eq!(sounding.rows[0], Row { depth: 0.02, cone: 0.5, friction: Some(0.01), ratio: Some(2.0) });
        // what the file marks as not measured is none
        assert_eq!(sounding.rows[2], Row { depth: 0.06, cone: 2.0, friction: None, ratio: None });
        // a missing friction ratio is the friction over the cone resistance: 0.06 / 12 = 0.5 %
        assert!((sounding.rows[3].ratio.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!((sounding.depth(), sounding.highest()), (0.5, 12.0));
    }

    #[test]
    fn a_gef_file_with_its_own_separators_and_a_corrected_depth_is_read() {
        let text = "#COLUMNINFO= 1, m, Sondeerlengte, 1\n#COLUMNINFO= 2, MPa, qc, 2\n#COLUMNINFO= 3, m, Diepte, 11\n#COLUMNSEPARATOR= ;\n#RECORDSEPARATOR= !\n#EOH=\n1.00;3.5;0.98;!\n2.00;7.0;1.95;!\n";
        let sounding = gef(text, "a.gef").unwrap();
        assert_eq!(sounding.name, "a.gef");
        assert_eq!((sounding.place, sounding.ground), (None, None));
        // the corrected depth goes before the penetration length
        assert_eq!(sounding.rows.iter().map(|row| (row.depth, row.cone)).collect::<Vec<_>>(), [(0.98, 3.5), (1.95, 7.0)]);
    }

    #[test]
    fn what_is_no_sounding_says_why() {
        assert!(gef("just text", "x.gef").unwrap_err().contains("not a GEF file"));
        assert!(gef("#REPORTCODE= GEF-BORE-Report, 1, 0, 0\n#EOH=\n", "b.gef").unwrap_err().contains("a boring"));
        assert!(gef("#COLUMNINFO= 1, m, Sondeerlengte, 1\n#EOH=\n1.0\n", "c.gef").unwrap_err().contains("no column"));
        assert!(gef("#COLUMNINFO= 1, m, l, 1\n#COLUMNINFO= 2, MPa, qc, 2\n#EOH=\n", "d.gef").unwrap_err().contains("no measurements"));
        // a place outside the national grid is no place in it
        let abroad = gef("#COLUMNINFO= 1, m, l, 1\n#COLUMNINFO= 2, MPa, qc, 2\n#XYID= 0, 10.0, 20.0\n#EOH=\n1 2\n", "e.gef").unwrap();
        assert_eq!(abroad.place, None);
    }

    #[test]
    fn the_answer_of_the_register_gives_a_sounding() {
        let sounding = bro(include_str!("../fixtures/bro-cpt.xml")).unwrap();
        assert_eq!(sounding.name, "CPT000000000785");
        assert_eq!(sounding.place, Some([154_652.0, 463_125.0]));
        assert_eq!(sounding.ground, Some(4.07));
        assert_eq!(sounding.date.as_deref(), Some("2002-09-17"));
        // the records come in any order and are sorted by depth
        assert_eq!(sounding.rows.len(), 8);
        assert_eq!(sounding.rows[0], Row { depth: 0.0, cone: 0.012, friction: None, ratio: None });
        assert_eq!(sounding.rows[5], Row { depth: 8.18, cone: 36.386, friction: Some(0.542), ratio: Some(1.4) });
        assert_eq!(sounding.depth(), 8.22);
        assert!(bro("<a/>").is_err());
    }

    #[test]
    fn the_cone_resistance_is_averaged_per_step_of_depth() {
        let sounding = gef(GEF, "s01.gef").unwrap();
        let steps = sounding.steps(0.25);
        // 0 to 0.25 m holds three measurements (0.5, 1 and 2 MPa), 0.5 to 0.75 m one; the step between has none
        assert_eq!(steps.len(), 2);
        assert_eq!((steps[0].0, steps[0].1), (0.0, 0.25));
        assert!((steps[0].2 - 3.5 / 3.0).abs() < 1e-9);
        assert_eq!(steps[1], (0.5, 0.75, 12.0));
    }
}
