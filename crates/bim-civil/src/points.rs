//! Survey points as surveyors' files give them: one point a line, with its number or name, its
//! coordinates, its height and often a code that says what was measured.
//!
//! The columns are parted by commas, semicolons, tabs or spaces. With semicolons or tabs a
//! number may have a decimal comma. Lines that are no point (a heading, a comment, a point
//! without a height) are counted and left out.

/// A measured point.
#[derive(Debug, Clone, PartialEq)]
pub struct SurveyPoint {
    /// Its number or name; empty when the file has none.
    pub name: String,
    /// East (m).
    pub x: f64,
    /// North (m).
    pub y: f64,
    /// Height (m).
    pub z: f64,
    /// What was measured, in the surveyor's code; empty when the file has none.
    pub code: String,
}

/// Which of the two coordinates of a line comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// East then north, unless the numbers are plainly north then east: both within the Dutch
    /// national grid only when read that way.
    Auto,
    EastNorth,
    NorthEast,
}

/// The points of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub points: Vec<SurveyPoint>,
    /// The lines that are no point.
    pub skipped: usize,
    /// The order the coordinates were read in.
    pub order: Order,
}

/// What parts the columns of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Separator {
    Semicolon,
    Tab,
    Comma,
    Space,
}

fn separator(text: &str) -> Separator {
    let has = |c: char| text.lines().filter(|line| !comment(line)).take(50).any(|line| line.contains(c));
    if has(';') {
        Separator::Semicolon
    } else if has('\t') {
        Separator::Tab
    } else if has(',') {
        Separator::Comma
    } else {
        Separator::Space
    }
}

fn comment(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with('#') || line.starts_with("//")
}

/// A number of a column; a decimal comma counts where commas do not part the columns.
fn number(field: &str, decimal_comma: bool) -> Option<f64> {
    let field = field.trim();
    let value = if decimal_comma { field.replace(',', ".").parse::<f64>() } else { field.parse::<f64>() };
    value.ok().filter(|v| v.is_finite())
}

/// A line as its name, its two coordinates in file order, its height and its code.
fn fields(line: &str, separator: Separator) -> Option<(String, f64, f64, f64, String)> {
    let parts: Vec<&str> = match separator {
        Separator::Semicolon => line.split(';').collect(),
        Separator::Tab => line.split('\t').collect(),
        Separator::Comma => line.split(',').collect(),
        Separator::Space => line.split_whitespace().collect(),
    };
    let parts: Vec<&str> = parts.into_iter().map(str::trim).collect();
    let decimal_comma = matches!(separator, Separator::Semicolon | Separator::Tab);
    let num = |i: usize| parts.get(i).and_then(|p| number(p, decimal_comma));
    let text = |i: usize| parts.get(i).map(|p| p.trim_matches('"').to_string()).unwrap_or_default();
    // where the three numbers start: after a name, unless the line is three numbers, or three
    // numbers and a code
    let named = match parts.len() {
        0..=2 => return None,
        3 => false,
        4 => num(3).is_some() || num(0).is_none(),
        _ => true,
    };
    let at = usize::from(named);
    let (a, b, z) = (num(at)?, num(at + 1)?, num(at + 2)?);
    let code = parts[at + 3..].iter().map(|p| p.trim_matches('"')).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    Some((if named { text(0) } else { String::new() }, a, b, z, code))
}

/// Whether a pair lies within the Dutch national grid as east, north.
fn in_grid(east: f64, north: f64) -> bool {
    (-7_000.0..=300_000.0).contains(&east) && (289_000.0..=629_000.0).contains(&north)
}

