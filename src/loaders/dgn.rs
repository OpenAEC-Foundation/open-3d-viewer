//! MicroStation design files (`.dgn`): V7 and earlier (Intergraph ISFF) and V8 (V8, V8i,
//! CONNECT and later: an OLE2 compound file holding zlib-compressed element streams).
//!
//! Adapter from the independent microstation-dll library (`dgn-read`, `dgn-export`) to the app's
//! scene model. The library is written under a clean-room protocol, only from Intergraph's public
//! "Intergraph Standard File Formats (Element Structure)", Microsoft's public [MS-CFB] and the
//! specification of the DGN analysis; what that specification does not describe yet (most
//! dimensions, multi-lines, tags, raster, Parasolid B-reps) is skipped and counted.
//!
//! The scene draws triangles only, so lines, arcs, curves and text boxes become thin ribbons: flat
//! in 2D files, two crossed ribbons in 3D files, as wide as the line weight on a scale set by the
//! drawing extent. Closed planar elements, cones, extruded solids, meshes and B-spline surfaces of
//! 3D files become faces. Each level is a layer; each top-level element (a whole cell, shared
//! cell, text node or complex chain) is one pickable element. Of a file with several models the
//! default model is shown; the others are listed in the project information.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};

pub struct DgnLoader;

impl Loader for DgnLoader {
    fn name(&self) -> &'static str { "MicroStation DGN" }
    fn extensions(&self) -> &'static [&'static str] { &["dgn"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        read(&data)
    }
}

/// Reads a whole design file and draws its default model: one layer per level.
pub fn read(data: &[u8]) -> Result<Loaded, String> {
    let design = dgn_read::read(data).map_err(String::from)?;
    let scene = dgn_export::draw(&design).scene()?;
    let layers = scene.layers.into_iter().map(|layer| Layer {
        name: layer.name, color: DEFAULT_COLOR, positions: layer.positions, normals: layer.normals, colors: Some(layer.colors),
        indices: layer.indices, elements: layer.elements, props: layer.props, ..Layer::default()
    }).collect();
    let mut project = ProjectData::default();
    project.info = scene.info;
    Ok(Loaded { layers, views: Vec::new(), project })
}

/// Files written in the tests from the clean-room specification (`dgn_read::synth`): V7 from the
/// ISFF element layouts, V8 as compound files of element streams. No sample files.
#[cfg(test)]
mod tests {
    use super::*;
    use dgn_read::synth::compound;
    use dgn_read::synth::v7::{dfloat, long, Writer};
    use dgn_read::synth::v8::*;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn load(bytes: Vec<u8>) -> Result<Loaded, String> {
        DgnLoader.load_with(&Source::Bytes { name: "drawing.dgn".into(), data: Arc::new(bytes) }, &Progress::default())
    }

