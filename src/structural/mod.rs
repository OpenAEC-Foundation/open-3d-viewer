//! Structural analysis models in the viewer: the documents of `bim-structural` stored on the
//! [`Model`](crate::model::Model) as [`StructuralPart`]s, and the [`StructuralIndex`] that makes
//! their objects elements (element id, properties, bounds, the unit they move and hide with).
//!
//! The document is never touched on the frame path. Properties are built when asked for; nothing
//! is written into `Model::props` per object at load. See
//! `docs/superpowers/specs/2026-11-06-structural-model-design.md`.

pub mod remote;

use crate::model::{Bounds, Origin};
use bim_structural::{
    frame, hinge, support, AnalysisModel, Curve, Dof, DofFrame, Hinge, Id, ItemKind, ItemRef, Load, LoadRef, ModelKind,
    NodeOrigin, Orientation, OffsetFrame, ResultFor, Shape, SupportCarrier, SupportPreset, Target, Thickness,
};
use glam::DVec3;
use std::collections::HashMap;
use std::sync::Arc;

/// First element id handed out when a structural id is already taken by an earlier part of the
/// same model (kind byte 0xFF: no `sam_id` kind, far above IFC express ids).
const FRESH_IDS: u64 = 0xFF << 56;

/// A structural object's key as the selection will know it: the source model (index into
/// `Model::origins`) and the id in its document. This is the seam to PR #91's
/// `ElementKey { model, source }`: once that lands, a structural object's key is
/// `ElementKey { model: key.model, source: key.source }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StructuralKey {
    pub model: u32,
    pub source: Id,
}

/// One analysis model of a loaded file, as the viewer keeps it.
#[derive(Debug, Clone)]
pub struct StructuralPart {
    /// The source model: index into `Model::origins`.
    pub model: u32,
    /// The document, f64 world metres of the source file (no anchor subtracted).
    pub doc: Arc<AnalysisModel>,
    /// Element id per source id where they differ (combined models, ids taken by an earlier
    /// part); every other object's element id is its source id.
    ids: Option<Arc<HashMap<Id, u64>>>,
}

impl StructuralPart {
    pub fn new(model: u32, doc: Arc<AnalysisModel>) -> Self {
        Self { model, doc, ids: None }
    }

    /// The element id of a document id.
    pub fn element_id(&self, source: Id) -> u64 {
        self.ids.as_ref().and_then(|m| m.get(&source).copied()).unwrap_or(source)
    }

    /// The selection key of a document id.
    pub fn key(&self, source: Id) -> StructuralKey {
        StructuralKey { model: self.model, source }
    }

    /// This part in another model: source model `model`, and element ids passed through `remap`
    /// (for [`crate::model::Model::combine`]). The document is shared, not copied.
    pub fn renumbered(&self, model: u32, remap: impl Fn(u64) -> u64) -> Self {
        let ids = items(&self.doc).map(|(id, _)| (id, remap(self.element_id(id)))).collect();
        Self { model, doc: self.doc.clone(), ids: Some(Arc::new(ids)) }
    }
}

/// Every addressable object of a document: id and slot.
pub fn items(doc: &AnalysisModel) -> impl Iterator<Item = (Id, ItemRef)> + '_ {
    let at = |kind: ItemKind| move |(slot, id): (usize, Id)| (id, ItemRef { kind, slot: slot as u32 });
    (doc.nodes.iter().map(|x| x.id).enumerate().map(at(ItemKind::Node)))
        .chain(doc.members.iter().map(|x| x.id).enumerate().map(at(ItemKind::Member)))
        .chain(doc.surfaces.iter().map(|x| x.id).enumerate().map(at(ItemKind::Surface)))
        .chain(doc.line_supports.iter().map(|x| x.id).enumerate().map(at(ItemKind::LineSupport)))
        .chain(doc.sections.iter().map(|x| x.id).enumerate().map(at(ItemKind::Section)))
        .chain(doc.materials.iter().map(|x| x.id).enumerate().map(at(ItemKind::Material)))
        .chain(doc.load_cases.iter().map(|x| x.id).enumerate().map(at(ItemKind::LoadCase)))
        .chain(doc.combinations.iter().map(|x| x.id).enumerate().map(at(ItemKind::Combination)))
        .chain(doc.actions.iter().map(|x| x.id).enumerate().map(at(ItemKind::Action)))
        .chain(doc.results.iter().map(|x| x.id).enumerate().map(at(ItemKind::ResultSet)))
}

