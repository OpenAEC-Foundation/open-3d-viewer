//! What the BIM Validator works with, apart from the interface: the clash test on meshes and
//! the rule sets that say what a project checks. The panel and its work on the open project are
//! in `crate::app::collab`; issues are in the `bim-bcf` crate, information checks in
//! `bim-validator-core`.

pub mod bcf_api;
pub mod bsdd;
pub mod clash;
pub mod deviation;
pub mod floor;
pub mod frame;
pub mod rules;
pub mod share;
