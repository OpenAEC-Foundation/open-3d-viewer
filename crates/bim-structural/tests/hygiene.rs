//! Clean-room hygiene: only our own source and exchange files live under `tests/`. FE-package
//! project files never enter this repository; fixtures are `.samjson` (and `.samr` blocks) or
//! IFC files we author ourselves.

use std::path::Path;

const ALLOWED: &[&str] = &["rs", "samjson", "samr", "ifc", "json", "md"];

fn walk(dir: &Path, bad: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, bad);
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if !ALLOWED.contains(&ext.as_str()) {
                bad.push(path.display().to_string());
            }
        }
    }
}

#[test]
fn only_allowed_file_types_under_tests() {
    let mut bad = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"), &mut bad);
    assert!(bad.is_empty(), "file types not on the allow-list ({ALLOWED:?}): {bad:#?}");
}
