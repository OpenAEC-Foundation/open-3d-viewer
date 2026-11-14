//! The hatches of a drawing's cut faces: what an element is made of, read from its properties or
//! its layer's name, gives the hatch of the drawing standard, and that hatch becomes lines inside
//! the face's outlines, in the drawing's own metres. Headless, like the drawing.

use crate::drawing::{Cut, Fill, Segment};
use crate::model::Model;
use bim_library::{DrawingStandard, HatchKind, HatchPattern};

/// A face takes about this many hatch lines at most; a larger one gets a coarser hatch.
const MOST_LINES: f64 = 20_000.0;
/// Insulation's zigzag, which wants the layer's own direction, is drawn in a free outline as
/// crossed lines this far apart (metres), at 60° and 120°.
const INSULATION_SPACING: f64 = 0.06;

/// Hatches the cut faces of `cut`, a drawing at 1 : `scale`, by what the model says its elements
/// are made of.
pub fn hatch(cut: &mut Cut, model: &Model, standard: DrawingStandard, scale: f64) {
    hatch_with(cut, model, standard, scale, None);
}

/// [`hatch`], with the looks a view gives its elements otherwise (`looks`, by element: the
/// filters of a Revit view) over what they are made of.
pub fn hatch_with(cut: &mut Cut, model: &Model, standard: DrawingStandard, scale: f64, looks: Option<&std::collections::HashMap<u64, Look>>) {
    let mut patterns = Vec::with_capacity(cut.fills.len());
    for fill in &mut cut.fills {
        let pattern = pattern_of(model, fill.layer, fill.element, standard);
        let pattern = match fill.element.and_then(|e| looks?.get(&e)) {
            Some(look) => look.apply(pattern),
            None => pattern,
        };
        fill.look = Some(pattern.look());
        (fill.rows, fill.apart) = rows(&fill.loops, &pattern, scale);
        patterns.push((fill.layer, pattern));
    }
    // elements of one category and one material that meet are of one piece: joined walls
    // and so are elements of one material that the file says are joined, whatever their
    // categories (a wall on a beam): the line between them goes, the elements stay apart
    let elements: Vec<Option<u64>> = cut.fills.iter().map(|f| f.element).collect();
    let joined = |a: usize, b: usize| match (elements[a], elements[b]) {
        (Some(a), Some(b)) => model.project.joins.contains(&(a.min(b), a.max(b))),
        _ => false,
    };
    crate::drawing::join(cut, &|a, b| patterns[a] == patterns[b] || (patterns[a].1 == patterns[b].1 && joined(a, b)));
}

/// The pieces the joined elements of the model make: elements the file says are joined and that
/// are hatched alike are of one piece (an element to its piece, counted from 1). A drawing leaves
/// out the edges on the seams within a piece; the elements stay what they are.
pub fn pieces(model: &Model, standard: DrawingStandard) -> std::collections::HashMap<u64, u32> {
    use std::collections::HashMap;
    let mut out = HashMap::new();
    if model.project.joins.is_empty() {
        return out;
    }
    let layer_of: HashMap<u64, usize> = model.layers.iter().enumerate().flat_map(|(i, l)| l.elements.iter().map(move |e| (e.0, i))).collect();
    let mut patterns: HashMap<u64, Option<Pattern>> = HashMap::new();
    let mut pattern = |id: u64| patterns.entry(id).or_insert_with(|| layer_of.get(&id).map(|&layer| pattern_of(model, layer, Some(id), standard))).clone();
    // every element starts as its own piece; a join of two hatched alike makes theirs one
    let mut head: HashMap<u64, u64> = HashMap::new();
    fn top(head: &mut HashMap<u64, u64>, mut id: u64) -> u64 {
        while let Some(&up) = head.get(&id).filter(|up| **up != id) {
            id = up;
        }
        id
    }
    for &(a, b) in &model.project.joins {
        let (Some(pa), Some(pb)) = (pattern(a), pattern(b)) else { continue };
        if pa == pb {
            let (ta, tb) = (top(&mut head, a), top(&mut head, b));
            head.insert(ta, ta);
            head.insert(tb, ta);
        }
    }
    let mut numbers: HashMap<u64, u32> = HashMap::new();
    for id in head.keys().copied().collect::<Vec<_>>() {
        let piece = top(&mut head, id);
        let next = numbers.len() as u32 + 1;
        out.insert(id, *numbers.entry(piece).or_insert(next));
    }
    out
}

