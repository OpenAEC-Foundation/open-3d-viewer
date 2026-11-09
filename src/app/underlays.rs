//! Underlays on the levels: a DWG, DXF, IFCCAD or PDF drawing laid on a level from the Levels tab,
//! listed under its level with a switch and a cross, and drawn as lines in the 3D view and in the
//! floor plans at the level's height.

use super::{App, Message, Status};
use crate::theme::Palette;
use crate::underlay::{Segment2, Underlay};
use iced::widget::{button, container, row, svg, text, Column};
use iced::{Center, Element, Fill, Task};
use openaec_ui::Language;
use std::sync::Arc;

/// What reading an underlay gave: its lines and how many entities it left out, or why not.
pub type UnderlayRead = Result<(Vec<Segment2>, usize), String>;

impl App {
    /// Asks for a drawing to lay on `level`, and reads it on a worker thread.
    pub(super) fn add_underlay(&mut self, level: Arc<str>) -> Task<Message> {
        pick_and_read(level, self.model.as_ref().and_then(|m| m.path.parent().map(std::path::PathBuf::from)))
    }

    /// Lays the drawing read (`name`) on `level`, or says why it could not be read.
    pub(super) fn underlay_read(&mut self, level: &str, name: &str, read: UnderlayRead) {
        let (segments, left_out) = match read {
            Ok(read) => read,
            Err(e) => {
                self.status = Status::Error(e);
                return;
            }
        };
        if segments.is_empty() {
            self.status = Status::Error(format!("{name}: no lines in its model space"));
            return;
        }
        // a PDF's lines are in metres of paper: laid at 1:100 first
        let pdf = crate::underlay::is_pdf(name);
        let scale = if pdf { crate::underlay::PDF_SCALES[0] } else { 1.0 };
        let lines = segments.len();
        let mut underlay = Underlay { name: name.to_string(), level: level.to_string(), segments: Arc::new(segments), offset: [0.0, 0.0], shown: true, scale };
        underlay.offset = self.placed(&underlay);
        let left = if left_out > 0 { format!(", {left_out} left out") } else { String::new() };
        let moved = if underlay.offset != [0.0, 0.0] { ", laid in the middle of the model" } else { "" };
        let at = if pdf { format!(", at 1:{scale}") } else { String::new() };
        self.underlays.push(underlay);
        self.status = Status::Loaded(format!("Underlay {name}: {lines} lines{at}{left}{moved}"));
        self.refresh_underlays();
    }

    /// The lines of the underlays switched on, in the model at their levels' heights.
    pub(super) fn underlay_segments(&self) -> Vec<[[f32; 3]; 2]> {
        self.underlays
            .iter()
            .filter(|u| u.shown)
            .filter_map(|u| Some(crate::underlay::world(u, self.document.level(&u.level)?.elevation)))
            .flatten()
            .collect()
    }

    /// Makes the lines the 3D view draws again, after an underlay changed.
    pub(super) fn refresh_underlays(&mut self) {
        self.underlay_lines = Arc::new(self.underlay_segments());
    }

    /// Where an underlay's origin goes in the model at its scale (see [`crate::underlay::offset`]).
    fn placed(&self, u: &Underlay) -> [f64; 2] {
        let model = self.world_bounds().map(|b| ([f64::from(b.min[0]), f64::from(b.min[1])], [f64::from(b.max[0]), f64::from(b.max[1])]));
        let scaled: Vec<Segment2> = u.segments.iter().map(|[a, b]| [[a[0] * u.scale, a[1] * u.scale], [b[0] * u.scale, b[1] * u.scale]]).collect();
        crate::underlay::offset(&scaled, model)
    }

    /// Lays PDF underlay `i` at the next scale, and places it again.
    pub(super) fn next_underlay_scale(&mut self, i: usize) {
        let Some(u) = self.underlays.get(i) else { return };
        let scales = crate::underlay::PDF_SCALES;
        let k = scales.iter().position(|s| *s == u.scale).map_or(0, |k| (k + 1) % scales.len());
        let mut u = u.clone();
        u.scale = scales[k];
        u.offset = self.placed(&u);
        self.underlays[i] = u;
        self.refresh_underlays();
    }

    /// Switches underlay `i` on or off.
    pub(super) fn toggle_underlay(&mut self, i: usize) {
        if let Some(u) = self.underlays.get_mut(i) {
            u.shown = !u.shown;
            self.refresh_underlays();
        }
    }

    /// Takes underlay `i` away.
    pub(super) fn remove_underlay(&mut self, i: usize) {
        if i < self.underlays.len() {
            self.underlays.remove(i);
            self.refresh_underlays();
        }
    }

    /// The underlays on `level` under it in the Levels tab: a switch with the file's name, and a
    /// cross; `None` when it has none.
    pub(super) fn underlay_rows(&self, level: &str, p: Palette, _l: Language) -> Option<Element<'_, Message>> {
        let mut rows = Column::new().spacing(1);
        let mut any = false;
        for (i, u) in self.underlays.iter().enumerate().filter(|(_, u)| u.level == level) {
            any = true;
            let shown = u.shown;
            let name = button(text(u.name.clone()).size(11).color(if shown { p.text } else { p.text_muted }))
                .padding([2, 6])
                .width(Fill)
                .style(move |_, s| p.ribbon_button(shown, s))
                .on_press(Message::ToggleUnderlay(i));
            let close = svg(svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
                .width(11)
                .height(11)
                .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
            let remove = button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemoveUnderlay(i));
            // a PDF is laid at a scale: a click takes the next
            let scale = crate::underlay::is_pdf(&u.name).then(|| {
                button(text(format!("1:{}", u.scale)).size(10).color(p.text_secondary)).padding([2, 4]).style(move |_, s| p.row_button(s)).on_press(Message::UnderlayScale(i))
            });
            rows = rows.push(row![name].push(scale).push(remove).spacing(4).align_y(Center));
        }
        any.then(|| container(rows).padding(iced::Padding { top: 0.0, right: 8.0, bottom: 2.0, left: 20.0 }).width(Fill).into())
    }
}

