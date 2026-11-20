//! What a structural plan says by itself: the labels of its beams and columns and the numbers of
//! its piles, made again from the model after every change (see `App::tag_plans`).
//!
//! - **A beam**: its profile, its strength class and the top of the beam to the plan's level in
//!   millimetres (`HEA 200 S355 b.k. +2800`), along the beam, over its line.
//! - **A column**: its profile and its strength class, beside it.
//! - **A pile**: a registered member with an issued project-wide number. Legacy columns below
//!   their level receive numbers in row order the first time they are read.
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
    pub number: u64,
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
        .filter(|(e, l)| e.pile_number.is_some() || (e.class() == "IfcColumn" && l.start.z.max(l.end.z) <= elevation + 1e-6))
        .map(|(e, l)| Pile { id: e.id, number: e.pile_number.unwrap_or(0), at: l.start.truncate(), head: l.start.z.max(l.end.z), tip: l.start.z.min(l.end.z), profile: l.profile.name(), grade: e.grade.clone() })
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
        if pile.number == 0 { pile.number = k as u64 + 1; }
    }
    numbered.sort_by_key(|pile| pile.number);
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

/// What a structural plan of `level` (at `elevation`, drawn at 1 : `scale`) says by itself;
/// `top_word` is what the top of a beam is called (`b.k.`).
pub fn tags(model: &Model, level: &str, elevation: f64, scale: f64, top_word: &str) -> Vec<Note> {
    member_tags(model, level, elevation, scale, top_word, true)
}

pub(super) fn member_tags(model: &Model, level: &str, elevation: f64, scale: f64, top_word: &str, include_piles: bool) -> Vec<Note> {
    // a millimetre on paper between a label and what it labels
    let gap = scale * 0.001;
    let note = |at: DVec2, text: String, align: Align, angle: f64| Note { at: at.to_array(), text, size: NOTE_HEIGHT, align, angle, level: false, face: crate::typeface::Face::default() };
    let piles = piles(model, level, elevation);
    let mut notes = Vec::new();
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
            "IfcColumn" | "IfcPile" => {
                let at = l.start.truncate() + DVec2::splat(side + gap);
                match piles.iter().find(|p| p.id == e.id) {
                    // to its lower right, clear of the label of a beam that lies over it
                    Some(_) if !include_piles => {}
                    Some(pile) => notes.push(note(l.start.truncate() + DVec2::new(side + gap, -(side + gap) - scale * NOTE_HEIGHT), pile.number.to_string(), Align::Left, 0.0)),
                    None => notes.push(note(at, what, Align::Left, 0.0)),
                }
            }
            _ => {}
        }
    }
    notes
}

/// An on-sheet schedule reads the same issued identity and live coordinates as the schedule pane.
pub fn schedule_table(model: &Model, plan: &crate::document::PlanView) -> (Vec<Note>, Vec<[[f64; 2]; 2]>) {
    let (lo, hi) = plan.crop.unwrap_or(([0.0, 0.0], [22.0, 40.0]));
    let mut entries: Vec<_> = model.iter().filter_map(|e| {
        let n = e.pile_number?;
        let Shape::Linear(l) = e.shape_in_project(&model.grid) else { return None };
        Some((n, e.pile_mark.as_deref().unwrap_or(&e.name), l.start))
    }).collect();
    entries.sort_by_key(|(n, _, _)| *n);
    let height = (plan.scale * NOTE_HEIGHT).max(0.001);
    let row = height * 3.2;
    let rows = (((hi[1] - lo[1]) / row).floor() as usize).saturating_sub(2).max(1);
    let groups = entries.len().div_ceil(rows).max(1);
    let group_width = (hi[0] - lo[0]) / groups as f64;
    let mut notes = Vec::new();
    let mut lines = Vec::new();
    for group in 0..groups {
        let length = entries.len().saturating_sub(group * rows).min(rows);
        let left = lo[0] + group as f64 * group_width;
        let top = hi[1] - 0.3 * row;
        let bottom = top - (1 + length) as f64 * row;
        for col in [0.0, 0.1, 0.525, 0.765, 0.99] {
            let x = left + col * group_width;
            lines.push([[x, top], [x, bottom]]);
        }
        for i in 0..=length + 1 {
            let y = top - i as f64 * row;
            lines.push([[left, y], [left + group_width * 0.99, y]]);
        }
        let mut write = |r: usize, cols: [String; 4]| {
            for (i, text) in cols.into_iter().enumerate() {
                let x = lo[0] + (group as f64 + [0.015, 0.115, 0.54, 0.78][i]) * group_width;
                notes.push(Note { at: [x, hi[1] - (r + 1) as f64 * row], text, size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, level: false, face: Default::default() });
            }
        };
        write(0, ["Nr.".into(), "ID".into(), "X (m)".into(), "Y (m)".into()]);
        for (r, (n, mark, at)) in entries.iter().skip(group * rows).take(rows).enumerate() {
            write(r + 1, [format!("{n:02}"), mark.to_string(), format!("{:.4}", at.x), format!("{:.4}", at.y)]);
        }
    }
    (notes, lines)
}

