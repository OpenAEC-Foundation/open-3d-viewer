//! What a structural plan says by itself: the labels of its beams and columns and the numbers of
//! its piles, made again from the model after every change (see `App::tag_plans`).
//!
//! - **A beam**: its profile, its strength class and the top of the beam to the plan's level in
//!   millimetres (`HEA 200 S355 b.k. +2800`), along the beam, over its line.
//! - **A column**: its profile and its strength class, beside it.
//! - **A pile**: a column that stands wholly under its level. The piles of a level are numbered
//!   row by row, from the top of the drawing down and from left to right; each has its number
//!   to its lower right. [`piles`] gives them in that order, for the plan and for the pile schedule.
//!
//! The pile schedule is a schedule of the project beside those a file brings: per pile its level,
//! number, place, profile, strength class, cut-off level, tip and length. It follows the model.

use crate::document::{Align, Note, NOTE_HEIGHT};
use bim_core::{ElementId, LinearDef, Model, Shape};
use glam::DVec2;

/// Piles as near in y as this (metres) stand in one row.
const ROW: f64 = 0.05;

/// A pile of a level: where it stands, the height of its head and its tip (metres, project
/// coordinates), and what it is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Pile {
    pub id: ElementId,
    pub number: usize,
    pub at: DVec2,
    pub head: f64,
    pub tip: f64,
    pub profile: String,
    pub grade: Option<String>,
}

/// How far a profile reaches to its sides and up from its axis (metres); none for a profile the
/// kernel refuses.
fn reach(l: &LinearDef) -> (f64, f64) {
    l.profile.profile().map_or((0.0, 0.0), |p| p.outer().iter().fold((0.0, 0.0), |(x, y): (f64, f64), q| (x.max(q.x.abs()), y.max(q.y))))
}

/// The linear elements of `level` (those on no level with them), in project coordinates.
fn members<'a>(model: &'a Model, level: &'a str) -> impl Iterator<Item = (&'a bim_core::Element, LinearDef)> + 'a {
    model.iter().filter(move |e| e.level.as_deref().is_none_or(|l| l == level)).filter_map(|e| match e.shape_in_project(&model.grid) {
        Shape::Linear(l) => Some((e, l)),
        _ => None,
    })
}

/// The piles of `level`, which lies at `elevation`: its columns whose head is not above it,
/// numbered row by row from the top of the drawing down, each row from left to right.
pub fn piles(model: &Model, level: &str, elevation: f64) -> Vec<Pile> {
    let mut piles: Vec<Pile> = members(model, level)
        .filter(|(e, l)| e.class() == "IfcColumn" && l.start.z.max(l.end.z) <= elevation + 1e-6)
        .map(|(e, l)| Pile { id: e.id, number: 0, at: l.start.truncate(), head: l.start.z.max(l.end.z), tip: l.start.z.min(l.end.z), profile: l.profile.name(), grade: e.grade.clone() })
        .collect();
    // rows by y, within a row by x
    piles.sort_by(|a, b| b.at.y.total_cmp(&a.at.y));
    let mut row_y = f64::MAX;
    let mut rows: Vec<Vec<Pile>> = Vec::new();
    for pile in piles {
        if (row_y - pile.at.y).abs() > ROW {
            row_y = pile.at.y;
            rows.push(Vec::new());
        }
        rows.last_mut().unwrap().push(pile);
    }
    let mut numbered = Vec::new();
    for mut row in rows {
        row.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
        numbered.extend(row);
    }
    for (k, pile) in numbered.iter_mut().enumerate() {
        pile.number = k + 1;
    }
    numbered
}

/// A height to a level in millimetres, with its sign: `+2800`, `-430`, `+0`.
pub fn to_level(height: f64) -> String {
    let mm = (height * 1000.0).round() as i64;
    if mm < 0 {
        format!("{mm}")
    } else {
        format!("+{mm}")
    }
}