/// The hatch of an element's cut: the cut pattern of its material where the file brought one (a
/// Revit model), else the hatch `standard` gives what it is made of.
pub fn pattern_of(model: &Model, layer: usize, element: Option<u64>, standard: DrawingStandard) -> Pattern {
    // the earth under a cut terrain: the material the site's settings give it
    let terrain = model.layers.get(layer).is_some_and(|l| l.name.contains("Topograph"));
    let earth = model.project.earth.as_ref().filter(|_| terrain).and_then(|e| e.1.as_ref()).and_then(|name| model.project.cut_patterns.get(name));
    if let Some(earth) = earth {
        return earth.clone();
    }
    material_pattern(model, element).unwrap_or_else(|| kind_of(model, layer, element).pattern(standard).into())
}

/// The cut pattern of the first of an element's materials that has one.
fn material_pattern(model: &Model, element: Option<u64>) -> Option<Pattern> {
    let patterns = &model.project.cut_patterns;
    if patterns.is_empty() {
        return None;
    }
    let props = model.element_props(element?);
    let (_, named) = props.iter().find(|(k, _)| k.eq_ignore_ascii_case("materials") || k.eq_ignore_ascii_case("material"))?;
    patterns.get(named.as_str()).or_else(|| named.split(", ").find_map(|name| patterns.get(name))).cloned()
}

/// What an element is made of: its material property, else what its other properties and its
/// layer's name say, else nothing in particular.
pub fn kind_of(model: &Model, layer: usize, element: Option<u64>) -> HatchKind {
    let props = element.map(|id| model.element_props(id)).unwrap_or_default();
    let named = |key: &str| {
        let key = key.to_lowercase();
        key.contains("materia") || key.contains("matéri")
    };
    let material = props.iter().filter(|(k, _)| named(k)).find_map(|(_, v)| kind_in(v));
    let other = || props.iter().filter(|(k, _)| !named(k)).find_map(|(_, v)| kind_in(v));
    let layer = || model.layers.get(layer).and_then(|l| kind_in(&l.name));
    material.or_else(other).or_else(layer).unwrap_or(HatchKind::Generic)
}

/// The hatch a text names (Dutch, English, German, French), if it names a material.
pub fn kind_in(text: &str) -> Option<HatchKind> {
    use HatchKind::*;
    const WORDS: [(HatchKind, &[&str]); 8] = [
        // the layered and the specific before the general: "kalkzandsteen" is not "steen"
        (Insulation, &["isolatie", "insulation", "dämm", "daemm", "isolant", "isolation", "minerale wol", "mineral wool", "mineralwolle", "glaswol", "steenwol", "rockwool", "eps", "xps", "pir"]),
        (SandLime, &["kalkzandsteen", "sand-lime", "sand lime", "kalksandstein", "silico-calcaire", "cellenbeton", "aerated", "porenbeton"]),
        (Gypsum, &["gips", "gypsum", "plasterboard", "plâtre", "platre", "stuc"]),
        (Concrete, &["beton", "béton", "concrete", "kanaalplaat", "hollow-core", "hollow core", "breedplaat", "cement", "screed", "estrich", "chape", "dekvloer"]),
        (Brick, &["metselwerk", "baksteen", "brick", "mauerwerk", "ziegel", "maçonnerie", "maconnerie", "brique", "masonry"]),
        (Timber, &["hout", "timber", "wood", "holz", "bois", "multiplex", "plywood", "osb", "vuren", "grenen", "eiken", "lvl", "clt"]),
        (Steel, &["staal", "steel", "stahl", "acier", "s235", "s355"]),
        (Air, &["spouw", "cavity", "luftschicht", "lame d'air", "luchtlaag"]),
    ];
    let text = text.to_lowercase();
    // short codes count as words only, so "pir" is not found in "inspiration"
    let has = |word: &str| {
        if word.len() > 3 {
            return text.contains(word);
        }
        text.split(|c: char| !c.is_alphanumeric()).any(|part| part == word)
    };
    WORDS.iter().find(|(_, words)| words.iter().any(|w| has(w))).map(|(kind, _)| *kind)
}