/// Points of member axes besides their nodes (arc mid points, polyline vertices), world metres.
pub fn curve_points(doc: &AnalysisModel) -> impl Iterator<Item = DVec3> + '_ {
    doc.members.iter().flat_map(|m| match &m.geometry {
        Curve::Line => Vec::new(),
        Curve::Arc { mid } => vec![*mid],
        Curve::Polyline(points) => points.clone(),
    })
}

/// The lower-case name of an item kind, as MCP tools take and give it.
pub fn kind_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Node => "node",
        ItemKind::Member => "member",
        ItemKind::Surface => "surface",
        ItemKind::LineSupport => "line_support",
        ItemKind::Section => "section",
        ItemKind::Material => "material",
        ItemKind::LoadCase => "load_case",
        ItemKind::Combination => "combination",
        ItemKind::Action => "action",
        ItemKind::ResultSet => "result_set",
    }
}

/// The item kind of a name from [`kind_name`], also plural and with spaces or hyphens
/// (`"line supports"`).
pub fn kind_from_name(name: &str) -> Option<ItemKind> {
    const ALL: [ItemKind; 10] = [
        ItemKind::Node,
        ItemKind::Member,
        ItemKind::Surface,
        ItemKind::LineSupport,
        ItemKind::Section,
        ItemKind::Material,
        ItemKind::LoadCase,
        ItemKind::Combination,
        ItemKind::Action,
        ItemKind::ResultSet,
    ];
    let name = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    let find = |n: &str| ALL.into_iter().find(|k| kind_name(*k) == n);
    find(&name).or_else(|| find(name.strip_suffix('s')?))
}

/// The class shown in the properties ("Class").
fn class_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Node => "Structural node",
        ItemKind::Member => "Structural member",
        ItemKind::Surface => "Structural surface",
        ItemKind::LineSupport => "Line support",
        ItemKind::Section => "Cross-section",
        ItemKind::Material => "Material",
        ItemKind::LoadCase => "Load case",
        ItemKind::Combination => "Load combination",
        ItemKind::Action => "Load",
        ItemKind::ResultSet => "Result set",
    }
}

/// True for the kinds that are units of their own (selected, hidden and moved as one): nodes,
/// members and surface members. Everything else follows the item it decorates.
pub fn has_unit(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Node | ItemKind::Member | ItemKind::Surface)
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    part: u32,
    source: Id,
    item: ItemRef,
}

/// Element id → structural object, over every part of a model.
#[derive(Debug, Default)]
pub struct StructuralIndex {
    by_element: HashMap<u64, Entry>,
}

impl StructuralIndex {
    /// Indexes `parts`. An object whose element id an earlier part already uses gets a fresh one
    /// (two analysis models of one file may number their nodes alike).
    pub fn build(mut parts: Vec<StructuralPart>) -> (Vec<StructuralPart>, Self) {
        let mut by_element: HashMap<u64, Entry> = HashMap::new();
        let mut fresh = FRESH_IDS;
        for (p, part) in parts.iter_mut().enumerate() {
            let mut moved: HashMap<Id, u64> = HashMap::new();
            for (source, item) in items(&part.doc) {
                let mut element = part.element_id(source);
                if by_element.contains_key(&element) {
                    while by_element.contains_key(&fresh) {
                        fresh += 1;
                    }
                    element = fresh;
                    moved.insert(source, element);
                }
                by_element.insert(element, Entry { part: p as u32, source, item });
            }
            if !moved.is_empty() {
                let mut ids = part.ids.as_deref().cloned().unwrap_or_default();
                ids.extend(moved);
                part.ids = Some(Arc::new(ids));
            }
        }
        (parts, Self { by_element })
    }

    /// Number of indexed objects.
    pub fn len(&self) -> usize {
        self.by_element.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_element.is_empty()
    }

