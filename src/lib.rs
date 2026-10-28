//! Open 3D Viewer Studio: a 3D model viewer for AEC files, in Rust (iced + wgpu).
//!
//! One code base for two targets:
//! - the desktop app (`src/main.rs`, [`desktop`]): command line, native window, file dialogs,
//!   screenshots, benchmarks and remote control for AI agents (the MCP server);
//! - the web app (`trunk build`, [`web`]): the same UI in the browser on WebGPU, with files
//!   opened through the browser.
//!
//! Formats outside this crate (Revit, `revit/`) plug in through [`loaders::add`].

mod annotations;
pub mod app;
#[cfg(not(target_arch = "wasm32"))]
mod bench;
#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;
mod explode;
pub mod export;
mod icons;
pub mod loaders;
mod measure;
pub mod model;
mod remote;
pub mod remote_link;
pub mod section;
pub mod shell;
mod theme;
mod view_cube;
mod viewport;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

/// Writes RGBA8 pixels as a PNG file.
pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}
