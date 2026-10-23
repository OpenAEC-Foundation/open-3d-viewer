use tauri::{AppHandle, Emitter};
use crate::converter::manager;
use crate::converter::types::{ConversionProgress, ConversionResult};

/// Check if a file extension is handled by the Rust converter.
#[tauri::command]
pub fn is_rust_convertible(extension: String) -> bool {
    manager::is_rust_format(&extension)
}

/// Convert a Revit file (.rvt) to triangle meshes per category.
///
/// The request body carries the raw file bytes; the response is the binary scene format of
/// `rvt_export::mesh::encode` ("RVTM"), parsed by the frontend's RVT loader.
#[tauri::command]
pub async fn convert_rvt(request: tauri::ipc::Request<'_>) -> Result<tauri::ipc::Response, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("convert_rvt expects the raw file bytes".to_string());
    };
    let bytes = bytes.clone();
    let encoded = tokio::task::spawn_blocking(move || convert_rvt_bytes(bytes))
        .await
        .map_err(|e| format!("Conversion thread error: {}", e))??;
    Ok(tauri::ipc::Response::new(encoded))
}

#[cfg(feature = "rvt")]
fn convert_rvt_bytes(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let doc = rvt_read::Document::from_bytes(bytes).map_err(|e| format!("Cannot read Revit file: {}", e))?;
    Ok(rvt_export::mesh::encode(&rvt_export::mesh::scene(&doc)))
}

#[cfg(not(feature = "rvt"))]
fn convert_rvt_bytes(_bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    Err("This build has no Revit support (feature \"rvt\" disabled)".to_string())
}

/// Convert a 3D file (STEP) to triangle meshes.
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
