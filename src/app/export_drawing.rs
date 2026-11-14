//! Exporting the view or sheet shown as a drawing: the bar's Export button with its formats, the
//! drawing made from what the tab shows (the cuts of its views made first), and the file saved
//! where the dialog says (desktop) or downloaded (browser).

use super::{App, Message};
use crate::drawing2d::{sheet_drawing, view_drawing, Datum2, Drawing2d};
use crate::export::drawing::{file_name, write, DrawingFormat};
use crate::theme::Palette;
use iced::widget::{button, text, Row};
use iced::{Center, Element, Task};
use openaec_ui::Language;
use std::path::PathBuf;
use web_time::Instant;

impl App {
    /// The bar's Export button; open, the formats to export in follow it.
    pub(super) fn export_control(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let open = self.export_menu;
        let export = button(text(l.text("Exporteren", "Export")).size(12))
            .padding([3, 10])
            .style(move |_, s| p.ribbon_button(open, s))
            .on_press(Message::ExportMenu(!open));
        let mut row = Row::new().spacing(4).align_y(Center).push(export);
        if open {
            for format in DrawingFormat::ALL {
                row = row.push(button(text(format.name()).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::ExportDrawing(format)));
            }
        }
        row.into()
    }

    /// The drawing of the sheet or view shown, its views cut first where they are not yet;
    /// `None` when neither shows.
    pub(super) fn drawing_to_export(&mut self) -> Option<Drawing2d> {
        if let Some(sheet) = self.active_doc_sheet.clone() {
            let views: Vec<String> = self.document.sheet(&sheet)?.views.iter().map(|v| v.plan.clone()).collect();
            for view in &views {
                self.cut_view(view);
            }
            return sheet_drawing(&self.document, &sheet, &|path| self.cuts.get(path).cloned());
        }
        let view = self.shown_view()?.to_string();
        self.cut_view(&view);
        let mut drawing = view_drawing(&self.document, &view, self.cuts.get(&view)?)?;
        drawing.datums(&self.view_datums(&view));
        Some(drawing)
    }

    /// Exports the drawing of the sheet or view shown in `format`.
    pub(super) fn export_drawing(&mut self, format: DrawingFormat) -> Task<Message> {
        match self.drawing_to_export() {
            Some(drawing) => export_task(drawing, format, self.document_folder()),
            None => {
                self.status = super::Status::Error("Nothing to export in this tab".to_string());
                Task::none()
            }
        }
    }

    /// The grid lines and levels a view's tab shows (none while the grid lines are off), on the
    /// view's drawing: a floor plan's in plan, a section's along its line and up.
    fn view_datums(&self, view: &str) -> Vec<Datum2> {
        let Some(model) = self.model.as_ref().filter(|_| self.grids_shown()) else { return Vec::new() };
        if self.document.plan(view).is_some() {
            let flat = |p: [f32; 3]| [f64::from(p[0]), f64::from(p[1])];
            return model.project.grids.iter().map(|g| Datum2 { name: g.name.clone(), a: flat(g.a), b: flat(g.b), level: None }).collect();
        }
        let Some(s) = self.document.section(view) else { return Vec::new() };
        let (along, _) = s.axes();
        let uv = |p: [f32; 3]| [(f64::from(p[0]) - s.from[0]) * along[0] + (f64::from(p[1]) - s.from[1]) * along[1], f64::from(p[2])];
        self.section_datums(s, &model.project.grids)
            .into_iter()
            .map(|d| Datum2 {
                a: uv(d.line.a),
                b: uv(d.line.b),
                level: match d.kind {
                    crate::annotations::DatumKind::Level(z) => Some(z),
                    crate::annotations::DatumKind::Grid(_) | crate::annotations::DatumKind::Outline => None,
                },
                name: d.line.name,
            })
            .collect()
    }

    /// Where the dialog starts: the model's folder, else the work folder.
    fn document_folder(&self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        return crate::managed::dialog_folder(self.model.as_ref().and_then(|m| m.path.parent().map(PathBuf::from)), crate::managed::work_folder());
        #[cfg(target_arch = "wasm32")]
        None
    }
}

/// The format of the file `path`: that of its extension when it is one of the formats (typed in
/// the dialog), else the one `chosen` with the button. The browser has no dialog.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn format_for(path: &std::path::Path, chosen: DrawingFormat) -> DrawingFormat {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    DrawingFormat::ALL.into_iter().find(|f| f.extension() == ext).unwrap_or(chosen)
}

/// Asks where the drawing goes, then writes it there on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn export_task(drawing: Drawing2d, format: DrawingFormat, folder: Option<PathBuf>) -> Task<Message> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Export").set_file_name(file_name(&drawing, format)).add_filter(format.name(), &[format.extension()]);
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    Task::perform(
        async move {
            let path = dialog.save_file().await?.path().to_path_buf();
            let format = format_for(&path, format);
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let start = Instant::now();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                // the dialog asked before replacing a file
                let written = write(&drawing, format).and_then(|bytes| {
                    crate::export::write_file(&path, true, |out| out.write_all(&bytes)).map_err(|e| format!("Cannot export {name}: {e}"))
                });
                let _ = tx.send(written.map(|()| (path, start.elapsed().as_secs_f32())));
            });
            Some(rx.await.unwrap_or_else(|_| Err("export thread stopped".to_string())))
        },
        Message::Exported,
    )
}

