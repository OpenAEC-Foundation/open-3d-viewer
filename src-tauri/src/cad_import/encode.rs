//! Binary scene buffer shared by the DWG and E57 importers ("O3DS").
//!
//! The importers collect geometry in world coordinates (metres, Z up, f64) grouped by a
//! name (DWG layer, E57 scan). `Scene::encode` recentres the model when it lies far from
//! the origin (e.g. Dutch RD coordinates) so f32 keeps millimetre precision, and writes:
//!
//! ```text
//! "O3DS", u32 version (1)
//! 3 x f64 origin            -- subtracted from all positions (add it back for source coords)
//! u32 note length, UTF-8 notes padded to 4 bytes
//! u32 group count
//!   u32 name length, UTF-8 name padded to 4, u32 item count
//!     u32 kind (0 mesh, 1 line segments, 2 points), 3 x f32 colour (0..1),
//!     u32 vertex count, u32 extra (mesh: index count; points: 1 = per-point colours)
//!     positions f32 x 3n (local, Z up)
//!     mesh:   normals f32 x 3n, indices u32 x extra
//!     points: if extra == 1, colours u8 x 3n padded to 4
//! 6 x f32 bounds (local min xyz, max xyz)
//! ```

use std::collections::HashMap;

pub const KIND_MESH: u32 = 0;
pub const KIND_LINES: u32 = 1;
pub const KIND_POINTS: u32 = 2;

/// Distance from the world origin beyond which the model is recentred.
const RECENTRE_DISTANCE: f64 = 10_000.0;

pub struct Item {
    pub kind: u32,
    pub color: [f32; 3],
    /// World positions (x, y, z), metres.
    pub positions: Vec<f64>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    /// Per-point RGB (points only; empty = use `color`).
    pub colors: Vec<u8>,
}

impl Item {
    fn new(kind: u32, color: [f32; 3]) -> Self {
        Item { kind, color, positions: Vec::new(), normals: Vec::new(), indices: Vec::new(), colors: Vec::new() }
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }
}

pub struct Group {
    pub name: String,
    pub items: Vec<Item>,
    index: HashMap<(u32, [u8; 3]), usize>,
}

#[derive(Default)]
pub struct Scene {
    pub groups: Vec<Group>,
    index: HashMap<String, usize>,
    pub notes: Vec<String>,
}

fn color_key(c: [f32; 3]) -> [u8; 3] {
    [(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8]
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// The item of `kind` and `color` in group `group`, created on first use.
    pub fn item(&mut self, group: &str, kind: u32, color: [f32; 3]) -> &mut Item {
        let gi = match self.index.get(group) {
            Some(&i) => i,
            None => {
                self.groups.push(Group { name: group.to_string(), items: Vec::new(), index: HashMap::new() });
                self.index.insert(group.to_string(), self.groups.len() - 1);
                self.groups.len() - 1
            }
        };
        let g = &mut self.groups[gi];
        let key = (kind, color_key(color));
        let ii = match g.index.get(&key) {
            Some(&i) => i,
            None => {
                g.items.push(Item::new(kind, color));
                g.index.insert(key, g.items.len() - 1);
                g.items.len() - 1
            }
        };
        &mut g.items[ii]
    }

    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.items.iter().all(|i| i.positions.is_empty()))
    }

    /// World-space bounds of all geometry, or None when empty.
    pub fn world_bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for i in self.groups.iter().flat_map(|g| g.items.iter()) {
            for p in i.positions.chunks_exact(3) {
                for k in 0..3 {
                    if p[k].is_finite() {
                        min[k] = min[k].min(p[k]);
                        max[k] = max[k].max(p[k]);
                    }
                }
            }
        }
        (min[0] <= max[0]).then_some((min, max))
    }

    /// The offset subtracted from all positions: zero for models near the origin, else the
    /// bounds centre rounded to whole kilometres.
    pub fn origin(&self) -> [f64; 3] {
        match self.world_bounds() {
            Some((min, max)) => {
                let c = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, (min[2] + max[2]) / 2.0];
                if c.iter().any(|v| v.abs() > RECENTRE_DISTANCE) {
                    [(c[0] / 1000.0).round() * 1000.0, (c[1] / 1000.0).round() * 1000.0, (c[2] / 1000.0).round() * 1000.0]
                } else {
                    [0.0; 3]
                }
            }
            None => [0.0; 3],
        }
    }

    pub fn encode(mut self) -> Vec<u8> {
        self.groups.sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
        let origin = self.origin();
        let mut out = Vec::new();
        out.extend_from_slice(b"O3DS");
        put_u32(&mut out, 1);
        for v in origin {
            out.extend_from_slice(&v.to_le_bytes());
        }
        put_str(&mut out, &self.notes.join("\n"));

        let groups: Vec<&Group> = self.groups.iter().filter(|g| g.items.iter().any(|i| !i.positions.is_empty())).collect();
        put_u32(&mut out, groups.len() as u32);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for g in groups {
            put_str(&mut out, &g.name);
            let items: Vec<&Item> = g.items.iter().filter(|i| !i.positions.is_empty()).collect();
            put_u32(&mut out, items.len() as u32);
            for item in items {
                put_u32(&mut out, item.kind);
                for c in item.color {
                    out.extend_from_slice(&c.to_le_bytes());
                }
                let n = item.vertex_count();
                put_u32(&mut out, n as u32);
                let has_colors = item.kind == KIND_POINTS && item.colors.len() == n * 3;
                let extra = match item.kind {
                    KIND_MESH => item.indices.len() as u32,
                    KIND_POINTS => has_colors as u32,
                    _ => 0,
                };
                put_u32(&mut out, extra);
                out.reserve(n * 12);
                for p in item.positions.chunks_exact(3) {
                    for k in 0..3 {
                        let v = (p[k] - origin[k]) as f32;
                        min[k] = min[k].min(v);
                        max[k] = max[k].max(v);
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                }
                match item.kind {
                    KIND_MESH => {
                        for v in &item.normals {
                            out.extend_from_slice(&v.to_le_bytes());
                        }
                        for v in &item.indices {
                            out.extend_from_slice(&v.to_le_bytes());
                        }
                    }
                    KIND_POINTS if has_colors => {
                        out.extend_from_slice(&item.colors);
                        pad4(&mut out);
                    }
                    _ => {}
                }
            }
        }
        if min[0] > max[0] {
            min = [0.0; 3];
            max = [0.0; 3];
        }
        for v in min.iter().chain(max.iter()) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// Sort key that orders "Level 2" before "Level 10".
fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut num = String::new();
    let mut text = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !text.is_empty() {
                key.push((1, 0, std::mem::take(&mut text).to_lowercase()));
            }
            num.push(ch);
        } else {
            if !num.is_empty() {
                key.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
                num.clear();
            }
            text.push(ch);
        }
    }
    if !num.is_empty() {
        key.push((0, num.parse().unwrap_or(u64::MAX), String::new()));
    }
    if !text.is_empty() {
        key.push((1, 0, text.to_lowercase()));
    }
    key
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
    pad4(out);
}

fn pad4(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
