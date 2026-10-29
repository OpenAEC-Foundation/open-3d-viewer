//! The B2 icons of OpenAEC UI 0.4 that the viewer shows, byte for byte the masters of the
//! Blueprint's `openaec-icons` crate (release 2026.44.1). That crate has no feature for the viewer
//! yet, so the viewer keeps the ones it uses in `assets/b2/{16,24}`; `scripts/b2_icons.py` copies
//! them from a Blueprint checkout and checks the copies.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

/// The two masters of every icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    S16,
    S24,
}

macro_rules! icons {
    ($($id:literal),* $(,)?) => {
        /// Every icon id in this list.
        #[allow(dead_code)]
        pub const IDS: &[&str] = &[$($id),*];

        fn svgz(id: &str, size: Size) -> Option<&'static [u8]> {
            Some(match (id, size) {
                $(
                    ($id, Size::S16) => include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/b2/16/", $id, ".svgz")),
                    ($id, Size::S24) => include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/b2/24/", $id, ".svgz")),
                )*
                _ => return None,
            })
        }
    };
}

icons! {
    "window.minimize", "window.maximize", "window.restore", "window.close",
    "file.open", "file.import", "file.save-as", "file.examples",
    "ui.settings", "ui.about", "ui.close", "ui.help",
    "panel.left", "panel.right", "panel.schedules", "panel.model-tree", "panel.measurements",
    "view.fit", "view.fit-selection", "view.isometric", "view.top", "view.front", "view.right",
    "view.orbit", "view.zoom-in", "view.zoom-out", "view.grid", "view.saved", "view.reset-camera",
    "mesh.surface", "mesh.faces", "mesh.closed",
    "section.box", "section.plane-z", "section.fit-selection", "section.reset-box", "section.clear",
    "transform.mirror", "transform.flip-vertical", "transform.explode",
    "measure.distance", "measure.angle", "measure.area", "measure.perimeter", "measure.takeoff",
    "edit.clear-all",
    "status.success", "status.error", "status.warning", "status.info",
    "object.show", "object.hide", "project.folder", "data.table",
}

/// The SVG text of an icon, unpacked once. Panics for an id that is not in the list, like
/// `openaec_icons::Icon::from_id(..).unwrap()` does in the Blueprint.
pub fn svg(id: &str, size: Size) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<(String, Size), &'static str>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(svg) = cache.get(&(id.to_string(), size)) {
        return svg;
    }
    let packed = svgz(id, size).unwrap_or_else(|| panic!("B2 icon {id} is not in src/b2.rs"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(packed).read_to_string(&mut text).expect("a B2 master is gzip-compressed UTF-8");
    let text: &'static str = Box::leak(text.into_boxed_str());
    cache.insert((id.to_string(), size), text);
    text
}

/// The 16 px master, for buttons, rows and the title bar.
pub fn small(id: &str) -> &'static str {
    svg(id, Size::S16)
}

/// The 24 px master, for the large ribbon buttons and empty states.
pub fn large(id: &str) -> &'static str {
    svg(id, Size::S24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_unpacks_in_both_sizes() {
        for id in IDS {
            for size in [Size::S16, Size::S24] {
                assert!(svg(id, size).contains("<svg"), "{id} {size:?}");
            }
        }
    }
}
