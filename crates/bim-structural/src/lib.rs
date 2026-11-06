//! The structural analysis model: a headless, vendor-neutral document of nodes, members,
//! surfaces, supports, releases, sections, loads and results in f64 and SI units, with the
//! `sam-json` v1 exchange format, support and hinge presets, section geometry and member frames.
//!
//! No GUI or GPU types; builds for every target the viewer has, wasm32 included. See
//! `docs/superpowers/specs/2026-11-06-structural-model-design.md` in the repository.

pub mod base64;
pub mod exchange;
pub mod frame;
pub mod hinge;
pub mod id;
pub mod model;
pub mod results;
pub mod section;
pub mod support;

pub use id::{sam_id, Id, IdKind};
pub use model::*;
