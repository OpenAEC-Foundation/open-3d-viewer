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
    member_tags(model, level, elevation, scale, top_word, true, |_| true)
}

pub(super) fn member_tags(model: &Model, level: &str, elevation: f64, scale: f64, top_word: &str, include_piles: bool, visible: impl Fn(&bim_core::Element) -> bool) -> Vec<Note> {
    // a millimetre on paper between a label and what it labels
    let gap = scale * 0.001;
    let note = |at: DVec2, text: String, align: Align, angle: f64| Note { at: at.to_array(), text, size: NOTE_HEIGHT, align, angle, level: false, face: crate::typeface::Face::default() };
    let piles = piles(model, level, elevation);
    let mut notes = Vec::new();
    for (e, l) in members(model, level).filter(|(e, _)| visible(e)) {
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

mod legend;
pub(super) use legend::{is_pile_legend, schedule_table};

/// What the pile schedule and its columns are called when the interface has not said.
pub const PILE_WORDS: [&str; 10] = ["Pile schedule", "Level", "No.", "X (mm)", "Y (mm)", "Pile", "Grade", "Cut-off level", "Pile tip", "Length (mm)"];

impl crate::app::App {
    pub(in crate::app) fn new_pile_schedule_view(&mut self, level: &str, name: &str) {
        if let Some(path) = self.add_pile_legend(level, name) { self.open_plan(&path); }
    }

    pub(in crate::app) fn add_pile_legend(&mut self, level: &str, name: &str) -> Option<String> {
        let path = self.document.add_plan_of(level, crate::document::PlanKind::Structural, name)?;
        self.document.set_view_drawing(&path, Some("pile-schedule".into()));
        let layers = self.model.iter().flat_map(|model| model.layers.iter().map(|l| (l.name.clone(), false))).collect();
        self.document.set_view_layers(&path, layers);
        self.tag_plans();
        Some(path)
    }

    /// Issue numbers before rebuilding the native scene so the saved definitions contain them.
    pub(in crate::app) fn issue_pile_numbers(&mut self) {
        let model = &mut self.modeller.model;
        let highest_member = model.iter().filter_map(|e| e.pile_number).max().unwrap_or(0);
        let levels: Vec<_> = self.document.levels().iter().map(|l| (l.path.clone(), l.elevation)).collect();
        let mut high = self.document.pile_high_water.max(highest_member);
        // Older drawings register the piles in issue order on their native legend.
        // A pile can start on its level and finish above it, outside the column heuristic.
        for plan in self.document.plans().iter().filter(|p| legend::registered(&p.presentation)) {
            for id in plan.presentation["legend"]["elementIds"].as_array().into_iter().flatten().filter_map(|v| v.as_u64()) {
                let Some(e) = model.get(ElementId(id)).filter(|e| matches!(e.shape, Shape::Linear(_))) else { continue };
                if e.pile_number.unwrap_or(0) > 0 { continue; }
                let Some(next) = high.checked_add(1) else { continue };
                let mut e = e.clone();
                e.pile_number = Some(next);
                model.insert(e);
                high = next;
            }
        }
        if self.document.plans().iter().any(|p| legend::registered(&p.presentation) && p.drawing.as_deref() != Some("pile-schedule")) {
            self.document.inherit_pile_visibility();
        }
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
        let rows = sheet_table.presentation["legend"]["rows"].as_array().unwrap();
        assert_eq!(rows.iter().filter_map(|row| row["count"].as_u64()).sum::<u64>(), count as u64);
        assert!(!sheet_table.tags.iter().any(|n| n.text == "X (m)"));
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
    fn grouped_pile_legend_migrates_changes_members_and_reopens() {
        use crate::app::App;
        use crate::document::{PlanKind, PlanView};
        use crate::loaders::{load_with, Progress, Source};
        use serde_json::json;
        use std::sync::Arc;

        let mut app = App::default();
        app.new_model("Ground");
        let level = app.document.levels()[0].path.clone();
        let view = app.document.add_plan_of(&level, PlanKind::Structural, "Legenda").unwrap();
        let sheet = app.document.add_sheet("Fundering");
        app.document.place_plan(&sheet, &view).unwrap();
        let grid = app.modeller.model.grid;
        let pile = |x| Element::in_project(&grid, "T1 object", Some(level.clone()), Shape::Linear(LinearDef {
            start: DVec3::new(x, 0.0, 0.5), end: DVec3::new(x, 0.0, 4.5),
            profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0,
        }));
        let a = app.modeller.model.add(pile(2.0)).unwrap();
        let b = app.modeller.model.add(pile(8.0)).unwrap();
        app.document.set_plan_tags(|p| {
            p.crop = Some(([1000.0, 990.0], [1010.0, 1000.0]));
            p.presentation = json!({"origin":[0,0], "legend":{"source":"openaec::element","symbolSource":"pile profile",
                "elementIds":[b.0,a.0], "rows":[{"type":"T1","count":2}]}});
            p.layers.insert("IfcColumn".into(), false);
            p.lines.push([[0.0, 0.0], [10.0, 0.0]]);
            for (i, text) in ["Legenda", "Type", "Symbool", "Schacht [mm]", "Aantal", "Lengte [m]", "Punt [m NAP]", "Kop [m NAP]",
                "T1", "300 × 300", "2", "4,00", "+0,50", "+4,50", "Toelichting"].into_iter().enumerate() {
                p.notes.push(Note { at: [1000.0, 1000.0-i as f64*0.4], text: text.into(), size: NOTE_HEIGHT, angle: 0.0,
                    align: Align::Left, level: false, face: Default::default() });
            }
            (vec![], vec![])
        });
        app.show_modelled();
        let counts = |p: &PlanView| p.presentation["legend"]["rows"].as_array().unwrap().iter()
            .map(|r| (r["type"].as_str().unwrap().to_owned(), r["count"].as_u64().unwrap())).collect::<std::collections::BTreeMap<_,_>>();
        let p = app.document.plan(&view).unwrap();
        assert_eq!(counts(p)["T1"], 2);
        assert_eq!(p.layers["IfcPile"], false);
        assert!(p.lines.is_empty());
        assert_eq!(p.notes.len(), 1);
        assert_eq!(p.notes[0].text, "Toelichting");
        assert_eq!(app.modeller.model.get(b).unwrap().pile_number, Some(1));
        assert_eq!(app.modeller.model.get(a).unwrap().pile_number, Some(2));

        let mut changed = app.modeller.model.get(a).unwrap().clone();
        let Shape::Linear(l) = &mut changed.shape else { unreachable!() };
        l.profile = ProfileDef::Catalogue { designation: "Rond".into(), shape: bim_library::ProfileShape::Circle { diameter: 0.4 } };
        app.modeller.model.replace(changed.clone()).unwrap();
        app.show_modelled();
        let p = app.document.plan(&view).unwrap();
        assert_eq!((counts(p)["T1"], counts(p)["Rond"]), (1, 1));
        assert!(p.tags.iter().any(|n| n.text == "Ø400"));
        assert!(p.tags.iter().any(|n| n.text == "+4,50"));
        let new = app.modeller.model.add(changed).unwrap();
        app.show_modelled();
        assert_eq!(counts(app.document.plan(&view).unwrap())["Rond"], 2);
        app.modeller.model.remove(b);
        app.show_modelled();
        let p = app.document.plan(&view).unwrap();
        assert_eq!(counts(p).into_values().collect::<Vec<_>>(), vec![2]);
        let (lo, hi) = p.crop.unwrap();
        assert_eq!((lo[0], hi[1]), (1000.0, 1000.0));
        for n in p.notes.iter().chain(&p.tags) { assert!(n.at[0] >= lo[0] && n.at[0] <= hi[0] && n.at[1] >= lo[1] && n.at[1] <= hi[1]); }
        let encoded = crate::export::to_bytes(app.model.as_ref().unwrap(), &app.document_now(), "piles.ifcx").unwrap();
        let source = Source::Bytes { name: "piles.ifcx".into(), data: Arc::new(encoded) };
        let mut back = App::default();
        back.set_model(Arc::new(load_with(&source, &Progress::default()).unwrap()));
        let restored = back.document.plan(&view).unwrap();
        assert_eq!(restored.presentation, p.presentation);
        assert_eq!(restored.tags, p.tags);
        assert_eq!(restored.symbols, p.symbols);
        assert_eq!(restored.notes, p.notes);
        assert_eq!(back.document.sheet(&sheet).unwrap().views[0].plan, view);
        assert_eq!(back.modeller.model.get(new).unwrap().pile_number, Some(3));
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
