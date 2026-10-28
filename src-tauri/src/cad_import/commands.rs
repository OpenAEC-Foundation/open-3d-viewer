//! Tauri commands: the frontend sends the raw file bytes and receives an "O3DS" buffer.

fn raw_body(request: &tauri::ipc::Request<'_>, command: &str) -> Result<Vec<u8>, String> {
    match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        _ => Err(format!("{command} expects the raw file bytes")),
    }
}

/// Convert an AutoCAD DWG file (model space, 3D) to the O3DS scene buffer.
#[tauri::command]
pub async fn convert_dwg(request: tauri::ipc::Request<'_>) -> Result<tauri::ipc::Response, String> {
    let bytes = raw_body(&request, "convert_dwg")?;
    let encoded = tokio::task::spawn_blocking(move || super::dwg::convert(&bytes))
        .await
        .map_err(|e| format!("Conversion thread error: {}", e))??;
    Ok(tauri::ipc::Response::new(encoded))
}

/// Convert an E57 point cloud file to the O3DS scene buffer.
#[tauri::command]
pub async fn convert_e57(request: tauri::ipc::Request<'_>) -> Result<tauri::ipc::Response, String> {
    let bytes = raw_body(&request, "convert_e57")?;
    let encoded = tokio::task::spawn_blocking(move || super::e57::convert(&bytes))
        .await
        .map_err(|e| format!("Conversion thread error: {}", e))??;
    Ok(tauri::ipc::Response::new(encoded))
}