/// A precast plank of a floor laid as a system (a hollow-core slab, a wide slab), as the laying
/// plan marks it.
#[derive(Debug, Clone, PartialEq)]
pub struct Plank {
    /// The floor it is a plank of.
    pub floor: ElementId,
    /// Its mark: planks of one floor type with the same sizes share one, counted from 1.
    pub mark: usize,
    /// The middle of it on the plan.
    pub at: DVec2,
    /// Its length (the way it spans), its width and its thickness, in whole millimetres.
    pub sizes: [i64; 3],
    /// Whether it spans along the project's y axis rather than its x axis.
    pub turned: bool,
}

/// The precast planks of the floors of `level`, marked: the panels of every lying system of a
/// concrete floor type there (what its type's panels are made of; not its joints or its topping), by their boxes along the project's axes (a floor laid askew is measured too large).
/// Marks go to the sizes in the order the planks lie, row by row from the top of the plan.
pub fn planks(model: &Model, library: &bim_library::Library, level: &str) -> Vec<Plank> {
    let mut found = Vec::new();
    for e in model.iter().filter(|e| e.level.as_deref() == Some(level)) {
        let Shape::System(system) = &e.shape else { continue };
        if !system.lies() {
            continue;
        }
        let Ok(parts) = model.evaluate(e.id) else { continue };
        let origin = model.grid.origin(e.tile);
        // the planks are the panels of its type: what they are made of tells them from the rest
        let Some(panel) = system.system_type.as_ref().and_then(|t| library.system_type(&t.id)).filter(|t| t.group == bim_library::SystemGroup::Floor).map(|t| t.panels.material.clone()) else { continue };
        for part in parts.iter().filter(|p| p.material.as_deref() == Some(panel.as_str())) {
            let (lo, hi) = part.solid.bounds();
            let size = hi - lo;
            let turned = size.y > size.x;
            let mm = |v: f64| (v * 1000.0).round() as i64;
            let sizes = if turned { [mm(size.y), mm(size.x), mm(size.z)] } else { [mm(size.x), mm(size.y), mm(size.z)] };
            let type_id = system.system_type.as_ref().map(|t| t.id.clone()).unwrap_or_default();
            found.push((type_id, Plank { floor: e.id, mark: 0, at: ((lo + hi) / 2.0 + origin).truncate(), sizes, turned }));
        }
    }
    found.sort_by(|a, b| b.1.at.y.total_cmp(&a.1.at.y).then(a.1.at.x.total_cmp(&b.1.at.x)));
    let mut marks: Vec<(String, [i64; 3])> = Vec::new();
    found
        .into_iter()
        .map(|(type_id, mut plank)| {
            let key = (type_id, plank.sizes);
            plank.mark = match marks.iter().position(|m| *m == key) {
                Some(k) => k + 1,
                None => {
                    marks.push(key);
                    marks.len()
                }
            };
            plank
        })
        .collect()
}

/// What a structural plan of `level` (at `elevation`, drawn at 1 : `scale`) says by itself;
/// `top_word` is what the top of a beam is called (`b.k.`).
pub fn tags(model: &Model, library: &bim_library::Library, level: &str, elevation: f64, scale: f64, top_word: &str) -> Vec<Note> {
    // a millimetre on paper between a label and what it labels
    let gap = scale * 0.001;
    let note = |at: DVec2, text: String, align: Align, angle: f64| Note { at: at.to_array(), text, size: NOTE_HEIGHT, align, angle, level: false, face: crate::typeface::Face::default() };
    let piles = piles(model, level, elevation);
    let mut notes = Vec::new();
    // a precast floor's planks: each its mark and its length, along the way it spans
    for plank in planks(model, library, level) {
        let angle = if plank.turned { std::f64::consts::FRAC_PI_2 } else { 0.0 };
        notes.push(note(plank.at, format!("P{} {}", plank.mark, plank.sizes[0]), Align::Center, angle));
    }
    for (e, l) in members(model, level) {
        let (side, up) = reach(&l);
        let what = match &e.grade {
            Some(grade) => format!("{} {grade}", l.profile.name()),
            None => l.profile.name(),
        };
        match e.class() {
            "IfcBeam" => {
                // read from the foot or from the right of the sheet: the text runs to the right, or up
                let mut along = (l.end - l.start).truncate().normalize_or_zero();
                if along.x < -1e-9 || (along.x.abs() <= 1e-9 && along.y < 0.0) {
                    along = -along;
                }
                let middle = ((l.start + l.end) / 2.0).truncate();
                let top = l.start.z.max(l.end.z) + up - elevation;
                notes.push(note(middle + along.perp() * (side + gap), format!("{what} {top_word} {}", to_level(top)), Align::Center, along.y.atan2(along.x)));
            }
            "IfcColumn" => {
                let at = l.start.truncate() + DVec2::splat(side + gap);
                match piles.iter().find(|p| p.id == e.id) {
                    // to its lower right, clear of the label of a beam that lies over it
                    Some(pile) => notes.push(note(l.start.truncate() + DVec2::new(side + gap, -(side + gap) - scale * NOTE_HEIGHT), pile.number.to_string(), Align::Left, 0.0)),
                    None => notes.push(note(at, what, Align::Left, 0.0)),
                }
            }
            _ => {}
        }
    }
    notes
}

