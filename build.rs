//! Windows: the app icon and version info in the .exe.

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico").set("ProductName", "Open BIM Suite").set("FileDescription", "Open BIM Suite");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no icon in the .exe: {e}");
        }
    }
}
