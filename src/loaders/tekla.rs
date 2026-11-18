//! Adapter from the independent tekla-read file reader to the app's scene model: the parts as
//! layers, the grids as the levels and grid lines of the project, the saved model views, and the
//! drawings of the model folder as sheets.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{DraftFill, DraftText, GridLine, Layer, PlanRange, ProjectData, SavedView, Sheet, SheetRect, ViewCamera, ViewKind};
use crate::section::SectionBox;
use glam::Vec3;
use tekla_read::drawings::{Drawing, Shape, Snapshot};
use tekla_read::{Grid, ModelView};

pub struct TeklaLoader;

/// Tekla is in millimetres; the scene in metres.
const MM: f32 = 0.001;
/// The height of a text's capitals to the size of its letters.
const CAPITALS: f32 = 0.72;

impl Loader for TeklaLoader {
    fn name(&self) -> &'static str { "Tekla Structures" }
    fn extensions(&self) -> &'static [&'static str] { &["db1"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        // Native projects keep the profile and bolt catalogues beside their DB1, and their
        // drawings in its folder.
        let folder = source.path().and_then(|p| p.parent()).map(std::path::Path::to_path_buf);
        let mut document = if let Some(path) = source.path() {
            tekla_read::read_path(path)?
        } else {
            tekla_read::read(&source.read()?)?
        };
        progress.set(Phase::Convert);
        let mut project = ProjectData::default();
        project.info.push(("Tekla database".into(), std::mem::take(&mut document.database_version)));
        if let Some(guid) = document.guid.take() { project.info.push(("GUID".into(), guid)); }
        project.info.append(&mut document.info);
        let (levels, grids) = levels_and_grids(&document.grids);
        project.levels = levels;
        project.grids = grids;
        let views: Vec<SavedView> = document.views.iter().filter_map(|v| saved_view(v, &project.levels)).collect();
        if !views.is_empty() {
            project.info.push(("Tekla model views".into(), views.len().to_string()));
        }
        if !document.drawings.is_empty() {
            match &folder {
                Some(folder) => {
                    let drawings = tekla_read::drawings::read_folder(folder, &document.drawings);
                    let shown = drawings.iter().filter(|d| d.snapshot.is_some()).count();
                    project.info.push(("Tekla drawings".into(), drawings.len().to_string()));
                    if shown < drawings.len() {
                        project.info.push(("Tekla drawings without a snapshot".into(), (drawings.len() - shown).to_string()));
                    }
                    project.sheets = sheets(drawings);
                }
                None => project.info.push(("Tekla drawings".into(), format!("{} (they are read from the model folder, on the desktop)", document.drawings.len()))),
            }
        }
        let layers = document.meshes.into_iter().map(|mesh| Layer {
            name: mesh.name, color: mesh.color, positions: mesh.positions, normals: mesh.normals, colors: mesh.colors, indices: mesh.indices, elements: mesh.elements, props: mesh.props, hidden: mesh.hidden, points: false, lines: Vec::new(), line_elements: Vec::new(), splats: None
        }).collect();
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views, project })
    }
}