/// What the bar schedule, its columns and its kinds of bar are called when the interface has not
/// said.
pub const BAR_WORDS: [&str; 11] = ["Bar schedule", "Element", "Mark", "Bar", "Ø (mm)", "Number", "Length (mm)", "Weight (kg)", "top", "bottom", "stirrup"];

/// What the precast schedule and its columns are called when the interface has not said.
pub const PLANK_WORDS: [&str; 8] = ["Precast schedule", "Level", "Mark", "Type", "Length (mm)", "Width (mm)", "Thickness (mm)", "Number"];

/// What the pile schedule and its columns are called when the interface has not said.
pub const PILE_WORDS: [&str; 10] = ["Pile schedule", "Level", "No.", "X (mm)", "Y (mm)", "Pile", "Grade", "Cut-off level", "Pile tip", "Length (mm)"];

impl crate::app::App {
    /// The pile schedule, made again from the model: a row per pile of every level, a click on
    /// which selects the pile. It comes when the first pile is drawn and stays, empty, when the
    /// last one goes, so the schedules keep their places.
    pub(in crate::app) fn list_piles(&mut self) {
        let words = if self.modeller.pile_words[0].is_empty() { PILE_WORDS } else { self.modeller.pile_words };
        let mm = |v: f64| format!("{}", (v * 1000.0).round() as i64);
        // a row selects its pile where the modelled project is shown alone
        let alone = self.models.len() <= 1;
        let mut rows = Vec::new();
        for level in self.document.levels() {
            for pile in piles(&self.modeller.model, &level.path, level.elevation) {
                let cells = [
                    level.name.clone(),
                    pile.number.to_string(),
                    mm(pile.at.x),
                    mm(pile.at.y),
                    pile.profile.clone(),
                    pile.grade.clone().unwrap_or_default(),
                    to_level(pile.head - level.elevation),
                    to_level(pile.tip - level.elevation),
                    mm(pile.head - pile.tip),
                ];
                rows.push(crate::model::ScheduleRow { cells: cells.into_iter().map(Some).collect(), elements: if alone { vec![pile.id.0] } else { Vec::new() }, ..Default::default() });
            }
        }
        let named = std::mem::replace(&mut self.modeller.pile_schedule, words[0].to_string());
        let schedule = crate::model::Schedule { name: words[0].to_string(), title: words[0].to_string(), columns: words[1..].iter().map(|w| w.to_string()).collect(), rows, computed: true };
        match self.schedules.iter_mut().find(|s| !named.is_empty() && s.name == named) {
            Some(listed) => *listed = schedule,
            None if !schedule.rows.is_empty() => self.schedules.push(schedule),
            None => self.modeller.pile_schedule = named,
        }
    }
}