    /// Every element id, in no particular order.
    pub fn element_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.by_element.keys().copied()
    }

    /// The object with element id `id`.
    pub fn get<'a>(&self, parts: &'a [StructuralPart], id: u64) -> Option<StructuralItem<'a>> {
        let e = self.by_element.get(&id)?;
        let part = parts.get(e.part as usize)?;
        Some(StructuralItem { part, part_index: e.part, element: id, source: e.source, item: e.item })
    }

    /// Every object, ordered by part and then kind and slot (document order).
    pub fn all<'a>(&self, parts: &'a [StructuralPart]) -> Vec<StructuralItem<'a>> {
        let mut out: Vec<StructuralItem<'a>> = Vec::with_capacity(self.len());
        for (p, part) in parts.iter().enumerate() {
            out.extend(items(&part.doc).map(|(source, item)| StructuralItem {
                part,
                part_index: p as u32,
                element: part.element_id(source),
                source,
                item,
            }));
        }
        out
    }

    /// The units (nodes, members, surfaces) with their bounds at rest (model space), for the
    /// exploded view and box selection.
    pub fn unit_bounds(&self, parts: &[StructuralPart], origins: &[Origin]) -> Vec<(u64, Bounds)> {
        self.all(parts)
            .into_iter()
            .filter(|i| has_unit(i.item.kind))
            .filter_map(|i| Some((i.element, i.bounds(origins)?)))
            .collect()
    }

    /// The element ids of the units (nodes, members, surfaces) whose bounds at rest pass `test`.
    ///
    /// This is the seam for box selection: PR #91's `units_in_box` gathers element units from
    /// `ExplodeIndex`; the structural units it finds there are these.
    pub fn ids_in_box(&self, parts: &[StructuralPart], origins: &[Origin], test: impl Fn(&Bounds) -> bool) -> Vec<u64> {
        self.unit_bounds(parts, origins).into_iter().filter(|(_, b)| test(b)).map(|(id, _)| id).collect()
    }
}

/// One structural object found through the [`StructuralIndex`].
#[derive(Debug, Clone, Copy)]
pub struct StructuralItem<'a> {
    pub part: &'a StructuralPart,
    /// Index of the part in `Model::structural`.
    pub part_index: u32,
    /// Element id in the model.
    pub element: u64,
    /// Id in the document.
    pub source: Id,
    pub item: ItemRef,
}

impl<'a> StructuralItem<'a> {
    pub fn kind(&self) -> ItemKind {
        self.item.kind
    }

