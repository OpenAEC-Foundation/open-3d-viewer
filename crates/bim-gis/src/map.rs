//! Maps as pictures: what to ask a map service for a picture of a square of the grid, and the
//! colour of a place in the picture that comes back.
//!
//! Two kinds of service:
//!
//! - a web map service (WMS) draws a picture of any square on request. The aerial photograph of
//!   the Netherlands is one, and any other can be named by its address and layer;
//! - a tile service (WMTS) keeps ready-made tiles. The background map of the Netherlands is one,
//!   in the tile grid of the national grid.
//!
//! A request tells the service which square is looked at; nothing else goes along. The caller
//! sends it and decodes the picture: no network and no picture formats here.

use crate::pdok::Bounds;
use crate::rd::Rd;

/// A web map service and the layer asked of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wms {
    /// The address of the service, without a question.
    pub url: String,
    pub layer: String,
}

impl Wms {
    /// The aerial photograph of the Netherlands, the newest at its finest (PDOK, CC BY 4.0).
    pub fn aerial() -> Self {
        Wms { url: "https://service.pdok.nl/hwh/luchtfotorgb/wms/v1_0".into(), layer: "Actueel_orthoHR".into() }
    }

    /// A service as typed: what follows a question mark is left off, so an address copied with a
    /// request behind it works.
    pub fn typed(url: &str, layer: &str) -> Option<Self> {
        let url = url.trim().split('?').next().unwrap_or_default().trim_end_matches('/');
        let layer = layer.trim();
        ((url.starts_with("https://") || url.starts_with("http://")) && !layer.is_empty()).then(|| Wms { url: url.to_string(), layer: layer.to_string() })
    }

    /// The address of a picture of `bounds`, `width` by `height` pixels, in the national grid.
    pub fn picture_url(&self, bounds: Bounds, width: usize, height: usize) -> String {
        let layer: String = self.layer.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.:,".contains(c) { c.to_string() } else { format!("%{:02X}", c as u32 & 0xff) }).collect();
        format!(
            "{}?service=WMS&version=1.3.0&request=GetMap&layers={layer}&styles=&crs=EPSG:28992&bbox={},{},{},{}&width={width}&height={height}&format=image/png",
            self.url, bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y
        )
    }
}

/// The side of a tile (pixels).
pub const TILE: usize = 256;
/// The west and north edge of the tile grid of the national grid, and the metres a pixel of its
/// widest zoom covers: the Dutch tiling scheme for EPSG:28992.
const WEST: f64 = -285_401.92;
const NORTH: f64 = 903_401.92;
const WIDEST: f64 = 3440.64;
/// The deepest zoom the background map has.
pub const DEEPEST: u32 = 14;
const BACKGROUND: &str = "https://service.pdok.nl/brt/achtergrondkaart/wmts/v2_0/standaard/EPSG:28992";
/// Where the background map comes from, for the properties of what is made of it.
pub const BACKGROUND_SOURCE: &str = "BRT Achtergrondkaart (Kadaster, PDOK, CC BY 4.0)";
/// Where the aerial photograph comes from.
pub const AERIAL_SOURCE: &str = "Luchtfoto Actueel Ortho HR (Beeldmateriaal Nederland, PDOK, CC BY 4.0)";

/// The tiles of the background map over a square, at one zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tiles {
    pub zoom: u32,
    /// The first and last column, west to east.
    pub cols: (u32, u32),
    /// The first and last row, north to south.
    pub rows: (u32, u32),
}

impl Tiles {
    /// The metres a pixel covers at a zoom.
    pub fn pixel(zoom: u32) -> f64 {
        WIDEST / f64::from(1u32 << zoom)
    }

    /// The zoom whose pixels are as fine as `metres` a pixel, or the deepest there is.
    pub fn zoom_for(metres: f64) -> u32 {
        (0..=DEEPEST).find(|zoom| Self::pixel(*zoom) <= metres).unwrap_or(DEEPEST)
    }

    /// The tiles that cover `bounds` at `zoom`.
    pub fn over(bounds: Bounds, zoom: u32) -> Self {
        let side = Self::pixel(zoom) * TILE as f64;
        let last = f64::from((1u32 << zoom) - 1);
        let col = |x: f64| ((x - WEST) / side).floor().clamp(0.0, last) as u32;
        let row = |y: f64| ((NORTH - y) / side).floor().clamp(0.0, last) as u32;
        Tiles { zoom, cols: (col(bounds.min.x), col(bounds.max.x)), rows: (row(bounds.max.y), row(bounds.min.y)) }
    }

    /// How many tiles wide and high.
    pub fn size(&self) -> (usize, usize) {
        ((self.cols.1 - self.cols.0 + 1) as usize, (self.rows.1 - self.rows.0 + 1) as usize)
    }

    /// The square the tiles cover together.
    pub fn bounds(&self) -> Bounds {
        let side = Self::pixel(self.zoom) * TILE as f64;
        Bounds {
            min: Rd::new(WEST + f64::from(self.cols.0) * side, NORTH - f64::from(self.rows.1 + 1) * side),
            max: Rd::new(WEST + f64::from(self.cols.1 + 1) * side, NORTH - f64::from(self.rows.0) * side),
        }
    }

    /// The address of a tile of the background map.
    pub fn url(&self, col: u32, row: u32) -> String {
        format!("{BACKGROUND}/{:02}/{col}/{row}.png", self.zoom)
    }
}

/// A picture of a square of the grid: rows from the north, each from the west.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub bounds: Bounds,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 3]>,
}

impl Picture {
    /// The picture, when the pixels are as many as its size says and it has a size.
    pub fn new(bounds: Bounds, width: usize, height: usize, pixels: Vec<[u8; 3]>) -> Option<Self> {
        (width > 0 && height > 0 && pixels.len() == width * height && bounds.max.x > bounds.min.x && bounds.max.y > bounds.min.y).then_some(Picture { bounds, width, height, pixels })
    }

