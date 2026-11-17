//! Exploded view (as BIM Vision's explode sliders): pulls the model apart along X, Y and Z, so
//! every element can be seen (for a quantity takeoff, say) without zooming in. Per storey (Revit
//! levels) or per element, away from the middle of the model; the same rules as the web app's
//! `ExplodeManager`.
//!
//! What moves together is a unit: an element (all its ranges, over all layers), the triangles of
//! a layer outside its elements, or a whole layer without elements. Structural nodes, members and
//! surfaces are units too (their bounds a point or a segment box, their storey the nearest level
//! below); a member with a solid shares one unit with its triangles. The units and their bounds at
//! rest are indexed once per model; the renderer adds the offset of each vertex's unit in the
//! vertex shader, so the geometry never changes and 0 % on every axis is the model as loaded.
//! The same units carry the hidden and selected flags of `crate::units`.

use crate::model::{Bounds, Model};
use crate::units::FastHashMap;
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

/// What a unit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitOwner {
    Element(u64),
    /// The triangles of a layer (index in `Model::layers`) outside its elements.
    LayerRest(usize),
}

/// The units of a model at rest.
#[derive(Debug)]
pub struct ExplodeIndex {
    /// Per layer, the unit of every vertex (0: none, never moves).
    pub vertex_units: Vec<Vec<u32>>,
    /// Per unit (0: none): the element it is, or the layer whose triangles outside elements it is.
    owners: Vec<Option<UnitOwner>>,
    /// The unit of every element, and per layer the unit of its triangles outside elements.
    element_units: FastHashMap<u64, u32>,
    rest_units: Vec<Option<u32>>,
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
    /// The unit of every structural node, member and surface (by element id). Loads, supports,
    /// hinges and the other decorations take the unit of the item they sit on
    /// (`StructuralItem::unit_key`).
    structural: HashMap<u64, u32>,
    /// The unit of every item of the model's structural display (nodes, members), in display
    /// order: written into the instances when they are uploaded.
    display_units: Vec<u32>,
    /// What the camera frames when it fits the model, when that is less than all of it: the
    /// units without a few small ones that stand far from the rest (model space). See
    /// [`framing`].
    framed: Option<Bounds>,
}

/// A unit stands far from the rest when the gap between them is more than this many times the
/// size of the rest (or of the unit, when that is larger)...
const STRAY_GAP: f32 = 10.0;
/// ...and the units that stand far hold together at most this part of the model's triangles.
const STRAY_SHARE: f64 = 0.01;

/// The bounds of the units (bounds, triangles) without the few small ones that stand far from
/// the rest: an object far from zero in a file whose model lies elsewhere (a default cube
/// thousands of kilometres from a map), which would leave the model a dot in the fitted view.
/// None when no unit stands far, or when those that do hold more than [`STRAY_SHARE`] of the
/// triangles: then they are part of the model.
///
/// The rest grows from the middle of the model (per axis the median of the units' centres,
/// weighted by their triangles) outwards: a unit joins it unless the gap between them is more
/// than [`STRAY_GAP`] times the size of the larger of the two.
fn framing(units: &[(Bounds, u64)]) -> Option<Bounds> {
    let units: Vec<(Bounds, u64)> = units.iter().filter(|(b, _)| b.is_valid()).map(|&(b, w)| (b, w.max(1))).collect();
    if units.len() < 2 {
        return None;
    }
    let total: u64 = units.iter().map(|u| u.1).sum();
    let middle = [0, 1, 2].map(|axis| {
        let mut centres: Vec<(f32, u64)> = units.iter().map(|(b, w)| (b.center()[axis], *w)).collect();
        centres.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut seen = 0;
        centres
            .iter()
            .find(|(_, w)| {
                seen += w;
                2 * seen >= total
            })
            .map_or(0.0, |c| c.0)
    });
    // the gap between two boxes (0 when they touch or overlap)
    let gap = |a: &Bounds, b: &Bounds| (0..3).map(|i| (a.min[i] - b.max[i]).max(b.min[i] - a.max[i]).max(0.0).powi(2)).sum::<f32>().sqrt();
    let diagonal = |b: &Bounds| 2.0 * b.radius();
    let at_middle = Bounds { min: middle, max: middle };
    let mut order: Vec<(f32, &(Bounds, u64))> = units.iter().map(|u| (gap(&u.0, &at_middle), u)).collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut framed, mut stray) = (Bounds::empty(), 0u64);
    for (_, (b, w)) in order {
        if framed.is_valid() && gap(&framed, b) > STRAY_GAP * diagonal(&framed).max(diagonal(b)).max(1e-6) {
            stray += w;
            continue;
        }
        framed.extend(b.min);
        framed.extend(b.max);
    }
    (stray > 0 && stray as f64 <= STRAY_SHARE * total as f64).then_some(framed)
}

