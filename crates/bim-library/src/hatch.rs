//! Hatches as tiles: the signs of a country's drawing standard and the suite's own coloured ones.
//!
//! A hatch here is a small tile with marks on it (lines, filled areas, dots), laid side by side,
//! scaled and turned: the way the INB template of the OpenAEC Foundation draws its hatches. The
//! catalogue (`data/hatches-nl.json`) was taken from that template's hatch patterns for drawings
//! at 1:50 (see the file's `source` and `licence`); it is compiled in, so nothing is read at run
//! time.
//!
//! - [`HatchSet::Nen47`]: the signs of NEN 47 the template has, numbered as the template numbers
//!   them, in black on the paper.
//! - [`HatchSet::OpenAec`]: the suite's own: the same signs on a colour, and under a sign those of
//!   one kind of material (a species of wood, a kind of board, of insulation, of stone, of
//!   ground).
//!
//! Sizes are millimetres on paper. Nothing is added to what the template draws, and where it
//! draws the same hatch twice the catalogue has it once.
//!
//! A material names a hatch of this catalogue as its own ([`crate::Material::sign`]), and a
//! drawing cuts it with that hatch: the hatch's lines as rows ([`Hatch::rows`]) over its colour.
//!
//! An element of several layers is cut a face per layer, each with its material's hatch.
//!
//! Not yet: a drawing does not draw a hatch's filled areas and dots (lead, sealant, sand,
//! gravel, water show as their colour).

use crate::material::Names;
use crate::{uuid, Lang};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::OnceLock;

/// The catalogue as taken from the template.
const CATALOGUE: &str = include_str!("../data/hatches-nl.json");

/// The attribute of a hatch's node with the hatch itself.
pub const HATCH: &str = "openaec::hatch";

/// A rectangle that would take more tiles than this is given its fill only.
const MOST_TILES: i64 = 50_000;

/// Whose hatches a hatch is one of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HatchSet {
    /// The signs of NEN 47 (the Netherlands), black on the paper.
    Nen47,
    /// The suite's own: the signs on a colour, and those of one kind of material.
    OpenAec,
}

impl HatchSet {
    pub const ALL: [HatchSet; 2] = [HatchSet::Nen47, HatchSet::OpenAec];

    /// Its name, as it is written everywhere.
    pub fn name(self) -> &'static str {
        match self {
            HatchSet::Nen47 => "NEN 47",
            HatchSet::OpenAec => "OpenAEC",
        }
    }

    /// What the key of each of its hatches starts with.
    fn prefix(self) -> &'static str {
        match self {
            HatchSet::Nen47 => "nen47",
            HatchSet::OpenAec => "openaec",
        }
    }
}

/// What a tile has on it. Points are millimetres on the tile, x to the right and y down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Mark {
    /// A line through its points, `width` wide.
    Line { points: Vec<[f64; 2]>, width: f64, colour: [u8; 3] },
    /// The inside of its points, filled.
    Area { points: Vec<[f64; 2]>, colour: [u8; 3] },
    /// A dot `width` across at each point.
    Dots { points: Vec<[f64; 2]>, width: f64, colour: [u8; 3] },
}

/// A hatch: a tile laid side by side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hatch {
    /// Stable, lower case with hyphens: its set, its sign and, for one kind of material, that
    /// kind in English ("nen47-12", "openaec-12-spruce").
    pub key: String,
    pub set: HatchSet,
    /// The number of its sign in NEN 47.
    pub sign: u32,
    pub names: Names,
    /// The tile's width and height.
    pub tile: [f64; 2],
    /// How far the tiles are turned, in degrees: clockwise on a drawing, whose y runs down.
    pub angle: f64,
    /// How much the tile is scaled before it is turned, along x and y.
    pub scale: [f64; 2],
    /// The colour under the marks; none: the paper.
    pub fill: Option<[u8; 3]>,
    pub marks: Vec<Mark>,
}

/// Lines of one width and colour, as a hatch draws them over a rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct Strokes {
    pub width: f64,
    pub colour: [u8; 3],
    pub segments: Vec<[[f64; 2]; 2]>,
}

