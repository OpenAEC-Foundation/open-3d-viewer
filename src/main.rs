//! Open 3D Viewer Studio: the desktop app, or the web app when built for wasm32 (`trunk build`).
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

fn main() {
    if cfg!(feature = "bim-validator") {
        eprintln!("Build open-3d-viewer without the bim-validator feature; run open-bim-validator for that product.");
        std::process::exit(2);
    }
    #[cfg(not(target_arch = "wasm32"))]
    open_3d_viewer::desktop::main();
    #[cfg(target_arch = "wasm32")]
    open_3d_viewer::web::main();
}
