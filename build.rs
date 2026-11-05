//! Windows: the app icon and version info in the .exe.

#[path = "src/product.rs"]
#[allow(dead_code)]
mod product;

fn main() {
    println!("cargo:rerun-if-changed=src/product.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_BIM_VALIDATOR");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico")
            .set("ProductName", product::CURRENT.name)
            .set("FileDescription", product::CURRENT.name)
            .set("OriginalFilename", &format!("{}.exe", product::CURRENT.id));
        if let Err(e) = res.compile() {
            println!("cargo:warning=no icon in the .exe: {e}");
        }
    }
}