/// The levels of the grids (their Z lines, by label, low to high) and their X and Y lines, drawn
/// at each grid's lowest level and extended past the outer lines as the grid says.
fn levels_and_grids(grids: &[Grid]) -> (Vec<(String, f32)>, Vec<GridLine>) {
    let mut levels: Vec<(String, f32)> = Vec::new();
    let mut lines = Vec::new();
    for g in grids {
        let o = g.origin_mm;
        for (name, z) in &g.z {
            let z = ((o[2] + z) as f32) * MM;
            if !levels.iter().any(|(n, h)| n == name && (h - z).abs() < 0.001) {
                levels.push((name.clone(), z));
            }
        }
        let low = g.z.iter().map(|l| l.1).fold(f64::INFINITY, f64::min);
        let z = ((o[2] + if low.is_finite() { low } else { 0.0 }) as f32) * MM;
        let span = |axis: &[(String, f64)]| {
            let lo = axis.iter().map(|l| l.1).fold(f64::INFINITY, f64::min);
            let hi = axis.iter().map(|l| l.1).fold(f64::NEG_INFINITY, f64::max);
            (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
        };
        let e = g.extensions_mm;
        // a line of the X axis runs along Y, past the first and last Y line by the X extensions
        if let Some((y0, y1)) = span(&g.y) {
            for (name, x) in &g.x {
                let x = o[0] + x;
                lines.push(GridLine { name: name.clone(), a: [x as f32 * MM, (o[1] + y0 - e[0]) as f32 * MM, z], b: [x as f32 * MM, (o[1] + y1 + e[1]) as f32 * MM, z] });
            }
        }
        if let Some((x0, x1)) = span(&g.x) {
            for (name, y) in &g.y {
                let y = o[1] + y;
                lines.push(GridLine { name: name.clone(), a: [(o[0] + x0 - e[2]) as f32 * MM, y as f32 * MM, z], b: [(o[0] + x1 + e[3]) as f32 * MM, y as f32 * MM, z] });
            }
        }
    }
    levels.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    (levels, lines)
}

/// A saved model view as the viewer shows it. Its frame (axes and origin) and work area say what
/// it shows, within its depth up toward the viewer and down away from it; how it is turned on
/// screen says its kind: a plan looks straight down, a section or elevation level, a 3D view
/// anywhere else.
fn saved_view(v: &ModelView, levels: &[(String, f32)]) -> Option<SavedView> {
    let a = |p: [f64; 3]| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let basis = [a(v.frame[0]), a(v.frame[1]), a(v.frame[2])];
    let origin = a(v.frame[3]) * MM;
    let toward = a(v.display[2]).normalize_or_zero();
    if toward == Vec3::ZERO {
        return None;
    }
    let (lo, hi) = (a(v.work_area_mm[0]) * MM, a(v.work_area_mm[1]) * MM);
    // along the frame's z: the work area within the view's depth; where the two do not meet, the
    // work area alone
    let (mut z0, mut z1) = (lo.z.max(-(v.depth_down_mm as f32) * MM), hi.z.min(v.depth_up_mm as f32 * MM));
    if !(z0 <= z1) {
        (z0, z1) = (lo.z, hi.z);
    }
    if !(z0 <= z1) {
        return None;
    }
    let (min, max) = (Vec3::new(lo.x, lo.y, z0), Vec3::new(hi.x, hi.y, z1));
    let clip = SectionBox::from_frame(origin, basis, min, max);
    let name = v.name.trim().to_string();
    let mut out = SavedView { folder: Vec::new(), style: None, drawing: None, name, kind: ViewKind::ThreeD, camera: None, clip: None, plan: None, level: None };
    // the middle of what it shows, in the world
    let middle = origin + basis[0] * (min.x + max.x) / 2.0 + basis[1] * (min.y + max.y) / 2.0 + basis[2] * (min.z + max.z) / 2.0;
    if toward.z > 0.999 && basis[2].z.abs() > 0.999 {
        // a plan: the heights of its depth about its plane
        let plane = origin.z;
        let (low, high) = if basis[2].z > 0.0 { (plane + z0, plane + z1) } else { (plane - z1, plane - z0) };
        out.kind = ViewKind::Plan;
        out.plan = Some(PlanRange { low, high, from_below: false });
        out.clip = clip;
        // its level: the one of its plane when the plane lies in what it shows; else (a view
        // made through the Open API keeps its height in its work area alone, its plane at 0)
        // the level in what it shows nearest the middle of it
        let within = |h: f32| h >= low - 0.005 && h <= high + 0.005;
        let mid = (low + high) / 2.0;
        out.level = levels
            .iter()
            .find(|(_, h)| (h - plane).abs() < 0.005 && within(*h))
            .or_else(|| levels.iter().filter(|(_, h)| within(*h)).min_by(|a, b| (a.1 - mid).abs().total_cmp(&(b.1 - mid).abs())))
            .map(|(n, _)| n.clone());
    } else if toward.z.abs() < 0.001 {
        // a section along a grid line: from in front of its depth, looking into it
        let depth = (z1 - z0).max(0.0);
        out.kind = ViewKind::Section;
        out.camera = Some(ViewCamera { eye: (middle + toward * (depth / 2.0 + 1.0)).to_array(), forward: (-toward).to_array(), perspective: false, fov_y: None });
        out.clip = clip;
    } else {
        // a 3D view: from far enough out along how it is turned to see its work area whole
        let reach = (max - min).length().max(1.0);
        out.camera = Some(ViewCamera { eye: (middle + toward * reach * 1.5).to_array(), forward: (-toward).to_array(), perspective: false, fov_y: None });
    }
    Some(out)
}

/// The drawings as sheets: general arrangements first, then assembly, cast unit, single-part
/// and multidrawings, each kind by mark.
fn sheets(mut drawings: Vec<Drawing>) -> Vec<Sheet> {
    let order = |k: char| "GACWM".find(k).unwrap_or(5);
    drawings.sort_by(|a, b| order(a.record.kind).cmp(&order(b.record.kind)).then_with(|| natural(&a.record.mark).cmp(&natural(&b.record.mark))));
    drawings.iter().map(sheet).collect()
}

/// Sorts "P2" before "P10".
fn natural(s: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((std::mem::take(&mut text), digits.parse().unwrap_or(u64::MAX)));
                digits.clear();
            }
            text.push(c);
        }
    }
    out.push((text, digits.parse().unwrap_or(0)));
    out
}

