use std::path::Path;
use crate::converter::types::{ConversionResult, ConvertedMesh, RustFormat};

/// Convert a 3D file to a list of mesh batches.
///
/// Currently supports:
/// - STEP/STP: Requires opencascade-rs (planned)
/// - SKP: Requires assimp-rs (planned)
///
/// Returns a ConversionResult with geometry data that can be sent to the frontend.
pub fn convert_file(file_path: &str) -> ConversionResult {
    let path = Path::new(file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let format = match RustFormat::from_extension(ext) {
        Some(f) => f,
        None => {
            return ConversionResult {
                success: false,
                meshes: vec![],
                total_vertices: 0,
                total_triangles: 0,
                bounds_min: [0.0; 3],
                bounds_max: [0.0; 3],
                error: Some(format!("Unsupported format: {}", ext)),
            };
        }
    };

    match format {
        RustFormat::Step => convert_step(file_path),
        RustFormat::Skp => convert_skp(file_path),
    }
}

/// Check if a file extension is handled by the Rust converter.
pub fn is_rust_format(ext: &str) -> bool {
    RustFormat::from_extension(ext).is_some()
}

/// STEP/STP converter.
///
/// TODO: Implement using opencascade-rs or truck crate.
/// For now returns a stub result.
fn convert_step(file_path: &str) -> ConversionResult {
    let _ = file_path;

    // Placeholder: actual implementation would use opencascade-rs to:
    // 1. Parse STEP file using BRep reader
    // 2. Tessellate each shape into triangle meshes
    // 3. Return batches of ConvertedMesh

    ConversionResult {
        success: false,
        meshes: vec![],
        total_vertices: 0,
        total_triangles: 0,
        bounds_min: [0.0; 3],
        bounds_max: [0.0; 3],
        error: Some(
            "STEP format support requires opencascade-rs. \
             Install via Cargo.toml: opencascade = \"0.2\""
                .to_string(),
        ),
    }
}

/// SketchUp SKP converter.
///
/// TODO: Implement using assimp-rs FFI.
/// For now returns a stub result.
fn convert_skp(file_path: &str) -> ConversionResult {
    let _ = file_path;

    ConversionResult {
        success: false,
        meshes: vec![],
        total_vertices: 0,
        total_triangles: 0,
        bounds_min: [0.0; 3],
        bounds_max: [0.0; 3],
        error: Some(
            "SKP format support requires assimp-rs. \
             Install via Cargo.toml: assimp = \"0.3\""
                .to_string(),
        ),
    }
}

/// Create a simple box mesh for testing the pipeline.
#[allow(dead_code)]
pub fn create_test_box(size: f32) -> ConvertedMesh {
    let h = size / 2.0;

    #[rustfmt::skip]
    let positions = vec![
        // Front face
        -h, -h,  h,   h, -h,  h,   h,  h,  h,  -h,  h,  h,
        // Back face
        -h, -h, -h,  -h,  h, -h,   h,  h, -h,   h, -h, -h,
        // Top face
        -h,  h, -h,  -h,  h,  h,   h,  h,  h,   h,  h, -h,
        // Bottom face
        -h, -h, -h,   h, -h, -h,   h, -h,  h,  -h, -h,  h,
        // Right face
         h, -h, -h,   h,  h, -h,   h,  h,  h,   h, -h,  h,
        // Left face
        -h, -h, -h,  -h, -h,  h,  -h,  h,  h,  -h,  h, -h,
    ];

    #[rustfmt::skip]
    let indices = vec![
        0, 1, 2,  0, 2, 3,     // Front
        4, 5, 6,  4, 6, 7,     // Back
        8, 9, 10, 8, 10, 11,   // Top
        12,13,14, 12,14,15,    // Bottom
        16,17,18, 16,18,19,    // Right
        20,21,22, 20,22,23,    // Left
    ];

    #[rustfmt::skip]
    let normals = vec![
        0.0, 0.0, 1.0,  0.0, 0.0, 1.0,  0.0, 0.0, 1.0,  0.0, 0.0, 1.0,
        0.0, 0.0,-1.0,  0.0, 0.0,-1.0,  0.0, 0.0,-1.0,  0.0, 0.0,-1.0,
        0.0, 1.0, 0.0,  0.0, 1.0, 0.0,  0.0, 1.0, 0.0,  0.0, 1.0, 0.0,
        0.0,-1.0, 0.0,  0.0,-1.0, 0.0,  0.0,-1.0, 0.0,  0.0,-1.0, 0.0,
        1.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 0.0, 0.0,
       -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0,
    ];

    ConvertedMesh {
        batch_index: 0,
        positions,
        indices,
        normals,
        is_last: true,
    }
}
