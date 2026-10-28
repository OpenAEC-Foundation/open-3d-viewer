// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod cad_import;
mod converter;

use commands::{save_file, load_file, read_file_bytes};
use converter::commands::{is_rust_convertible, convert_3d_file, convert_rvt};
use cad_import::commands::{convert_dwg, convert_e57};
use std::sync::Mutex;

/// File passed on the command line (file association, "Open with"). The frontend takes it once it
/// is ready; pushing it with a timed event raced with the frontend's start-up.
struct StartupFile(Mutex<Option<String>>);

#[tauri::command]
fn take_startup_file(state: tauri::State<StartupFile>) -> Option<String> {
    state.0.lock().ok().and_then(|mut f| f.take())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(StartupFile(Mutex::new(std::env::args().nth(1))))
        .invoke_handler(tauri::generate_handler![
            save_file,
            load_file,
            read_file_bytes,
            is_rust_convertible,
            convert_3d_file,
            convert_rvt,
            convert_dwg,
            convert_e57,
            take_startup_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