    pub fn doc(&self) -> &'a AnalysisModel {
        &self.part.doc
    }

    fn slot(&self) -> usize {
        self.item.slot as usize
    }

    /// The selection key (see [`StructuralKey`]).
    pub fn key(&self) -> StructuralKey {
        self.part.key(self.source)
    }

    /// The source numbering or name, as text.
    pub fn number(&self) -> String {
        let d = self.doc();
        let i = self.slot();
        match self.kind() {
            ItemKind::Node => d.nodes[i].number.clone(),
            ItemKind::Member => d.members[i].number.clone(),
            ItemKind::Surface => d.surfaces[i].number.clone(),
            ItemKind::LineSupport => d.line_supports[i].number.clone(),
            ItemKind::Section => d.sections[i].name.clone(),
            ItemKind::Material => d.materials[i].name.clone(),
            ItemKind::LoadCase => d.load_cases[i].number.clone(),
            ItemKind::Combination => d.combinations[i].number.clone(),
            ItemKind::Action => bim_structural::id::number_of(self.source).to_string(),
            ItemKind::ResultSet => d.results[i].number.clone(),
        }
    }

    /// The element id of the unit this object moves, hides and highlights with: itself for a
    /// node, member or surface; the item a load or line support sits on; `None` for objects
    /// without a place (sections, materials, cases, combinations, result sets, free loads).
    pub fn unit_key(&self) -> Option<u64> {
        let d = self.doc();
        let target = match self.kind() {
            k if has_unit(k) => return Some(self.element),
            ItemKind::Action => match &d.actions[self.slot()].target {
                Target::Node(id) | Target::Surface(id) | Target::Member { id, .. } => *id,
                Target::Free(_) => return None,
            },
            ItemKind::LineSupport => match &d.line_supports[self.slot()].along {
                SupportCarrier::Member(id) => *id,
                SupportCarrier::SurfaceEdge { surface, .. } => *surface,
                SupportCarrier::Polyline(_) => return None,
            },
            _ => return None,
        };
        d.item(target).is_some().then(|| self.part.element_id(target))
    }

    /// World points (f64, metres) that span the object, for its bounds.
    pub fn world_points(&self) -> Vec<DVec3> {
        let d = self.doc();
        let node = |id: &Id| d.node(*id).map(|n| n.p);
        match self.kind() {
            ItemKind::Node => vec![d.nodes[self.slot()].p],
            ItemKind::Member => {
                let m = &d.members[self.slot()];
                let mut out: Vec<DVec3> = m.nodes.iter().filter_map(node).collect();
                match &m.geometry {
                    Curve::Line => {}
                    Curve::Arc { mid } => {
                        out.push(*mid);
                        if let (Some(&a), Some(&b)) = (out.first(), out.get(1)) {
                            out.extend(arc_samples(a, *mid, b));
                        }
                    }
                    Curve::Polyline(points) => out.extend(points.iter().copied()),
                }
                out
            }
            ItemKind::Surface => {
                let s = &d.surfaces[self.slot()];
                s.boundary.iter().filter_map(node).collect()
            }
            ItemKind::LineSupport => match &d.line_supports[self.slot()].along {
                SupportCarrier::Member(id) => self.sibling(*id).map(|i| i.world_points()).unwrap_or_default(),
                SupportCarrier::SurfaceEdge { from, to, .. } => [from, to].into_iter().filter_map(node).collect(),
                SupportCarrier::Polyline(points) => points.clone(),
            },
            ItemKind::Action => match &d.actions[self.slot()].target {
                Target::Node(id) | Target::Surface(id) | Target::Member { id, .. } => {
                    self.sibling(*id).map(|i| i.world_points()).unwrap_or_default()
                }
                Target::Free(points) => points.clone(),
            },
            _ => Vec::new(),
        }
    }

    /// Another object of the same document.
    fn sibling(&self, source: Id) -> Option<StructuralItem<'a>> {
        let item = self.doc().item(source)?;
        Some(StructuralItem { element: self.part.element_id(source), source, item, ..*self })
    }

    /// The bounds at rest (model space): a node's point, a member's or surface's box, the box of
    /// the item a load or line support sits on. `None` for objects without a place.
    pub fn bounds(&self, origins: &[Origin]) -> Option<Bounds> {
        let origin = origins.get(self.part.model as usize).copied().unwrap_or_default();
        let mut b = Bounds::empty();
        for p in self.world_points() {
            let q = origin.display(p).as_vec3();
            if q.is_finite() {
                b.extend(q.to_array());
            }
        }
        b.is_valid().then_some(b)
    }

    /// The properties as (key, value) pairs, built from the document.
    pub fn props(&self) -> Vec<(String, String)> {
        let d = self.doc();
        let i = self.slot();
        let mut out: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, v: String| {
            if !v.is_empty() {
                out.push((k.to_string(), v));
            }
        };
        let node_no = |id: &Id| d.node(*id).map_or_else(|| id.to_string(), |n| n.number.clone());
        put("Class", class_name(self.kind()).into());
        match self.kind() {
            ItemKind::Node => {
                let n = &d.nodes[i];
                put("Number", n.number.clone());
                put("X", metres(n.p.x));
                put("Y", metres(n.p.y));
                put("Z", metres(n.p.z));
                if let Some(s) = &n.support {
                    put("Support", preset_text(s.preset));
                    put("Support DOFs", support::code(&s.dofs));
                    put("Support name", s.named.clone().unwrap_or_default());
                    put("Support frame", dof_frame_text(d, s.frame));
                    put("Springs", springs_text(&s.dofs));
                }
                if let Some(f) = n.frame {
                    put("Frame", format!("x {}, y {}, z {}", vector(f.x_axis), vector(f.y_axis), vector(f.z_axis)));
                }
                if let NodeOrigin::DerivedFromPhysical { element, approximate } = n.origin {
                    put("Derived from element", format!("{element}{}", if approximate { " (approximate)" } else { "" }));
                }
            }
            ItemKind::Member => {
                let m = &d.members[i];
                put("Number", m.number.clone());
                put("Kind", format!("{:?}", m.kind));
                put("Start node", m.nodes.first().map(node_no).unwrap_or_default());
                put("End node", m.nodes.last().map(node_no).unwrap_or_default());
                if m.nodes.len() > 2 {
                    put("Intermediate nodes", m.nodes[1..m.nodes.len() - 1].iter().map(node_no).collect::<Vec<_>>().join(", "));
                }
                if let Some((a, b)) = d.member_ends(m) {
                    put("Length", format!("{:.3} m", frame::curve_length(a, b, &m.geometry)));
                }
                put("Geometry", match &m.geometry {
                    Curve::Line => "line".into(),
                    Curve::Arc { mid } => format!("arc through {}", vector(*mid)),
                    Curve::Polyline(p) => format!("polyline, {} inner points", p.len()),
                });
                if let Some((start, end)) = m.section {
                    let name = |id: Id| d.section(id).map_or_else(|| id.to_string(), |s| s.name.clone());
                    put("Section", match end {
                        Some(end) if end != start => format!("{} → {}", name(start), name(end)),
                        _ => name(start),
                    });
                }
                if m.section_rotation != 0.0 {
                    put("Section rotation", format!("{:.3}°", m.section_rotation.to_degrees()));
                }
                put("Material", m.material.and_then(|id| d.material(id)).map(|x| x.name.clone()).unwrap_or_default());
                put("Local z", vector(m.z_ref));
                put("Orientation", match &m.orientation {
                    Orientation::IfcAxis => "IFC Axis".into(),
                    Orientation::Beta { angle, rule } => format!("beta {:.3}° ({rule})", angle.to_degrees()),
                    Orientation::RefNode(id) => format!("towards node {}", node_no(id)),
                    Orientation::RefVector(v) => format!("towards {}", vector(*v)),
                    Orientation::Default => "default".into(),
                });
                put("Releases", format!("start {}, end {}", release_text(&m.releases[0]), release_text(&m.releases[1])));
                let ins = &m.insertion;
                put("Cardinal point", if ins.cardinal_point == 0 { "none".into() } else { ins.cardinal_point.to_string() });
                if ins.eccentricity != glam::DVec2::ZERO {
                    put("Eccentricity", format!("ey {}, ez {} (member axes)", metres(ins.eccentricity.x), metres(ins.eccentricity.y)));
                }
                if ins.end_offsets != [DVec3::ZERO; 2] {
                    let frame = match ins.end_offset_frame {
                        OffsetFrame::MemberLocal => "member axes",
                        OffsetFrame::Global => "global",
                    };
                    put("End offsets", format!("start {}, end {} ({frame})", vector(ins.end_offsets[0]), vector(ins.end_offsets[1])));
                }
                if ins.profile_position != glam::DAffine2::IDENTITY {
                    put("Profile position", format!("{}", ins.profile_position.translation));
                }
                if let Some([a, b]) = ins.profile_offsets {
                    put("Profile offsets", format!("start {a}, end {b}"));
                }
                put("Eccentricity policy", format!("{:?}", d.source.eccentricity_policy));
                put("Linked element", ids_text(&m.links));
            }
            ItemKind::Surface => {
                let s = &d.surfaces[i];
                put("Number", s.number.clone());
                put("Kind", format!("{:?}", s.kind));
                put("Boundary nodes", s.boundary.iter().map(node_no).collect::<Vec<_>>().join(", "));
                if !s.openings.is_empty() {
                    put("Openings", s.openings.len().to_string());
                }
                put("Thickness", match &s.thickness {
                    Thickness::Uniform(t) => format!("{:.4} m", t),
                    Thickness::Linear(_) => "linear".into(),
                    Thickness::Unknown => "unknown".into(),
                });
                if s.offset_z != 0.0 {
                    put("Offset z", metres(s.offset_z));
                }
                put("Material", s.material.and_then(|id| d.material(id)).map(|x| x.name.clone()).unwrap_or_default());
                put("Linked element", ids_text(&s.links));
            }
            ItemKind::LineSupport => {
                let l = &d.line_supports[i];
                put("Number", l.number.clone());
                put("Along", match &l.along {
                    SupportCarrier::Member(id) => format!("member {}", d.member(*id).map_or_else(|| id.to_string(), |m| m.number.clone())),
                    SupportCarrier::SurfaceEdge { surface, from, to } => format!(
                        "edge {}-{} of surface {}",
                        node_no(from),
                        node_no(to),
                        d.surface(*surface).map_or_else(|| surface.to_string(), |s| s.number.clone())
                    ),
                    SupportCarrier::Polyline(p) => format!("polyline of {} points", p.len()),
                });
                put("Support DOFs", support::code(&l.dofs));
                put("Support name", l.named.clone().unwrap_or_default());
                put("Support frame", dof_frame_text(d, l.frame));
                put("Springs", springs_text(&l.dofs));
            }
            ItemKind::Section => {
                let s = &d.sections[i];
                put("Name", s.name.clone());
                put("Shape", shape_name(&s.shape).into());
                put("Area", format!("{:.4e} m²", s.props.area));
                put("Iy", format!("{:.4e} m⁴", s.props.iy));
                put("Iz", format!("{:.4e} m⁴", s.props.iz));
                if let Some(it) = s.imported.as_ref().and_then(|p| p.it).or(s.props.it) {
                    put("It", format!("{it:.4e} m⁴"));
                }
                put("Properties", format!("{:?}", s.props_source));
                put("Material", s.material.and_then(|id| d.material(id)).map(|x| x.name.clone()).unwrap_or_default());
            }
            ItemKind::Material => {
                let m = &d.materials[i];
                put("Name", m.name.clone());
                put("E", m.e.map(|v| format!("{v:.4e} Pa")).unwrap_or_default());
                put("G", m.g.map(|v| format!("{v:.4e} Pa")).unwrap_or_default());
                put("nu", m.nu.map(|v| format!("{v}")).unwrap_or_default());
                put("Density", m.density.map(|v| format!("{v} kg/m³")).unwrap_or_default());
                put("Thermal expansion", m.alpha_t.map(|v| format!("{v:e} 1/K")).unwrap_or_default());
            }
            ItemKind::LoadCase => {
                let c = &d.load_cases[i];
                put("Number", c.number.clone());
                put("Name", c.name.clone());
                put("Action type", format!("{:?}", c.action_type));
                put("Action source", c.action_source.clone());
                put("Coefficient", format!("{}", c.coefficient));
                put("Self weight", c.self_weight.map(vector).unwrap_or_default());
            }
            ItemKind::Combination => {
                let c = &d.combinations[i];
                put("Number", c.number.clone());
                put("Name", c.name.clone());
                put("Purpose", format!("{:?}", c.purpose));
                put("Terms", c.terms.iter().map(|(r, f)| format!("{f} × {}", load_ref_text(d, *r))).collect::<Vec<_>>().join(" + "));
            }
            ItemKind::Action => {
                let a = &d.actions[i];
                put("Load case", load_ref_text(d, a.load));
                put("Target", match &a.target {
                    Target::Node(id) => format!("node {}", node_no(id)),
                    Target::Member { id, span } => format!(
                        "member {}{}",
                        d.member(*id).map_or_else(|| id.to_string(), |m| m.number.clone()),
                        span.map(|(a, b)| format!(" from {a:.3} m to {b:.3} m")).unwrap_or_default()
                    ),
                    Target::Surface(id) => format!("surface {}", d.surface(*id).map_or_else(|| id.to_string(), |s| s.number.clone())),
                    Target::Free(p) => format!("free, {} points", p.len()),
                });
                put("Frame", format!("{:?}", a.frame));
                put("Load", load_text(&a.kind));
                if a.destabilising {
                    put("Destabilising", "yes".into());
                }
            }
            ItemKind::ResultSet => {
                let r = &d.results[i];
                put("Number", r.number.clone());
                put("Name", r.name.clone());
                put("For", match &r.for_ {
                    ResultFor::Case(id) => load_ref_text(d, LoadRef::Case(*id)),
                    ResultFor::Combination(id) => load_ref_text(d, LoadRef::Combination(*id)),
                    ResultFor::Envelope(s) => format!("envelope {s}"),
                });
                put("Theory", format!("{:?}", r.theory));
                put("Linear", if r.linear { "yes" } else { "no" }.into());
                put("Tables", r.available.iter().map(|t| t.name()).collect::<Vec<_>>().join(", "));
                put("Result signs", format!("{:?}", d.source.conventions.result_signs));
            }
        }
        put("Analysis model", d.name.clone());
        put("Model kind", match d.kind {
            ModelKind::Analytical => "analytical".into(),
            ModelKind::PhysicalAxes => "physical axes".into(),
        });
        put("Source", d.source.format.clone());
        put("Source id", self.source.to_string());
        let notes: Vec<String> =
            d.diagnostics.iter().filter(|x| x.object == Some(self.source)).map(|x| format!("{}: {}", x.code, x.message)).collect();
        put("Notes", notes.join("; "));
        out
    }
}