/// One drawing as a sheet: its paper, and what its snapshot draws on it as the sheet's own lines,
/// texts and fills (sheet metres, the paper's lower left corner at the origin). A drawing without
/// a snapshot is its paper and title only.
fn sheet(d: &Drawing) -> Sheet {
    let r = &d.record;
    let number = r.mark.trim().trim_start_matches('[').trim_end_matches(']').trim().to_string();
    let field = |k: &str| d.metadata.as_ref().and_then(|m| m.get(k)).and_then(|v| v.parse::<f32>().ok()).filter(|v| *v > 0.0);
    let size = d.snapshot.as_ref().map(|s| s.paper_mm.map(|v| v as f32)).or_else(|| Some([field("Width")?, field("Height")?]));
    let paper = size.map(|[w, h]| SheetRect { min: [0.0, 0.0], max: [w * MM, h * MM] });
    let mut sheet = Sheet::new(number, r.name.clone(), paper, Vec::new());
    if let Some(s) = &d.snapshot {
        draw(&mut sheet, s);
    }
    sheet
}

/// What a snapshot draws, onto a sheet.
fn draw(sheet: &mut Sheet, s: &Snapshot) {
    let p = |q: [f64; 2]| [q[0] as f32 * MM, q[1] as f32 * MM];
    let mut colors: Vec<Option<[u8; 3]>> = Vec::new();
    for item in &s.items {
        let mut line = |a: [f32; 2], b: [f32; 2]| {
            sheet.lines.push([a, b]);
            colors.push(item.colour);
        };
        match &item.shape {
            Shape::Line { from, to } => line(p(*from), p(*to)),
            Shape::Arc { from, to, bulge } => {
                let points = arc(*from, *to, *bulge);
                for w in points.windows(2) {
                    line(p(w[0]), p(w[1]));
                }
            }
            Shape::Circle { center, radius } => {
                let n = segments(*radius, std::f64::consts::TAU);
                let at = |k: usize| {
                    let t = std::f64::consts::TAU * k as f64 / n as f64;
                    p([center[0] + radius * t.cos(), center[1] + radius * t.sin()])
                };
                for k in 0..n {
                    line(at(k), at(k + 1));
                }
            }
            Shape::Polygon { edges, fill } => {
                let mut outline: Vec<[f32; 2]> = Vec::new();
                for &(from, to, bulge) in edges {
                    let points = arc(from, to, bulge);
                    for w in points.windows(2) {
                        line(p(w[0]), p(w[1]));
                    }
                    outline.extend(points.iter().skip(usize::from(!outline.is_empty())).map(|&q| p(q)));
                }
                if let Some(color) = fill.filter(|_| outline.len() >= 3) {
                    sheet.fills.push(DraftFill { element: u64::from(item.object), loops: vec![outline], color });
                }
            }
            Shape::Symbol { at, angle, scale, file, .. } => {
                // the dimension arrows: an open arrowhead pointing its way, 2.5 mm long; other
                // symbols (their files are Tekla's) are left out
                if file.to_ascii_lowercase().contains("arrow") {
                    let (len, wide) = (2.5 * scale[0].abs().max(0.1), 0.8 * scale[1].abs().max(0.1));
                    let (c, s_) = (angle.cos(), angle.sin());
                    let back = |side: f64| p([at[0] - len * c - side * wide * s_, at[1] - len * s_ + side * wide * c]);
                    line(back(1.0), p(*at));
                    line(p(*at), back(-1.0));
                }
            }
            Shape::Text { at, angle, height, width, text, font } => {
                if text.trim().is_empty() || !(*height > 0.0) {
                    continue;
                }
                let capitals = *height as f32 * MM;
                let size = capitals / CAPITALS;
                let mut face = crate::typeface::Face { color: item.colour, ..Default::default() };
                if !font.is_empty() {
                    face.family = font.as_str().into();
                }
                // its letters narrower or wider than the font draws them
                let wide = (*width / *height) as f32;
                if (wide - 1.0).abs() > 0.05 && (0.2..=3.0).contains(&wide) {
                    face.width = wide;
                }
                sheet.texts.push(DraftText {
                    element: u64::from(item.object),
                    at: p(*at),
                    width: text.chars().count() as f32 * size * 0.55 * face.width,
                    height: size,
                    size,
                    align: 0,
                    angle: *angle as f32,
                    text: text.clone(),
                    face,
                });
            }
            Shape::Image { .. } => {}
        }
    }
    // a colour per line only where one has its own
    sheet.line_colors = if colors.iter().any(Option::is_some) { colors } else { Vec::new() };
}

