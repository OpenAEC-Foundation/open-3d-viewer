//! Drawings laid on a level as underlays: the lines of a DWG, DXF or IFCCAD drawing's model space
//! (lines, polylines with their arcs, arcs, circles, ellipses, and the blocks inserted there) in
//! metres, or the paths of a PDF drawing's first page in metres of paper (laid at a scale, 1:100
//! at first), drawn in the 3D view and in the floor plans at the level's height. A drawing far from
//! the model (in national grid coordinates, say) is laid in the middle of the model. Headless.

use glam::{DMat4, DVec3};
use opencaddocument::{Document, Dwg, Dxf, EntityType, Format, Ifcx};
use std::collections::BTreeSet;
use std::f64::consts::TAU;
use std::sync::Arc;

/// A line of an underlay: from, to (x, y, metres).
pub type Segment2 = [[f64; 2]; 2];

/// How many lines a full circle is drawn with.
const CIRCLE: f64 = 48.0;
/// The deepest blocks go inside blocks, and the most entities a drawing is read for.
const MAX_DEPTH: usize = 32;
const MAX_ENTITIES: usize = 2_000_000;
/// How far above its level an underlay is drawn (metres), so the floor does not hide it.
const ABOVE: f64 = 0.005;

/// A drawing on a level.
#[derive(Debug, Clone, PartialEq)]
pub struct Underlay {
    /// The file's name.
    pub name: String,
    /// The path of its level.
    pub level: String,
    /// Its lines in its own metres.
    pub segments: Arc<Vec<Segment2>>,
    /// Where its origin lies in the model (x, y, metres).
    pub offset: [f64; 2],
    pub shown: bool,
    /// Model metres per metre of the drawing: 1 for a CAD drawing, the scale's denominator for a
    /// PDF (its lines are in metres of paper).
    pub scale: f64,
}

/// The scales a PDF underlay can be laid at (1 : this), the first the one it starts with.
pub const PDF_SCALES: [f64; 7] = [100.0, 200.0, 500.0, 1000.0, 20.0, 50.0, 1.0];