/// Dots of one size and colour, as a hatch draws them over a rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct Dots {
    pub width: f64,
    pub colour: [u8; 3],
    pub at: Vec<[f64; 2]>,
}

/// What a hatch draws over a rectangle of paper, all of it inside the rectangle: its fill, then
/// its filled areas, its lines and its dots. Millimetres on paper.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawn {
    pub fill: Option<[u8; 3]>,
    pub areas: Vec<(Vec<[f64; 2]>, [u8; 3])>,
    pub lines: Vec<Strokes>,
    pub dots: Vec<Dots>,
}

#[derive(Deserialize)]
struct File {
    hatches: Vec<Hatch>,
}

/// Every hatch of the catalogue: NEN 47's own, then the suite's, each by its sign.
pub fn catalogue() -> &'static [Hatch] {
    static HATCHES: OnceLock<Vec<Hatch>> = OnceLock::new();
    HATCHES.get_or_init(|| serde_json::from_str::<File>(CATALOGUE).expect("the hatch catalogue is compiled in").hatches)
}

/// The hatches of `set`.
pub fn of(set: HatchSet) -> impl Iterator<Item = &'static Hatch> {
    catalogue().iter().filter(move |hatch| hatch.set == set)
}

/// The hatch with `key`.
pub fn hatch(key: &str) -> Option<&'static Hatch> {
    catalogue().iter().find(|hatch| hatch.key == key)
}

/// The hatch of sign `sign` itself in `set`: NEN 47's own, or the suite's on its colour.
pub fn sign(set: HatchSet, sign: u32) -> Option<&'static Hatch> {
    of(set).find(|hatch| hatch.sign == sign && hatch.general())
}

/// The path of the node of hatch `key`.
pub fn hatch_path(key: &str) -> String {
    uuid(&format!("openaec-library/hatch/{key}"))
}

/// A hatch as a node of IFCX: its English name, and the hatch itself under [`HATCH`].
pub fn hatch_node(hatch: &Hatch) -> Value {
    json!({
        "path": hatch_path(&hatch.key),
        "attributes": {
            "bsi::ifc::prop::Name": hatch.name(Lang::En),
            HATCH: hatch,
        }
    })
}

/// The hatches of `set` as nodes of IFCX.
pub fn nodes(set: HatchSet) -> Vec<Value> {
    of(set).map(hatch_node).collect()
}

impl Hatch {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// Whether it is the hatch of its sign itself, not that of one kind of material under it.
    pub fn general(&self) -> bool {
        self.key.strip_prefix(self.set.prefix()).and_then(|rest| rest.strip_prefix('-')).is_some_and(|rest| rest.parse::<u32>() == Ok(self.sign))
    }