/// The points of an arc from `a` to `b` whose height over its chord is `bulge` times the chord,
/// to the right of the way from `a` to `b` when positive; a straight edge without bulge.
fn arc(a: [f64; 2], b: [f64; 2], bulge: f64) -> Vec<[f64; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = dx.hypot(dy);
    if bulge.abs() < 1e-6 || chord < 1e-9 {
        return vec![a, b];
    }
    // the height over the chord, and the radius through both ends and the top
    let h = bulge.abs() * chord;
    let radius = (chord * chord / 4.0 + h * h) / (2.0 * h);
    // the right of the way is (dy, -dx); the top lies on that side for a positive bulge
    let side = bulge.signum();
    let (nx, ny) = (dy / chord * side, -dx / chord * side);
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    // the centre lies from the top back toward the chord by the radius
    let top = [mid[0] + nx * h, mid[1] + ny * h];
    let center = [top[0] - nx * radius, top[1] - ny * radius];
    let start = (a[1] - center[1]).atan2(a[0] - center[0]);
    let end = (b[1] - center[1]).atan2(b[0] - center[0]);
    // the way round that passes the top
    let through = (top[1] - center[1]).atan2(top[0] - center[0]);
    let tau = std::f64::consts::TAU;
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(tau);
    let sweep = if ccw(start, through) <= ccw(start, end) { ccw(start, end) } else { ccw(start, end) - tau };
    let n = segments(radius, sweep.abs());
    // its ends exactly where the edge has them, so the pieces of an outline meet
    (0..=n)
        .map(|k| match k {
            0 => a,
            k if k == n => b,
            k => {
                let t = start + sweep * k as f64 / n as f64;
                [center[0] + radius * t.cos(), center[1] + radius * t.sin()]
            }
        })
        .collect()
}

