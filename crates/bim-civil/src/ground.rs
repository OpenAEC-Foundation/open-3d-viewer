//! Ground investigation: cone penetration tests and borings as their files give them.
//!
//! A sounding is read from two kinds of text:
//!
//! - a GEF file, the exchange format Dutch soil investigation firms deliver (a header of
//!   `#KEY= values` lines up to `#EOH=`, then a row of numbers per measurement);
//! - the XML the Dutch register of the subsurface (BRO) gives for one test.
//!
//! What is read is what was measured: the depth, the cone resistance, the local friction and
//! the friction ratio. Nothing is derived but a missing friction ratio, which is the friction
//! over the cone resistance. No soil is named from a sounding.
//!
//! A boring is read the same two ways, with the layers described in it: each its top, its bottom
//! and its name in the words of the description. The main soil is read from that name.

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

/// The main soil of a described layer, read from its name: what its column is coloured by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Soil {
    Gravel,
    Sand,
    Silt,
    Clay,
    Peat,
    /// Anything else the description names: rubble, paving, a name not known here.
    Other,
}

impl Soil {
    pub const ALL: [Soil; 6] = [Soil::Gravel, Soil::Sand, Silt, Soil::Clay, Soil::Peat, Soil::Other];

    /// The main soil of a name as the register writes it (`zwakGrindigZand`: the last word is
    /// the soil, the words before it say what is mixed in) or of a code of a boring's GEF file
    /// (`Zs1g1`: the first letter is the soil).
    pub fn of(name: &str) -> Soil {
        let name = name.trim();
        // the last word of a name written as one: from its last capital on
        let last = name.rfind(|c: char| c.is_uppercase()).map_or(name, |at| &name[at..]).to_lowercase();
        let word = |soil: &str| last.starts_with(soil);
        if word("zand") {
            Soil::Sand
        } else if word("klei") {
            Soil::Clay
        } else if word("veen") {
            Soil::Peat
        } else if word("grind") {
            Soil::Gravel
        } else if word("silt") || word("leem") {
            Soil::Silt
        } else if name.len() <= 8 && !name.contains(|c: char| c.is_lowercase() && !"sgzkhl".contains(c)) {
            // a code: its first letter
            match name.chars().next() {
                Some('Z') => Soil::Sand,
                Some('K') => Soil::Clay,
                Some('V') => Soil::Peat,
                Some('G') => Soil::Gravel,
                Some('L') => Soil::Silt,
                _ => Soil::Other,
            }
        } else {
            Soil::Other
        }
    }
}

use Soil::Silt;

/// A described layer of a boring.
#[derive(Debug, Clone, PartialEq)]
pub struct Stratum {
    /// Its top and bottom below the ground (m).
    pub top: f64,
    pub bottom: f64,
    /// What it is, in the words of the description.
    pub name: String,
}

impl Stratum {
    pub fn soil(&self) -> Soil {
        Soil::of(&self.name)
    }
}

/// A boring with the layers described in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Boring {
    pub name: String,
    /// Where it was made, in the Dutch national grid (m east, m north), when the file says so.
    pub place: Option<[f64; 2]>,
    /// The height of the ground there above NAP (m), when the file says so.
    pub ground: Option<f64>,
    pub date: Option<String>,
    /// The layers, from the ground down.
    pub layers: Vec<Stratum>,
}

impl Boring {
    /// How deep it went (m).
    pub fn depth(&self) -> f64 {
        self.layers.iter().map(|layer| layer.bottom).fold(0.0, f64::max)
    }

    fn finished(mut self) -> Result<Self, String> {
        self.layers.retain(|layer| layer.top.is_finite() && layer.bottom.is_finite() && layer.bottom > layer.top && layer.top >= 0.0);
        if self.layers.is_empty() {
            return Err(format!("{}: no described layers found", self.name));
        }
        self.layers.sort_by(|a, b| a.top.total_cmp(&b.top));
        Ok(self)
    }
}

/// A boring of the XML the register of the subsurface gives for one geotechnical boring.
pub fn boring_bro(xml: &str) -> Result<Boring, String> {
    let name = inner(xml, "broId", 0).map(|(id, _)| id.trim().to_string()).ok_or("the register answered without a boring")?;
    let mut boring = Boring { name, ..Boring::default() };
    if let Some(at) = xml.find("<deliveredLocation") {
        boring.place = inner(xml, "pos", at).and_then(|(pos, _)| {
            let mut numbers = pos.split_whitespace().map(str::parse::<f64>);
            Some([numbers.next()?.ok()?, numbers.next()?.ok()?])
        }).filter(|[x, y]| in_grid(*x, *y));
    }
    boring.ground = inner(xml, "offset", 0).and_then(|(offset, _)| offset.trim().parse().ok());
    if let Some(at) = xml.find("boringStartDate") {
        boring.date = inner(xml, "date", at).map(|(day, _)| day.trim().to_string());
    }
    // every other piece between the tags of a layer is a layer
    for layer in xml.split(":layer>").skip(1).step_by(2) {
        let number = |name: &str| inner(layer, name, 0).and_then(|(value, _)| value.trim().parse::<f64>().ok());
        let (Some(top), Some(bottom)) = (number("upperBoundary"), number("lowerBoundary")) else { continue };
        // the soil, or the material where it is no soil (paving, rubble)
        let name = inner(layer, "geotechnicalSoilName", 0).or_else(|| inner(layer, "specialMaterial", 0)).map_or(String::new(), |(name, _)| name.trim().to_string());
        boring.layers.push(Stratum { top, bottom, name });
    }
    boring.finished()
}