/// Points along the arc from `a` through `mid` to `b`, for its bounds.
fn arc_samples(a: DVec3, mid: DVec3, b: DVec3) -> Vec<DVec3> {
    let Some((c, n, _)) = frame::circle_through(a, mid, b) else { return Vec::new() };
    let (u, w) = (a - c, b - c);
    let angle = |v: DVec3| {
        let x = u.normalize();
        let y = n.cross(x);
        v.dot(y).atan2(v.dot(x)).rem_euclid(std::f64::consts::TAU)
    };
    let (sweep, through) = (angle(w), angle(mid - c));
    // the arc runs from a to b the way that passes mid
    let sweep = if through <= sweep { sweep } else { sweep - std::f64::consts::TAU };
    (1..16).map(|k| c + glam::DQuat::from_axis_angle(n, sweep * k as f64 / 16.0) * u).collect()
}

/// Metres with up to six decimals.
fn metres(v: f64) -> String {
    format!("{} m", trimmed(v))
}

fn trimmed(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

fn vector(v: DVec3) -> String {
    format!("({}, {}, {})", trimmed(v.x), trimmed(v.y), trimmed(v.z))
}

fn ids_text(ids: &[Id]) -> String {
    ids.iter().map(Id::to_string).collect::<Vec<_>>().join(", ")
}

fn preset_text(preset: SupportPreset) -> String {
    match support::preset_name(preset) {
        Some(name) => name.into(),
        None => match preset {
            SupportPreset::Roller { .. } => "roller".into(),
            SupportPreset::Sliding { .. } => "sliding".into(),
            SupportPreset::Spring => "spring".into(),
            SupportPreset::Free => "free".into(),
            _ => "custom".into(),
        },
    }
}

/// The support preset family name, for filtering: fixed, pinned, roller, sliding, spring, free,
/// custom.
pub fn preset_family(preset: SupportPreset) -> &'static str {
    match preset {
        SupportPreset::Fixed => "fixed",
        SupportPreset::Pinned => "pinned",
        SupportPreset::Roller { .. } => "roller",
        SupportPreset::Sliding { .. } => "sliding",
        SupportPreset::Spring => "spring",
        SupportPreset::Free => "free",
        SupportPreset::Custom => "custom",
    }
}

