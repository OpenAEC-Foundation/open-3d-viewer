//! Adapter from the DICOM reader (crate dcm-read) to the app's scene model: the STL or OBJ a
//! DICOM file carries is read by the app's own reader of that format.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct DicomLoader;

impl Loader for DicomLoader {
    fn name(&self) -> &'static str { "DICOM" }
    fn extensions(&self) -> &'static [&'static str] { &["dcm", "dicom"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let doc = dcm_read::read(&source.read()?)?;
        progress.set(Phase::Convert);
        let mut info = doc.info.clone();
        let (mut layers, y_up) = match doc.kind {
            dcm_read::Kind::Stl => {
                let stl = Source::Bytes { name: format!("{}.stl", source.stem()), data: std::sync::Arc::new(doc.bytes.clone()) };
                (super::stl::StlLoader.load(&stl)?, false)
            }
            dcm_read::Kind::Obj => {
                let mtl = material_library(source, &doc.references);
                if !doc.references.is_empty() && mtl.is_none() {
                    info.push(("Material library".into(), "not found beside the file".into()));
                }
                (super::obj::from_bytes(&doc.bytes, mtl.as_deref())?, true)
            }
            dcm_read::Kind::Mtl => return Err("DICOM: this file is the material library of a model, not a model; open the model's file".into()),
        };
        for layer in &mut layers {
            // the model is in the patient's frame as written: the OBJ reader's turn from Y up
            // is undone, and millimetres become metres
            for v in layer.positions.chunks_exact_mut(3).chain(layer.normals.chunks_exact_mut(3)) {
                if y_up {
                    let (y, z) = (v[1], v[2]);
                    v[1] = z;
                    v[2] = -y;
                }
            }
            for p in &mut layer.positions {
                *p *= doc.scale;
            }
            if layer.name.is_empty() {
                layer.name = source.stem();
            }
        }
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() }, structural: Vec::new(), origin: None })
    }
}

/// The material library an OBJ document refers to: a DICOM file beside it whose instance is
/// one of `references`.
#[cfg(not(target_arch = "wasm32"))]
fn material_library(source: &Source, references: &[String]) -> Option<Vec<u8>> {
    let folder = source.path()?.parent()?;
    std::fs::read_dir(folder).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dcm"))).find_map(|path| {
        super::check_reference(source.path()?, &path).ok()?;
        // material libraries are small; leave image files and the like unread
        if std::fs::metadata(&path).ok()?.len() > 16 << 20 {
            return None;
        }
        let doc = dcm_read::read(&std::fs::read(&path).ok()?).ok()?;
        (doc.kind == dcm_read::Kind::Mtl && references.contains(&doc.instance)).then_some(doc.bytes)
    })
}

#[cfg(target_arch = "wasm32")]
fn material_library(_: &Source, _: &[String]) -> Option<Vec<u8>> {
    None
}
