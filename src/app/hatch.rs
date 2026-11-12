//! The cut faces of a drawing on a canvas: each face in its fill, then its hatch lines, cut back
//! to the view's crop; and, dashed, what a structural plan sees through the floor. Shared by a view on screen and by the views placed on a sheet.

use crate::drawing::Cut;
use iced::widget::canvas::{fill::Rule, Fill, Frame, Path, Stroke};
use iced::{Color, Point};

/// Hatch lines stand at least this far apart (logical pixels): rows are left out below it.
const LEAST: f32 = 3.0;
/// The dashes of what lies under a floor, and the gaps between them (logical pixels).
const DASH: f32 = 5.0;
const GAP: f32 = 3.0;

fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

/// Draws the cut faces of `cut` within `crop`: `to` gives a drawing point on the canvas (`None`
/// when it cannot be shown), `per_metre` how many pixels a metre of the drawing is, `outline`
/// the colour of the faces' outlines where the drawing's own lines do not draw them.
pub(super) fn draw_fills(frame: &mut Frame, cut: &Cut, crop: Option<([f64; 2], [f64; 2])>, to: &dyn Fn([f64; 2]) -> Option<Point>, per_metre: f32, outline: Option<Color>) {
    for fill in &cut.fills {
        let Some((behind, line)) = fill.look else { continue };
        let face = Path::new(|path| {
            for outline in fill.cropped(crop) {
                let Some(points) = outline.iter().map(|q| to(*q)).collect::<Option<Vec<Point>>>() else { continue };
                let Some((first, rest)) = points.split_first() else { continue };
                path.move_to(*first);
                for p in rest {
                    path.line_to(*p);
                }
                path.close();
            }
        });
        frame.fill(&face, Fill { rule: Rule::EvenOdd, ..Fill::from(rgb(behind)) });
        if fill.rows.is_empty() {
            continue;
        }
        let lines = Path::new(|path| {
            for [a, b] in fill.lines(fill.spacing() * per_metre, LEAST) {
                let (a, b) = (a.map(f64::from), b.map(f64::from));
                let piece = match crop {
                    Some((lo, hi)) => crate::drawing2d::clip(a, b, lo, hi),
                    None => Some((a, b)),
                };
                if let Some((Some(a), Some(b))) = piece.map(|(a, b)| (to(a), to(b))) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        });
        frame.stroke(&lines, Stroke::default().with_color(rgb(line)).with_width(0.6));
    }
    // the outlines of the faces: the lines of the cut, without those between faces of one piece
    if let Some(color) = outline {
        let lines = Path::new(|path| {
            for [a, b] in &cut.segments {
                let (a, b) = (a.map(f64::from), b.map(f64::from));
                let piece = match crop {
                    Some((lo, hi)) => crate::drawing2d::clip(a, b, lo, hi),
                    None => Some((a, b)),
                };
                if let Some((Some(a), Some(b))) = piece.map(|(a, b)| (to(a), to(b))) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        });
        frame.stroke(&lines, Stroke::default().with_color(color).with_width(1.2));
    }
    // what a structural plan sees through the floor: dashed
    if !cut.hidden.is_empty() {
        let covered = Path::new(|path| {
            for [a, b] in &cut.hidden {
                let (a, b) = (a.map(f64::from), b.map(f64::from));
                let piece = match crop {
                    Some((lo, hi)) => crate::drawing2d::clip(a, b, lo, hi),
                    None => Some((a, b)),
                };
                let Some((Some(a), Some(b))) = piece.map(|(a, b)| (to(a), to(b))) else { continue };
                // each dash a line of its own: a dashed stroke would join the lines of a path
                let run = a.distance(b);
                let at = |d: f32| Point::new(a.x + (b.x - a.x) * d / run, a.y + (b.y - a.y) * d / run);
                let mut d = 0.0;
                while d < run {
                    path.move_to(at(d));
                    path.line_to(at((d + DASH).min(run)));
                    d += DASH + GAP;
                }
            }
        });
        frame.stroke(&covered, Stroke::default().with_color(outline.unwrap_or(Color::from_rgb8(40, 40, 40))).with_width(0.8));
    }
}
