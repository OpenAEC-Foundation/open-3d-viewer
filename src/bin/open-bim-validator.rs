//! Native Open BIM Validator desktop entrypoint.
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(code) = open_3d_viewer::validation_cli::run() {
            std::process::exit(code);
        }
        open_3d_viewer::desktop::main();
    }
    #[cfg(target_arch = "wasm32")]
    compile_error!("Open BIM Validator is a native desktop product.");
}