    /// A point of the tile on paper: scaled, then turned by the angle.
    pub fn on_paper(&self, p: [f64; 2]) -> [f64; 2] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (x, y) = (p[0] * self.scale[0], p[1] * self.scale[1]);
        [x * cos - y * sin, x * sin + y * cos]
    }

    /// A point of the paper on the tiles' own plane: what [`Hatch::on_paper`] takes back.
    pub fn on_tile(&self, p: [f64; 2]) -> [f64; 2] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (x, y) = (p[0] * cos + p[1] * sin, -p[0] * sin + p[1] * cos);
        [x / self.scale[0], y / self.scale[1]]
    }

    /// What the hatch draws over the rectangle of paper from `min` to `max` (millimetres): the
    /// marks of every tile that reaches it, on paper and cut to the rectangle. A line is as wide
    /// as the tile's scale makes it.
    pub fn over(&self, min: [f64; 2], max: [f64; 2]) -> Drawn {
        let mut drawn = Drawn { fill: self.fill, ..Drawn::default() };
        let [w, h] = self.tile;
        if !(w > 0.0 && h > 0.0 && max[0] > min[0] && max[1] > min[1] && self.scale[0] != 0.0 && self.scale[1] != 0.0) {
            return drawn;
        }
        // the tiles that reach the rectangle: those over its corners' reach on the tiles' plane
        let corners = [[min[0], min[1]], [max[0], min[1]], [max[0], max[1]], [min[0], max[1]]].map(|c| self.on_tile(c));
        let reach = |axis: usize, size: f64| -> (i64, i64) {
            let (lo, hi) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c[axis]), hi.max(c[axis])));
            ((lo / size).floor() as i64 - 1, (hi / size).floor() as i64 + 1)
        };
        let ((i0, i1), (j0, j1)) = (reach(0, w), reach(1, h));
        if (i1 - i0 + 1).saturating_mul(j1 - j0 + 1) > MOST_TILES {
            return drawn;
        }
        let wider = (self.scale[0] * self.scale[1]).abs().sqrt();
        let inside = |p: [f64; 2]| p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1];
        for j in j0..=j1 {
            for i in i0..=i1 {
                let place = |p: &[f64; 2]| self.on_paper([p[0] + i as f64 * w, p[1] + j as f64 * h]);
                for mark in &self.marks {
                    match mark {
                        Mark::Line { points, width, colour } => {
                            let width = width * wider;
                            let segments = points.windows(2).filter_map(|pair| cut_segment(place(&pair[0]), place(&pair[1]), min, max));
                            match drawn.lines.iter_mut().find(|s| s.width == width && s.colour == *colour) {
                                Some(strokes) => strokes.segments.extend(segments),
                                None => drawn.lines.push(Strokes { width, colour: *colour, segments: segments.collect() }),
                            }
                        }
                        Mark::Area { points, colour } => {
                            let area = cut_area(points.iter().map(place).collect(), min, max);
                            if area.len() >= 3 {
                                drawn.areas.push((area, *colour));
                            }
                        }
                        Mark::Dots { points, width, colour } => {
                            let width = width * wider;
                            let at = points.iter().map(place).filter(|p| inside(*p));
                            match drawn.dots.iter_mut().find(|d| d.width == width && d.colour == *colour) {
                                Some(dots) => dots.at.extend(at),
                                None => drawn.dots.push(Dots { width, colour: *colour, at: at.collect() }),
                            }
                        }
                    }
                }
            }
        }
        drawn.lines.retain(|strokes| !strokes.segments.is_empty());
        drawn.dots.retain(|dots| !dots.at.is_empty());
        drawn
    }
}

/// The part of the segment from `a` to `b` inside the rectangle from `min` to `max`.
fn cut_segment(a: [f64; 2], b: [f64; 2], min: [f64; 2], max: [f64; 2]) -> Option<[[f64; 2]; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for axis in 0..2 {
        if d[axis].abs() < 1e-12 {
            if a[axis] < min[axis] || a[axis] > max[axis] {
                return None;
            }
        } else {
            let (enter, leave) = ((min[axis] - a[axis]) / d[axis], (max[axis] - a[axis]) / d[axis]);
            let (enter, leave) = if enter <= leave { (enter, leave) } else { (leave, enter) };
            t0 = t0.max(enter);
            t1 = t1.min(leave);
        }
    }
    // (a segment that only touches the rectangle draws nothing)
    (t1 - t0 > 1e-9).then(|| [[a[0] + d[0] * t0, a[1] + d[1] * t0], [a[0] + d[0] * t1, a[1] + d[1] * t1]])
}

