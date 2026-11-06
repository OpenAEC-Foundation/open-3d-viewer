//! What the renderer draws of the structural parts: f32 instance arrays derived from the f64
//! documents once per model, on the load worker ([`crate::model::Model::set_structural`]), never
//! on the frame path.
//!
//! - **Segments** ([`SegInstance`], the `seg` pipeline): member axes, arcs sampled at
//!   [`ARC_SEGMENTS`] chords, polylines and members through intermediate nodes vertex by vertex.
//!   Later phases add rigid offsets, load and diagram outlines and the deformed shape to the same
//!   array.
//! - **Glyphs** ([`GlyphInstance`], the `glyph` pipeline): small 3D symbols drawn at a size in
//!   pixels with a world cap. Phase S3 has the node cube; phase S4 adds supports, hinges and
//!   arrows. Instances are sorted by [`GlyphKind`], one indexed draw per kind.
//!
//! Positions are display coordinates (the part's origin subtracted in f64, then cast). Every
//! instance belongs to a [`DisplayItem`] (a node or a member, by element id), whose unit of the
//! exploded view the renderer writes into the instances when it uploads them
//! ([`crate::explode::ExplodeIndex::display_units`]), so the exploded view and the section box
//! and plane apply to every instance.

use super::{arc_samples, StructuralPart};
use crate::model::Origin;
use bim_structural::{frame, Curve, ItemKind};
use bytemuck::{Pod, Zeroable};
use glam::DVec3;

/// Chords per circular arc.
pub const ARC_SEGMENTS: usize = 16;
/// A node glyph is never larger (in the world) than this share of its shortest member, so
/// symbols do not swallow short members when zoomed out.
pub const NODE_CAP: f64 = 0.4;

/// `SegInstance::style`: which scale multiplies the offsets (bits 0-1), a dashed line (bit 2,
/// for later phases) and the width as a multiple of the line width in sixteenths (bits 8-15; 0
/// reads as 16).
pub mod style {
    /// The offsets are taken as they are.
    pub const SCALE_NONE: u32 = 0;
    pub const SCALE_DIAGRAM: u32 = 1;
    pub const SCALE_LOAD: u32 = 2;
    pub const SCALE_DEFORMATION: u32 = 3;
    pub const DASHED: u32 = 1 << 2;
    pub const WIDTH_SHIFT: u32 = 8;
    /// A member axis: unscaled, solid, one line width.
    pub const AXIS: u32 = SCALE_NONE | (16 << WIDTH_SHIFT);
}

/// A wide segment: an instance of a four-vertex triangle strip that the vertex shader spreads to
/// a quad of the line width in pixels. Its ends are `a + a_off · scale` and `b + b_off · scale`
/// (the scale chosen by `style`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct SegInstance {
    pub a: [f32; 3],
    pub a_off: [f32; 3],
    pub b: [f32; 3],
    pub b_off: [f32; 3],
    /// sRGB; alpha 0 takes the theme's axis colour.
    pub color: [u8; 4],
    /// Unit of the exploded view; written at upload.
    pub unit: u32,
    pub style: u32,
}

/// A glyph: an instance of one mesh of the glyph library.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GlyphInstance {
    pub pos: [f32; 3],
    /// Unit quaternion (x, y, z, w) from the glyph's frame to model space.
    pub rot: [f32; 4],
    /// sRGB; alpha 0 takes the theme's colour for the kind.
    pub color: [u8; 4],
    /// Unit of the exploded view; written at upload.
    pub unit: u32,
    /// Size in logical pixels (0: the default glyph size) and the world cap (0: none).
    pub size: [f32; 2],
    /// Per kind. Node: the length of the shortest adjacent member (level of detail: the node is
    /// dropped when that projects to fewer than `lod.z` pixels; 0: never dropped).
    pub param: [f32; 2],
    /// The [`GlyphKind`] in bits 0-7; a level for arrows in bits 8-15 (later phases).
    pub kind_lod: u32,
}

/// The meshes of the glyph library, in the order their instances are drawn.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GlyphKind {
    /// A small cube.
    Node = 0,
}

impl GlyphKind {
    pub const ALL: [GlyphKind; 1] = [GlyphKind::Node];
}

