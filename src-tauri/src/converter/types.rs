use serde::{Deserialize, Serialize};

/// A single mesh batch in the binary wire format.
/// Sent from Rust → JS as binary data for efficient IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertedMesh {
    /// Batch index (for incremental loading)
    pub batch_index: u32,
    /// Vertex positions (f32 xyz interleaved)
    pub positions: Vec<f32>,
    /// Triangle indices (u32)
    pub indices: Vec<u32>,
    /// Vertex normals (f32 xyz interleaved, optional)
    pub normals: Vec<f32>,
    /// Whether this is the last batch
    pub is_last: bool,
}

/// Result from a full file conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    pub success: bool,
    pub meshes: Vec<ConvertedMesh>,
    pub total_vertices: u64,
    pub total_triangles: u64,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    pub error: Option<String>,
}

/// Progress event sent during conversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionProgress {
    pub file_id: String,
    pub progress: f64,
    pub phase: String,
}

/// Supported Rust-side formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustFormat {
    Step,
    Skp,
}

impl RustFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "step" | "stp" => Some(RustFormat::Step),
            "skp" => Some(RustFormat::Skp),
            _ => None,
        }
    }
}