/// Asks for a drawing (the native dialog), then reads it on a worker thread; nothing when the
/// dialog is cancelled.
#[cfg(not(target_arch = "wasm32"))]
fn pick_and_read(level: Arc<str>, folder: Option<std::path::PathBuf>) -> Task<Message> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Underlay").add_filter("DWG, DXF, IFCCAD, PDF", &["dwg", "dxf", "ifcx", "pdf"]);
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    Task::perform(
        async move {
            let file = dialog.pick_file().await?;
            let (name, path) = (file.file_name(), file.path().to_path_buf());
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let reading = name.clone();
            std::thread::spawn(move || {
                let read = std::fs::read(&path).map_err(|e| format!("{reading}: {e}")).and_then(|bytes| crate::underlay::read(&reading, &bytes));
                let _ = tx.send(read);
            });
            let read = rx.await.unwrap_or_else(|_| Err("reading stopped".to_string()));
            Some(Message::UnderlayRead(level, name, read))
        },
        std::convert::identity,
    )
    .and_then(Task::done)
}

/// In the browser: the browser's file picker, which hands over the bytes, read on the page.
#[cfg(target_arch = "wasm32")]
fn pick_and_read(level: Arc<str>, _folder: Option<std::path::PathBuf>) -> Task<Message> {
    let dialog = rfd::AsyncFileDialog::new().set_title("Underlay").add_filter("DWG, DXF, IFCCAD, PDF", &["dwg", "dxf", "ifcx", "pdf"]);
    Task::perform(
        async move {
            let file = dialog.pick_file().await?;
            let name = file.file_name();
            let bytes = file.read().await;
            crate::web::next_frame().await;
            let read = crate::underlay::read(&name, &bytes);
            Some(Message::UnderlayRead(level, name, read))
        },
        std::convert::identity,
    )
    .and_then(Task::done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;

    /// A room of 8 x 5 m with levels at 0 and 3; the path of the upper level.
    fn room() -> (App, Arc<str>) {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        let upper = app.document.levels().iter().find(|l| l.name == "01").unwrap().path.clone();
        (app, Arc::from(upper.as_str()))
    }

    fn read_one(app: &mut App, level: &Arc<str>) {
        let _ = app.update(Message::UnderlayRead(level.clone(), "plan.dxf".into(), Ok((vec![[[1.0, 1.0], [7.0, 1.0]]], 2))));
    }

    #[test]
    fn an_underlay_lies_on_its_level_in_the_3d_view() {
        let (mut app, upper) = room();
        read_one(&mut app, &upper);
        assert_eq!(app.underlays.len(), 1);
        assert_eq!((app.underlays[0].name.as_str(), &*app.underlays[0].level), ("plan.dxf", &*upper));
        let lines = app.underlay_segments();
        assert_eq!(lines.len(), 1);
        assert!(lines.iter().flatten().all(|p| p[2] > 3.0 && p[2] < 3.01), "{lines:?}");
        // near the model: where it is
        assert_eq!((lines[0][0][0], lines[0][0][1]), (1.0, 1.0));
        assert!(matches!(&app.status, Status::Loaded(s) if s.contains("plan.dxf") && s.contains("2 left out")));
    }

    #[test]
    fn an_underlay_is_switched_off_and_taken_away() {
        let (mut app, upper) = room();
        read_one(&mut app, &upper);
        let _ = app.update(Message::ToggleUnderlay(0));
        assert!(app.underlay_segments().is_empty() && app.underlays.len() == 1);
        let _ = app.update(Message::ToggleUnderlay(0));
        assert_eq!(app.underlay_segments().len(), 1);
        let _ = app.update(Message::RemoveUnderlay(0));
        assert!(app.underlays.is_empty());
    }

    #[test]
    fn a_drawing_that_cannot_be_read_says_why() {
        let (mut app, upper) = room();
        let _ = app.update(Message::UnderlayRead(upper, "plan.dwg".into(), Err("plan.dwg: not a drawing".into())));
        assert!(app.underlays.is_empty());
        assert!(matches!(&app.status, Status::Error(e) if e.contains("not a drawing")));
    }

    #[test]
    fn a_pdf_is_laid_at_1_100_and_its_scale_changes() {
        let (mut app, upper) = room();
        let _ = app.update(Message::UnderlayRead(upper, "plan.pdf".into(), Ok((vec![[[0.0, 0.0], [0.08, 0.0]]], 0))));
        assert!(matches!(&app.status, Status::Loaded(s) if s.contains("1:100")));
        let length = |app: &App| {
            let s = app.underlay_segments()[0];
            (s[1][0] - s[0][0]).abs()
        };
        // 8 cm of paper: 8 m at 1:100, 16 m at 1:200
        assert!((length(&app) - 8.0).abs() < 1e-4);
        let _ = app.update(Message::UnderlayScale(0));
        assert_eq!(app.underlays[0].scale, 200.0);
        assert!((length(&app) - 16.0).abs() < 1e-4);
    }

    #[test]
    fn underlays_go_with_their_model() {
        let (mut app, upper) = room();
        read_one(&mut app, &upper);
        let (other, _) = room();
        app.set_model(other.model.clone().unwrap());
        assert!(app.underlays.is_empty());
    }
}