/// A hatch as it is drawn: what lies behind its lines, their colour, and the lines themselves as
/// grids of parallel rows. Open BIM Studio's hatches and the cut patterns of a Revit material
/// both come to this.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pattern {
    /// The colour behind the lines; paper white without one.
    pub fill: Option<[u8; 3]>,
    pub line: [u8; 3],
    /// Filled in the line colour, without lines.
    pub solid: bool,
    pub grids: Vec<Grid>,
    /// The grids are sized on paper (a drafting pattern), so they grow with the drawing's scale;
    /// else in the model.
    pub on_paper: bool,
}

/// Parallel rows of a hatch: lines at `angle` (radians, anticlockwise from x), the first through
/// `origin`, each next one `offset` across and `shift` along; `dashes` are the lengths drawn and
/// left open in turn from the row's start (none: a full line). Metres.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Grid {
    pub angle: f64,
    pub origin: [f64; 2],
    pub shift: f64,
    pub offset: f64,
    pub dashes: Vec<f64>,
}

impl From<HatchPattern> for Pattern {
    fn from(p: HatchPattern) -> Self {
        let grid = |degrees: f64, offset: f64| Grid { angle: degrees.to_radians(), offset, ..Grid::default() };
        let grids = if p.zigzag {
            vec![grid(60.0, INSULATION_SPACING), grid(120.0, INSULATION_SPACING)]
        } else if p.spacing <= 0.0 {
            Vec::new()
        } else if p.crossed {
            vec![grid(p.angle, p.spacing), grid(p.angle + 90.0, p.spacing)]
        } else {
            vec![grid(p.angle, p.spacing)]
        };
        Pattern { fill: Some(p.fill), line: p.line, solid: false, grids, on_paper: false }
    }
}

/// How a view draws an element otherwise than what it is made of says (a Revit view's filters):
/// what is not given stays as the material has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    /// Drawn faint (in halftone).
    pub halftone: bool,
    /// The pattern of its cut: its grids and whether they are sized on paper; no grids: solid.
    pub pattern: Option<(Vec<Grid>, bool)>,
    /// The colour of that pattern.
    pub color: Option<[u8; 3]>,
    /// The colour behind the pattern.
    pub behind: Option<[u8; 3]>,
    /// The pattern of its cut is not drawn.
    pub no_pattern: bool,
    /// Nothing is drawn behind the pattern.
    pub no_behind: bool,
    /// The pen of its cut lines, and of its edges beyond the cut.
    pub cut_line: crate::drawing::Ink,
    pub seen_line: crate::drawing::Ink,
    /// The pens (numbers) of those as the file gives them, whose width depends on the view's
    /// scale: read into their inks per view.
    pub pens: [Option<i64>; 2],
    /// The colour of its surfaces as seen (not cut): in a 3D view, where a solid pattern
    /// covers them.
    pub surface: Option<[u8; 3]>,
    /// How transparent its surfaces are, in percent (0: as its material).
    pub transparency: u8,
}

impl Look {
    /// Whether it changes anything.
    pub fn is_empty(&self) -> bool {
        *self == Look::default()
    }

    /// `self` where it gives something, else `under` (a filter lower in the view's list).
    pub fn over(self, under: &Look) -> Look {
        Look {
            halftone: self.halftone || under.halftone,
            pattern: self.pattern.or_else(|| under.pattern.clone()),
            color: self.color.or(under.color),
            behind: self.behind.or(under.behind),
            no_pattern: self.no_pattern || under.no_pattern,
            no_behind: self.no_behind || under.no_behind,
            cut_line: ink_over(self.cut_line, under.cut_line),
            seen_line: ink_over(self.seen_line, under.seen_line),
            pens: [self.pens[0].or(under.pens[0]), self.pens[1].or(under.pens[1])],
            surface: self.surface.or(under.surface),
            transparency: if self.transparency > 0 { self.transparency } else { under.transparency },
        }
    }

    /// `pattern` as this look draws it.
    pub fn apply(&self, mut pattern: Pattern) -> Pattern {
        if let Some((grids, on_paper)) = &self.pattern {
            pattern.solid = grids.is_empty();
            pattern.grids = grids.clone();
            pattern.on_paper = *on_paper;
        }
        if let Some(color) = self.color {
            pattern.line = color;
        }
        if let Some(behind) = self.behind {
            pattern.fill = Some(behind);
        }
        if self.no_pattern {
            pattern.solid = false;
            pattern.grids.clear();
        }
        if self.no_behind {
            pattern.fill = None;
        }
        pattern
    }
}