impl crate::app::App {
    /// The precast schedule, made again from the model: a row per mark of the planks of every
    /// level's floors, with how many there are of it. A click on a row selects the floors they
    /// lie in. It comes with the first precast floor and stays, empty, when the last one goes.
    pub(in crate::app) fn list_planks(&mut self) {
        let words = if self.modeller.plank_words[0].is_empty() { PLANK_WORDS } else { self.modeller.plank_words };
        let alone = self.models.len() <= 1;
        let mut rows = Vec::new();
        for level in self.document.levels() {
            let planks = planks(&self.modeller.model, &self.library, &level.path);
            let marks = planks.iter().map(|p| p.mark).max().unwrap_or(0);
            for mark in 1..=marks {
                let same: Vec<&Plank> = planks.iter().filter(|p| p.mark == mark).collect();
                let Some(first) = same.first() else { continue };
                let kind = self.modeller.model.get(first.floor).and_then(|e| match &e.shape {
                    Shape::System(s) => s.system_type.as_ref().map(|t| self.library.system_name(&t.id, bim_library::Lang::En).unwrap_or_else(|| t.id.clone())),
                    _ => None,
                });
                let cells = [level.name.clone(), format!("P{mark}"), kind.unwrap_or_default(), first.sizes[0].to_string(), first.sizes[1].to_string(), first.sizes[2].to_string(), same.len().to_string()];
                let mut floors: Vec<u64> = same.iter().map(|p| p.floor.0).collect();
                floors.dedup();
                rows.push(crate::model::ScheduleRow { cells: cells.into_iter().map(Some).collect(), elements: if alone { floors } else { Vec::new() }, ..Default::default() });
            }
        }
        let named = std::mem::replace(&mut self.modeller.plank_schedule, words[0].to_string());
        let schedule = crate::model::Schedule { name: words[0].to_string(), title: words[0].to_string(), columns: words[1..].iter().map(|w| w.to_string()).collect(), rows, computed: true };
        match self.schedules.iter_mut().find(|s| !named.is_empty() && s.name == named) {
            Some(listed) => *listed = schedule,
            None if !schedule.rows.is_empty() => self.schedules.push(schedule),
            None => self.modeller.plank_schedule = named,
        }
    }