/// The browser has no save dialog: the drawing goes to the downloads.
#[cfg(target_arch = "wasm32")]
fn export_task(drawing: Drawing2d, format: DrawingFormat, _folder: Option<PathBuf>) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            let start = Instant::now();
            let name = file_name(&drawing, format);
            let done = write(&drawing, format).and_then(|bytes| crate::web::download(&name, &bytes, format.mime()));
            Some(done.map(|()| (PathBuf::from(name), start.elapsed().as_secs_f32())))
        },
        Message::Exported,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing2d::Pen;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A room of 8 x 5 m with walls 3 m high, levels at 0 and 3, its floor plan shown in its tab.
    fn plan_shown() -> (App, String) {
        plan_shown_with(Vec::new())
    }

    /// The same, the model with these grids.
    fn plan_shown_with(grids: Vec<crate::model::GridLine>) -> (App, String) {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        model.project.grids = grids;
        let mut app = App::default();
        app.viewport_size = (800.0, 600.0);
        app.set_model(Arc::new(model));
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let plan = app.document.plans()[0].path.clone();
        (app, plan)
    }

    fn cut_lines(d: &Drawing2d) -> usize {
        d.lines.iter().filter(|l| l.pen == Pen::Cut).count()
    }

    #[test]
    fn export_of_a_floor_plan_gives_its_cut_dimensions_and_texts() {
        let (mut app, plan) = plan_shown();
        assert!(app.document.add_dimension(&plan, [0.0, 0.0], [8.0, 0.0]));
        app.document.add_note(&plan, [1.0, 1.0], "Kamer");
        let d = app.drawing_to_export().expect("the plan's tab shows");
        assert_eq!(d.name, app.document.view_name(&plan).unwrap());
        assert!(cut_lines(&d) >= 4, "{:?}", d.lines);
        assert_eq!((d.dimensions.len(), d.texts.len()), (1, 1));
    }

    #[test]
    fn export_of_a_sheet_computes_the_cuts_of_its_views() {
        let (mut app, plan) = plan_shown();
        let sheet = app.document.add_sheet("Plattegronden");
        app.document.place_plan(&sheet, &plan).unwrap();
        let _ = app.update(Message::OpenDocSheet(Arc::from(sheet.as_str())));
        app.cuts.clear();
        let d = app.drawing_to_export().expect("the sheet's tab shows");
        assert_eq!(d.paper, Some(crate::document::A3_LANDSCAPE));
        assert!(app.cuts.contains_key(&plan), "cut for the export");
        assert!(cut_lines(&d) >= 4, "{:?}", d.lines);
    }

    #[test]
    fn the_export_menu_opens_and_a_format_closes_it() {
        let (mut app, _) = plan_shown();
        let _ = app.update(Message::ExportMenu(true));
        assert!(app.export_menu);
        let _ = app.update(Message::ExportDrawing(DrawingFormat::Dwg));
        assert!(!app.export_menu);
    }

    #[test]
    fn nothing_to_export_without_a_view_or_sheet_shown() {
        let mut app = App::default();
        assert!(app.drawing_to_export().is_none());
        // and the status says so
        let _ = app.update(Message::ExportDrawing(DrawingFormat::Pdf));
        assert!(matches!(&app.status, super::super::Status::Error(e) if e.contains("export")), "{:?}", std::mem::discriminant(&app.status));
    }

    #[test]
    fn a_floor_plans_drawing_has_its_grids_and_a_sections_its_levels() {
        let (mut app, _) = plan_shown_with(vec![
            crate::model::GridLine { name: "A".into(), a: [-2.0, 2.5, 0.0], b: [10.0, 2.5, 0.0] },
            crate::model::GridLine { name: "1".into(), a: [4.0, -2.0, 0.0], b: [4.0, 7.0, 0.0] },
        ]);
        let d = app.drawing_to_export().unwrap();
        assert!(d.texts.iter().any(|t| t.text == "A") && d.texts.iter().any(|t| t.text == "1"), "{:?}", d.texts);
        assert!(d.lines.iter().any(|l| l.pen == crate::drawing2d::Pen::Grids && l.a == [-2.0, 2.5] && l.b == [10.0, 2.5]), "the grid in plan");
        // a section across: grid 1 upright where it crosses, the levels across
        let _ = app.update(Message::NewSection([0.0, 2.0], [8.0, 2.0], "Section"));
        let d = app.drawing_to_export().unwrap();
        assert!(d.texts.iter().any(|t| t.text == "1"), "{:?}", d.texts);
        assert!(d.texts.iter().any(|t| t.text.starts_with("01 +3.000")), "{:?}", d.texts);
        // in the section's drawing: u along its line (grid 1 at u = 4), v up
        assert!(d.lines.iter().any(|l| l.pen == crate::drawing2d::Pen::Grids && (l.a[0] - 4.0).abs() < 1e-3 && (l.b[0] - 4.0).abs() < 1e-3), "{:?}", d.lines);
    }

    #[test]
    fn the_typed_extension_picks_the_writer() {
        assert_eq!(format_for(std::path::Path::new("plan.pdf"), DrawingFormat::Dwg), DrawingFormat::Pdf);
        assert_eq!(format_for(std::path::Path::new("plan.IFCX"), DrawingFormat::Dwg), DrawingFormat::Ifccad);
        assert_eq!(format_for(std::path::Path::new("plan"), DrawingFormat::Dxf), DrawingFormat::Dxf);
        assert_eq!(format_for(std::path::Path::new("plan.txt"), DrawingFormat::Svg), DrawingFormat::Svg);
    }
}