/// A node or member and the instances that draw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayItem {
    pub element: u64,
    pub kind: ItemKind,
    /// The source model of its part (`StructuralPart::model`, an index into `Model::origins`).
    pub model: u32,
    /// First segment and count.
    pub segs: (u32, u32),
    /// First glyph and count.
    pub glyphs: (u32, u32),
}

/// Instance ranges to draw again, highlighted (the selection or the hover): (first, count),
/// sorted, adjacent ranges merged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    pub segs: Vec<(u32, u32)>,
    pub glyphs: Vec<(u32, u32)>,
}

impl Marks {
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty() && self.glyphs.is_empty()
    }
}

/// The instance arrays of every structural part of a model.
#[derive(Debug, Default)]
pub struct StructuralDisplay {
    pub segs: Vec<SegInstance>,
    /// The item of every segment.
    pub seg_item: Vec<u32>,
    /// Sorted by kind.
    pub glyphs: Vec<GlyphInstance>,
    /// The item of every glyph.
    pub glyph_item: Vec<u32>,
    /// Per kind present: (kind, first glyph, count), in [`GlyphKind`] order.
    pub glyph_kinds: Vec<(GlyphKind, u32, u32)>,
    pub items: Vec<DisplayItem>,
    /// (element id, item), sorted by element id.
    item_of: Vec<(u64, u32)>,
}

impl StructuralDisplay {
    /// Derives the instances of `parts` in display coordinates (`origins` indexed by the parts'
    /// `model`).
    pub fn build(parts: &[StructuralPart], origins: &[Origin]) -> Self {
        let mut d = Self::default();
        let mut glyphs: Vec<Vec<(GlyphInstance, u32)>> = vec![Vec::new(); GlyphKind::ALL.len()];
        for part in parts {
            let doc = &part.doc;
            let origin = origins.get(part.model as usize).copied().unwrap_or_default();
            let display = |p: DVec3| origin.display(p).as_vec3().to_array();
            // the ends of every member, and the shortest member at each node (by its slot), for
            // the world cap and the level of detail
            let slot = |id: u64| doc.item(id).filter(|r| r.kind == ItemKind::Node).map(|r| r.slot as usize);
            let mut shortest = vec![f64::INFINITY; doc.nodes.len()];
            let mut ends: Vec<Option<(DVec3, DVec3)>> = Vec::with_capacity(doc.members.len());
            for m in &doc.members {
                let slots = (m.nodes.first().and_then(|&n| slot(n)), m.nodes.last().and_then(|&n| slot(n)));
                let (Some(sa), Some(sb)) = slots else {
                    ends.push(None);
                    continue;
                };
                let (a, b) = (doc.nodes[sa].p, doc.nodes[sb].p);
                ends.push(Some((a, b)));
                let len = frame::curve_length(a, b, &m.geometry);
                if !(len > 0.0 && len.is_finite()) {
                    continue;
                }
                let mut note = |s: usize| shortest[s] = shortest[s].min(len);
                match m.nodes.len() {
                    2 => {
                        note(sa);
                        note(sb);
                    }
                    _ => m.nodes.iter().filter_map(|&n| slot(n)).for_each(note),
                }
            }
            for (k, n) in doc.nodes.iter().enumerate() {
                if !n.p.is_finite() {
                    continue;
                }
                let item = d.push_item(part.element_id(n.id), ItemKind::Node, part.model);
                let len = Some(shortest[k]).filter(|l| l.is_finite()).unwrap_or(0.0);
                let g = GlyphInstance {
                    pos: display(n.p),
                    rot: [0.0, 0.0, 0.0, 1.0],
                    color: [0; 4],
                    unit: 0,
                    size: [0.0, (NODE_CAP * len) as f32],
                    param: [len as f32, 0.0],
                    kind_lod: GlyphKind::Node as u32,
                };
                glyphs[GlyphKind::Node as usize].push((g, item));
            }
            let axis = |a: DVec3, b: DVec3| SegInstance {
                a: display(a),
                a_off: [0.0; 3],
                b: display(b),
                b_off: [0.0; 3],
                color: [0; 4],
                unit: 0,
                style: style::AXIS,
            };
            for (m, ends) in doc.members.iter().zip(ends) {
                let Some((a, b)) = ends else { continue };
                let first = d.segs.len() as u32;
                // a plain line between two nodes, the common case, without the points vector
                if m.nodes.len() == 2 && matches!(m.geometry, Curve::Line) {
                    if !(a.is_finite() && b.is_finite()) || a == b {
                        continue;
                    }
                    let item = d.push_item(part.element_id(m.id), ItemKind::Member, part.model);
                    d.segs.push(axis(a, b));
                    d.seg_item.push(item);
                    d.items[item as usize].segs = (first, 1);
                    continue;
                }
                let points = member_points(doc, m);
                if points.len() < 2 {
                    continue;
                }
                let item = d.push_item(part.element_id(m.id), ItemKind::Member, part.model);
                for w in points.windows(2) {
                    d.segs.push(axis(w[0], w[1]));
                    d.seg_item.push(item);
                }
                d.items[item as usize].segs = (first, d.segs.len() as u32 - first);
            }
        }
        d.item_of.sort_unstable();
        for (kind, list) in GlyphKind::ALL.into_iter().zip(glyphs) {
            if list.is_empty() {
                continue;
            }
            let first = d.glyphs.len() as u32;
            for (g, item) in list {
                let at = d.glyphs.len() as u32;
                let range = &mut d.items[item as usize].glyphs;
                *range = if range.1 == 0 { (at, 1) } else { (range.0, range.1 + 1) };
                d.glyphs.push(g);
                d.glyph_item.push(item);
            }
            d.glyph_kinds.push((kind, first, d.glyphs.len() as u32 - first));
        }
        d
    }

