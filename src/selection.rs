//! What is selected: elements, or whole layers of formats without elements, in pick order.

use crate::model::Model;
use crate::units::FastHashSet;

/// What a click selects: an element, or a whole layer when the format has no elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pickable {
    Element(u64),
    /// A layer of the shown model (index in `Model::layers`) without elements.
    Layer(usize),
}

/// The selection, in pick order and without duplicates; the first item is the one whose
/// properties are shown.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    items: Vec<Pickable>,
    set: FastHashSet<Pickable>,
}

impl Selection {
    /// Only `item`, or nothing.
    pub fn set(&mut self, item: Option<Pickable>) {
        self.clear();
        self.extend(item);
    }

    /// Adds `item`, or removes it when it is selected.
    pub fn toggle(&mut self, item: Pickable) {
        if self.set.remove(&item) {
            self.items.retain(|&i| i != item);
        } else {
            self.extend([item]);
        }
    }

    /// Adds the items that are not selected yet, in their order.
    pub fn extend(&mut self, items: impl IntoIterator<Item = Pickable>) {
        let items = items.into_iter();
        self.set.reserve(items.size_hint().0);
        self.items.reserve(items.size_hint().0);
        for item in items {
            if self.set.insert(item) {
                self.items.push(item);
            }
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.set.clear();
    }

    pub fn items(&self) -> &[Pickable] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, item: Pickable) -> bool {
        self.set.contains(&item)
    }

    /// The first item picked.
    pub fn primary(&self) -> Option<Pickable> {
        self.items.first().copied()
    }

    /// Where the selection is drawn: (layer, first index, index count), layer by layer. One pass
    /// over the layers, so it stays fast for a selection of every element.
    pub fn ranges(&self, model: &Model) -> Vec<(usize, u32, u32)> {
        let mut out = Vec::new();
        if self.is_empty() {
            return out;
        }
        for (l, layer) in model.layers.iter().enumerate() {
            if self.set.contains(&Pickable::Layer(l)) {
                out.push((l, 0, layer.indices.len() as u32));
                continue;
            }
            out.extend(layer.elements.iter().filter(|e| self.set.contains(&Pickable::Element(e.0))).map(|e| (l, e.1, e.2)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;

    #[test]
    fn toggle_removes_and_keeps_order() {
        let mut s = Selection::default();
        s.extend([Pickable::Element(1), Pickable::Element(2), Pickable::Element(3)]);
        s.toggle(Pickable::Element(2));
        assert_eq!(s.items(), &[Pickable::Element(1), Pickable::Element(3)]);
        s.toggle(Pickable::Element(2));
        assert_eq!(s.items().last(), Some(&Pickable::Element(2)));
    }

    #[test]
    fn extend_skips_duplicates() {
        let mut s = Selection::default();
        s.extend([Pickable::Element(1), Pickable::Element(1)]);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn ranges_cover_every_item() {
        let walls = Layer {
            name: "Walls".into(),
            positions: vec![0.0; 24],
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        let mesh = Layer { name: "Mesh".into(), positions: vec![0.0; 9], indices: vec![0, 1, 2], ..Layer::default() };
        let model = Model::new(PathBuf::from("x.rvt"), "Revit", vec![walls, mesh]);
        let mut s = Selection::default();
        s.extend([Pickable::Element(20), Pickable::Layer(1)]);
        assert_eq!(s.ranges(&model), vec![(0, 6, 6), (1, 0, 3)]);
    }
}
