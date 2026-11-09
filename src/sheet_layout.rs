//! Laying views out on sheets. A sheet's views get one scale, the largest drawing of the scales
//! a view can have at which they all fit, and stand in rows from the top, each row and the rows
//! together centred in the part of the paper views go on; a view stands on the bottom of its
//! row, so the names under them line up. Views that are on no sheet yet fill new sheets: the
//! scale at which each of them fits on a sheet of its own, then as many in their order as fit
//! on a sheet before the next one starts.
//!
//! Sizes are paper metres; a view's drawing is given in model metres around the point its place
//! on the paper stands for (the middle of its whole cut).

/// Space between two views side by side or above each other (paper metres).
pub const GAP: f64 = 0.012;
/// Space around a view's drawing for its frame and dimension lines (paper metres).
pub const PAD: f64 = 0.010;
/// Space under a view for its name and scale (paper metres).
pub const LABEL: f64 = 0.010;
/// The band at the bottom of the frame kept for the sheet's number and name, which stand at
/// its lower right (paper metres).
pub const TITLE: f64 = 0.014;

/// A view's drawing: its box in model metres around the point its place stands for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item {
    pub lo: [f64; 2],
    pub hi: [f64; 2],
}

impl Item {
    /// The room it takes on the paper at 1 : `scale`, with its frame and name.
    fn size(&self, scale: f64) -> [f64; 2] {
        [(self.hi[0] - self.lo[0]) / scale + 2.0 * PAD, (self.hi[1] - self.lo[1]) / scale + 2.0 * PAD + LABEL]
    }

    /// Where its place is when its room's lower left corner is at `corner`.
    fn place(&self, scale: f64, corner: [f64; 2]) -> [f64; 2] {
        [corner[0] + PAD - self.lo[0] / scale, corner[1] + PAD + LABEL - self.lo[1] / scale]
    }
}

/// The part of a paper views go on: inside its frame (20 mm on the binding side, 10 mm
/// elsewhere) with 5 mm to spare, above the title band.
pub fn area(paper: [f64; 2]) -> ([f64; 2], [f64; 2]) {
    ([0.025, 0.015 + TITLE], [paper[0] - 0.015, paper[1] - 0.015])
}

/// The places of the items at 1 : `scale` within the area, or `None` when they do not fit.
pub fn fit(items: &[Item], scale: f64, (lo, hi): ([f64; 2], [f64; 2])) -> Option<Vec<[f64; 2]>> {
    let (width, height) = (hi[0] - lo[0], hi[1] - lo[1]);
    let sizes: Vec<[f64; 2]> = items.iter().map(|i| i.size(scale)).collect();
    if sizes.iter().any(|s| s[0] > width + 1e-9) {
        return None;
    }
    // rows, in the items' order: a row ends where the next would stick out
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = f64::INFINITY;
    for (k, s) in sizes.iter().enumerate() {
        match rows.last_mut() {
            Some(row) if used + GAP + s[0] <= width + 1e-9 => {
                row.push(k);
                used += GAP + s[0];
            }
            _ => {
                rows.push(vec![k]);
                used = s[0];
            }
        }
    }
    let row_height = |row: &[usize]| row.iter().map(|&k| sizes[k][1]).fold(0.0, f64::max);
    let total = rows.iter().map(|r| row_height(r)).sum::<f64>() + GAP * rows.len().saturating_sub(1) as f64;
    if total > height + 1e-9 {
        return None;
    }
    let mut places = vec![[0.0; 2]; items.len()];
    let mut top = hi[1] - (height - total) / 2.0;
    for row in &rows {
        let h = row_height(row);
        let w = row.iter().map(|&k| sizes[k][0]).sum::<f64>() + GAP * (row.len() - 1) as f64;
        let mut x = lo[0] + (width - w) / 2.0;
        for &k in row {
            places[k] = items[k].place(scale, [x, top - h]);
            x += sizes[k][0] + GAP;
        }
        top -= h + GAP;
    }
    Some(places)
}

/// One scale for all the items (the first of `scales`, largest drawing first, at which they
/// fit) and their places; when none fits, the last scale, its places as far as they go.
pub fn arrange(items: &[Item], area: ([f64; 2], [f64; 2]), scales: &[f64]) -> (f64, Vec<[f64; 2]>) {
    for &scale in scales {
        if let Some(places) = fit(items, scale, area) {
            return (scale, places);
        }
    }
    let scale = scales.last().copied().unwrap_or(100.0);
    (scale, squeeze(items, scale, area))
}

/// Places at `scale` when they do not fit: rows as wide as the area, from its top down.
fn squeeze(items: &[Item], scale: f64, (lo, hi): ([f64; 2], [f64; 2])) -> Vec<[f64; 2]> {
    let mut places = Vec::with_capacity(items.len());
    let (mut x, mut top, mut row) = (lo[0], hi[1], 0.0_f64);
    for item in items {
        let s = item.size(scale);
        if x > lo[0] && x + s[0] > hi[0] {
            x = lo[0];
            top -= row + GAP;
            row = 0.0;
        }
        places.push(item.place(scale, [x, top - s[1]]));
        x += s[0] + GAP;
        row = row.max(s[1]);
    }
    places
}