/// What the pile schedule and its columns are called when the interface has not said.
pub const PILE_WORDS: [&str; 10] = ["Pile schedule", "Level", "No.", "X (mm)", "Y (mm)", "Pile", "Grade", "Cut-off level", "Pile tip", "Length (mm)"];

impl crate::app::App {
    pub(in crate::app) fn new_pile_schedule_view(&mut self, level: &str, name: &str) {
        let Some(path) = self.document.add_plan_of(level, crate::document::PlanKind::Structural, name) else { return };
        self.document.set_view_drawing(&path, Some("pile-schedule".into()));
        let layers = self.model.iter().flat_map(|model| model.layers.iter().map(|l| (l.name.clone(), false))).collect();
        self.document.set_view_layers(&path, layers);
        let count = self.modeller.model.iter().filter(|e| e.pile_number.is_some()).count();
        self.document.set_crop(&path, Some(([0.0, 0.0], [22.0, 1.0 + 0.8 * (count + 1) as f64])));
        self.tag_plans();
        self.open_plan(&path);
    }

    /// Issue numbers before rebuilding the native scene so the saved definitions contain them.
    pub(in crate::app) fn issue_pile_numbers(&mut self) {
        let model = &mut self.modeller.model;
        let highest_member = model.iter().filter_map(|e| e.pile_number).max().unwrap_or(0);
        let levels: Vec<_> = self.document.levels().iter().map(|l| (l.path.clone(), l.elevation)).collect();
        let mut high = self.document.pile_high_water.max(highest_member);
        for (level, elevation) in levels {
            for pile in piles(model, &level, elevation) {
                let Some(e) = model.get(pile.id) else { continue };
                if e.pile_number.unwrap_or(0) > 0 { continue; }
                let Some(next) = high.checked_add(1) else { continue };
                let mut e = e.clone();
                e.pile_number = Some(next);
                model.insert(e);
                high = next;
            }
        }
        self.document.pile_high_water = high;
    }

    pub(in crate::app) fn designate_members(&mut self, reinforcement: bool) {
        let ids = self.selected_modelled();
        let model = &mut self.modeller.model;
        for id in ids {
            if let Some(e) = model.get(id).filter(|e| matches!(e.shape, Shape::Linear(_)) && e.pile_number.is_none() && (reinforcement || e.class() == "IfcColumn")) {
                let mut e = e.clone();
                if reinforcement { e.reinforcing_bar = true; } else { e.pile_number = Some(0); }
                model.insert(e);
            }
        }
        self.show_modelled();
    }