/// Whether the drawing `name` is a PDF (laid at a scale).
pub fn is_pdf(name: &str) -> bool {
    std::path::Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// The lines of the drawing `name` (its extension says DWG, DXF or IFCCAD `.ifcx`) in metres, and
/// how many entities it left out (texts, hatches, solids and the like).
pub fn read(name: &str, bytes: &[u8]) -> Result<(Vec<Segment2>, usize), String> {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let format: &dyn Format = match ext.as_str() {
        "dwg" => &Dwg,
        "dxf" => &Dxf,
        "ifcx" => &Ifcx,
        "pdf" => return pdf(bytes).map_err(|e| format!("{name}: {e}")),
        _ => return Err(format!("{name}: an underlay is a DWG, DXF, IFCCAD (.ifcx) or PDF drawing")),
    };
    let (doc, _) = format.read(bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(segments(&doc))
}

/// The lines of a drawing's model space in metres, and how many entities were left out.
pub fn segments(doc: &Document) -> (Vec<Segment2>, usize) {
    let mut reader = Reader { doc, out: Vec::new(), left_out: 0, seen: 0 };
    for entity in doc.model_space_entities() {
        reader.entity(entity, &DMat4::IDENTITY, &mut BTreeSet::new(), 0);
    }
    let scale = metres_per_unit(doc.header.insertion_units, &reader.out);
    let lines = reader.out.into_iter().map(|[a, b]| [[a[0] * scale, a[1] * scale], [b[0] * scale, b[1] * scale]]).collect();
    (lines, reader.left_out)
}

/// Metres per unit of the drawing: its units, or for a drawing without units millimetres when it
/// is larger than a kilometre would be in metres, else metres.
fn metres_per_unit(units: i16, lines: &[Segment2]) -> f64 {
    if units != 0 {
        return crate::loaders::cad::metres_per_unit(units);
    }
    let (lo, hi) = extent(lines).unwrap_or(([0.0; 2], [0.0; 2]));
    if (hi[0] - lo[0]).max(hi[1] - lo[1]) > 1000.0 { 0.001 } else { 1.0 }
}

/// Lower left and upper right of the lines.
fn extent(lines: &[Segment2]) -> Option<([f64; 2], [f64; 2])> {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in lines.iter().flatten() {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

struct Reader<'a> {
    doc: &'a Document,
    out: Vec<Segment2>,
    left_out: usize,
    seen: usize,
}

impl Reader<'_> {
    /// A point of an entity in the drawing (its units), through the blocks it is in.
    fn at(t: &DMat4, x: f64, y: f64, z: f64) -> [f64; 2] {
        let p = t.transform_point3(DVec3::new(x, y, z));
        [p.x, p.y]
    }

    /// The lines through `points` (in the entity's own plane), closed back to the first or not.
    fn polyline(&mut self, t: &DMat4, points: &[[f64; 3]], closed: bool) {
        let n = points.len();
        let ends = if closed && n > 2 { n } else { n.saturating_sub(1) };
        for k in 0..ends {
            let (a, b) = (points[k], points[(k + 1) % n]);
            self.out.push([Self::at(t, a[0], a[1], a[2]), Self::at(t, b[0], b[1], b[2])]);
        }
    }

    /// The points of an arc round `c` of radius `r` from angle `from` turning `sweep`
    /// (radians, counter-clockwise when positive) at height `z`.
    fn arc_points(c: [f64; 2], r: f64, from: f64, sweep: f64, z: f64) -> Vec<[f64; 3]> {
        let pieces = ((sweep.abs() / TAU) * CIRCLE).ceil().max(1.0) as usize;
        (0..=pieces)
            .map(|k| {
                let a = from + sweep * k as f64 / pieces as f64;
                [c[0] + r * a.cos(), c[1] + r * a.sin(), z]
            })
            .collect()
    }

    /// The points from `p` to `q` (2D, at height `z`): straight, or round when `bulge` (the
    /// tangent of a quarter of its angle) is not 0, the last point left out.
    fn bulged(p: [f64; 2], q: [f64; 2], bulge: f64, z: f64) -> Vec<[f64; 3]> {
        let d = (q[0] - p[0]).hypot(q[1] - p[1]);
        if bulge.abs() < 1e-9 || d < 1e-12 {
            return vec![[p[0], p[1], z]];
        }
        let theta = 4.0 * bulge.atan();
        let r = d / (2.0 * (theta / 2.0).sin()).abs();
        let left = [-(q[1] - p[1]) / d, (q[0] - p[0]) / d];
        let h = (d / 2.0) / (theta / 2.0).tan();
        let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let c = [m[0] + left[0] * h, m[1] + left[1] * h];
        let from = (p[1] - c[1]).atan2(p[0] - c[0]);
        let mut points = Self::arc_points(c, r, from, theta, z);
        points.pop();
        points
    }

    /// The object coordinate system of an entity with `normal`, as a matrix.
    fn ocs(t: &DMat4, normal: opencaddocument::Vector3) -> DMat4 {
        let n = DVec3::new(normal.x, normal.y, normal.z).normalize_or(DVec3::Z);
        if (n - DVec3::Z).length() < 1e-9 {
            return *t;
        }
        // the arbitrary axis algorithm
        let x = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }.normalize();
        let y = n.cross(x).normalize();
        *t * DMat4::from_cols(x.extend(0.0), y.extend(0.0), n.extend(0.0), DVec3::ZERO.extend(1.0))
    }

    fn entity(&mut self, entity: &EntityType, t: &DMat4, blocks: &mut BTreeSet<String>, depth: usize) {
        self.seen += 1;
        if self.seen > MAX_ENTITIES {
            return;
        }
        let common = entity.as_entity().common();
        let off = self.doc.layers.get(&common.layer).is_some_and(|l| l.flags.off || l.flags.frozen);
        if common.invisible || off {
            return;
        }
        match entity {
            EntityType::Line(l) => self.out.push([Self::at(t, l.start.x, l.start.y, l.start.z), Self::at(t, l.end.x, l.end.y, l.end.z)]),
            EntityType::LwPolyline(p) => {
                let t = Self::ocs(t, p.normal);
                let n = p.vertices.len();
                let mut points = Vec::new();
                let ends = if p.is_closed { n } else { n.saturating_sub(1) };
                for k in 0..ends {
                    let (v, w) = (&p.vertices[k], &p.vertices[(k + 1) % n]);
                    points.extend(Self::bulged([v.location.x, v.location.y], [w.location.x, w.location.y], v.bulge, p.elevation));
                }
                if let Some(last) = p.vertices.get(if p.is_closed { 0 } else { n.wrapping_sub(1) }) {
                    points.push([last.location.x, last.location.y, p.elevation]);
                }
                self.polyline(&t, &points, false);
            }
            EntityType::Polyline2D(p) => {
                let t = Self::ocs(t, p.normal);
                let n = p.vertices.len();
                let closed = p.is_closed();
                let mut points = Vec::new();
                let ends = if closed { n } else { n.saturating_sub(1) };
                for k in 0..ends {
                    let (v, w) = (&p.vertices[k], &p.vertices[(k + 1) % n]);
                    points.extend(Self::bulged([v.location.x, v.location.y], [w.location.x, w.location.y], v.bulge, p.elevation));
                }
                if let Some(last) = p.vertices.get(if closed { 0 } else { n.wrapping_sub(1) }) {
                    points.push([last.location.x, last.location.y, p.elevation]);
                }
                self.polyline(&t, &points, false);
            }
            EntityType::Polyline(p) => {
                let points: Vec<[f64; 3]> = p.vertices.iter().map(|v| [v.location.x, v.location.y, v.location.z]).collect();
                self.polyline(t, &points, p.is_closed());
            }
            EntityType::Circle(c) => {
                let t = Self::ocs(t, c.normal);
                let points = Self::arc_points([c.center.x, c.center.y], c.radius, 0.0, TAU, c.center.z);
                self.polyline(&t, &points, false);
            }
            EntityType::Arc(a) => {
                let t = Self::ocs(t, a.normal);
                let mut sweep = a.end_angle - a.start_angle;
                if sweep <= 0.0 {
                    sweep += TAU;
                }
                let points = Self::arc_points([a.center.x, a.center.y], a.radius, a.start_angle, sweep, a.center.z);
                self.polyline(&t, &points, false);
            }
            EntityType::Ellipse(e) => {
                let (c, major) = (DVec3::new(e.center.x, e.center.y, e.center.z), DVec3::new(e.major_axis.x, e.major_axis.y, e.major_axis.z));
                let normal = DVec3::new(e.normal.x, e.normal.y, e.normal.z).normalize_or(DVec3::Z);
                let minor = normal.cross(major) * e.minor_axis_ratio;
                let mut sweep = e.end_parameter - e.start_parameter;
                if sweep <= 0.0 {
                    sweep += TAU;
                }
                let pieces = ((sweep / TAU) * CIRCLE).ceil().max(1.0) as usize;
                let points: Vec<[f64; 3]> = (0..=pieces)
                    .map(|k| {
                        let a = e.start_parameter + sweep * k as f64 / pieces as f64;
                        (c + major * a.cos() + minor * a.sin()).to_array()
                    })
                    .collect();
                self.polyline(t, &points, false);
            }
            EntityType::Insert(insert) => self.insert(insert, t, blocks, depth),
            _ => self.left_out += 1,
        }
    }

    /// A block inserted: its entities through the insert's placement, scale and turn (not an
    /// array or an external drawing, which are left out).
    fn insert(&mut self, insert: &opencaddocument::entities::Insert, t: &DMat4, blocks: &mut BTreeSet<String>, depth: usize) {
        let key = insert.block_name.to_uppercase();
        let doc = self.doc;
        let Some(block) = doc.block_records.get(&insert.block_name) else {
            self.left_out += 1;
            return;
        };
        if depth >= MAX_DEPTH || insert.is_minsert() || block.flags.is_xref || block.flags.is_xref_overlay || !block.xref_path.is_empty() || !blocks.insert(key.clone()) {
            self.left_out += 1;
            return;
        }
        let base = doc
            .get_entity(block.block_entity_handle)
            .and_then(|e| match e {
                EntityType::Block(b) => Some(b.base_point),
                _ => None,
            })
            .unwrap_or(block.base_point);
        let local = DMat4::from_cols_array_2d(&insert.get_transform().matrix.m).transpose() * DMat4::from_translation(-DVec3::new(base.x, base.y, base.z));
        let placed = *t * local;
        for handle in &block.entity_handles {
            if let Some(child) = doc.get_entity(*handle) {
                self.entity(child, &placed, blocks, depth + 1);
            }
        }
        blocks.remove(&key);
    }
}

/// Where a drawing's origin goes in the model: where it is, unless its lines lie far from the
/// model (more than the model's size away), then so that its middle is the model's middle.
pub fn offset(segments: &[Segment2], model: Option<([f64; 2], [f64; 2])>) -> [f64; 2] {
    let (Some((lo, hi)), Some((mlo, mhi))) = (extent(segments), model) else { return [0.0, 0.0] };
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let model_middle = [(mlo[0] + mhi[0]) / 2.0, (mlo[1] + mhi[1]) / 2.0];
    let size = (mhi[0] - mlo[0]).hypot(mhi[1] - mlo[1]).max(10.0);
    let apart = (middle[0] - model_middle[0]).hypot(middle[1] - model_middle[1]);
    if apart > size {
        [model_middle[0] - middle[0], model_middle[1] - middle[1]]
    } else {
        [0.0, 0.0]
    }
}

/// Metres per PDF point (a 72nd of an inch).
const POINT: f64 = 0.0254 / 72.0;
/// The deepest drawings (form XObjects) go inside each other.
const MAX_FORMS: usize = 16;
/// How many straight lines a Bézier curve of a PDF path is drawn with.
const CURVE: usize = 8;

/// An affine matrix of PDF: a b c d e f (x' = a x + c y + e, y' = b x + d y + f).
type Matrix = [f64; 6];

/// `m` then `n` (first `m`, then `n`), as PDF's `cm` concatenates.
fn times(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

fn apply(m: &Matrix, p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

fn number(o: &lopdf::Object) -> f64 {
    match o {
        lopdf::Object::Integer(i) => *i as f64,
        lopdf::Object::Real(r) => f64::from(*r),
        _ => 0.0,
    }
}

/// The lines of the paths of a PDF's first page (and the drawings it places, form XObjects), in
/// metres of paper, and how many texts and images it left out.
pub fn pdf(bytes: &[u8]) -> Result<(Vec<Segment2>, usize), String> {
    let doc = lopdf::Document::load_mem(bytes).map_err(|e| format!("not a PDF that can be read ({e})"))?;
    let (_, &page) = doc.get_pages().iter().next().ok_or("a PDF without pages")?;
    let content = doc.get_page_content(page);
    let (own, inherited) = doc.get_page_resources(page).map_err(|e| e.to_string())?;
    let resources = own.cloned().or_else(|| inherited.iter().find_map(|id| doc.get_dictionary(*id).ok().cloned()));
    let mut reader = PdfReader { doc: &doc, out: Vec::new(), left_out: 0 };
    reader.content(&content, resources.as_ref(), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0);
    let lines = reader.out.into_iter().map(|[a, b]| [[a[0] * POINT, a[1] * POINT], [b[0] * POINT, b[1] * POINT]]).collect();
    Ok((lines, reader.left_out))
}

struct PdfReader<'a> {
    doc: &'a lopdf::Document,
    /// Lines in points of the page.
    out: Vec<Segment2>,
    left_out: usize,
}

impl PdfReader<'_> {
    /// The operators of a content stream with `resources`, under the matrix `ctm`.
    fn content(&mut self, bytes: &[u8], resources: Option<&lopdf::Dictionary>, ctm: Matrix, depth: usize) {
        let Ok(content) = lopdf::content::Content::decode(bytes) else {
            self.left_out += 1;
            return;
        };
        let mut ctm = ctm;
        let mut saved: Vec<Matrix> = Vec::new();
        // the path being made (lines in points of the page), where it is and where it started
        let mut path: Vec<Segment2> = Vec::new();
        let (mut at, mut start) = ([0.0f64; 2], [0.0f64; 2]);
        for op in &content.operations {
            let n: Vec<f64> = op.operands.iter().map(number).collect();
            let point = |k: usize| [n.get(k).copied().unwrap_or(0.0), n.get(k + 1).copied().unwrap_or(0.0)];
            match op.operator.as_str() {
                "q" => saved.push(ctm),
                "Q" => ctm = saved.pop().unwrap_or(ctm),
                "cm" if n.len() == 6 => ctm = times(&[n[0], n[1], n[2], n[3], n[4], n[5]], &ctm),
                "m" => {
                    at = point(0);
                    start = at;
                }
                "l" => {
                    let to = point(0);
                    path.push([apply(&ctm, at), apply(&ctm, to)]);
                    at = to;
                }
                "c" | "v" | "y" => {
                    let (c1, c2, to) = match op.operator.as_str() {
                        "c" => (point(0), point(2), point(4)),
                        "v" => (at, point(0), point(2)),
                        _ => (point(0), point(2), point(2)),
                    };
                    let mut last = at;
                    for k in 1..=CURVE {
                        let t = k as f64 / CURVE as f64;
                        let s = 1.0 - t;
                        let p = [
                            s * s * s * at[0] + 3.0 * s * s * t * c1[0] + 3.0 * s * t * t * c2[0] + t * t * t * to[0],
                            s * s * s * at[1] + 3.0 * s * s * t * c1[1] + 3.0 * s * t * t * c2[1] + t * t * t * to[1],
                        ];
                        path.push([apply(&ctm, last), apply(&ctm, p)]);
                        last = p;
                    }
                    at = to;
                }
                "re" => {
                    let ([x, y], [w, h]) = (point(0), point(2));
                    let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
                    for k in 0..4 {
                        path.push([apply(&ctm, corners[k]), apply(&ctm, corners[(k + 1) % 4])]);
                    }
                    at = [x, y];
                    start = at;
                }
                "h" => {
                    path.push([apply(&ctm, at), apply(&ctm, start)]);
                    at = start;
                }
                // painted: its lines are drawn; a path only to clip with is not
                "S" | "f" | "F" | "f*" | "B" | "B*" => self.out.append(&mut path),
                "s" | "b" | "b*" => {
                    path.push([apply(&ctm, at), apply(&ctm, start)]);
                    self.out.append(&mut path);
                }
                "n" => path.clear(),
                "BT" => self.left_out += 1,
                "Do" => {
                    let name = op.operands.first().and_then(|o| o.as_name().ok());
                    self.place(name, resources, ctm, depth);
                }
                _ => {}
            }
        }
    }

    /// The drawing (form XObject) `name` of `resources` placed under `ctm`; an image is left out.
    fn place(&mut self, name: Option<&[u8]>, resources: Option<&lopdf::Dictionary>, ctm: Matrix, depth: usize) {
        let doc = self.doc;
        let found = (|| {
            let xobjects = doc.dereference(resources?.get(b"XObject").ok()?).ok()?.1.as_dict().ok()?;
            let (_, object) = doc.dereference(xobjects.get(name?).ok()?).ok()?;
            object.as_stream().ok()
        })();
        let Some(stream) = found else {
            self.left_out += 1;
            return;
        };
        if depth >= MAX_FORMS || stream.dict.get(b"Subtype").and_then(|s| s.as_name()).ok() != Some(b"Form".as_slice()) {
            self.left_out += 1;
            return;
        }
        let matrix: Matrix = match stream.dict.get(b"Matrix").and_then(|m| m.as_array()) {
            Ok(m) if m.len() == 6 => [number(&m[0]), number(&m[1]), number(&m[2]), number(&m[3]), number(&m[4]), number(&m[5])],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        let own = stream.dict.get(b"Resources").ok().and_then(|r| doc.dereference(r).ok()).and_then(|(_, r)| r.as_dict().ok()).cloned();
        let bytes = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
        self.content(&bytes, own.as_ref().or(resources), times(&matrix, &ctm), depth + 1);
    }
}

/// The underlay's lines in the model at height `z` (a hair above it, so the floor does not hide
/// them).
pub fn world(u: &Underlay, z: f64) -> Vec<[[f32; 3]; 2]> {
    let at = |p: [f64; 2]| [(p[0] * u.scale + u.offset[0]) as f32, (p[1] * u.scale + u.offset[1]) as f32, (z + ABOVE) as f32];
    u.segments.iter().map(|[a, b]| [at(*a), at(*b)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencaddocument::entities::{Arc as CadArc, Circle, Insert, Line, LwPolyline, LwVertex, Text};
    use opencaddocument::{EntityType, Format, Vector2, Vector3};

    /// A drawing in millimetres.
    fn drawing() -> Document {
        let mut doc = Document::new();
        doc.header.insertion_units = 4;
        doc
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn lines_polylines_and_circles_become_lines_in_metres() {
        let mut doc = drawing();
        doc.add_entity(EntityType::Line(Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1000.0, 0.0, 0.0)))).unwrap();
        let mut square = LwPolyline::new();
        for (x, y) in [(0.0, 0.0), (2000.0, 0.0), (2000.0, 2000.0), (0.0, 2000.0)] {
            square.vertices.push(LwVertex::new(Vector2::new(x, y)));
        }
        square.is_closed = true;
        doc.add_entity(EntityType::LwPolyline(square)).unwrap();
        let mut circle = Circle::new();
        circle.center = Vector3::new(5000.0, 5000.0, 0.0);
        circle.radius = 500.0;
        doc.add_entity(EntityType::Circle(circle)).unwrap();
        doc.add_entity(EntityType::Text(Text::with_value("Kamer", Vector3::new(0.0, 0.0, 0.0)))).unwrap();
        let (lines, left_out) = segments(&doc);
        assert!(lines.iter().any(|s| near(s[0], [0.0, 0.0]) && near(s[1], [1.0, 0.0])), "{lines:?}");
        // the square, closed: four sides
        assert!(lines.iter().any(|s| near(s[0], [0.0, 2.0]) && near(s[1], [0.0, 0.0])), "{lines:?}");
        // the circle: short lines all on it, round
        let round: Vec<&Segment2> = lines.iter().filter(|s| ((s[0][0] - 5.0).hypot(s[0][1] - 5.0) - 0.5).abs() < 1e-9).collect();
        assert!(round.len() >= 24, "{} pieces", round.len());
        assert_eq!(left_out, 1, "the text");
    }

    #[test]
    fn an_arc_and_a_bulge_are_drawn_round() {
        let mut doc = drawing();
        let mut arc = CadArc::new();
        arc.center = Vector3::new(0.0, 0.0, 0.0);
        arc.radius = 1000.0;
        arc.start_angle = 0.0;
        arc.end_angle = std::f64::consts::FRAC_PI_2;
        doc.add_entity(EntityType::Arc(arc)).unwrap();
        // a half circle from (3, 0) to (5, 0) through (4, -1): bulge 1, counter-clockwise
        let mut half = LwPolyline::new();
        let mut a = LwVertex::new(Vector2::new(3000.0, 0.0));
        a.bulge = 1.0;
        half.vertices.push(a);
        half.vertices.push(LwVertex::new(Vector2::new(5000.0, 0.0)));
        doc.add_entity(EntityType::LwPolyline(half)).unwrap();
        let (lines, _) = segments(&doc);
        let ends: Vec<[f64; 2]> = lines.iter().flat_map(|s| *s).collect();
        assert!(ends.iter().any(|p| near(*p, [1.0, 0.0])) && ends.iter().any(|p| near(*p, [0.0, 1.0])), "the arc's ends");
        assert!(ends.iter().filter(|p| p[0] < 2.0).all(|p| (p[0].hypot(p[1]) - 1.0).abs() < 1e-9), "on the arc");
        let lowest = ends.iter().filter(|p| p[0] > 2.0).map(|p| p[1]).fold(f64::INFINITY, f64::min);
        assert!((lowest + 1.0).abs() < 1e-6, "through (4, -1): {lowest}");
    }

    #[test]
    fn a_block_is_placed_scaled_and_turned() {
        let mut doc = drawing();
        let mut record = opencaddocument::tables::BlockRecord::new("door");
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1000.0, 0.0, 0.0));
        line.common.owner_handle = owner;
        doc.add_entity(EntityType::Line(line)).unwrap();
        let mut insert = Insert::new("door", Vector3::new(5000.0, 0.0, 0.0));
        insert.rotation = std::f64::consts::FRAC_PI_2;
        insert.set_x_scale(2.0);
        insert.set_y_scale(2.0);
        doc.add_entity(EntityType::Insert(insert)).unwrap();
        let (lines, _) = segments(&doc);
        assert_eq!(lines.len(), 1, "the block's line, once: {lines:?}");
        assert!(near(lines[0][0], [5.0, 0.0]) && near(lines[0][1], [5.0, 2.0]), "{lines:?}");
    }

    #[test]
    fn a_dxf_is_read_by_its_extension() {
        let mut doc = drawing();
        doc.add_entity(EntityType::Line(Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1000.0, 0.0, 0.0)))).unwrap();
        let (bytes, _) = opencaddocument::Dxf.write(&doc).unwrap();
        let (lines, _) = read("plan.DXF", &bytes).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(read("plan.txt", &bytes).is_err());
    }

    #[test]
    fn a_drawing_far_away_is_laid_in_the_middle_of_the_model() {
        let model = Some(([0.0, 0.0], [20.0, 10.0]));
        // in national grid coordinates: 155 km east, 463 km north
        let far = [[[155_000.0, 463_000.0], [155_010.0, 463_000.0]], [[155_000.0, 463_004.0], [155_010.0, 463_004.0]]];
        let o = offset(&far, model);
        assert!(near([155_005.0 + o[0], 463_002.0 + o[1]], [10.0, 5.0]), "{o:?}");
        // near the model: where it is
        assert_eq!(offset(&[[[1.0, 1.0], [5.0, 1.0]]], model), [0.0, 0.0]);
        assert_eq!(offset(&far, None), [0.0, 0.0]);
    }

    /// A PDF of one page with this content, a drawing (form XObject) `/F1` moved 10 points right
    /// holding a line of 10 points, and a text.
    fn pdf_with(content: &[u8]) -> Vec<u8> {
        use lopdf::{dictionary, Object, Stream};
        let mut doc = lopdf::Document::with_version("1.5");
        let pages = doc.new_object_id();
        let form = Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 10.into(), 0.into()] },
            b"0 0 m 10 0 l S BT /F 12 Tf (x) Tj ET".to_vec(),
        );
        let form = doc.add_object(form);
        let resources = doc.add_object(dictionary! { "XObject" => dictionary! { "F1" => form } });
        let contents = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Contents" => contents, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()] });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn a_pdfs_paths_become_lines_in_metres_of_paper() {
        let bytes = pdf_with(b"q 2 0 0 2 0 0 cm 0 0 m 72 0 l S Q 0 0 72 36 re S 0 0 m 0 72 72 72 72 0 c S 0 0 m 1 1 l W n /F1 Do");
        let (lines, left_out) = read("plan.PDF", &bytes).unwrap();
        let inch = 0.0254;
        // the line drawn at twice the size: two inches
        assert!(lines.iter().any(|s| near(s[0], [0.0, 0.0]) && near(s[1], [2.0 * inch, 0.0])), "{lines:?}");
        // the rectangle, one inch by half an inch
        assert!(lines.iter().any(|s| near(s[0], [inch, 0.0]) && near(s[1], [inch, inch / 2.0])), "{lines:?}");
        // the curve in eight pieces, ending at one inch
        let curve: Vec<&Segment2> = lines.iter().filter(|s| s[0][1] > 1e-9 || s[1][1] > inch * 0.6).collect();
        assert!(curve.len() >= 8, "{curve:?}");
        assert!(lines.iter().any(|s| near(s[1], [inch, 0.0]) && s[0][1] > 0.0), "the curve's end");
        // the clipping path is not drawn; the placed drawing's line is, 10 points right
        assert!(!lines.iter().any(|s| near(s[1], [inch / 72.0, inch / 72.0])), "{lines:?}");
        assert!(lines.iter().any(|s| near(s[0], [10.0 * inch / 72.0, 0.0]) && near(s[1], [20.0 * inch / 72.0, 0.0])), "{lines:?}");
        assert_eq!(left_out, 1, "the text in the placed drawing");
    }

    #[test]
    fn a_pdf_the_viewer_exported_reads_back_at_paper_size() {
        use crate::drawing2d::{Drawing2d, Line2d, Pen, Space};
        let sheet = Drawing2d {
            name: "A01".into(),
            space: Space::Paper,
            paper: Some([0.42, 0.297]),
            lines: vec![Line2d { pen: Pen::Cut, a: [0.05, 0.05], b: [0.30, 0.05] }],
            texts: Vec::new(),
            dimensions: Vec::new(),
        };
        let (lines, _) = read("sheet.pdf", &crate::export::pdf::pdf(&sheet)).unwrap();
        assert!(lines.iter().any(|s| (s[0][0] - 0.05).abs() < 1e-4 && (s[1][0] - 0.30).abs() < 1e-4 && (s[0][1] - 0.05).abs() < 1e-4), "{lines:?}");
        assert!(is_pdf("sheet.pdf") && !is_pdf("plan.dwg"));
    }

    #[test]
    fn a_pdf_underlay_is_laid_at_its_scale() {
        let u = Underlay { name: "plan.pdf".into(), level: "l".into(), segments: Arc::new(vec![[[0.0, 0.0], [0.08, 0.0]]]), offset: [0.0, 0.0], shown: true, scale: 100.0 };
        let w = world(&u, 0.0);
        // 8 cm of paper at 1:100: 8 m
        assert!((w[0][1][0] - 8.0).abs() < 1e-5, "{w:?}");
    }

    #[test]
    fn an_underlay_lies_at_its_levels_height() {
        let u = Underlay { name: "plan.dwg".into(), level: "l".into(), segments: Arc::new(vec![[[0.0, 0.0], [1.0, 0.0]]]), offset: [2.0, 3.0], shown: true, scale: 1.0 };
        let w = world(&u, 3.0);
        assert_eq!(w.len(), 1);
        assert_eq!((w[0][0][0], w[0][0][1], w[0][1][0]), (2.0, 3.0, 3.0));
        assert!(w[0][0][2] > 3.0 && w[0][0][2] < 3.01, "{:?}", w[0]);
    }
}