/// The part of a polygon inside the rectangle from `min` to `max`: cut by each of its sides.
fn cut_area(mut points: Vec<[f64; 2]>, min: [f64; 2], max: [f64; 2]) -> Vec<[f64; 2]> {
    // each side as an axis, a bound, and whether the inside is above it
    for (axis, bound, above) in [(0, min[0], true), (0, max[0], false), (1, min[1], true), (1, max[1], false)] {
        if points.len() < 3 {
            return Vec::new();
        }
        let inside = |p: &[f64; 2]| if above { p[axis] >= bound } else { p[axis] <= bound };
        let mut cut = Vec::with_capacity(points.len() + 4);
        for (k, now) in points.iter().enumerate() {
            let before = &points[(k + points.len() - 1) % points.len()];
            if inside(now) != inside(before) {
                let t = (bound - before[axis]) / (now[axis] - before[axis]);
                cut.push([before[0] + (now[0] - before[0]) * t, before[1] + (now[1] - before[1]) * t]);
            }
            if inside(now) {
                cut.push(*now);
            }
        }
        points = cut;
    }
    if points.len() < 3 {
        return Vec::new();
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn within(p: [f64; 2], min: [f64; 2], max: [f64; 2]) -> bool {
        p[0] >= min[0] - 1e-9 && p[0] <= max[0] + 1e-9 && p[1] >= min[1] - 1e-9 && p[1] <= max[1] + 1e-9
    }

    fn surface(points: &[[f64; 2]]) -> f64 {
        (0..points.len()).map(|k| (points[k], points[(k + 1) % points.len()])).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>().abs() / 2.0
    }

    #[test]
    fn the_catalogue_has_the_signs_of_nen_47_and_the_suites_coloured_ones() {
        assert_eq!((of(HatchSet::Nen47).count(), of(HatchSet::OpenAec).count()), (23, 72));
        let keys: BTreeSet<&str> = catalogue().iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys.len(), catalogue().len(), "a key twice");
        // NEN 47's own are black on the paper, by their number; each has the suite's coloured one
        let signs: Vec<u32> = of(HatchSet::Nen47).map(|h| h.sign).collect();
        assert_eq!(signs, [1, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 20, 22, 23, 25, 26, 27, 28, 29]);
        for own in of(HatchSet::Nen47) {
            assert!(own.general() && own.fill.is_none(), "{}", own.key);
            assert_eq!(hatch(&own.key), Some(own));
            let coloured = sign(HatchSet::OpenAec, own.sign).unwrap_or_else(|| panic!("sign {} in colour", own.sign));
            assert_eq!((&coloured.names, coloured.tile == own.tile || own.sign == 6), (&own.names, true), "{}", coloured.key);
        }
        assert_eq!(sign(HatchSet::Nen47, 1).map(|h| (h.name(Lang::Nl), h.name(Lang::En), h.tile, h.angle, h.scale, h.marks.len())), Some(("metselwerk van baksteen", "brickwork", [3.0, 3.0], 45.0, [0.75, 0.75], 2)));
        // under a sign the suite has one kind of material each: its own name, the sign's number
        let spruce = hatch("openaec-12-spruce").expect("spruce");
        assert_eq!((spruce.name(Lang::Nl), spruce.name(Lang::De), spruce.sign, spruce.general(), spruce.set), ("vuren", "Fichte", 12, false, HatchSet::OpenAec));
        assert!(spruce.fill.is_some());
        let kinds = |number: u32| of(HatchSet::OpenAec).filter(|h| h.sign == number && !h.general()).count();
        assert_eq!((kinds(1), kinds(9), kinds(12), kinds(16), kinds(17), kinds(25), kinds(29)), (5, 5, 17, 8, 7, 5, 2));
        // every hatch draws something, on a tile with a size
        for h in catalogue() {
            assert!(h.tile[0] > 0.0 && h.tile[1] > 0.0 && (h.fill.is_some() || !h.marks.is_empty()), "{}", h.key);
            assert!(!h.names.nl.is_empty() && !h.names.en.is_empty() && !h.names.de.is_empty() && !h.names.fr.is_empty(), "{}", h.key);
        }
    }

    #[test]
    fn a_hatch_is_its_tile_laid_side_by_side_scaled_and_turned() {
        // brickwork: on a tile of 3 two lines, at 1 and at 1.5, scaled by 0.75 and turned 45°
        let brick = hatch("nen47-1").expect("brickwork");
        let p = brick.on_paper([1.0, 0.0]);
        let half = 0.75 * std::f64::consts::FRAC_1_SQRT_2;
        assert!((p[0] - half).abs() < 1e-12 && (p[1] - half).abs() < 1e-12, "{p:?}");
        let back = brick.on_tile(p);
        assert!((back[0] - 1.0).abs() < 1e-12 && back[1].abs() < 1e-12, "{back:?}");

        let (min, max) = ([0.0, 0.0], [20.0, 20.0]);
        let drawn = brick.over(min, max);
        assert!(drawn.fill.is_none() && drawn.areas.is_empty() && drawn.dots.is_empty());
        assert_eq!(drawn.lines.len(), 1, "one width, one colour");
        let strokes = &drawn.lines[0];
        assert!((strokes.width - 0.15 * 0.75).abs() < 1e-12 && strokes.colour == [0, 0, 0]);
        // every piece is inside, and runs along the turned tile's y
        let (sin, cos) = 45.0_f64.to_radians().sin_cos();
        let mut across = BTreeSet::new();
        for [a, b] in &strokes.segments {
            assert!(within(*a, min, max) && within(*b, min, max), "{a:?} {b:?}");
            let d = [b[0] - a[0], b[1] - a[1]];
            assert!((d[0] * cos + d[1] * sin).abs() < 1e-9, "not along the lines: {d:?}");
            across.insert(((a[0] * cos + a[1] * sin) * 1000.0).round() as i64);
        }
        // the lines: at 0.75 and 1.125 mm from the corner, and every 2.25 mm on, across the
        // rectangle's diagonal of 28.28 mm
        let expected: BTreeSet<i64> = (0..13).flat_map(|k| [750 + 2250 * k, 1125 + 2250 * k]).collect();
        assert_eq!(across, expected);
        // another rectangle, away from the corner the tiles start at
        let (min, max) = ([100.0, 50.0], [104.0, 53.0]);
        let there = brick.over(min, max);
        assert!(!there.lines[0].segments.is_empty() && there.lines[0].segments.iter().all(|[a, b]| within(*a, min, max) && within(*b, min, max)));
    }

    #[test]
    fn what_a_hatch_draws_is_cut_to_its_rectangle() {
        // lead: of every tile of 6 by 3 the left half black; at 0.75 a square of 2.25 every 4.5
        let lead = hatch("nen47-20").expect("lead");
        let drawn = lead.over([0.0, 0.0], [9.0, 2.25]);
        assert!(drawn.lines.is_empty() && drawn.dots.is_empty());
        let black: f64 = drawn.areas.iter().map(|(points, _)| surface(points)).sum();
        assert!((black - 2.0 * 2.25 * 2.25).abs() < 1e-9, "{black}");
        assert!(drawn.areas.iter().all(|(points, colour)| *colour == [0, 0, 0] && points.iter().all(|p| within(*p, [0.0, 0.0], [9.0, 2.25]))));
        // half a square where the rectangle ends in one
        let cut: f64 = lead.over([0.0, 0.0], [1.125, 2.25]).areas.iter().map(|(points, _)| surface(points)).sum();
        assert!((cut - 1.125 * 2.25).abs() < 1e-9, "{cut}");
        // sand in colour: its fill and its dots, none outside
        let sand = hatch("openaec-26").expect("sand");
        let (min, max) = ([5.0, 5.0], [25.0, 15.0]);
        let drawn = sand.over(min, max);
        assert!(drawn.fill.is_some() && drawn.dots.len() == 1 && drawn.dots[0].at.len() > 20);
        assert!(drawn.dots[0].at.iter().all(|p| within(*p, min, max)));
        // (the same dots on the paper, where NEN 47 has no colour)
        assert_eq!(hatch("nen47-26").expect("sand").over(min, max).dots, drawn.dots);
        // water: its waves, filled and drawn
        let water = hatch("nen47-28").expect("water").over([0.0, 0.0], [12.0, 6.0]);
        assert!(!water.areas.is_empty() && water.lines.len() == 1 && water.lines[0].segments.len() > 24);
        // nothing where there is no rectangle
        assert_eq!(sand.over([5.0, 5.0], [5.0, 9.0]), Drawn { fill: sand.fill, ..Drawn::default() });
    }

    #[test]
    fn a_hatch_is_a_node_of_ifcx() {
        let pir = hatch("openaec-17-pir").expect("PIR");
        let node = hatch_node(pir);
        assert_eq!(node["path"], hatch_path("openaec-17-pir"));
        assert_eq!(node["attributes"]["bsi::ifc::prop::Name"], "PIR");
        assert_eq!(node["attributes"][HATCH]["set"], "openaec");
        assert_eq!(node["attributes"][HATCH]["marks"][0]["kind"], "line");
        // read back, it is the hatch
        let back: Hatch = serde_json::from_value(node["attributes"][HATCH].clone()).expect("a hatch");
        assert_eq!(&back, pir);
        assert_eq!((nodes(HatchSet::Nen47).len(), nodes(HatchSet::OpenAec).len()), (23, 72));
        assert_ne!(hatch_path("nen47-1"), hatch_path("openaec-1"));
    }
}

