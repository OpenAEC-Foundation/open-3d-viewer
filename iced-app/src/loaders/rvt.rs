//! Revit (.rvt) via the OpenAEC rvt reader: one layer per category batch.

use super::Loader;
use crate::model::{y_up_to_z_up, Layer};
use std::path::Path;

pub struct RvtLoader;

impl Loader for RvtLoader {
    fn name(&self) -> &'static str {
        "Revit"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rvt", "rfa"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, path: &Path) -> Result<Vec<Layer>, String> {
        let doc = rvt_read::Document::open(path).map_err(|e| format!("Cannot read Revit file: {e}"))?;
        // `scene` returns view coordinates: metres, Y up (x, z, -y)
        let scene = rvt_export::mesh::scene(&doc);
        Ok(scene
            .batches
            .into_iter()
            .map(|b| {
                let mut positions = b.positions;
                let mut normals = b.normals;
                y_up_to_z_up(&mut positions);
                y_up_to_z_up(&mut normals);
                Layer { name: b.name, color: b.color, positions, normals, colors: None, indices: b.indices }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Read-only sample outside the repo; skipped when absent.
    const SAMPLE: &str = r"C:\Users\AIA12\rvt-cleanroom\samples\web\autodesk_help\racbasicsampleproject_2026.rvt";

    #[test]
    fn loads_revit_sample() {
        let path = Path::new(SAMPLE);
        if !path.exists() {
            eprintln!("skipped: {SAMPLE} not found");
            return;
        }
        let model = crate::loaders::load(path).expect("rvt");
        assert!(model.layers.len() > 3, "{} layers", model.layers.len());
        assert!(model.triangle_count() > 1000);
        // Z up: a building is wider than it is tall
        let s = model.bounds.size();
        assert!(s[2] < s[0].max(s[1]), "size {s:?}");
        eprintln!(
            "rvt: {} layers, {} triangles, {:?}, bounds {:?}",
            model.layers.len(),
            model.triangle_count(),
            model.load_time,
            model.bounds
        );
    }
}
