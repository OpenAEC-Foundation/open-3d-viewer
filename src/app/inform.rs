//! What the elements of the open models say, made visible: the model coloured by a property
//! with a legend of its values, sets of elements kept under a name, and takeoffs of their
//! information and quantities as a table that goes to Excel.
//!
//! A property is one of an element's own (its file's), or what every element has: the model it
//! comes from, its layer and its class.

use super::collab::{field, Side};
use super::{App, Message};
use crate::collab::rules::{matches, SelectionSet, Takeoff};
use crate::explode::UnitOwner;
use crate::model::{Model, Schedule, ScheduleRow};
use crate::selection::Pickable;
use glam::Vec3;
use iced::widget::{button, column, container, row, Space};
use iced::{Alignment, Element, Length, Task};
use openaec_ui::design::a11y::{node as named, Node, Role};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D, Tone};
use openaec_ui::design::Look;
use openaec_ui::Language;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// The properties every element has: its model, its layer, its class.
pub const MODEL: &str = "@model";
pub const LAYER: &str = "@layer";
pub const CLASS: &str = "@class";
/// The type: the file's own, else the name without the number an exporter adds to it
/// (`Basic Wall:CLT 100:1763035` is of type `Basic Wall:CLT 100`).
pub const TYPE: &str = "@type";

/// The colour of the elements without the value coloured by.
const NONE: [u8; 3] = [0xC8, 0xC8, 0xC8];

/// A property to colour, filter or group by, as a list to choose from shows it; the empty key
/// is none.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub key: String,
    label: String,
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A value of the property coloured by: its colour and its elements.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendRow {
    pub value: String,
    pub color: [u8; 3],
    pub ids: Vec<u64>,
}

#[derive(Debug, Clone)]
pub enum Event {
    ColourBy(Key),
    /// The elements of a value of the legend: selected.
    LegendPick(usize),
    /// A value of the legend kept as a set.
    LegendSet(usize),
    SetName(String),
    /// The selection kept as a set under the name typed.
    SaveSet,
    /// A set: selected (0), shown alone (1) or hidden (2).
    ApplySet(usize, u8),
    RemoveSet(usize),
    TakeoffName(String),
    TakeoffSide(Side),
    TakeoffFilter(Key),
    TakeoffValue(String),
    TakeoffGroup(usize, Key),
    MakeTakeoff,
    KeepTakeoff,
    RunTakeoff(usize),
    RemoveTakeoff(usize),
}

/// What the panel keeps of it.
#[derive(Debug, Default)]
pub struct State {
    /// The properties to choose from (the first: none), for the model with this id.
    pub keys: Vec<Key>,
    keys_for: Option<(u64, Language)>,
    /// The property coloured by, its values, and the colour of each element.
    pub colour_by: String,
    pub legend: Vec<LegendRow>,
    pub painted: Arc<HashMap<u64, [u8; 3]>>,
    pub set_name: String,
    /// The takeoff being set up.
    pub draft: Takeoff,
}