/// A boring of a GEF file: per row the top and bottom of a layer and, in quotes, its soil code.
pub fn boring_gef(text: &str, file: &str) -> Result<Boring, String> {
    let mut boring = Boring { name: file.to_string(), ..Boring::default() };
    let (mut column_mark, mut record_mark) = (';', '!');
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
            "TESTID" if !value.trim().is_empty() => boring.name = value.trim().to_string(),
            "REPORTCODE" | "PROCEDURECODE" => report.push_str(&value.to_ascii_uppercase()),
            "XYID" => boring.place = number(1).zip(number(2)).filter(|(x, y)| in_grid(*x, *y)).map(|(x, y)| [x, y]),
            "ZID" => boring.ground = number(1),
            "STARTDATE" => {
                if let (Some(y), Some(m), Some(d)) = (number(0), number(1), number(2)) {
                    boring.date = Some(format!("{y:04.0}-{m:02.0}-{d:02.0}"));
                }
            }
            "COLUMNSEPARATOR" => column_mark = value.trim().chars().next().unwrap_or(column_mark),
            "RECORDSEPARATOR" => record_mark = value.trim().chars().next().unwrap_or(record_mark),
            _ => {}
        }
    }
    if !ended {
        return Err(format!("{file}: not a GEF file (no end of the header)"));
    }
    if !report.contains("BORE") {
        return Err(format!("{file}: not a boring"));
    }
    for line in lines {
        let parts: Vec<&str> = line.split(|c: char| c == column_mark || c == record_mark).map(str::trim).filter(|part| !part.is_empty()).collect();
        // without a column separator in the row: numbers and quoted codes parted by spaces
        let parts: Vec<&str> = if parts.len() < 2 { line.split_whitespace().collect() } else { parts };
        let mut numbers = parts.iter().filter_map(|part| part.parse::<f64>().ok());
        let (Some(top), Some(bottom)) = (numbers.next(), numbers.next()) else { continue };
        let name = parts.iter().find(|part| part.starts_with('\'')).map_or(String::new(), |part| part.trim_matches('\'').trim().to_string());
        boring.layers.push(Stratum { top, bottom, name });
    }
    boring.finished()
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
    fn the_main_soil_is_read_from_a_name_or_a_code() {
        assert_eq!(Soil::of("zwakGrindigZand"), Soil::Sand);
        assert_eq!(Soil::of("zand"), Soil::Sand);
        assert_eq!(Soil::of("sterkZandigeKlei"), Soil::Clay);
        assert_eq!(Soil::of("mineraalarmVeen"), Soil::Peat);
        assert_eq!(Soil::of("siltigGrind"), Soil::Gravel);
        assert_eq!(Soil::of("zwakZandigSilt"), Soil::Silt);
        // what is no soil, or not known here
        assert_eq!(Soil::of("puin"), Soil::Other);
        assert_eq!(Soil::of("wegverhardingsmateriaal"), Soil::Other);
        assert_eq!(Soil::of(""), Soil::Other);
        // the codes of a boring's GEF file: the first letter
        assert_eq!((Soil::of("Zs1g1"), Soil::of("Ks2h1"), Soil::of("Vk1"), Soil::of("Gz2"), Soil::of("Lz1"), Soil::of("NBE")), (Soil::Sand, Soil::Clay, Soil::Peat, Soil::Gravel, Soil::Silt, Soil::Other));
    }

    #[test]
    fn the_answer_of_the_register_gives_a_boring() {
        let boring = boring_bro(include_str!("../fixtures/bro-boring.xml")).unwrap();
        assert_eq!(boring.name, "BHR000000361212");
        assert_eq!(boring.place, Some([155_000.0, 463_000.0]));
        assert_eq!(boring.ground, Some(3.78));
        assert_eq!(boring.date.as_deref(), Some("2022-01-19"));
        assert_eq!(boring.layers.len(), 8);
        // paving has no soil name: its material is its name
        assert_eq!(boring.layers[0], Stratum { top: 0.0, bottom: 0.1, name: "wegverhardingsmateriaal".into() });
        assert_eq!(boring.layers[1], Stratum { top: 0.1, bottom: 1.1, name: "zwakGrindigZand".into() });
        assert_eq!(boring.layers[1].soil(), Soil::Sand);
        assert_eq!(boring.depth(), 5.0);
        assert!(boring_bro("<a/>").is_err());
    }

    #[test]
    fn a_gef_file_of_a_boring_gives_its_layers() {
        let text = "#REPORTCODE= GEF-BORE-Report, 1, 0, 0\n#TESTID= B12\n#XYID= 31000, 154652.00, 463125.00\n#ZID= 31000, 4.07\n#COLUMNSEPARATOR= ;\n#RECORDSEPARATOR= !\n#EOH=\n0.00;1.20;'Zs1';'ON';!\n1.20;2.50;'Ks2h1';!\n2.50;2.50;'Vk1';!\n";
        let boring = boring_gef(text, "b12.gef").unwrap();
        assert_eq!((boring.name.as_str(), boring.place, boring.ground), ("B12", Some([154_652.0, 463_125.0]), Some(4.07)));
        // a layer without thickness is none
        assert_eq!(boring.layers, [Stratum { top: 0.0, bottom: 1.2, name: "Zs1".into() }, Stratum { top: 1.2, bottom: 2.5, name: "Ks2h1".into() }]);
        assert_eq!((boring.layers[0].soil(), boring.layers[1].soil()), (Soil::Sand, Soil::Clay));
        // a sounding is no boring, and a boring no sounding
        assert!(boring_gef(GEF, "s01.gef").unwrap_err().contains("not a boring"));
        assert!(gef(text, "b12.gef").unwrap_err().contains("a boring"));
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
