//! The images a Revit family draws, such as the logo of a title block.
//!
//! - where an image lies is in the saved graphics of the type: a `GImage` with an
//!   `ImageInfoScaled`: `m_Trf::m_or` its centre (feet), `m_baseVec` the direction of its width,
//!   `m_symbolId` its image type and `m_dWidthScale`/`m_dHeightScale` how much of that type's
//!   size it takes;
//! - the image type, an `ImageSymbol` of the project, gives its size (`m_widthInFeet`,
//!   `m_heightInFeet`) and its `ImageHolder` (`m_idImageHolder`), whose
//!   `ARasterImage::m_compressedImage` is the image file as it was loaded (PNG, JPEG, ...).

use rvt_model::FieldValue;

const FT: f64 = 0.3048;

/// An image of a family, from the family's origin (metres).
pub(super) struct Image {
    pub center: [f64; 2],
    /// Its width and height.
    pub size: [f64; 2],
    /// How it is turned (radians, counterclockwise).
    pub angle: f64,
    /// The image file.
    pub file: Vec<u8>,
}

/// The images the type `kind` draws.
pub(super) fn images(doc: &rvt_read::Document, kind: u64) -> Vec<Image> {
    /// The flags of a piece of graphics that shows at no level of detail.
    const SWITCHED_OFF: i64 = 0xE0;
    let Some(saved) = doc.saved_graphics() else { return Vec::new() };
    let Some(graph) = saved.bodies.get(&kind).and_then(|body| rvt_core::graph::Graph::decode(&saved.schema, body).ok()) else { return Vec::new() };
    let class = |o: &rvt_core::graph::Object| saved.schema.class(o.class).map_or("", |c| c.name.as_str());
    let mut out = Vec::new();
    for o in graph.objects.iter().filter(|o| class(o) == "GImage") {
        if o.get("m_GInfo").and_then(|g| g.as_obj()).and_then(|g| g.get("m_flags")?.as_i64()).is_some_and(|f| f & SWITCHED_OFF == SWITCHED_OFF) {
            continue;
        }
        let Some(info) = o.get("m_imageInfo").and_then(|p| graph.objects.get(p.as_ptr()?)) else { continue };
        let floats = |v: Option<&rvt_core::graph::Value>| v.and_then(|v| v.floats()).filter(|v| v.len() >= 2 && v.iter().all(|x| x.is_finite()));
        let Some(at) = floats(info.get("m_Trf").and_then(|t| t.as_obj()).and_then(|t| t.get("m_or"))) else { continue };
        let Some(symbol) = info.get("m_symbolId").and_then(|s| s.element_id()).and_then(|s| u64::try_from(s).ok()) else { continue };
        let scale = |name: &str| info.get(name).and_then(|s| s.as_f64()).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
        let Some((size, file)) = source(doc, symbol) else { continue };
        let base = floats(info.get("m_baseVec")).unwrap_or(vec![1.0, 0.0]);
        out.push(Image {
            center: [at[0] * FT, at[1] * FT],
            size: [size[0] * scale("m_dWidthScale") * FT, size[1] * scale("m_dHeightScale") * FT],
            angle: base[1].atan2(base[0]),
            file,
        });
    }
    out
}

/// The image a parameter of an element names, or of its type: the image file. None when the
/// parameter names no image there.
pub(super) fn of_parameter(doc: &rvt_read::Document, element: u64, parameter: i64) -> Option<Vec<u8>> {
    let o = doc.object(element)?;
    let named = |o: &rvt_model::ObjectView| {
        let own = o.get("m_pParamValueSetElementId").and_then(|v| o.deref(v)).and_then(|s| s.get("m_paramSet")?.as_list());
        let own = own.unwrap_or(&[]).iter().filter_map(FieldValue::as_object).find(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).and_then(|p| p.get("m_value")?.element());
        // a family type keeps the values of its family's parameters
        let typed = || {
            let all = o.path(&["m_pParams", "m_params"]).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
            all.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).find_map(|p| p.get("m_elemId")?.element())
        };
        own.or_else(typed).and_then(|e| u64::try_from(e).ok())
    };
    let symbol = named(&o).or_else(|| {
        let kind = ["m_masterSymbolId", "m_symbolId"].into_iter().find_map(|f| u64::try_from(o.get(f)?.element()?).ok())?;
        named(&doc.object(kind)?)
    })?;
    Some(source(doc, symbol)?.1)
}

/// The size of an image type (feet) and its image file.
fn source(doc: &rvt_read::Document, symbol: u64) -> Option<([f64; 2], Vec<u8>)> {
    let o = doc.object(symbol)?;
    let side = |name: &str| o.get(name).and_then(FieldValue::as_f64).filter(|s| s.is_finite() && *s > 0.0);
    let size = [side("m_widthInFeet")?, side("m_heightInFeet")?];
    let holder = doc.object(u64::try_from(o.get("m_idImageHolder")?.element()?).ok()?)?;
    let raster = holder.get("m_pRasterImage").and_then(|v| holder.deref(v))?;
    let file: Vec<u8> = raster.get("m_compressedImage")?.as_list()?.iter().filter_map(|b| u8::try_from(b.as_i64()?).ok()).collect();
    (!file.is_empty()).then(|| (size, file))
}

#[cfg(test)]
mod tests {
    /// A project whose title blocks hold an image (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn a_title_block_gives_its_image() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with an image in a title block");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let blocks = rvt_export::sheets::sheets(&doc).into_iter().filter_map(|s| s.title_block?.symbol);
        let images: Vec<_> = blocks.flat_map(|kind| super::images(&doc, kind)).collect();
        for i in &images {
            eprintln!("image at {:?}, {:?} m, {} bytes, starts {:02x?}", i.center, i.size, i.file.len(), &i.file[..i.file.len().min(8)]);
        }
        assert!(images.iter().any(|i| i.size[0] > 0.0 && i.file.len() > 100), "no image in any title block");
    }
}
