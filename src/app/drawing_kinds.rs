//! A view's kind of drawing: the drawing standard steers the drawings.
//!
//! A floor plan, a section or an elevation is a kind of drawing of the project's drawing
//! standard (`bim_library::drawing`): a floor plan the kind "floor plan" until it is given
//! another, a structural plan "structural plan", and so on. Its kind says how it is drawn: the
//! width of what is cut and of what is seen beyond it, the dashes of what is hidden, the height
//! of its texts and dimensions, whether what is cut is hatched, and the scales it is drawn at.
//! The drawing on a sheet and every exported drawing follow the kind as it is at that moment:
//! change a pen of a kind in Library, or in the view's own properties, and its drawings change.
//!
//! A view made new takes the usual scale of its kind; given another kind, it takes that one's.
//! A view placed on a sheet starts at its own scale, and is offered the scales of its kind.

use super::standards::{Setting, StandardView};
use super::App;
use crate::document::{PlanKind, SectionKind};
use crate::drawing2d::Rules;
use bim_library::drawing::{DrawingKind, View};

impl App {
    /// What kind of view `path` is to the drawing standard, and the key of the kind of drawing
    /// usual for it; `None` for a 3D view, a sheet or no view.
    fn view_sort(&self, path: &str) -> Option<(View, &'static str)> {
        if let Some(plan) = self.document.plan(path) {
            return Some(match plan.kind {
                PlanKind::Floor => (View::Plan, "floor-plan"),
                PlanKind::Structural => (View::Plan, "structural-plan"),
                PlanKind::Ceiling => (View::CeilingPlan, "ceiling-plan"),
                PlanKind::StructuralUp => (View::Plan, "pile-plan"),
            });
        }
        Some(match self.document.section(path)?.kind {
            SectionKind::Section => (View::Section, "section"),
            SectionKind::Elevation => (View::Elevation, "elevation"),
        })
    }

    /// The kinds of drawing view `path` can be: those of the standard that are its kind of view.
    pub(super) fn view_kinds(&self, path: &str) -> Vec<&DrawingKind> {
        let Some((view, _)) = self.view_sort(path) else { return Vec::new() };
        self.standard.kinds.iter().filter(|kind| kind.view == view).collect()
    }

    /// The kind of drawing view `path` is: the one it was given, while the standard has it and
    /// it is that kind of view; else the one usual for what it is; else the first of the
    /// standard that is its kind of view.
    pub(super) fn view_kind(&self, path: &str) -> Option<&DrawingKind> {
        let (view, usual) = self.view_sort(path)?;
        let fits = |key: &str| self.standard.kind(key).filter(|kind| kind.view == view);
        self.document.view_drawing(path).and_then(fits).or_else(|| fits(usual)).or_else(|| self.standard.kinds.iter().find(|kind| kind.view == view))
    }

    /// How view `path` is drawn: by the rules of its kind of drawing, or plainly without one.
    pub(super) fn view_rules(&self, path: &str) -> Rules {
        self.view_kind(path).map_or_else(Rules::default, |kind| Rules::of(&self.standard, kind))
    }

    /// The scales view `path` is offered: those of its kind, the usual one first.
    pub(super) fn view_scales(&self, path: &str) -> Vec<u32> {
        self.view_kind(path).map_or(Vec::new(), |kind| kind.scales.clone())
    }

    /// A view was just made: it takes the usual scale of its kind.
    pub(super) fn view_made(&mut self, path: &str) {
        if let Some(scale) = self.view_kind(path).and_then(|kind| kind.scales.first().copied()) {
            self.document.set_view_scale(path, f64::from(scale));
        }
    }

    /// Gives the view the right pane shows the kind of drawing `key`: it takes that kind's usual
    /// scale. A key the standard lacks, or a kind of another kind of view, is not taken.
    pub(super) fn set_view_kind(&mut self, key: &str) {
        let Some(path) = self.properties_view() else { return };
        if !self.view_kinds(&path).iter().any(|kind| kind.key == key) {
            return;
        }
        self.document.set_view_drawing(&path, Some(key.to_string()));
        self.view_made(&path);
        // the kind's own fields, when they stand open under the view's, are those of this kind
        if matches!(self.standard_view.picked, Some(Setting::Kind(_))) {
            self.standard_view = StandardView { picked: Some(Setting::Kind(key.to_string())), ..StandardView::default() };
        }
    }