    fn push_item(&mut self, element: u64, kind: ItemKind, model: u32) -> u32 {
        let item = self.items.len() as u32;
        self.items.push(DisplayItem { element, kind, model, segs: (0, 0), glyphs: (0, 0) });
        self.item_of.push((element, item));
        item
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty() && self.glyphs.is_empty()
    }

    /// The item that draws element `element`.
    pub fn item(&self, element: u64) -> Option<u32> {
        let k = self.item_of.binary_search_by_key(&element, |e| e.0).ok()?;
        Some(self.item_of[k].1)
    }

    /// The glyph kind of glyph `g`.
    pub fn glyph_kind(&self, g: u32) -> Option<GlyphKind> {
        self.glyph_kinds.iter().find(|(_, first, count)| (*first..first + count).contains(&g)).map(|k| k.0)
    }

    /// The instance ranges of `items`, merged.
    pub fn marks(&self, items: impl IntoIterator<Item = u32>) -> Marks {
        let mut items: Vec<u32> = items.into_iter().filter(|&i| (i as usize) < self.items.len()).collect();
        items.sort_unstable();
        items.dedup();
        let merge = |ranges: &mut Vec<(u32, u32)>, (first, count): (u32, u32)| {
            if count == 0 {
                return;
            }
            match ranges.last_mut() {
                Some(last) if last.0 + last.1 == first => last.1 += count,
                _ => ranges.push((first, count)),
            }
        };
        let mut marks = Marks::default();
        for i in items {
            let item = &self.items[i as usize];
            merge(&mut marks.segs, item.segs);
            merge(&mut marks.glyphs, item.glyphs);
        }
        marks.segs.sort_unstable();
        marks.glyphs.sort_unstable();
        marks
    }
}

/// The axis of a member as points (f64 world): through its intermediate nodes for a straight
/// member, its own inner points for a polyline, [`ARC_SEGMENTS`] chords for an arc (a straight
/// line when the three points are collinear).
pub fn member_points(doc: &bim_structural::AnalysisModel, m: &bim_structural::Member) -> Vec<DVec3> {
    let Some((a, b)) = doc.member_ends(m) else { return Vec::new() };
    let mut out = vec![a];
    match &m.geometry {
        Curve::Line => {
            let inner = m.nodes.get(1..m.nodes.len().saturating_sub(1)).unwrap_or(&[]);
            out.extend(inner.iter().filter_map(|id| doc.node(*id)).map(|n| n.p));
        }
        Curve::Polyline(points) => out.extend(points.iter().copied()),
        Curve::Arc { mid } => out.extend(arc_samples(a, *mid, b)),
    }
    out.push(b);
    out.retain(|p| p.is_finite());
    out.dedup();
    out
}
