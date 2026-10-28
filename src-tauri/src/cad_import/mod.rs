//! DWG (3D model space) and E57 (point cloud) import.
//!
//! Both formats are converted in Rust and handed to the frontend as one binary "O3DS"
//! buffer (see `encode`), read by `src/engine/loaders/formats/CadImportLoaders.ts`.

pub mod acis;
pub mod commands;
pub mod dwg;
pub mod e57;
pub mod encode;
