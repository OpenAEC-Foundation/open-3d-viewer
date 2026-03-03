// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod converter;

use commands::{save_file, load_file, read_file_bytes};
use converter::commands::{is_rust_convertible, convert_3d_file};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            save_file,
            load_file,
            read_file_bytes,
            is_rust_convertible,
            convert_3d_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