/// Parallel rows of lines, as a drawing hatches a cut with them: lines at `angle` (radians,
/// anticlockwise from x, y up), the first through `origin`, each next one `offset` across (to
/// its left) and `shift` along; `dashes` are the lengths drawn and left open in turn from the
/// row's start (none: a full line). Metres on the model.
#[derive(Debug, Clone, PartialEq)]
pub struct Rows {
    pub angle: f64,
    pub origin: [f64; 2],
    pub shift: f64,
    pub offset: f64,
    pub dashes: Vec<f64>,
    pub colour: [u8; 3],
}

/// The scale the catalogue's tiles are drawn for: a millimetre of a tile is this many metres of
/// the model (1:50).
pub const DRAWN_FOR: f64 = 50.0 / 1000.0;

impl Hatch {
    /// Its lines as rows a drawing hatches with, in metres on the model at the scale the
    /// catalogue is drawn for. A straight piece of a line that runs along the tiles (across,
    /// down, or from a corner of a tile to a corner some tiles on) repeats as a row: a full
    /// line when it spans its tiles, else dashes. Filled areas, dots and pieces that run at
    /// another slope are no rows: `whole` says whether every mark became rows.
    pub fn rows(&self) -> (Vec<Rows>, bool) {
        const EPS: f64 = 1e-9;
        let unit = DRAWN_FOR;
        let (w, h) = (self.tile[0] * self.scale[0] * unit, self.tile[1] * self.scale[1] * unit);
        // a point of the tile, y up and before the turn
        let on_model = |p: [f64; 2]| [p[0] * self.scale[0] * unit, -p[1] * self.scale[1] * unit];
        // (clockwise on a drawing whose y runs down is clockwise with y up too)
        let turn = -self.angle.to_radians();
        let (sin, cos) = turn.sin_cos();
        let turned = |p: [f64; 2]| [p[0] * cos - p[1] * sin, p[0] * sin + p[1] * cos];
        let mut rows = Vec::new();
        let mut whole = true;
        if w <= EPS || h <= EPS {
            return (rows, self.marks.is_empty());
        }
        for mark in &self.marks {
            let Mark::Line { points, colour, .. } = mark else {
                whole = false;
                continue;
            };
            for pair in points.windows(2) {
                let (a, b) = (on_model(pair[0]), on_model(pair[1]));
                let d = [b[0] - a[0], b[1] - a[1]];
                let length = d[0].hypot(d[1]);
                if length <= EPS {
                    continue;
                }
                // the tiles it runs along: m across and n down, coprime
                let along = (-6i32..=6).flat_map(|m| (-6i32..=6).map(move |n| (m, n))).filter(|&(m, n)| (m, n) != (0, 0) && gcd(m, n) == 1).find(|&(m, n)| {
                    let t = [f64::from(m) * w, -f64::from(n) * h];
                    (d[0] * t[1] - d[1] * t[0]).abs() <= 1e-6 * length * t[0].hypot(t[1]) && d[0] * t[0] + d[1] * t[1] > 0.0
                });
                let Some((m, n)) = along else {
                    whole = false;
                    continue;
                };
                let t = [f64::from(m) * w, -f64::from(n) * h];
                let period = t[0].hypot(t[1]);
                let unit_t = [t[0] / period, t[1] / period];
                let left = [-unit_t[1], unit_t[0]];
                // a second vector of the tiles that makes a cell with the first, to its left
                let Some(s) = (-6i32..=6).flat_map(|p| (-6i32..=6).map(move |q| (p, q))).filter(|&(p, q)| (m * q - n * p).abs() == 1).map(|(p, q)| [f64::from(p) * w, -f64::from(q) * h]).find(|s| s[0] * left[0] + s[1] * left[1] > 0.0) else {
                    whole = false;
                    continue;
                };
                let offset = s[0] * left[0] + s[1] * left[1];
                let shift = (s[0] * unit_t[0] + s[1] * unit_t[1]).rem_euclid(period);
                let dashes = if length >= period - 1e-9 { Vec::new() } else { vec![length, period - length] };
                rows.push(Rows { angle: unit_t[1].atan2(unit_t[0]) + turn, origin: turned(a), shift: if dashes.is_empty() { 0.0 } else { shift }, offset, dashes, colour: *colour });
            }
        }
        (rows, whole)
    }
}

