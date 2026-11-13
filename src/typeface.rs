//! The typeface a text of a drawing is set in, as its file names it: the family by name, bold,
//! italic, underlined, and how much wider its letters are than the font draws them.
//!
//! A family is looked up among the fonts of the system (the desktop loads them with the app's
//! own); one that is not there is set in the app's own font, with the same weight and slant.

use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{alignment, Point, Size, Vector};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A typeface: an empty family is the app's own font.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub family: Arc<str>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// How wide its letters are to what the font draws (1 as drawn).
    pub width: f32,
    /// How far apart the stops are a tab runs to, in ems (0: [`TAB`]).
    pub tab: f32,
    /// The colour of its letters (red, green, blue), when it has one of its own: else the ink of
    /// the drawing.
    pub color: Option<[u8; 3]>,
}

impl Default for Face {
    fn default() -> Self {
        Self { family: Arc::from(""), bold: false, italic: false, underline: false, width: 1.0, tab: 0.0, color: None }
    }
}

/// How far apart tab stops are where a text does not say (ems).
const TAB: f32 = 4.0;

/// A line's height to its em where a text does not say.
const LINE: f32 = 1.3;

impl Face {
    /// A face of a family, as drawn.
    pub fn of(family: &str) -> Self {
        Self { family: Arc::from(family.trim()), ..Self::default() }
    }

    /// Whether it says nothing: the app's own font, as drawn.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// As an attribute of a document's text; none for a plain one.
    pub fn to_json(&self) -> Option<Value> {
        if self.is_plain() {
            return None;
        }
        let mut v = json!({});
        if !self.family.is_empty() {
            v["family"] = (&*self.family).into();
        }
        for (key, on) in [("bold", self.bold), ("italic", self.italic), ("underline", self.underline)] {
            if on {
                v[key] = true.into();
            }
        }
        if (self.width - 1.0).abs() > 1e-4 {
            v["width"] = self.width.into();
        }
        if self.tab > 0.0 {
            v["tab"] = self.tab.into();
        }
        if let Some(color) = self.color {
            v["color"] = json!(color);
        }
        Some(v)
    }

