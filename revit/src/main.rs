//! Open 3D Viewer Studio with Revit support: the desktop app plus the OpenAEC Revit reader
//! (`rvt-reader`, private). Same command line as `open-3d-viewer`.
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

mod rvt;

fn main() {
    open_3d_viewer::loaders::add(vec![&rvt::RvtLoader]);
    open_3d_viewer::desktop::main();
}