fn gcd(a: i32, b: i32) -> i32 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

#[cfg(test)]
mod rows_tests {
    use super::*;

    fn of(key: &str) -> (Vec<Rows>, bool) {
        hatch(key).unwrap_or_else(|| panic!("{key}")).rows()
    }

    #[test]
    fn brickwork_is_pairs_of_lines_at_45_degrees() {
        // NEN 47, sign 1: a tile of 3 mm with two lines down it, 0.5 mm apart, at 0.75 and 45°
        let (rows, whole) = of("nen47-1");
        assert!(whole);
        assert_eq!(rows.len(), 2);
        for row in &rows {
            // a line down the tile, turned 45° clockwise: at 225° (the same line as 45°)
            let slope = row.angle.rem_euclid(std::f64::consts::PI);
            assert!((slope - 45f64.to_radians()).abs() < 1e-9, "{}", row.angle.to_degrees());
            assert!((row.offset - 0.1125).abs() < 1e-12 && row.dashes.is_empty(), "{row:?}");
        }
        // the two lines of a pair lie 0.5 mm of the tile apart: 0.01875 m at 1:50
        let (a, b) = (&rows[0], &rows[1]);
        let left = [-a.angle.sin(), a.angle.cos()];
        let apart = (b.origin[0] - a.origin[0]) * left[0] + (b.origin[1] - a.origin[1]) * left[1];
        assert!((apart.abs() - 0.01875).abs() < 1e-12, "{apart}");
    }

