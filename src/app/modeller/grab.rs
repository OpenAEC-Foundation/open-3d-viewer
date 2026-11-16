//! G, as in Blender: the selected elements are grabbed and follow the cursor at once.
//!
//! - **G** with modelled elements selected (outside edit mode, where G grabs points) starts the
//!   grab. Where the cursor is then is where the move starts from; the elements move as far as the
//!   cursor goes from there, on the level worked on, rounded to 10 mm, and show where they would
//!   stand.
//! - **X** or **Y** holds the move to that axis (the same key again lets it go).
//! - **Digits** type the distance along the axis held, in metres (a point or a comma, a minus
//!   in front); while a distance is typed the cursor moves nothing. Backspace takes a character
//!   back.
//! - **A click or Enter** puts the elements down: one step to take back. **Esc** puts them back
//!   where they were.
//! - The walls joined to a moved wall follow, as with Move; what the kernel refuses stays where it
//!   was and the banner says why.

use crate::app::edit::Axis;
use crate::app::transform::Map;
use crate::app::App;
use bim_core::{ElementId, Shape};
use glam::DVec3;
use openaec_ui::Language;
use std::sync::Arc;

/// A grabbed move is rounded to this (metres).
const ROUND: f64 = 0.01;

/// A grab under way.
#[derive(Debug, Clone)]
pub struct ObjectGrab {
    pub elements: Vec<ElementId>,
    /// The model as it was when the grab began: every move is made from it.
    before: Arc<bim_core::Model>,
    /// Where the cursor was when it first came over the view (scene coordinates).
    origin: Option<DVec3>,
    /// How far the elements have gone.
    pub delta: DVec3,
    pub axis: Option<Axis>,
    /// The distance being typed.
    pub typed: String,
}

impl App {
    /// G: the selected modelled elements are grabbed. False when none is selected (or only
    /// connections, which have no place of their own).
    pub(in crate::app) fn begin_grab_objects(&mut self) -> bool {
        let grid = self.modeller.model.grid;
        let elements: Vec<ElementId> = self
            .selected_modelled()
            .into_iter()
            .filter(|id| self.modeller.model.get(*id).is_some_and(|e| !matches!(e.shape_in_project(&grid), Shape::Connection(_))))
            .collect();
        if elements.is_empty() {
            return false;
        }
        if self.modeller.tool.is_some() {
            self.set_model_tool(None);
        }
        self.modeller.transform = None;
        self.modeller.refusal = None;
        self.modeller.grab = Some(ObjectGrab { elements, before: Arc::new(self.modeller.model.clone()), origin: None, delta: DVec3::ZERO, axis: None, typed: String::new() });
        true
    }

    /// The elements as the grab has them now: from where they were, moved by its delta.
    fn show_grabbed(&mut self) {
        let Some(grab) = self.modeller.grab.clone() else { return };
        self.modeller.model = (*grab.before).clone();
        if grab.delta == DVec3::ZERO {
            self.modeller.refusal = None;
            self.show_modelled();
            if let [one] = grab.elements[..] {
                self.select_modelled(one);
            }
        } else {
            self.modeller.refusal = None;
            self.transform_elements(&grab.elements, &Map::Shift(grab.delta), false);
        }
    }

    /// The move the grab's typed distance gives, along the axis held; `None` when nothing is
    /// typed, it is no number yet, or no axis is held.
    fn typed_grab(&self) -> Option<DVec3> {
        let grab = self.modeller.grab.as_ref()?;
        let distance = crate::document::parse_elevation(&grab.typed)?;
        Some(match grab.axis? {
            Axis::X => DVec3::X,
            Axis::Y => DVec3::Y,
            Axis::Z => DVec3::Z,
        } * distance)
    }