    fn info<'a>(loaded: &'a Loaded, key: &str) -> &'a str {
        &loaded.project.info.iter().find(|(k, _)| k == key).unwrap().1
    }

    fn bounds(layer: &Layer) -> ([f32; 3], [f32; 3]) {
        layer.positions.chunks_exact(3).fold(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
            for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); }
            (lo, hi)
        })
    }

    fn element_bounds(layer: &Layer, k: usize) -> ([f32; 3], [f32; 3]) {
        let (_, first, count) = layer.elements[k];
        layer.indices[first as usize..(first + count) as usize].iter().fold(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), |(mut lo, mut hi), &i| {
            for a in 0..3 {
                lo[a] = lo[a].min(layer.positions[3 * i as usize + a]);
                hi[a] = hi[a].max(layer.positions[3 * i as usize + a]);
            }
            (lo, hi)
        })
    }

    fn names(loaded: &Loaded) -> Vec<&str> { loaded.layers.iter().map(|l| l.name.as_str()).collect() }

    fn colors(layer: &Layer) -> &[[u8; 4]] { layer.colors.as_deref().unwrap() }

    // ---------------------------------------------------------------------------------------------
    // V7

    #[test]
    fn v7_reads_a_2d_drawing_in_metres_by_level_with_colours_and_text() {
        // 1 FT = 10 TH = 1000 UOR, origin shifted like a State Plane drawing.
        let mut w = Writer::new(false, "FT", "TH", 10, 100, [-1000.0, -2000.0, 0.0]);
        let mut table = vec![0u16];
        let mut rgb = vec![[0u8, 0, 0]; 256];
        rgb[4] = [200, 10, 20];
        let flat: Vec<u8> = rgb.concat();
        table.extend(flat.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
        w.element(5, 1, 0, 0, 0, &table);
        w.element(3, 5, 3, 2, 0, &w.points(&[[-1000, -2000, 0], [9000, -2000, 0]]));
        let mut string = vec![3];
        string.extend(w.points(&[[0, 0, 0], [1000, 0, 0], [1000, 1000, 0]]));
        w.element(4, 6, 0, 0, 0, &string);
        // A deleted line far away must not count.
        let far = w.points(&[[90_000_000, 0, 0], [91_000_000, 0, 0]]);
        w.element(3, 6, 0, 0, 0, &far);
        let deleted = w.words.len() - far.len() - 18;
        w.words[deleted] |= 0x8000;
        // Text "AB" of height 6 * 50000 / 1000 = 300 UOR at (0, 3000).
        let mut text = vec![0x0000];
        text.extend(long(50_000));
        text.extend(long(50_000));
        text.extend(long(0));
        text.extend(w.points(&[[0, 3000, 0]]));
        text.extend([2, u16::from_le_bytes(*b"AB")]);
        w.element(17, 7, 0, 0, 0, &text);
        let loaded = load(w.bytes()).unwrap();
        assert_eq!(names(&loaded), ["Level 5", "Level 6", "Level 7"]);
        assert!(info(&loaded, "DGN version").contains("2D"));
        assert_eq!(info(&loaded, "DGN units"), "1 FT = 10 TH = 1000 UOR");
        assert!(info(&loaded, "DGN elements").contains("Line: 1"));
        // The line from the global origin runs 10 ft east: 3.048 m plus the square ends.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 3.048).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(lo[2] == 0.0 && hi[2] == 0.0);
        // Colour 3 is slot 4 of the stored table.
        assert_eq!(colors(&loaded.layers[0])[0], [200, 10, 20, 255]);
        assert_eq!(loaded.layers[0].elements.len(), 1);
        // Origin (world) is the first point, which is the global origin: (0, 0) in metres.
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "0, 0, 0");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!(hi[0] < 4.0 && lo[0] > 0.0, "deleted line is skipped: {lo:?} {hi:?}");
        let text_layer = &loaded.layers[2];
        assert!(text_layer.props[0].1.contains("\"Text\":\"AB\""), "{}", text_layer.props[0].1);
        let (lo, hi) = bounds(text_layer);
        // Box 2 characters x 300 UOR wide, 300 UOR high: 0.18 by 0.09 m, above y = (3000 + 2000) UOR.
        assert!((hi[0] - lo[0] - 0.18).abs() < 0.02 && (hi[1] - lo[1] - 0.09).abs() < 0.02, "{lo:?} {hi:?}");
        assert!((lo[1] - 1.524).abs() < 0.02, "{lo:?}");
    }

    #[test]
    fn v7_arcs_and_ellipses_follow_their_geometry() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        // A quarter arc of radius 1 m around (0, 0), from 0 to 90 degrees.
        let mut arc = Vec::new();
        arc.extend(long(0));
        arc.extend(long(90 * 360_000));
        arc.extend(dfloat(1000.0));
        arc.extend(dfloat(1000.0));
        arc.extend(long(0));
        arc.extend(dfloat(0.0));
        arc.extend(dfloat(0.0));
        w.element(16, 1, 1, 0, 0, &arc);
        // An ellipse 4 x 2 m turned 90 degrees, around (10, 0).
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(2000.0));
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(long(90 * 360_000));
        ellipse.extend(dfloat(10_000.0));
        ellipse.extend(dfloat(0.0));
        w.element(15, 2, 1, 0, 0, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        // The world origin is the arc's first point (1, 0): the arc spans x -1..0 and y 0..1.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((lo[0] + 1.0).abs() < 0.02 && hi[0].abs() < 0.02 && lo[1].abs() < 0.02 && (hi[1] - 1.0).abs() < 0.02, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[0] - lo[0] - 2.0).abs() < 0.02 && (hi[1] - lo[1] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(((hi[0] + lo[0]) / 2.0 - 9.0).abs() < 0.01);
    }

    #[test]
    fn v7_cells_and_complex_shapes_are_one_element_each() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let line = w.points(&[[0, 0, 0], [1000, 0, 0]]);
        let line2 = w.points(&[[0, 1000, 0], [1000, 1000, 0]]);
        // A cell: totlength counts the header words after word 18 and the components.
        let mut cell = vec![0u16];
        cell.extend(long(0x0693_0000)); // RADIX-50 "ABC"
        cell.extend([0u16; 5 + 8 + 8 + 4]);
        cell[0] = (18 + cell.len() + 2 * (18 + line.len()) - 19) as u16;
        w.element(2, 3, 0, 0, 0, &cell);
        w.element(3, 3, 0, 0, 0x80, &line);
        w.element(3, 4, 2, 0, 0x80, &line2);
        // A complex shape of two line strings.
        let mut shape = vec![0u16, 2, 0, 0, 0, 0];
        let mut a = vec![2u16];
        a.extend(w.points(&[[5000, 0, 0], [6000, 0, 0]]));
        let mut b = vec![3u16];
        b.extend(w.points(&[[6000, 0, 0], [6000, 1000, 0], [5000, 0, 0]]));
        shape[0] = ((18 + shape.len()) + (18 + a.len()) + (18 + b.len()) - 19) as u16;
        w.element(14, 5, 0, 0, 0, &shape);
        w.element(4, 5, 0, 0, 0x80, &a);
        w.element(4, 5, 0, 0, 0x80, &b);
        let loaded = load(w.bytes()).unwrap();
        let ids: BTreeSet<u64> = loaded.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids.len(), 2, "{:?}", loaded.layers.iter().map(|l| &l.elements).collect::<Vec<_>>());
        assert_eq!(loaded.layers.len(), 3, "cell components keep their own levels");
        assert!(loaded.layers[0].props[0].1.contains("\"Cell\":\"ABC\""));
        assert!(info(&loaded, "DGN elements").contains("Cell: 1") && info(&loaded, "DGN elements").contains("Complex shape: 1"));
    }

    #[test]
    fn v7_solids_cones_and_shapes_of_3d_files_are_faces() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A unit cube as a solid of projection: two square boundaries and four rule lines.
        let square = |z: i32| {
            let mut s = vec![5u16];
            s.extend(w.points(&[[0, 0, z], [1000, 0, z], [1000, 1000, z], [0, 1000, z], [0, 0, z]]));
            s
        };
        let (bottom, top) = (square(0), square(1000));
        let rule = w.points(&[[0, 0, 0], [0, 0, 1000]]);
        let mut solid = vec![0u16, 3, 0, 0, 0, 0, 0];
        solid[0] = ((18 + solid.len()) + (18 + bottom.len()) + (18 + top.len()) + (18 + rule.len()) - 19) as u16;
        w.element(19, 2, 0, 0, 0, &solid);
        w.element(6, 2, 0, 0, 0x80, &bottom);
        w.element(6, 2, 0, 0, 0x80, &top);
        w.element(3, 2, 0, 0, 0x80 | 4, &rule);
        // A cylinder of radius 1 m and height 2 m with an identity quaternion, beside it.
        let mut cone = vec![0u16];
        cone.extend(long(i32::MAX));
        cone.extend([0u16; 6]);
        for v in [5000.0, 0.0, 0.0, 1000.0, 5000.0, 0.0, 2000.0, 1000.0] { cone.extend(dfloat(v)); }
        w.element(23, 3, 0, 0, 0, &cone);
        let loaded = load(w.bytes()).unwrap();
        let cube = &loaded.layers[0];
        assert_eq!(cube.triangle_count(), 12, "the capped cube");
        assert!(!cube.solid_indices().is_empty(), "the capped solid encloses volume");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 1e-4 && (hi[0] - lo[0] - 2.0).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!(info(&loaded, "DGN version").contains("3D"));
    }

    #[test]
    fn v7_rotations_of_3d_elements_use_the_stored_quaternion() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A circle of radius 1 m turned 90 degrees about X lies in the XZ plane.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(dfloat(1000.0));
        for q in [s, -s, 0.0, 0.0] { ellipse.extend(long((q * f64::from(i32::MAX)) as i32)); }
        for _ in 0..3 { ellipse.extend(dfloat(0.0)); }
        w.element(15, 1, 0, 0, 0x8000, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 0.05 && (hi[1] - lo[1]) < 0.05, "{lo:?} {hi:?}");
    }

    #[test]
    fn v7_bspline_curves_are_clamped_to_their_end_poles() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let header = vec![0u16, 0, 2, 4, 0];
        w.element(27, 1, 0, 0, 0, &header);
        let mut poles = vec![4u16];
        poles.extend(w.points(&[[0, 0, 0], [1000, 2000, 0], [3000, 2000, 0], [4000, 0, 0]]));
        w.element(21, 1, 0, 0, 0x80, &poles);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(hi[1] < 1.6 && hi[1] > 1.4, "a cubic stays inside its control polygon: {hi:?}");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    #[test]
    fn v7_bspline_surfaces_are_sampled_faces() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A bilinear patch of 2 x 2 poles (order 2 both ways): the unit square at z = 0.
        let mut header = vec![0u16; 11];
        header[20 - 18] = 0;
        header[24 - 18] = 0;
        w.element(24, 1, 0, 0, 0, &header);
        for y in [0, 1000] {
            let mut row = vec![2u16];
            row.extend(w.points(&[[0, y, 0], [1000, y, 0]]));
            w.element(21, 1, 0, 0, 0x80, &row);
        }
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 1.0).abs() < 1e-5 && (hi[1] - lo[1] - 1.0).abs() < 1e-5 && hi[2] == lo[2], "{lo:?} {hi:?}");
        assert_eq!(loaded.layers[0].triangle_count(), 2 * 8 * 8);
        assert_eq!(info(&loaded, "DGN elements"), "B-spline surface: 1");
    }

    #[test]
    fn broken_v8_and_foreign_files_are_refused_clearly() {
        let mut v8 = dgn_read::OLE_SIGNATURE.to_vec();
        v8.resize(512, 0);
        assert!(load(v8).err().unwrap().contains("V8"));
        let other = compound(&[("WordDocument".into(), vec![1; 100])]);
        assert!(load(other).err().unwrap().contains("no design model"));
        assert!(load(b"not a design file at all".to_vec()).err().unwrap().contains("not a design file"));
        let header_only = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]).bytes();
        assert!(load(header_only).err().unwrap().contains("no drawable elements"));
    }

    // ---------------------------------------------------------------------------------------------
    // V8, written from the clean-room specification (SPEC_v0.1.md §B) and [MS-CFB].

    #[test]
    fn v8_files_open_with_units_levels_and_colours() {
        // 1000 UOR per metre; the global origin is (-1000, -2000, 0) UOR.
        let mut f = File::new(1000.0, 1.0, 1.0, [-1000.0, -2000.0, 0.0]);
        f.table(level_entry(64, "Default")).table(level_entry(70, "Walls")).table(color_entry(&[(4, [200, 10, 20])]));
        // A 2D line 10 m long from the global origin, colour 3, on "Walls".
        f.graphic(element(3, 0x1000, 70, shown(0, 3), &doubles(&[-1000.0, -2000.0, 9000.0, -2000.0]), &[]));
        // A non-graphic element among the graphics is passed over.
        f.graphic(element(66, 0x0400, 9, None, &[0; 16], &[]));
        // A long 3D line string, which runs across several streams, on "Default".
        let coordinates: Vec<f64> = (0..400).flat_map(|k| [f64::from(k) * 10.0, 5000.0, f64::from(k % 2) * 10.0]).collect();
        f.graphic(element(4, 0x1000, 64, shown(D3, 1), &counted(400, &coordinates), &[]));
        // A line on a level without a name.
        f.graphic(element(3, 0x1000, 99, shown(0, 0), &doubles(&[0.0, 0.0, 0.0, 1000.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(names(&loaded), ["Default", "Walls", "Level 99"]);
        assert_eq!(info(&loaded, "DGN version"), "V8, 3D");
        assert!(info(&loaded, "DGN units").contains("1 UOR = 0.001 m"), "{}", info(&loaded, "DGN units"));
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "0, 0, 0");
        assert_eq!(info(&loaded, "DGN elements"), "Line: 2, Line string: 1");
        assert_eq!(info(&loaded, "DGN skipped elements"), "0");
        let walls = &loaded.layers[1];
        let (lo, hi) = bounds(walls);
        assert!((hi[0] - lo[0] - 10.0).abs() < 0.05 && lo[0] < 0.0, "{lo:?} {hi:?}");
        // Colour 3 is slot 4 of the stored table.
        assert_eq!(colors(walls)[0], [200, 10, 20, 255]);
        assert!(walls.props[0].1.contains("\"Level\":\"Walls\""), "{}", walls.props[0].1);
        // The line string is 3.99 m long at y = 7 m (5000 + 2000 UOR).
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 3.99).abs() < 0.05 && ((lo[1] + hi[1]) / 2.0 - 7.0).abs() < 0.01, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_shapes_arcs_text_cells_and_skipped_elements() {
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        let square: Vec<f64> = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0], [0.0, 0.0]].iter().flat_map(|p| [p[0], p[1], 0.0]).collect();
        // A closed 3D shape is a face; a hole is its outline.
        f.graphic(element(6, 0x1000, 1, shown(D3, 0), &counted(5, &square), &[]));
        f.graphic(element(6, 0x1000, 2, shown(D3 | HOLE, 0), &counted(5, &square), &[]));
        // A quarter arc (2D) of radius 1 m around (10, 0) m, from 0 to 90 degrees.
        f.graphic(element(16, 0x1000, 3, shown(0, 0), &doubles(&[0.0, std::f64::consts::FRAC_PI_2, 1000.0, 1000.0, 0.0, 10_000.0, 0.0]), &[]));
        // A 3D ellipse of 4 x 2 m with the identity quaternion: a filled disc.
        f.graphic(element(15, 0x1000, 4, shown(D3, 0), &doubles(&[2000.0, 1000.0, 1.0, 0.0, 0.0, 0.0, 20_000.0, 0.0, 0.0]), &[]));
        // Text in Latin-1 after the marker, 1800 x 300 UOR.
        let mut characters = vec![0xFF, 0xFE, 0x01, 0x00];
        characters.extend(b"myT\xE9xt");
        f.graphic(element(17, 0x1000, 5, shown(0, 0), &text_data([1800.0, 300.0], [0.0, 5000.0], &characters), &[]));
        // A cell of two lines on levels 6 and 7.
        f.graphic(element(2, 0x3000, 6, shown(0, 0), &[2, 0, 0, 0, 0, 0, 0, 0], &[(0, "NOT A NAME"), (1, "DOOR")]));
        f.graphic(element(3, 0x5000, 6, shown(0, 0), &doubles(&[0.0, 8000.0, 1000.0, 8000.0]), &[]));
        f.graphic(element(3, 0x5000, 7, shown(0, 2), &doubles(&[0.0, 9000.0, 1000.0, 9000.0]), &[]));
        // A 3D complex shape of two line strings: one triangle.
        f.graphic(element(14, 0x3000, 8, shown(D3, 0), &2u32.to_le_bytes(), &[]));
        f.graphic(element(4, 0x5000, 8, shown(D3, 0), &counted(2, &[30_000.0, 0.0, 0.0, 31_000.0, 0.0, 0.0]), &[]));
        f.graphic(element(4, 0x5000, 8, shown(D3, 0), &counted(3, &[31_000.0, 0.0, 0.0, 31_000.0, 1000.0, 0.0, 30_000.0, 0.0, 0.0]), &[]));
        // A disjoint point string: three point markers.
        f.graphic(element(22, 0x1000, 9, shown(D3 | HOLE, 0), &counted(3, &[0.0, 12_000.0, 0.0, 500.0, 12_000.0, 0.0, 1000.0, 12_000.0, 0.0]), &[]));
        // A text node of two texts, the second 8-bit text without a marker.
        f.graphic(element(7, 0x3000, 10, shown(0, 0), &[2, 0, 0, 0, 1, 0, 0, 0], &[]));
        f.graphic(element(17, 0x5000, 10, shown(0, 0), &text_data([300.0, 300.0], [0.0, 14_000.0], b"A"), &[]));
        f.graphic(element(17, 0x5000, 10, shown(0, 0), &text_data([600.0, 300.0], [0.0, 15_000.0], b"\xB0C\0"), &[]));
        // A cell whose only string linkage has another key: no name.
        f.graphic(element(2, 0x3000, 12, shown(0, 0), &[1, 0, 0, 0, 0, 0, 0, 0], &[(0, "NOT A NAME")]));
        f.graphic(element(3, 0x5000, 12, shown(0, 0), &doubles(&[0.0, 16_000.0, 1000.0, 16_000.0]), &[]));
        // Not described yet: a dimension and an unknown type; and a second model, which is read.
        f.graphic(element(33, 0x1000, 11, shown(0, 0), &[0; 64], &[]));
        f.graphic(element(107, 0x1000, 11, shown(0, 0), &[0; 64], &[]));
        f.extra.push(("Dgn-Md/#000001/Dgn^G/$1".into(), vec![0; 16]));
        let loaded = load(f.bytes()).unwrap();
        let layer = |name: &str| loaded.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}"));
        assert_eq!(layer("Level 1").triangle_count(), 2, "a closed 3D shape is a face");
        assert_eq!(layer("Level 2").triangle_count(), 16, "a hole is four crossed ribbons");
        let (lo, hi) = bounds(layer("Level 3"));
        // Sizes of ribbons include their width (about 0.01 m at this drawing extent).
        assert!((hi[0] - lo[0] - 1.0).abs() < 0.05 && (hi[1] - lo[1] - 1.0).abs() < 0.05, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(layer("Level 4"));
        assert!((hi[0] - lo[0] - 4.0).abs() < 0.01 && (hi[1] - lo[1] - 2.0).abs() < 0.01 && hi[2] == lo[2], "{lo:?} {hi:?}");
        let text = layer("Level 5");
        assert!(text.props[0].1.contains("\"Text\":\"myTéxt\""), "{}", text.props[0].1);
        let (lo, hi) = bounds(text);
        assert!((hi[0] - lo[0] - 1.8).abs() < 0.05 && (hi[1] - lo[1] - 0.3).abs() < 0.05, "the stored width and height: {lo:?} {hi:?}");
        let (door, door_frame) = (layer("Level 6"), layer("Level 7"));
        assert_eq!(door.elements[0].0, door_frame.elements[0].0, "a cell is one pickable element");
        assert!(door.props[0].1.contains("\"Cell\":\"DOOR\""), "{}", door.props[0].1);
        assert_eq!(layer("Level 8").triangle_count(), 1);
        assert_eq!(layer("Level 9").triangle_count(), 12, "three point markers");
        assert!(layer("Level 10").props[0].1.contains("\"Text\":\"A\\n°C\""), "{}", layer("Level 10").props[0].1);
        assert!(!layer("Level 12").props[0].1.contains("\"Cell\":"), "{}", layer("Level 12").props[0].1);
        let ids: BTreeSet<u64> = loaded.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids.len(), 10);
        let elements = info(&loaded, "DGN elements");
        for drawn in ["Arc: 1", "Cell: 2", "Complex shape: 1", "Ellipse: 1", "Point string: 1", "Shape: 2", "Text: 1", "Text node: 1"] {
            assert!(elements.contains(drawn), "{drawn} in {elements}");
        }
        assert_eq!(info(&loaded, "DGN skipped elements"), "Dimension: 1, Element type 107: 1");
        assert_eq!(info(&loaded, "DGN models"), "2: #0, #1");
    }

    #[test]
    fn v8_shared_cells_place_their_definition_by_the_row_major_matrix() {
        const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        f.table(level_entry(70, "Furniture"));
        // The definition: a line of 1 m along X, on level 0.
        f.table(element(34, 0x3000, 0, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(1, "ARROW")]));
        f.table(element(3, 0x5000, 0, shown(0, 0), &doubles(&[0.0, 0.0, 1000.0, 0.0]), &[]));
        // Turned +90 degrees about Z (row by row) and placed at (5, 0) m: the line runs along +Y.
        f.graphic(element(35, 0x1000, 70, shown(0, 0), &shared_data([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], [5000.0, 0.0, 0.0]), &[(1, "ARROW")]));
        // Twice as long, at (0, 3) m.
        f.graphic(element(35, 0x1000, 70, shown(0, 0), &shared_data([2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0], [0.0, 3000.0, 0.0]), &[(1, "arrow")]));
        f.graphic(element(35, 0x1000, 70, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(1, "MISSING")]));
        // A name under another key is no name.
        f.graphic(element(35, 0x1000, 70, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(0, "ARROW")]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(loaded.layers.len(), 1, "components on level 0 take the level of the instance");
        let layer = &loaded.layers[0];
        assert_eq!(layer.name, "Furniture");
        assert_eq!(layer.elements.len(), 2);
        assert!(layer.props[0].1.contains("\"Cell\":\"ARROW\""), "{}", layer.props[0].1);
        // The world origin is the first point of the first instance, (5, 0) m.
        let (lo, hi) = element_bounds(layer, 0);
        assert!(lo[1] > -0.01 && (hi[1] - 1.0).abs() < 0.01 && hi[0] - lo[0] < 0.01, "{lo:?} {hi:?}");
        let (lo, hi) = element_bounds(layer, 1);
        assert!((lo[0] + 5.0).abs() < 0.01 && (hi[0] - lo[0] - 2.0).abs() < 0.01 && (lo[1] - 3.0).abs() < 0.01, "{lo:?} {hi:?}");
        assert_eq!(info(&loaded, "DGN elements"), "Shared cell: 2");
        assert_eq!(info(&loaded, "DGN skipped elements"), "Shared cell without definition: 2");
    }

    #[test]
    fn v8_shared_cells_that_hold_themselves_stop() {
        const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // A definition of a line and an instance of itself, one metre further.
        f.table(element(34, 0x3000, 0, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(1, "LOOP")]));
        f.table(element(3, 0x5000, 0, shown(0, 0), &doubles(&[0.0, 0.0, 1000.0, 0.0]), &[]));
        f.table(element(35, 0x5000, 0, shown(0, 0), &shared_data(IDENTITY, [1000.0, 0.0, 0.0]), &[(1, "LOOP")]));
        f.graphic(element(35, 0x1000, 70, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(1, "LOOP")]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(info(&loaded, "DGN skipped elements"), "Shared cell that contains itself: 1");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    #[test]
    fn v8_line_strings_with_flag_0100_hold_n_minus_1_points_after_a_marker() {
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // N = 4: a 16-byte marker, then three points (B.11); the length is 0x70 + 16 N.
        let mut data = 4u32.to_le_bytes().to_vec();
        data.extend([0; 4]);
        data.extend([0xF0, 0x06, 0x91, 0x7C, 0xFF, 0xFF, 0xFF, 0xFF].repeat(2));
        data.extend(doubles(&[1000.0, 1000.0, 3000.0, 1000.0, 3000.0, 2000.0]));
        let mut line = element(4, 0x1100, 64, shown(0, 0), &data, &[]);
        assert_eq!(line.len(), 0x70 + 16 * 4);
        // An invalid stored range: low = the first point, the "size" 2^63 - low x.
        set_range(&mut line, [1000, 1000, 0], [i64::MAX - 999, 0, 0]);
        f.graphic(line);
        // An ordinary line string beside it.
        f.graphic(element(4, 0x1000, 65, shown(0, 0), &counted(2, &[0.0, 0.0, 0.0, 500.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(info(&loaded, "DGN elements"), "Line string: 2");
        assert_eq!(info(&loaded, "DGN skipped elements"), "0");
        // The world origin is the first point, (1, 1) m; the points span 2 x 1 m.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!(lo[0] > -0.01 && (hi[0] - 2.0).abs() < 0.01 && lo[1] > -0.01 && (hi[1] - 1.0).abs() < 0.01, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((lo[0] + 1.0).abs() < 0.01 && (hi[1] + 0.5).abs() < 0.01, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_rational_closed_bspline_is_a_circle() {
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // A circle of radius 1 m: three arcs of 120 degrees, order 3, weights 1 and 0.5.
        let (r, s) = (1000.0, 3f64.sqrt());
        let poles = [r, 0.0, r, s * r, -r / 2.0, s / 2.0 * r, -2.0 * r, 0.0, -r / 2.0, -s / 2.0 * r, r, -s * r, r, 0.0];
        let header: Vec<u8> = [3u32, 1 | 0x80 | 0x100, 7, 6].iter().flat_map(|v| v.to_le_bytes()).collect();
        f.graphic(element(27, 0x3000, 1, shown(0, 0), &header, &[]));
        f.graphic(element(26, 0x4400, 0, None, &doubles(&[0.0, 1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0]), &[]));
        f.graphic(element(21, 0x5000, 1, shown(0, 0), &counted(7, &poles), &[]));
        f.graphic(element(28, 0x4400, 0, None, &doubles(&[1.0, 0.5, 1.0, 0.5, 1.0, 0.5, 1.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        // The world origin is the first point, (1, 0) m.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((lo[0] + 2.0).abs() < 0.01 && hi[0].abs() < 0.01 && (lo[1] + 1.0).abs() < 0.01 && (hi[1] - 1.0).abs() < 0.01, "{lo:?} {hi:?}");
        assert_eq!(info(&loaded, "DGN elements"), "B-spline curve: 1");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    #[test]
    fn v8_linear_dimensions_draw_extension_and_dimension_lines() {
        const IDENTITY: [f64; 4] = [1.0, 0.0, 0.0, 1.0];
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // From (0, 0) to (3, 0) m with the dimension line 1 m above.
        f.graphic(dimension(31, &[[0.0, 0.0], [3000.0, 0.0]], 1000.0, IDENTITY, [0, 0], [3000, 1000], 0));
        // A chain of three points along Y at x = 10 m, the dimension line 1 m to the left: the
        // x axis is the first column (0, 1), the y axis the second (-1, 0). Read as rows, the line
        // would lie 1 m to the right, outside the range.
        f.graphic(dimension(32, &[[10_000.0, 0.0], [10_000.0, 2000.0], [10_000.0, 5000.0]], 1000.0, [0.0, -1.0, 1.0, 0.0], [9000, 0], [1000, 5000], 0));
        // Skipped: feet outside the range (another kind of dimension), one point, a 3D dimension.
        f.graphic(dimension(33, &[[0.0, 20_000.0], [3000.0, 20_000.0]], 1000.0, IDENTITY, [50_000, 50_000], [3000, 1000], 0));
        f.graphic(dimension(33, &[[0.0, 20_000.0]], 1000.0, IDENTITY, [0, 20_000], [3000, 1000], 0));
        f.graphic(dimension(33, &[[0.0, 20_000.0], [3000.0, 20_000.0]], 1000.0, IDENTITY, [0, 20_000], [3000, 1000], D3));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(info(&loaded, "DGN elements"), "Dimension: 2");
        assert_eq!(info(&loaded, "DGN skipped elements"), "Dimension: 3");
        assert_eq!(names(&loaded), ["Level 31", "Level 32"]);
        let (plain, chain) = (&loaded.layers[0], &loaded.layers[1]);
        assert_eq!(plain.elements.len(), 1, "a dimension is one element");
        assert!(plain.props[0].1.contains("\"DGN element\":\"Dimension\"") && plain.props[0].1.contains("\"Measured (master units)\":\"3\""), "{}", plain.props[0].1);
        // The world origin is the first point (0, 0); lines up to the dimension line at y = 1 m.
        let (lo, hi) = bounds(plain);
        assert!(lo[0].abs() < 0.02 && (hi[0] - 3.0).abs() < 0.02 && lo[1].abs() < 0.02 && (hi[1] - 1.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(chain.props[0].1.contains("\"Measured (master units)\":\"2; 3\""), "{}", chain.props[0].1);
        let (lo, hi) = bounds(chain);
        assert!((lo[0] - 9.0).abs() < 0.02 && (hi[0] - 10.0).abs() < 0.02 && lo[1].abs() < 0.02 && (hi[1] - 5.0).abs() < 0.02, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_isometric_dimensions_solve_for_their_skewed_axes() {
        // x = (cos 30, sin 30), y = (0, 1): vertical extension lines, a dimension line at 30 degrees.
        let c = 3f64.sqrt() / 2.0;
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // The dimension line runs 1 m below P0 = (0, 0) (h = -1), so its feet are B = (0, -1) m and
        // B + 3 x = (2.598, 0.5) m; P1 lies 1.5 m above its foot.
        let p1 = [3000.0 * c, 2000.0];
        f.graphic(dimension(34, &[[0.0, 0.0], p1], -1000.0, [c, 0.0, 0.5, 1.0], [0, -1000], [(3000.0 * c) as i64, 1500], 0));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(info(&loaded, "DGN elements"), "Dimension: 1");
        assert_eq!(info(&loaded, "DGN skipped elements"), "0");
        let layer = &loaded.layers[0];
        assert!(layer.props[0].1.contains("\"Measured (master units)\":\"3\""), "{}", layer.props[0].1);
        let (lo, hi) = bounds(layer);
        assert!(lo[0].abs() < 0.02 && (hi[0] - 3.0 * c as f32).abs() < 0.02 && (lo[1] + 1.0).abs() < 0.02 && (hi[1] - 2.0).abs() < 0.02, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_control_elements_with_the_empty_range_are_not_drawn() {
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // A "Pattern Control Element" text (B.12): level 0, properties 0x4281, at the origin, of
        // size 0, with the empty range (low -2^31, size 0).
        let mut control = element(17, 0x1000, 0, shown(0x4281, 0), &text_data([0.0, 0.0], [0.0, 0.0], b"Pattern Control Element"), &[]);
        set_range(&mut control, [-(1i64 << 31), -(1i64 << 31), 0], [0, 0, 0]);
        f.graphic(control);
        // The drawing itself, some 152 km from the origin: a line of 10 m.
        f.graphic(element(3, 0x1000, 5, shown(0, 0), &doubles(&[152_000_000.0, 50_000_000.0, 152_010_000.0, 50_000_000.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(info(&loaded, "DGN elements"), "Line: 1");
        assert_eq!(info(&loaded, "DGN skipped elements"), "Control element: 1");
        assert_eq!(names(&loaded), ["Level 5"]);
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "152000, 50000, 0");
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!(lo[0] > -0.01 && (hi[0] - 10.0).abs() < 0.01 && hi[1] - lo[1] < 0.01, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_fill_colours_and_by_level_symbology() {
        let mut f = File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // Level 11: a custom line style (drawn solid), weight 2, colour 5 (magenta in our palette).
        f.table(level_with_symbology(11, "Grid", -71, 2, 5));
        // Grid lines by level (colour and weight 0xFFFFFFFF, style 0x7FFFFFFF) on level 11 and on
        // level 13, which has no entry; a line of weight 0 and colour 5 on level 12 for comparison.
        for (level, y) in [(11, 10_000.0), (12, 12_000.0), (13, 14_000.0)] {
            let display = if level == 12 { [0, 5, 0] } else { [0, u32::MAX, u32::MAX] };
            let mut line = element(3, 0x1000, level, Some(display), &doubles(&[0.0, y, 10_000.0, y]), &[]);
            if level != 12 { line[0x2C..0x30].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes()); }
            f.graphic(line);
        }
        // A note frame on level 20: a 2D shape with outline colour 1 (blue), filled with colour 4 (yellow).
        let square: Vec<f64> = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0], [0.0, 0.0]].concat();
        f.graphic(with_fill(element(6, 0x1000, 20, shown(0, 1), &counted(5, &square), &[]), 4));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(names(&loaded), ["Grid", "Level 12", "Level 13", "Level 20"]);
        let (grid, reference, missing, frame) = (&loaded.layers[0], &loaded.layers[1], &loaded.layers[2], &loaded.layers[3]);
        assert_eq!(colors(grid)[0], [255, 0, 255, 255], "the colour of level 11");
        let props = &grid.props[0].1;
        assert!(props.contains("\"Colour index\":5") && props.contains("\"Line weight\":2") && props.contains("\"Line style\":-71"), "{props}");
        assert!(props.contains("\"By level\":\"line style, line weight, colour\""), "{props}");
        // Weight 2 is 1.7 times as wide as weight 0 (not 31, which would be 11.85 times).
        let width = |layer: &Layer| { let (lo, hi) = bounds(layer); hi[1] - lo[1] };
        let ratio = width(grid) / width(reference);
        assert!((ratio - 1.7).abs() < 0.05, "{ratio}");
        // A level without an entry: colour 0 (white, drawn as dark ink) and weight 0.
        assert_eq!(colors(missing)[0], [45, 45, 48, 255]);
        assert!((width(missing) / width(reference) - 1.0).abs() < 0.01);
        // The frame: two yellow triangles and the blue outline.
        assert_eq!(colors(frame).iter().filter(|c| **c == [255, 255, 0, 255]).count(), 6, "two fill triangles");
        assert!(colors(frame).iter().filter(|c| **c != [255, 255, 0, 255]).all(|c| *c == [0, 0, 255, 255]));
        assert!(frame.props[0].1.contains("\"Fill colour index\":4"), "{}", frame.props[0].1);
    }

    /// Loads every `.dgn` below `$DGN_SAMPLES` (files that are not in the repository) and prints
    /// what came through: `DGN_SAMPLES=dir cargo test --lib dgn -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn samples_from_environment() {
        let Ok(root) = std::env::var("DGN_SAMPLES") else { return };
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dgn")) {
                match load(std::fs::read(&path).unwrap()) {
                    Ok(loaded) => {
                        let triangles: usize = loaded.layers.iter().map(Layer::triangle_count).sum();
                        println!("{}: {} layers, {triangles} triangles", path.display(), loaded.layers.len());
                        for (key, value) in &loaded.project.info { println!("    {key}: {value}"); }
                    }
                    Err(error) => println!("{}: {error}", path.display()),
                }
            }
        }
    }
}