    /// The bar schedule, made again from the model: a row for the top bars, the bottom bars and
    /// the stirrups of every reinforced member, marked 1, 2, 3 … through the project, with what
    /// they weigh. A click on a row selects its member. It comes with the first reinforced
    /// member and stays, empty, when the last one loses its reinforcement.
    pub(in crate::app) fn list_bars(&mut self) {
        use bim_core::reinforcement::BarKind;
        let words = if self.modeller.bar_words[0].is_empty() { BAR_WORDS } else { self.modeller.bar_words };
        let alone = self.models.len() <= 1;
        let mut rows = Vec::new();
        let mut mark = 0;
        for e in self.modeller.model.iter() {
            let Some(Ok(bars)) = crate::app::element_props::bars_of(e) else { continue };
            for bar in bars {
                mark += 1;
                let kind = match bar.kind {
                    BarKind::Top => words[8],
                    BarKind::Bottom => words[9],
                    BarKind::Stirrup => words[10],
                };
                let cells = [
                    e.name.clone(),
                    mark.to_string(),
                    kind.to_string(),
                    format!("{}", (bar.diameter * 1000.0).round() as i64),
                    bar.count.to_string(),
                    format!("{}", (bar.length * 1000.0).round() as i64),
                    format!("{:.1}", bar.weight()),
                ];
                rows.push(crate::model::ScheduleRow { cells: cells.into_iter().map(Some).collect(), elements: if alone { vec![e.id.0] } else { Vec::new() }, ..Default::default() });
            }
        }
        let named = std::mem::replace(&mut self.modeller.bar_schedule, words[0].to_string());
        let schedule = crate::model::Schedule { name: words[0].to_string(), title: words[0].to_string(), columns: words[1..8].iter().map(|w| w.to_string()).collect(), rows, computed: true };
        match self.schedules.iter_mut().find(|s| !named.is_empty() && s.name == named) {
            Some(listed) => *listed = schedule,
            None if !schedule.rows.is_empty() => self.schedules.push(schedule),
            None => self.modeller.bar_schedule = named,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_core::{Element, ProfileDef, TileGrid};
    use glam::DVec3;

    fn model() -> Model {
        let grid = TileGrid::default();
        let mut model = Model::new(grid);
        let mut add = |name: &str, grade: Option<&str>, start: DVec3, end: DVec3, profile: ProfileDef| {
            let mut e = Element::in_project(&grid, name, Some("ground".into()), Shape::Linear(LinearDef { start, end, profile, roll: 0.0 }));
            e.grade = grade.map(str::to_string);
            model.add(e).unwrap()
        };
        let pile = |x: f64, y: f64| (DVec3::new(x, y, -18.93), DVec3::new(x, y, -0.93));
        let round = || ProfileDef::Circle { diameter: 0.3 };
        // two rows of piles, given out of order
        for (x, y) in [(6.0, 0.0), (0.0, 5.0), (0.0, 0.0), (6.0, 5.02)] {
            let (a, b) = pile(x, y);
            add("Pile", Some("C35/45"), a, b, round());
        }
        // a foundation beam over the lower row, its top 430 under the level, drawn from right to left
        add("Beam", Some("C30/37"), DVec3::new(6.0, 0.0, -0.68), DVec3::new(0.0, 0.0, -0.68), ProfileDef::Rectangle { width: 0.4, height: 0.5 });
        // a beam running up the sheet and a column standing on the level
        add("Beam", None, DVec3::new(3.0, 0.0, 2.6), DVec3::new(3.0, 5.0, 2.6), ProfileDef::Rectangle { width: 0.2, height: 0.4 });
        add("Column", Some("S355"), DVec3::new(3.0, 2.0, 0.0), DVec3::new(3.0, 2.0, 3.0), ProfileDef::steel("HEA 200").unwrap());
        model
    }

    #[test]
    fn piles_are_the_columns_under_their_level_numbered_row_by_row() {
        let piles = piles(&model(), "ground", 0.0);
        let found: Vec<(usize, f64, f64)> = piles.iter().map(|p| (p.number, p.at.x, p.at.y)).collect();
        assert_eq!(found, [(1, 0.0, 5.0), (2, 6.0, 5.02), (3, 0.0, 0.0), (4, 6.0, 0.0)]);
        assert_eq!((piles[0].head, piles[0].tip, piles[0].profile.as_str(), piles[0].grade.as_deref()), (-0.93, -18.93, "Ø300", Some("C35/45")));
        // of another level: none
        assert!(super::piles(&model(), "first", 3.0).is_empty());
    }

    #[test]
    fn a_structural_plan_labels_its_beams_and_columns_and_numbers_its_piles() {
        let notes = tags(&model(), &bim_library::Library::starter(), "ground", 0.0, 100.0, "b.k.");
        let text = |t: &str| notes.iter().find(|n| n.text == t).unwrap_or_else(|| panic!("{t}: {:?}", notes.iter().map(|n| &n.text).collect::<Vec<_>>()));
        // the foundation beam: over its line (0.2 to its side and a millimetre on paper), read from the foot
        let beam = text("400×500 C30/37 b.k. -430");
        assert!((beam.at[0] - 3.0).abs() < 1e-9 && (beam.at[1] - 0.3).abs() < 1e-9 && beam.angle == 0.0 && beam.align == Align::Center, "{beam:?}");
        // the beam running up: read from the right, its text on its left
        let up = text("200×400 b.k. +2800");
        assert!((up.at[0] - 2.8).abs() < 1e-9 && (up.at[1] - 2.5).abs() < 1e-9 && (up.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{up:?}");
        let column = text("HEA 200 S355");
        assert!((column.at[0] - 3.2).abs() < 1e-6 && column.align == Align::Left, "{column:?}");
        // the piles say their numbers, not what they are
        for number in ["1", "2", "3", "4"] {
            text(number);
        }
        // to the lower right of the pile, under its foot by the height of its letters
        let first = text("1");
        assert!((first.at[0] - 0.25).abs() < 1e-9 && (first.at[1] - (5.0 - 0.25 - 0.25)).abs() < 1e-9, "{first:?}");
        assert_eq!(notes.len(), 7);
        assert_eq!((to_level(0.0), to_level(-0.0004), to_level(2.8)), ("+0".to_string(), "+0".to_string(), "+2800".to_string()));
    }

    #[test]
    fn the_pile_schedule_follows_the_model() {
        use crate::app::modeller::ModelTool;
        use crate::app::placement::{PlaceField, PlaceMessage};
        use crate::app::{App, Message};
        let mut app = App::default();
        app.new_model("Level 0");
        assert!(app.schedules.is_empty());
        // a round concrete column from 18.93 under the level, 18 long: a pile
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        for message in [PlaceMessage::Kind(Some("column.concrete-round-300".into())), PlaceMessage::Grade(Some("C35/45".into())), PlaceMessage::Field(PlaceField::Offset, "-18.93".into()), PlaceMessage::Field(PlaceField::Height, "18".into())] {
            let _ = app.update(Message::Place(message));
        }
        let level = app.document.levels()[0].path.clone();
        for (x, y) in [(2.0, 0.0), (0.0, 0.0)] {
            app.model_point(ModelTool::Column, "Column", Some(level.clone()), DVec3::new(x, y, 0.0));
        }
        let [schedule] = &app.schedules[..] else { panic!("{:?}", app.schedules) };
        assert_eq!((schedule.name.as_str(), schedule.columns.len(), schedule.rows.len()), ("Pile schedule", 9, 2));
        let cells = |row: usize| schedule.rows[row].cells.iter().map(|c| c.clone().unwrap()).collect::<Vec<_>>();
        assert_eq!(cells(0), ["Level 0", "1", "0", "0", "Concrete round 300", "C35/45", "-930", "-18930", "18000"]);
        assert_eq!(cells(1)[1..4], ["2", "2000", "0"]);
        // a structural plan says their numbers
        let _ = app.update(Message::NewPlanOf(level.into(), crate::document::PlanKind::Structural, "structure"));
        let plan = app.document.plans()[0].path.clone();
        let tags: Vec<&str> = app.document.view_tags(&plan).iter().map(|n| n.text.as_str()).collect();
        assert_eq!(tags, ["2", "1"]);
        // the piles gone: the schedule stays, empty
        for id in [1, 2] {
            app.select_modelled(ElementId(id));
            let _ = app.update(Message::DeletePicked);
        }
        assert_eq!((app.schedules.len(), app.schedules[0].rows.len()), (1, 0));
    }

    #[test]
    fn a_reinforced_beam_lists_its_bars_and_loses_them_again() {
        use crate::app::{App, Message};
        use bim_core::{LinearDef, Shape};
        let mut app = App::default();
        let _ = app.update(Message::NewModel("Level 0"));
        let level = app.work_plane().2;
        let beam = Shape::Linear(LinearDef { start: DVec3::new(0.0, 0.0, 3.0), end: DVec3::new(6.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 });
        let id = app.add_element("Beam", level.clone(), beam).unwrap();
        let steel = app.add_element("Steel", level, Shape::Linear(LinearDef { start: DVec3::new(0.0, 4.0, 3.0), end: DVec3::new(6.0, 4.0, 3.0), profile: ProfileDef::steel("IPE 300").unwrap(), roll: 0.0 })).unwrap();
        assert!(app.schedules.iter().all(|s| s.name != "Bar schedule"));
        let select = |app: &mut App, id: bim_core::ElementId| {
            let scene = app.model.clone().unwrap();
            let shown = scene.element_with_key(crate::model::ElementKey { model: 0, source: id.0 }).unwrap();
            app.sel.set(Some(crate::selection::Pickable::Element(shown)));
        };
        select(&mut app, id);
        let _ = app.update(Message::Reinforce(true));
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let listed = app.schedules.iter().find(|s| s.name == "Bar schedule").expect("the bar schedule");
        let cells: Vec<Vec<String>> = listed.rows.iter().map(|r| r.cells.iter().map(|c| c.clone().unwrap_or_default()).collect()).collect();
        assert_eq!(cells[0], ["Beam 1", "1", "top", "12", "2", "5940", "10.5"]);
        assert_eq!(cells[1][..6], ["Beam 1", "2", "bottom", "16", "3", "5940"]);
        assert_eq!(cells[2][..6], ["Beam 1", "3", "stirrup", "8", "40", "1488"]);
        assert_eq!(listed.rows[0].elements, vec![id.0]);
        // an I section takes none
        select(&mut app, steel);
        let _ = app.update(Message::Reinforce(true));
        assert_eq!(app.modeller.refusal, Some(bim_core::Refusal::NotRectangular));
        assert!(app.modeller.model.get(steel).unwrap().reinforcement.is_none());
        // taken away again: the schedule stays, empty
        select(&mut app, id);
        let _ = app.update(Message::Reinforce(false));
        assert!(app.schedules.iter().find(|s| s.name == "Bar schedule").unwrap().rows.is_empty());
    }

    #[test]
    fn a_hollow_core_floor_is_marked_plank_by_plank_and_listed() {
        use crate::app::{App, Message};
        use bim_core::SystemDef;
        use glam::DVec2;
        let mut app = App::default();
        let _ = app.update(Message::NewModel("Level 0"));
        let level = app.work_plane().2;
        let path = level.clone().unwrap();
        // 4.2 m wide (three planks of 1200 and a narrow one) spanning 6 m
        let t = app.library.system_type("system.hollow-core-200").unwrap().clone();
        let generic = SystemDef { start: DVec2::ZERO, end: DVec2::new(4.2, 0.0), base: t.lying.unwrap(), height: 6.0, spacing: DVec2::splat(0.6), member: DVec2::new(0.045, 0.14), panel: 0.012, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new() };
        let floor = app.add_element("Floor", level, Shape::System(generic.clone().laid(&t).unwrap())).unwrap();
        // a second floor beside it, spanning 4 m: other planks, another mark
        let short = SystemDef { start: DVec2::new(5.0, 0.0), end: DVec2::new(7.4, 0.0), height: 4.0, ..generic.clone() };
        app.add_element("Floor", app.work_plane().2, Shape::System(short.laid(&t).unwrap())).unwrap();
        let planks = planks(&app.modeller.model, &app.library, &path);
        // the system lays the first in four equal planks, 20 mm of grout between them
        let first: Vec<&Plank> = planks.iter().filter(|p| p.floor == floor).collect();
        assert_eq!(first.len(), 4, "{planks:?}");
        assert!(first.iter().all(|p| p.sizes == [5980, 1030, 200] && p.turned && p.mark == first[0].mark), "{planks:?}");
        let other: Vec<&Plank> = planks.iter().filter(|p| p.floor != floor).collect();
        assert!(!other.is_empty() && other.iter().all(|p| p.sizes[0] == 3980 && p.mark != first[0].mark), "{planks:?}");
        assert_eq!(planks.iter().map(|p| p.mark).max(), Some(2));
        // a wide-slab floor: its planks of 60 count, its joints and its topping do not
        let wide = app.library.system_type("system.wide-slab-200").unwrap().clone();
        let slab = SystemDef { start: DVec2::new(10.0, 0.0), end: DVec2::new(14.8, 0.0), ..generic.clone() };
        let slab = app.add_element("Floor", app.work_plane().2, Shape::System(slab.laid(&wide).unwrap())).unwrap();
        let planks = super::planks(&app.modeller.model, &app.library, &path);
        let wide: Vec<&Plank> = planks.iter().filter(|p| p.floor == slab).collect();
        assert!(wide.len() == 2 && wide.iter().all(|p| p.sizes[2] == 60 && p.sizes[1] > 2000), "{wide:?}");
        // the plan says each plank's mark and length; the schedule counts them
        let notes = tags(&app.modeller.model, &app.library, &path, 0.0, 100.0, "top");
        assert_eq!(notes.iter().filter(|n| n.text.starts_with('P')).count(), planks.len());
        let listed = app.schedules.iter().find(|s| s.name == "Precast schedule").expect("the precast schedule");
        let number: usize = listed.rows.iter().map(|r| r.cells[6].as_deref().unwrap().parse::<usize>().unwrap()).sum();
        assert_eq!(number, planks.len());
        assert_eq!(listed.rows.iter().map(|r| r.cells[1].as_deref().unwrap()).collect::<Vec<_>>(), ["P1", "P2", "P3"]);
        assert!(listed.rows.iter().any(|r| r.elements == vec![floor.0]));
    }
}