    /// The cursor rests at (x, y) while elements are grabbed: they follow it. False when no grab
    /// is under way.
    pub(in crate::app) fn grab_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some(grab) = self.modeller.grab.as_ref() else { return false };
        // a distance typed says how far: the cursor does not
        if !grab.typed.is_empty() {
            return true;
        }
        let (z, ..) = self.work_plane();
        let Some(p) = self.plane_point(x, y, width.max(1.0), height.max(1.0), z) else { return true };
        let Some(grab) = self.modeller.grab.as_mut() else { return true };
        let origin = *grab.origin.get_or_insert(p);
        let round = |v: f64| (v / ROUND).round() * ROUND;
        let mut delta = DVec3::new(round(p.x - origin.x), round(p.y - origin.y), 0.0);
        match grab.axis {
            Some(Axis::X) => delta.y = 0.0,
            Some(Axis::Y) => delta.x = 0.0,
            // up and down is typed: the cursor moves on the level
            Some(Axis::Z) => delta = DVec3::ZERO,
            None => {}
        }
        if delta != grab.delta {
            grab.delta = delta;
            self.show_grabbed();
        }
        true
    }

    /// A key while elements are grabbed: X, Y or Z holds the move to that axis, a digit, a point,
    /// a comma or a minus types the distance along it. False when no grab is under way.
    pub(in crate::app) fn grab_key(&mut self, key: char) -> bool {
        let Some(grab) = self.modeller.grab.as_mut() else { return false };
        match key.to_ascii_lowercase() {
            c @ ('x' | 'y' | 'z') => {
                let axis = match c {
                    'x' => Axis::X,
                    'y' => Axis::Y,
                    _ => Axis::Z,
                };
                grab.axis = if grab.axis == Some(axis) { None } else { Some(axis) };
                match grab.axis {
                    Some(Axis::X) => grab.delta.y = 0.0,
                    Some(Axis::Y) => grab.delta.x = 0.0,
                    _ => {}
                }
            }
            c => {
                let fits = match c {
                    '-' => grab.typed.is_empty(),
                    '.' | ',' => !grab.typed.contains(['.', ',']),
                    c => c.is_ascii_digit(),
                };
                if !fits {
                    return true;
                }
                grab.typed.push(c);
            }
        }
        if let Some(delta) = self.typed_grab() {
            if let Some(grab) = self.modeller.grab.as_mut() {
                grab.delta = delta;
            }
        }
        self.show_grabbed();
        true
    }

    /// Backspace while a distance is typed in a grab: its last character goes. False when no grab
    /// is under way.
    pub(in crate::app) fn grab_backspace(&mut self) -> bool {
        let Some(grab) = self.modeller.grab.as_mut() else { return false };
        if grab.typed.pop().is_some() {
            let delta = self.typed_grab().unwrap_or(DVec3::ZERO);
            if let Some(grab) = self.modeller.grab.as_mut() {
                grab.delta = delta;
            }
            self.show_grabbed();
        }
        true
    }

    /// A click or Enter while elements are grabbed: they are put down where they are. False when
    /// no grab is under way.
    pub(in crate::app) fn put_grab_down(&mut self) -> bool {
        let Some(grab) = self.modeller.grab.take() else { return false };
        // what the kernel refused left the model as it was; else it is the move, one step
        self.modeller.touched = true;
        if let [one] = grab.elements[..] {
            self.select_modelled(one);
        }
        true
    }

    /// Esc while elements are grabbed: they go back where they were. False when no grab is under
    /// way.
    pub(in crate::app) fn drop_grab(&mut self) -> bool {
        let Some(grab) = self.modeller.grab.as_mut() else { return false };
        grab.delta = DVec3::ZERO;
        self.show_grabbed();
        self.modeller.grab = None;
        self.modeller.refusal = None;
        true
    }

    /// The banner while elements are grabbed.
    pub(in crate::app) fn grab_hint(&self, l: Language) -> Option<String> {
        let grab = self.modeller.grab.as_ref()?;
        let axis = match grab.axis {
            Some(Axis::X) => " · X",
            Some(Axis::Y) => " · Y",
            Some(Axis::Z) => " · Z",
            None => "",
        };
        let typed = if grab.typed.is_empty() { String::new() } else { format!(" · {} m", grab.typed) };
        let hint = l.text(
            "Pakken: beweeg de muis · klik of Enter: neerzetten · X, Y, Z: as, daarna een afstand typen · Esc: annuleren",
            "Grab: move the mouse · click or Enter: put down · X, Y, Z: axis, then type a distance · Esc: cancel",
        );
        Some(match self.modeller.refusal {
            Some(refusal) => format!("{}{axis}{typed} · {}", l.translate(refusal.reason()), hint),
            None => format!("{hint}{axis}{typed}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::click;
    use super::super::ModelTool;
    use super::*;
    use crate::app::edit::edit_points;
    use crate::app::modeller::keys::ModelKey;
    use crate::app::Message;
    use crate::viewport::Navigate;
    use glam::DVec2;

    /// A new model in the Modeller, seen from above, with a wall from (0, 0) to (4, 0), selected.
    fn wall_selected() -> App {
        let mut app = super::super::tests::modelling();
        app.modeller.in_modeller = true;
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        app.set_model_tool(None);
        app.keep_model_history();
        app.select_modelled(ElementId(1));
        app
    }

    fn hover(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
    }

    fn ends(app: &App, id: u64) -> (DVec2, DVec2) {
        let (p, _) = edit_points(&app.modeller.model.get(ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid));
        (p[0].truncate(), p[1].truncate())
    }

    fn near(a: (DVec2, DVec2), b: [(f64, f64); 2]) -> bool {
        (a.0 - DVec2::new(b[0].0, b[0].1)).length() < 1e-9 && (a.1 - DVec2::new(b[1].0, b[1].1)).length() < 1e-9
    }

    #[test]
    fn g_grabs_the_selection_which_follows_the_cursor_until_a_click() {
        let mut app = wall_selected();
        let _ = app.update(Message::ModelKey(ModelKey::Grab, Language::English));
        assert!(app.modeller.grab.is_some() && app.modeller.transform.is_none(), "a grab, not Move's two clicks");
        // where the cursor first is, is where the move starts; from there it follows
        hover(&mut app, 6.0, 5.0);
        assert!(near(ends(&app, 1), [(0.0, 0.0), (4.0, 0.0)]));
        hover(&mut app, 7.0, 7.5);
        assert!(near(ends(&app, 1), [(1.0, 2.5), (5.0, 2.5)]), "{:?}", ends(&app, 1));
        hover(&mut app, 6.5, 6.0);
        assert!(near(ends(&app, 1), [(0.5, 1.0), (4.5, 1.0)]), "always from where it was, not step on step");
        // a click puts it down: the grab ends, one step to take back
        click(&mut app, 6.5, 6.0);
        assert!(app.modeller.grab.is_none());
        assert!(near(ends(&app, 1), [(0.5, 1.0), (4.5, 1.0)]));
        assert_eq!(app.selected_modelled(), vec![ElementId(1)]);
        let _ = app.update(Message::Undo);
        assert!(near(ends(&app, 1), [(0.0, 0.0), (4.0, 0.0)]), "one step back: {:?}", ends(&app, 1));
    }

    #[test]
    fn an_axis_holds_the_move_a_typed_distance_sets_it_and_esc_puts_it_back() {
        let mut app = wall_selected();
        let _ = app.update(Message::ModelKey(ModelKey::Grab, Language::English));
        hover(&mut app, 2.0, 2.0);
        // held to y: the cursor's way along x counts for nothing
        let _ = app.update(Message::EditKey('y'));
        hover(&mut app, 5.0, 3.5);
        assert!(near(ends(&app, 1), [(0.0, 1.5), (4.0, 1.5)]), "{:?}", ends(&app, 1));
        // 2.4 typed: that far along y, whatever the cursor does; Backspace takes a character back
        for c in "2,45".chars() {
            let _ = app.update(Message::EditKey(c));
        }
        let _ = app.update(Message::DeletePicked);
        hover(&mut app, 9.0, 9.0);
        assert!(near(ends(&app, 1), [(0.0, 2.4), (4.0, 2.4)]), "{:?}", ends(&app, 1));
        assert!(app.grab_hint(Language::English).unwrap().ends_with(" · Y · 2,4 m"));
        // Esc: back where it was, nothing to take back
        let _ = app.update(Message::Escape);
        assert!(app.modeller.grab.is_none());
        assert!(near(ends(&app, 1), [(0.0, 0.0), (4.0, 0.0)]));
        assert_eq!(app.selected_modelled(), vec![ElementId(1)], "it stays selected");
        // again, held to x and typed, put down with Enter
        let _ = app.update(Message::ModelKey(ModelKey::Grab, Language::English));
        for c in "x-1.5".chars() {
            let _ = app.update(Message::EditKey(c));
        }
        let _ = app.update(Message::MeasureFinish);
        assert!(app.modeller.grab.is_none());
        assert!(near(ends(&app, 1), [(-1.5, 0.0), (2.5, 0.0)]), "{:?}", ends(&app, 1));
        let _ = app.update(Message::Undo);
        assert!(near(ends(&app, 1), [(0.0, 0.0), (4.0, 0.0)]));
    }

    #[test]
    fn with_nothing_selected_g_grabs_nothing() {
        let mut app = wall_selected();
        let _ = app.update(Message::Escape);
        assert!(app.selected_modelled().is_empty());
        let _ = app.update(Message::ModelKey(ModelKey::Grab, Language::English));
        assert!(app.modeller.grab.is_none());
    }
}