/// `ink` where it gives something, else `under`.
fn ink_over(ink: crate::drawing::Ink, under: crate::drawing::Ink) -> crate::drawing::Ink {
    crate::drawing::Ink { color: ink.color.or(under.color), width: ink.width.or(under.width) }
}

impl Pattern {
    /// The colours a face takes: behind its lines, and its lines.
    pub fn look(&self) -> ([u8; 3], [u8; 3]) {
        (if self.solid { self.line } else { self.fill.unwrap_or([255; 3]) }, self.line)
    }

    /// How far apart its closest rows are (metres) on a drawing at 1 : `scale`; 0 without lines.
    pub fn apart(&self, scale: f64) -> f64 {
        if self.solid {
            return 0.0;
        }
        let unit = if self.on_paper { scale } else { 1.0 };
        let closest = self.grids.iter().map(|g| g.offset.abs() * unit).filter(|o| *o > 0.0).fold(f64::INFINITY, f64::min);
        if closest.is_finite() { closest } else { 0.0 }
    }
}

/// The lines of `pattern` inside `loops` on a drawing at 1 : `scale`, each with its row, and how
/// far apart its rows came to be: a face that would take more than [`MOST_LINES`] lines gets a
/// coarser hatch, twice as wide each time, until it fits.
pub fn rows(loops: &[Vec<[f32; 2]>], pattern: &Pattern, scale: f64) -> (Vec<(i32, Segment)>, f32) {
    if pattern.apart(scale) <= 0.0 {
        return (Vec::new(), 0.0);
    }
    let (lo, hi) = loops.iter().flatten().fold(([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    let across = f64::from((hi[0] - lo[0]).hypot(hi[1] - lo[1]));
    if !(across > 0.0) {
        return (Vec::new(), 0.0);
    }
    let unit = if pattern.on_paper { scale } else { 1.0 };
    // about how many lines a grid draws across the face
    let lines = |g: &Grid, unit: f64| {
        let rows = across / (g.offset.abs() * unit).max(1e-9);
        let period: f64 = g.dashes.iter().map(|d| d.abs()).sum::<f64>() * unit;
        let pieces = if period > 0.0 { (across / period).max(1.0) * (g.dashes.len() as f64 / 2.0).max(1.0) } else { 1.0 };
        rows * pieces
    };
    let mut coarse = 1.0;
    while pattern.grids.iter().filter(|g| g.offset != 0.0).map(|g| lines(g, unit * coarse)).sum::<f64>() > MOST_LINES {
        coarse *= 2.0;
    }
    let out = pattern.grids.iter().filter(|g| g.offset != 0.0).flat_map(|g| grid_lines(loops, g, unit * coarse)).collect();
    (out, (pattern.apart(scale) * coarse) as f32)
}

/// The lines of one grid, its sizes times `unit`, inside `loops` by the even-odd rule. Rows count
/// from the drawing's origin, so the hatches of faces side by side run on.
fn grid_lines(loops: &[Vec<[f32; 2]>], grid: &Grid, unit: f64) -> Vec<(i32, Segment)> {
    let (sin, cos) = grid.angle.sin_cos();
    // along the lines and across them
    let turned = |p: [f64; 2]| [p[0] * cos + p[1] * sin, -p[0] * sin + p[1] * cos];
    let back = |q: [f64; 2]| [(q[0] * cos - q[1] * sin) as f32, (q[0] * sin + q[1] * cos) as f32];
    let edges: Vec<[[f64; 2]; 2]> = loops
        .iter()
        .flat_map(|l| (0..l.len()).map(move |k| [l[k], l[(k + 1) % l.len()]]))
        .map(|[a, b]| [turned(a.map(f64::from)), turned(b.map(f64::from))])
        .collect();
    let (low, high) = edges.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    if !(high > low) {
        return Vec::new();
    }
    let start = turned([grid.origin[0] * unit, grid.origin[1] * unit]);
    let (offset, shift) = (grid.offset * unit, grid.shift * unit);
    let dashes: Vec<f64> = grid.dashes.iter().map(|d| d.abs() * unit).collect();
    let period: f64 = dashes.iter().sum();
    let (a, b) = ((low - start[1]) / offset, (high - start[1]) / offset);
    let (first, last) = (a.min(b).ceil() as i64, a.max(b).floor() as i64);
    let mut out = Vec::new();
    for row in first..=last {
        // half a hair off the row, so a line never runs along an edge or through a corner
        let v = start[1] + row as f64 * offset + offset.abs() * 1e-4;
        let mut crossings: Vec<f64> = edges
            .iter()
            .filter(|[a, b]| (a[1] > v) != (b[1] > v))
            .map(|[a, b]| a[0] + (v - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
            .collect();
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.chunks_exact(2) {
            let (from, to) = (pair[0], pair[1]);
            if to - from <= 1e-6 {
                continue;
            }
            if period <= 0.0 {
                out.push((row as i32, [back([from, v]), back([to, v])]));
                continue;
            }
            // the dashes of this row, from where it starts
            let zero = start[0] + row as f64 * shift;
            let mut at = zero + ((from - zero) / period).floor() * period;
            while at < to {
                let mut x = at;
                for (k, d) in dashes.iter().enumerate() {
                    if k % 2 == 0 {
                        // a dot is drawn as a dash a hair long
                        let (p, q) = (x.max(from), (x + d.max(period * 1e-3)).min(to));
                        if q > p {
                            out.push((row as i32, [back([p, v]), back([q, v])]));
                        }
                    }
                    x += d;
                }
                at += period;
            }
        }
    }
    out
}

/// A polygon cut back to the rectangle `lo`–`hi`, side by side.
fn within(mut polygon: Vec<[f64; 2]>, lo: [f64; 2], hi: [f64; 2]) -> Vec<[f64; 2]> {
    for (axis, at, above) in [(0, lo[0], true), (0, hi[0], false), (1, lo[1], true), (1, hi[1], false)] {
        let inside = |p: &[f64; 2]| if above { p[axis] >= at } else { p[axis] <= at };
        let mut out = Vec::with_capacity(polygon.len() + 4);
        for k in 0..polygon.len() {
            let (a, b) = (polygon[k], polygon[(k + 1) % polygon.len()]);
            if inside(&a) {
                out.push(a);
            }
            if inside(&a) != inside(&b) {
                let t = (at - a[axis]) / (b[axis] - a[axis]);
                out.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
            }
        }
        polygon = out;
    }
    polygon
}

impl Fill {
    /// Its outlines within `crop` (all of them without one), each cut back on its own, which
    /// keeps what the even-odd rule fills.
    pub fn cropped(&self, crop: Option<([f64; 2], [f64; 2])>) -> Vec<Vec<[f64; 2]>> {
        let wide = self.loops.iter().map(|l| l.iter().map(|p| p.map(f64::from)).collect::<Vec<_>>());
        match crop {
            None => wide.collect(),
            Some((lo, hi)) => wide.map(|l| within(l, lo, hi)).filter(|l| l.len() >= 3).collect(),
        }
    }

    /// The hatch lines to draw when a row is `apart` on the medium (pixels, or what `least` is
    /// in): every row, or every n-th when they would stand closer than `least`.
    pub fn lines(&self, apart: f32, least: f32) -> impl Iterator<Item = &Segment> {
        let every = if apart > 0.0 && apart < least { (least / apart).ceil() as i32 } else { 1 };
        self.rows.iter().filter(move |(row, _)| row.rem_euclid(every) == 0).map(|(_, s)| s)
    }

    /// How far apart its rows are (metres of the drawing); 0 without lines.
    pub fn spacing(&self) -> f32 {
        self.apart
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view's filter changes what it gives of a material's hatch, the filter higher in the
    /// view's list first.
    #[test]
    fn a_look_changes_only_what_it_gives() {
        let brick = Pattern { fill: Some([200, 100, 50]), line: [0; 3], solid: false, grids: vec![Grid { angle: 0.785, offset: 0.003, ..Grid::default() }], on_paper: true };
        let grey = Look { pattern: Some((Vec::new(), true)), color: Some([192; 3]), ..Look::default() };
        let solid = grey.apply(brick.clone());
        assert!(solid.solid && solid.grids.is_empty());
        assert_eq!(solid.look(), ([192; 3], [192; 3]));
        let bare = Look { no_behind: true, ..Look::default() }.apply(brick.clone());
        assert_eq!((bare.fill, bare.grids.len()), (None, 1));
        assert_eq!(Look::default().apply(brick.clone()), brick);
        // the higher filter's colour, the lower one's pattern and halftone
        let lower = Look { halftone: true, pattern: Some((Vec::new(), true)), color: Some([1; 3]), ..Look::default() };
        let both = Look { color: Some([9; 3]), ..Look::default() }.over(&lower);
        assert_eq!((both.color, both.halftone, both.pattern.is_some()), (Some([9; 3]), true, true));
        assert!(Look::default().is_empty() && !both.is_empty());
        // the pens of its lines: the higher filter's colour, the lower one's width
        let red = crate::drawing::Ink { color: Some([255, 0, 0]), width: None };
        let thick = crate::drawing::Ink { color: Some([0; 3]), width: Some(0.5) };
        let lines = Look { cut_line: red, pens: [None, Some(3)], ..Look::default() }.over(&Look { cut_line: thick, pens: [Some(5), Some(1)], ..Look::default() });
        assert_eq!((lines.cut_line.color, lines.cut_line.width, lines.pens), (Some([255, 0, 0]), Some(0.5), [Some(5), Some(3)]));
        assert_eq!(lines.apply(brick.clone()), brick, "a pen leaves the faces as they are");
    }
    use crate::drawing::{cut_at, section_at, Shown};
    use crate::model::Layer;
    use std::path::PathBuf;

    /// A closed box from (0, 0, 0) to (4, 2, 3).
    fn box_layer(name: &str) -> Layer {
        let positions = vec![0., 0., 0., 4., 0., 0., 4., 2., 0., 0., 2., 0., 0., 0., 3., 4., 0., 3., 4., 2., 3., 0., 2., 3.];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        Layer { name: name.into(), color: [0.5; 3], normals: vec![0.0; positions.len()], positions, colors: None, indices, elements: Vec::new(), props: Vec::new(), hidden: false, points: false }
    }

    fn model(layers: Vec<Layer>) -> Model {
        Model::new(PathBuf::from("test.obj"), "OBJ", layers)
    }

    #[test]
    fn joined_elements_of_one_material_lose_the_line_between_them() {
        // a concrete wall against a concrete beam: two categories, two elements, side by side
        let mut wall = box_layer("Wanden beton");
        wall.elements = vec![(1, 0, 36)];
        let mut beam = box_layer("Balken beton");
        beam.elements = vec![(2, 0, 36)];
        for p in beam.positions.chunks_exact_mut(3) {
            p[0] += 4.0;
        }
        let between = |cut: &Cut| cut.segments.iter().filter(|[a, b]| (a[0] - 4.0).abs() < 1e-3 && (b[0] - 4.0).abs() < 1e-3).map(|[a, b]| (b[1] - a[1]).abs()).sum::<f32>();
        let mut model = model(vec![wall, beam]);
        let mut apart = cut_at(&model, Shown::all(), 1.5);
        hatch(&mut apart, &model, DrawingStandard::Nen47, 100.0);
        assert!(between(&apart) > 1.9, "not joined in the file: the line between them stays ({})", between(&apart));
        model.project.joins.insert((1, 2));
        let mut joined = cut_at(&model, Shown::all(), 1.5);
        hatch(&mut joined, &model, DrawingStandard::Nen47, 100.0);
        assert!(between(&joined) < 1e-3, "joined: the line is gone ({})", between(&joined));
        assert_eq!(joined.fills.iter().map(|f| f.element).collect::<Vec<_>>(), [Some(1), Some(2)], "the elements stay apart");
        // of another material the line stays, joined or not
        let mut brick = box_layer("Wanden metselwerk");
        brick.elements = vec![(1, 0, 36)];
        let mut beam = box_layer("Balken beton");
        beam.elements = vec![(2, 0, 36)];
        for p in beam.positions.chunks_exact_mut(3) {
            p[0] += 4.0;
        }
        let mut model = super::tests::model(vec![brick, beam]);
        model.project.joins.insert((1, 2));
        let mut mixed = cut_at(&model, Shown::all(), 1.5);
        hatch(&mut mixed, &model, DrawingStandard::Nen47, 100.0);
        assert!(between(&mixed) > 1.9, "brick against concrete: {}", between(&mixed));
    }

    #[test]
    fn a_text_names_its_material_in_four_languages() {
        for (text, kind) in [
            ("Metselwerk - rood", HatchKind::Brick),
            ("Kalkzandsteen 100", HatchKind::SandLime),
            ("Concrete, Cast-in-Place gray", HatchKind::Concrete),
            ("Stahlbeton C30/37", HatchKind::Concrete),
            ("Bois lamellé", HatchKind::Timber),
            ("Isolatie PIR 120", HatchKind::Insulation),
            ("Gipsplaat 12,5", HatchKind::Gypsum),
            ("S355", HatchKind::Steel),
        ] {
            assert_eq!(kind_in(text), Some(kind), "{text}");
        }
        assert_eq!(kind_in("Inspiration"), None, "a short code counts as a word only");
        assert_eq!(kind_in("Wanden"), None);
    }

    #[test]
    fn a_cut_box_gives_one_face_hatched_by_its_layers_name() {
        let model = model(vec![box_layer("Metselwerk")]);
        let mut cut = cut_at(&model, Shown::all(), 1.5);
        assert_eq!(cut.fills.len(), 1);
        assert_eq!((cut.fills[0].loops.len(), cut.fills[0].loops[0].len()), (1, 8), "its corners and where the diagonals of its sides cross: {:?}", cut.fills[0].loops);
        hatch(&mut cut, &model, DrawingStandard::Nen47, 100.0);
        let fill = &cut.fills[0];
        assert_eq!(fill.look, Some(([212, 144, 143], [0, 0, 0])), "brick under NEN 47");
        // 45° lines 0.08 apart across 4 × 2: the box is (4 + 2) / √2 wide across them
        let across = 6.0 / 2f32.sqrt();
        assert!((fill.rows.len() as f32 - across / 0.08).abs() <= 1.0, "{} rows", fill.rows.len());
        for (_, [a, b]) in &fill.rows {
            for p in [a, b] {
                assert!(p[0] > -1e-3 && p[0] < 4.001 && p[1] > -1e-3 && p[1] < 2.001, "{p:?} is in the box");
            }
            assert!(((b[1] - a[1]) - (b[0] - a[0])).abs() < 1e-3, "at 45°");
        }
        assert!((fill.spacing() - 0.08).abs() < 1e-3, "{}", fill.spacing());
        assert_eq!(fill.lines(1.0, 3.0).count(), fill.rows.iter().filter(|(r, _)| r.rem_euclid(3) == 0).count(), "every third row when they stand too close");
        assert_eq!(fill.lines(5.0, 3.0).count(), fill.rows.len());
    }

    #[test]
    fn concrete_is_filled_without_lines_and_a_section_has_its_face_too() {
        let model = model(vec![box_layer("Beton")]);
        let mut cut = section_at(&model, Shown::all(), [-1.0, 1.0], [5.0, 1.0], 1.0);
        hatch(&mut cut, &model, DrawingStandard::Nen47, 100.0);
        assert_eq!(cut.fills.len(), 1);
        assert_eq!(cut.fills[0].look, Some(([192, 192, 192], [192, 192, 192])));
        assert!(cut.fills[0].rows.is_empty());
        hatch(&mut cut, &model, DrawingStandard::Din1356, 100.0);
        assert!(!cut.fills[0].rows.is_empty(), "crossed in DIN 1356");
    }

    #[test]
    fn a_hole_stays_open() {
        // a square ring: 4 × 4 with a 2 × 2 hole
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let inner = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let rows = grid_lines(&[outer, inner], &Grid { offset: 0.5, ..Grid::default() }, 1.0);
        let through: Vec<_> = rows.iter().filter(|(_, [a, _])| (a[1] - 2.0).abs() < 0.01).collect();
        assert_eq!(through.len(), 2, "a line through the hole is two pieces: {through:?}");
        assert!(rows.iter().all(|(_, [a, b])| !(a[1] > 1.01 && a[1] < 2.99 && a[0].min(b[0]) < 2.0 && a[0].max(b[0]) > 2.0)), "none crosses the hole");
    }

    #[test]
    fn a_face_is_cut_back_to_the_crop() {
        let fill = Fill { loops: vec![vec![[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]]], ..Fill::default() };
        assert_eq!(fill.cropped(None).len(), 1);
        let cut = fill.cropped(Some(([1.0, -1.0], [3.0, 1.0])));
        assert_eq!(cut.len(), 1);
        let (lo, hi) = cut[0].iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
        assert_eq!((lo, hi), ([1.0, 0.0], [3.0, 1.0]));
        assert!(fill.cropped(Some(([5.0, 5.0], [6.0, 6.0]))).is_empty(), "nothing of it is in the crop");
    }

    #[test]
    fn an_open_surface_gives_no_face() {
        let mut open = box_layer("Beton");
        open.indices.truncate(30); // one side left out
        let model = model(vec![open]);
        assert!(cut_at(&model, Shown::all(), 1.5).fills.is_empty());
    }
}
