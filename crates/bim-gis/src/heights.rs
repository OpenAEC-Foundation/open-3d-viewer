//! The heights of the ground from the national height model of the Netherlands (AHN), through
//! the open coverage service of PDOK: what to ask and how to read the answer.
//!
//! The model is a grid of ground heights (m above NAP) on the Dutch national grid, measured
//! from the air, half a metre fine. Under buildings and over water it has no height. The
//! service gives a square of it at the size asked for, as a GeoTIFF.
//!
//! No network here: the caller sends the request.

use crate::pdok::Bounds;
use crate::rd::Rd;

const COVERAGE: &str = "https://service.pdok.nl/rws/ahn/wcs/v1_0";
/// The side of a grid the service is asked for, at most (cells).
pub const MOST: usize = 2000;
/// Whose data it is, to say with the data.
pub const SOURCE: &str = "Actueel Hoogtebestand Nederland, ground level (PDOK)";

/// The address of the ground heights in `bounds`, as a grid of `cols` by `rows` cells.
pub fn coverage_url(bounds: Bounds, cols: usize, rows: usize) -> String {
    format!(
        "{COVERAGE}?SERVICE=WCS&VERSION=2.0.1&REQUEST=GetCoverage&COVERAGEID=dtm_05m&FORMAT=image/tiff&SUBSET=x({:.2},{:.2})&SUBSET=y({:.2},{:.2})&SCALESIZE=x({}),y({})",
        bounds.min.x,
        bounds.max.x,
        bounds.min.y,
        bounds.max.y,
        cols.clamp(2, MOST),
        rows.clamp(2, MOST)
    )
}

/// The cells a square of `side` metres is asked in, so that it has about `most` cells in all
/// and none finer than the model itself.
pub fn cells(side: f64, most: usize) -> usize {
    let finest = (side / 0.5).round() as usize;
    finest.min((most as f64).sqrt() as usize).clamp(2, MOST)
}

/// A grid of heights on the national grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// The west and north edge of the grid.
    pub west: f64,
    pub north: f64,
    /// The size of a cell, east and south (m).
    pub cell: [f64; 2],
    pub cols: usize,
    pub rows: usize,
    /// The height of each cell (m above NAP), row after row from the north; not a number
    /// where the model has none.
    pub heights: Vec<f32>,
}

impl Grid {
    /// The middle of a cell.
    pub fn point(&self, col: usize, row: usize) -> Rd {
        Rd::new(self.west + (col as f64 + 0.5) * self.cell[0], self.north - (row as f64 + 0.5) * self.cell[1])
    }

    /// How many cells have a height.
    pub fn known(&self) -> usize {
        self.heights.iter().filter(|z| z.is_finite()).count()
    }
}

/// The grid in the answer of the service.
pub fn parse(tiff: &[u8]) -> Result<Grid, String> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let wrong = |e: tiff::TiffError| format!("the height service answered something else than a height grid: {e}");
    let mut decoder = Decoder::new(std::io::Cursor::new(tiff)).map_err(wrong)?;
    let (cols, rows) = decoder.dimensions().map_err(wrong)?;
    let scale = decoder.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(wrong)?;
    let tie = decoder.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(wrong)?;
    let (&[cell_x, cell_y, ..], &[i, j, _, x, y, ..]) = (&scale[..], &tie[..]) else {
        return Err("the height grid does not say where it lies".into());
    };
    if !(cell_x > 0.0 && cell_y > 0.0) {
        return Err("the height grid has no cell size".into());
    }
    let none = decoder.get_tag_ascii_string(Tag::GdalNodata).ok().and_then(|text| text.trim().trim_end_matches('\0').parse::<f64>().ok());
    let heights: Vec<f32> = match decoder.read_image().map_err(wrong)? {
        DecodingResult::F32(values) => values,
        DecodingResult::F64(values) => values.into_iter().map(|v| v as f32).collect(),
        _ => return Err("the height grid does not hold heights as decimal numbers".into()),
    };
    if heights.len() != cols as usize * rows as usize {
        return Err("the height grid has more than one value a cell".into());
    }
    // no height: the value the grid names for it, and anything far outside the country's heights
    let missing = |z: f32| !z.is_finite() || none.is_some_and(|none| f64::from(z) == none || z == none as f32) || !(-100.0..=1000.0).contains(&z);
    let heights = heights.into_iter().map(|z| if missing(z) { f32::NAN } else { z }).collect();
    Ok(Grid { west: x - i * cell_x, north: y + j * cell_y, cell: [cell_x, cell_y], cols: cols as usize, rows: rows as usize, heights })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 40 by 40 m of the ground at the town hall square of Amersfoort, in cells of 2 m, as the
    /// service gave it.
    const SAMPLE: &[u8] = include_bytes!("../fixtures/ahn-dtm-20x20.tif");

    #[test]
    fn the_ground_is_asked_as_a_grid_of_the_size_wanted() {
        let bounds = Bounds::around(Rd::new(154_792.0, 463_172.0), 20.0);
        assert_eq!(
            coverage_url(bounds, 20, 20),
            "https://service.pdok.nl/rws/ahn/wcs/v1_0?SERVICE=WCS&VERSION=2.0.1&REQUEST=GetCoverage&COVERAGEID=dtm_05m&FORMAT=image/tiff&SUBSET=x(154772.00,154812.00)&SUBSET=y(463152.00,463192.00)&SCALESIZE=x(20),y(20)"
        );
        // never more cells than the service gives at once
        assert!(coverage_url(bounds, 9000, 1, ).ends_with("SCALESIZE=x(2000),y(2)"));
    }

    #[test]
    fn a_square_is_asked_no_finer_than_the_model_and_no_larger_than_wanted() {
        // 200 m at half a metre is 400 cells a side; 1000 m is held to 500 a side
        assert_eq!(cells(200.0, 250_000), 400);
        assert_eq!(cells(500.0, 250_000), 500);
        assert_eq!(cells(1000.0, 250_000), 500);
        assert_eq!(cells(0.2, 250_000), 2);
    }

    #[test]
    fn the_answer_is_a_grid_of_heights_above_nap_with_gaps_under_buildings() {
        let grid = parse(SAMPLE).unwrap();
        assert_eq!((grid.cols, grid.rows, grid.cell), (20, 20, [2.0, 2.0]));
        assert_eq!((grid.west, grid.north), (154_772.0, 463_192.0));
        assert_eq!(grid.point(0, 0), Rd::new(154_773.0, 463_191.0));
        assert_eq!(grid.point(19, 19), Rd::new(154_811.0, 463_153.0));
        // the square lies a few metres above NAP; part of it is under a building
        let known: Vec<f32> = grid.heights.iter().copied().filter(|z| z.is_finite()).collect();
        assert_eq!(known.len(), grid.known());
        assert!(known.len() > 100 && known.len() < 400, "{}", known.len());
        let (low, high) = known.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), z| (low.min(*z), high.max(*z)));
        assert!(low > 0.0 && high < 10.0 && high - low < 5.0, "{low} {high}");
    }

    #[test]
    fn an_answer_that_is_no_grid_says_so() {
        assert!(parse(b"<ServiceExceptionReport>").unwrap_err().contains("height grid"));
        assert!(parse(&[]).is_err());
    }
}
