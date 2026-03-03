use tauri::{AppHandle, Emitter};
use crate::converter::manager;
use crate::converter::types::{ConversionProgress, ConversionResult};

/// Check if a file extension is handled by the Rust converter.
#[tauri::command]
pub fn is_rust_convertible(extension: String) -> bool {
    manager::is_rust_format(&extension)
}

/// Convert a 3D file (STEP, SKP) to triangle meshes.
/// Emits progress events during conversion.
#[tauri::command]
pub async fn convert_3d_file(
    app: AppHandle,
    file_id: String,
    file_path: String,
) -> Result<ConversionResult, String> {
    // Emit initial progress
    let _ = app.emit(
        "conversion-progress",
        ConversionProgress {
            file_id: file_id.clone(),
            progress: 0.0,
            phase: "Starting conversion...".to_string(),
        },
    );

    // Run conversion in a blocking thread to avoid blocking the async runtime
    let result = tokio::task::spawn_blocking(move || {
        manager::convert_file(&file_path)
    })
    .await
    .map_err(|e| format!("Conversion thread error: {}", e))?;

    // Emit completion
    let _ = app.emit(
        "conversion-progress",
        ConversionProgress {
            file_id,
            progress: 1.0,
            phase: if result.success {
                "Conversion complete".to_string()
            } else {
                result.error.clone().unwrap_or_default()
            },
        },
    );

    Ok(result)
}
