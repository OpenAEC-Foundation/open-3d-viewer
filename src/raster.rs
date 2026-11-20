//! An embedded drawing image and the rectangle it occupies, in drawing coordinates.
use base64::{engine::general_purpose::STANDARD, Engine};
use iced::{
    widget::{canvas, image::Handle},
    Point, Rectangle, Size,
};
use image::ImageFormat;
use serde_json::{json, Value};
use std::{io::Cursor, sync::Arc};

#[derive(Clone, Debug)]
pub struct PlacedImage {
    pub bounds: [[f64; 2]; 2],
    pub clip: [[f64; 2]; 2],
    pub(crate) bytes: Arc<[u8]>,
    pub(crate) format: ImageFormat,
    pub(crate) pixels: [u32; 2],
    tiles: Arc<Vec<([u32; 4], Handle)>>,
}

impl PartialEq for PlacedImage {
    fn eq(&self, rhs: &Self) -> bool {
        self.bounds == rhs.bounds && self.clip == rhs.clip && self.bytes == rhs.bytes
    }
}

impl PlacedImage {
    pub fn new(bytes: Vec<u8>, bounds: [[f64; 2]; 2]) -> Result<Self, String> {
        if !valid(bounds) || bytes.len() > 32 * 1024 * 1024 {
            return Err("Invalid drawing image bounds or byte count".into());
        }
        let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
            return Err("Drawing images must be JPEG or PNG".into());
        }
        let (w, h) = image::ImageReader::with_format(Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 32_000_000 {
            return Err("Invalid drawing image dimensions".into());
        }
        let decoded =
            image::load_from_memory_with_format(&bytes, format).map_err(|e| e.to_string())?;
        if format == ImageFormat::Jpeg && decoded.color() != image::ColorType::Rgb8 {
            return Err("A drawing JPEG must contain RGB colour".into());
        }
        // Individual canvas uploads fit in the synchronous texture budget, including
        // in the renderer used for a single printed-sheet preview frame.
        let pixels = decoded.into_rgba8();
        let mut tiles = Vec::new();
        for y in (0..h).step_by(512) {
            for x in (0..w).step_by(512) {
                let tw = (w - x).min(512);
                let th = (h - y).min(512);
                let tile = image::imageops::crop_imm(&pixels, x, y, tw, th).to_image();
                tiles.push(([x, y, tw, th], Handle::from_rgba(tw, th, tile.into_raw())));
            }
        }
        Ok(Self {
            bounds,
            clip: bounds,
            bytes: bytes.into(),
            format,
            pixels: [w, h],
            tiles: Arc::new(tiles),
        })
    }

    pub(crate) fn placed(
        &self,
        crop: Option<([f64; 2], [f64; 2])>,
        to: impl Fn([f64; 2]) -> [f64; 2],
    ) -> Option<Self> {
        let [mut lo, mut hi] = self.clip;
        if let Some((a, b)) = crop {
            for k in 0..2 {
                lo[k] = lo[k].max(a[k]);
                hi[k] = hi[k].min(b[k]);
            }
        }
        valid([lo, hi]).then(|| Self {
            bounds: self.bounds.map(&to),
            clip: [to(lo), to(hi)],
            ..self.clone()
        })
    }

    pub(crate) fn draw(&self, frame: &mut canvas::Frame, on: impl Fn([f64; 2]) -> Option<Point>) {
        let rect = |[lo, hi]: [[f64; 2]; 2]| {
            let a = on([lo[0], hi[1]])?;
            let b = on([hi[0], lo[1]])?;
            (b.x > a.x && b.y > a.y).then(|| Rectangle::new(a, Size::new(b.x - a.x, b.y - a.y)))
        };
        if let (Some(bounds), Some(clip)) = (rect(self.bounds), rect(self.clip)) {
            frame.with_clip(clip, |f| {
                let dx = bounds.width / self.pixels[0] as f32;
                let dy = bounds.height / self.pixels[1] as f32;
                for &([x, y, w, h], ref handle) in self.tiles.iter() {
                    let tile = Rectangle::new(Point::new(bounds.x + x as f32 * dx, bounds.y + y as f32 * dy), Size::new(w as f32 * dx, h as f32 * dy));
                    if tile.intersects(&clip) { f.draw_image(tile, canvas::Image::new(handle.clone())); }
                }
            });
        }
    }

    pub(crate) fn encoded(&self) -> String {
        STANDARD.encode(&self.bytes)
    }
}

fn valid([lo, hi]: [[f64; 2]; 2]) -> bool {
    (0..2).all(|k| lo[k].is_finite() && hi[k].is_finite() && hi[k] > lo[k])
}

/// Registration of a north-up image relative to the drawing origin, in RD New metres.
#[derive(Clone, Debug, PartialEq)]
pub struct OrthoPhoto {
    pub image: PlacedImage,
    pub rd_origin: [f64; 2],
    pub source: String,
}

impl OrthoPhoto {
    pub(crate) fn read(v: &Value) -> Option<Self> {
        if v.get("crs")?.as_str()? != "EPSG:28992" || v.get("rd_unit")?.as_str()? != "metre" {
            return None;
        }
        let rd_origin: [f64; 2] = serde_json::from_value(v.get("rd_origin")?.clone()).ok()?;
        if !bim_gis::rd::Rd::new(rd_origin[0], rd_origin[1]).valid() {
            return None;
        }
        let encoded = v.get("image")?.as_str()?;
        if encoded.len() > 44 * 1024 * 1024 {
            return None;
        }
        let bounds = serde_json::from_value(v.get("bounds")?.clone()).ok()?;
        let image = PlacedImage::new(STANDARD.decode(encoded).ok()?, bounds).ok()?;
        Some(Self {
            image,
            rd_origin,
            source: v.get("source")?.as_str()?.to_owned(),
        })
    }

    pub(crate) fn write(&self, v: &mut Value) {
        for (k, value) in [
            ("image", json!(self.image.encoded())),
            ("bounds", json!(self.image.bounds)),
            ("crs", json!("EPSG:28992")),
            ("rd_origin", json!(self.rd_origin)),
            ("rd_unit", json!("metre")),
            ("source", json!(self.source)),
        ] {
            v[k] = value;
        }
    }
}

/// A separate canvas layer keeps vector annotations above the image batch.
pub(crate) struct Backdrop<F>(pub F);

impl<Message, F: Fn(&mut canvas::Frame, Rectangle)> canvas::Program<Message> for Backdrop<F> {
    type State = ();

    fn draw(&self, _: &(), renderer: &iced::Renderer, _: &iced::Theme, bounds: Rectangle, _: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        (self.0)(&mut frame, bounds);
        vec![frame.into_geometry()]
    }
}