    /// The pile schedule, made again from the model: a row per pile of every level, a click on
    /// which selects the pile. It comes when the first pile is drawn and stays, empty, when the
    /// last one goes, so the schedules keep their places.
    pub(in crate::app) fn list_piles(&mut self) {
        let words = if self.modeller.pile_words[0].is_empty() { PILE_WORDS } else { self.modeller.pile_words };
        let mm = |v: f64| format!("{}", (v * 1000.0).round() as i64);
        // a row selects its pile where the modelled project is shown alone
        let alone = self.models.len() <= 1;
        let mut rows = Vec::new();
        let mut listed = std::collections::HashSet::new();
        for level in self.document.levels() {
            for pile in piles(&self.modeller.model, &level.path, level.elevation) {
                if !listed.insert(pile.id) { continue; }
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
        let found: Vec<(u64, f64, f64)> = piles.iter().map(|p| (p.number, p.at.x, p.at.y)).collect();
        assert_eq!(found, [(1, 0.0, 5.0), (2, 6.0, 5.02), (3, 0.0, 0.0), (4, 6.0, 0.0)]);
        assert_eq!((piles[0].head, piles[0].tip, piles[0].profile.as_str(), piles[0].grade.as_deref()), (-0.93, -18.93, "Ø300", Some("C35/45")));
        // of another level: none
        assert!(super::piles(&model(), "first", 3.0).is_empty());
    }

    #[test]
    fn a_structural_plan_labels_its_beams_and_columns_and_numbers_its_piles() {
        let notes = tags(&model(), "ground", 0.0, 100.0, "b.k.");
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a local issued IFCX fixture and an empty output folder"]
    fn issued_piles_survive_project_revision() {
        use crate::app::App;
        use crate::loaders::{load_with, Progress, Source};
        use std::{path::PathBuf, sync::Arc};
        let source = PathBuf::from(std::env::var_os("PILE_TEST_IFCX").unwrap());
        let out = PathBuf::from(std::env::var_os("PILE_TEST_OUTPUT").unwrap());
        std::fs::create_dir(&out).expect("fresh output folder");
        let open = |path| {
            let mut app = App::default();
            app.set_model(Arc::new(load_with(&Source::File(path), &Progress::default()).unwrap()));
            app
        };
        let save = |app: &App, name: &str| {
            let bytes = crate::export::to_bytes(app.model.as_ref().unwrap(), &app.document_now(), name).unwrap();
            let path = out.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let mut app = open(source);
        let issued = |app: &App| -> std::collections::BTreeMap<u64, ElementId> {
            app.modeller.model.iter().filter_map(|e| Some((e.pile_number?, e.id))).collect()
        };
        let before = issued(&app);
        let (&highest, _) = before.last_key_value().unwrap();
        let (&first_number, &first_id) = before.first_key_value().unwrap();
        let placements = |app: &App, id: ElementId| -> Vec<[f64; 2]> {
            let mut positions = Vec::new();
            for p in app.document.plans() {
                for &(i, owner, _) in &p.pile_tags {
                    if owner == id.0 { positions.push(p.tags[i].at); }
                }
                for d in &p.dimensions {
                    for (anchor, point) in d.anchors.iter().zip([d.from, d.to]) {
                        if anchor.is_some_and(|a| a.element == id.0) { positions.push(point); }
                    }
                }
            }
            positions
        };
        let placed = placements(&app, first_id);
        assert!(placed.len() >= 3, "linked plan label, detail label and dimension endpoint");
        let model = &mut app.modeller.model;
        let mut moved = model.get(first_id).unwrap().clone();
        moved.translate(&model.grid, DVec3::new(0.125, -0.25, 0.0));
        model.replace(moved.clone()).unwrap();
        let added = model.add(moved.clone()).unwrap();
        app.show_modelled();
        let after = placements(&app, first_id);
        assert_eq!(after.len(), placed.len());
        for (before, after) in placed.iter().zip(&after) {
            assert!((after[0] - before[0] - 0.125).abs() < 1e-6);
            assert!((after[1] - before[1] + 0.25).abs() < 1e-6);
        }
        assert_eq!(issued(&app)[&first_number], first_id);
        assert_eq!(issued(&app)[&(highest + 1)], added);
        app.modeller.model.remove(added);
        app.show_modelled();
        let mut back = open(save(&app, "reopened.ifcx"));
        assert_eq!(issued(&back), before);
        assert_eq!(placements(&back, first_id), after);
        assert_eq!(back.modeller.model.get(first_id).unwrap(), &moved);
        let inserted = back.modeller.model.add(moved).unwrap();
        back.show_modelled();
        assert_eq!(issued(&back)[&(highest + 2)], inserted);
        let count = before.len() + 1;
        let schedule = back.schedules.iter().find(|s| s.name == back.modeller.pile_schedule).unwrap();
        assert_eq!(schedule.rows.len(), count);
        let sheet_table = back.document.plans().iter().find(|p| p.drawing.as_deref() == Some("pile-schedule")).unwrap();
        assert!(sheet_table.tags.iter().any(|n| n.text == format!("{:02}", highest + 2)));
        let baseline = open(save(&open(std::env::var_os("PILE_TEST_IFCX").unwrap().into()), "issued.ifcx"));
        assert_eq!(issued(&baseline), before);
        let mut baseline = baseline;
        let sheets: Vec<_> = baseline.document.sheets().iter().map(|s| (s.path.clone(), s.number.clone())).collect();
        for (index, (path, number)) in sheets.iter().enumerate() {
            baseline.open_doc_sheet(path);
            baseline.export_shown_to(&out.join(format!("sheet-{}.pdf", index + 1))).unwrap();
            eprintln!("exported {}: {}", index + 1, number);
        }
        eprintln!("{} issued piles; retained {}, deleted {}, next {}", before.len(), first_number, highest + 1, highest + 2);
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
        assert_eq!(cells(0), ["Level 0", "1", "2000", "0", "Concrete round 300", "C35/45", "-930", "-18930", "18000"]);
        assert_eq!(cells(1)[1..4], ["2", "0", "0"]);
        // a structural plan says their numbers
        let _ = app.update(Message::NewPlanOf(level.into(), crate::document::PlanKind::Structural, "structure"));
        let plan = app.document.plans()[0].path.clone();
        let tags: Vec<&str> = app.document.view_tags(&plan).iter().map(|n| n.text.as_str()).collect();
        assert_eq!(tags, ["1", "2"]);
        // the piles gone: the schedule stays, empty
        for id in [1, 2] {
            app.select_modelled(ElementId(id));
            let _ = app.update(Message::DeletePicked);
        }
        assert_eq!((app.schedules.len(), app.schedules[0].rows.len()), (1, 0));
    }
}
