//! Open BIM Collaborator: the Collaborator of this code base on its own, without the switch between the products
//! (docs/architecture/products.md).
#![cfg_attr(all(not(debug_assertions), not(test)), windows_subsystem = "windows")]

use open_3d_viewer::product::{Edition, Product};

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    open_3d_viewer::desktop::main_as(Edition::Only(Product::Collaborator));
    #[cfg(target_arch = "wasm32")]
    open_3d_viewer::web::main_as(Edition::Only(Product::Collaborator));
}
