//! The hatches of a drawing's cut faces: what an element is made of, read from its properties or
//! its layer's name, gives the hatch of the drawing standard, and that hatch becomes lines inside
//! the face's outlines, in the drawing's own metres. Headless, like the drawing.

use crate::drawing::{Cut, Fill, Segment};
use crate::model::Model;
use bim_library::{DrawingStandard, HatchKind, HatchPattern};

/// A face takes at most this many hatch lines per direction; a larger one keeps its fill only.
const MOST_ROWS: i32 = 4000;
/// Insulation's zigzag, which wants the layer's own direction, is drawn in a free outline as
/// crossed lines this far apart (metres), at 60° and 120°.
const INSULATION_SPACING: f64 = 0.06;

/// Hatches the cut faces of `cut` by what the model says its elements are made of.
pub fn hatch(cut: &mut Cut, model: &Model, standard: DrawingStandard) {
    for fill in &mut cut.fills {
        let pattern = kind_of(model, fill.layer, fill.element).pattern(standard);
        fill.look = Some((pattern.fill, pattern.line));
        fill.rows = rows(&fill.loops, pattern);
    }
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

/// The lines of `pattern` inside `loops`, each with its row.
pub fn rows(loops: &[Vec<[f32; 2]>], pattern: HatchPattern) -> Vec<(i32, Segment)> {
    let (angles, spacing): (Vec<f64>, f64) = if pattern.zigzag {
        (vec![60.0, 120.0], INSULATION_SPACING)
    } else if pattern.crossed {
        (vec![pattern.angle, pattern.angle + 90.0], pattern.spacing)
    } else {
        (vec![pattern.angle], pattern.spacing)
    };
    if spacing <= 0.0 {
        return Vec::new();
    }
    angles.into_iter().flat_map(|angle| lines(loops, angle, spacing)).collect()
}

/// The lines at `angle` (degrees, anticlockwise from x), `spacing` apart, inside `loops` by the
/// even-odd rule. Rows count from the drawing's origin, so the hatches of faces side by side
/// run on.
fn lines(loops: &[Vec<[f32; 2]>], angle: f64, spacing: f64) -> Vec<(i32, Segment)> {
    let (sin, cos) = angle.to_radians().sin_cos();
    // along the lines and across them
    let turned = |p: [f32; 2]| [p[0] as f64 * cos + p[1] as f64 * sin, -(p[0] as f64) * sin + p[1] as f64 * cos];
    let back = |q: [f64; 2]| [(q[0] * cos - q[1] * sin) as f32, (q[0] * sin + q[1] * cos) as f32];
    let edges: Vec<[[f64; 2]; 2]> = loops
        .iter()
        .flat_map(|l| (0..l.len()).map(move |k| [l[k], l[(k + 1) % l.len()]]))
        .map(|[a, b]| [turned(a), turned(b)])
        .collect();
    let (low, high) = edges.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    if !(high > low) {
        return Vec::new();
    }
    let (first, last) = ((low / spacing).ceil() as i64, (high / spacing).floor() as i64);
    if last - first >= i64::from(MOST_ROWS) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for row in first..=last {
        // half a hair off the row, so a line never runs along an edge or through a corner
        let v = row as f64 * spacing + spacing * 1e-4;
        let mut crossings: Vec<f64> = edges
            .iter()
            .filter(|[a, b]| (a[1] > v) != (b[1] > v))
            .map(|[a, b]| a[0] + (v - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
            .collect();
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.chunks_exact(2) {
            if pair[1] - pair[0] > 1e-6 {
                out.push((row as i32, [back([pair[0], v]), back([pair[1], v])]));
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
        let mut rows = self.rows.iter();
        let Some((row, [a, b])) = rows.next() else { return 0.0 };
        // the next row of the same direction: its distance across the lines
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let run = dx.hypot(dy).max(1e-9);
        rows.find(|(r, [c, d])| r != row && ((d[0] - c[0]) * dy - (d[1] - c[1]) * dx).abs() < 1e-3 * run)
            .map_or(0.0, |(r, [c, _])| (((c[0] - a[0]) * dy - (c[1] - a[1]) * dx) / run).abs() / (r - row).unsigned_abs() as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        hatch(&mut cut, &model, DrawingStandard::Nen47);
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
        hatch(&mut cut, &model, DrawingStandard::Nen47);
        assert_eq!(cut.fills.len(), 1);
        assert_eq!(cut.fills[0].look, Some(([192, 192, 192], [192, 192, 192])));
        assert!(cut.fills[0].rows.is_empty());
        hatch(&mut cut, &model, DrawingStandard::Din1356);
        assert!(!cut.fills[0].rows.is_empty(), "crossed in DIN 1356");
    }

    #[test]
    fn a_hole_stays_open() {
        // a square ring: 4 × 4 with a 2 × 2 hole
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let inner = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let rows = lines(&[outer, inner], 0.0, 0.5);
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