/// New sheets for the items: the scale at which each fits on a sheet of its own (the last of
/// `scales` when one does not), then per sheet as many items in their order as fit, with
/// their places.
pub fn fill(items: &[Item], area: ([f64; 2], [f64; 2]), scales: &[f64]) -> (f64, Vec<Vec<(usize, [f64; 2])>>) {
    let scale = scales.iter().copied().find(|&s| items.iter().all(|i| fit(std::slice::from_ref(i), s, area).is_some())).or(scales.last().copied()).unwrap_or(100.0);
    let mut sheets = Vec::new();
    let mut start = 0;
    while start < items.len() {
        // the most that fit from here, at least one
        let mut end = start + 1;
        while end < items.len() && fit(&items[start..=end], scale, area).is_some() {
            end += 1;
        }
        let places = fit(&items[start..end], scale, area).unwrap_or_else(|| squeeze(&items[start..end], scale, area));
        sheets.push((start..end).zip(places).collect());
        start = end;
    }
    (scale, sheets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{A3_LANDSCAPE, SCALES};

    /// A floor plan of a room of 8 x 5 m, its place standing for its middle.
    fn room() -> Item {
        Item { lo: [-4.0, -2.5], hi: [4.0, 2.5] }
    }

    /// The room an item takes at its place.
    fn bounds(item: &Item, scale: f64, at: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        let s = item.size(scale);
        let lo = [at[0] + item.lo[0] / scale - PAD, at[1] + item.lo[1] / scale - PAD - LABEL];
        (lo, [lo[0] + s[0], lo[1] + s[1]])
    }

    fn inside((lo, hi): ([f64; 2], [f64; 2]), (alo, ahi): ([f64; 2], [f64; 2])) -> bool {
        lo[0] >= alo[0] - 1e-9 && lo[1] >= alo[1] - 1e-9 && hi[0] <= ahi[0] + 1e-9 && hi[1] <= ahi[1] + 1e-9
    }

    #[test]
    fn two_rooms_side_by_side_at_the_largest_scale_that_fits() {
        let a3 = area(A3_LANDSCAPE);
        let (scale, at) = arrange(&[room(), room()], a3, &SCALES);
        // 1:20 is 0.42 m wide for one, more than the 0.38 m there is; at 1:50 they stand side by side
        assert_eq!(scale, 50.0);
        assert!((at[0][1] - at[1][1]).abs() < 1e-9, "on one row: {at:?}");
        let middle = (a3.0[0] + a3.1[0]) / 2.0;
        assert!(((at[0][0] + at[1][0]) / 2.0 - middle).abs() < 1e-9, "the row centred: {at:?}");
        let (b0, b1) = (bounds(&room(), scale, at[0]), bounds(&room(), scale, at[1]));
        assert!(inside(b0, a3) && inside(b1, a3));
        assert!((b1.0[0] - b0.1[0] - GAP).abs() < 1e-9, "a gap between them");
    }

    #[test]
    fn a_drawing_off_its_middle_is_placed_by_its_own_box() {
        // a crop to the right of the cut's middle: the place moves left so the drawing is centred
        let cropped = Item { lo: [2.0, -1.0], hi: [6.0, 1.0] };
        let a3 = area(A3_LANDSCAPE);
        let (scale, at) = arrange(&[cropped], a3, &SCALES);
        assert_eq!(scale, 20.0);
        let (lo, hi) = bounds(&cropped, scale, at[0]);
        assert!(((lo[0] + hi[0]) / 2.0 - (a3.0[0] + a3.1[0]) / 2.0).abs() < 1e-9);
        assert!(at[0][0] < (lo[0] + hi[0]) / 2.0);
    }

    #[test]
    fn a_large_building_gets_a_smaller_scale_and_too_large_the_smallest() {
        let a3 = area(A3_LANDSCAPE);
        let tower = Item { lo: [-58.0, -15.0], hi: [58.0, 15.0] };
        assert_eq!(arrange(&[tower], a3, &SCALES).0, 500.0);
        let city = Item { lo: [-1000.0, -1000.0], hi: [1000.0, 1000.0] };
        let (scale, at) = arrange(&[city, room()], a3, &SCALES);
        assert_eq!((scale, at.len()), (1000.0, 2));
    }

    #[test]
    fn views_on_no_sheet_fill_as_few_sheets_as_they_need() {
        let a3 = area(A3_LANDSCAPE);
        // five rooms: two fit side by side at 1:50, a second row does not fit under them
        let (scale, sheets) = fill(&[room(); 5], a3, &SCALES);
        assert_eq!(scale, 50.0);
        assert_eq!(sheets.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2, 1]);
        assert_eq!(sheets.concat().iter().map(|(k, _)| *k).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
        for (k, at) in sheets.concat() {
            assert!(inside(bounds(&room(), scale, at), a3), "{k}");
        }
        assert!(fill(&[], a3, &SCALES).1.is_empty());
    }
}