/// The value of an element's own property: a text as it is, another value as JSON writes it.
fn prop_value(props: &str, key: &str) -> Option<String> {
    if let Some(v) = field(props, key) {
        return Some(v.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if !props.contains(&format!("\"{key}\"")) {
        return None;
    }
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(props).ok()?;
    map.get(key).filter(|v| !v.is_null()).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
}

/// The keys an element keeps from one version of its model to the next.
fn lasting_key(model: &Model, id: u64) -> String {
    let props = model.props.get(&id).map(String::as_str).unwrap_or("");
    ["GlobalId", "UniqueId", "id"].iter().find_map(|k| field(props, k).filter(|v| !v.is_empty()).map(str::to_string)).unwrap_or_else(|| format!("#{id}"))
}

/// A colour for the `k`th value: hues a golden angle apart, so neighbours differ.
pub fn colour(k: usize) -> [u8; 3] {
    let h = (k as f32 * 137.508) % 360.0 / 60.0;
    let (s, v) = (0.62, if k % 2 == 0 { 0.88 } else { 0.72 });
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r, g, b].map(|t| ((t + m) * 255.0).round() as u8)
}

/// How long, how large and how much an element is, from its triangles (metres): its longest
/// side, the area of its largest face turned one way (a wall's side, a slab's top) and its
/// volume when it is closed.
pub fn quantities(triangles: &[[Vec3; 3]], closed: bool) -> (f32, f32, Option<f32>) {
    if triangles.is_empty() {
        return (0.0, 0.0, None);
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    // the area turned each way: a face split in many triangles (around an opening) counts whole
    let mut ways: Vec<(Vec3, f32)> = Vec::new();
    for t in triangles {
        for p in t {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        let (way, area) = (n.normalize_or_zero(), 0.5 * n.length());
        if area <= 0.0 {
            continue;
        }
        match ways.iter().position(|(w, _)| w.dot(way) > 0.98) {
            Some(k) => ways[k].1 += area,
            None if ways.len() < 256 => ways.push((way, area)),
            None => {}
        }
    }
    let length = (hi - lo).max_element();
    let area = ways.iter().map(|w| w.1).fold(0.0f32, f32::max);
    let volume = closed.then(|| triangles.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum::<f32>().abs() / 6.0);
    (length, area, volume)
}

impl App {
    /// The drawn elements of the model, each once, with the layer it is first drawn in.
    fn inform_elements(model: &Model) -> Vec<(u64, usize)> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (l, layer) in model.layers.iter().enumerate().filter(|(_, layer)| !layer.points) {
            for &(id, ..) in &layer.elements {
                if seen.insert(id) {
                    out.push((id, l));
                }
            }
        }
        out
    }

    /// The value of property `key` of element `id`, drawn in layer `layer`.
    pub(crate) fn inform_value(&self, model: &Model, id: u64, layer: usize, key: &str) -> Option<String> {
        let props = model.props.get(&id).map(String::as_str).unwrap_or("");
        match key {
            MODEL => {
                let k = model.element_key(id).model as usize;
                self.models.get(k).or(self.models.first()).and_then(|e| e.model.path.file_name()).map(|n| n.to_string_lossy().into_owned())
            }
            LAYER => model.layers.get(layer).map(|l| l.name.clone()),
            CLASS => field(props, "IfcType").or_else(|| field(props, "category")).map(str::to_string).or_else(|| model.layers.get(layer).map(|l| l.name.clone())),
            TYPE => ["ObjectType", "type", "Type", "family"].iter().find_map(|k| field(props, k).filter(|v| !v.trim().is_empty())).map(str::to_string).or_else(|| {
                let name = field(props, "Name")?.trim();
                let bare = match name.rsplit_once(':') {
                    Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
                    _ => name,
                };
                (!bare.is_empty()).then(|| bare.to_string())
            }),
            _ => prop_value(props, key).filter(|v| !v.trim().is_empty()),
        }
    }

    /// The properties to choose from: none, the model, the layer, the class, then those the
    /// elements have, the most common first. Worked out once per model and language.
    pub(super) fn inform_refresh(&mut self) {
        let l = self.collab.language;
        let id = self.model.as_ref().map(|m| m.id);
        if id.is_none() {
            self.collab.inform.keys.clear();
            self.collab.inform.keys_for = None;
            return;
        }
        let for_now = id.map(|id| (id, l));
        if self.collab.inform.keys_for == for_now {
            return;
        }
        let model_changed = self.collab.inform.keys_for.map(|k| k.0) != id;
        self.collab.inform.keys_for = for_now;
        let Some(model) = self.model.clone() else { return };
        let mut count: HashMap<String, usize> = HashMap::new();
        for json in model.props.values() {
            if let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) {
                for (k, v) in map {
                    if !v.is_null() && !matches!(k.as_str(), "GlobalId" | "ExpressId" | "id" | "UniqueId") {
                        *count.entry(k).or_default() += 1;
                    }
                }
            }
        }
        let mut found: Vec<(String, usize)> = count.into_iter().collect();
        found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let key = |key: &str, label: &str| Key { key: key.to_string(), label: label.to_string() };
        self.collab.inform.keys = [key("", "—"), key(MODEL, l.text("Model", "Model")), key(LAYER, l.text("Laag", "Layer")), key(CLASS, l.text("Klasse", "Class")), key(TYPE, l.text("Type", "Type"))]
            .into_iter()
            .chain(found.into_iter().map(|(k, _)| key(&k, &k)))
            .collect();
        if model_changed {
            self.collab.inform.colour_by.clear();
            self.collab.inform.legend.clear();
            self.collab.inform.painted = Arc::default();
            self.update_units();
        }
    }

    /// Colours the model by property `key` (none: as it is): a colour for each value, the most
    /// common first, grey for the elements without one.
    pub(crate) fn colour_by(&mut self, key: &str) {
        self.collab.inform.colour_by = key.to_string();
        let mut legend: Vec<LegendRow> = Vec::new();
        let mut painted = HashMap::new();
        if let Some(model) = self.model.clone().filter(|_| !key.is_empty()) {
            let mut values: HashMap<String, Vec<u64>> = HashMap::new();
            let mut without = Vec::new();
            for (id, layer) in Self::inform_elements(&model) {
                match self.inform_value(&model, id, layer, key) {
                    Some(v) => values.entry(v).or_default().push(id),
                    None => without.push(id),
                }
            }
            let mut values: Vec<(String, Vec<u64>)> = values.into_iter().collect();
            values.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
            for (k, (value, ids)) in values.into_iter().enumerate() {
                let color = colour(k);
                painted.extend(ids.iter().map(|&id| (id, color)));
                legend.push(LegendRow { value, color, ids });
            }
            if !without.is_empty() {
                let l = self.collab.language;
                legend.push(LegendRow { value: l.text("(geen waarde)", "(no value)").to_string(), color: NONE, ids: without });
            }
        }
        self.collab.inform.legend = legend;
        self.collab.inform.painted = Arc::new(painted);
        self.update_units();
    }

    /// The look of each unit when the model is coloured by a property: its value's colour, the
    /// elements without a value grey and see-through. None when it is not coloured.
    pub(crate) fn painted_looks(&self, units: usize, owner: impl Fn(usize) -> Option<UnitOwner>) -> Option<Vec<[f32; 4]>> {
        let painted = &self.collab.inform.painted;
        if painted.is_empty() {
            return None;
        }
        let pack = |[r, g, b]: [u8; 3]| ((u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)) + 1) as f32;
        Some(
            (0..units)
                .map(|u| match owner(u).filter(|_| u > 0) {
                    Some(UnitOwner::Element(id)) => match painted.get(&id) {
                        Some(&c) => [pack(c), 0.0, 0.0, 0.0],
                        None => [pack(NONE), 0.0, 0.6, 0.0],
                    },
                    _ if u > 0 => [pack(NONE), 0.0, 0.6, 0.0],
                    _ => [0.0; 4],
                })
                .collect(),
        )
    }

    /// The elements of a set as the open models have them now.
    pub(crate) fn set_elements(&self, set: &SelectionSet) -> Vec<u64> {
        let Some(model) = self.model.clone() else { return Vec::new() };
        let elements = Self::inform_elements(&model);
        if !set.property.is_empty() {
            return elements.into_iter().filter(|&(id, layer)| self.inform_value(&model, id, layer, &set.property).is_some_and(|v| matches(&set.value, &v))).map(|(id, _)| id).collect();
        }
        let wanted: HashSet<&str> = set.keys.iter().map(String::as_str).collect();
        elements.into_iter().map(|(id, _)| id).filter(|&id| wanted.contains(lasting_key(&model, id).as_str())).collect()
    }

    /// Makes the table of a takeoff: a row for each group (or each element), how many, and their
    /// length, area and volume summed; a total under them.
    pub(crate) fn takeoff_schedule(&self, takeoff: &Takeoff) -> Option<Schedule> {
        let model = self.model.clone()?;
        let l = self.collab.language;
        let file = |id: u64| self.inform_value(&model, id, 0, MODEL).unwrap_or_default();
        let taken: Vec<(u64, usize)> = Self::inform_elements(&model)
            .into_iter()
            .filter(|&(id, layer)| {
                let class = self.inform_value(&model, id, layer, CLASS).unwrap_or_default();
                takeoff.take.takes_element(id, &file(id), &model.layers[layer].name, &class)
            })
            .filter(|&(id, layer)| takeoff.property.is_empty() || self.inform_value(&model, id, layer, &takeoff.property).is_some_and(|v| matches(&takeoff.value, &v)))
            .collect();
        // each element's quantities, once
        let measure = |id: u64| -> (f32, f32, Option<f32>) {
            let mut triangles = Vec::new();
            let mut closed = true;
            for (layer, first, count) in model.element_ranges(id) {
                let layer = &model.layers[layer];
                let Some(indices) = layer.indices.get(first as usize..(first + count) as usize) else { continue };
                closed &= crate::model::encloses_volume(&layer.positions, indices);
                let at = |i: u32| layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
                triangles.extend(indices.chunks_exact(3).filter_map(|t| Some([at(t[0])?, at(t[1])?, at(t[2])?])));
            }
            quantities(&triangles, closed)
        };
        let label = |key: &str| self.collab.inform.keys.iter().find(|k| k.key == key).map_or_else(|| key.to_string(), |k| k.label.clone());
        let mut columns: Vec<String> = if takeoff.group_by.is_empty() { vec![l.text("Element", "Element").to_string()] } else { takeoff.group_by.iter().map(|k| label(k)).collect() };
        columns.extend([l.text("Aantal", "Count"), l.text("Lengte (m)", "Length (m)"), l.text("Oppervlak (m²)", "Area (m²)"), l.text("Volume (m³)", "Volume (m³)")].map(str::to_string));
        let mut groups: BTreeMap<Vec<String>, Vec<u64>> = BTreeMap::new();
        for &(id, layer) in &taken {
            let key: Vec<String> = if takeoff.group_by.is_empty() {
                vec![model.props.get(&id).and_then(|p| field(p, "Name")).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| format!("#{id}")), format!("{id:020}")]
            } else {
                takeoff.group_by.iter().map(|k| self.inform_value(&model, id, layer, k).unwrap_or_default()).collect()
            };
            groups.entry(key).or_default().push(id);
        }
        let number = |v: f32, places: usize| format!("{v:.places$}");
        let mut rows = Vec::new();
        let mut total = (0usize, 0.0f32, 0.0f32, 0.0f32, true);
        for (key, ids) in groups {
            let (mut length, mut area, mut volume, mut all_closed) = (0.0f32, 0.0f32, 0.0f32, true);
            for &id in &ids {
                let (len, a, v) = measure(id);
                length += len;
                area += a;
                match v {
                    Some(v) => volume += v,
                    None => all_closed = false,
                }
            }
            total = (total.0 + ids.len(), total.1 + length, total.2 + area, total.3 + volume, total.4 && all_closed);
            let shown: Vec<String> = if takeoff.group_by.is_empty() { key.into_iter().take(1).collect() } else { key };
            let mut cells: Vec<Option<String>> = shown.into_iter().map(|v| Some(if v.is_empty() { "—".to_string() } else { v })).collect();
            cells.extend([Some(ids.len().to_string()), Some(number(length, 2)), Some(number(area, 2)), Some(if all_closed { number(volume, 3) } else { format!("{} *", number(volume, 3)) })]);
            rows.push(ScheduleRow { cells, images: Vec::new(), elements: ids, total: false });
        }
        let leading = columns.len() - 4;
        let mut cells: Vec<Option<String>> = (0..leading).map(|k| Some(if k == 0 { l.text("Totaal", "Total").to_string() } else { String::new() })).collect();
        cells.extend([Some(total.0.to_string()), Some(number(total.1, 2)), Some(number(total.2, 2)), Some(if total.4 { number(total.3, 3) } else { format!("{} *", number(total.3, 3)) })]);
        rows.push(ScheduleRow { cells, images: Vec::new(), elements: taken.iter().map(|e| e.0).collect(), total: true });
        let name = if takeoff.name.trim().is_empty() { l.text("Uittrekstaat", "Takeoff").to_string() } else { takeoff.name.trim().to_string() };
        Some(Schedule { name: name.clone(), title: name, columns, rows, computed: true })
    }

    /// Shows a takeoff's table under the 3D view, in place of an older one of its name.
    fn show_takeoff(&mut self, takeoff: &Takeoff) -> Task<Message> {
        let l = self.collab.language;
        let Some(schedule) = self.takeoff_schedule(takeoff) else {
            self.collab_note(Tone::Warning, l.text("Open eerst een model.", "Open a model first."));
            return Task::none();
        };
        let count = schedule.rows.last().map_or(0, |r| r.elements.len());
        let i = match self.schedules.iter().position(|s| s.name == schedule.name) {
            Some(i) => {
                self.schedules[i] = schedule;
                i
            }
            None => {
                self.schedules.push(schedule);
                self.schedules.len() - 1
            }
        };
        self.collab_note(Tone::Success, format!("{count} {}", l.text("elementen in de staat; Excel via de knop boven de tabel.", "elements in the takeoff; Excel from the button above the table.")));
        self.open_schedule(i)
    }

    pub(super) fn inform_handle(&mut self, event: Event) -> Task<Message> {
        let l = self.collab.language;
        match event {
            Event::ColourBy(key) => self.colour_by(&key.key),
            Event::LegendPick(k) => {
                if let Some(row) = self.collab.inform.legend.get(k) {
                    let ids = row.ids.clone();
                    self.sel.clear();
                    self.sel.extend(ids.into_iter().map(Pickable::Element));
                    self.selection_changed();
                }
            }
            Event::LegendSet(k) => {
                let Some(row) = self.collab.inform.legend.get(k).cloned() else { return Task::none() };
                let property = self.collab.inform.colour_by.clone();
                let label = self.collab.inform.keys.iter().find(|x| x.key == property).map_or_else(|| property.clone(), |x| x.label.clone());
                // the elements without a value have no value to keep: they are kept as they are
                let set = if row.color == NONE && k + 1 == self.collab.inform.legend.len() {
                    let model = self.model.clone();
                    SelectionSet { name: format!("{label}: {}", row.value), keys: model.map(|m| row.ids.iter().map(|&id| lasting_key(&m, id)).collect()).unwrap_or_default(), ..SelectionSet::default() }
                } else {
                    SelectionSet { name: format!("{label} = {}", row.value), property, value: row.value, ..SelectionSet::default() }
                };
                if let Some(rules) = self.collab.rules_mut() {
                    rules.selection_sets.push(set);
                }
            }
            Event::SetName(name) => self.collab.inform.set_name = name,
            Event::SaveSet => {
                let Some(model) = self.model.clone() else { return Task::none() };
                let ids: Vec<u64> = self.sel.items().iter().filter_map(|i| if let Pickable::Element(id) = i { Some(*id) } else { None }).collect();
                if ids.is_empty() {
                    self.collab_note(Tone::Warning, l.text("Selecteer eerst de elementen van de set.", "Select the elements of the set first."));
                    return Task::none();
                }
                let typed = std::mem::take(&mut self.collab.inform.set_name);
                let count = self.collab.rules().map_or(0, |r| r.selection_sets.len());
                let name = if typed.trim().is_empty() { format!("{} {}", l.text("Set", "Set"), count + 1) } else { typed.trim().to_string() };
                let set = SelectionSet { name, keys: ids.iter().map(|&id| lasting_key(&model, id)).collect(), ..SelectionSet::default() };
                if let Some(rules) = self.collab.rules_mut() {
                    rules.selection_sets.push(set);
                }
            }
            Event::ApplySet(k, how) => {
                let Some(set) = self.collab.rules().and_then(|r| r.selection_sets.get(k)).cloned() else { return Task::none() };
                let ids = self.set_elements(&set);
                if ids.is_empty() {
                    self.collab_note(Tone::Warning, format!("{}: {}", set.name, l.text("geen elementen in de open modellen.", "no elements in the open models.")));
                    return Task::none();
                }
                self.show_all_elements();
                self.sel.clear();
                self.sel.extend(ids.into_iter().map(Pickable::Element));
                self.selection_changed();
                match how {
                    1 => self.isolate_selection(),
                    2 => self.hide_selection(),
                    _ => {}
                }
            }
            Event::RemoveSet(k) => {
                if let Some(rules) = self.collab.rules_mut().filter(|r| k < r.selection_sets.len()) {
                    rules.selection_sets.remove(k);
                }
            }
            Event::TakeoffName(name) => self.collab.inform.draft.name = name,
            Event::TakeoffSide(side) => self.collab.inform.draft.take = side.selector,
            Event::TakeoffFilter(key) => self.collab.inform.draft.property = key.key,
            Event::TakeoffValue(value) => self.collab.inform.draft.value = value,
            Event::TakeoffGroup(k, key) => {
                let group = &mut self.collab.inform.draft.group_by;
                if key.key.is_empty() {
                    group.truncate(k);
                } else if k < group.len() {
                    group[k] = key.key;
                } else {
                    group.push(key.key);
                }
            }
            Event::MakeTakeoff => {
                let draft = self.collab.inform.draft.clone();
                return self.show_takeoff(&draft);
            }
            Event::KeepTakeoff => {
                let mut draft = self.collab.inform.draft.clone();
                if draft.name.trim().is_empty() {
                    draft.name = format!("{} {}", l.text("Uittrekstaat", "Takeoff"), self.collab.rules().map_or(0, |r| r.takeoffs.len()) + 1);
                }
                if let Some(rules) = self.collab.rules_mut() {
                    match rules.takeoffs.iter().position(|t| t.name == draft.name) {
                        Some(i) => rules.takeoffs[i] = draft,
                        None => rules.takeoffs.push(draft),
                    }
                }
            }
            Event::RunTakeoff(k) => {
                let Some(takeoff) = self.collab.rules().and_then(|r| r.takeoffs.get(k)).cloned() else { return Task::none() };
                self.collab.inform.draft = takeoff.clone();
                return self.show_takeoff(&takeoff);
            }
            Event::RemoveTakeoff(k) => {
                if let Some(rules) = self.collab.rules_mut().filter(|r| k < r.takeoffs.len()) {
                    rules.takeoffs.remove(k);
                }
            }
        }
        Task::none()
    }

    /// The remote's commands: `collab_colours` (colour by a property, or off), `collab_sets`
    /// (list, save the selection, add one by property, apply, remove) and `collab_takeoff` (make
    /// one, keep it in the rule set, list those kept).
    pub(crate) fn inform_command(&mut self, cmd: &str, a: &serde_json::Value) -> Result<serde_json::Value, String> {
        use serde_json::json;
        let text = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let index = |k: &str| a.get(k).and_then(|v| v.as_u64()).map(|v| v as usize);
        let key_named = |app: &App, name: &str| -> Result<Key, String> {
            let wanted = match name.to_ascii_lowercase().as_str() {
                "model" => MODEL.to_string(),
                "layer" => LAYER.to_string(),
                "class" => CLASS.to_string(),
                "type" => TYPE.to_string(),
                "" | "none" | "off" => String::new(),
                _ => name.to_string(),
            };
            app.collab.inform.keys.iter().find(|k| k.key == wanted).cloned().ok_or_else(|| format!("no property {name}; the models have: model, layer, class, {}", app.collab.inform.keys.iter().skip(5).take(40).map(|k| k.key.as_str()).collect::<Vec<_>>().join(", ")))
        };
        let sets_json = |app: &App| -> serde_json::Value {
            json!(app.collab.rules().map(|r| r.selection_sets.iter().enumerate().map(|(k, s)| json!({ "index": k, "name": s.name, "property": s.property, "value": s.value, "keys": s.keys.len(), "elements": app.set_elements(s).len() })).collect::<Vec<_>>()))
        };
        match cmd {
            "collab_colours" => {
                let key = key_named(self, &text("property").unwrap_or_default())?;
                let _ = self.inform_handle(Event::ColourBy(key));
                let state = &self.collab.inform;
                Ok(json!({ "property": state.colour_by, "legend": state.legend.iter().take(100).map(|r| json!({ "value": r.value, "color": format!("#{:02x}{:02x}{:02x}", r.color[0], r.color[1], r.color[2]), "elements": r.ids.len() })).collect::<Vec<_>>() }))
            }
            "collab_sets" => match text("action").unwrap_or_default().as_str() {
                "list" | "" => Ok(json!({ "sets": sets_json(self) })),
                "save" => {
                    if let Some(name) = text("name") {
                        self.collab.inform.set_name = name;
                    }
                    let _ = self.inform_handle(Event::SaveSet);
                    Ok(json!({ "sets": sets_json(self), "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }))
                }
                "add" => {
                    let property = key_named(self, &text("property").ok_or("give the property")?)?.key;
                    let set = SelectionSet { name: text("name").unwrap_or_else(|| format!("{property} = {}", text("value").unwrap_or_default())), property, value: text("value").ok_or("give the value (* for any)")?, ..SelectionSet::default() };
                    self.collab.rules_mut().ok_or("no rule set")?.selection_sets.push(set);
                    Ok(json!({ "sets": sets_json(self) }))
                }
                how @ ("select" | "isolate" | "hide") => {
                    let k = index("index").ok_or("give the set's index")?;
                    let _ = self.inform_handle(Event::ApplySet(k, match how { "isolate" => 1, "hide" => 2, _ => 0 }));
                    Ok(json!({ "selected": self.sel.len(), "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }))
                }
                "remove" => {
                    let _ = self.inform_handle(Event::RemoveSet(index("index").ok_or("give the set's index")?));
                    Ok(json!({ "sets": sets_json(self) }))
                }
                other => Err(format!("collab_sets: unknown action {other} (list, save, add, select, isolate, hide, remove)")),
            },
            _ => match text("action").unwrap_or_default().as_str() {
                "make" | "" | "keep" => {
                    let mut draft = Takeoff { name: text("name").unwrap_or_default(), ..Takeoff::default() };
                    if let Some(classes) = a.get("classes").and_then(|v| v.as_array()) {
                        draft.take.classes = classes.iter().filter_map(|c| c.as_str().map(str::to_string)).collect();
                    }
                    if let Some(property) = text("property") {
                        draft.property = key_named(self, &property)?.key;
                        draft.value = text("value").unwrap_or_else(|| "*".into());
                    }
                    for g in a.get("group_by").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|g| g.as_str()).take(2) {
                        draft.group_by.push(key_named(self, g)?.key);
                    }
                    self.collab.inform.draft = draft;
                    if text("action").as_deref() == Some("keep") {
                        let _ = self.inform_handle(Event::KeepTakeoff);
                        return Ok(json!({ "kept": self.collab.rules().map(|r| r.takeoffs.iter().map(|t| t.name.clone()).collect::<Vec<_>>()) }));
                    }
                    let schedule = self.takeoff_schedule(&self.collab.inform.draft.clone()).ok_or("open a model first")?;
                    let rows = schedule.rows.iter().take(200).map(|r| r.cells.iter().map(|c| c.clone().unwrap_or_default()).collect::<Vec<_>>()).collect::<Vec<_>>();
                    let answer = json!({ "name": schedule.name, "columns": schedule.columns, "rows": rows, "row_count": schedule.rows.len() });
                    let _ = self.inform_handle(Event::MakeTakeoff);
                    Ok(answer)
                }
                "list" => Ok(json!({ "kept": self.collab.rules().map(|r| r.takeoffs.iter().map(|t| json!({ "name": t.name, "group_by": t.group_by, "classes": t.take.classes })).collect::<Vec<_>>()) })),
                other => Err(format!("collab_takeoff: unknown action {other} (make, keep, list)")),
            },
        }
    }

    /// The tab of colours and sets: what the model is coloured by and its legend; the sets of the
    /// rule set, each to select, show alone or hide.
    pub(super) fn inform_colours<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab.inform;
        let p = self.theme.palette();
        let go = |e: Event| Message::Collab(super::collab::Event::Inform(e));
        let picked = state.keys.iter().find(|k| k.key == state.colour_by).cloned();
        let mut out = column![
            w::heading(look, l.text("Inkleuren op eigenschap", "Colour by property"), 13.0),
            row![
                container(w::select(look, &state.keys, picked, move |k: Key| go(Event::ColourBy(k)), D::Compact)).width(Length::Fill),
                w::button(look, l.text("Uit", "Off"), K::Quiet, D::Compact, None, (!state.colour_by.is_empty()).then(|| go(Event::ColourBy(Key { key: String::new(), label: String::new() })))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ]
        .spacing(4);
        if state.colour_by.is_empty() {
            out = out.push(w::secondary(look, l.text("Kies een eigenschap: elk element krijgt de kleur van zijn waarde.", "Choose a property: each element takes the colour of its value.")));
        }
        for (k, row) in state.legend.iter().enumerate().take(200) {
            let [r, g, b] = row.color;
            let swatch = container(Space::new().width(14).height(14)).style(move |_| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb8(r, g, b))),
                border: iced::Border { color: p.border, width: 1.0, radius: 2.0.into() },
                ..Default::default()
            });
            let line = row![swatch, w::body(look, row.value.clone()).width(Length::Fill), w::secondary(look, row.ids.len().to_string())].spacing(6).align_y(Alignment::Center);
            out = out.push(
                row![
                    named(button(line).width(Length::Fill).padding([3, 6]).style(move |_, s| p.row_button(s)).on_press(go(Event::LegendPick(k))), Node::new(Role::Row, format!("{}: {}", row.value, row.ids.len()))),
                    w::button(look, l.text("Als set", "As set"), K::Quiet, D::Compact, None, Some(go(Event::LegendSet(k)))),
                ]
                .spacing(2)
                .align_y(Alignment::Center),
            );
        }
        if state.legend.len() > 200 {
            out = out.push(w::secondary(look, format!("+ {}", state.legend.len() - 200)));
        }
        let selected = self.sel.items().iter().any(|i| matches!(i, Pickable::Element(_)));
        out = out.push(w::divider(look)).push(w::heading(look, l.text("Selectiesets", "Selection sets"), 13.0)).push(
            row![
                container(w::text_field(look, l.text("Naam van de set", "Name of the set"), &state.set_name, Some(move |t: String| go(Event::SetName(t))), D::Compact, None)).width(Length::Fill),
                w::button(look, l.text("Selectie opslaan", "Save selection"), K::Secondary, D::Compact, None, selected.then(|| go(Event::SaveSet))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
        let sets = self.collab.rules().map(|r| r.selection_sets.as_slice()).unwrap_or(&[]);
        if sets.is_empty() {
            out = out.push(w::secondary(look, l.text("Nog geen sets. Ze gaan mee met de regelset (Regels opslaan).", "No sets yet. They go with the rule set (Save rules).")));
        }
        for (k, set) in sets.iter().enumerate() {
            let what = if set.property.is_empty() { format!("{} {}", set.keys.len(), l.text("elementen", "elements")) } else { l.text("op eigenschap", "by property").to_string() };
            out = out.push(
                column![
                    row![w::body(look, set.name.clone()).width(Length::Fill), w::secondary(look, what)].spacing(6).align_y(Alignment::Center),
                    row![
                        w::button(look, l.text("Selecteren", "Select"), K::Secondary, D::Compact, None, Some(go(Event::ApplySet(k, 0)))),
                        w::button(look, l.text("Isoleren", "Isolate"), K::Quiet, D::Compact, None, Some(go(Event::ApplySet(k, 1)))),
                        w::button(look, l.text("Verbergen", "Hide"), K::Quiet, D::Compact, None, Some(go(Event::ApplySet(k, 2)))),
                        Space::new().width(Length::Fill),
                        w::button(look, l.text("Weg", "Remove"), K::Quiet, D::Compact, None, Some(go(Event::RemoveSet(k)))),
                    ]
                    .spacing(4)
                    .wrap(),
                ]
                .spacing(2),
            );
        }
        out.into()
    }

    /// The tab of takeoffs: which elements, grouped how, made into a table under the 3D view;
    /// the takeoffs the rule set keeps.
    pub(super) fn inform_takeoffs<'a>(&'a self, look: Look, l: Language, sides: &'a [Side]) -> Element<'a, Message> {
        let state = &self.collab.inform;
        let go = |e: Event| Message::Collab(super::collab::Event::Inform(e));
        let draft = &state.draft;
        let key_of = |key: &str| state.keys.iter().find(|k| k.key == key).cloned();
        let side = sides.iter().find(|s| s.selector == draft.take).cloned();
        let label = |s: &'static str| w::secondary(look, s).width(110);
        let mut out = column![
            w::heading(look, l.text("Uittrekstaat", "Takeoff"), 13.0),
            row![label(l.text("Naam", "Name")), container(w::text_field(look, l.text("Uittrekstaat", "Takeoff"), &draft.name, Some(move |t: String| go(Event::TakeoffName(t))), D::Compact, None)).width(Length::Fill)].spacing(4).align_y(Alignment::Center),
            row![
                label(l.text("Elementen", "Elements")),
                {
                    // what a rule set or the remote set, as it is when the list has no such choice
                    let known = side.is_some();
                    let mut choice = column![container(w::select(look, sides, side, move |s: Side| go(Event::TakeoffSide(s)), D::Compact)).width(Length::Fill)].spacing(1).width(Length::Fill);
                    if !known {
                        choice = choice.push(w::secondary(look, draft.take.summary(l.text("alles", "everything"))));
                    }
                    choice
                },
            ]
            .spacing(4)
            .align_y(Alignment::Start),
            row![
                label(l.text("Met eigenschap", "With property")),
                container(w::select(look, &state.keys, key_of(&draft.property), move |k: Key| go(Event::TakeoffFilter(k)), D::Compact)).width(Length::FillPortion(3)),
                container(w::text_field(look, l.text("waarde, * voor iets", "value, * for any"), &draft.value, Some(move |t: String| go(Event::TakeoffValue(t))), D::Compact, None)).width(Length::FillPortion(2)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ]
        .spacing(4);
        // grouping: as many as chosen, and one more to choose
        for k in 0..=draft.group_by.len().min(1) {
            let now = draft.group_by.get(k).and_then(|key| key_of(key)).or_else(|| key_of(""));
            let title = if k == 0 { l.text("Groeperen op", "Group by") } else { l.text("en dan op", "then by") };
            out = out.push(row![label(title), container(w::select(look, &state.keys, now, move |key: Key| go(Event::TakeoffGroup(k, key)), D::Compact)).width(Length::Fill)].spacing(4).align_y(Alignment::Center));
        }
        out = out.push(
            row![
                w::button(look, l.text("Staat maken", "Make takeoff"), K::Primary, D::Compact, None, self.model.is_some().then(|| go(Event::MakeTakeoff))),
                w::button(look, l.text("In regelset bewaren", "Keep in rule set"), K::Secondary, D::Compact, None, Some(go(Event::KeepTakeoff))),
            ]
            .spacing(6)
            .wrap(),
        );
        out = out.push(w::secondary(look, l.text(
            "Lengte is de langste zijde, oppervlak het grootste vlak naar één kant (zijkant van een wand, bovenkant van een vloer), volume alleen van gesloten elementen (* waar niet alle elementen gesloten zijn).",
            "Length is the longest side, area the largest face turned one way (a wall's side, a slab's top), volume only of closed elements (* where not all elements are closed).",
        )));
        let kept = self.collab.rules().map(|r| r.takeoffs.as_slice()).unwrap_or(&[]);
        if !kept.is_empty() {
            out = out.push(w::divider(look)).push(w::heading(look, l.text("In de regelset", "In the rule set"), 13.0));
        }
        for (k, t) in kept.iter().enumerate() {
            out = out.push(
                row![
                    w::body(look, t.name.clone()).width(Length::Fill),
                    w::button(look, l.text("Maken", "Make"), K::Secondary, D::Compact, None, self.model.is_some().then(|| go(Event::RunTakeoff(k)))),
                    w::button(look, l.text("Weg", "Remove"), K::Quiet, D::Compact, None, Some(go(Event::RemoveTakeoff(k)))),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
        }
        out.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_values_get_colours_apart() {
        let colours: Vec<[u8; 3]> = (0..12).map(colour).collect();
        for k in 1..colours.len() {
            let d: i32 = (0..3).map(|c| (colours[k][c] as i32 - colours[k - 1][c] as i32).abs()).sum();
            assert!(d > 60, "{k}: {:?} {:?}", colours[k - 1], colours[k]);
        }
    }

    #[test]
    fn a_wall_measures_its_length_its_side_and_its_volume() {
        // a wall 4 m long, 0.2 m thick, 3 m high, as twelve triangles facing out
        let (lo, hi) = ([0.0f32, 0.0, 0.0], [4.0f32, 0.2, 3.0]);
        let p = |i: usize| Vec3::new(if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] });
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let triangles: Vec<[Vec3; 3]> = quads.iter().flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]]).collect();
        let (length, area, volume) = quantities(&triangles, true);
        assert!((length - 4.0).abs() < 1e-5 && (area - 12.0).abs() < 1e-4, "{length} {area}");
        assert!((volume.unwrap() - 2.4).abs() < 1e-4);
        assert_eq!(quantities(&triangles, false).2, None);
        // a thin wall's side cut in many small triangles (around its openings): its side still
        // counts, not its top, which has the largest triangles
        let mut cut: Vec<[Vec3; 3]> = vec![[Vec3::new(0.0, 0.0, 3.0), Vec3::new(4.0, 0.0, 3.0), Vec3::new(4.0, 0.07, 3.0)]];
        for k in 0..100 {
            let (x0, x1) = (k as f32 * 0.04, (k + 1) as f32 * 0.04);
            cut.push([Vec3::new(x0, 0.0, 0.0), Vec3::new(x1, 0.0, 0.0), Vec3::new(x1, 0.0, 3.0)]);
            cut.push([Vec3::new(x0, 0.0, 0.0), Vec3::new(x1, 0.0, 3.0), Vec3::new(x0, 0.0, 3.0)]);
        }
        let (_, side, _) = quantities(&cut, false);
        assert!((side - 12.0).abs() < 1e-3, "{side}");
    }

    #[test]
    fn a_value_reads_as_text_whatever_it_is() {
        let props = r#"{"IfcType":"IfcWall","Name":"W \"1\"","LoadBearing":true,"Width":0.2}"#;
        assert_eq!(prop_value(props, "Name").as_deref(), Some("W \"1\""));
        assert_eq!(prop_value(props, "LoadBearing").as_deref(), Some("true"));
        assert_eq!(prop_value(props, "Width").as_deref(), Some("0.2"));
        assert_eq!(prop_value(props, "FireRating"), None);
    }
}
