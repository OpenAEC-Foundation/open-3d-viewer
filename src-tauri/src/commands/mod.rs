use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct SaveResult {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoadResult {
    success: bool,
    data: Option<String>,
    message: String,
}

/// Save data to a file
#[tauri::command]
pub fn save_file(path: String, data: String) -> SaveResult {
    match fs::write(&path, &data) {
        Ok(_) => SaveResult {
            success: true,
            message: format!("File saved to {}", path),
        },
        Err(e) => SaveResult {
            success: false,
            message: format!("Failed to save file: {}", e),
        },
    }
}

/// Load text data from a file
#[tauri::command]
pub fn load_file(path: String) -> LoadResult {
    match fs::read_to_string(&path) {
        Ok(content) => LoadResult {
            success: true,
            data: Some(content),
            message: "File loaded successfully".to_string(),
        },
        Err(e) => LoadResult {
            success: false,
            data: None,
            message: format!("Failed to load file: {}", e),
        },
    }
}

/// Read raw file bytes (for binary 3D formats)
#[tauri::command]
pub fn read_file_bytes(path: String) -> Result<Vec<u8>, String> {
    fs::read(&path).map_err(|e| format!("Failed to read file: {}", e))
}