    /// From an attribute of a document's text; plain for none.
    pub fn from_json(v: Option<&Value>) -> Self {
        let Some(v) = v.filter(|v| v.is_object()) else { return Self::default() };
        let on = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            family: Arc::from(v.get("family").and_then(Value::as_str).unwrap_or_default().trim()),
            bold: on("bold"),
            italic: on("italic"),
            underline: on("underline"),
            width: v.get("width").and_then(Value::as_f64).filter(|w| w.is_finite() && (0.1..=10.0).contains(w)).map_or(1.0, |w| w as f32),
            tab: v.get("tab").and_then(Value::as_f64).filter(|t| t.is_finite() && (0.0..=1000.0).contains(t)).map_or(0.0, |t| t as f32),
            color: v.get("color").and_then(Value::as_array).filter(|c| c.len() == 3).and_then(|c| Some([u8::try_from(c[0].as_u64()?).ok()?, u8::try_from(c[1].as_u64()?).ok()?, u8::try_from(c[2].as_u64()?).ok()?])),
        }
    }

    /// Whether its family is among the fonts the app can draw with (an empty one is).
    pub fn installed(&self) -> bool {
        self.family.is_empty() || family(&self.family).is_some()
    }

    /// The font to draw it with: its family when the system has it, else the app's own.
    pub fn font(&self) -> iced::Font {
        let own = openaec_ui::fonts::MEDIUM;
        let named = family(&self.family);
        iced::Font {
            family: named.map_or(own.family, iced::font::Family::Name),
            weight: match (self.bold, named) {
                (true, _) => iced::font::Weight::Bold,
                (false, Some(_)) => iced::font::Weight::Normal,
                (false, None) => own.weight,
            },
            style: if self.italic { iced::font::Style::Italic } else { iced::font::Style::Normal },
            ..own
        }
    }

    /// How wide `text` is at a letter size of `size`: its widest line.
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        use iced::advanced::text::Paragraph as _;
        let font = self.font();
        let line = |content: &str| {
            iced::advanced::graphics::text::Paragraph::with_text(iced::advanced::Text {
                content,
                bounds: Size::INFINITE,
                size: iced::Pixels(size),
                line_height: iced::widget::text::LineHeight::Relative(LINE),
                font,
                align_x: iced::widget::text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: iced::widget::text::Shaping::Advanced,
                wrapping: iced::widget::text::Wrapping::None,
            })
            .min_bounds()
            .width
        };
        let stop = self.stop(size);
        let tabbed = |content: &str| content.split('\t').fold(None, |x: Option<f32>, piece| Some(x.map_or(0.0, |x| ((x / stop).floor() + 1.0) * stop) + if piece.is_empty() { 0.0 } else { line(piece) })).unwrap_or(0.0);
        text.lines().map(tabbed).fold(0.0, f32::max) * self.width
    }

    /// How far apart its tab stops are at a letter size of `size`, as the font draws.
    fn stop(&self, size: f32) -> f32 {
        (if self.tab > 0.0 { self.tab } else { TAB }) * size / self.width.max(0.1)
    }

    /// `text` with lines of at most `width` at a letter size of `size`: longer ones break between
    /// words, as a text box breaks them.
    pub fn wrap(&self, text: &str, size: f32, width: f32) -> String {
        let space = self.measure("a a", size) - self.measure("aa", size);
        let mut lines: Vec<String> = Vec::new();
        for paragraph in text.lines() {
            // a line with tabs is measured whole: how far a tab runs depends on what stands
            // before it, so the widths of its words do not add up
            let tabbed = paragraph.contains('\t');
            let (mut line, mut run) = (String::new(), 0.0);
            for word in paragraph.split(' ') {
                // a word may break after a hyphen in it
                for (k, piece) in word.split_inclusive('-').enumerate() {
                    let gap = if k == 0 && !line.is_empty() { space } else { 0.0 };
                    let w = if tabbed { self.measure(&format!("{line}{}{piece}", if gap > 0.0 { " " } else { "" }), size) - run - gap } else { self.measure(piece, size) };
                    if !line.is_empty() && run + gap + w > width {
                        lines.push(std::mem::take(&mut line));
                        run = 0.0;
                    } else if gap > 0.0 {
                        line.push(' ');
                        run += gap;
                    }
                    line.push_str(piece);
                    run = if tabbed { self.measure(&line, size) } else { run + w };
                }
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    /// Draws a text of a canvas in this face: `text` as it would be drawn, its font replaced.
    pub fn fill(&self, frame: &mut Frame, text: canvas::Text) {
        // in its own colour, as dark or light as the drawing's ink is meant to be seen
        let color = self.color.map_or(text.color, |c| iced::Color { a: text.color.a, ..iced::Color::from_rgb8(c[0], c[1], c[2]) });
        let text = canvas::Text { font: self.font(), color, ..text };
        let wide = (self.width - 1.0).abs() > 1e-3;
        if text.content.contains('\t') && text.max_width.is_infinite() {
            return self.fill_tabbed(frame, text);
        }
        if !self.underline && !wide {
            frame.fill_text(text);
            return;
        }
        let at = text.position;
        let (size, color) = (text.size.0, text.color);
        // its lines: where each starts, how long it is and where its foot lies, about `at`
        let rules: Vec<(f32, f32, f32)> = if self.underline && text.max_width.is_infinite() {
            let lead = text.line_height.to_absolute(text.size).0;
            let count = text.content.lines().count().max(1) as f32;
            let top = match text.align_y {
                alignment::Vertical::Top => 0.0,
                alignment::Vertical::Center => -lead * count / 2.0,
                alignment::Vertical::Bottom => -lead * count,
            };
            let plain = Face { width: 1.0, ..self.clone() };
            text.content
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.trim().is_empty())
                .map(|(i, l)| {
                    let w = plain.measure(l, size);
                    let x = match text.align_x {
                        iced::widget::text::Alignment::Center => -w / 2.0,
                        iced::widget::text::Alignment::Right => -w,
                        _ => 0.0,
                    };
                    // the letters stand in the middle of their line; the rule lies under their feet
                    (x, w, top + lead * (i as f32 + 0.5) + size * 0.52)
                })
                .collect()
        } else {
            Vec::new()
        };
        frame.with_save(|frame| {
            frame.translate(Vector::new(at.x, at.y));
            if wide {
                frame.scale_nonuniform(Vector::new(self.width, 1.0));
            }
            frame.fill_text(canvas::Text { position: Point::ORIGIN, ..text });
            for (x, w, y) in rules {
                frame.stroke(&Path::line(Point::new(x, y), Point::new(x + w, y)), Stroke::default().with_color(color).with_width((size * 0.06).max(1.0)));
            }
        });
    }
}

impl Face {
    /// Draws a text with tabs: each line in the pieces between its tabs, each piece from the stop
    /// its tab runs to.
    fn fill_tabbed(&self, frame: &mut Frame, text: canvas::Text) {
        let size = text.size.0;
        let lead = text.line_height.to_absolute(text.size).0;
        let stop = self.stop(size);
        let plain = Face { width: 1.0, ..self.clone() };
        let count = text.content.lines().count().max(1) as f32;
        let top = match text.align_y {
            alignment::Vertical::Top => 0.0,
            alignment::Vertical::Center => -lead * count / 2.0,
            alignment::Vertical::Bottom => -lead * count,
        };
        let at = text.position;
        frame.with_save(|frame| {
            frame.translate(Vector::new(at.x, at.y));
            for (i, line) in text.content.lines().enumerate() {
                // where each piece starts, as the font draws
                let mut pieces: Vec<(f32, &str)> = Vec::new();
                let mut x = 0.0;
                for (k, piece) in line.split('\t').enumerate() {
                    if k > 0 {
                        x = ((x / stop).floor() + 1.0) * stop;
                    }
                    if !piece.is_empty() {
                        pieces.push((x, piece));
                        x += plain.measure(piece, size);
                    }
                }
                let shift = match text.align_x {
                    iced::widget::text::Alignment::Center => -x / 2.0,
                    iced::widget::text::Alignment::Right => -x,
                    _ => 0.0,
                } * self.width;
                for (x, piece) in pieces {
                    let piece = canvas::Text { content: piece.to_string(), position: Point::new(shift + x * self.width, top + lead * i as f32), align_x: iced::widget::text::Alignment::Left, align_y: alignment::Vertical::Top, ..text.clone() };
                    self.fill(frame, piece);
                }
            }
        });
    }
}

/// The name the fonts know a family by, when one of them is of it.
fn family(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    static KNOWN: OnceLock<Mutex<HashMap<String, Option<&'static str>>>> = OnceLock::new();
    let mut known = KNOWN.get_or_init(Default::default).lock().ok()?;
    if let Some(found) = known.get(name) {
        return *found;
    }
    let found = {
        let mut fonts = iced::advanced::graphics::text::font_system().write().ok()?;
        let spelled = fonts.raw().db().faces().flat_map(|f| f.families.iter()).find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone());
        // a name lives as long as the app: the fonts of a file are few
        spelled.map(|n| &*Box::leak(n.into_boxed_str()))
    };
    known.insert(name.to_string(), found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_keeps_what_it_says_as_an_attribute() {
        assert_eq!(Face::default().to_json(), None);
        let face = Face { family: Arc::from("Arial"), bold: true, italic: false, underline: true, width: 0.8, tab: 3.5, color: Some([255, 255, 255]) };
        assert_eq!(Face::from_json(face.to_json().as_ref()), face);
        assert_eq!(Face::from_json(None), Face::default());
    }

    #[test]
    fn a_family_the_system_lacks_is_set_in_the_apps_own() {
        let face = Face::of("No Such Family 12345");
        assert!(!face.installed());
        assert_eq!(face.font().family, openaec_ui::fonts::MEDIUM.family);
    }
}
