//! Support presets: the one place that turns DOFs into a symbol, and the documented table that
//! expands a named support type into DOFs.
//!
//! DOF codes are six characters `ux uy uz rx ry rz`: `1` restrained, `0` free, `s` spring,
//! `n` nonlinear.
//!
//! | Name (case-insensitive) | Support DOFs | Notes |
//! |---|---|---|
//! | `fixed`, `clamped` | `111111` | |
//! | `pinned`, `hinged`, `hinge` | `111000` | |
//! | `roller` | `001000` | free in both horizontal directions of the support frame |
//! | `roller_x`, `roller_y` | `011000`, `101000` | free along the named axis only |
//! | `sliding_x`, `sliding_y` | `011111`, `101111` | guided: rotations fixed |
//! | `free` | `000000` | |
//!
//! A name not in the table is never guessed: the importer keeps it as `Custom` with every DOF
//! free and adds an `unknown-support-name` diagnostic.

use glam::DVec3;

use crate::model::{Dimension, Dof, Dofs, SupportPreset};

/// One row of a preset table.
#[derive(Clone, Copy, Debug)]
pub struct PresetRow {
    /// Accepted names; the first is the canonical one.
    pub names: &'static [&'static str],
    /// DOF code `ux uy uz rx ry rz`.
    pub code: &'static str,
}

/// The support preset table.
pub const SUPPORT_TABLE: &[PresetRow] = &[
    PresetRow { names: &["fixed", "clamped"], code: "111111" },
    PresetRow { names: &["pinned", "hinged", "hinge"], code: "111000" },
    PresetRow { names: &["roller"], code: "001000" },
    PresetRow { names: &["roller_x"], code: "011000" },
    PresetRow { names: &["roller_y"], code: "101000" },
    PresetRow { names: &["sliding_x"], code: "011111" },
    PresetRow { names: &["sliding_y"], code: "101111" },
    PresetRow { names: &["free"], code: "000000" },
];

/// The six-character DOF code of a DOF set (`1` restrained/connected, `0` free/released,
/// `s` spring, `n` nonlinear).
pub fn code(dofs: &Dofs) -> String {
    dofs.iter()
        .map(|d| match d {
            Dof::Fixed => '1',
            Dof::Free => '0',
            Dof::Spring(_) => 's',
            Dof::Nonlinear { .. } => 'n',
        })
        .collect()
}

/// DOFs from a code of `1` and `0` only.
pub fn dofs_from_code(code: &str) -> Option<Dofs> {
    let chars: Vec<char> = code.chars().collect();
    if chars.len() != 6 {
        return None;
    }
    let mut out: Dofs = [Dof::Free, Dof::Free, Dof::Free, Dof::Free, Dof::Free, Dof::Free];
    for (slot, c) in out.iter_mut().zip(chars) {
        *slot = match c {
            '1' => Dof::Fixed,
            '0' => Dof::Free,
            _ => return None,
        };
    }
    Some(out)
}

pub(crate) fn lookup<'a>(table: &'a [PresetRow], name: &str) -> Option<&'a PresetRow> {
    let name = name.trim().to_ascii_lowercase();
    table.iter().find(|row| row.names.contains(&name.as_str()))
}

/// The DOFs of a named support type, from [`SUPPORT_TABLE`].
pub fn expand(name: &str) -> Option<Dofs> {
    lookup(SUPPORT_TABLE, name).and_then(|row| dofs_from_code(row.code))
}

/// Whether a name is in [`SUPPORT_TABLE`].
pub fn is_known(name: &str) -> bool {
    lookup(SUPPORT_TABLE, name).is_some()
}

/// The display preset of a support.
///
/// In a plane model whose normal is a global axis, only the in-plane DOFs count (the two
/// in-plane translations and the rotation about the normal): the out-of-plane translation is
/// taken as restrained and the out-of-plane rotations follow the in-plane one. Other normals
/// classify as in 3D.
///
/// Springs and nonlinear DOFs give `Spring` when no DOF is restrained, else `Custom`.
pub fn classify(dofs: &Dofs, dimension: Dimension) -> SupportPreset {
    match dimension {
        Dimension::Space3d => classify_3d(dofs),
        Dimension::Plane2d { normal } => match axis_of(normal) {
            Some(k) => {
                let mut d = dofs.clone();
                d[k] = Dof::Fixed;
                for j in 0..3 {
                    if j != k {
                        d[3 + j] = dofs[3 + k].clone();
                    }
                }
                classify_3d(&d)
            }
            None => classify_3d(dofs),
        },
    }
}

