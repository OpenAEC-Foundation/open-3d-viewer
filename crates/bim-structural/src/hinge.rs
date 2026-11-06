//! Member-end release presets.
//!
//! For member ends `1` means connected and `0` released (codes `ux uy uz rx ry rz`).
//!
//! | Name | Member-end DOFs | Notes |
//! |---|---|---|
//! | `rigid` | `111111` | no release |
//! | `hinge`, `moment_hinge` | `111100` | ry and rz released |
//! | `hinge_y`, `hinge_z` | `111101`, `111110` | one bending axis released |
//! | `torsion` | `111011` | rx released |
//! | `axial` | `011111` | ux released |
//!
//! A name not in the table is never guessed: the importer keeps the end connected (`111111`)
//! and adds an `unknown-release-name` diagnostic.

use crate::model::{Dof, Dofs};
use crate::support::{code, dofs_from_code, lookup, PresetRow};

/// The member-end release table.
pub const HINGE_TABLE: &[PresetRow] = &[
    PresetRow { names: &["rigid"], code: "111111" },
    PresetRow { names: &["hinge", "moment_hinge"], code: "111100" },
    PresetRow { names: &["hinge_y"], code: "111101" },
    PresetRow { names: &["hinge_z"], code: "111110" },
    PresetRow { names: &["torsion"], code: "111011" },
    PresetRow { names: &["axial"], code: "011111" },
];

/// The symbol a member-end release is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HingePreset {
    Rigid,
    Hinge,
    HingeY,
    HingeZ,
    Torsion,
    Axial,
    /// A spring or nonlinear DOF.
    Spring,
    Custom,
}

/// The DOFs of a named release type, from [`HINGE_TABLE`].
pub fn expand(name: &str) -> Option<Dofs> {
    lookup(HINGE_TABLE, name).and_then(|row| dofs_from_code(row.code))
}

/// Whether a name is in [`HINGE_TABLE`].
pub fn is_known(name: &str) -> bool {
    lookup(HINGE_TABLE, name).is_some()
}

/// The display preset of a member-end release.
pub fn classify(dofs: &Dofs) -> HingePreset {
    if dofs.iter().any(|d| matches!(d, Dof::Spring(_) | Dof::Nonlinear { .. })) {
        return HingePreset::Spring;
    }
    match code(dofs).as_str() {
        "111111" => HingePreset::Rigid,
        "111100" => HingePreset::Hinge,
        "111101" => HingePreset::HingeY,
        "111110" => HingePreset::HingeZ,
        "111011" => HingePreset::Torsion,
        "011111" => HingePreset::Axial,
        _ => HingePreset::Custom,
    }
}

/// The canonical table name of a preset, if it has one.
pub fn preset_name(preset: HingePreset) -> Option<&'static str> {
    Some(match preset {
        HingePreset::Rigid => "rigid",
        HingePreset::Hinge => "hinge",
        HingePreset::HingeY => "hinge_y",
        HingePreset::HingeZ => "hinge_z",
        HingePreset::Torsion => "torsion",
        HingePreset::Axial => "axial",
        HingePreset::Spring | HingePreset::Custom => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_entry_classifies_back_to_its_name() {
        for row in HINGE_TABLE {
            for name in row.names {
                let dofs = expand(name).unwrap();
                assert_eq!(code(&dofs), row.code, "{name}");
                assert_eq!(preset_name(classify(&dofs)), Some(row.names[0]), "{name}");
            }
        }
        assert_eq!(expand("pinned"), None);
        let semi = [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Spring(5e6), Dof::Fixed];
        assert_eq!(classify(&semi), HingePreset::Spring);
    }
}