/// The points of a file's text.
pub fn parse(text: &str, order: Order) -> Parsed {
    let separator = separator(text);
    let mut rows = Vec::new();
    let mut skipped = 0;
    for line in text.lines() {
        if comment(line) {
            continue;
        }
        match fields(line, separator) {
            Some(row) => rows.push(row),
            None => skipped += 1,
        }
    }
    let order = match order {
        Order::Auto if !rows.is_empty() && rows.iter().all(|(_, a, b, _, _)| in_grid(*b, *a) && !in_grid(*a, *b)) => Order::NorthEast,
        Order::Auto => Order::EastNorth,
        given => given,
    };
    let points = rows
        .into_iter()
        .map(|(name, a, b, z, code)| {
            let (x, y) = if order == Order::NorthEast { (b, a) } else { (a, b) };
            SurveyPoint { name, x, y, z, code }
        })
        .collect();
    Parsed { points, skipped, order }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_of_numbered_points_with_codes_is_read_with_its_heading_left_out() {
        let parsed = parse("Point,Easting,Northing,Elevation,Code\n1,1000.5,2000.25,12.3,TOPO\n2,1010,2000,12.9,EDGE OF ROAD\n\n# a remark\n3,1020,2000,,TOPO\n", Order::Auto);
        assert_eq!((parsed.points.len(), parsed.skipped, parsed.order), (2, 2, Order::EastNorth));
        assert_eq!(parsed.points[0], SurveyPoint { name: "1".into(), x: 1000.5, y: 2000.25, z: 12.3, code: "TOPO".into() });
        assert_eq!(parsed.points[1].code, "EDGE OF ROAD");
    }

    #[test]
    fn three_numbers_a_line_are_a_point_without_a_name() {
        let parsed = parse("155000.00 463000.00 1.25\n155001.00 463000.50 1.31\n", Order::Auto);
        assert_eq!(parsed.points, vec![
            SurveyPoint { name: String::new(), x: 155_000.0, y: 463_000.0, z: 1.25, code: String::new() },
            SurveyPoint { name: String::new(), x: 155_001.0, y: 463_000.5, z: 1.31, code: String::new() },
        ]);
        // three numbers and a code, and a number in front of three numbers
        let coded = parse("10,20,1.5,KERB\n", Order::Auto);
        assert_eq!((coded.points[0].name.as_str(), coded.points[0].x, coded.points[0].code.as_str()), ("", 10.0, "KERB"));
        let numbered = parse("7,10,20,1.5\n", Order::Auto);
        assert_eq!((numbered.points[0].name.as_str(), numbered.points[0].x, numbered.points[0].z), ("7", 10.0, 1.5));
    }

    #[test]
    fn semicolons_and_tabs_allow_a_decimal_comma() {
        let parsed = parse("P1;155000,25;463000,75;-0,42;MV\nP2;155010,00;463000,00;0,10;MV\n", Order::Auto);
        assert_eq!(parsed.points[0], SurveyPoint { name: "P1".into(), x: 155_000.25, y: 463_000.75, z: -0.42, code: "MV".into() });
        let tabs = parse("P1\t1,5\t2,5\t3,5\n", Order::Auto);
        assert_eq!((tabs.points[0].x, tabs.points[0].y, tabs.points[0].z), (1.5, 2.5, 3.5));
    }

    #[test]
    fn north_before_east_is_seen_in_the_dutch_grid_and_can_be_said() {
        // northing first: only that way do both lie within the grid
        let parsed = parse("1,463000,155000,1.0,TOPO\n2,463010,155005,1.1,TOPO\n", Order::Auto);
        assert_eq!((parsed.order, parsed.points[0].x, parsed.points[0].y), (Order::NorthEast, 155_000.0, 463_000.0));
        // local coordinates say nothing: east first, unless told
        let local = parse("1,2000,1000,1.0\n", Order::Auto);
        assert_eq!((local.order, local.points[0].x), (Order::EastNorth, 2000.0));
        let told = parse("1,2000,1000,1.0\n", Order::NorthEast);
        assert_eq!((told.points[0].x, told.points[0].y), (1000.0, 2000.0));
    }

    #[test]
    fn a_file_without_points_gives_none() {
        assert_eq!(parse("", Order::Auto).points.len(), 0);
        let words = parse("these are\nno points at all\n", Order::Auto);
        assert_eq!((words.points.len(), words.skipped), (0, 2));
    }
}