fn dof_frame_text(d: &AnalysisModel, frame: DofFrame) -> String {
    match frame {
        DofFrame::Global => "global".into(),
        DofFrame::NodeLocal => "node".into(),
        DofFrame::MemberLocal(id) => format!("member {}", d.member(id).map_or_else(|| id.to_string(), |m| m.number.clone())),
    }
}

fn springs_text(dofs: &bim_structural::Dofs) -> String {
    const NAMES: [&str; 6] = ["ux", "uy", "uz", "rx", "ry", "rz"];
    dofs.iter()
        .zip(NAMES)
        .filter_map(|(d, n)| match d {
            Dof::Spring(k) => Some(format!("{n} {k:e}")),
            Dof::Nonlinear { kind, base } => Some(format!("{n} {kind:?} {base:e}")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A member end: `rigid`, or the DOF code with the preset name when it has one (`111100 hinge`).
fn release_text(h: &Option<Hinge>) -> String {
    match h {
        None => "rigid".into(),
        Some(h) => {
            let code = support::code(&h.dofs);
            let name = h.named.clone().or_else(|| hinge::preset_name(hinge::classify(&h.dofs)).map(str::to_string));
            match name {
                Some(name) => format!("{code} {name}"),
                None => code,
            }
        }
    }
}

fn load_ref_text(d: &AnalysisModel, r: LoadRef) -> String {
    match r {
        LoadRef::Case(id) => d.load_case(id).map_or_else(|| id.to_string(), |c| format!("{} {}", c.number, c.name).trim().to_string()),
        LoadRef::Combination(id) => d
            .combinations
            .iter()
            .find(|c| c.id == id)
            .map_or_else(|| id.to_string(), |c| format!("{} {}", c.number, c.name).trim().to_string()),
    }
}

fn load_text(load: &Load) -> String {
    match load {
        Load::NodalForce { f, m } => format!("force {} N, moment {} N·m", vector(*f), vector(*m)),
        Load::MemberPoint { x, f, m } => format!("at {x:.3} m: force {} N, moment {} N·m", vector(*f), vector(*m)),
        Load::MemberDistributed { dir, moment, dist } => {
            format!("distributed {} along {}: {dist:?}", if *moment { "moment" } else { "force" }, vector(*dir))
        }
        Load::Surface { dir, dist } => format!("surface load along {}: {dist:?}", vector(*dir)),
        Load::Temperature { dt, dt_y, dt_z } => format!("temperature ΔT {dt} K, ΔTy {dt_y} K, ΔTz {dt_z} K"),
        Load::Displacement { u } => format!("displacement {u:?}"),
    }
}

fn shape_name(shape: &Shape) -> &'static str {
    match shape {
        Shape::I { .. } => "I",
        Shape::AsymI { .. } => "asymmetric I",
        Shape::U { .. } => "U",
        Shape::L { .. } => "L",
        Shape::T { .. } => "T",
        Shape::C { .. } => "C",
        Shape::Z { .. } => "Z",
        Shape::Rect { .. } => "rectangle",
        Shape::Circle { .. } => "circle",
        Shape::Chs { .. } => "circular hollow",
        Shape::Rhs { .. } => "rectangular hollow",
        Shape::Polygon { .. } => "polygon",
        Shape::Composite(_) => "composite",
        Shape::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::*;
    use bim_structural::{sam_id, IdKind, Node, SourceInfo};

    pub(crate) fn nodes(name: &str, points: &[[f64; 3]]) -> AnalysisModel {
        let mut m = AnalysisModel::new(1, name, SourceInfo::default());
        for (k, p) in points.iter().enumerate() {
            m.nodes.push(Node {
                id: sam_id(IdKind::Node, k as u64 + 1),
                number: (k + 1).to_string(),
                p: DVec3::from_array(*p),
                frame: None,
                support: None,
                origin: NodeOrigin::Analytical,
            });
        }
        m.rebuild_index();
        m
    }

    #[test]
    fn colliding_parts_get_fresh_ids() {
        let a = Arc::new(nodes("A", &[[0.0; 3], [1.0, 0.0, 0.0]]));
        let b = Arc::new(nodes("B", &[[0.0, 0.0, 5.0]]));
        let (parts, index) = StructuralIndex::build(vec![StructuralPart::new(0, a), StructuralPart::new(0, b)]);
        assert_eq!(index.len(), 3);
        let n1 = sam_id(IdKind::Node, 1);
        assert_eq!(parts[0].element_id(n1), n1);
        let moved = parts[1].element_id(n1);
        assert_ne!(moved, n1);
        let item = index.get(&parts, moved).unwrap();
        assert_eq!((item.part_index, item.source, item.kind()), (1, n1, ItemKind::Node));
        assert_eq!(item.key(), StructuralKey { model: 0, source: n1 });
        let origins = [Origin::default()];
        assert_eq!(item.bounds(&origins).unwrap().min, [0.0, 0.0, 5.0]);
    }

    #[test]
    fn kind_names_round_trip() {
        for k in ["node", "members", "line support", "load-case", "result_sets", "surface"] {
            assert!(kind_from_name(k).is_some(), "{k}");
        }
        assert_eq!(kind_from_name("nodes"), Some(ItemKind::Node));
        assert_eq!(kind_from_name("wall"), None);
    }

    #[test]
    fn arcs_bound_their_bulge() {
        // a half circle of radius 1 from (-1,0,0) through (0,1,0) to (1,0,0): it reaches y = 1
        let pts = arc_samples(DVec3::new(-1.0, 0.0, 0.0), DVec3::new(0.0, 1.0, 0.0), DVec3::new(1.0, 0.0, 0.0));
        assert!(pts.iter().all(|p| (p.length() - 1.0).abs() < 1e-12 && p.y >= -1e-12));
        assert!(pts.iter().any(|p| (p.y - 1.0).abs() < 1e-12));
    }
}
