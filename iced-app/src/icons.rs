//! Ribbon icons (24x24 line icons in the style of the React app's ribbon), tinted at draw time.

use iced::widget::svg;

macro_rules! icon {
    ($body:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            "</svg>"
        )
    };
}

pub const OPEN: &str = icon!(r#"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 11v6M9 14l3 3 3-3"/>"#);
pub const SAVE_AS: &str = icon!(r#"<path d="M5 3h10l4 4v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M7 3v5h7V3"/><rect x="7" y="13" width="10" height="8" rx="1"/>"#);
pub const SOLID: &str = icon!(r#"<path d="M12 2L2 7l10 5 10-5-10-5z" fill="black" fill-opacity="0.3"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>"#);
pub const WIREFRAME: &str = icon!(r#"<path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>"#);
pub const EDGES: &str = icon!(r#"<path d="M4 8L12 4L20 8L12 12Z" fill="black" fill-opacity="0.25"/><path d="M4 8L12 12L20 8"/><path d="M12 12V20"/><path d="M4 8V16L12 20L20 16V8"/>"#);
pub const GRID: &str = icon!(r#"<rect x="3" y="3" width="18" height="18" rx="1"/><path d="M3 9h18M3 15h18M9 3v18M15 3v18"/>"#);
pub const UP_AXIS: &str = icon!(r#"<path d="M12 20V4M8 8l4-4 4 4"/><path d="M4 20h16"/>"#);
pub const FIT: &str = icon!(r#"<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/><rect x="8" y="8" width="8" height="8" rx="1"/>"#);
pub const VIEW_TOP: &str = icon!(r#"<path d="M12 3l9 4.5-9 4.5-9-4.5z" fill="black" fill-opacity="0.35"/><path d="M3 7.5v9l9 4.5 9-4.5v-9"/><path d="M12 12v9"/>"#);
pub const VIEW_FRONT: &str = icon!(r#"<path d="M3 7.5l9-4.5 9 4.5"/><path d="M3 7.5v9l9 4.5v-9z" fill="black" fill-opacity="0.35"/><path d="M21 7.5v9l-9 4.5"/><path d="M3 7.5l9 4.5 9-4.5"/>"#);
pub const VIEW_RIGHT: &str = icon!(r#"<path d="M3 7.5l9-4.5 9 4.5"/><path d="M21 7.5v9l-9 4.5v-9z" fill="black" fill-opacity="0.35"/><path d="M3 7.5v9l9 4.5"/><path d="M3 7.5l9 4.5"/>"#);
pub const VIEW_ISO: &str = icon!(r#"<path d="M12 3l9 4.5v9L12 21l-9-4.5v-9z"/><path d="M12 12l9-4.5M12 12v9M12 12L3 7.5"/>"#);
pub const LAYERS: &str = icon!(r#"<path d="M12 3L3 8l9 5 9-5-9-5z"/><path d="M3 13l9 5 9-5"/>"#);
pub const INFO: &str = icon!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>"#);
pub const EYE: &str = icon!(r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>"#);
pub const EYE_OFF: &str = icon!(r#"<path d="M3 3l18 18"/><path d="M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4.2M6.6 6.6C3.8 8.4 2 12 2 12s3.5 7 10 7a9.6 9.6 0 0 0 5.4-1.6"/>"#);

/// A cached SVG handle per icon source.
pub fn handle(source: &'static str) -> svg::Handle {
    svg::Handle::from_memory(source.as_bytes())
}
pub const PERSPECTIVE: &str = icon!(r#"<path d="M6 4L18 4L21 20H3L6 4Z"/><path d="M12 4v16" stroke-dasharray="3 2"/>"#);
pub const ORTHO: &str = icon!(r#"<rect x="4" y="4" width="16" height="16"/><path d="M12 4v16" stroke-dasharray="3 2"/>"#);