fn axis_of(n: DVec3) -> Option<usize> {
    let n = n.normalize_or_zero();
    (0..3).find(|&k| n[k].abs() > 1.0 - 1e-9)
}

fn classify_3d(dofs: &Dofs) -> SupportPreset {
    let elastic = dofs.iter().any(|d| matches!(d, Dof::Spring(_) | Dof::Nonlinear { .. }));
    let fixed = |d: &Dof| matches!(d, Dof::Fixed);
    let free = |d: &Dof| matches!(d, Dof::Free);
    if elastic {
        return if dofs.iter().any(fixed) { SupportPreset::Custom } else { SupportPreset::Spring };
    }
    if dofs.iter().all(fixed) {
        return SupportPreset::Fixed;
    }
    if dofs.iter().all(free) {
        return SupportPreset::Free;
    }
    let t_free = [free(&dofs[0]), free(&dofs[1]), free(&dofs[2])];
    let any_t_free = t_free.iter().any(|&f| f);
    let any_t_fixed = t_free.iter().any(|&f| !f);
    let rot_free = dofs[3..].iter().all(free);
    let rot_fixed = dofs[3..].iter().all(fixed);
    if rot_free {
        if !any_t_free {
            return SupportPreset::Pinned;
        }
        return SupportPreset::Roller { free: t_free };
    }
    if rot_fixed && any_t_free && any_t_fixed {
        return SupportPreset::Sliding { free: t_free };
    }
    SupportPreset::Custom
}

/// The canonical table name of a preset, if it has one.
pub fn preset_name(preset: SupportPreset) -> Option<&'static str> {
    Some(match preset {
        SupportPreset::Fixed => "fixed",
        SupportPreset::Pinned => "pinned",
        SupportPreset::Roller { free: [true, true, false] } => "roller",
        SupportPreset::Roller { free: [true, false, false] } => "roller_x",
        SupportPreset::Roller { free: [false, true, false] } => "roller_y",
        SupportPreset::Sliding { free: [true, false, false] } => "sliding_x",
        SupportPreset::Sliding { free: [false, true, false] } => "sliding_y",
        SupportPreset::Free => "free",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_entry_classifies_back_to_its_name() {
        for row in SUPPORT_TABLE {
            for name in row.names {
                let dofs = expand(name).unwrap();
                assert_eq!(code(&dofs), row.code, "{name}");
                let preset = classify(&dofs, Dimension::Space3d);
                assert_eq!(preset_name(preset), Some(row.names[0]), "{name}");
                assert_eq!(expand(&name.to_uppercase()), Some(dofs));
            }
        }
        assert_eq!(expand("bogus"), None);
    }

    #[test]
    fn springs_and_custom() {
        let mut d = expand("pinned").unwrap();
        d[2] = Dof::Spring(1e6);
        assert_eq!(classify(&d, Dimension::Space3d), SupportPreset::Custom);
        let s = [Dof::Free, Dof::Free, Dof::Spring(1e6), Dof::Free, Dof::Free, Dof::Free];
        assert_eq!(classify(&s, Dimension::Space3d), SupportPreset::Spring);
        let odd = dofs_from_code("110100").unwrap();
        assert_eq!(classify(&odd, Dimension::Space3d), SupportPreset::Custom);
    }

    #[test]
    fn plane_models_use_in_plane_dofs() {
        let xz = Dimension::Plane2d { normal: DVec3::Y };
        // ux free, uz fixed, ry free; the out-of-plane DOFs say nothing.
        let roller = dofs_from_code("001100").unwrap();
        assert_eq!(preset_name(classify(&roller, xz)), Some("roller_x"));
        let pinned = dofs_from_code("101000").unwrap();
        assert_eq!(classify(&pinned, xz), SupportPreset::Pinned);
        let fixed = dofs_from_code("101010").unwrap();
        assert_eq!(classify(&fixed, xz), SupportPreset::Fixed);
    }
}