    /// The metres a pixel covers, east and north.
    pub fn pixel(&self) -> [f64; 2] {
        [(self.bounds.max.x - self.bounds.min.x) / self.width as f64, (self.bounds.max.y - self.bounds.min.y) / self.height as f64]
    }

    /// The colour at a place, between the four pixels around it; outside the picture, of its edge.
    pub fn colour(&self, place: Rd) -> [u8; 3] {
        let [px, py] = self.pixel();
        // in pixels, measured from the middle of the north-west pixel
        let x = ((place.x - self.bounds.min.x) / px - 0.5).clamp(0.0, (self.width - 1) as f64);
        let y = ((self.bounds.max.y - place.y) / py - 0.5).clamp(0.0, (self.height - 1) as f64);
        let (c, r) = (x.floor() as usize, y.floor() as usize);
        let (c1, r1) = ((c + 1).min(self.width - 1), (r + 1).min(self.height - 1));
        let (fx, fy) = (x - c as f64, y - r as f64);
        let at = |c: usize, r: usize| self.pixels[r * self.width + c];
        std::array::from_fn(|k| {
            let top = f64::from(at(c, r)[k]) * (1.0 - fx) + f64::from(at(c1, r)[k]) * fx;
            let bottom = f64::from(at(c, r1)[k]) * (1.0 - fx) + f64::from(at(c1, r1)[k]) * fx;
            (top * (1.0 - fy) + bottom * fy).round() as u8
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Bounds {
        Bounds::around(Rd::new(155_000.0, 463_000.0), 100.0)
    }

    #[test]
    fn a_web_map_service_is_asked_for_a_square_in_the_national_grid() {
        let url = Wms::aerial().picture_url(square(), 512, 256);
        assert!(url.starts_with("https://service.pdok.nl/hwh/luchtfotorgb/wms/v1_0?service=WMS"), "{url}");
        assert!(url.contains("layers=Actueel_orthoHR") && url.contains("crs=EPSG:28992"), "{url}");
        assert!(url.contains("bbox=154900,462900,155100,463100") && url.contains("width=512&height=256"), "{url}");
    }

    #[test]
    fn a_typed_service_loses_its_question_and_needs_a_layer() {
        let wms = Wms::typed(" https://example.org/wms?service=WMS&request=GetCapabilities ", " roads ").unwrap();
        assert_eq!((wms.url.as_str(), wms.layer.as_str()), ("https://example.org/wms", "roads"));
        assert_eq!(Wms::typed("https://example.org/wms", " "), None);
        assert_eq!(Wms::typed("example.org/wms", "roads"), None);
        // a layer name with a space is written so an address takes it
        assert!(Wms::typed("https://example.org/wms", "my roads").unwrap().picture_url(square(), 8, 8).contains("layers=my%20roads"));
    }

    #[test]
    fn the_tile_grid_halves_its_pixels_with_every_zoom() {
        assert_eq!(Tiles::pixel(0), 3440.64);
        assert!((Tiles::pixel(12) - 0.84).abs() < 1e-9 && (Tiles::pixel(14) - 0.21).abs() < 1e-9);
        // the zoom as fine as asked, or the deepest
        assert_eq!(Tiles::zoom_for(1.0), 12);
        assert_eq!(Tiles::zoom_for(0.84), 12);
        assert_eq!(Tiles::zoom_for(0.01), DEEPEST);
    }

    #[test]
    fn the_tiles_over_a_square_cover_it() {
        let bounds = square();
        let tiles = Tiles::over(bounds, 12);
        let covered = tiles.bounds();
        assert!(covered.min.x <= bounds.min.x && covered.min.y <= bounds.min.y && covered.max.x >= bounds.max.x && covered.max.y >= bounds.max.y);
        // a tile of zoom 12 is 215.04 m: a square of 200 m lies in one to two tiles each way
        let (wide, high) = tiles.size();
        assert!((1..=2).contains(&wide) && (1..=2).contains(&high));
        assert!((covered.max.x - covered.min.x - wide as f64 * 215.04).abs() < 1e-6);
        // the tile grid is centred on the centre of the national grid: at zoom 1 its four tiles meet there
        assert_eq!(Tiles::over(Bounds::around(Rd::new(155_000.0, 463_000.0), 1.0), 1), Tiles { zoom: 1, cols: (0, 1), rows: (0, 1) });
        assert_eq!(tiles.url(tiles.cols.0, tiles.rows.0), format!("{BACKGROUND}/12/{}/{}.png", tiles.cols.0, tiles.rows.0));
    }

    #[test]
    fn a_picture_gives_the_colour_of_a_place() {
        // two by two pixels over 2 m: red and green in the north, blue and white in the south
        let bounds = Bounds { min: Rd::new(0.0, 0.0), max: Rd::new(2.0, 2.0) };
        let picture = Picture::new(bounds, 2, 2, vec![[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]]).unwrap();
        assert_eq!(picture.pixel(), [1.0, 1.0]);
        assert_eq!(picture.colour(Rd::new(0.5, 1.5)), [255, 0, 0]);
        assert_eq!(picture.colour(Rd::new(1.5, 0.5)), [255, 255, 255]);
        // between the two northern pixels: half of each
        assert_eq!(picture.colour(Rd::new(1.0, 1.5)), [128, 128, 0]);
        // outside: the edge
        assert_eq!(picture.colour(Rd::new(-5.0, 9.0)), [255, 0, 0]);
        assert_eq!(Picture::new(bounds, 2, 2, vec![[0; 3]; 3]), None);
    }
}
