//! Picking the model's grid lines in the viewport: a click on a grid's line or on one of its
//! bubbles picks it, unless something lies in front of the line there (the bubbles are drawn on
//! top, so they are always seen). The picked grid is drawn in the accent colour and the properties
//! pane shows it; a click on anything else, Escape, or switching the grid lines off lets it go.

use super::{prop_row, App, Message};
use crate::theme::Palette;
use glam::Vec3;
use iced::widget::column;
use iced::{Element, Point};
use openaec_ui::Language;

impl App {
    /// Whether the grid lines are drawn (not in the flipped view, whose up is another axis).
    pub(super) fn grids_shown(&self) -> bool {
        self.show_grids && !self.flip_up
    }

    /// The grid under viewport point (x, y) of a viewport of width x height, unless a triangle
    /// lies clearly in front of its line there (`triangle`: the world point of the triangle the
    /// same click hits); `None` while the grid lines are not drawn.
    pub(crate) fn grid_under(&self, x: f32, y: f32, width: f32, height: f32, triangle: Option<Vec3>) -> Option<usize> {
        let model = self.model.as_ref().filter(|_| self.grids_shown())?;
        let view_proj = self.camera.view_proj(width / height);
        crate::annotations::grid_at(&view_proj, (width, height), &model.project.grids, Point::new(x, y), self.not_behind(triangle))
    }

    /// Picks grid `g` (`None`: no grid) instead of the elements selected.
    pub(crate) fn pick_grid(&mut self, g: Option<usize>) {
        self.sel.clear();
        self.selection_changed();
        self.set_structural_selection(Vec::new());
        self.picked_grid = g;
    }

    /// The picked grid's properties: its name, length, direction (degrees from x, 0 to 180),
    /// elevation and ends (x, y in metres).
    pub(super) fn grid_properties(&self, p: Palette, l: Language) -> Option<Element<'static, Message>> {
        let grid = self.model.as_ref()?.project.grids.get(self.picked_grid?)?;
        let (a, b) = (Vec3::from_array(grid.a), Vec3::from_array(grid.b));
        let along = b - a;
        let direction = along.y.atan2(along.x).to_degrees().rem_euclid(180.0);
        let end = |q: Vec3| format!("{:.3}; {:.3}", q.x, q.y);
        let row = |k: &str, v: String| prop_row(p, k.to_string(), v);
        Some(
            column![
                row(l.text("Naam", "Name"), grid.name.clone()),
                row(l.text("Lengte", "Length"), format!("{:.3} m", along.length())),
                row(l.text("Richting", "Direction"), format!("{direction:.1}°")),
                row(l.text("Hoogte", "Elevation"), format!("{:+.3} m", a.z)),
                row(l.text("Begin", "Start"), end(a)),
                row(l.text("Eind", "End"), end(b)),
            ]
            .spacing(5)
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GridLine, Layer, Model};
    use crate::viewport::camera::ViewPreset;
    use crate::viewport::Navigate;
    use crate::app::Message;
    use glam::Vec3;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A floor of 24 x 24 m at z = 0 (element 10), a slab over it at z = 3 from x = 4 to 13 along
    /// grid A (element 20), and grids A (along x) and 1 (along y) on the floor, seen from above.
    fn app() -> App {
        let square = |x0: f32, y0: f32, x1: f32, y1: f32, z: f32| [x0, y0, z, x1, y0, z, x1, y1, z, x0, y1, z];
        let mut positions = square(-12.0, -12.0, 12.0, 12.0, 0.0).to_vec();
        positions.extend(square(4.0, -1.0, 13.0, 1.0, 3.0));
        let layer = Layer {
            name: "Floors".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        let mut model = Model::new(PathBuf::from("grids.rvt"), "Revit", vec![layer]);
        model.project.grids = vec![
            GridLine { name: "A".into(), a: [-10.0, 0.0, 0.0], b: [10.0, 0.0, 0.0] },
            GridLine { name: "1".into(), a: [0.0, -10.0, 0.0], b: [0.0, 10.0, 0.0] },
        ];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        if let Some((_, to, _)) = app.anim.take() {
            app.camera = to;
        }
        app
    }

    /// A click `dy` pixels below where world point `p` is on screen.
    fn click(app: &mut App, p: Vec3, dy: f32) {
        let (w, h) = app.viewport_size;
        let n = app.camera.view_proj(w / h).project_point3(p);
        click_px(app, (n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h + dy);
    }

    fn click_px(app: &mut App, x: f32, y: f32) {
        let (width, height) = app.viewport_size;
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width, height, add: false }));
    }

    /// Where grid `g`'s second bubble is on screen.
    fn bubble(app: &App, g: usize) -> iced::Point {
        let (w, h) = app.viewport_size;
        let grid = &app.model.as_ref().unwrap().project.grids[g];
        crate::annotations::bubble_centres(&app.camera.view_proj(w / h), (w, h), grid).expect("on screen")[1]
    }

    fn shows(app: &App, wording: &str) -> bool {
        let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view());
        ui.find(wording).is_ok()
    }

    #[test]
    fn a_click_on_a_grid_picks_it_and_shows_its_properties() {
        let mut app = app();
        // beside the line, over the floor in the same plane
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 3.0);
        assert_eq!(app.picked_grid, Some(0));
        assert!(!app.has_selection());
        for shown in ["GRID", "A", "20.000 m", "0.0°", "+0.000 m"] {
            assert!(shows(&app, shown), "{shown}");
        }
        // the slab in front of the line takes the click there
        click(&mut app, Vec3::new(5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, None);
        assert_eq!(app.sel.primary(), Some(crate::app::Pickable::Element(20)));
        assert!(!shows(&app, "GRID"));
        // a bubble is always seen, also over the slab
        let end = bubble(&app, 0);
        click_px(&mut app, end.x, end.y + 4.0);
        assert_eq!(app.picked_grid, Some(0));
        assert!(!app.has_selection());
        click(&mut app, Vec3::new(0.0, 6.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, Some(1));
        assert!(shows(&app, "90.0°"));
        let _ = app.update(Message::Escape);
        assert_eq!(app.picked_grid, None);
    }

    #[test]
    fn switching_the_grid_lines_off_lets_the_picked_grid_go_and_clicks_pass_through() {
        let mut app = app();
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, Some(0));
        let _ = app.update(Message::ToggleGrids);
        assert_eq!(app.picked_grid, None);
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, None);
        assert_eq!(app.sel.primary(), Some(crate::app::Pickable::Element(10)));
    }
}