/// How many straight pieces draw an arc of `radius` mm through `sweep` radians: about one per
/// half millimetre of paper, at least 2 and at most 64, and even, so its middle is a point of it.
fn segments(radius: f64, sweep: f64) -> usize {
    let n = ((radius * sweep / 0.5).ceil() as usize).clamp(2, 64);
    n + n % 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use tekla_read::drawings::{Item, Metadata, SnapshotView};
    use tekla_read::DrawingRecord;

    fn grid() -> Grid {
        Grid {
            id: 1,
            origin_mm: [1000.0, 0.0, -500.0],
            x: vec![("A".into(), 0.0), ("B".into(), 6000.0)],
            y: vec![("1".into(), 0.0), ("2".into(), 4000.0)],
            z: vec![("+0".into(), 500.0), ("+3600".into(), 4100.0)],
            extensions_mm: [1000.0, 2000.0, 500.0, 500.0, 0.0, 0.0],
        }
    }

    #[test]
    fn the_z_lines_of_a_grid_are_its_levels_and_its_x_and_y_lines_its_grid_lines() {
        let (levels, lines) = levels_and_grids(&[grid(), grid()]);
        // a level two grids share is one level
        assert_eq!(levels.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["+0", "+3600"]);
        assert!(levels[0].1.abs() < 1e-6 && (levels[1].1 - 3.6).abs() < 1e-6, "{levels:?}");
        assert_eq!(lines.len(), 8);
        // line A along Y at x 1 m, past line 1 by 1 m and past line 2 by 2 m, at the lowest level
        let is = |l: &GridLine, name: &str, a: [f32; 3], b: [f32; 3]| l.name == name && (0..3).all(|k| (l.a[k] - a[k]).abs() < 1e-5 && (l.b[k] - b[k]).abs() < 1e-5);
        assert!(is(&lines[0], "A", [1.0, -1.0, 0.0], [1.0, 6.0, 0.0]), "{:?}", lines[0]);
        assert!(is(&lines[3], "2", [0.5, 4.0, 0.0], [7.5, 4.0, 0.0]), "{:?}", lines[3]);
    }

    fn view(name: &str, frame: [[f64; 3]; 4], display: [[f64; 3]; 3], depth: [f64; 2], work: [[f64; 3]; 2]) -> ModelView {
        ModelView { id: 1, name: name.into(), frame, display, depth_up_mm: depth[0], depth_down_mm: depth[1], work_area_mm: work, angles_deg: [0.0, 0.0], properties: "standard".into() }
    }

    const AXES: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn a_view_seen_from_above_is_a_plan_within_its_depth_at_its_level() {
        let levels = [("+3600".to_string(), 3.6)];
        let v = view(" PLAN +3600", [AXES[0], AXES[1], AXES[2], [0.0, 0.0, 3600.0]], AXES, [1100.0, 700.0], [[-2000.0, -2000.0, -300.0], [18000.0, 10000.0, 3000.0]]);
        let s = saved_view(&v, &levels).unwrap();
        assert_eq!((s.name.as_str(), s.kind, s.level.as_deref()), ("PLAN +3600", ViewKind::Plan, Some("+3600")));
        // down to the work area's bottom (above its depth down), up to its depth up
        let r = s.plan.unwrap();
        assert!((r.low - 3.3).abs() < 1e-5 && (r.high - 4.7).abs() < 1e-5, "{r:?}");
        let clip = s.clip.unwrap();
        assert!((clip.min.x + 2.0).abs() < 1e-5 && (clip.max.y - 10.0).abs() < 1e-5);
    }

    /// A plan made through the Open API keeps its plane at 0 and its height in its work area: its
    /// level is the one in what it shows, not the one at 0.
    #[test]
    fn a_plan_whose_height_is_its_work_area_gets_the_level_in_it() {
        let levels = [("+0".to_string(), 0.0), ("+4200".to_string(), 4.2), ("+8400".to_string(), 8.4)];
        let v = view("PLAN +8400", [AXES[0], AXES[1], AXES[2], [0.0; 3]], AXES, [500.0, 500.0], [[-3000.0, -3000.0, 7900.0], [30000.0, 18000.0, 8900.0]]);
        let s = saved_view(&v, &levels).unwrap();
        assert_eq!((s.kind, s.level.as_deref()), (ViewKind::Plan, Some("+8400")));
        let r = s.plan.unwrap();
        assert!((r.low - 7.9).abs() < 1e-5 && (r.high - 8.9).abs() < 1e-5, "{r:?}");
        // the plan at +0 keeps the level of its plane
        let v = view("PLAN +0", [AXES[0], AXES[1], AXES[2], [0.0; 3]], AXES, [1000.0, 1000.0], [[-3000.0, -3000.0, -1000.0], [30000.0, 18000.0, 1000.0]]);
        assert_eq!(saved_view(&v, &levels).unwrap().level.as_deref(), Some("+0"));
    }

    #[test]
    fn a_level_view_is_a_section_looking_into_its_depth() {
        // along grid line B: the screen's x along Y, up along Z, toward the viewer along X
        let frame = [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [6000.0, 0.0, 0.0]];
        let v = view("GRID B", frame, [frame[0], frame[1], frame[2]], [900.0, 800.0], [[-1500.0, -600.0, -500.0], [10000.0, 7400.0, 500.0]]);
        let s = saved_view(&v, &[]).unwrap();
        assert_eq!(s.kind, ViewKind::Section);
        let camera = s.camera.unwrap();
        assert_eq!(camera.forward, [-1.0, 0.0, 0.0]);
        // in front of its depth (x 6.5 m), looking toward −X
        assert!(camera.eye[0] > 6.5, "{camera:?}");
        // its box: 0.5 m either side of the line in x, the work area along it and up
        let clip = s.clip.unwrap();
        let corners: Vec<Vec3> = (0..8).map(|k| clip.to_world(Vec3::new([clip.min.x, clip.max.x][k & 1], [clip.min.y, clip.max.y][(k >> 1) & 1], [clip.min.z, clip.max.z][k >> 2]))).collect();
        let range = |f: fn(&Vec3) -> f32| (corners.iter().map(f).fold(f32::MAX, f32::min), corners.iter().map(f).fold(f32::MIN, f32::max));
        let near = |(a, b): (f32, f32), (c, d): (f32, f32)| (a - c).abs() < 1e-4 && (b - d).abs() < 1e-4;
        assert!(near(range(|p| p.x), (5.5, 6.5)) && near(range(|p| p.y), (-1.5, 10.0)) && near(range(|p| p.z), (-0.6, 7.4)), "{corners:?}");
    }

    #[test]
    fn a_turned_view_is_a_3d_view_looking_the_way_it_is_turned() {
        let display = [[0.6, -0.8, 0.0], [0.4, 0.3, 0.866], [-0.6928, -0.5196, 0.5]];
        let v = view(" 3D", [AXES[0], AXES[1], AXES[2], [0.0; 3]], display, [15000.0, 1000.0], [[-1500.0, -1500.0, -1500.0], [31500.0, 13500.0, 10500.0]]);
        let s = saved_view(&v, &[]).unwrap();
        assert_eq!((s.name.as_str(), s.kind), ("3D", ViewKind::ThreeD));
        let f = Vec3::from_array(s.camera.unwrap().forward);
        assert!((f - Vec3::new(0.6928, 0.5196, -0.5)).length() < 1e-3);
    }

    #[test]
    fn an_arc_bulges_to_the_right_of_its_way_by_its_height_over_the_chord() {
        // half a circle of radius 1 from (0, 0) up to (0, 2): to the right of going up is +x
        let points = arc([0.0, 0.0], [0.0, 2.0], 0.5);
        assert_eq!((points[0], *points.last().unwrap()), ([0.0, 0.0], [0.0, 2.0]));
        let far = points.iter().map(|q| q[0]).fold(f64::MIN, f64::max);
        assert!((far - 1.0).abs() < 1e-3, "{points:?}");
        assert!(points.iter().all(|q| ((q[0]).hypot(q[1] - 1.0) - 1.0).abs() < 1e-9));
        // the other way: to the left
        let left = arc([0.0, 0.0], [0.0, 2.0], -0.5);
        assert!(left.iter().map(|q| q[0]).fold(f64::MAX, f64::min) < -0.99);
        // a shallow arc stays near its chord
        let shallow = arc([0.0, 0.0], [10.0, 0.0], 0.1);
        let sag = shallow.iter().map(|q| q[1]).fold(f64::MAX, f64::min);
        assert!((sag + 1.0).abs() < 1e-2, "{sag}");
    }

    fn drawing(kind: char, mark: &str, snapshot: Option<Snapshot>) -> Drawing {
        let record = DrawingRecord { id: 1, kind, mark: mark.into(), drawing_type: 1, file: "D1.dg".into(), name: "KOLOM".into() };
        let metadata = Some(Metadata { fields: vec![("Width".into(), "420".into()), ("Height".into(), "297".into())] });
        Drawing { record, metadata, snapshot, note: None, definition: None, definition_note: None }
    }

    #[test]
    fn a_drawing_is_a_sheet_with_what_its_snapshot_draws() {
        let item = |object: u32, colour: Option<[u8; 3]>, shape: Shape| Item { object, colour, shape };
        let snapshot = Snapshot {
            paper_mm: [297.0, 210.0],
            title: "KOLOM A [K1]".into(),
            views: vec![SnapshotView { id: 9, origin_mm: [100.0, 50.0], scale: 20.0, frame: [[0.0; 3]; 3], shortening: [Vec::new(), Vec::new()] }],
            items: vec![
                item(0, None, Shape::Line { from: [5.0, 5.0], to: [292.0, 5.0] }),
                item(7, Some([255, 0, 0]), Shape::Text { at: [10.0, 20.0], angle: 0.0, height: 3.6, width: 3.6, text: "K1".into(), font: "Arial".into() }),
                item(8, None, Shape::Circle { center: [50.0, 50.0], radius: 2.0 }),
                item(9, None, Shape::Polygon { edges: vec![([0.0, 0.0], [10.0, 0.0], 0.0), ([10.0, 0.0], [10.0, 10.0], 0.0), ([10.0, 10.0], [0.0, 0.0], 0.0)], fill: Some([0, 128, 0]) }),
            ],
        };
        let s = sheet(&drawing('A', "[K1]", Some(snapshot)));
        assert_eq!((s.number.as_str(), s.name.as_str(), s.title.as_str()), ("K1", "KOLOM", "K1 KOLOM"));
        // the snapshot's paper, in metres
        let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
        let paper = s.paper.unwrap();
        assert!(close(paper.min, [0.0, 0.0]) && close(paper.max, [0.297, 0.21]), "{paper:?}");
        assert!(close(s.lines[0][0], [0.005, 0.005]) && close(s.lines[0][1], [0.292, 0.005]));
        let t = &s.texts[0];
        assert!(close(t.at, [0.01, 0.02]));
        assert_eq!((t.text.as_str(), t.element, &*t.face.family, t.face.color), ("K1", 7, "Arial", Some([255, 0, 0])));
        assert!((t.size - 0.0036 / CAPITALS).abs() < 1e-7);
        // the circle in pieces, the outline's edges, and its fill
        assert!(s.lines.len() > 1 + 8 + 3);
        assert_eq!(s.fills.len(), 1);
        assert_eq!((s.fills[0].loops[0].len(), s.fills[0].color), (4, [0, 128, 0]));
        // no line has a colour of its own
        assert!(s.line_colors.is_empty());
        // without a snapshot: the paper of its metadata, nothing drawn
        let bare = sheet(&drawing('W', "[P10]", None));
        assert!(close(bare.paper.unwrap().max, [0.42, 0.297]) && bare.lines.is_empty());
    }

    #[test]
    fn sheets_go_by_kind_then_mark() {
        let names: Vec<String> = sheets(vec![drawing('W', "[P10]", None), drawing('A', "[K1]", None), drawing('W', "[P2]", None), drawing('G', "[1]", None)]).into_iter().map(|s| s.number).collect();
        assert_eq!(names, ["1", "K1", "P2", "P10"]);
    }

    /// A model saved by Tekla Structures 2026 with its drawings: set TEKLA_DB1 to its .db1.
    #[test]
    fn a_tekla_2026_model_has_levels_views_and_drawings() {
        let Some(path) = std::env::var_os("TEKLA_DB1") else {
            eprintln!("skipped: set TEKLA_DB1 to a Tekla Structures model with drawings");
            return;
        };
        let loaded = TeklaLoader.load_with(&Source::File(path.into()), &Progress::default()).unwrap();
        let p = &loaded.project;
        eprintln!("levels {:?}", p.levels);
        eprintln!("grids {}", p.grids.len());
        for v in &loaded.views {
            eprintln!("view {:?} {:?} level {:?} plan {:?}", v.name, v.kind, v.level, v.plan);
        }
        for s in &p.sheets {
            eprintln!("sheet {:?} paper {:?} lines {} texts {} fills {}", s.title, s.paper.map(|r| r.max), s.lines.len(), s.texts.len(), s.fills.len());
        }
        assert!(!loaded.layers.is_empty());
        assert!(!p.levels.is_empty() && !p.grids.is_empty());
        assert!(!loaded.views.is_empty());
        assert!(p.sheets.iter().any(|s| !s.lines.is_empty()));
    }
}