    /// Sets the scale the view the right pane shows is drawn at in its own tab (1 : `scale`).
    pub(super) fn set_view_scale(&mut self, scale: u32) {
        if let Some(path) = self.properties_view() {
            self.document.set_view_scale(&path, f64::from(scale));
        }
    }

    /// Opens the fields of the kind of drawing of the view the right pane shows, under the
    /// view's own, or closes them.
    pub(super) fn edit_view_kind(&mut self, open: bool) {
        let kind = self.properties_view().and_then(|path| self.view_kind(&path).map(|kind| kind.key.clone()));
        self.standard_view = StandardView { picked: kind.filter(|_| open).map(Setting::Kind), ..StandardView::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::super::layers::tests::{app_with_plan, shows};
    use super::super::standards::{Field, StandardMessage};
    use super::super::Message;
    use super::*;
    use crate::drawing::Cut;
    use crate::drawing2d::{sheet_drawing, view_drawing, Pen};
    use std::sync::Arc;

    /// A cut with two cut lines, one edge beyond, one hidden edge and a cut face.
    fn cut() -> Cut {
        Cut {
            segments: vec![[[0.0, 0.0], [4.0, 0.0]], [[4.0, 0.0], [4.0, 3.0]]],
            below: vec![[[1.0, 1.0], [3.0, 1.0]]],
            hidden: vec![[[0.0, 2.0], [4.0, 2.0]]],
            extent: Some(([0.0, 0.0], [4.0, 3.0])),
            ..Cut::default()
        }
    }

    #[test]
    fn a_view_is_the_kind_of_drawing_usual_for_it_until_it_is_given_another() {
        let (mut app, plan) = app_with_plan();
        assert_eq!(app.view_kind(&plan).map(|kind| kind.key.as_str()), Some("floor-plan"));
        // the kinds it can be are those seen from above; a section's are sections
        let keys: Vec<&str> = app.view_kinds(&plan).iter().map(|kind| kind.key.as_str()).collect();
        assert!(keys.contains(&"floor-plan") && keys.contains(&"site-plan") && !keys.contains(&"section") && !keys.contains(&"ceiling-plan"), "{keys:?}");
        let section = app.document.add_section("Section A", [0.0, 0.0], [5.0, 0.0], 10.0).unwrap();
        let elevation = app.document.add_section_of(SectionKind::Elevation, "North", [5.0, 6.0], [0.0, 6.0], 10.0).unwrap();
        assert_eq!((app.view_kind(&section).map(|kind| kind.key.as_str()), app.view_kind(&elevation).map(|kind| kind.key.as_str())), (Some("section"), Some("elevation")));
        assert!(app.view_kinds(&section).iter().any(|kind| kind.key == "detail"));
        // given another kind, it takes that kind's usual scale; a kind of another view is not taken
        let _ = app.update(Message::ViewKind("site-plan".into()));
        assert_eq!((app.document.view_drawing(&plan), app.document.view_scale(&plan)), (Some("site-plan"), Some(500.0)));
        let _ = app.update(Message::ViewKind("section".into()));
        assert_eq!(app.view_kind(&plan).map(|kind| kind.key.as_str()), Some("site-plan"));
        let _ = app.update(Message::ViewScale(200));
        assert_eq!(app.document.view_scale(&plan), Some(200.0));
        // it is kept in the file, and read back
        let nodes = app.document.ifcx_drawing_nodes();
        let text = serde_json::to_string(&nodes).unwrap();
        assert!(text.contains(r#""drawing":"site-plan""#), "{text}");
        // a kind the standard no longer has: the usual one again
        app.standard.kinds.retain(|kind| kind.key != "site-plan");
        assert_eq!(app.view_kind(&plan).map(|kind| kind.key.as_str()), Some("floor-plan"));
    }

    #[test]
    fn a_view_made_new_takes_the_usual_scale_of_its_kind() {
        let (mut app, _) = app_with_plan();
        let level = app.document.levels()[1].path.clone();
        // the standard's floor plans are drawn at 1:50
        app.new_plan_of(&level, PlanKind::Floor, "");
        let plan = app.document.plans().last().unwrap().path.clone();
        assert_eq!(app.document.view_scale(&plan), Some(50.0));
        // placed on a sheet, it starts at its own scale and is offered those of its kind
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet.clone(), Arc::from(plan.as_str())));
        assert_eq!(app.document.sheet(&sheet).unwrap().views[0].scale, 50.0);
        assert_eq!(app.view_scales(&plan), [50, 100]);
    }

    #[test]
    fn a_drawing_is_drawn_by_the_rules_of_its_kind() {
        let (mut app, plan) = app_with_plan();
        // a floor plan of the Dutch standard: cut 0.5, beyond 0.25, dashes of 12 and 3 line
        // widths, text 2.5 mm, hatched
        let rules = app.view_rules(&plan);
        assert_eq!((rules.cut, rules.beyond, rules.hidden.clone(), rules.text, rules.hatched), (0.5, 0.25, vec![3.0, 0.75], 0.0025, true));
        let widths = |app: &App, pen: Pen| view_drawing(&app.document, &plan, &cut(), &app.view_rules(&plan)).unwrap().widths(pen);
        assert_eq!((widths(&app, Pen::Cut), widths(&app, Pen::Beyond)), (vec![0.5], vec![0.25]));
        // the kind's pens changed in the standard: the drawing follows
        let _ = app.update(Message::Standard(StandardMessage::Pick(Setting::Kind("floor-plan".into()))));
        let _ = app.update(Message::Standard(StandardMessage::Set(Field::Cut, "0.7".into())));
        let _ = app.update(Message::Standard(StandardMessage::Set(Field::Beyond, "0.35".into())));
        assert_eq!((widths(&app, Pen::Cut), widths(&app, Pen::Beyond)), (vec![0.7], vec![0.35]));
        // what is hidden in the kind's line style: a continuous one draws the edge whole
        let hidden = |app: &App| view_drawing(&app.document, &plan, &cut(), &app.view_rules(&plan)).unwrap().lines.iter().filter(|line| line.pen == Pen::Beyond && line.a[1] == 2.0).count();
        let dashed = hidden(&app);
        // 4 m on paper at the plan's scale, in dashes of 4.2 mm and gaps of 1.05 mm: a dash to
        // every 5.25 mm begun
        let on_paper = 4000.0 / app.document.view_scale(&plan).unwrap();
        assert_eq!(dashed, (on_paper / 5.25).ceil() as usize, "{on_paper} mm on paper");
        let _ = app.update(Message::Standard(StandardMessage::Set(Field::Hidden, "continuous".into())));
        assert_eq!(hidden(&app), 1);
        // its texts and dimensions as high as the kind's text style, on paper
        app.document.add_dimension(&plan, [0.0, 0.0], [4.0, 0.0]);
        let _ = app.update(Message::Standard(StandardMessage::Set(Field::Text, "3.5".into())));
        let drawing = view_drawing(&app.document, &plan, &cut(), &app.view_rules(&plan)).unwrap();
        let scale = app.document.view_scale(&plan).unwrap();
        assert!((drawing.dimensions[0].text_height - 0.0035 * scale).abs() < 1e-12, "{}", drawing.dimensions[0].text_height);
        // another view of another kind keeps its own on the same sheet
        let section = app.document.add_section("Section A", [0.0, 0.0], [5.0, 0.0], 10.0).unwrap();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = app.document.sheets()[0].path.clone();
        app.document.place_plan(&sheet, &plan);
        app.document.place_plan(&sheet, &section);
        let both = Arc::new(cut());
        let drawing = sheet_drawing(&app.document, &sheet, &|_| Some(both.clone()), &|_| Vec::new(), &|path| app.view_rules(path)).unwrap();
        assert_eq!(drawing.widths(Pen::Cut), [0.5, 0.7], "the section's and the floor plan's");
    }

    #[test]
    fn the_view_section_picks_the_kind_and_opens_its_fields() {
        let (mut app, plan) = app_with_plan();
        assert!(shows(&app, "Kind of drawing") && shows(&app, "Edit the kind") && shows(&app, "0.5 / 0.25 mm"));
        assert!(!shows(&app, "Drawn at"), "the kind's fields are closed");
        let _ = app.update(Message::ViewKindEdit(true));
        assert_eq!(app.standard_view.picked, Some(Setting::Kind("floor-plan".into())));
        assert!(shows(&app, "Close the kind") && shows(&app, "Drawn at"));
        // another kind picked while they are open: they are that kind's
        let _ = app.update(Message::ViewKind("fire-plan".into()));
        assert_eq!((app.document.view_drawing(&plan), app.standard_view.picked.clone()), (Some("fire-plan"), Some(Setting::Kind("fire-plan".into()))));
        let _ = app.update(Message::ViewKindEdit(false));
        assert_eq!(app.standard_view.picked, None);
    }
}
