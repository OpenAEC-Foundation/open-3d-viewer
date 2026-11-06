//! What the GPU knows per unit (an element, or the rest of a layer; see [`crate::explode`]): the
//! offset of the exploded view in `xyz` and flags in `w`, one texel of the units texture per unit.
//! Hiding or highlighting an element is one write of that texture; the geometry never changes.
//! `scene.wgsl` reads the flags with the same values.

use crate::model::ElementKey;

/// A unit that keeps its name when another model is added: an element by its key, or the
/// triangles outside elements of a layer by its model and its index within that model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKey {
    Element(ElementKey),
    LayerRest { model: u32, layer: u32 },
}

/// Not drawn: no faces, edges or cut fill.
pub const UNIT_HIDDEN: u8 = 1;
/// Drawn in the selection colour.
pub const UNIT_SELECTED: u8 = 2;
/// Units the texture can hold: 1024 per row and, with WebGL2, at most 2048 rows. Units beyond
/// keep no offset and no flags.
pub const MAX_UNITS: usize = 2048 * 1024;

/// The state of every unit: the offset from `offsets` (zero where it has none) and the flags
/// from `flags` as a number in `w`. Unit 0, the geometry outside any unit, never gets flags.
pub fn unit_state(offsets: &[[f32; 4]], flags: &[u8]) -> Vec<[f32; 4]> {
    let n = offsets.len().max(flags.len()).min(MAX_UNITS);
    (0..n)
        .map(|u| {
            let [x, y, z, _] = offsets.get(u).copied().unwrap_or([0.0; 4]);
            let f = if u == 0 { 0 } else { flags.get(u).copied().unwrap_or(0) };
            [x, y, z, f as f32]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_state_puts_flags_in_w() {
        let s = unit_state(&[[0.0; 4], [1.0, 2.0, 3.0, 0.0]], &[0, UNIT_HIDDEN | UNIT_SELECTED, UNIT_SELECTED]);
        assert_eq!(s.len(), 3);
        assert_eq!(s[1], [1.0, 2.0, 3.0, 3.0]);
        assert_eq!(s[2], [0.0, 0.0, 0.0, 2.0]);
    }

    #[test]
    fn unit_zero_never_gets_flags() {
        assert_eq!(unit_state(&[], &[UNIT_HIDDEN])[0][3], 0.0);
    }

    #[test]
    fn unit_state_is_capped_to_texture_limit() {
        assert_eq!(unit_state(&[], &vec![UNIT_HIDDEN; MAX_UNITS + 10]).len(), MAX_UNITS);
    }
}
