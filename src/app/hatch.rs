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

/// A colour in halftone: half way to white.
fn halftone(c: [u8; 3]) -> [u8; 3] {
    c.map(|v| ((u16::from(v) + 255) / 2) as u8)
}

/// The lines of what a view draws faint (existing work, in halftone): mid grey.
const FAINT: [u8; 3] = [150, 150, 150];

/// Draws the cut faces of `cut` within `crop`: `to` gives a drawing point on the canvas (`None`
/// when it cannot be shown), `per_metre` how many pixels a metre of the drawing is, `outline`
/// the colour of the faces' outlines where the drawing's own lines do not draw them.
pub(super) fn draw_fills(frame: &mut Frame, cut: &Cut, crop: Option<([f64; 2], [f64; 2])>, to: &dyn Fn([f64; 2]) -> Option<Point>, per_metre: f32, outline: Option<Color>) {
    for fill in &cut.fills {
        let Some((behind, line)) = fill.look else { continue };
        // a face drawn faint: its colours half way to the paper's white
        let (behind, line) = if fill.faint { (halftone(behind), halftone(line)) } else { (behind, line) };
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
    // what the view draws faint: its edges beyond the cut thin, its cut lines bolder
    for (lines, width) in [(&cut.faint_below, 0.5), (&cut.faint_segments, 1.0)] {
        if lines.is_empty() {
            continue;
        }
        let path = Path::new(|path| {
            for [a, b] in lines {
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
        frame.stroke(&path, Stroke::default().with_color(rgb(FAINT)).with_width(width));
    }
    // what a structural plan sees through the floor: dashed, in halftone for what it draws faint
    for (segments, color) in [(&cut.hidden, outline.unwrap_or(Color::from_rgb8(40, 40, 40))), (&cut.faint_hidden, rgb(FAINT))] {
        if segments.is_empty() {
            continue;
        }
        let covered = Path::new(|path| {
            // run by run, so the dashes run on round a curve of many short pieces
            for run in crate::drawing::chained(segments) {
                // where along the run the pattern stands: a dash ends at DASH, a gap at DASH + GAP
                let mut phase = 0.0f32;
                for pair in run.windows(2) {
                    let (a, b) = (pair[0].map(f64::from), pair[1].map(f64::from));
                    let piece = match crop {
                        Some((lo, hi)) => crate::drawing2d::clip(a, b, lo, hi),
                        None => Some((a, b)),
                    };
                    let Some((Some(a), Some(b))) = piece.map(|(a, b)| (to(a), to(b))) else { continue };
                    // each dash a line of its own: a dashed stroke would join the lines of a path
                    let length = a.distance(b);
                    let at = |d: f32| Point::new(a.x + (b.x - a.x) * d / length, a.y + (b.y - a.y) * d / length);
                    let mut d = 0.0;
                    while d < length {
                        // a step too small to move on (the pattern at the very end of a dash or gap): the next begins
                        if (if phase < DASH { DASH - phase } else { DASH + GAP - phase }) < 1e-3 {
                            phase = if phase < DASH { DASH } else { 0.0 };
                        }
                        let step = if phase < DASH { DASH - phase } else { DASH + GAP - phase };
                        let to = (d + step).min(length);
                        if phase < DASH {
                            path.move_to(at(d));
                            path.line_to(at(to));
                        }
                        phase = (phase + (to - d)) % (DASH + GAP);
                        d = to;
                    }
                }
            }
        });
        frame.stroke(&covered, Stroke::default().with_color(color).with_width(0.8));
    }
}

