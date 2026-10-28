//! Exploded view (as BIM Vision's explode sliders): pulls the model apart along X, Y and Z, so
//! every element can be seen (for a quantity takeoff, say) without zooming in. Per storey (Revit
//! levels) or per element, away from the middle of the model; the same rules as the web app's
//! `ExplodeManager`.
//!
//! What moves together is a unit: an element (all its ranges, over all layers), the triangles of
//! a layer outside its elements, or a whole layer without elements. The units and their bounds at
//! rest are indexed once per model; the renderer adds the offset of each vertex's unit in the
//! vertex shader, so the geometry never changes and 0 % on every axis is the model as loaded.

use crate::model::{Bounds, Model};
use crate::viewport::render::transform_bounds;
use glam::{Mat4, Vec3};
use std::collections::HashMap;

/// At 100 %, elements move to three times their distance from the middle of the model.
const ELEMENT_SPREAD: f32 = 2.0;
/// At 100 %, storeys lie side by side with a 10 % gap.
const STOREY_SPREAD_XY: f32 = 1.1;
/// At 100 %, each storey rises by two storey heights.
const STOREY_SPREAD_Z: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplodeMode {
    /// Whole storeys apart; elements without a level stay put. Needs levels, else `Elements`.
    #[default]
    Storeys,
    /// Every element away from the middle of the model.
    Elements,
}

/// The units of a model at rest.
#[derive(Debug)]
pub struct ExplodeIndex {
    /// Per layer, the unit of every vertex (0: none, never moves).
    pub vertex_units: Vec<Vec<u32>>,
    /// Per unit (0 unused): bounds at rest (model space).
    bounds: Vec<Bounds>,
    /// Per unit: storey, 0 for the lowest (None: no level).
    storey: Vec<Option<u32>>,
    /// Storeys with elements.
    storeys: usize,
    /// All units at rest (model space).
    rest: Bounds,
    /// The units on a storey at rest (model space): the building, without site or terrain, for
    /// the storey spacing.
    building: Bounds,
}

