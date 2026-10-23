// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod converter;

use commands::{save_file, load_file, read_file_bytes};
use converter::commands::{is_rust_convertible, convert_3d_file, convert_rvt};
use tauri::Emitter;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            // If a file path was passed as CLI argument (e.g. from file association),
            // emit it to the frontend so it can load the file.
            let args: Vec<String> = std::env::args().collect();
            if args.len() > 1 {
                let file_path = args[1].clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Small delay to ensure frontend is mounted and listening
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    let _ = handle.emit("open-file", file_path);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            save_file,
            load_file,
            read_file_bytes,
            is_rust_convertible,
            convert_3d_file,
            convert_rvt,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
