//! DICOM files that carry a 3D model: the Encapsulated STL, OBJ and MTL documents of DICOM
//! supplements 205 and 208. The file is read by `dicom-object`; the document it carries is
//! handed back as the bytes of an STL, OBJ or MTL file, for a reader of that format.
//!
//! Image series (CT, MR and the other images) and other documents (PDF, CDA) carry no model:
//! [`read`] says what such a file holds instead.

use dicom_dictionary_std::{tags, uids};
use dicom_object::file::ReadPreamble;
use dicom_object::{DefaultDicomObject, OpenFileOptions};

/// The kind of model document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Stl,
    Obj,
    Mtl,
}

impl Kind {
    /// The extension of the carried file.
    pub fn extension(self) -> &'static str {
        match self {
            Kind::Stl => "stl",
            Kind::Obj => "obj",
            Kind::Mtl => "mtl",
        }
    }
}

/// A model document read from a DICOM file.
#[derive(Debug, Clone)]
pub struct Document {
    pub kind: Kind,
    /// The carried file, without the padding DICOM adds to an even length.
    pub bytes: Vec<u8>,
    /// The instance's own UID.
    pub instance: String,
    /// The UIDs of the instances it refers to (an OBJ to its MTL).
    pub references: Vec<String>,
    /// The scale from the document's units to metres: DICOM models are in millimetres.
    pub scale: f32,
    /// What the file says of it: title, patient-free description, manufacturer, dates.
    pub info: Vec<(String, String)>,
}

/// Reads a DICOM file that carries a model.
pub fn read(data: &[u8]) -> Result<Document, String> {
    let object = OpenFileOptions::new().read_preamble(ReadPreamble::Auto).from_reader(data).map_err(|e| format!("DICOM: {e}"))?;
    document(&object)
}

/// The model document of a parsed file, or what the file holds instead.
pub fn document(object: &DefaultDicomObject) -> Result<Document, String> {
    let class = object.meta().media_storage_sop_class_uid().trim_end_matches('\0').trim().to_string();
    let text = |tag| object.element_opt(tag).ok().flatten().and_then(|e| e.to_str().ok()).map(|s| s.trim_end_matches('\0').trim().to_string()).filter(|s| !s.is_empty());
    let mime = text(tags::MIME_TYPE_OF_ENCAPSULATED_DOCUMENT).unwrap_or_default().to_lowercase();
    let kind = match class.as_str() {
        uids::ENCAPSULATED_STL_STORAGE => Some(Kind::Stl),
        uids::ENCAPSULATED_OBJ_STORAGE => Some(Kind::Obj),
        uids::ENCAPSULATED_MTL_STORAGE => Some(Kind::Mtl),
        _ => match mime.as_str() {
            "model/stl" | "model/x.stl-binary" | "application/sla" => Some(Kind::Stl),
            "model/obj" => Some(Kind::Obj),
            "model/mtl" => Some(Kind::Mtl),
            _ => None,
        },
    };
    let Some(kind) = kind else {
        return Err(not_a_model(object, &class, &mime));
    };
    let element = object.element(tags::ENCAPSULATED_DOCUMENT).map_err(|_| "DICOM: the model document has no encapsulated file".to_string())?;
    let mut bytes = element.to_bytes().map_err(|e| format!("DICOM: {e}"))?.into_owned();
    // the length of the document before DICOM padded it to an even length
    let length = object.element_opt(tags::ENCAPSULATED_DOCUMENT_LENGTH).ok().flatten().and_then(|e| e.to_int::<u32>().ok());
    match length {
        Some(n) if (n as usize) <= bytes.len() => bytes.truncate(n as usize),
        _ => {
            while bytes.last() == Some(&0) && kind != Kind::Stl {
                bytes.pop();
            }
        }
    }
    let instance = object.meta().media_storage_sop_instance_uid().trim_end_matches('\0').trim().to_string();
    let references = object
        .element_opt(tags::REFERENCED_INSTANCE_SEQUENCE)
        .ok()
        .flatten()
        .and_then(|e| e.items().map(|items| items.to_vec()))
        .unwrap_or_default()
        .iter()
        .filter_map(|item| item.element_opt(tags::REFERENCED_SOP_INSTANCE_UID).ok().flatten().and_then(|e| e.to_str().ok()).map(|s| s.trim_end_matches('\0').trim().to_string()))
        .collect();
    let mut info = Vec::new();
    for (name, tag) in [
        ("Document title", tags::DOCUMENT_TITLE),
        ("Series description", tags::SERIES_DESCRIPTION),
        ("Manufacturer", tags::MANUFACTURER),
        ("Model name", tags::MANUFACTURER_MODEL_NAME),
        ("Content date", tags::CONTENT_DATE),
        ("Modality", tags::MODALITY),
    ] {
        if let Some(value) = text(tag) {
            info.push((name.to_string(), value));
        }
    }
    info.push(("Encapsulated document".into(), kind.extension().to_uppercase()));
    Ok(Document { kind, bytes, instance, references, scale: 0.001, info })
}