    #[test]
    fn crossed_and_dashed_lines_and_zigzags_are_rows() {
        // hardwood: lines down and across the tile, so crossed
        let (rows, whole) = of("nen47-13");
        assert!(whole && rows.len() == 2);
        let between = (rows[0].angle - rows[1].angle).rem_euclid(std::f64::consts::PI);
        assert!((between - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        // natural stone: a full line and a half one, which is dashes of half the tile
        let (rows, whole) = of("nen47-9");
        assert!(whole);
        let dashed = rows.iter().find(|r| !r.dashes.is_empty()).expect("a dashed row");
        let tile = 1.5 * 0.75 * DRAWN_FOR;
        assert!((dashed.dashes[0] - tile / 2.0).abs() < 1e-12 && (dashed.dashes[1] - tile / 2.0).abs() < 1e-12, "{dashed:?}");
        assert!(rows.iter().any(|r| r.dashes.is_empty()));
        // a finish: a zigzag over a tile of 6 by 3, each leg from a corner to the middle of the far side
        let (rows, whole) = of("nen47-10");
        assert!(whole && rows.len() == 2 && rows.iter().all(|r| r.dashes.len() == 2), "{rows:?}");
        // insulation: the two diagonals of a square tile, full lines
        let (rows, whole) = of("nen47-17");
        assert!(whole && rows.len() == 2 && rows.iter().all(|r| r.dashes.is_empty()));
    }

    #[test]
    fn areas_and_dots_are_no_rows() {
        for key in ["nen47-20", "nen47-26", "nen47-27"] {
            let (rows, whole) = of(key);
            assert!(rows.is_empty() && !whole, "{key}");
        }
        // every hatch of the catalogue gives rows that are sound
        for h in catalogue() {
            for row in h.rows().0 {
                assert!(row.offset > 0.0 && row.angle.is_finite() && row.dashes.iter().all(|d| *d > 0.0), "{}: {row:?}", h.key);
            }
        }
    }
}