impl ExplodeIndex {
    pub fn new(model: &Model) -> Self {
        // per unit: the element it is (None: the rest of a layer)
        let mut elements: Vec<Option<u64>> = vec![None];
        let mut element_units: FastHashMap<u64, u32> = FastHashMap::default();
        let mut bounds = vec![Bounds::empty()];
        // per unit: its triangles, for the framing
        let mut triangles = vec![0u64];
        let mut vertex_units = Vec::with_capacity(model.layers.len());
        let mut owners: Vec<Option<UnitOwner>> = vec![None];
        let mut rest_units = Vec::with_capacity(model.layers.len());
        for (l, layer) in model.layers.iter().enumerate() {
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
                        owners.push(Some(UnitOwner::Element(id)));
                        elements.len() as u32 - 1
                    }),
                    None => *rest.get_or_insert_with(|| {
                        elements.push(None);
                        owners.push(Some(UnitOwner::LayerRest(l)));
                        elements.len() as u32 - 1
                    }),
                };
                bounds.resize(elements.len(), Bounds::empty());
                triangles.resize(elements.len(), 0);
                triangles[unit as usize] += tris.len() as u64 / 3;
                for &i in tris {
                    let Some(p) = layer.positions.get(3 * i as usize..3 * i as usize + 3) else { continue };
                    units[i as usize] = unit;
                    if p.iter().all(|v| v.is_finite()) {
                        bounds[unit as usize].extend([p[0], p[1], p[2]]);
                    }
                }
            }
            vertex_units.push(units);
            rest_units.push(rest);
        }
        // structural units, after the layers: a member drawn as a solid keeps its triangles' unit
        let mut structural = HashMap::new();
        for (id, b) in model.structural_index.unit_bounds(&model.structural, &model.origins) {
            let unit = *element_units.entry(id).or_insert_with(|| {
                elements.push(Some(id));
                owners.push(Some(UnitOwner::Element(id)));
                elements.len() as u32 - 1
            });
            bounds.resize(elements.len(), Bounds::empty());
            bounds[unit as usize].extend(b.min);
            bounds[unit as usize].extend(b.max);
            structural.insert(id, unit);
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
        // a structural unit without a level of its own: the nearest storey at or below its base
        for &u in structural.values() {
            let base = bounds[u as usize].min[2];
            if storey[u as usize].is_none() && base.is_finite() {
                storey[u as usize] = order.iter().rposition(|(z, _)| *z <= base + 1e-3).map(|i| i as u32);
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
        // the framing; a model with points (a cloud) is framed whole, its points are no units
        triangles.resize(bounds.len(), 0);
        let units: Vec<(Bounds, u64)> = bounds.iter().copied().zip(triangles.iter().copied()).skip(1).collect();
        let framed = if model.layers.iter().any(|l| l.points) { None } else { framing(&units) };
        let display_units = model
            .structural_display
            .as_ref()
            .map(|d| d.items.iter().map(|i| structural.get(&i.element).copied().unwrap_or(0)).collect())
            .unwrap_or_default();
        Self { vertex_units, owners, element_units, rest_units, bounds, storey, storeys, rest, building, structural, display_units, framed }
    }

    /// What the camera frames when it fits the model at rest (model space), when that is less
    /// than all of it: the model without a few small things far from the rest.
    pub fn framed(&self) -> Option<Bounds> {
        self.framed
    }

    /// All units at rest (model space).
    pub fn rest(&self) -> Bounds {
        self.rest
    }

    /// The unit of a structural node, member or surface.
    pub fn structural_unit(&self, id: u64) -> Option<u32> {
        self.structural.get(&id).copied()
    }

    /// The unit of every item of the model's structural display, in display order.
    pub fn display_units(&self) -> &[u32] {
        &self.display_units
    }

    /// The bounds of a unit at rest (model space).
    pub fn unit_bounds(&self, unit: u32) -> Option<Bounds> {
        self.bounds.get(unit as usize).copied().filter(Bounds::is_valid)
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

    /// The unit of element `id`.
    pub fn element_unit(&self, id: u64) -> Option<usize> {
        self.element_units.get(&id).map(|&u| u as usize)
    }

    /// The unit of the triangles of layer `layer` outside its elements (all of them for a layer
    /// without elements).
    pub fn layer_rest_unit(&self, layer: usize) -> Option<usize> {
        self.rest_units.get(layer).copied().flatten().map(|u| u as usize)
    }

    /// What unit `unit` is (None for unit 0).
    pub fn owner(&self, unit: usize) -> Option<UnitOwner> {
        self.owners.get(unit).copied().flatten()
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
    pub(super) fn cube(positions: &mut Vec<f32>, indices: &mut Vec<u32>, at: [f32; 3]) {
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

#[cfg(test)]
mod framing_tests {
    use super::*;
    use crate::model::Layer;
    use std::path::PathBuf;

    /// A grid of 20 x 10 unit cubes 2 m apart (the model, from 0,0,0 to 39,19,1), each an element,
    /// and unit cubes at `others`, each an element too.
    fn site(others: &[[f32; 3]], points: bool) -> Model {
        let (mut positions, mut indices, mut elements) = (Vec::new(), Vec::new(), Vec::new());
        let grid = (0..200).map(|k| [2.0 * (k % 20) as f32, 2.0 * (k / 20) as f32, 0.0]);
        for (id, at) in grid.chain(others.iter().copied()).enumerate() {
            elements.push((id as u64 + 1, indices.len() as u32, 36));
            super::tests::cube(&mut positions, &mut indices, at);
        }
        let mut layers = vec![Layer { name: "Model".into(), positions, indices, elements, ..Layer::default() }];
        if points {
            layers.push(Layer { name: "Cloud".into(), positions: vec![1.0, 1.0, 1.0, 3.0, 3.0, 3.0], points: true, ..Layer::default() });
        }
        Model::new(PathBuf::from("site.blend"), "Blender", layers)
    }

    const MODEL: Bounds = Bounds { min: [0.0, 0.0, 0.0], max: [39.0, 19.0, 1.0] };

    #[test]
    fn a_small_thing_far_away_is_left_out_of_the_framing() {
        // a cube a thousand kilometres away, as a default cube left far from a map
        let index = ExplodeIndex::new(&site(&[[1.0e6, -2.0e6, 0.0]], false));
        assert_eq!(index.framed(), Some(MODEL));
        // the model itself keeps it
        assert_eq!(index.rest().max[0], 1.0e6 + 1.0);
        // nothing stands far: the model is framed whole
        assert_eq!(ExplodeIndex::new(&site(&[], false)).framed(), None);
    }

    #[test]
    fn what_is_near_or_holds_much_of_the_model_is_framed() {
        // a shed 30 m from the grid (less than ten times the grid's size)
        assert_eq!(ExplodeIndex::new(&site(&[[70.0, 0.0, 0.0]], false)).framed(), None);
        // five cubes far away: more than 1 % of the triangles, so part of the model
        let far: Vec<[f32; 3]> = (0..5).map(|k| [1.0e6 + 2.0 * k as f32, 0.0, 0.0]).collect();
        assert_eq!(ExplodeIndex::new(&site(&far, false)).framed(), None);
        // a model with a point cloud is framed whole: its points are no units
        assert_eq!(ExplodeIndex::new(&site(&[[1.0e6, 0.0, 0.0]], true)).framed(), None);
    }
}