/// The message for a file without a model: what it holds instead.
fn not_a_model(object: &DefaultDicomObject, class: &str, mime: &str) -> String {
    let images = object.element_opt(tags::PIXEL_DATA).ok().flatten().is_some();
    let modality = object.element_opt(tags::MODALITY).ok().flatten().and_then(|e| e.to_str().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if images {
        let what = modality.map(|m| format!(" ({m})")).unwrap_or_default();
        return format!("DICOM: this file is an image{what}, not a 3D model; image series (CT, MRI) are not shown, only DICOM files with an embedded STL or OBJ model");
    }
    if !mime.is_empty() {
        return format!("DICOM: this file holds a {mime} document, not a 3D model");
    }
    format!("DICOM: this file (class {class}) holds no 3D model; only DICOM files with an embedded STL or OBJ model are shown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dicom_core::{dicom_value, DataElement, VR};
    use dicom_object::meta::FileMetaTableBuilder;
    use dicom_object::InMemDicomObject;

    /// A DICOM file of `class` with `document` encapsulated (or pixel data, for `None`).
    fn file(class: &str, instance: &str, document: Option<&[u8]>, references: &[&str]) -> Vec<u8> {
        let mut obj = InMemDicomObject::new_empty();
        obj.put(DataElement::new(tags::SOP_CLASS_UID, VR::UI, dicom_value!(Str, class)));
        obj.put(DataElement::new(tags::SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, instance)));
        obj.put(DataElement::new(tags::MODALITY, VR::CS, dicom_value!(Str, if document.is_some() { "M3D" } else { "CT" })));
        match document {
            Some(bytes) => {
                let mut padded = bytes.to_vec();
                if padded.len() % 2 == 1 {
                    padded.push(0);
                }
                obj.put(DataElement::new(tags::DOCUMENT_TITLE, VR::ST, dicom_value!(Str, "Femur")));
                obj.put(DataElement::new(tags::ENCAPSULATED_DOCUMENT_LENGTH, VR::UL, dicom_value!(U32, [bytes.len() as u32])));
                obj.put(DataElement::new(tags::ENCAPSULATED_DOCUMENT, VR::OB, dicom_core::PrimitiveValue::U8(padded.into())));
            }
            None => {
                obj.put(DataElement::new(tags::PIXEL_DATA, VR::OW, dicom_core::PrimitiveValue::U8(vec![0u8; 8].into())));
            }
        }
        if !references.is_empty() {
            let items = references
                .iter()
                .map(|uid| InMemDicomObject::from_element_iter([DataElement::new(tags::REFERENCED_SOP_INSTANCE_UID, VR::UI, dicom_value!(Str, *uid))]))
                .collect::<Vec<_>>();
            obj.put(DataElement::new(tags::REFERENCED_INSTANCE_SEQUENCE, VR::SQ, dicom_core::value::DataSetSequence::from(items)));
        }
        let meta = FileMetaTableBuilder::new().transfer_syntax(uids::EXPLICIT_VR_LITTLE_ENDIAN).media_storage_sop_class_uid(class).media_storage_sop_instance_uid(instance);
        let file = obj.with_meta(meta).unwrap();
        let mut out = Vec::new();
        file.write_all(&mut out).unwrap();
        out
    }

    const STL: &[u8] = b"solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";

    #[test]
    fn hands_back_the_embedded_stl() {
        let doc = read(&file(uids::ENCAPSULATED_STL_STORAGE, "1.2.3.4", Some(STL), &[])).unwrap();
        assert_eq!(doc.kind, Kind::Stl);
        assert_eq!(doc.bytes, STL, "the padding is cut off");
        assert_eq!(doc.scale, 0.001);
        assert!(doc.info.contains(&("Document title".into(), "Femur".into())));
    }

    #[test]
    fn an_obj_names_its_material_library() {
        let doc = read(&file(uids::ENCAPSULATED_OBJ_STORAGE, "1.2.3.4", Some(b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n"), &["9.8.7"])).unwrap();
        assert_eq!(doc.kind, Kind::Obj);
        assert_eq!(doc.references, ["9.8.7"]);
        assert_eq!(doc.instance, "1.2.3.4");
    }

    #[test]
    fn an_image_series_is_refused_with_a_reason() {
        let error = read(&file(uids::CT_IMAGE_STORAGE, "1.2.3.4", None, &[])).unwrap_err();
        assert!(error.contains("image (CT)") && error.contains("not a 3D model"), "{error}");
        assert!(read(b"not dicom").is_err());
    }
}
