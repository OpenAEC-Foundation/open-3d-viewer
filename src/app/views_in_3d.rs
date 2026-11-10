//! Views in 3D in the 3D view's tab: the switch in the View tab, the views' frames and cut lines
//! drawn with the underlays' lines (made again only when a view, its cut or an underlay changes),
//! and the views' names drawn at their frames.

use super::{App, DocTab};
use crate::views3d::{self, Colored, Views3d};
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame};
use iced::{alignment, mouse, Color, Point, Rectangle};
use std::sync::Arc;

/// The blue of the underlays' lines.
const UNDERLAY: [u8; 4] = [52, 128, 204, 255];

impl App {
    /// The 3D view's tab is shown (not a view's, a sheet's or a schedule's).
    fn model_tab_shown(&self) -> bool {
        matches!(self.doc_tabs().get(self.active_doc_tab()), Some(DocTab::Model) | None)
    }

    /// The views drawn in 3D now: none unless switched on and in the 3D view's tab.
    fn views_in_3d_now(&self) -> Views3d {
        if !self.views_in_3d || !self.model_tab_shown() {
            return Views3d::default();
        }
        let bounds = self.world_bounds();
        let mut v = views3d::document_views(&self.document, &|p| self.cuts.get(p).cloned(), bounds.as_ref());
        if let Some(model) = &self.model {
            let saved = views3d::saved_views(&model.views, bounds.as_ref());
            v.lines.extend(saved.lines);
            v.labels.extend(saved.labels);
        }
        v
    }

    /// What the views in 3D are drawn from: whether they show, and every view with its cut and
    /// crop; the lines are made again when it changes.
    fn views_in_3d_key(&self) -> Vec<u64> {
        let mut key = vec![u64::from(self.views_in_3d && self.model_tab_shown()), self.model.as_ref().map_or(0, |m| Arc::as_ptr(m) as u64)];
        if key[0] == 0 {
            return key;
        }
        let crop = |c: Option<([f64; 2], [f64; 2])>| c.map_or(0, |(lo, hi)| (lo[0] + 3.0 * lo[1] + 7.0 * hi[0] + 11.0 * hi[1]).to_bits());
        for p in self.document.plans() {
            key.extend([self.cuts.get(&p.path).map_or(0, |c| Arc::as_ptr(c) as u64), crop(p.crop), p.cut.to_bits()]);
        }
        for s in self.document.sections() {
            key.extend([self.cuts.get(&s.path).map_or(0, |c| Arc::as_ptr(c) as u64), crop(s.crop), s.from[0].to_bits(), s.to[1].to_bits()]);
        }
        key.extend(self.document.levels().iter().map(|l| l.elevation.to_bits()));
        key
    }

    /// After any change: the views in 3D cut where they are not yet, and drawn again when what
    /// they are drawn from changed.
    pub(super) fn keep_views_in_3d(&mut self) {
        if self.views_in_3d && self.model_tab_shown() {
            let paths: Vec<String> = self.document.plans().iter().map(|p| p.path.clone()).chain(self.document.sections().iter().map(|s| s.path.clone())).collect();
            for path in paths {
                self.cut_view(&path);
            }
        }
        let key = self.views_in_3d_key();
        if key != self.views_3d_key {
            self.views_3d_key = key;
            self.refresh_overlay();
        }
    }

    /// Makes the lines the 3D view draws over the model again: the underlays' and the views'.
    pub(super) fn refresh_overlay(&mut self) {
        let mut lines: Vec<Colored> = self.underlay_segments().into_iter().map(|s| (s, UNDERLAY)).collect();
        let views = self.views_in_3d_now();
        lines.extend(views.lines);
        self.views_3d_labels = views.labels;
        self.overlay_lines = Arc::new(lines);
    }
}

/// The names of the views in 3D, at the corners of their frames.
pub(super) struct ViewLabels {
    pub labels: Vec<(String, [f32; 3])>,
    pub view_proj: Mat4,
    pub ink: Color,
}

impl<Message> canvas::Program<Message> for ViewLabels {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        for (name, at) in &self.labels {
            let clip = self.view_proj * Vec3::from_array(*at).extend(1.0);
            if clip.w <= 1e-6 {
                continue;
            }
            let ndc = clip.truncate() / clip.w;
            if ndc.x.abs() > 1.2 || ndc.y.abs() > 1.2 || ndc.z < 0.0 || ndc.z > 1.0 {
                continue;
            }
            let position = Point::new((ndc.x + 1.0) * 0.5 * bounds.width + 4.0, (1.0 - ndc.y) * 0.5 * bounds.height - 4.0);
            frame.fill_text(canvas::Text {
                content: name.clone(),
                position,
                align_x: alignment::Horizontal::Left.into(),
                align_y: alignment::Vertical::Bottom,
                size: iced::Pixels(12.0),
                color: self.ink,
                font: openaec_ui::fonts::MEDIUM,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::model::{Layer, Model};
    use crate::views3d::{CUT, FRAME};
    use std::path::PathBuf;

    /// A room of 8 x 5 m with walls 3 m high and levels at 0 and 3, a floor plan of the lower level
    /// made, and the 3D view's tab shown again.
    fn room_with_a_plan() -> App {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        let mut app = App::default();
        app.viewport_size = (800.0, 600.0);
        app.set_model(Arc::new(model));
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let _ = app.update(Message::SelectTab(0));
        app
    }

    fn colored(app: &App, color: [u8; 4]) -> Vec<[[f32; 3]; 2]> {
        app.overlay_lines.iter().filter(|(_, c)| *c == color).map(|(s, _)| *s).collect()
    }

    #[test]
    fn a_floor_plan_lies_in_the_3d_view_with_its_name() {
        let mut app = room_with_a_plan();
        assert!(colored(&app, CUT).is_empty(), "off at first");
        let _ = app.update(Message::ToggleViewsIn3d);
        let cut = colored(&app, CUT);
        assert!(cut.len() >= 4, "the walls cut: {cut:?}");
        assert!(cut.iter().flatten().all(|p| (p[2] - 1.2).abs() < 1e-4), "at the cut, 1.2 m up");
        assert_eq!(colored(&app, FRAME).len(), 4);
        assert_eq!(app.views_3d_labels.len(), 1);
        let _ = app.update(Message::ToggleViewsIn3d);
        assert!(colored(&app, CUT).is_empty() && app.views_3d_labels.is_empty());
    }

    #[test]
    fn a_new_section_shows_in_the_3d_view_at_once() {
        let mut app = room_with_a_plan();
        let _ = app.update(Message::ToggleViewsIn3d);
        let before = colored(&app, FRAME).len();
        let _ = app.update(Message::NewSection([0.0, 2.0], [8.0, 2.0], "Section"));
        // the section opened in its tab: views in 3D show in the 3D view's tab only
        assert!(colored(&app, FRAME).is_empty());
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(colored(&app, FRAME).len(), before + 4);
    }

    #[test]
    fn the_underlays_lines_stay_with_the_views_off() {
        let mut app = room_with_a_plan();
        let upper = Arc::<str>::from(app.document.levels()[1].path.as_str());
        let _ = app.update(Message::UnderlayRead(upper, "plan.dxf".into(), Ok((vec![[[1.0, 1.0], [7.0, 1.0]]], 0))));
        assert_eq!(colored(&app, UNDERLAY).len(), 1);
    }
}