impl ExplodeIndex {
    pub fn new(model: &Model) -> Self {
        // per unit: the element it is (None: the rest of a layer)
        let mut elements: Vec<Option<u64>> = vec![None];
        let mut element_units: HashMap<u64, u32> = HashMap::new();
        let mut bounds = vec![Bounds::empty()];
        let mut vertex_units = Vec::with_capacity(model.layers.len());
        for layer in &model.layers {
            let mut units = vec![0u32; layer.vertex_count()];
            // index ranges with their element, the gaps between them included (sorted by first index)
            let mut runs = Vec::with_capacity(2 * layer.elements.len() + 1);
            let mut at = 0usize;
            for &(id, first, count) in &layer.elements {
                let (first, end) = (first as usize, (first + count) as usize);
                if first > at {
                    runs.push((at, first, None));
                }
                runs.push((first.max(at), end, Some(id)));
                at = at.max(end);
            }
            runs.push((at, layer.indices.len(), None));
            let mut rest: Option<u32> = None;
            for (start, end, element) in runs {
                let Some(tris) = layer.indices.get(start..end.min(layer.indices.len())).filter(|t| !t.is_empty()) else {
                    continue;
                };
                let unit = match element {
                    Some(id) => *element_units.entry(id).or_insert_with(|| {
                        elements.push(Some(id));
                        elements.len() as u32 - 1
                    }),
                    None => *rest.get_or_insert_with(|| {
                        elements.push(None);
                        elements.len() as u32 - 1
                    }),
                };
                bounds.resize(elements.len(), Bounds::empty());
                for &i in tris {
                    let Some(p) = layer.positions.get(3 * i as usize..3 * i as usize + 3) else { continue };
                    units[i as usize] = unit;
                    if p.iter().all(|v| v.is_finite()) {
                        bounds[unit as usize].extend([p[0], p[1], p[2]]);
                    }
                }
            }
            vertex_units.push(units);
        }

        // storeys: the level of every element ("level" in its properties, Revit), low to high by
        // the levels' elevations when all of them are known, else by the average base of their
        // elements
        let mut levels: HashMap<String, Vec<u32>> = HashMap::new();
        for (u, id) in elements.iter().enumerate() {
            let Some(json) = id.and_then(|id| model.props.get(&id)).filter(|j| j.contains("\"level\"")) else { continue };
            let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) else { continue };
            if let Some(level) = map.get("level").and_then(|v| v.as_str()).filter(|l| !l.is_empty()) {
                levels.entry(level.to_string()).or_default().push(u as u32);
            }
        }
        let elevation: HashMap<&str, f32> = model.project.levels.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        let by_elevation = levels.keys().all(|n| elevation.contains_key(n.as_str()));
        let mut order: Vec<(f32, &String)> = levels
            .iter()
            .map(|(name, units)| {
                if by_elevation {
                    return (elevation[name.as_str()], name);
                }
                let bases: Vec<f32> = units.iter().map(|&u| bounds[u as usize].min[2]).filter(|z| z.is_finite()).collect();
                (bases.iter().sum::<f32>() / bases.len().max(1) as f32, name)
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        let mut storey = vec![None; elements.len()];
        for (i, (_, name)) in order.iter().enumerate() {
            for &u in &levels[*name] {
                storey[u as usize] = Some(i as u32);
            }
        }

        let (mut rest, mut building) = (Bounds::empty(), Bounds::empty());
        for (b, storey) in bounds.iter().zip(&storey).filter(|(b, _)| b.is_valid()) {
            rest.extend(b.min);
            rest.extend(b.max);
            if storey.is_some() {
                building.extend(b.min);
                building.extend(b.max);
            }
        }
        let storeys = if building.is_valid() { order.len() } else { 0 };
        Self { vertex_units, bounds, storey, storeys, rest, building }
    }

    /// Units, the unused unit 0 included: the length of [`ExplodeIndex::offsets`].
    pub fn len(&self) -> usize {
        self.bounds.len()
    }

    /// Storeys with elements (0: storeys mode is not available).
    pub fn storeys(&self) -> usize {
        self.storeys
    }

    /// The mode in effect: storeys only when the model has them.
    pub fn mode(&self, wanted: ExplodeMode) -> ExplodeMode {
        if self.storeys > 0 { wanted } else { ExplodeMode::Elements }
    }

    /// The unit of vertex `vertex` of layer `layer` (0 when unknown).
    pub fn unit(&self, layer: usize, vertex: u32) -> usize {
        self.vertex_units.get(layer).and_then(|u| u.get(vertex as usize)).map_or(0, |&u| u as usize)
    }

    /// The offset (world, w unused) of every unit for `amount` (0..1 per axis) in `mode`, the
    /// model turned by `m` (the up-axis override). Storey i moves by (i·fx·1.1·sizeX,
    /// i·fy·1.1·sizeY, i·fz·2·storey height), sizes of the building (the units on a storey;
    /// storey height = its height / storeys); an element by (centre − middle of the model)·2·f.
    pub fn offsets(&self, mode: ExplodeMode, amount: [f32; 3], m: &Mat4) -> Vec<[f32; 4]> {
        let mut out = vec![[0.0; 4]; self.len()];
        if amount == [0.0; 3] || !self.rest.is_valid() {
            return out;
        }
        let f = Vec3::from(amount);
        match self.mode(mode) {
            ExplodeMode::Storeys => {
                let size = Vec3::from(transform_bounds(&self.building, m).size());
                let height = size.z / self.storeys as f32;
                let step = Vec3::new(STOREY_SPREAD_XY * size.x, STOREY_SPREAD_XY * size.y, STOREY_SPREAD_Z * height) * f;
                for (o, storey) in out.iter_mut().zip(&self.storey) {
                    if let Some(i) = storey {
                        *o = (step * *i as f32).extend(0.0).to_array();
                    }
                }
            }
            ExplodeMode::Elements => {
                let middle = Vec3::from(transform_bounds(&self.rest, m).center());
                for (o, b) in out.iter_mut().zip(&self.bounds).filter(|(_, b)| b.is_valid()) {
                    let centre = m.transform_point3(Vec3::from(b.center()));
                    *o = ((centre - middle) * ELEMENT_SPREAD * f).extend(0.0).to_array();
                }
            }
        }
        out
    }

    /// World bounds of the units moved by `offsets`, the model turned by `m`.
    pub fn bounds(&self, offsets: &[[f32; 4]], m: &Mat4) -> Bounds {
        let mut out = Bounds::empty();
        for (b, o) in self.bounds.iter().zip(offsets).filter(|(b, _)| b.is_valid()) {
            let w = transform_bounds(b, m);
            out.extend([0, 1, 2].map(|k| w.min[k] + o[k]));
            out.extend([0, 1, 2].map(|k| w.max[k] + o[k]));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;
    use std::path::PathBuf;

    /// A unit cube of 8 vertices and 12 triangles at `at`.
    fn cube(positions: &mut Vec<f32>, indices: &mut Vec<u32>, at: [f32; 3]) {
        let base = (positions.len() / 3) as u32;
        for k in 0..8 {
            positions.extend([0, 1, 2].map(|a| at[a] + ((k >> a) & 1) as f32));
        }
        const FACES: [u32; 36] = [
            0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5,
        ];
        indices.extend(FACES.map(|i| base + i));
    }

    /// Three storeys of one cube each (levels named low to high "00", "01", "02" at z 0, 3, 6),
    /// a cube without a level, and a second layer without elements.
    fn building(levels: Vec<(String, f32)>) -> Model {
        let (mut positions, mut indices) = (Vec::new(), Vec::new());
        let mut elements = Vec::new();
        let mut props = Vec::new();
        for (k, (id, level)) in [(10u64, Some("01")), (11, Some("00")), (12, Some("02")), (13, None)].into_iter().enumerate() {
            let z = match level {
                Some(l) => 3.0 * l.parse::<f32>().unwrap(),
                None => 0.0,
            };
            elements.push((id, indices.len() as u32, 36));
            cube(&mut positions, &mut indices, [4.0 * k as f32, 0.0, z]);
            let level = level.map(|l| format!(r#","level":"{l}""#)).unwrap_or_default();
            props.push((id, format!(r#"{{"id":"{id}"{level}}}"#)));
        }
        let walls = Layer { name: "Walls".into(), positions, indices, elements, props, ..Layer::default() };
        let (mut positions, mut indices) = (Vec::new(), Vec::new());
        cube(&mut positions, &mut indices, [0.0, 7.0, 0.0]);
        let site = Layer { name: "Site".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("x.rvt"), "Revit", vec![walls, site]);
        model.project.levels = levels;
        model
    }

    fn levels(names: &[(&str, f32)]) -> Vec<(String, f32)> {
        names.iter().map(|(n, z)| (n.to_string(), *z)).collect()
    }

    #[test]
    fn storeys_follow_the_level_elevations() {
        let model = building(levels(&[("00", 0.0), ("01", 3.0), ("02", 6.0)]));
        let index = ExplodeIndex::new(&model);
        // units 1-4: the elements in layer order, 5: the layer without elements
        assert_eq!(index.len(), 6);
        assert_eq!(index.storeys(), 3);
        assert_eq!(index.storey, vec![None, Some(1), Some(0), Some(2), None, None]);
        assert_eq!(index.unit(0, 0), 1);
        assert_eq!(index.unit(0, 8), 2);
        assert_eq!(index.unit(1, 0), 5);
        // the building (the cubes on a storey) spans x 0..9, y 0..1, z 0..7: storey height 7 / 3
        let o = index.offsets(ExplodeMode::Storeys, [0.0, 0.0, 1.0], &Mat4::IDENTITY);
        let h = 7.0 / 3.0;
        assert_eq!(o[2], [0.0; 4], "the lowest storey stays put");
        assert_eq!(o[1], [0.0, 0.0, 2.0 * h, 0.0]);
        assert_eq!(o[3], [0.0, 0.0, 4.0 * h, 0.0]);
        assert_eq!((o[4], o[5]), ([0.0; 4], [0.0; 4]), "no level: stays put");
        // X at 50 %: storeys side by side
        let o = index.offsets(ExplodeMode::Storeys, [0.5, 0.0, 0.0], &Mat4::IDENTITY);
        assert_eq!(o[3], [2.0 * 0.5 * 1.1 * 9.0, 0.0, 0.0, 0.0]);
        // Y at 100 %: the site (y 7..8) does not widen the gap
        let o = index.offsets(ExplodeMode::Storeys, [0.0, 1.0, 0.0], &Mat4::IDENTITY);
        assert_eq!(o[1], [0.0, 1.1, 0.0, 0.0]);
        // at rest nothing moves
        assert!(index.offsets(ExplodeMode::Storeys, [0.0; 3], &Mat4::IDENTITY).iter().all(|o| *o == [0.0; 4]));
    }

    #[test]
    fn storeys_without_elevations_follow_their_elements() {
        // listed high to low and "02" unknown: the cubes' bases decide
        let model = building(levels(&[("01", -50.0), ("00", 40.0)]));
        let index = ExplodeIndex::new(&model);
        assert_eq!(index.storey, vec![None, Some(1), Some(0), Some(2), None, None]);
    }

    #[test]
    fn elements_move_away_from_the_middle() {
        let model = building(Vec::new());
        let index = ExplodeIndex::new(&model);
        // the middle of the model is (6.5, 4, 3.5); cube 12 (x 8..9, z 6..7) is centred at (8.5, 0.5, 6.5)
        let o = index.offsets(ExplodeMode::Elements, [1.0, 0.5, 0.0], &Mat4::IDENTITY);
        assert_eq!(o[3], [(8.5 - 6.5) * 2.0, (0.5 - 4.0) * 2.0 * 0.5, 0.0, 0.0]);
        // the layer without elements moves as one: centred at (0.5, 7.5, 0.5)
        assert_eq!(o[5], [(0.5 - 6.5) * 2.0, (7.5 - 4.0) * 2.0 * 0.5, 0.0, 0.0]);
        // the outer cubes: x 0..1 and 12..13 move by -12 and +12
        let b = index.bounds(&o, &Mat4::IDENTITY);
        assert_eq!((b.min[0], b.max[0]), (-12.0, 25.0));
    }

    #[test]
    fn without_levels_storeys_mode_explodes_elements() {
        let mut model = building(Vec::new());
        model.props.clear();
        let index = ExplodeIndex::new(&model);
        assert_eq!(index.storeys(), 0);
        assert_eq!(index.mode(ExplodeMode::Storeys), ExplodeMode::Elements);
        let m = Mat4::IDENTITY;
        assert_eq!(index.offsets(ExplodeMode::Storeys, [1.0; 3], &m), index.offsets(ExplodeMode::Elements, [1.0; 3], &m));
    }

    #[test]
    fn shared_vertices_do_not_drag_neighbours_along() {
        // two triangles of different elements sharing an edge: after loading, each has its own
        let layer = Layer {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 1, 3, 2],
            elements: vec![(1, 0, 3), (2, 3, 3)],
            ..Layer::default()
        };
        let model = Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer]);
        let index = ExplodeIndex::new(&model);
        let tri = |t: usize| model.layers[0].indices[3 * t..3 * t + 3].iter().map(|&v| index.unit(0, v)).collect::<Vec<_>>();
        assert_eq!((tri(0), tri(1)), (vec![1, 1, 1], vec![2, 2, 2]));
    }
}
