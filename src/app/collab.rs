//! The BIM Collaborator's work on the open project: issues (BCF), information checks (IDS, the
//! built-in model checks, the BIM basis ILS profile) and clash tests, kept together in a rule
//! set. The panel lists what each found; a click goes to it in the model.
//!
//! An issue goes to its viewpoint: the camera, what is selected, what is hidden and where the
//! model is cut. A new issue takes those from the view as it is, with a picture of it. BCF
//! writes its coordinates in metres in the model's own place, so they pass through the model's
//! origin on the way in and out.

use super::{App, Message};
use crate::collab::clash::{self, Kind, Solid};
use crate::collab::rules::{ClashKind, ClashRule, RuleSet, Selector};
use crate::model::Model;
use crate::section::SectionPlane;
use crate::selection::Pickable;
use crate::units::UnitKey;
use bim_bcf::issues::{ClippingPlane, Issues, Lens, Topic, ViewCamera, Viewpoint};
use bim_validator_core::{IlsOptions, Severity, SpecificationResult, ValidationOptions, ValidationReport, ValidationStatus};
use glam::{DVec3, Vec3};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Task};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D, Tone};
use openaec_ui::design::a11y::{node as named, Node, Role};
use openaec_ui::design::{site, Look};
use openaec_ui::Language;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// The most clashes and findings a list shows; the reports hold all.
const SHOWN: usize = 300;
/// The most clashes a run keeps.
const MOST_CLASHES: usize = 5000;

/// A tab of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Issues,
    Checks,
    Clashes,
    /// Two versions of a model side by side: what was added, removed and changed.
    Compare,
    /// The model coloured by a property, and the sets of elements kept.
    Colours,
    /// Takeoffs of the elements' information and quantities.
    Takeoff,
}

/// What a person decided on a clash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Decision {
    /// Not looked at yet.
    #[default]
    Open,
    /// Accepted: it may stay as it is.
    Accepted,
    /// Rejected: it has to be solved (an issue made of it is rejected too).
    Rejected,
}

impl Decision {
    pub fn key(self) -> &'static str {
        match self {
            Decision::Open => "open",
            Decision::Accepted => "accepted",
            Decision::Rejected => "rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Decision> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" | "undefined" => Some(Decision::Open),
            "accepted" | "accept" | "akkoord" => Some(Decision::Accepted),
            "rejected" | "reject" | "afgewezen" => Some(Decision::Rejected),
            _ => None,
        }
    }
}

/// What changed between two versions of a model, element by element (by GlobalId).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comparison {
    pub old: String,
    pub new: String,
    /// In the new version only: its element and name.
    pub added: Vec<(u64, String)>,
    /// In the old version only.
    pub removed: Vec<(u64, String)>,
    /// In both, otherwise: the new version's element, its name and what changed.
    pub changed: Vec<(u64, String, String)>,
    /// How many elements with a GlobalId each version has.
    pub counts: (usize, usize),
}

/// Compares the elements of open model `old` with those of model `new` (their places among the
/// open models) by their GlobalId: their properties (but the express id, which a new export
/// numbers again) and their place and size, to a millimetre.
pub fn compare_models(model: &Model, old: u32, new: u32) -> Comparison {
    let of = |which: u32| -> HashMap<String, u64> {
        model
            .props
            .iter()
            .filter(|(id, _)| model.element_key(**id).model == which)
            .filter_map(|(&id, p)| Some((field(p, "GlobalId")?.to_string(), id)))
            .collect()
    };
    let (a, b) = (of(old), of(new));
    let props = |id: u64| -> serde_json::Map<String, serde_json::Value> {
        let mut map: serde_json::Map<String, serde_json::Value> = model.props.get(&id).and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
        map.remove("ExpressId");
        map
    };
    let mut out = Comparison::default();
    out.counts = (a.len(), b.len());
    for (guid, &id) in &b {
        let Some(&before) = a.get(guid) else {
            out.added.push((id, element_name(model, id)));
            continue;
        };
        let (p, q) = (props(before), props(id));
        let mut what: Vec<String> = q.iter().filter(|(k, v)| p.get(*k) != Some(*v)).map(|(k, _)| k.clone()).chain(p.keys().filter(|k| !q.contains_key(*k)).cloned()).collect();
        what.sort();
        what.dedup();
        let moved = match (model.element_bounds(before), model.element_bounds(id)) {
            (Some(x), Some(y)) => (0..3).any(|k| (x.min[k] - y.min[k]).abs() > 0.001 || (x.max[k] - y.max[k]).abs() > 0.001),
            (x, y) => x.is_some() != y.is_some(),
        };
        if moved {
            what.insert(0, "geometry".into());
        }
        if !what.is_empty() {
            let shown: Vec<String> = what.iter().take(4).cloned().collect();
            let more = what.len().saturating_sub(4);
            out.changed.push((id, element_name(model, id), if more > 0 { format!("{} +{more}", shown.join(", ")) } else { shown.join(", ") }));
        }
    }
    out.removed = a.iter().filter(|(guid, _)| !b.contains_key(*guid)).map(|(_, &id)| (id, element_name(model, id))).collect();
    for list in [&mut out.added, &mut out.removed] {
        list.sort_by(|x, y| x.1.cmp(&y.1));
    }
    out.changed.sort_by(|x, y| x.1.cmp(&y.1));
    out
}

/// Where the Collaborator keeps who you are: beside the preferences.
/// Where the Collaborator keeps the folder the project is shared through.
fn share_file() -> Option<PathBuf> {
    Some(crate::remote_link::settings_dir()?.join("collaborator-share.txt"))
}

fn user_file() -> Option<PathBuf> {
    Some(crate::remote_link::settings_dir()?.join("collaborator-user.txt"))
}

/// How far a hard clash goes: the one only scrapes the other, goes partly into it, or goes
/// right through it (a pipe through a wall). The worst sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Reach {
    /// Elements too near (a clearance test): nothing crosses.
    #[default]
    Near,
    Graze,
    Partly,
    Through,
}

impl Reach {
    pub fn key(self) -> &'static str {
        match self {
            Reach::Near => "clearance",
            Reach::Graze => "graze",
            Reach::Partly => "partly",
            Reach::Through => "through",
        }
    }

    /// How far a clash of this kind goes: `depth` is how far the elements cross, the boxes are
    /// those of the two elements (min, max).
    pub fn of(kind: Kind, depth: f32, a: (Vec3, Vec3), b: (Vec3, Vec3)) -> Reach {
        match kind {
            Kind::Near => Reach::Near,
            Kind::Hidden => Reach::Partly,
            Kind::Inside => Reach::Through,
            Kind::Crossing => {
                let size = |x: (Vec3, Vec3)| x.1 - x.0;
                let (small, large) = if size(a).length_squared() <= size(b).length_squared() { (a, b) } else { (b, a) };
                // the thinnest side of the smaller one: a clash less deep than a little of it scrapes
                let thin = size(small).min_element().max(0.001);
                if depth < 0.01 || depth < 0.15 * thin {
                    return Reach::Graze;
                }
                // through: along some axis the smaller one sticks out of the larger on both sides
                let through = (0..3).any(|k| small.0[k] < large.0[k] - 0.005 && small.1[k] > large.1[k] + 0.005);
                if through { Reach::Through } else { Reach::Partly }
            }
        }
    }
}

/// A clash as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClashRow {
    pub rule: String,
    pub a: u64,
    pub b: u64,
    pub a_name: String,
    pub b_name: String,
    pub kind: Kind,
    pub at: [f32; 3],
    /// How far they cross or are apart (metres).
    pub distance: f32,
    pub decision: Decision,
    /// The group it is in (see [`group_clashes`]): the rows of a group follow each other.
    pub group: usize,
    /// The two are one element twice: of one class, in one place, of one size.
    pub copy: bool,
    /// Its two elements by the keys they keep between versions (see [`App::collab_take_clashes`]).
    pub key: String,
    /// The run before did not have it.
    pub fresh: bool,
    /// How far it goes.
    pub reach: Reach,
}

/// What a group of clashes is called: the element most of them have.
pub(super) fn group_title(rows: &[&ClashRow]) -> String {
    let mut count: Vec<(&str, usize)> = Vec::new();
    for r in rows {
        for name in [r.a_name.as_str(), r.b_name.as_str()] {
            match count.iter_mut().find(|(n, _)| *n == name) {
                Some(c) => c.1 += 1,
                None => count.push((name, 1)),
            }
        }
    }
    // the first of those most named
    count.iter().rev().max_by_key(|(_, c)| *c).map(|(n, _)| n.to_string()).unwrap_or_default()
}

/// How the clashes of a test are put together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grouping {
    /// Each clash on its own.
    Off,
    /// By the element they share: a duct through ten beams is one group.
    #[default]
    Element,
    /// By where they are: the clashes within a few metres of each other.
    Place,
}

/// A way to group in a list to choose from.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupChoice(Grouping, String);

impl std::fmt::Display for GroupChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// The ways to group, in the language of the interface.
fn groupings(l: Language) -> Vec<GroupChoice> {
    vec![
        GroupChoice(Grouping::Off, l.text("Niet groeperen", "No groups").to_string()),
        GroupChoice(Grouping::Element, l.text("Per element", "By element").to_string()),
        GroupChoice(Grouping::Place, l.text("Per plek", "By place").to_string()),
    ]
}

/// The size of the cells clashes are grouped by place in (metres).
const PLACE: f32 = 3.0;

/// Puts the clashes of each test in groups, and the rows of a group after each other: the tests
/// keep their order, and so do the groups and the clashes in a group. By element, a clash goes
/// with the one of its two elements that clashes most in its test, so one element with many
/// clashes is one group, not a chain of everything that touches; by place, with the clashes of
/// its test in the same cell of a few metres.
pub fn group_clashes(rows: &mut Vec<ClashRow>, by: Grouping) {
    let mut count: HashMap<(&str, u64), usize> = HashMap::new();
    for r in rows.iter() {
        for id in [r.a, r.b] {
            *count.entry((r.rule.as_str(), id)).or_default() += 1;
        }
    }
    let rule_order: HashMap<&str, usize> = rows.iter().enumerate().rev().map(|(k, r)| (r.rule.as_str(), k)).collect();
    let keys: Vec<(usize, [i64; 3])> = rows
        .iter()
        .enumerate()
        .map(|(k, r)| match by {
            Grouping::Off => (rule_order[r.rule.as_str()], [k as i64, 0, 0]),
            Grouping::Element => {
                let (ca, cb) = (count[&(r.rule.as_str(), r.a)], count[&(r.rule.as_str(), r.b)]);
                (rule_order[r.rule.as_str()], [if cb > ca { r.b } else { r.a } as i64, 0, 0])
            }
            Grouping::Place => (rule_order[r.rule.as_str()], r.at.map(|v| (v / PLACE).floor() as i64)),
        })
        .collect();
    // the groups in the order their first clash comes
    let mut first: HashMap<(usize, [i64; 3]), usize> = HashMap::new();
    for (k, key) in keys.iter().enumerate() {
        first.entry(*key).or_insert(k);
    }
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&k| (keys[k].0, first[&keys[k]], k));
    let mut sorted: Vec<ClashRow> = order.iter().map(|&k| rows[k].clone()).collect();
    let mut group = 0;
    for (n, &k) in order.iter().enumerate() {
        if n > 0 && keys[k] != keys[order[n - 1]] {
            group += 1;
        }
        sorted[n].group = group;
    }
    *rows = sorted;
}

/// What a check of one model gave.
#[derive(Debug, Clone)]
pub struct Checked {
    /// The model's place among the open models, and its file name.
    pub model: usize,
    pub name: String,
    pub report: Result<Arc<ValidationReport>, String>,
}

/// The Collaborator's state of a project.
pub struct State {
    pub tab: Tab,
    pub issues: Issues,
    /// The pictures of the viewpoints, read once: by the viewpoint's guid and the picture's size
    /// in bytes, the picture small (for the list) and whole. Drawing a list must not unpack its
    /// pictures again every frame.
    pub pictures: HashMap<(String, usize), (iced::widget::image::Handle, iced::widget::image::Handle)>,
    /// Where the issues were read from or last saved.
    pub issues_path: Option<PathBuf>,
    pub issues_changed: bool,
    /// The topic shown, by its guid.
    pub topic: Option<String>,
    pub comment: String,
    pub author: String,
    /// The rule sets to choose from: the built-in ones, then those opened; and the one in use.
    pub rule_sets: Vec<RuleSet>,
    pub rule_set: usize,
    pub checked: Vec<Checked>,
    /// The specification opened in the list: the checked model and its place in the report.
    pub open_spec: Option<(usize, usize)>,
    pub clashes: Vec<ClashRow>,
    pub clash: Option<usize>,
    /// How the clashes are grouped, and the group opened in the list.
    pub grouping: Grouping,
    /// Whether the clash tests and the matrix show above the clashes.
    pub show_tests: bool,
    pub open_group: Option<usize>,
    /// Colours, sets and takeoffs.
    pub inform: super::inform::State,
    /// A point cloud against the model.
    pub cloud: super::cloud::State,
    /// The PDF report being made.
    pub report_job: Option<ReportJob>,
    /// The folder the project is shared through; the issues removed here since the last sync;
    /// what the folder said of the clashes then; and the issues as they were then.
    pub share_folder: Option<PathBuf>,
    share_removed: Vec<String>,
    share_known: HashMap<String, String>,
    share_seen: u64,
    /// The walk of each room to its way out (by the room's element), and the room whose walk
    /// the 3D view draws.
    pub routes: HashMap<u64, Vec<[f32; 3]>>,
    pub route_shown: Option<u64>,
    /// The issues are written as BCF 3.0, not 2.1.
    pub bcf3: bool,
    /// The clashes of the run before that this run no longer found, and whether the list shows
    /// them; whether this run was compared with one before; whether it is of a selection only.
    pub resolved: Vec<crate::collab::rules::ClashMemo>,
    pub show_resolved: bool,
    pub compared: bool,
    partial_run: bool,
    /// The clashing elements a picture shows: the first of each clash and the second.
    pub clash_shown: Option<(std::collections::HashSet<u64>, std::collections::HashSet<u64>)>,
    /// With them: the elements near enough to show as their surroundings, and the element most
    /// of the clashes share (a floor many pipes go through), drawn lighter.
    clash_near: std::collections::HashSet<u64>,
    clash_shared: Option<u64>,
    /// The boxes of the elements, for the model with this id: worked out once for a report.
    boxes: Option<(u64, Arc<HashMap<u64, crate::model::Bounds>>)>,
    /// The amounts of the room checks as typed.
    room_amounts: Vec<String>,
    /// Per clash test, the share of places that may be without sight, as typed.
    sight_shares: Vec<String>,
    /// The tolerance of the matrix as typed, and the ways to group in the list to choose from.
    matrix_amount: String,
    groupings: Vec<GroupChoice>,
    pub running: bool,
    /// What the list of issues is narrowed to, and whether closed ones show.
    pub search: String,
    pub show_closed: bool,
    /// What a clash test can take as a side: everything, a model, a layer, a class; for the
    /// model with this id.
    sides: Vec<Side>,
    sides_for: Option<(u64, Language)>,
    /// The labels of the issue shown, as typed.
    labels_text: String,
    /// The people an issue can be assigned to: you and everyone the issues name.
    team: Vec<Choice>,
    /// Whether the list of clashes shows the accepted ones too.
    pub show_accepted: bool,
    /// The two open models compared (their places), and what changed.
    pub compare_pair: (usize, usize),
    pub comparison: Option<Comparison>,
    /// The open models to choose from, by name.
    model_names: Vec<Choice>,
    /// Whether what a click in a list goes to shows alone, the rest of the model hidden.
    pub isolate: bool,
    /// Whether the checks list only what does not pass.
    pub only_failed: bool,
    /// What the lists to choose from offer, in the language of the interface, and the amounts of
    /// the clash tests as typed: the panel shows them as they are kept here.
    rule_names: Vec<Choice>,
    statuses: Vec<Label>,
    priorities: Vec<Label>,
    amounts: Vec<String>,
    labelled: Option<Language>,
    pub notice: Option<(Tone, String)>,
    /// A file dialog was asked for and has not answered: Windows can take seconds to show the
    /// first one, so the panel says it is coming.
    pub dialog_open: bool,
    /// The language of the interface: what the panel reports is said in it.
    pub language: Language,
    cancel: Arc<AtomicBool>,
    run: u64,
    /// The elements of the model by GlobalId, for the model with this id.
    guids: Option<(u64, Arc<HashMap<String, u64>>)>,
    /// The topic a picture of the view is being taken for.
    shot_for: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        // who you are, as you set it, else the name you log in with
        let kept = user_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let author = kept.unwrap_or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into()));
        State {
            pictures: HashMap::new(),
            tab: Tab::default(),
            issues: Issues::new(),
            issues_path: None,
            issues_changed: false,
            topic: None,
            comment: String::new(),
            author,
            rule_sets: Vec::new(),
            rule_set: 0,
            checked: Vec::new(),
            open_spec: None,
            clashes: Vec::new(),
            clash: None,
            grouping: Grouping::default(),
            show_tests: true,
            open_group: None,
            matrix_amount: String::new(),
            inform: Default::default(),
            cloud: Default::default(),
            report_job: None,
            share_folder: share_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| PathBuf::from(s.trim())).filter(|p| p.is_dir()),
            share_removed: Vec::new(),
            share_known: HashMap::new(),
            share_seen: 0,
            routes: HashMap::new(),
            route_shown: None,
            bcf3: false,
            resolved: Vec::new(),
            show_resolved: false,
            compared: false,
            partial_run: false,
            clash_shown: None,
            clash_near: Default::default(),
            clash_shared: None,
            boxes: None,
            room_amounts: Vec::new(),
            sight_shares: Vec::new(),
            groupings: Vec::new(),
            running: false,
            search: String::new(),
            show_closed: true,
            team: Vec::new(),
            labels_text: String::new(),
            show_accepted: true,
            compare_pair: (0, 1),
            comparison: None,
            model_names: Vec::new(),
            sides: Vec::new(),
            sides_for: None,
            isolate: false,
            only_failed: true,
            rule_names: Vec::new(),
            statuses: Vec::new(),
            priorities: Vec::new(),
            amounts: Vec::new(),
            labelled: None,
            notice: None,
            dialog_open: false,
            language: Language::English,
            cancel: Arc::default(),
            run: 0,
            guids: None,
            shot_for: None,
        }
    }
}

impl State {
    /// The rule set in use.
    pub fn rules(&self) -> Option<&RuleSet> {
        self.rule_sets.get(self.rule_set)
    }

    pub(super) fn rules_mut(&mut self) -> Option<&mut RuleSet> {
        self.rule_sets.get_mut(self.rule_set)
    }

    /// Reads the pictures of the viewpoints that were not read yet, and forgets those of
    /// viewpoints that are gone.
    fn refresh_pictures(&mut self) {
        let wanted = || self.issues.topics.iter().flat_map(|t| &t.viewpoints).filter_map(|v| v.snapshot.as_ref().map(|(_, bytes)| ((v.guid.clone(), bytes.len()), bytes)));
        let count = wanted().count();
        if count == self.pictures.len() && wanted().all(|(key, _)| self.pictures.contains_key(&key)) {
            return;
        }
        let mut pictures = HashMap::with_capacity(count);
        for (key, bytes) in wanted() {
            let kept = self.pictures.get(&key).cloned();
            let picture = kept.unwrap_or_else(|| {
                let whole = iced::widget::image::Handle::from_bytes(bytes.clone());
                // small for the list: a few thousand pixels instead of the whole screenshot
                let small = image::load_from_memory(bytes).ok().map(|picture| {
                    let small = picture.thumbnail(168, 126).to_rgba8();
                    iced::widget::image::Handle::from_rgba(small.width(), small.height(), small.into_raw())
                });
                (small.unwrap_or_else(|| whole.clone()), whole)
            });
            pictures.insert(key, picture);
        }
        self.pictures = pictures;
    }

    /// Brings what the panel chooses from up to date: the rule sets the app comes with, their
    /// names, the statuses and priorities in the language of the interface, the amounts of the
    /// clash tests.
    pub fn refresh(&mut self) {
        let l = self.language;
        self.refresh_pictures();
        if self.rule_sets.is_empty() {
            self.rule_sets = RuleSet::built_in(l == Language::Dutch);
        }
        if self.labelled != Some(l) {
            self.statuses = STATUSES.iter().map(|&s| Label(s, status_label(s, l))).collect();
            self.priorities = PRIORITIES.iter().map(|&s| Label(s, priority_label(s, l))).collect();
            self.groupings = groupings(l);
            self.labelled = Some(l);
        }
        let mut people: Vec<String> = std::iter::once(self.author.clone())
            .chain(self.issues.topics.iter().flat_map(|t| [t.creation_author.clone(), t.assigned_to.clone(), t.modified_author.clone()].into_iter().chain(t.comments.iter().map(|c| c.author.clone()))))
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        people.sort_by_key(|p| p.to_lowercase());
        people.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        let team: Vec<Choice> = people.into_iter().enumerate().map(|(k, p)| Choice(k, p)).collect();
        if team != self.team {
            self.team = team;
        }
        let names: Vec<Choice> = self.rule_sets.iter().enumerate().map(|(k, r)| Choice(k, r.name.clone())).collect();
        if names != self.rule_names {
            self.rule_names = names;
        }
        let amounts: Vec<f32> = self.rules().map(|r| r.clashes.iter().map(|c| match c.kind { ClashKind::Hard => c.tolerance_mm, ClashKind::Clearance => c.clearance_mm, ClashKind::Sight => c.sight_m }).collect()).unwrap_or_default();
        // what is typed stays as typed while it says the same amount
        let same = |typed: &String, amount: f32| typed.trim().replace(',', ".").parse::<f32>().ok() == Some(amount);
        if amounts.len() != self.amounts.len() || !self.amounts.iter().zip(&amounts).all(|(t, a)| same(t, *a) || t.trim().is_empty() || t.ends_with(['.', ','])) {
            self.amounts = amounts.iter().map(|a| a.to_string()).collect();
        }
        let shares: Vec<f32> = self.rules().map(|r| r.clashes.iter().map(|c| c.sight_blocked_pct).collect()).unwrap_or_default();
        if shares.len() != self.sight_shares.len() || !self.sight_shares.iter().zip(&shares).all(|(t, v)| same(t, *v) || t.trim().is_empty() || t.ends_with(['.', ','])) {
            self.sight_shares = shares.iter().map(|v| v.to_string()).collect();
        }
        if let Some(r) = self.rules().map(|r| r.rooms.clone()) {
            let values = [r.room_height, r.door_width, r.door_height, r.escape_distance, r.threshold, r.ramp_run, r.turning_circle];
            if self.room_amounts.len() != 7 || !self.room_amounts.iter().zip(values).all(|(t, v)| same(t, v) || t.trim().is_empty() || t.ends_with(['.', ','])) {
                self.room_amounts = values.iter().map(|v| v.to_string()).collect();
            }
        }
        if let Some(mm) = self.rules().map(|r| r.matrix_tolerance_mm) {
            let typed = &self.matrix_amount;
            if !(same(typed, mm) || (!typed.is_empty() && typed.ends_with(['.', ',']))) {
                self.matrix_amount = mm.to_string();
            }
        }
    }

    /// How many issues are not closed.
    pub fn open_issues(&self) -> usize {
        self.issues.topics.iter().filter(|t| !closed(&t.status)).count()
    }
}

fn closed(status: &str) -> bool {
    matches!(status.to_ascii_lowercase().as_str(), "closed" | "resolved" | "done")
}

fn go(event: Event) -> Message {
    Message::Collab(event)
}

/// A file a dialog asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum File {
    Bcf,
    Ids,
    Rules,
}

/// A report to save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    Html,
    Csv,
    Json,
    ClashesCsv,
    IssuesHtml,
    /// The checks and the clashes as a PDF to read, and as an Excel workbook.
    Pdf,
    Xlsx,
    /// The issues alone as a PDF.
    IssuesPdf,
}

/// A picture a report takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shot {
    /// The view as it is, for the cover.
    Cover,
    /// A group of clashes, shown.
    Group(usize),
    /// An issue without a picture of its own, from its viewpoint.
    Issue(usize),
}

/// A PDF report being made: the pictures still to take and those taken, and the view as it
/// was, to come back to.
pub struct ReportJob {
    path: PathBuf,
    scope: super::reports::Scope,
    todo: Vec<Shot>,
    pictures: HashMap<usize, crate::export::report::Jpeg>,
    issue_pictures: HashMap<usize, crate::export::report::Jpeg>,
    cover: Option<crate::export::report::Jpeg>,
    camera: crate::viewport::camera::Camera,
    selection: Vec<Pickable>,
    hidden: Arc<crate::units::UnitKeySet>,
    plane: Option<SectionPlane>,
    measured: Vec<crate::measure::Measurement>,
    open_group: Option<usize>,
    clash: Option<usize>,
    started: Instant,
}

#[derive(Debug, Clone)]
pub enum Event {
    Tab(Tab),
    Open(File),
    Opened(File, Option<PathBuf>),
    SaveIssues,
    SaveIssuesTo(Option<PathBuf>),
    SaveRules,
    SaveRulesTo(Option<PathBuf>),
    Topic(Option<String>),
    Viewpoint(String, usize),
    NewIssue,
    Shot(Arc<iced::window::Screenshot>),
    Title(String),
    Description(String),
    Status(&'static str),
    Priority(&'static str),
    Assigned(String),
    Comment(String),
    AddComment,
    /// Who you are: the author of what you make.
    User(String),
    Due(String),
    Labels(String),
    KindOf(&'static str),
    AssignTo(Choice),
    Decide(usize, Decision),
    ShowAccepted(bool),
    /// A section box around what is selected.
    SectionAround,
    CompareOld(Choice),
    CompareNew(Choice),
    Compare,
    /// An element of the comparison: 0 added, 1 removed, 2 changed, and its place in that list.
    ComparePick(u8, usize),
    /// The issues as a report: a page with each issue, its picture and its comments.
    ExportIssues,
    Isolate(bool),
    /// To the clash before (−1) or after (1) the one picked.
    Step(i32),
    Search(String),
    ShowClosed(bool),
    OnlyFailed(bool),
    RemoveTopic,
    RuleSet(usize),
    ModelChecks(bool),
    Ils(bool),
    /// The checks of rooms, doors, escape routes and fire separations.
    Rooms(bool),
    /// One of their amounts as typed: room height, door width, door height, escape distance.
    RoomAmount(usize, String),
    RemoveIds(usize),
    Check,
    Checked(u64, Vec<Checked>),
    Spec(Option<(usize, usize)>),
    Finding(usize, u32),
    IssuesFromChecks,
    Export(Report),
    ExportTo(Report, Option<PathBuf>),
    ClashOn(usize, bool),
    /// Whether a clash test is only between models.
    ClashBetween(usize, bool),
    /// The storeys a clash test is of, and its tolerances per pair of classes, as typed.
    ClashStoreys(usize, String),
    ClashPairs(usize, String),
    /// Its property and the value it asks, the names a sight-line test wants seen, and the
    /// share of places that may be without sight, as typed.
    ClashProperty(usize, String),
    ClashValue(usize, String),
    ClashSightNames(usize, String),
    ClashSightShare(usize, String),
    ClashKindOf(usize, ClashKind),
    ClashAmount(usize, String),
    /// The first (true) or second side of a clash test.
    ClashSide(usize, bool, Side),
    /// The selected elements against the rest of the model.
    ClashSelection,
    AddClashRule,
    RemoveClashRule(usize),
    Clash,
    Clashed(u64, Vec<ClashRow>),
    PickClash(usize),
    /// A picture of the view for the report being made.
    ReportShot(Arc<iced::window::Screenshot>),
    /// Colours, sets and takeoffs.
    Inform(super::inform::Event),
    /// A point cloud against the model.
    Cloud(super::cloud::Event),
    IssueFromClash(usize),
    IssuesFromClashes,
    GroupBy(GroupChoice),
    ShowResolved(bool),
    /// The folder the project is shared through: asked for, chosen (none: not shared), synced.
    ShareFolder,
    ShareFolderIs(Option<PathBuf>),
    Unshare,
    Sync,
    /// Whether the issues are written as BCF 3.0 (else 2.1).
    Bcf3(bool),
    /// The clash picked, or else the group open: decided on, or made an issue of.
    DecidePicked(Decision),
    IssueFromPicked,
    /// The clash tests and the matrix shown (true) or folded away.
    ShowTests(bool),
    /// A group of clashes: opens it in the list (or closes it) and selects its elements.
    PickGroup(usize),
    DecideGroup(usize, Decision),
    IssueFromGroup(usize),
    /// The pair of disciplines (by their places) ticked in the matrix, or taken out.
    MatrixCell(usize, usize),
    AddDiscipline,
    RemoveDiscipline(usize),
    DisciplineName(usize, String),
    DisciplineSide(usize, Side),
    MatrixAmount(String),
    MatrixBetween(bool),
    Cancel,
    Dismiss,
}

/// Asks for a file to open (or, with `save`, where to save one) in a dialog that belongs to the
/// window, so it opens in front of it, and then the message `then` makes of the answer.
fn ask_file(
    make: impl Fn() -> rfd::AsyncFileDialog + Send + Sync + Clone + 'static,
    save: bool,
    then: impl Fn(Option<PathBuf>) -> Message + Send + Sync + Clone + 'static,
) -> Task<Message> {
    let alone = make.clone();
    iced::window::latest()
        .then(move |id| match id {
            Some(id) => {
                let make = make.clone();
                iced::window::run(id, move |window| make().set_parent(window))
            }
            None => Task::done(alone()),
        })
        .then(move |dialog| {
            let then = then.clone();
            Task::perform(
                async move {
                    let file = if save { dialog.save_file().await } else { dialog.pick_file().await };
                    file.map(|f| f.path().to_path_buf())
                },
                then,
            )
        })
}

/// Work on a thread of its own, its result as an event.
fn job(work: impl FnOnce() -> Event + Send + 'static) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(work());
            });
            rx.await.unwrap_or(Event::Dismiss)
        },
        Message::Collab,
    )
}

/// The value of a text field of an element's flat JSON properties.
pub(super) fn field<'a>(props: &'a str, key: &str) -> Option<&'a str> {
    let start = props.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let rest = &props[start..];
    let mut end = 0;
    let bytes = rest.as_bytes();
    while end < bytes.len() && bytes[end] != b'"' {
        end += if bytes[end] == b'\\' { 2 } else { 1 };
    }
    rest.get(..end)
}

/// An element's name for a list: its name, else its class.
fn element_name(model: &Model, id: u64) -> String {
    let props = model.props.get(&id).map(String::as_str).unwrap_or("");
    let (name, class) = (field(props, "Name").unwrap_or("").trim(), field(props, "IfcType").unwrap_or(""));
    match (name.is_empty(), class.is_empty()) {
        (false, false) => format!("{name} ({class})"),
        (false, true) => name.to_string(),
        (true, false) => class.to_string(),
        (true, true) => format!("#{id}"),
    }
}

/// The clash tests of a rule set on a model: its elements as solids, each rule's two sides, the
/// clashes found, each pair once over all the tests (with the first that finds it), each on its
/// own until [`group_clashes`] puts them together.
fn run_clashes(model: &Model, models: &[(String, std::ops::Range<usize>)], rules: &[ClashRule], cancel: &AtomicBool) -> Vec<ClashRow> {
    struct Item {
        id: u64,
        model: usize,
        layer: usize,
        class: String,
        solid: Solid,
    }
    let mut items: Vec<Item> = Vec::new();
    for (l, layer) in model.layers.iter().enumerate().filter(|(_, layer)| !layer.points) {
        let which = models.iter().position(|(_, range)| range.contains(&l)).unwrap_or(0);
        let point = |i: u32| {
            let k = i as usize * 3;
            layer.positions.get(k..k + 3).map(|p| Vec3::new(p[0], p[1], p[2]))
        };
        for &(id, first, count) in &layer.elements {
            let indices = layer.indices.get(first as usize..(first + count) as usize).unwrap_or(&[]);
            let triangles = indices.chunks_exact(3).filter_map(|t| Some([point(t[0])?, point(t[1])?, point(t[2])?])).collect();
            let Some(solid) = Solid::new(triangles) else { continue };
            // an element drawn in several layers is one element: its parts join
            match items.iter_mut().rev().take(8).find(|item| item.id == id) {
                Some(item) => {
                    item.solid.min = item.solid.min.min(solid.min);
                    item.solid.max = item.solid.max.max(solid.max);
                    item.solid.triangles.extend(solid.triangles);
                }
                None => {
                    let class = model.props.get(&id).and_then(|p| field(p, "IfcType")).unwrap_or("").to_string();
                    items.push(Item { id, model: which, layer: l, class, solid });
                }
            }
        }
    }
    let levels = &model.project.levels;
    let mut rows: Vec<ClashRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for rule in rules.iter().filter(|r| r.on) {
        let side = |s: &Selector| -> Vec<usize> {
            let takes = |item: &Item| {
                s.takes_element(item.id, &models[item.model.min(models.len().saturating_sub(1))].0, &model.layers[item.layer].name, &item.class)
                    && (s.names.is_empty() || s.takes_name(model.props.get(&item.id).and_then(|p| field(p, "Name")).unwrap_or("")))
            };
            (0..items.len()).filter(|&k| models.is_empty() || takes(&items[k])).collect()
        };
        // the test's scope: its storeys and its property, on both sides
        let storey = |item: &Item| -> &str {
            let foot = item.solid.min.z + 0.3;
            let under = levels.iter().filter(|l| l.1 <= foot).max_by(|x, y| x.1.total_cmp(&y.1));
            under.or_else(|| levels.iter().min_by(|x, y| x.1.total_cmp(&y.1))).map_or("", |l| l.0.as_str())
        };
        let in_scope = |k: &usize| {
            let item = &items[*k];
            (rule.storeys.trim().is_empty() || rule.takes_storey(storey(item)))
                && (rule.property.is_empty() || model.props.get(&item.id).and_then(|p| field(p, &rule.property)).is_some_and(|v| crate::collab::rules::matches(&rule.value, v)))
        };
        let (a, b): (Vec<usize>, Vec<usize>) = (side(&rule.a).into_iter().filter(in_scope).collect(), side(&rule.b).into_iter().filter(in_scope).collect());
        let solids = |side: &[usize]| side.iter().map(|&k| items[k].solid.clone()).collect::<Vec<_>>();
        if rule.kind == ClashKind::Sight {
            // each element of the first side has to be seen from the room it is in: from places
            // at eye height all over it (or, without a room, around it), no further off than the
            // test says; what of the second side stands in more of those lines than the test
            // allows hides it. What an eye sees through is no hindrance.
            let lower = |k: usize| items[k].class.to_ascii_lowercase();
            let open = |k: usize| ["ifcspace", "ifcopening", "ifcwindow", "ifcvirtual", "ifccurtainwall", "ifcrailing"].iter().any(|c| lower(k).starts_with(c));
            let rooms: Vec<usize> = (0..items.len()).filter(|&k| lower(k) == "ifcspace").collect();
            for &t in &a {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if !rule.takes_sight_name(model.props.get(&items[t].id).and_then(|p| field(p, "Name")).unwrap_or("")) {
                    continue;
                }
                let target = &items[t].solid;
                let centre = (target.min + target.max) / 2.0;
                let room = rooms.iter().map(|&k| &items[k].solid).filter(|s| centre.cmpge(s.min - Vec3::splat(0.3)).all() && centre.cmple(s.max + Vec3::splat(0.3)).all()).min_by(|x, y| (x.max - x.min).length_squared().total_cmp(&(y.max - y.min).length_squared()));
                let reach = rule.sight_m.max(1.0);
                let (lo, hi, floor) = match room {
                    Some(r) => (r.min, r.max, r.min.z),
                    None => (centre - Vec3::splat(reach), centre + Vec3::splat(reach), centre.z - 2.2),
                };
                let eye = floor + 1.6;
                let (mut places, mut hidden) = (0usize, 0usize);
                let mut by: HashMap<usize, usize> = HashMap::new();
                const GRID: usize = 6;
                for i in 0..GRID {
                    for j in 0..GRID {
                        let at = |lo: f32, hi: f32, k: usize| lo + (hi - lo) * (k as f32 + 0.5) / GRID as f32;
                        let from = Vec3::new(at(lo.x, hi.x, i), at(lo.y, hi.y, j), eye);
                        let far = from.distance(centre);
                        if far > reach || far < 0.5 {
                            continue;
                        }
                        places += 1;
                        // up to just before it: it does not hide itself
                        let to = centre + (from - centre) / far * 0.1;
                        if let Some(&k) = b.iter().find(|&&k| k != t && !open(k) && clash::segment_hits(&items[k].solid, from, to)) {
                            hidden += 1;
                            *by.entry(k).or_default() += 1;
                        }
                    }
                }
                let Some((&most, _)) = by.iter().max_by_key(|(k, n)| (**n, std::cmp::Reverse(**k))) else { continue };
                if places == 0 || (hidden * 100) as f32 <= rule.sight_blocked_pct * places as f32 {
                    continue;
                }
                let (ia, ib) = (&items[t], &items[most]);
                if !seen.insert((ia.id.min(ib.id), ia.id.max(ib.id))) {
                    continue;
                }
                rows.push(ClashRow {
                    rule: rule.name.clone(),
                    a: ia.id,
                    b: ib.id,
                    a_name: element_name(model, ia.id),
                    b_name: element_name(model, ib.id),
                    kind: Kind::Hidden,
                    at: centre.to_array(),
                    // the share of the places it is not seen from
                    distance: hidden as f32 / places as f32,
                    decision: Decision::Open,
                    group: rows.len(),
                    copy: false,
                    key: String::new(),
                    fresh: false,
                    reach: Reach::Partly,
                });
            }
            continue;
        }
        let (sa, sb) = (solids(&a), if rule.within() { Vec::new() } else { solids(&b) });
        // a test between models leaves the pairs of one model out, when there is more than one
        let apart = rule.between_models && models.len() > 1;
        let skip = |i: usize, j: usize| apart && items[a[i]].model == items[b[j]].model;
        let found = clash::clashes_but(&sa, if rule.within() { &sa } else { &sb }, rule.within(), rule.test(), &skip, cancel);
        for c in found {
            let (ia, ib) = (&items[a[c.a]], &items[b[c.b]]);
            // a pair two tests find is one clash: it stays with the first
            // their classes may cross further than the test allows
            if c.kind == Kind::Crossing && rule.pair_tolerance(&ia.class, &ib.class).is_some_and(|t| c.distance <= t) {
                continue;
            }
            if ia.id == ib.id || !seen.insert((ia.id.min(ib.id), ia.id.max(ib.id))) {
                continue;
            }
            let same = |p: Vec3, q: Vec3| (p - q).abs().max_element() < 0.005;
            let copy = c.kind != Kind::Near && ia.class == ib.class && same(ia.solid.min, ib.solid.min) && same(ia.solid.max, ib.solid.max);
            rows.push(ClashRow {
                rule: rule.name.clone(),
                a: ia.id,
                b: ib.id,
                a_name: element_name(model, ia.id),
                b_name: element_name(model, ib.id),
                kind: c.kind,
                at: c.at.to_array(),
                distance: c.distance,
                decision: Decision::Open,
                group: rows.len(),
                copy,
                key: String::new(),
                fresh: false,
                reach: if copy { Reach::Through } else { Reach::of(c.kind, c.distance, (ia.solid.min, ia.solid.max), (ib.solid.min, ib.solid.max)) },
            });
            if rows.len() >= MOST_CLASHES {
                return rows;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
    }
    rows
}

/// Checks the files of the models open on a rule set, several at a time: each file is read and
/// checked on its own, so they share the processor's cores. The answers come in the files' order.
fn check_files(files: Vec<(usize, String, PathBuf)>, rules: &RuleSet, cancel: &AtomicBool) -> Vec<Checked> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done: std::sync::Mutex<Vec<(usize, Checked)>> = std::sync::Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8).min(files.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some((model, name, path)) = files.get(k) else { break };
                let report = std::panic::catch_unwind(|| check_file(path, rules, cancel)).unwrap_or_else(|_| Err("The check stopped unexpectedly".into()));
                let checked = Checked { model: *model, name: name.clone(), report: report.map(Arc::new) };
                done.lock().unwrap_or_else(|e| e.into_inner()).push((k, checked));
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(|e| e.into_inner());
    done.sort_by_key(|(k, _)| *k);
    done.into_iter().map(|(_, checked)| checked).collect()
}

/// Checks one IFC file on a rule set: each IDS in turn, the built-in checks with the first.
fn check_file(path: &std::path::Path, rules: &RuleSet, cancel: &AtomicBool) -> Result<ValidationReport, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let ils = rules.ils.then(|| IlsOptions {
        storey_name_pattern: if rules.storey_pattern.trim().is_empty() { bim_validator_core::DEFAULT_STOREY_PATTERN.into() } else { rules.storey_pattern.clone() },
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
        ..IlsOptions::default()
    });
    let mut runs: Vec<Option<String>> = Vec::new();
    for ids in &rules.ids {
        runs.push(Some(crate::collab::rules::ids_text(ids)?));
    }
    if runs.is_empty() {
        runs.push(None);
    }
    let mut whole: Option<ValidationReport> = None;
    for (k, ids_xml) in runs.into_iter().enumerate() {
        let first = k == 0;
        let rooms = rules.rooms.on.then(|| bim_validator_core::RoomOptions { door_width: f64::from(rules.rooms.door_width), door_height: f64::from(rules.rooms.door_height), threshold: f64::from(rules.rooms.threshold), ramp_run: f64::from(rules.rooms.ramp_run) });
        let options = ValidationOptions { ids_xml, model_checks: first && rules.model_checks, ils: ils.clone().filter(|_| first), rooms: rooms.filter(|_| first) };
        let report = bim_validator_core::validate_model(&bytes, &options, cancel)?;
        match &mut whole {
            None => whole = Some(report),
            Some(all) => {
                // the specifications of this IDS join; a failure of one fails the whole
                if report.status == ValidationStatus::Failed || (report.status == ValidationStatus::NotCheckable && all.status == ValidationStatus::Passed) {
                    all.status = report.status;
                }
                let at = all.specifications.iter().position(|s| s.source != bim_validator_core::RuleSource::Ids).unwrap_or(all.specifications.len());
                all.specifications.splice(at..at, report.specifications);
            }
        }
    }
    whole.ok_or_else(|| "Nothing to check".to_string())
}

/// The quality of what was checked as a number from 0 to 100 and a letter (A from 90, B from 80,
/// C from 70, D from 60, else E): of each rule the share of its elements that pass, a rule that
/// fails the report counting twice as much as a warning. None when nothing was checked.
pub fn quality_score<'a>(specs: impl Iterator<Item = &'a SpecificationResult>) -> Option<(u8, char)> {
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for s in specs.filter(|s| s.status != ValidationStatus::NotCheckable) {
        let w = if s.severity == Severity::Warning { 1.0 } else { 2.0 };
        let share = if s.applicable_count > 0 { s.passed_count as f64 / s.applicable_count as f64 } else if s.status == ValidationStatus::Passed { 1.0 } else { 0.0 };
        sum += w * share;
        weight += w;
    }
    (weight > 0.0).then(|| {
        let score = (100.0 * sum / weight).round().clamp(0.0, 100.0) as u8;
        (score, match score { 90.. => 'A', 80.. => 'B', 70.. => 'C', 60.. => 'D', _ => 'E' })
    })
}

/// The findings of a report as CSV.
fn report_csv(checked: &[Checked]) -> String {
    fn cell(value: &str) -> String {
        let mut value = value.to_string();
        // a spreadsheet takes these as the start of a formula
        if value.trim_start().starts_with(['=', '+', '-', '@']) {
            value.insert(0, '\'');
        }
        format!("\"{}\"", value.replace('"', "\"\""))
    }
    let mut out = "model,specification,code,severity,status,express_id,global_id,entity_type,name,storey,reason\r\n".to_string();
    for c in checked {
        let Ok(report) = &c.report else { continue };
        for spec in &report.specifications {
            let head = format!("{},{},{},{}", cell(&c.name), cell(&spec.name), cell(spec.identifier.as_deref().unwrap_or("")), if spec.severity == Severity::Warning { "warning" } else { "error" });
            if spec.findings.is_empty() {
                out.push_str(&format!("{head},{},,,,,,{}\r\n", status_key(spec.status), cell(&spec.reasons.join("; "))));
            }
            for f in &spec.findings {
                out.push_str(&format!(
                    "{head},{},{},{},{},{},{},{}\r\n",
                    status_key(f.status),
                    f.express_id,
                    cell(f.global_id.as_deref().unwrap_or("")),
                    cell(&f.entity_type),
                    cell(f.name.as_deref().unwrap_or("")),
                    cell(f.storey.as_deref().unwrap_or("")),
                    cell(&f.reason)
                ));
            }
        }
    }
    out
}

fn status_key(status: ValidationStatus) -> &'static str {
    match status {
        ValidationStatus::Passed => "passed",
        ValidationStatus::Failed => "failed",
        ValidationStatus::NotCheckable => "not_checkable",
    }
}

/// The statuses and priorities an issue can have, as BCF names them.
const STATUSES: [&str; 4] = ["Open", "In Progress", "Resolved", "Closed"];
const PRIORITIES: [&str; 4] = ["Low", "Normal", "High", "Critical"];

fn status_label(status: &str, l: Language) -> String {
    match status.to_ascii_lowercase().as_str() {
        "open" | "active" => l.text("Open", "Open").to_string(),
        "in progress" | "inprogress" => l.text("In behandeling", "In progress").to_string(),
        "resolved" | "done" => l.text("Opgelost", "Resolved").to_string(),
        "closed" => l.text("Gesloten", "Closed").to_string(),
        _ => status.to_string(),
    }
}

fn priority_label(priority: &str, l: Language) -> String {
    match priority.to_ascii_lowercase().as_str() {
        "low" => l.text("Laag", "Low").to_string(),
        "normal" | "medium" => l.text("Normaal", "Normal").to_string(),
        "high" | "major" => l.text("Hoog", "High").to_string(),
        "critical" => l.text("Kritiek", "Critical").to_string(),
        _ => priority.to_string(),
    }
}

fn status_tone(status: &str) -> Option<Tone> {
    match status.to_ascii_lowercase().as_str() {
        "open" | "active" => Some(Tone::Error),
        "in progress" | "inprogress" => Some(Tone::Warning),
        "resolved" | "done" => Some(Tone::Info),
        "closed" => Some(Tone::Success),
        _ => None,
    }
}

impl App {
    /// The elements of the model by GlobalId, worked out once per model.
    fn collab_guids(&mut self) -> Arc<HashMap<String, u64>> {
        let Some(model) = self.model.clone() else { return Arc::default() };
        if let Some((id, known)) = &self.collab.guids {
            if *id == model.id {
                return known.clone();
            }
        }
        let index: HashMap<String, u64> = model.props.iter().filter_map(|(&id, props)| Some((field(props, "GlobalId")?.to_string(), id))).collect();
        let index = Arc::new(index);
        self.collab.guids = Some((model.id, index.clone()));
        index
    }

    pub(super) fn collab_guid_of(&self, id: u64) -> Option<String> {
        self.model.as_ref()?.props.get(&id).and_then(|p| field(p, "GlobalId")).map(str::to_string)
    }

    /// The model's place in the world: BCF's coordinates are the model's own, in metres.
    fn collab_to_display(&self, world: [f64; 3]) -> Vec3 {
        let world = DVec3::from_array(world);
        self.model.as_ref().and_then(|m| m.origins.first().copied()).map_or(world, |o| o.display(world)).as_vec3()
    }

    pub(super) fn collab_to_world(&self, display: Vec3) -> [f64; 3] {
        let display = display.as_dvec3();
        self.model.as_ref().and_then(|m| m.origins.first().copied()).map_or(display, |o| o.world(display)).to_array()
    }

    /// Selects these elements and nothing else.
    fn collab_select(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.sel.clear();
        self.sel.extend(ids.into_iter().map(Pickable::Element));
        self.selection_changed();
    }

    /// Moves the camera so the selection fills the view.
    fn collab_frame_selection(&mut self) {
        if let Some(bounds) = self.selection_bounds() {
            let mut to = self.camera;
            to.fit(&bounds, self.aspect());
            self.anim = Some((self.camera, to, Instant::now()));
        }
    }

    /// What a clash test can take as a side, worked out once per model and language: everything,
    /// the structure and the services, each model, each layer and each class of the model.
    pub(crate) fn collab_refresh_sides(&mut self) {
        let l = self.collab.language;
        // a coordination file brought its issues: they come in, unless issues are being worked on
        if self.collab.issues.topics.is_empty() && !self.collab.issues_changed {
            if let Some((path, issues)) = crate::loaders::take_file_issues() {
                let count = issues.topics.len();
                self.collab.issues = issues;
                self.collab.issues_path = None;
                self.collab.topic = None;
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.collab_note(Tone::Success, format!("{count} {} {name}", l.text("issues uit", "issues from")));
            }
        }
        let key = self.model.as_ref().map(|m| (m.id, l));
        if self.collab.sides_for == key && !self.collab.sides.is_empty() && self.collab.model_names.len() == self.models.len() {
            return;
        }
        let mut sides = vec![
            Side { label: l.text("Alles", "Everything").to_string(), selector: Selector::default() },
            Side { label: l.text("Constructie", "Structure").to_string(), selector: Selector::classes(&crate::collab::rules::STRUCTURE) },
            Side { label: l.text("Installaties", "Services").to_string(), selector: Selector::classes(&crate::collab::rules::SERVICES) },
        ];
        if let Some(model) = &self.model {
            if self.models.len() > 1 {
                for entry in &self.models {
                    let name = entry.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    sides.push(Side { label: format!("{}: {name}", l.text("Model", "Model")), selector: Selector { models: vec![name], ..Selector::default() } });
                }
            }
            for layer in model.layers.iter().filter(|layer| !layer.elements.is_empty()) {
                sides.push(Side { label: format!("{}: {}", l.text("Laag", "Layer"), layer.name), selector: Selector { layers: vec![layer.name.clone()], ..Selector::default() } });
            }
            let classes: std::collections::BTreeSet<&str> = model.props.values().filter_map(|p| field(p, "IfcType")).collect();
            for class in classes {
                sides.push(Side { label: format!("{}: {class}", l.text("Klasse", "Class")), selector: Selector::classes(&[class]) });
            }
        }
        self.collab.model_names = self.models.iter().enumerate().map(|(k, e)| Choice(k, e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())).collect();
        self.collab.sides = sides;
        self.collab.sides_for = key;
    }

    /// Goes to elements a list names: selects them, alone when the panel isolates, and frames
    /// them.
    fn collab_go_to(&mut self, ids: impl IntoIterator<Item = u64>) {
        if self.collab.isolate {
            self.show_all_elements();
        }
        self.collab_select(ids);
        if self.collab.isolate {
            self.isolate_selection();
        }
        self.collab_frame_selection();
    }

    /// The lines the 3D view draws for the clash picked: the boxes of its two elements, the
    /// first red and the second blue, and a cross where they clash.
    pub(super) fn collab_marks(&self) -> Vec<crate::views3d::Colored> {
        const FIRST: [u8; 4] = [214, 40, 40, 255];
        const SECOND: [u8; 4] = [30, 100, 220, 255];
        const AT: [u8; 4] = [255, 140, 0, 255];
        const ROUTE: [u8; 4] = [20, 150, 60, 255];
        let Some(model) = self.model.as_ref() else { return Vec::new() };
        let matrix = self.model_matrix();
        let world = |p: Vec3| matrix.transform_point3(p).to_array();
        let mut lines = Vec::new();
        // the walk of the room picked to its way out, a little above the floor
        if let Some(walk) = self.collab.route_shown.and_then(|id| self.collab.routes.get(&id)) {
            let up = |p: &[f32; 3]| world(Vec3::from_array(*p) + Vec3::Z * 0.1);
            lines.extend(walk.windows(2).map(|w| ([up(&w[0]), up(&w[1])], ROUTE)));
        }
        let Some(clash) = self.collab.clash.and_then(|k| self.collab.clashes.get(k)) else { return lines };
        for (id, color) in [(clash.a, FIRST), (clash.b, SECOND)] {
            let Some(bounds) = model.element_bounds(id) else { continue };
            let (lo, hi) = (Vec3::from_array(bounds.min), Vec3::from_array(bounds.max));
            let corner = |i: usize| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                lines.push(([world(corner(a)), world(corner(b))], color));
            }
        }
        let at = Vec3::from_array(clash.at);
        let reach = model.element_bounds(clash.a).map_or(0.3, |b| (b.radius() * 0.08).clamp(0.08, 0.5));
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            lines.push(([world(at - axis * reach), world(at + axis * reach)], AT));
        }
        lines
    }

    /// Shows a viewpoint: what it hides, selects and cuts, from where it looks.
    fn collab_show_viewpoint(&mut self, view: &Viewpoint) {
        let guids = self.collab_guids();
        let ids = |list: &[String]| -> Vec<u64> { list.iter().filter_map(|g| guids.get(g).copied()).collect() };
        self.show_all_elements();
        let exceptions = ids(&view.exceptions);
        if !exceptions.is_empty() {
            if view.default_visibility {
                let hidden = exceptions.iter().map(|&id| self.unit_key(crate::explode::UnitOwner::Element(id))).collect();
                self.hidden = Arc::new(hidden);
                self.update_units();
            } else {
                self.collab_select(exceptions);
                self.isolate_selection();
            }
        }
        self.collab_select(ids(&view.selection));
        // its dimensions and remarks come back in place of what was measured
        if !(view.lines.is_empty() && view.notes.is_empty() && view.marks.is_empty()) {
            let lines = view.lines.iter().map(|(from, to)| crate::measure::Measurement::Distance(crate::measure::geometry::Distance { p1: self.collab_to_display(*from), p2: self.collab_to_display(*to), extensions: Vec::new() }));
            let notes = view.notes.iter().map(|(at, text)| crate::measure::Measurement::Note { at: self.collab_to_display(*at), text: text.clone() });
            let marks = view.marks.iter().filter_map(|(kind, points)| Some(crate::measure::Measurement::Mark { kind: crate::measure::MarkKind::parse(kind)?, points: points.iter().map(|p| self.collab_to_display(*p)).collect() }));
            self.measure.results = lines.chain(notes).chain(marks).collect();
        }
        // the first plane cuts; the side its direction points to is left out
        self.plane = view.clipping_planes.first().and_then(|plane| {
            let normal = Vec3::new(plane.direction[0] as f32, plane.direction[1] as f32, plane.direction[2] as f32).try_normalize()?;
            Some(SectionPlane { normal, offset: normal.dot(self.collab_to_display(plane.location)) })
        });
        match view.camera {
            Some(camera) => {
                let Some(f) = Vec3::new(camera.direction[0] as f32, camera.direction[1] as f32, camera.direction[2] as f32).try_normalize() else { return };
                let eye = self.collab_to_display(camera.position);
                // a viewpoint of another model, or of other coordinates: far outside this one
                if let Some(bounds) = self.model.as_ref().map(|m| m.bounds) {
                    if eye.distance(Vec3::from_array(bounds.center())) > bounds.radius() * 20.0 + 100.0 {
                        let l = self.collab.language;
                        self.collab_note(Tone::Warning, l.text("Het standpunt van dit issue ligt ver buiten het model: de camera blijft waar hij is.", "The viewpoint of this issue lies far outside the model: the camera stays where it is."));
                        return;
                    }
                }
                // it looks at what it selects, else at the model
                let aim = self.selection_bounds().or_else(|| self.model.as_ref().map(|m| m.bounds)).map(|b| Vec3::from_array(b.center()));
                let max_pitch = 89.9f32.to_radians();
                let mut to = self.camera;
                to.yaw = (-f.y).atan2(-f.x);
                to.pitch = (-f.z).clamp(-1.0, 1.0).asin().clamp(-max_pitch, max_pitch);
                match camera.lens {
                    Lens::Perspective { field_of_view } => {
                        to.ortho = false;
                        to.fov_y = (field_of_view as f32).clamp(5.0, 120.0).to_radians();
                        to.distance = aim.map_or(10.0, |a| (a - eye).dot(f)).max(1.0);
                    }
                    Lens::Orthogonal { view_to_world_scale } => {
                        to.ortho = true;
                        // the height it sees is what the lens sees at its distance
                        to.distance = (view_to_world_scale as f32 / 2.0 / (to.fov_y / 2.0).tan()).max(0.5);
                    }
                }
                to.target = eye + f * to.distance;
                self.anim = Some((self.camera, to, Instant::now()));
            }
            None => self.collab_frame_selection(),
        }
    }

    /// The view as it is, as a viewpoint without a picture.
    fn collab_viewpoint_now(&self) -> Viewpoint {
        let mut view = Viewpoint::new();
        let eye = self.camera.eye();
        let f = (self.camera.target - eye).try_normalize().unwrap_or(Vec3::X);
        let up = (Vec3::Z - f * Vec3::Z.dot(f)).try_normalize().unwrap_or(Vec3::Y);
        let lens = if self.camera.ortho {
            Lens::Orthogonal { view_to_world_scale: (2.0 * self.camera.distance * (self.camera.fov_y / 2.0).tan()) as f64 }
        } else {
            Lens::Perspective { field_of_view: self.camera.fov_y.to_degrees() as f64 }
        };
        view.camera = Some(ViewCamera { position: self.collab_to_world(eye), direction: f.as_dvec3().to_array(), up: up.as_dvec3().to_array(), lens });
        view.selection = self.sel.items().iter().filter_map(|item| if let Pickable::Element(id) = item { self.collab_guid_of(*id) } else { None }).collect();
        if let Some(model) = &self.model {
            view.exceptions = self.hidden.iter().filter_map(|key| if let UnitKey::Element(k) = key { model.element_with_key(*k) } else { None }).filter_map(|id| self.collab_guid_of(id)).collect();
            view.exceptions.sort();
        }
        if let Some(plane) = self.plane {
            view.clipping_planes.push(ClippingPlane { location: self.collab_to_world(plane.normal * plane.offset), direction: plane.normal.as_dvec3().to_array() });
        }
        // what is measured and remarked in the view goes with it
        for result in &self.measure.results {
            match result {
                crate::measure::Measurement::Distance(d) => view.lines.push((self.collab_to_world(d.p1), self.collab_to_world(d.p2))),
                crate::measure::Measurement::Note { at, text } => view.notes.push((self.collab_to_world(*at), text.clone())),
                crate::measure::Measurement::Mark { kind, points } => view.marks.push((kind.key().to_string(), points.iter().map(|p| self.collab_to_world(*p)).collect())),
                _ => {}
            }
        }
        view
    }

    /// A viewpoint that looks at a place from where the camera looks now, with elements selected.
    pub(super) fn collab_viewpoint_at(&self, at: Vec3, reach: f32, ids: &[u64]) -> Viewpoint {
        let mut view = Viewpoint::new();
        let f = (self.camera.target - self.camera.eye()).try_normalize().unwrap_or(Vec3::new(-0.5, 0.6, -0.6).normalize());
        let up = (Vec3::Z - f * Vec3::Z.dot(f)).try_normalize().unwrap_or(Vec3::Y);
        let eye = at - f * (reach * 3.0).max(2.0);
        view.camera = Some(ViewCamera { position: self.collab_to_world(eye), direction: f.as_dvec3().to_array(), up: up.as_dvec3().to_array(), lens: Lens::Perspective { field_of_view: 45.0 } });
        view.selection = ids.iter().filter_map(|&id| self.collab_guid_of(id)).collect();
        view
    }

    fn collab_topic_mut(&mut self) -> Option<&mut Topic> {
        let guid = self.collab.topic.clone()?;
        self.collab.issues_changed = true;
        self.collab.issues.topics.iter_mut().find(|t| t.guid == guid)
    }

    pub(super) fn collab_note(&mut self, tone: Tone, message: impl Into<String>) {
        self.collab.notice = Some((tone, message.into()));
    }

    /// The topic of a clash: what clashes, and a viewpoint on it.
    fn collab_clash_topic(&self, row: &ClashRow, l: Language) -> Topic {
        let mut topic = Topic::new(&format!("{}: {} × {}", row.rule, row.a_name, row.b_name), &self.collab.author);
        topic.kind = "Clash".into();
        topic.priority = "Normal".into();
        let mm = (row.distance * 1000.0).round();
        topic.description = match row.kind {
            Kind::Crossing => format!("{} {mm} mm", l.text("De elementen doorsnijden elkaar over", "The elements cross by")),
            Kind::Inside => l.text("Het ene element ligt binnen het andere", "One element lies inside the other").to_string(),
            Kind::Near => format!("{} {mm} mm", l.text("De elementen liggen op", "The elements are apart by")),
            Kind::Hidden => format!("{} {:.0}% {}", l.text("Het eerste element is vanaf", "The first element is out of sight from"), row.distance * 100.0, l.text("van de plekken in zijn ruimte niet te zien: het tweede staat ervoor", "of the places in its room: the second stands before it")),
        };
        let reach = self.model.as_ref().and_then(|m| Some(m.element_bounds(row.a)?.radius().max(m.element_bounds(row.b)?.radius()))).unwrap_or(2.0);
        topic.viewpoints.push(self.collab_viewpoint_at(Vec3::from_array(row.at), reach.min(6.0), &[row.a, row.b]));
        topic
    }

    /// The key of a clash that lasts from one run to the next: its two elements by GlobalId
    /// (else by their number), the lesser first.
    fn collab_clash_key(&self, c: &ClashRow) -> String {
        let of = |id: u64| self.collab_guid_of(id).filter(|g| !g.is_empty()).unwrap_or_else(|| format!("#{id}"));
        let (a, b) = (of(c.a), of(c.b));
        if a <= b { format!("{a}|{b}") } else { format!("{b}|{a}") }
    }

    /// Keeps the clashes as they are now in the rule set, with what was decided on them, for the
    /// next run to compare with. Nothing when there are none: what was remembered stays.
    fn collab_remember(&mut self) {
        if self.collab.clashes.is_empty() || self.collab.partial_run {
            return;
        }
        let memory: Vec<crate::collab::rules::ClashMemo> = self
            .collab
            .clashes
            .iter()
            .map(|c| crate::collab::rules::ClashMemo { key: if c.key.is_empty() { self.collab_clash_key(c) } else { c.key.clone() }, decision: c.decision.key().into(), rule: c.rule.clone(), a: c.a_name.clone(), b: c.b_name.clone() })
            .collect();
        if let Some(rules) = self.collab.rules_mut() {
            rules.clash_memory = memory;
        }
    }

    /// Takes the clashes a run found: each gets its lasting key and, where the run before had
    /// it, the decision made on it then; the others are new. What the run before had and this one
    /// has not is solved. The run is added to the runs so far. A run of a selection only takes
    /// the decisions over.
    fn collab_take_clashes(&mut self, rows: &mut [ClashRow]) {
        let memory: Vec<crate::collab::rules::ClashMemo> = self.collab.rules().map(|r| r.clash_memory.clone()).unwrap_or_default();
        let known: HashMap<&str, &crate::collab::rules::ClashMemo> = memory.iter().map(|m| (m.key.as_str(), m)).collect();
        let compared = !memory.is_empty() && !self.collab.partial_run;
        let mut found = std::collections::HashSet::new();
        for c in rows.iter_mut() {
            c.key = self.collab_clash_key(c);
            match known.get(c.key.as_str()) {
                Some(memo) => c.decision = Decision::parse(&memo.decision).unwrap_or_default(),
                None => c.fresh = compared,
            }
            found.insert(c.key.clone());
        }
        if self.collab.partial_run {
            return;
        }
        self.collab.compared = compared;
        self.collab.resolved = memory.iter().filter(|m| !found.contains(&m.key)).cloned().collect();
        let date = crate::export::ifcx::timestamp();
        let run = crate::collab::rules::ClashRun {
            date: date.get(..16).unwrap_or(&date).replace('T', " "),
            total: rows.len(),
            new: rows.iter().filter(|c| c.fresh).count(),
            resolved: self.collab.resolved.len(),
            open: rows.iter().filter(|c| c.decision != Decision::Accepted).count(),
        };
        if let Some(rules) = self.collab.rules_mut() {
            rules.clash_runs.push(run);
            // the last hundred are enough to see where it goes
            let extra = rules.clash_runs.len().saturating_sub(100);
            rules.clash_runs.drain(..extra);
        }
    }

    /// What the issues and the decisions on the clashes are now, as one number: a change in them
    /// changes it.
    fn collab_share_mark(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for t in &self.collab.issues.topics {
            (&t.guid, &t.modified_date, &t.status, &t.title, t.comments.len(), t.viewpoints.len()).hash(&mut h);
        }
        for c in &self.collab.clashes {
            (&c.key, c.decision.key()).hash(&mut h);
        }
        self.collab.share_removed.hash(&mut h);
        h.finish()
    }

    /// Brings the issues and the decisions on clashes here and in the shared folder to the
    /// same; `say` tells what it did also when that is nothing.
    pub(super) fn collab_sync(&mut self, say: bool) {
        let Some(folder) = self.collab.share_folder.clone() else { return };
        let l = self.collab.language;
        let removed = std::mem::take(&mut self.collab.share_removed);
        let mut done = match crate::collab::share::sync_issues(&folder, &mut self.collab.issues, &removed) {
            Ok(done) => done,
            Err(e) => {
                self.collab.share_removed = removed;
                self.collab.share_seen = self.collab_share_mark();
                self.collab_note(Tone::Error, format!("{}: {e}", l.text("Delen lukt niet", "Sharing failed")));
                return;
            }
        };
        let mine: Vec<(String, String)> = self.collab.clashes.iter().map(|c| (c.key.clone(), c.decision.key().to_string())).collect();
        let now = bim_bcf::issues::now();
        let author = self.collab.author.clone();
        match crate::collab::share::sync_decisions(&folder, &mine, &mut self.collab.share_known, &author, &now, &mut done) {
            Ok(take) => {
                for (key, decision) in take {
                    if let (Some(c), Some(d)) = (self.collab.clashes.iter_mut().find(|c| c.key == key), Decision::parse(&decision)) {
                        c.decision = d;
                    }
                }
            }
            Err(e) => self.collab_note(Tone::Error, format!("{}: {e}", l.text("Delen lukt niet", "Sharing failed"))),
        }
        if self.collab.topic.as_ref().is_some_and(|g| !self.collab.issues.topics.iter().any(|t| &t.guid == g)) {
            self.collab.topic = None;
        }
        self.collab.share_seen = self.collab_share_mark();
        if say || done.pulled + done.removed + done.decisions_pulled > 0 {
            let got = done.pulled + done.removed + done.decisions_pulled;
            let sent = done.pushed + done.decisions_pushed;
            self.collab_note(Tone::Success, format!("{}: {got} {} · {sent} {}", l.text("Gedeelde map", "Shared folder"), l.text("opgehaald", "taken over"), l.text("weggeschreven", "written")));
        }
    }

    /// The groups of the clashes, each as the places of its rows, in their order.
    fn collab_groups(&self) -> Vec<Vec<usize>> {
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (k, c) in self.collab.clashes.iter().enumerate() {
            match groups.last_mut() {
                Some(last) if self.collab.clashes[last[0]].group == c.group => last.push(k),
                _ => groups.push(vec![k]),
            }
        }
        groups
    }

    /// The topic of a group of clashes: what they share, each clash in the description, and a
    /// viewpoint on all of them.
    fn collab_group_topic(&self, rows: &[&ClashRow], l: Language) -> Topic {
        let title = group_title(rows);
        let mut topic = Topic::new(&format!("{}: {title} ({} {})", rows[0].rule, rows.len(), l.text("clashes", "clashes")), &self.collab.author);
        topic.kind = "Clash".into();
        topic.priority = "Normal".into();
        topic.description = rows
            .iter()
            .take(30)
            .map(|r| format!("{} × {}: {} mm", r.a_name, r.b_name, (r.distance * 1000.0).round()))
            .chain((rows.len() > 30).then(|| format!("+ {}", rows.len() - 30)))
            .collect::<Vec<_>>()
            .join("\n");
        let middle = rows.iter().map(|r| Vec3::from_array(r.at)).sum::<Vec3>() / rows.len() as f32;
        let reach = rows.iter().map(|r| Vec3::from_array(r.at).distance(middle)).fold(1.0f32, f32::max);
        let mut ids: Vec<u64> = rows.iter().flat_map(|r| [r.a, r.b]).collect();
        ids.sort_unstable();
        ids.dedup();
        topic.viewpoints.push(self.collab_viewpoint_at(middle, reach.min(30.0), &ids));
        topic
    }

    /// The viewport out of a picture of the window, as a JPEG at most 1600 pixels wide.
    fn viewport_picture(&self, shot: &iced::window::Screenshot) -> Option<crate::export::report::Jpeg> {
        let s = shot.scale_factor;
        let (w, h) = (shot.size.width, shot.size.height);
        if w == 0 || h == 0 {
            return None;
        }
        let x = ((self.viewport_origin.0 * s).round().max(0.0) as u32).min(w - 1);
        let y = ((self.viewport_origin.1 * s).round().max(0.0) as u32).min(h - 1);
        let (cw, ch) = (((self.viewport_size.0 * s).round() as u32).clamp(1, w - x), ((self.viewport_size.1 * s).round() as u32).clamp(1, h - y));
        let mut rgb = Vec::with_capacity((cw * ch * 3) as usize);
        for row in y..y + ch {
            let from = ((row * w + x) * 4) as usize;
            for px in shot.rgba.get(from..from + (cw * 4) as usize).unwrap_or(&[]).chunks_exact(4) {
                rgb.extend_from_slice(&px[..3]);
            }
        }
        if rgb.len() != (cw * ch * 3) as usize {
            return None;
        }
        super::reports::jpeg(cw, ch, rgb, 1600)
    }

    /// Begins a PDF report: with the checks and clashes, the pictures are taken first (the view
    /// as it is for the cover, then each group of clashes shown); the issues alone have theirs.
    fn report_begin(&mut self, path: PathBuf, scope: super::reports::Scope) -> Task<Message> {
        let mut todo: Vec<Shot> = Vec::new();
        if self.model.is_some() {
            if scope == super::reports::Scope::All {
                todo.push(Shot::Cover);
                todo.extend(self.report_pictured().into_iter().map(Shot::Group));
            }
            // the issues without a picture of their own: one from their viewpoint
            let bare = self.collab.issues.topics.iter().enumerate().filter(|(_, t)| t.viewpoints.first().is_some_and(|v| v.snapshot.is_none())).map(|(k, _)| Shot::Issue(k));
            todo.extend(bare.take(super::reports::PICTURED));
        }
        self.collab.report_job = Some(ReportJob {
            path,
            scope,
            todo,
            pictures: HashMap::new(),
            issue_pictures: HashMap::new(),
            cover: None,
            camera: self.camera,
            selection: self.sel.items().to_vec(),
            hidden: self.hidden.clone(),
            plane: self.plane,
            measured: self.measure.results.clone(),
            open_group: self.collab.open_group,
            clash: self.collab.clash,
            started: Instant::now(),
        });
        self.collab.running = true;
        self.report_next()
    }

    /// The next picture of the report, or, when they are all taken, the report written.
    fn report_next(&mut self) -> Task<Message> {
        let Some(job) = self.collab.report_job.as_ref() else { return Task::none() };
        let Some(&target) = job.todo.first() else {
            self.report_finish();
            return Task::none();
        };
        match target {
            // the cover: the view as it was, nothing selected
            Shot::Cover => {
                self.collab.clash_shown = None;
                self.camera = job.camera;
                self.anim = None;
                self.collab.open_group = None;
                self.collab.clash = None;
                self.sel.clear();
                self.selection_changed();
            }
            // a group: its elements selected and framed
            Shot::Group(g) => {
                self.collab.open_group = Some(g);
                self.collab.clash = None;
                self.sel.clear();
                self.selection_changed();
                self.collab_show_clashes(g);
            }
            // an issue: its viewpoint
            Shot::Issue(k) => {
                self.collab.clash_shown = None;
                let view = self.collab.issues.topics.get(k).and_then(|t| t.viewpoints.first().cloned());
                self.collab.open_group = None;
                self.collab.clash = None;
                if let Some(view) = view {
                    self.collab_show_viewpoint(&view);
                }
                self.finish_animation();
            }
        }
        self.refresh_overlay();
        // a few frames for the window to draw the view as it is now, then the picture
        #[cfg(not(target_arch = "wasm32"))]
        let wait = Task::perform(async { std::thread::sleep(std::time::Duration::from_millis(60)) }, |_| ());
        #[cfg(target_arch = "wasm32")]
        let wait = Task::done(());
        wait.then(|_| iced::window::latest().and_then(iced::window::screenshot)).map(|shot| Message::Collab(Event::ReportShot(Arc::new(shot))))
    }

    /// Shows the clashes of group `g` as an issue of them shows: the camera on where they
    /// clash, as near as their smaller elements ask, looking the way it looked; their elements
    /// red (the first of each clash) and blue, everything else see-through.
    fn collab_show_clashes(&mut self, g: usize) {
        let Some(model) = self.model.clone() else { return };
        let rows: Vec<ClashRow> = self.collab.clashes.iter().filter(|c| c.group == g).cloned().collect();
        // clashes spread over the building make a picture of nothing: of such a group the
        // picture shows those within a few metres of the one with the most others near it
        let near = |c: &ClashRow, d: &ClashRow| Vec3::from_array(c.at).distance(Vec3::from_array(d.at)) <= 4.0;
        let middle = rows.iter().max_by_key(|c| rows.iter().filter(|d| near(c, d)).count()).cloned();
        let rows: Vec<ClashRow> = match middle {
            Some(m) if rows.iter().any(|c| !near(&m, c)) => rows.into_iter().filter(|c| near(&m, c)).collect(),
            _ => rows,
        };
        let mut bounds = crate::model::Bounds::empty();
        for c in &rows {
            // the smaller of the two is what to look at: the pipe, not the floor it goes through
            let (a, b) = (model.element_bounds(c.a), model.element_bounds(c.b));
            let small = match (a, b) {
                (Some(a), Some(b)) => Some(if a.radius() <= b.radius() { a } else { b }),
                (a, b) => a.or(b),
            };
            let at = Vec3::from_array(c.at);
            let reach = small.map_or(0.5, |s| s.radius()).clamp(0.4, 4.0);
            bounds.extend((at - Vec3::splat(reach)).to_array());
            bounds.extend((at + Vec3::splat(reach)).to_array());
        }
        // what stands within reach shows as the surroundings; the rest is left out
        if self.collab.boxes.as_ref().map(|b| b.0) != Some(model.id) {
            let mut boxes = HashMap::new();
            for layer in &model.layers {
                for &(id, ..) in &layer.elements {
                    if !boxes.contains_key(&id) {
                        if let Some(b) = model.element_bounds(id) {
                            boxes.insert(id, b);
                        }
                    }
                }
            }
            self.collab.boxes = Some((model.id, Arc::new(boxes)));
        }
        let grow = bounds.radius() * 0.5;
        let (lo, hi) = (Vec3::from_array(bounds.min) - Vec3::splat(grow), Vec3::from_array(bounds.max) + Vec3::splat(grow));
        let reach = bounds.radius() * 4.0;
        self.collab.clash_near = self
            .collab
            .boxes
            .as_ref()
            .map(|(_, boxes)| boxes.iter().filter(|(_, b)| Vec3::from_array(b.min).cmple(hi).all() && Vec3::from_array(b.max).cmpge(lo).all() && b.radius() <= reach.max(6.0)).map(|(&id, _)| id).collect())
            .unwrap_or_default();
        // the element most clashes of a group share
        let mut count: HashMap<u64, usize> = HashMap::new();
        for id in rows.iter().flat_map(|c| [c.a, c.b]) {
            *count.entry(id).or_default() += 1;
        }
        self.collab.clash_shared = count.into_iter().filter(|(_, n)| rows.len() > 1 && *n * 2 > rows.len()).max_by_key(|(_, n)| *n).map(|(id, _)| id);
        self.collab.clash_shown = Some((rows.iter().map(|c| c.a).collect(), rows.iter().map(|c| c.b).collect()));
        self.update_units();
        let mut to = self.camera;
        to.fit(&bounds, self.aspect());
        self.camera = to;
        self.anim = None;
    }

    /// The look of each unit while a picture of clashes is taken; None otherwise.
    pub(crate) fn clash_looks(&self, units: usize, owner: impl Fn(usize) -> Option<crate::explode::UnitOwner>) -> Option<Vec<[f32; 4]>> {
        let (first, second) = self.collab.clash_shown.as_ref()?;
        let (near, shared) = (&self.collab.clash_near, self.collab.clash_shared);
        let pack = |[r, g, b]: [u8; 3]| ((u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)) + 1) as f32;
        Some(
            (0..units)
                .map(|u| match owner(u).filter(|_| u > 0) {
                    Some(crate::explode::UnitOwner::Element(id)) if shared == Some(id) => [pack([0xE8, 0x9A, 0x94]), 0.0, 0.7, 0.0],
                    Some(crate::explode::UnitOwner::Element(id)) if first.contains(&id) => [pack([0xD0, 0x34, 0x2C]), 0.0, 0.0, 0.0],
                    Some(crate::explode::UnitOwner::Element(id)) if second.contains(&id) => [pack([0x2F, 0x6F, 0xDE]), 0.0, 0.0, 0.0],
                    Some(crate::explode::UnitOwner::Element(id)) if near.contains(&id) => [pack([0xC8, 0xC8, 0xC8]), 0.0, 0.8, 0.0],
                    _ if u > 0 => [pack([0xFF, 0xFF, 0xFF]), 0.0, 1.0, 0.0],
                    _ => [0.0; 4],
                })
                .collect(),
        )
    }

    /// The remote's PDF report: it begins, and the answer says so; the pictures and the file
    /// follow, while `collab_state` says `report: true`.
    pub(crate) fn collab_pdf_remote(&mut self, a: &serde_json::Value) -> (Result<serde_json::Value, String>, Task<Message>) {
        let Some(path) = a.get("path").and_then(|v| v.as_str()).map(PathBuf::from) else { return (Err("give the path".into()), Task::none()) };
        let kind = if a.get("kind").and_then(|v| v.as_str()) == Some("issues_pdf") { Report::IssuesPdf } else { Report::Pdf };
        self.collab.refresh();
        let task = self.collab_handle(Event::ExportTo(kind, Some(path.clone())));
        let pictures = self.collab.report_job.as_ref().map_or(0, |j| j.todo.len());
        (Ok(serde_json::json!({ "started": true, "path": path.display().to_string(), "pictures": pictures, "note": "the report is written when its pictures are taken; ask collab_state until report is false" })), task)
    }

    /// Writes the report with the pictures taken, and brings the view back as it was.
    fn report_finish(&mut self) {
        let Some(job) = self.collab.report_job.take() else { return };
        let l = self.collab.language;
        self.collab.running = false;
        self.collab.clash_shown = None;
        self.camera = job.camera;
        self.anim = None;
        self.collab.open_group = job.open_group;
        self.collab.clash = job.clash;
        self.hidden = job.hidden.clone();
        self.plane = job.plane;
        self.measure.results = job.measured.clone();
        self.update_units();
        self.sel.clear();
        self.sel.extend(job.selection.iter().copied());
        self.selection_changed();
        self.refresh_overlay();
        let taken = job.pictures.len() + job.issue_pictures.len() + usize::from(job.cover.is_some());
        let cover = self.report_cover(job.scope, l, job.cover);
        let blocks = self.report_blocks(job.scope, l, &job.pictures, &job.issue_pictures);
        let title = self.report_title(job.scope, l);
        let date = self.report_date();
        let pdf = crate::export::report::report_pdf(&cover, &title, &date, &blocks);
        match std::fs::write(&job.path, pdf) {
            Ok(()) => self.collab_note(
                Tone::Success,
                format!("{} {} · {} {} · {:.1} s", l.text("Opgeslagen:", "Saved:"), job.path.display(), taken, l.text("beelden", "pictures"), job.started.elapsed().as_secs_f32()),
            ),
            Err(e) => self.collab_note(Tone::Error, e.to_string()),
        }
    }

    /// A report as text: the checks as HTML, CSV or JSON, the issues as HTML, the clashes as CSV.
    fn collab_text_report(&self, kind: Report, l: Language) -> String {
        let reports = || self.collab.checked.iter().filter_map(|c| Some((c.name.as_str(), c.report.as_ref().ok()?)));
        match kind {
            Report::Csv => report_csv(&self.collab.checked),
            Report::Json => serde_json::to_string_pretty(&reports().map(|(name, r)| serde_json::json!({"model": name, "report": **r})).collect::<Vec<_>>()).unwrap_or_default(),
            Report::Html => {
                let language = if l == Language::Dutch { bim_validator_core::ReportLanguage::Dutch } else { bim_validator_core::ReportLanguage::English };
                // one model: its report; more: the reports one after the other
                reports().map(|(name, r)| bim_validator_core::report_html(r, name, language)).collect::<Vec<_>>().join("
")
            }
            Report::IssuesHtml => issues_html(&self.collab.issues, l),
            Report::ClashesCsv => {
                let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
                let mut out = "rule,group,kind,distance_mm,decision,a,b,a_global_id,b_global_id,x,y,z
".to_string();
                for c in &self.collab.clashes {
                    let kind = match c.kind {
                        _ if c.copy => "double",
                        Kind::Crossing => "crossing",
                        Kind::Inside => "inside",
                        Kind::Near => "clearance",
                        Kind::Hidden => "out of sight",
                    };
                    let at = self.collab_to_world(Vec3::from_array(c.at));
                    let guid = |id| self.collab_guid_of(id).unwrap_or_default();
                    out.push_str(&format!("{},{},{kind},{:.0},{},{},{},{},{},{:.3},{:.3},{:.3}
", quote(&c.rule), c.group + 1, c.distance * 1000.0, c.decision.key(), quote(&c.a_name), quote(&c.b_name), guid(c.a), guid(c.b), at[0], at[1], at[2]));
                }
                out
            }
            Report::Pdf | Report::Xlsx | Report::IssuesPdf => String::new(),
        }
    }

    pub(super) fn collab_update(&mut self, event: Event) -> Task<Message> {
        let looked = matches!(event, Event::Tab(Tab::Issues | Tab::Clashes));
        self.collab.refresh();
        if let Event::ClashAmount(k, typed) = &event {
            if let Some(kept) = self.collab.amounts.get_mut(*k) {
                *kept = typed.clone();
            }
        }
        if let Event::ClashSightShare(k, typed) = &event {
            if let Some(kept) = self.collab.sight_shares.get_mut(*k) {
                *kept = typed.clone();
            }
        }
        if let Event::RoomAmount(k, typed) = &event {
            if let Some(kept) = self.collab.room_amounts.get_mut(*k) {
                *kept = typed.clone();
            }
        }
        if let Event::MatrixAmount(typed) = &event {
            self.collab.matrix_amount = typed.clone();
        }
        let task = self.collab_handle(event);
        self.collab.refresh();
        self.collab_refresh_sides();
        self.inform_refresh();
        // a project shared through a folder: what changed here goes to it, and what the others
        // wrote comes in when the issues or the clashes are looked at
        if self.collab.share_folder.is_some() && !self.collab.running && (self.collab_share_mark() != self.collab.share_seen || looked) {
            self.collab_sync(false);
        }
        task
    }

    fn collab_handle(&mut self, event: Event) -> Task<Message> {
        let l = self.collab.language;
        if self.collab.rule_sets.is_empty() {
            self.collab.rule_sets = RuleSet::built_in(l == Language::Dutch);
        }
        // a file dialog answered: its notice goes
        if matches!(event, Event::Opened(..) | Event::SaveIssuesTo(_) | Event::SaveRulesTo(_) | Event::ExportTo(..)) && self.collab.dialog_open {
            self.collab.dialog_open = false;
            self.collab.notice = None;
        }
        let asks = matches!(event, Event::Open(_) | Event::SaveIssues | Event::SaveRules | Event::Export(_));
        let task = self.collab_dispatch(event);
        // a file dialog asked for: say it is coming until it answers
        if asks {
            self.collab.dialog_open = true;
            self.collab_note(Tone::Info, l.text("Het bestandsvenster wordt geopend; de eerste keer kan dat even duren.", "The file dialog is opening; the first time it can take a moment."));
        }
        task
    }

    fn collab_dispatch(&mut self, event: Event) -> Task<Message> {
        let l = self.collab.language;
        match event {
            Event::Tab(tab) => self.collab.tab = tab,
            Event::Dismiss => self.collab.notice = None,
            Event::Cancel => {
                self.collab.cancel.store(true, Ordering::Relaxed);
                self.collab.run += 1;
                self.collab.running = false;
            }
            Event::Open(kind) => {
                let (name, extensions): (&str, &[&str]) = match kind {
                    File::Bcf => ("BCF", &["bcfzip", "bcf"]),
                    File::Ids => ("IDS", &["ids"]),
                    File::Rules => ("Rule set", &["json"]),
                };
                let (name, extensions) = (name.to_string(), extensions.iter().map(|e| e.to_string()).collect::<Vec<_>>());
                return ask_file(move || rfd::AsyncFileDialog::new().add_filter(&name, &extensions), false, move |path| Message::Collab(Event::Opened(kind, path)));
            }
            Event::Opened(_, None) => {}
            Event::Opened(File::Bcf, Some(path)) => match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| Issues::read(&bytes)) {
                Ok(issues) => {
                    let count = issues.topics.len();
                    self.collab.issues = issues;
                    self.collab.issues_path = Some(path);
                    self.collab.issues_changed = false;
                    self.collab.topic = None;
                    self.collab.tab = Tab::Issues;
                    self.collab_note(Tone::Success, format!("{count} {}", l.text("issues geopend", "issues opened")));
                }
                Err(e) => self.collab_note(Tone::Error, e),
            },
            Event::Opened(File::Ids, Some(path)) => {
                if let Some(rules) = self.collab.rules_mut() {
                    if !rules.ids.contains(&path) {
                        rules.ids.push(path);
                    }
                }
                self.collab.tab = Tab::Checks;
            }
            Event::Opened(File::Rules, Some(path)) => match RuleSet::read(&path) {
                Ok(set) => {
                    self.collab.rule_sets.push(set);
                    self.collab.rule_set = self.collab.rule_sets.len() - 1;
                }
                Err(e) => self.collab_note(Tone::Error, e),
            },
            Event::SaveIssues => {
                let at = self.collab.issues_path.clone();
                let make = move || {
                    let dialog = rfd::AsyncFileDialog::new().add_filter("BCF", &["bcfzip"]).set_file_name("issues.bcfzip");
                    match at.as_ref().and_then(|p| Some((p.parent()?, p.file_name()?))) {
                        Some((folder, name)) => dialog.set_directory(folder).set_file_name(name.to_string_lossy()),
                        None => dialog,
                    }
                };
                return ask_file(make, true, |path| Message::Collab(Event::SaveIssuesTo(path)));
            }
            Event::Bcf3(on) => self.collab.bcf3 = on,
            Event::SaveIssuesTo(Some(path)) => match self.collab.issues.write_as(self.collab.bcf3).and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
                Ok(()) => {
                    self.collab_note(Tone::Success, format!("{} {}", l.text("Opgeslagen:", "Saved:"), path.display()));
                    self.collab.issues_path = Some(path);
                    self.collab.issues_changed = false;
                }
                Err(e) => self.collab_note(Tone::Error, e),
            },
            Event::SaveRules => {
                let name = self.collab.rules().map(|r| r.name.clone()).unwrap_or_default();
                return ask_file(move || rfd::AsyncFileDialog::new().add_filter("Rule set", &["json"]).set_file_name(format!("{name}.rules.json")), true, |path| Message::Collab(Event::SaveRulesTo(path)));
            }
            Event::SaveRulesTo(Some(path)) => {
                // the decisions on the clashes as they are now go with the file
                self.collab_remember();
                if let Some(Err(e)) = self.collab.rules().map(|r| r.write(&path)) {
                    self.collab_note(Tone::Error, e);
                } else {
                    self.collab_note(Tone::Success, format!("{} {}", l.text("Opgeslagen:", "Saved:"), path.display()));
                }
            }
            Event::SaveIssuesTo(None) | Event::SaveRulesTo(None) | Event::ExportTo(_, None) => {}
            Event::Topic(guid) => {
                self.collab.topic = guid.clone();
                self.collab.labels_text = guid.as_ref().and_then(|g| self.collab.issues.topics.iter().find(|t| &t.guid == g)).map(|t| t.labels.join(", ")).unwrap_or_default();
                self.collab.comment.clear();
                let view = guid.and_then(|g| self.collab.issues.topics.iter().find(|t| t.guid == g)).and_then(|t| t.viewpoints.first().cloned());
                if let Some(view) = view {
                    self.collab_show_viewpoint(&view);
                }
            }
            Event::Viewpoint(guid, k) => {
                let view = self.collab.issues.topics.iter().find(|t| t.guid == guid).and_then(|t| t.viewpoints.get(k).cloned());
                if let Some(view) = view {
                    self.collab_show_viewpoint(&view);
                }
            }
            Event::NewIssue => {
                let mut topic = Topic::new(l.text("Nieuw issue", "New issue"), &self.collab.author);
                topic.priority = "Normal".into();
                if self.model.is_some() {
                    topic.viewpoints.push(self.collab_viewpoint_now());
                    self.collab.shot_for = Some(topic.guid.clone());
                }
                self.collab.topic = Some(topic.guid.clone());
                self.collab.issues.topics.push(topic);
                self.collab.issues_changed = true;
                self.collab.tab = Tab::Issues;
                if self.collab.shot_for.is_some() {
                    return iced::window::latest().and_then(iced::window::screenshot).map(|shot| Message::Collab(Event::Shot(Arc::new(shot))));
                }
            }
            Event::Shot(shot) => {
                let Some(guid) = self.collab.shot_for.take() else { return Task::none() };
                // the viewport out of the window's picture
                let s = shot.scale_factor;
                let (w, h) = (shot.size.width, shot.size.height);
                let x = ((self.viewport_origin.0 * s).round().max(0.0) as u32).min(w.saturating_sub(1));
                let y = ((self.viewport_origin.1 * s).round().max(0.0) as u32).min(h.saturating_sub(1));
                let (cw, ch) = (((self.viewport_size.0 * s).round() as u32).clamp(1, w - x), ((self.viewport_size.1 * s).round() as u32).clamp(1, h - y));
                let mut rgba = Vec::with_capacity((cw * ch * 4) as usize);
                for row in y..y + ch {
                    let from = ((row * w + x) * 4) as usize;
                    rgba.extend_from_slice(shot.rgba.get(from..from + (cw * 4) as usize).unwrap_or(&[]));
                }
                rgba.chunks_exact_mut(4).for_each(|px| px[3] = 255);
                if let Ok(png) = crate::remote::encode_png(cw, ch, &rgba) {
                    if let Some(view) = self.collab.issues.topics.iter_mut().find(|t| t.guid == guid).and_then(|t| t.viewpoints.first_mut()) {
                        view.snapshot = Some(("snapshot.png".into(), png));
                    }
                }
            }
            Event::Title(title) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.title = title;
                    topic.touch(&author);
                }
            }
            Event::Description(description) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.description = description;
                    topic.touch(&author);
                }
            }
            Event::Status(status) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.status = status.to_string();
                    topic.touch(&author);
                }
            }
            Event::Priority(priority) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.priority = priority.to_string();
                    topic.touch(&author);
                }
            }
            Event::Assigned(to) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.assigned_to = to;
                    topic.touch(&author);
                }
            }
            Event::Comment(draft) => self.collab.comment = draft,
            Event::User(name) => {
                self.collab.author = name.clone();
                if let (Some(file), false) = (user_file(), name.trim().is_empty()) {
                    let _ = file.parent().map(std::fs::create_dir_all);
                    let _ = std::fs::write(file, name.trim());
                }
            }
            Event::Due(due) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.due_date = due;
                    topic.touch(&author);
                }
            }
            Event::Labels(labels) => {
                self.collab.labels_text = labels.clone();
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.labels = labels.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    topic.touch(&author);
                }
            }
            Event::KindOf(kind) => {
                let author = self.collab.author.clone();
                if let Some(topic) = self.collab_topic_mut() {
                    topic.kind = kind.to_string();
                    topic.touch(&author);
                }
            }
            Event::AssignTo(person) => return self.collab_handle(Event::Assigned(person.1)),
            Event::Decide(k, decision) => {
                if let Some(clash) = self.collab.clashes.get_mut(k) {
                    clash.decision = decision;
                }
            }
            Event::ShowAccepted(on) => self.collab.show_accepted = on,
            Event::SectionAround => return self.update(Message::SectionToSelection),
            Event::CompareOld(c) => self.collab.compare_pair.0 = c.0,
            Event::CompareNew(c) => self.collab.compare_pair.1 = c.0,
            Event::Compare => {
                let (old, new) = self.collab.compare_pair;
                let Some(model) = self.model.clone() else { return Task::none() };
                if self.models.len() < 2 || old == new || old >= self.models.len() || new >= self.models.len() {
                    self.collab_note(Tone::Warning, l.text("Open twee versies van een model (Toevoegen) en kies welke oud en welke nieuw is.", "Open two versions of a model (Add) and choose which is old and which is new."));
                    return Task::none();
                }
                let mut found = compare_models(&model, old as u32, new as u32);
                let name = |k: usize| self.models[k].model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (found.old, found.new) = (name(old), name(new));
                self.collab.comparison = Some(found);
                self.collab.tab = Tab::Compare;
            }
            Event::ComparePick(list, k) => {
                let id = self.collab.comparison.as_ref().and_then(|c| match list {
                    0 => c.added.get(k).map(|x| x.0),
                    1 => c.removed.get(k).map(|x| x.0),
                    _ => c.changed.get(k).map(|x| x.0),
                });
                if let Some(id) = id {
                    self.collab_go_to([id]);
                }
            }
            Event::ExportIssues => return self.collab_handle(Event::Export(Report::IssuesHtml)),
            Event::Isolate(on) => {
                self.collab.isolate = on;
                // what is selected alone, or everything again
                self.show_all_elements();
                if on {
                    self.isolate_selection();
                }
            }
            Event::Step(by) if self.collab.grouping != Grouping::Off && self.collab.clash.is_none() => {
                // through the groups: the one after (or before) the one open
                let groups: Vec<usize> = self.collab_groups().iter().map(|g| self.collab.clashes[g[0]].group).collect();
                if !groups.is_empty() {
                    let at = self.collab.open_group.and_then(|g| groups.iter().position(|x| *x == g));
                    let next = at.map_or(0, |k| (k as i32 + by).rem_euclid(groups.len() as i32) as usize);
                    self.collab.open_group = None;
                    return self.collab_handle(Event::PickGroup(groups[next]));
                }
            }
            Event::Step(by) => {
                let count = self.collab.clashes.len() as i32;
                if count > 0 {
                    let next = self.collab.clash.map_or(0, |k| (k as i32 + by).rem_euclid(count));
                    return self.collab_handle(Event::PickClash(next as usize));
                }
            }
            Event::Search(search) => self.collab.search = search,
            Event::ShowClosed(on) => self.collab.show_closed = on,
            Event::OnlyFailed(on) => self.collab.only_failed = on,
            Event::AddComment => {
                let (author, draft) = (self.collab.author.clone(), std::mem::take(&mut self.collab.comment));
                if draft.trim().is_empty() {
                    return Task::none();
                }
                if let Some(topic) = self.collab_topic_mut() {
                    let at = topic.viewpoints.first().map(|v| v.guid.clone());
                    topic.comment(draft.trim(), &author, at);
                }
            }
            Event::RemoveTopic => {
                if let Some(guid) = self.collab.topic.take() {
                    self.collab.share_removed.push(guid.clone());
                    self.collab.issues.topics.retain(|t| t.guid != guid);
                    self.collab.issues_changed = true;
                }
            }
            Event::RuleSet(k) => {
                if k < self.collab.rule_sets.len() {
                    self.collab.rule_set = k;
                }
            }
            Event::ModelChecks(on) => {
                if let Some(rules) = self.collab.rules_mut() {
                    rules.model_checks = on;
                }
            }
            Event::Ils(on) => {
                if let Some(rules) = self.collab.rules_mut() {
                    rules.ils = on;
                }
            }
            Event::Rooms(on) => {
                if let Some(rules) = self.collab.rules_mut() {
                    rules.rooms.on = on;
                }
            }
            Event::RoomAmount(k, typed) => {
                if let (Some(rules), Ok(v)) = (self.collab.rules_mut(), typed.trim().replace(',', ".").parse::<f32>()) {
                    if v.is_finite() && (0.0..=1000.0).contains(&v) {
                        let r = &mut rules.rooms;
                        *[&mut r.room_height, &mut r.door_width, &mut r.door_height, &mut r.escape_distance, &mut r.threshold, &mut r.ramp_run, &mut r.turning_circle][k.min(6)] = v;
                    }
                }
            }
            Event::RemoveIds(k) => {
                if let Some(rules) = self.collab.rules_mut().filter(|r| k < r.ids.len()) {
                    rules.ids.remove(k);
                }
            }
            Event::Check => {
                let Some(rules) = self.collab.rules().cloned() else { return Task::none() };
                let files: Vec<(usize, String, PathBuf)> = self
                    .models
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.model.path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc")))
                    .map(|(k, e)| (k, e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.model.path.clone()))
                    .collect();
                self.collab.tab = Tab::Checks;
                if files.is_empty() {
                    self.collab_note(Tone::Warning, l.text("Open een IFC-model om het te controleren: de controles lezen het IFC-bestand zelf.", "Open an IFC model to check it: the checks read the IFC file itself."));
                    return Task::none();
                }
                if rules.ids.is_empty() && !rules.model_checks && !rules.ils {
                    self.collab_note(Tone::Warning, l.text("De regelset heeft geen controles: laad een IDS of zet de modelcontroles aan.", "The rule set has no checks: load an IDS or switch the model checks on."));
                    return Task::none();
                }
                self.collab.cancel = Arc::new(AtomicBool::new(false));
                self.collab.run += 1;
                self.collab.running = true;
                self.collab.checked.clear();
                self.collab.open_spec = None;
                self.collab.notice = None;
                let (run, cancel) = (self.collab.run, self.collab.cancel.clone());
                return job(move || {
                    Event::Checked(run, check_files(files, &rules, &cancel))
                });
            }
            Event::Checked(run, mut checked) => {
                if run == self.collab.run {
                    self.collab.running = false;
                    // the checks that need the rooms' shapes, on the model as it is drawn
                    if let Some(rooms) = self.collab.rules().map(|r| r.rooms.clone()).filter(|r| r.on) {
                        self.collab.routes = self.room_shape_checks(&mut checked, &rooms);
                    }
                    self.collab.route_shown = None;
                    self.collab.checked = checked;
                }
            }
            Event::Spec(open) => self.collab.open_spec = open,
            Event::Finding(checked, express_id) => {
                let Some(model) = self.model.clone() else { return Task::none() };
                let which = self.collab.checked.get(checked).map_or(0, |c| c.model) as u32;
                match model.element_with_key(crate::model::ElementKey { model: which, source: u64::from(express_id) }) {
                    Some(id) if !model.element_ranges(id).is_empty() => {
                        // a room's walk to its way out is drawn with it
                        self.collab.route_shown = self.collab.routes.contains_key(&id).then_some(id);
                        self.collab_go_to([id]);
                        self.refresh_overlay();
                    }
                    _ => self.collab_note(Tone::Info, l.text("Dit element heeft geen geometrie in het model.", "This element has no geometry in the model.")),
                }
            }
            Event::IssuesFromChecks => {
                let author = self.collab.author.clone();
                let mut made = 0;
                for c in &self.collab.checked {
                    let Ok(report) = &c.report else { continue };
                    for spec in report.specifications.iter().filter(|s| s.status == ValidationStatus::Failed) {
                        let mut topic = Topic::new(&format!("{}: {}", c.name, spec.name), &author);
                        topic.kind = if spec.severity == Severity::Warning { "Remark".into() } else { "Issue".into() };
                        topic.priority = if spec.severity == Severity::Warning { "Low".into() } else { "Normal".into() };
                        topic.labels = spec.identifier.iter().cloned().collect();
                        let failed: Vec<&bim_validator_core::Finding> = spec.findings.iter().filter(|f| f.status == ValidationStatus::Failed).collect();
                        let mut reasons: Vec<&str> = failed.iter().map(|f| f.reason.as_str()).collect();
                        reasons.dedup();
                        topic.description = format!("{} {}\n{}", failed.len(), l.text("elementen voldoen niet.", "elements fail."), reasons.into_iter().take(5).collect::<Vec<_>>().join("\n"));
                        let mut view = Viewpoint::new();
                        view.selection = failed.iter().filter_map(|f| f.global_id.clone()).take(2000).collect();
                        topic.viewpoints.push(view);
                        self.collab.issues.topics.push(topic);
                        made += 1;
                    }
                }
                self.collab.issues_changed |= made > 0;
                self.collab.tab = Tab::Issues;
                self.collab_note(Tone::Success, format!("{made} {}", l.text("issues gemaakt", "issues made")));
            }
            Event::Export(kind) => {
                let (name, extension) = match kind {
                    Report::Html => ("check", "html"),
                    Report::Csv => ("check", "csv"),
                    Report::Json => ("check", "json"),
                    Report::ClashesCsv => ("clashes", "csv"),
                    Report::IssuesHtml => ("issues", "html"),
                    Report::Pdf => ("report", "pdf"),
                    Report::IssuesPdf => ("issues", "pdf"),
                    Report::Xlsx => ("report", "xlsx"),
                };
                return ask_file(move || rfd::AsyncFileDialog::new().add_filter(extension, &[extension]).set_file_name(format!("{name}.{extension}")), true, move |path| Message::Collab(Event::ExportTo(kind, path)));
            }
            Event::ExportTo(kind, Some(path)) => {
                let body: Vec<u8> = match kind {
                    // the pictures are taken first, one frame each; the file is written then
                    Report::Pdf => return self.report_begin(path, super::reports::Scope::All),
                    Report::IssuesPdf => return self.report_begin(path, super::reports::Scope::Issues),
                    Report::Xlsx => match crate::export::xlsx::workbook(&self.report_sheets(l).iter().collect::<Vec<_>>()) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            self.collab_note(Tone::Error, e.to_string());
                            return Task::none();
                        }
                    },
                    _ => self.collab_text_report(kind, l).into_bytes(),
                };
                match std::fs::write(&path, body) {
                    Ok(()) => self.collab_note(Tone::Success, format!("{} {}", l.text("Opgeslagen:", "Saved:"), path.display())),
                    Err(e) => self.collab_note(Tone::Error, e.to_string()),
                }
            }
            Event::ClashOn(k, on) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.on = on;
                }
            }
            Event::ClashStoreys(k, typed) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.storeys = typed;
                }
            }
            Event::ClashProperty(k, typed) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.property = typed.trim().to_string();
                }
            }
            Event::ClashValue(k, typed) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.value = typed;
                }
            }
            Event::ClashSightNames(k, typed) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.sight_names = typed;
                }
            }
            Event::ClashSightShare(k, typed) => {
                if let (Some(rule), Ok(pct)) = (self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)), typed.trim().trim_end_matches('%').replace(',', ".").parse::<f32>()) {
                    if (0.0..=100.0).contains(&pct) {
                        rule.sight_blocked_pct = pct;
                    }
                }
            }
            Event::ClashPairs(k, typed) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.pair_tolerances = typed;
                }
            }
            Event::ClashBetween(k, on) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.between_models = on;
                }
            }
            Event::ClashKindOf(k, kind) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.kind = kind;
                }
            }
            Event::ClashAmount(k, amount) => {
                if let (Some(rule), Ok(mm)) = (self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)), amount.trim().replace(',', ".").parse::<f32>()) {
                    if mm.is_finite() && (0.0..=10_000.0).contains(&mm) {
                        match rule.kind {
                            ClashKind::Hard => rule.tolerance_mm = mm,
                            ClashKind::Clearance => rule.clearance_mm = mm,
                            ClashKind::Sight => rule.sight_m = mm,
                        }
                    }
                }
            }
            Event::ClashSide(k, first, side) => {
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    if first {
                        rule.a = side.selector;
                    } else {
                        rule.b = side.selector;
                    }
                }
                // the test is named after what it takes
                let everything = l.text("alles", "everything");
                if let Some(rule) = self.collab.rules_mut().and_then(|r| r.clashes.get_mut(k)) {
                    rule.name = if rule.within() && rule.a.is_all() {
                        l.text("Alle elementen onderling", "All elements with each other").to_string()
                    } else {
                        format!("{} × {}", rule.a.summary(everything), rule.b.summary(everything))
                    };
                }
            }
            Event::ClashSelection => {
                let Some(model) = self.model.clone() else { return Task::none() };
                let ids: Vec<u64> = self.sel.items().iter().filter_map(|item| if let Pickable::Element(id) = item { Some(*id) } else { None }).collect();
                if ids.is_empty() {
                    self.collab_note(Tone::Warning, l.text("Selecteer eerst de elementen die je wilt clashen.", "Select the elements to clash first."));
                    return Task::none();
                }
                // as the first test of the rule set asks, against everything else
                let like = self.collab.rules().and_then(|r| r.clashes.first().cloned()).unwrap_or_default();
                let rule = ClashRule {
                    name: format!("{} ({})", l.text("Selectie tegen de rest", "Selection against the rest"), ids.len()),
                    a: Selector { only: ids.clone(), ..Selector::default() },
                    b: Selector { except: ids, ..Selector::default() },
                    on: true,
                    ..like
                };
                let models: Vec<(String, std::ops::Range<usize>)> = self.models.iter().map(|e| (e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.layers.clone())).collect();
                self.collab.tab = Tab::Clashes;
                self.collab_remember();
                self.collab.partial_run = true;
                self.collab.cancel = Arc::new(AtomicBool::new(false));
                self.collab.run += 1;
                self.collab.running = true;
                self.collab.clashes.clear();
                self.collab.clash = None;
                self.collab.notice = None;
                let (run, cancel) = (self.collab.run, self.collab.cancel.clone());
                return job(move || Event::Clashed(run, std::panic::catch_unwind(|| run_clashes(&model, &models, &[rule], &cancel)).unwrap_or_default()));
            }
            Event::AddClashRule => {
                let name = l.text("Alle elementen onderling", "All elements with each other").to_string();
                if let Some(rules) = self.collab.rules_mut() {
                    rules.clashes.push(ClashRule { name, ..ClashRule::default() });
                }
            }
            Event::RemoveClashRule(k) => {
                if let Some(rules) = self.collab.rules_mut().filter(|r| k < r.clashes.len()) {
                    rules.clashes.remove(k);
                }
            }
            Event::Clash => {
                let (Some(model), Some(rules)) = (self.model.clone(), self.collab.rules().cloned()) else {
                    self.collab_note(Tone::Warning, l.text("Open eerst een model.", "Open a model first."));
                    return Task::none();
                };
                self.collab.tab = Tab::Clashes;
                let tests = rules.clash_rules();
                if tests.is_empty() {
                    self.collab_note(Tone::Warning, l.text("De regelset heeft geen clashtest die aan staat.", "The rule set has no clash test switched on."));
                    return Task::none();
                }
                let models: Vec<(String, std::ops::Range<usize>)> = self.models.iter().map(|e| (e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.layers.clone())).collect();
                self.collab_remember();
                self.collab.partial_run = false;
                self.collab.cancel = Arc::new(AtomicBool::new(false));
                self.collab.run += 1;
                self.collab.running = true;
                self.collab.clashes.clear();
                self.collab.clash = None;
                self.collab.notice = None;
                let (run, cancel) = (self.collab.run, self.collab.cancel.clone());
                return job(move || Event::Clashed(run, std::panic::catch_unwind(|| run_clashes(&model, &models, &tests, &cancel)).unwrap_or_default()));
            }
            Event::Clashed(run, mut rows) => {
                if run == self.collab.run {
                    self.collab.running = false;
                    if rows.len() >= MOST_CLASHES {
                        self.collab_note(Tone::Warning, format!("{} {MOST_CLASHES} {}", l.text("Alleen de eerste", "Only the first"), l.text("clashes zijn bewaard: maak de test strenger.", "clashes are kept: make the test narrower.")));
                    }
                    self.collab_take_clashes(&mut rows);
                    group_clashes(&mut rows, self.collab.grouping);
                    self.collab.show_tests = rows.is_empty();
                    self.collab.clashes = rows;
                    self.collab.open_group = None;
                    self.refresh_overlay();
                }
            }
            Event::PickClash(k) => {
                let Some(row) = self.collab.clashes.get(k).cloned() else { return Task::none() };
                self.collab.clash = Some(k);
                self.collab.open_group = Some(row.group);
                self.collab_go_to([row.a, row.b]);
                self.refresh_overlay();
            }
            Event::IssueFromClash(k) => {
                let Some(row) = self.collab.clashes.get(k).cloned() else { return Task::none() };
                if let Some(clash) = self.collab.clashes.get_mut(k) {
                    clash.decision = Decision::Rejected;
                }
                let topic = self.collab_clash_topic(&row, l);
                self.collab.topic = Some(topic.guid.clone());
                self.collab.issues.topics.push(topic);
                self.collab.issues_changed = true;
                self.collab.tab = Tab::Issues;
            }
            Event::IssuesFromClashes => {
                // one issue for each group: the accepted clashes are left out
                let mut topics: Vec<Topic> = Vec::new();
                for group in self.collab_groups().into_iter().take(1000) {
                    let rows: Vec<&ClashRow> = group.iter().map(|&k| &self.collab.clashes[k]).filter(|c| c.decision != Decision::Accepted).collect();
                    match rows.as_slice() {
                        [] => {}
                        [one] => topics.push(self.collab_clash_topic(one, l)),
                        more => topics.push(self.collab_group_topic(more, l)),
                    }
                }
                let made = topics.len();
                self.collab.issues.topics.extend(topics);
                self.collab.issues_changed |= made > 0;
                self.collab.tab = Tab::Issues;
                self.collab_note(Tone::Success, format!("{made} {}", l.text("issues gemaakt", "issues made")));
            }
            Event::Inform(e) => return self.inform_handle(e),
            Event::Cloud(e) => return self.cloud_handle(e),
            Event::ReportShot(shot) => {
                let picture = self.viewport_picture(&shot);
                if let Some(job) = self.collab.report_job.as_mut() {
                    if !job.todo.is_empty() {
                        match (job.todo.remove(0), picture) {
                            (Shot::Cover, picture) => job.cover = picture,
                            (Shot::Group(g), Some(p)) => {
                                job.pictures.insert(g, p);
                            }
                            (Shot::Issue(k), Some(p)) => {
                                job.issue_pictures.insert(k, p);
                            }
                            _ => {}
                        }
                    }
                }
                return self.report_next();
            }
            Event::ShowTests(on) => self.collab.show_tests = on,
            Event::ShowResolved(on) => self.collab.show_resolved = on,
            Event::ShareFolder => {
                self.collab.dialog_open = true;
                return Task::perform(async { rfd::AsyncFileDialog::new().pick_folder().await.map(|f| f.path().to_path_buf()) }, |folder| Message::Collab(Event::ShareFolderIs(folder)));
            }
            Event::ShareFolderIs(folder) => {
                self.collab.dialog_open = false;
                if let Some(folder) = folder {
                    if let Some(file) = share_file() {
                        let _ = std::fs::write(file, folder.display().to_string());
                    }
                    self.collab.share_folder = Some(folder);
                    self.collab.share_known.clear();
                    self.collab_sync(true);
                }
            }
            Event::Unshare => {
                self.collab.share_folder = None;
                if let Some(file) = share_file() {
                    let _ = std::fs::remove_file(file);
                }
            }
            Event::Sync => self.collab_sync(true),
            Event::DecidePicked(decision) => match (self.collab.clash, self.collab.open_group) {
                (Some(k), _) => {
                    let now = self.collab.clashes.get(k).map(|c| c.decision);
                    return self.collab_handle(Event::Decide(k, if now == Some(decision) { Decision::Open } else { decision }));
                }
                (None, Some(g)) => {
                    let all = self.collab.clashes.iter().filter(|c| c.group == g).all(|c| c.decision == decision);
                    return self.collab_handle(Event::DecideGroup(g, if all { Decision::Open } else { decision }));
                }
                _ => {}
            },
            Event::IssueFromPicked => match (self.collab.clash, self.collab.open_group) {
                (Some(k), _) => return self.collab_handle(Event::IssueFromClash(k)),
                (None, Some(g)) => return self.collab_handle(Event::IssueFromGroup(g)),
                _ => {}
            },
            Event::GroupBy(choice) => {
                self.collab.grouping = choice.0;
                group_clashes(&mut self.collab.clashes, choice.0);
                self.collab.clash = None;
                self.collab.open_group = None;
                self.refresh_overlay();
            }
            Event::PickGroup(g) => {
                let ids: Vec<u64> = self.collab.clashes.iter().filter(|c| c.group == g).flat_map(|c| [c.a, c.b]).collect();
                self.collab.open_group = if self.collab.open_group == Some(g) { None } else { Some(g) };
                self.collab.clash = None;
                if !ids.is_empty() {
                    self.collab_go_to(ids);
                }
                self.refresh_overlay();
            }
            Event::DecideGroup(g, decision) => {
                for c in self.collab.clashes.iter_mut().filter(|c| c.group == g) {
                    c.decision = decision;
                }
            }
            Event::IssueFromGroup(g) => {
                let rows: Vec<ClashRow> = self.collab.clashes.iter().filter(|c| c.group == g).cloned().collect();
                if rows.is_empty() {
                    return Task::none();
                }
                let topic = self.collab_group_topic(&rows.iter().collect::<Vec<_>>(), l);
                for c in self.collab.clashes.iter_mut().filter(|c| c.group == g) {
                    c.decision = Decision::Rejected;
                }
                self.collab.topic = Some(topic.guid.clone());
                self.collab.issues.topics.push(topic);
                self.collab.issues_changed = true;
                self.collab.tab = Tab::Issues;
            }
            Event::MatrixCell(i, j) => {
                if let Some(rules) = self.collab.rules_mut() {
                    if let (Some(a), Some(b)) = (rules.disciplines.get(i).map(|d| d.name.clone()), rules.disciplines.get(j).map(|d| d.name.clone())) {
                        rules.toggle_matrix(&a, &b);
                    }
                }
            }
            Event::AddDiscipline => {
                if let Some(rules) = self.collab.rules_mut() {
                    // a name not taken yet
                    let base = l.text("Discipline", "Discipline");
                    let name = (1..).map(|n| format!("{base} {n}")).find(|n| !rules.disciplines.iter().any(|d| &d.name == n)).unwrap_or_default();
                    rules.disciplines.push(crate::collab::rules::Discipline { name, take: Selector::default() });
                }
            }
            Event::RemoveDiscipline(k) => {
                if let Some(rules) = self.collab.rules_mut().filter(|r| k < r.disciplines.len()) {
                    let gone = rules.disciplines.remove(k).name;
                    rules.matrix.retain(|pair| !pair.contains(&gone));
                }
            }
            Event::DisciplineName(k, name) => {
                if let Some(rules) = self.collab.rules_mut() {
                    // a name another discipline has would join their pairs
                    let taken = rules.disciplines.iter().enumerate().any(|(i, d)| i != k && d.name == name);
                    if let (false, Some(old)) = (taken, rules.disciplines.get(k).map(|d| d.name.clone())) {
                        for pair in &mut rules.matrix {
                            for side in pair.iter_mut().filter(|s| **s == old) {
                                *side = name.clone();
                            }
                        }
                        rules.disciplines[k].name = name;
                    }
                }
            }
            Event::DisciplineSide(k, side) => {
                if let Some(d) = self.collab.rules_mut().and_then(|r| r.disciplines.get_mut(k)) {
                    d.take = side.selector;
                }
            }
            Event::MatrixAmount(amount) => {
                if let (Some(rules), Ok(mm)) = (self.collab.rules_mut(), amount.trim().replace(',', ".").parse::<f32>()) {
                    if mm.is_finite() && (0.0..=10_000.0).contains(&mm) {
                        rules.matrix_tolerance_mm = mm;
                    }
                }
            }
            Event::MatrixBetween(on) => {
                if let Some(rules) = self.collab.rules_mut() {
                    rules.matrix_between_models = on;
                }
            }
        }
        Task::none()
    }

    /// Opens a BCF file, adds an IDS and runs a check or the clash tests, all at once and
    /// before returning: for a picture of the window without one (`--screenshot`).
    pub fn collab_now(&mut self, bcf: Option<&std::path::Path>, ids: Option<&std::path::Path>, run: Option<&str>) -> Result<(), String> {
        let _ = self.collab_update(Event::Tab(Tab::Issues));
        if let Some(path) = bcf {
            let _ = self.collab_update(Event::Opened(File::Bcf, Some(path.to_path_buf())));
            if let Some((Tone::Error, e)) = &self.collab.notice {
                return Err(e.clone());
            }
            let first = self.collab.issues.topics.first().map(|t| t.guid.clone());
            let _ = self.collab_update(Event::Topic(first));
            self.finish_animation();
        }
        if let Some(path) = ids {
            let _ = self.collab_update(Event::Opened(File::Ids, Some(path.to_path_buf())));
        }
        match run {
            Some("check") => {
                let rules = self.collab.rules().cloned().ok_or("no rule set")?;
                let cancel = AtomicBool::new(false);
                let files = self.models.iter().enumerate().filter(|(_, e)| e.model.path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ifc")));
                let mut checked: Vec<Checked> = check_files(files.map(|(model, e)| (model, e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.model.path.clone())).collect(), &rules, &cancel);
                if rules.rooms.on {
                    self.collab.routes = self.room_shape_checks(&mut checked, &rules.rooms);
                }
                self.collab.checked = checked;
                let failed = self.collab.checked.first().and_then(|c| c.report.as_ref().ok()).and_then(|r| r.specifications.iter().position(|s| s.status == ValidationStatus::Failed));
                self.collab.open_spec = failed.map(|s| (0, s));
                self.collab.tab = Tab::Checks;
            }
            Some(what @ ("clash" | "clash-isolate")) => {
                self.collab.isolate = what == "clash-isolate";
                let (model, rules) = (self.model.clone().ok_or("no model")?, self.collab.rules().cloned().ok_or("no rule set")?);
                let models: Vec<(String, std::ops::Range<usize>)> = self.models.iter().map(|e| (e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.layers.clone())).collect();
                let started = Instant::now();
                self.collab_remember();
                self.collab.partial_run = false;
                let mut rows = run_clashes(&model, &models, &rules.clash_rules(), &AtomicBool::new(false));
                self.collab_take_clashes(&mut rows);
                self.collab.clashes = rows;
                group_clashes(&mut self.collab.clashes, self.collab.grouping);
                self.collab.show_tests = self.collab.clashes.is_empty();
                eprintln!("{} clashes in {:.2} s", self.collab.clashes.len(), started.elapsed().as_secs_f32());
                self.collab.tab = Tab::Clashes;
                if !self.collab.clashes.is_empty() {
                    let _ = self.collab_update(Event::PickClash(0));
                    self.finish_animation();
                }
            }
            Some("issues") | None => {}
            Some(other) => return Err(format!("--collab issues|check|clash|clash-isolate, not {other}")),
        }
        Ok(())
    }

    /// The Collaborator's panel: its tabs, and under them the issues, the checks or the clashes.
    pub fn collab_body(&self, look: Look, l: Language) -> Element<'_, Message> {
        let p = self.theme.palette();
        let state = &self.collab;
        let tab = |which: Tab, label: String| {
            let active = state.tab == which;
            named(button(text(label.clone()).size(12).font(if active { crate::app::FONT_BOLD } else { crate::app::FONT })).padding([4, 10]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::Tab(which))), Node::new(Role::Tab, label.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ')).selected(active))
        };
        let count = |n: usize| if n == 0 { String::new() } else { format!(" {n}") };
        let failed: usize = state.checked.iter().filter_map(|c| c.report.as_ref().ok()).flat_map(|r| &r.specifications).filter(|s| s.status == ValidationStatus::Failed).count();
        let tabs = row![
            tab(Tab::Issues, format!("{}{}", l.text("Issues", "Issues"), count(state.open_issues()))),
            tab(Tab::Checks, format!("{}{}", l.text("Controles", "Checks"), count(failed))),
            tab(Tab::Clashes, format!("{}{}", l.text("Clashes", "Clashes"), count(state.clashes.iter().filter(|c| c.decision != Decision::Accepted).count()))),
            tab(Tab::Compare, l.text("Vergelijken", "Compare").to_string()),
            tab(Tab::Colours, l.text("Kleuren", "Colours").to_string()),
            tab(Tab::Takeoff, l.text("Staat", "Takeoff").to_string()),
        ]
        .spacing(2)
        .wrap();
        // the models of the project, as the viewer's explorer has them: shown or not, and more to add
        let mut models = column![row![
            w::heading(look, format!("{} · {}", l.text("Modellen", "Models"), self.models.len()), 13.0).width(Length::Fill),
            w::button(look, l.text("Model toevoegen", "Add model"), K::Secondary, D::Compact, None, Some(if self.models.is_empty() { Message::OpenDialog } else { Message::AddDialog })),
        ]
        .spacing(6)
        .align_y(Alignment::Center)]
        .spacing(2);
        let mut rows = column![].spacing(2);
        for (i, entry) in self.models.iter().enumerate() {
            let line = row![
                w::checkbox(look, entry.model.path.file_name().and_then(|n| n.to_str()).unwrap_or("3D"), entry.visible, Some(move |on| Message::ModelVisible(i, on))),
                Space::new().width(Length::Fill),
                w::secondary(look, entry.model.format.clone()),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            rows = rows.push(line);
        }
        // many models: their list scrolls in a few rows, so the tabs under it stay in view
        models = if self.models.len() > 6 { models.push(scrollable(container(rows).padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(128)) } else { models.push(rows) };
        let mut body = column![
            models,
            w::divider(look),
            tabs,
            w::divider(look),
            row![w::secondary(look, l.text("Jij", "You")), container(w::text_field(look, l.text("Naam of e-mail", "Name or e-mail"), &state.author, Some(|t| go(Event::User(t))), D::Compact, None)).width(Length::Fill)].spacing(6).align_y(Alignment::Center),
        ]
        .spacing(6);
        if let Some((tone, message)) = &state.notice {
            // a notice of its own: what it says can take several lines in a narrow panel
            let (edge, fill) = (tone.color(look), tone.tint(look));
            let close = named(button(text("×").size(14)).padding([0, 6]).style(move |_, s| p.ribbon_button(false, s)).on_press(go(Event::Dismiss)), Node::new(Role::Button, l.text("Melding sluiten", "Close the notice")));
            body = body.push(
                container(row![w::body(look, message.clone()).width(Length::Fill), close].spacing(6).align_y(Alignment::Start))
                    .padding([6, 8])
                    .width(Length::Fill)
                    .style(move |_| container::Style { background: Some(iced::Background::Color(fill)), border: iced::Border { color: edge, width: 1.0, radius: 4.0.into() }, ..Default::default() }),
            );
        }
        body = body.push(w::switch(look, l.text("Isoleren: alleen tonen wat je aanklikt", "Isolate: show only what you click"), state.isolate, Some(|on| go(Event::Isolate(on)))));
        if state.running {
            body = body.push(
                row![w::secondary(look, l.text("Bezig…", "Working…")).width(Length::Fill), w::button(look, l.text("Stoppen", "Stop"), K::Quiet, D::Compact, None, Some(go(Event::Cancel)))].align_y(Alignment::Center),
            );
        }
        let content: Element<'_, Message> = match state.tab {
            Tab::Issues => self.collab_issues(look, l),
            Tab::Checks => self.collab_checks(look, l),
            Tab::Clashes => self.collab_clashes(look, l),
            Tab::Compare => column![self.collab_compare(look, l), w::divider(look), self.cloud_view(look, l)].spacing(8).into(),
            Tab::Colours => self.inform_colours(look, l),
            Tab::Takeoff => self.inform_takeoffs(look, l, &state.sides),
        };
        body.push(scrollable(container(content).padding(iced::Padding { right: 10.0, ..iced::Padding::ZERO })).height(Length::Fill)).height(Length::Fill).into()
    }

    /// The rule set in use, to choose and to keep.
    fn collab_rule_set<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let description = state.rules().map(|r| r.description.clone()).unwrap_or_default();
        column![
            w::mono_label(look, l.text("Regelset", "Rule set")),
            row![
                container(w::select(look, &state.rule_names, state.rule_names.get(state.rule_set).cloned(), |c: Choice| go(Event::RuleSet(c.0)), D::Compact)).width(Length::Fill),
                w::button(look, l.text("Openen", "Open"), K::Quiet, D::Compact, None, Some(go(Event::Open(File::Rules)))),
                w::button(look, l.text("Opslaan", "Save"), K::Quiet, D::Compact, None, Some(go(Event::SaveRules))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            w::secondary(look, description),
        ]
        .spacing(4)
        .into()
    }

    fn collab_issues<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let p = self.theme.palette();
        let mut list = column![row![
            w::button(look, l.text("Nieuw issue", "New issue"), K::Primary, D::Compact, None, Some(go(Event::NewIssue))),
            w::button(look, l.text("BCF openen", "Open BCF"), K::Secondary, D::Compact, None, Some(go(Event::Open(File::Bcf)))),
            w::button(look, l.text("BCF opslaan", "Save BCF"), K::Secondary, D::Compact, None, (!state.issues.topics.is_empty()).then(|| go(Event::SaveIssues))),
            w::checkbox(look, "BCF 3.0", state.bcf3, Some(|on| go(Event::Bcf3(on)))),
            w::button(look, l.text("Rapport", "Report"), K::Quiet, D::Compact, None, (!state.issues.topics.is_empty()).then(|| go(Event::ExportIssues))),
            w::button(look, "PDF", K::Quiet, D::Compact, None, (!state.issues.topics.is_empty()).then(|| go(Event::Export(Report::IssuesPdf)))),
        ]
        .spacing(6)
        .wrap()]
        .spacing(6);
        // the project shared through a folder
        list = list.push(match &state.share_folder {
            Some(folder) => row![
                w::secondary(look, format!("{}: {}", l.text("Gedeeld via", "Shared through"), folder.display())).width(Length::Fill),
                w::button(look, l.text("Synchroniseren", "Sync"), K::Secondary, D::Compact, None, Some(go(Event::Sync))),
                w::button(look, l.text("Stoppen", "Stop"), K::Quiet, D::Compact, None, Some(go(Event::Unshare))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            None => row![
                w::secondary(look, l.text("Deel issues en clashbesluiten via een gedeelde map.", "Share issues and clash decisions through a shared folder.")).width(Length::Fill),
                w::button(look, l.text("Map kiezen", "Choose folder"), K::Secondary, D::Compact, None, Some(go(Event::ShareFolder))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        });
        if state.issues.topics.is_empty() {
            return list
                .push(w::secondary(
                    look,
                    l.text(
                        "Nog geen issues. Maak er een van de weergave zoals die nu is, open een BCF-bestand, of maak issues van een controle of clashtest.",
                        "No issues yet. Make one of the view as it is, open a BCF file, or make issues of a check or a clash test.",
                    ),
                ))
                .into();
        }
        let needle = state.search.trim().to_lowercase();
        let fits = |t: &Topic| {
            let said = [&t.title, &t.description, &t.assigned_to, &t.kind].iter().any(|s| s.to_lowercase().contains(&needle)) || t.labels.iter().any(|s| s.to_lowercase().contains(&needle));
            (needle.is_empty() || said) && (state.show_closed || !closed(&t.status))
        };
        let shown: Vec<&Topic> = state.issues.topics.iter().filter(|t| fits(t)).collect();
        list = list.push(w::text_field(look, l.text("Zoeken in issues", "Search issues"), &state.search, Some(|t| go(Event::Search(t))), D::Compact, None));
        let name = state.issues_path.as_ref().and_then(|path| path.file_name()).map(|n| n.to_string_lossy().into_owned());
        let mut facts = vec![format!("{} / {}", shown.len(), state.issues.topics.len())];
        facts.extend(name);
        if state.issues_changed {
            facts.push(l.text("gewijzigd", "changed").to_string());
        }
        list = list.push(
            row![w::secondary(look, facts.join(" · ")).width(Length::Fill), w::checkbox(look, l.text("Gesloten", "Closed"), state.show_closed, Some(|on| go(Event::ShowClosed(on))))].spacing(6).align_y(Alignment::Center),
        );
        for topic in shown.into_iter().take(SHOWN) {
            let picked = state.topic.as_deref() == Some(topic.guid.as_str());
            let mut about = vec![kind_label(&topic.kind, l)];
            if !topic.priority.is_empty() {
                about.push(priority_label(&topic.priority, l));
            }
            if !topic.assigned_to.is_empty() {
                about.push(topic.assigned_to.clone());
            }
            if !topic.comments.is_empty() {
                about.push(format!("{} {}", topic.comments.len(), l.text("opmerkingen", "comments")));
            }
            let words = column![
                w::body(look, topic.title.clone()),
                row![w::secondary(look, about.join(" · ")).width(Length::Fill), if topic.status.trim().is_empty() { Element::from(Space::new()) } else { site::chip(look, &status_label(&topic.status, l), status_tone(&topic.status)) }].spacing(6).align_y(Alignment::Center),
            ]
            .spacing(2)
            .width(Length::Fill);
            let mut line = row![].spacing(8).align_y(Alignment::Center);
            // the picture of its first viewpoint, small
            let small = topic.viewpoints.first().filter(|_| !picked).and_then(|v| state.pictures.get(&(v.guid.clone(), v.snapshot.as_ref()?.1.len())));
            if let Some((small, _)) = small {
                line = line.push(iced::widget::image(small.clone()).width(56).height(42).content_fit(iced::ContentFit::Cover));
            }
            list = list.push(named(button(line.push(words)).width(Length::Fill).padding([5, 6]).style(move |_, s| p.ribbon_button(picked, s)).on_press(go(Event::Topic((!picked).then(|| topic.guid.clone())))), Node::new(Role::Row, format!("{}: {}", l.text("Issue", "Issue"), topic.title)).selected(picked).value(topic.status.clone())));
            if picked {
                list = list.push(self.collab_topic(look, l, topic));
            }
        }
        list.into()
    }

    /// An issue in full: what it says, how it stands, its viewpoints and its comments.
    fn collab_topic<'a>(&'a self, look: Look, l: Language, topic: &'a Topic) -> Element<'a, Message> {
        let state = &self.collab;
        let p = self.theme.palette();
        let status = state.statuses.iter().find(|s| topic.status.eq_ignore_ascii_case(s.0)).cloned();
        let priority = state.priorities.iter().find(|s| topic.priority.eq_ignore_ascii_case(s.0)).cloned();
        let mut detail = column![
            w::text_field(look, l.text("Titel", "Title"), &topic.title, Some(|t| go(Event::Title(t))), D::Compact, None),
            w::text_field(look, l.text("Omschrijving", "Description"), &topic.description, Some(|t| go(Event::Description(t))), D::Compact, None),
            row![
                column![w::mono_label(look, l.text("Status", "Status")), w::select(look, &state.statuses, status, |s: Label| go(Event::Status(s.0)), D::Compact)].spacing(2).width(Length::FillPortion(1)),
                column![w::mono_label(look, l.text("Prioriteit", "Priority")), w::select(look, &state.priorities, priority, |s: Label| go(Event::Priority(s.0)), D::Compact)].spacing(2).width(Length::FillPortion(1)),
            ]
            .spacing(8),
            row![
                container(w::text_field(look, l.text("Toegewezen aan", "Assigned to"), &topic.assigned_to, Some(|t| go(Event::Assigned(t))), D::Compact, None)).width(Length::FillPortion(3)),
                container(w::select(look, &state.team, None::<Choice>, |c: Choice| go(Event::AssignTo(c)), D::Compact)).width(Length::FillPortion(2)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
            row![
                container(w::text_field(look, l.text("Uiterlijk (JJJJ-MM-DD)", "Due (YYYY-MM-DD)"), &topic.due_date, Some(|t| go(Event::Due(t))), D::Compact, None)).width(Length::FillPortion(1)),
                container(w::text_field(look, l.text("Labels, met komma's", "Labels, with commas"), &state.labels_text, Some(|t| go(Event::Labels(t))), D::Compact, None)).width(Length::FillPortion(1)),
            ]
            .spacing(4),
            row(KINDS.iter().map(|&k| {
                let active = topic.kind.eq_ignore_ascii_case(k);
                let p = self.theme.palette();
                named(button(text(kind_label(k, l)).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::KindOf(k))), Node::new(Role::ToggleButton, format!("{}: {}", l.text("Soort", "Type"), kind_label(k, l))).selected(active))
            }))
            .spacing(4)
            .wrap(),
        ]
        .spacing(6);
        if let Some(first) = topic.viewpoints.first() {
            let mut views = row![].spacing(4).align_y(Alignment::Center);
            for k in 0..topic.viewpoints.len() {
                let label = format!("{} {}", l.text("Standpunt", "Viewpoint"), k + 1);
                views = views.push(named(button(text(label.clone()).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(true, s)).on_press(go(Event::Viewpoint(topic.guid.clone(), k))), Node::new(Role::Button, label)));
            }
            detail = detail.push(views.wrap());
            let mut facts = vec![format!("{} {}", first.selection.len(), l.text("geselecteerd", "selected"))];
            if !first.exceptions.is_empty() {
                facts.push(format!("{} {}", first.exceptions.len(), if first.default_visibility { l.text("verborgen", "hidden") } else { l.text("getoond", "shown") }));
            }
            if !first.clipping_planes.is_empty() {
                facts.push(format!("{} {}", first.clipping_planes.len(), l.text("snijvlakken", "clipping planes")));
            }
            if !first.coloring.is_empty() {
                facts.push(l.text("kleuren (niet getoond)", "colours (not shown)").to_string());
            }
            detail = detail.push(w::secondary(look, facts.join(" · ")));
            if let Some((_, whole)) = first.snapshot.as_ref().and_then(|(_, bytes)| self.collab.pictures.get(&(first.guid.clone(), bytes.len()))) {
                detail = detail.push(iced::widget::image(whole.clone()).width(Length::Fill));
            }
        }
        let day = |date: &str| date.get(..10).unwrap_or(date).to_string();
        detail = detail.push(w::secondary(look, format!("{} {} · {}", l.text("Gemaakt door", "Made by"), topic.creation_author, day(&topic.creation_date))));
        if !topic.comments.is_empty() {
            detail = detail.push(w::divider(look));
        }
        for comment in &topic.comments {
            detail = detail.push(column![w::secondary(look, format!("{} · {}", comment.author, day(&comment.date))), w::body(look, comment.text.clone())].spacing(1));
        }
        detail = detail.push(
            row![
                container(w::text_field(look, l.text("Opmerking", "Comment"), &state.comment, Some(|t| go(Event::Comment(t))), D::Compact, None)).width(Length::Fill),
                w::button(look, l.text("Plaatsen", "Post"), K::Secondary, D::Compact, None, (!state.comment.trim().is_empty()).then(|| go(Event::AddComment))),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        );
        detail = detail.push(w::button(look, l.text("Issue verwijderen", "Remove issue"), K::Danger, D::Compact, None, Some(go(Event::RemoveTopic))));
        container(detail).padding(iced::Padding { left: 8.0, bottom: 10.0, top: 2.0, right: 0.0 }).into()
    }

    fn collab_checks<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let p = self.theme.palette();
        let mut list = column![self.collab_rule_set(look, l)].spacing(6);
        if let Some(rules) = state.rules() {
            list = list.push(w::checkbox(look, l.text("Modelcontroles", "Model checks"), rules.model_checks, Some(|on| go(Event::ModelChecks(on)))));
            list = list.push(w::checkbox(look, l.text("BIM basis ILS", "BIM basis ILS"), rules.ils, Some(|on| go(Event::Ils(on)))));
            list = list.push(w::checkbox(look, l.text("Ruimtes, deuren, vluchtwegen en brand", "Rooms, doors, escape routes and fire"), rules.rooms.on, Some(|on| go(Event::Rooms(on)))));
            if rules.rooms.on {
                let names = [l.text("Vrije hoogte ruimte", "Free height of a room"), l.text("Breedte deur", "Width of a door"), l.text("Hoogte deur", "Height of a door"), l.text("Loopafstand tot uitgang", "Walk to a way out"), l.text("Drempel deur", "Threshold of a door"), l.text("Helling 1 op", "Ramp 1 in"), l.text("Draaicirkel", "Turning circle")];
                for (k, name) in names.into_iter().enumerate() {
                    let typed = state.room_amounts.get(k).map(String::as_str).unwrap_or("");
                    list = list.push(
                        container(row![w::secondary(look, name).width(Length::Fill), container(w::text_field(look, "m", typed, Some(move |t| go(Event::RoomAmount(k, t))), D::Compact, None)).width(64), w::secondary(look, "m")].spacing(4).align_y(Alignment::Center))
                            .padding(iced::Padding { left: 24.0, ..iced::Padding::ZERO }),
                    );
                }
            }
            for (k, ids) in rules.ids.iter().enumerate() {
                let name = crate::collab::rules::ids_name(ids);
                list = list.push(row![w::body(look, format!("IDS · {name}")).width(Length::Fill), w::button(look, l.text("Weg", "Remove"), K::Quiet, D::Compact, None, Some(go(Event::RemoveIds(k))))].align_y(Alignment::Center));
            }
        }
        list = list.push(
            row![
                w::button(look, l.text("Model controleren", "Check model"), K::Primary, D::Compact, None, (!state.running).then(|| go(Event::Check))),
                w::button(look, l.text("IDS laden", "Load IDS"), K::Secondary, D::Compact, None, Some(go(Event::Open(File::Ids)))),
            ]
            .spacing(6)
            .wrap(),
        );
        if state.checked.is_empty() {
            return list.push(w::secondary(look, l.text("Nog niet gecontroleerd. De controle leest de IFC-bestanden van de geopende modellen.", "Not checked yet. The check reads the IFC files of the open models."))).into();
        }
        // how the rules came out, over all models
        let specs = || state.checked.iter().filter_map(|c| c.report.as_ref().ok()).flat_map(|r| &r.specifications);
        let count = |f: &dyn Fn(&SpecificationResult) -> bool| specs().filter(|s| f(s)).count();
        let failed = count(&|s| s.status == ValidationStatus::Failed && s.severity != Severity::Warning);
        let warned = count(&|s| s.status == ValidationStatus::Failed && s.severity == Severity::Warning);
        let unknown = count(&|s| s.status == ValidationStatus::NotCheckable);
        let passed = count(&|s| s.status == ValidationStatus::Passed);
        let mut summary = row![].spacing(4);
        // the models' quality as one number: how much of what was checked passes
        if let Some((score, letter)) = quality_score(specs()) {
            summary = summary.push(site::chip(look, &format!("{} {score} · {letter}", l.text("Kwaliteit", "Quality")), Some(if score >= 80 { Tone::Success } else if score >= 60 { Tone::Warning } else { Tone::Error })));
        }
        for (n, label, tone) in [
            (failed, l.text("voldoen niet", "failed"), Tone::Error),
            (warned, l.text("waarschuwingen", "warnings"), Tone::Warning),
            (unknown, l.text("niet te controleren", "not checkable"), Tone::Info),
            (passed, l.text("voldoen", "pass"), Tone::Success),
        ] {
            if n > 0 {
                summary = summary.push(site::chip(look, &format!("{n} {label}"), Some(tone)));
            }
        }
        list = list.push(w::divider(look)).push(summary.wrap());
        list = list.push(
            row![
                w::button(look, l.text("Issues maken", "Make issues"), K::Secondary, D::Compact, None, (failed + warned > 0).then(|| go(Event::IssuesFromChecks))),
                w::button(look, "PDF", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Pdf)))),
                w::button(look, "Excel", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Xlsx)))),
                w::button(look, "HTML", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Html)))),
                w::button(look, "CSV", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Csv)))),
                w::button(look, "JSON", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Json)))),
            ]
            .spacing(4)
            .wrap(),
        );
        list = list.push(w::checkbox(look, l.text("Alleen wat niet voldoet", "Only what does not pass"), state.only_failed, Some(|on| go(Event::OnlyFailed(on)))));
        for (c, checked) in state.checked.iter().enumerate() {
            let report = match &checked.report {
                Ok(report) => report,
                Err(e) => {
                    list = list.push(w::heading(look, checked.name.clone(), 13.0)).push(w::secondary(look, e.clone()));
                    continue;
                }
            };
            list = list.push(w::heading(look, checked.name.clone(), 13.0));
            if let Some(m) = &report.model {
                list = list.push(w::secondary(
                    look,
                    format!("{} {} · {} proxy · {} {}", m.element_count, l.text("elementen", "elements"), m.proxy_count, m.without_storey, l.text("zonder bouwlaag", "without storey")),
                ));
            }
            let mut any = false;
            for (s, spec) in report.specifications.iter().enumerate() {
                if state.only_failed && spec.status != ValidationStatus::Failed {
                    continue;
                }
                any = true;
                let open = state.open_spec == Some((c, s));
                let title = match &spec.identifier {
                    Some(code) if !code.is_empty() && !spec.name.starts_with(code.as_str()) => format!("{code} · {}", spec.name),
                    _ => spec.name.clone(),
                };
                // the name over its count and how it came out: a panel is narrow
                let line = column![
                    w::body(look, title),
                    row![
                        w::secondary(look, format!("{} / {} {}", spec.passed_count, spec.applicable_count, l.text("voldoen", "pass"))).width(Length::Fill),
                        site::chip(look, spec_text(spec, l), spec_tone(spec.status, spec.severity)),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                ]
                .spacing(2);
                list = list.push(named(button(line).width(Length::Fill).padding([4, 6]).style(move |_, st| p.ribbon_button(open, st)).on_press(go(Event::Spec((!open).then_some((c, s))))), Node::new(Role::Row, format!("{}: {}", l.text("Regel", "Rule"), spec.identifier.clone().unwrap_or_else(|| spec.name.clone()))).expanded(open).value(spec_text(spec, l))));
                if open {
                    let mut findings = column![].spacing(2);
                    for reason in &spec.reasons {
                        findings = findings.push(w::secondary(look, reason.clone()));
                    }
                    let failed = spec.findings.iter().filter(|f| f.status != ValidationStatus::Passed);
                    for f in failed.clone().take(SHOWN) {
                        let name = f.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("#{}", f.express_id));
                        let line = column![w::body(look, format!("{name} ({})", f.entity_type)), w::secondary(look, f.reason.clone())].spacing(1);
                        findings = findings.push(named(button(line).width(Length::Fill).padding([3, 6]).style(move |_, st| p.ribbon_button(false, st)).on_press(go(Event::Finding(c, f.express_id))), Node::new(Role::Row, format!("{}: {name}", l.text("Element", "Element")))));
                    }
                    let more = failed.count().saturating_sub(SHOWN);
                    if more > 0 {
                        findings = findings.push(w::secondary(look, format!("+ {more} {}", l.text("meer, zie het rapport", "more, see the report"))));
                    }
                    list = list.push(container(findings).padding(iced::Padding { left: 10.0, ..iced::Padding::ZERO }));
                }
            }
            if !any {
                list = list.push(w::secondary(look, l.text("Niets dat niet voldoet.", "Nothing that does not pass.")));
            }
        }
        list.into()
    }

    fn collab_clashes<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let p = self.theme.palette();
        let everything = l.text("alles", "everything");
        let mut list = column![self.collab_rule_set(look, l)].spacing(6);
        // the tests and the matrix fold away, so the clashes found show
        let open = state.show_tests;
        let fold = row![text(if open { "▾" } else { "▸" }).size(12).color(p.text_secondary), w::heading(look, l.text("Clashtests en disciplinematrix", "Clash tests and discipline matrix"), 13.0)].spacing(6).align_y(Alignment::Center);
        list = list.push(named(button(fold).width(Length::Fill).padding([3, 0]).style(move |_, s| p.row_button(s)).on_press(go(Event::ShowTests(!open))), Node::new(Role::Button, l.text("Clashtests en disciplinematrix", "Clash tests and discipline matrix")).value(if open { "open" } else { "closed" })));
        if let Some(rules) = state.rules().filter(|_| open) {
            for (k, rule) in rules.clashes.iter().enumerate() {
                let hard = rule.kind == ClashKind::Hard;
                let amount = state.amounts.get(k).map(String::as_str).unwrap_or("");
                let kind = |label: &'static str, active: bool, kind: ClashKind| named(button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::ClashKindOf(k, kind))), Node::new(Role::ToggleButton, format!("{} {}", rule.name, label)).selected(active));
                list = list.push(
                    column![
                        row![
                            container(w::checkbox(look, &rule.name, rule.on, Some(move |on| go(Event::ClashOn(k, on))))).width(Length::Fill),
                            w::button(look, l.text("Weg", "Remove"), K::Quiet, D::Compact, None, Some(go(Event::RemoveClashRule(k)))),
                        ]
                        .align_y(Alignment::Center),
                        {
                            // each side to choose; one a file set otherwise shows as it is
                            let side = |first: bool| {
                                let now = if first { &rule.a } else { &rule.b };
                                let picked = state.sides.iter().find(|s| &s.selector == now).cloned();
                                let known = picked.is_some();
                                let list: Element<'a, Message> = w::select(look, &state.sides, picked, move |s: Side| go(Event::ClashSide(k, first, s)), D::Compact);
                                let mut col = column![container(list).width(Length::Fill)].spacing(1).width(Length::FillPortion(1));
                                if !known {
                                    col = col.push(w::secondary(look, now.summary(everything)));
                                }
                                col
                            };
                            row![side(true), w::secondary(look, "×"), side(false)].spacing(4).align_y(Alignment::Start)
                        },
                        row![
                            kind(l.text("Doorsnijding", "Crossing"), hard, ClashKind::Hard),
                            kind(l.text("Vrije ruimte", "Clearance"), rule.kind == ClashKind::Clearance, ClashKind::Clearance),
                            kind(l.text("Zichtlijn", "Sight line"), rule.kind == ClashKind::Sight, ClashKind::Sight),
                            Space::new().width(Length::Fill),
                            container(w::text_field(look, "", amount, Some(move |t| go(Event::ClashAmount(k, t))), D::Compact, None)).width(64),
                            w::secondary(look, if rule.kind == ClashKind::Sight { "m" } else { "mm" }),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        named(
                            w::checkbox(look, l.text("Alleen tussen modellen", "Only between models"), rule.between_models, Some(move |on| go(Event::ClashBetween(k, on)))),
                            Node::new(Role::CheckBox, format!("{} {}", rule.name, l.text("Alleen tussen modellen", "Only between models"))).checked(rule.between_models),
                        ),
                        row![
                            w::secondary(look, l.text("Bouwlagen", "Storeys")).width(96),
                            container(w::text_field(look, l.text("alle; bijv. 00*, 01*", "all; e.g. 00*, 01*"), &rule.storeys, Some(move |t| go(Event::ClashStoreys(k, t))), D::Compact, None)).width(Length::Fill),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        row![
                            w::secondary(look, l.text("Eigenschap", "Property")).width(96),
                            container(w::text_field(look, l.text("naam, bijv. LoadBearing", "name, e.g. LoadBearing"), &rule.property, Some(move |t| go(Event::ClashProperty(k, t))), D::Compact, None)).width(Length::FillPortion(3)),
                            container(w::text_field(look, l.text("waarde, * voor iets", "value, * for any"), &rule.value, Some(move |t| go(Event::ClashValue(k, t))), D::Compact, None)).width(Length::FillPortion(2)),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        row![
                            w::secondary(look, l.text("Te zien", "To be seen")).width(96),
                            container(w::text_field(look, "*exit*, *vlucht*", &rule.sight_names, (rule.kind == ClashKind::Sight).then_some(move |t| go(Event::ClashSightNames(k, t))), D::Compact, None)).width(Length::FillPortion(3)),
                            container(w::text_field(look, "%", state.sight_shares.get(k).map(String::as_str).unwrap_or(""), (rule.kind == ClashKind::Sight).then_some(move |t| go(Event::ClashSightShare(k, t))), D::Compact, None)).width(Length::FillPortion(1)),
                            w::secondary(look, l.text("% zonder zicht", "% out of sight")),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        row![
                            w::secondary(look, l.text("Per typepaar", "Per pair of types")).width(96),
                            container(w::text_field(look, "IfcPipe* x IfcWall* = 25; …", &rule.pair_tolerances, Some(move |t| go(Event::ClashPairs(k, t))), D::Compact, None)).width(Length::Fill),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                    ]
                    .spacing(3),
                );
            }
        }
        if open {
            list = list.push(w::divider(look)).push(self.collab_matrix(look, l)).push(w::divider(look));
        }
        list = list.push(
            row![
                w::button(look, l.text("Clashes zoeken", "Find clashes"), K::Primary, D::Compact, None, (!state.running).then(|| go(Event::Clash))),
                w::button(look, l.text("Nieuwe test", "New test"), K::Secondary, D::Compact, None, Some(go(Event::AddClashRule))),
                w::button(look, l.text("Selectie clashen", "Clash selection"), K::Secondary, D::Compact, None, (!state.running && self.sel.items().iter().any(|i| matches!(i, Pickable::Element(_)))).then(|| go(Event::ClashSelection))),
            ]
            .spacing(6)
            .wrap(),
        );
        if state.clashes.is_empty() {
            return list.push(w::secondary(look, l.text("Geen clashes gevonden, of nog niet gezocht.", "No clashes found, or not looked for yet."))).into();
        }
        let place = state.clash.map_or(String::new(), |k| format!("{} / ", k + 1));
        list = list.push(w::divider(look)).push(
            row![
                w::icon_button(look, crate::b2::small("page.previous"), 16.0, false, l.text("Vorige clash", "Previous clash"), Some(go(Event::Step(-1)))),
                w::icon_button(look, crate::b2::small("page.next"), 16.0, false, l.text("Volgende clash", "Next clash"), Some(go(Event::Step(1)))),
                w::secondary(look, format!("{place}{} {}", state.clashes.len(), l.text("clashes", "clashes"))).width(Length::Fill),
                w::button(look, l.text("Issues maken", "Make issues"), K::Secondary, D::Compact, None, Some(go(Event::IssuesFromClashes))),
                w::button(look, "PDF", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Pdf)))),
                w::button(look, "Excel", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::Xlsx)))),
                w::button(look, "CSV", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::ClashesCsv)))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
        // against the run before: what is new, what stayed, what is solved; and the runs so far
        if state.compared {
            let new = state.clashes.iter().filter(|c| c.fresh).count();
            list = list.push(
                row![
                    site::chip(look, &format!("{new} {}", l.text("nieuw", "new")), Some(if new > 0 { Tone::Error } else { Tone::Success })),
                    site::chip(look, &format!("{} {}", state.clashes.len() - new, l.text("gebleven", "stayed")), None),
                    site::chip(look, &format!("{} {}", state.resolved.len(), l.text("opgelost", "solved")), Some(Tone::Success)),
                ]
                .spacing(4)
                .wrap(),
            );
        }
        let runs = state.rules().map(|r| r.clash_runs.as_slice()).unwrap_or(&[]);
        if runs.len() > 1 {
            // the last runs as bars of how many clashes are open, the latest at the right
            let last: Vec<&crate::collab::rules::ClashRun> = runs.iter().rev().take(12).rev().collect();
            let most = last.iter().map(|r| r.open).max().unwrap_or(1).max(1) as f32;
            let mut bars = row![].spacing(3).align_y(Alignment::End).height(36);
            for r in &last {
                let h = (r.open as f32 / most * 30.0).max(2.0);
                let fill = if std::ptr::eq(*r, *last.last().unwrap_or(r)) { p.accent } else { p.text_muted };
                bars = bars.push(named(
                    container(Space::new().width(10).height(h)).style(move |_| container::Style { background: Some(iced::Background::Color(fill)), ..Default::default() }),
                    Node::new(Role::Row, format!("{}: {} {}", r.date, r.open, l.text("open", "open"))),
                ));
            }
            let (first, latest) = (last[0], last[last.len() - 1]);
            list = list.push(row![bars, w::secondary(look, format!("{} {} · {} → {} {}", last.len(), l.text("runs", "runs"), first.open, latest.open, l.text("open", "open")))].spacing(8).align_y(Alignment::End));
        }
        let grouped = state.groupings.iter().find(|c| c.0 == state.grouping).cloned();
        let accepted = state.clashes.iter().filter(|c| c.decision == Decision::Accepted).count();
        let mut options = row![container(w::select(look, &state.groupings, grouped, |c| go(Event::GroupBy(c)), D::Compact)).width(150)].spacing(8).align_y(Alignment::Center);
        if accepted > 0 {
            options = options.push(w::checkbox(look, l.text("Geaccepteerde tonen", "Show accepted"), state.show_accepted, Some(|on| go(Event::ShowAccepted(on)))));
        }
        list = list.push(options);
        // how a clash is said: how far, or that it lies inside, or that it is one element twice
        let says = |clash: &ClashRow| -> (String, Tone) {
            let mm = (clash.distance * 1000.0).round();
            let (what, tone) = match clash.kind {
                _ if clash.copy => (l.text("dubbel element", "double element").to_string(), Tone::Error),
                Kind::Crossing => match clash.reach {
                    Reach::Graze => (format!("{} · {mm} mm", l.text("schampt", "grazes")), Tone::Warning),
                    Reach::Through => (format!("{} · {mm} mm", l.text("erdoorheen", "through")), Tone::Error),
                    _ => (format!("{mm} mm"), Tone::Error),
                },
                Kind::Inside => (l.text("ligt erin", "lies inside").to_string(), Tone::Error),
                Kind::Near => (format!("{mm} mm {}", l.text("ertussen", "apart")), Tone::Warning),
                Kind::Hidden => (format!("{:.0}% {}", clash.distance * 100.0, l.text("niet te zien", "out of sight")), Tone::Error),
            };
            let what = if clash.fresh { format!("{} · {what}", l.text("nieuw", "new")) } else { what };
            match clash.decision {
                Decision::Accepted => (l.text("akkoord", "accepted").to_string(), Tone::Success),
                Decision::Rejected => (format!("{what} · {}", l.text("afgewezen", "rejected")), tone),
                Decision::Open => (what, tone),
            }
        };
        let clash_row = |k: usize, indent: f32| -> Element<'a, Message> {
            let clash = &state.clashes[k];
            let picked = state.clash == Some(k);
            let (what, tone) = says(clash);
            let line = row![column![w::body(look, clash.a_name.clone()), w::secondary(look, format!("× {}", clash.b_name))].spacing(1).width(Length::Fill), site::chip(look, &what, Some(tone))].spacing(6).align_y(Alignment::Center);
            let mut out = column![named(button(line).width(Length::Fill).padding([4, 6]).style(move |_, st| p.ribbon_button(picked, st)).on_press(go(Event::PickClash(k))), Node::new(Role::Row, format!("{} {}: {} × {}", l.text("Clash", "Clash"), k + 1, clash.a_name, clash.b_name)).selected(picked).value(clash.decision.key()))].spacing(4);
            if picked {
                let decide = |label: &'static str, d: Decision| {
                    let active = clash.decision == d;
                    named(button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::Decide(k, if active { Decision::Open } else { d }))), Node::new(Role::ToggleButton, label).selected(active))
                };
                out = out.push(
                    container(
                        row![
                            w::button(look, l.text("Issue maken", "Make issue"), K::Secondary, D::Compact, None, Some(go(Event::IssueFromClash(k)))),
                            decide(l.text("Akkoord", "Accept"), Decision::Accepted),
                            decide(l.text("Afwijzen", "Reject"), Decision::Rejected),
                            w::button(look, l.text("Doorsnede", "Section"), K::Quiet, D::Compact, None, Some(go(Event::SectionAround))),
                        ]
                        .spacing(4)
                        .wrap(),
                    )
                    .padding(iced::Padding { left: 8.0, ..iced::Padding::ZERO }),
                );
            }
            container(out).padding(iced::Padding { left: indent, ..iced::Padding::ZERO }).into()
        };
        let hidden = |k: usize| !state.show_accepted && state.clashes[k].decision == Decision::Accepted && state.clash != Some(k);
        let groups = self.collab_groups();
        let (mut rule, mut shown, mut covered): (Option<&str>, usize, usize) = (None, 0, 0);
        for group in &groups {
            if shown >= SHOWN {
                break;
            }
            covered += group.len();
            let first = &state.clashes[group[0]];
            // the clashes of one test under its name
            if rule != Some(first.rule.as_str()) {
                rule = Some(first.rule.as_str());
                let of_rule = state.clashes.iter().filter(|c| c.rule == first.rule).count();
                list = list.push(w::heading(look, format!("{} · {of_rule}", first.rule), 13.0));
            }
            let visible: Vec<usize> = group.iter().copied().filter(|&k| !hidden(k)).collect();
            if visible.is_empty() {
                continue;
            }
            if group.len() == 1 {
                list = list.push(clash_row(group[0], 0.0));
                shown += 1;
                continue;
            }
            // a group: what it shares, how many, the worst of them; open, its clashes under it
            let g = first.group;
            let open = state.open_group == Some(g);
            let rows: Vec<&ClashRow> = group.iter().map(|&k| &state.clashes[k]).collect();
            let title = group_title(&rows);
            let undecided = rows.iter().filter(|c| c.decision == Decision::Open).count();
            let worst = rows.iter().filter(|c| c.decision != Decision::Accepted).max_by(|x, y| x.reach.cmp(&y.reach).then(x.distance.total_cmp(&y.distance))).copied().unwrap_or(first);
            let (what, tone) = if undecided == 0 && rows.iter().all(|c| c.decision == Decision::Accepted) { (l.text("akkoord", "accepted").to_string(), Tone::Success) } else { says(worst) };
            let count = format!("{} {}{}", rows.len(), l.text("clashes", "clashes"), if undecided < rows.len() { format!(" · {undecided} {}", l.text("open", "open")) } else { String::new() });
            let line = row![
                text(if open { "▾" } else { "▸" }).size(12).color(p.text_secondary),
                column![w::body(look, title.clone()), w::secondary(look, count)].spacing(1).width(Length::Fill),
                site::chip(look, &what, Some(tone)),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            let picked = open && state.clash.is_none();
            list = list.push(named(
                button(line).width(Length::Fill).padding([4, 6]).style(move |_, st| p.ribbon_button(picked, st)).on_press(go(Event::PickGroup(g))),
                Node::new(Role::Row, format!("{} {title}: {} {}", l.text("Groep", "Group"), rows.len(), l.text("clashes", "clashes"))).selected(open),
            ));
            shown += 1;
            if open {
                let all = |d: Decision| rows.iter().all(|c| c.decision == d);
                let decide = |label: &'static str, d: Decision| {
                    let active = all(d);
                    named(button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::DecideGroup(g, if active { Decision::Open } else { d }))), Node::new(Role::ToggleButton, format!("{label} ({})", l.text("groep", "group"))).selected(active))
                };
                list = list.push(
                    container(
                        row![
                            w::button(look, l.text("Eén issue", "One issue"), K::Secondary, D::Compact, None, Some(go(Event::IssueFromGroup(g)))),
                            decide(l.text("Alle akkoord", "Accept all"), Decision::Accepted),
                            decide(l.text("Alle afwijzen", "Reject all"), Decision::Rejected),
                            w::button(look, l.text("Doorsnede", "Section"), K::Quiet, D::Compact, None, Some(go(Event::SectionAround))),
                        ]
                        .spacing(4)
                        .wrap(),
                    )
                    .padding(iced::Padding { left: 16.0, ..iced::Padding::ZERO }),
                );
                for &k in &visible {
                    list = list.push(clash_row(k, 16.0));
                    shown += 1;
                }
            }
        }
        if covered < state.clashes.len() {
            list = list.push(w::secondary(look, format!("+ {} {}", state.clashes.len() - covered, l.text("meer, zie het CSV-bestand", "more, see the CSV file"))));
        }
        // what the run before had and this one no longer finds
        if !state.resolved.is_empty() {
            let open = state.show_resolved;
            let fold = row![text(if open { "▾" } else { "▸" }).size(12).color(p.text_secondary), w::heading(look, format!("{} · {}", l.text("Opgelost sinds de vorige run", "Solved since the run before"), state.resolved.len()), 13.0)].spacing(6).align_y(Alignment::Center);
            list = list.push(w::divider(look)).push(named(button(fold).width(Length::Fill).padding([3, 0]).style(move |_, s| p.row_button(s)).on_press(go(Event::ShowResolved(!open))), Node::new(Role::Button, l.text("Opgelost sinds de vorige run", "Solved since the run before")).value(if open { "open" } else { "closed" })));
            if open {
                for memo in state.resolved.iter().take(SHOWN) {
                    list = list.push(row![column![w::body(look, memo.a.clone()), w::secondary(look, format!("× {}", memo.b))].spacing(1).width(Length::Fill), site::chip(look, l.text("opgelost", "solved"), Some(Tone::Success))].spacing(6).align_y(Alignment::Center));
                }
            }
        }
        list.into()
    }

    /// The matrix of the rule set: its disciplines, each with what it takes, and which of them
    /// are tested against which, one tick for each pair.
    fn collab_matrix<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let Some(rules) = state.rules() else { return column![].into() };
        let mut out = column![w::heading(look, l.text("Disciplinematrix", "Discipline matrix"), 13.0)].spacing(4);
        for (k, d) in rules.disciplines.iter().enumerate() {
            let picked = state.sides.iter().find(|s| s.selector == d.take).cloned();
            let mut side = column![container(w::select(look, &state.sides, picked.clone(), move |s: Side| go(Event::DisciplineSide(k, s)), D::Compact)).width(Length::Fill)].spacing(1).width(Length::FillPortion(3));
            if picked.is_none() {
                side = side.push(w::secondary(look, d.take.summary(l.text("alles", "everything"))));
            }
            out = out.push(
                row![
                    container(w::text_field(look, l.text("Naam", "Name"), &d.name, Some(move |t: String| go(Event::DisciplineName(k, t))), D::Compact, None)).width(Length::FillPortion(2)),
                    side,
                    w::button(look, l.text("Weg", "Remove"), K::Quiet, D::Compact, None, Some(go(Event::RemoveDiscipline(k)))),
                ]
                .spacing(4)
                .align_y(Alignment::Start),
            );
        }
        // the pairs: each discipline against those before it and itself
        if !rules.disciplines.is_empty() {
            // the rows by name and number, the columns by number: names would not fit above
            let cell = 32.0;
            let mut head = row![Space::new().width(Length::Fill)].spacing(2);
            for k in 0..rules.disciplines.len() {
                head = head.push(container(w::secondary(look, (k + 1).to_string())).width(cell).center_x(cell));
            }
            let mut grid = column![head].spacing(4);
            for (i, a) in rules.disciplines.iter().enumerate() {
                let mut line = row![container(w::body(look, format!("{} {}", i + 1, a.name))).width(Length::Fill)].spacing(2).align_y(Alignment::Center);
                for (j, b) in rules.disciplines.iter().enumerate() {
                    if j > i {
                        line = line.push(Space::new().width(cell));
                        continue;
                    }
                    let on = rules.in_matrix(&a.name, &b.name);
                    line = line.push(named(
                        container(w::checkbox(look, "", on, Some(move |_| go(Event::MatrixCell(i, j))))).width(cell).center_x(cell),
                        Node::new(Role::CheckBox, format!("{} × {}", a.name, b.name)).checked(on),
                    ));
                }
                grid = grid.push(line);
            }
            out = out.push(grid);
        }
        out = out.push(
            row![
                w::button(look, l.text("Discipline toevoegen", "Add discipline"), K::Secondary, D::Compact, None, Some(go(Event::AddDiscipline))),
                Space::new().width(Length::Fill),
                w::secondary(look, l.text("Tolerantie", "Tolerance")),
                container(w::text_field(look, "mm", &state.matrix_amount, Some(|t: String| go(Event::MatrixAmount(t))), D::Compact, None)).width(64),
                w::secondary(look, "mm"),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
        out.push(named(
            w::checkbox(look, l.text("Matrix alleen tussen modellen", "Matrix only between models"), rules.matrix_between_models, Some(|on| go(Event::MatrixBetween(on)))),
            Node::new(Role::CheckBox, l.text("Matrix alleen tussen modellen", "Matrix only between models")).checked(rules.matrix_between_models),
        ))
        .into()
    }
}

/// A side of a clash test in a list to choose from: what it is called and what it takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    label: String,
    pub(super) selector: Selector,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The kinds an issue can be, as BCF names them.
const KINDS: [&str; 5] = ["Issue", "Clash", "Remark", "Request", "Fault"];

fn kind_label(kind: &str, l: Language) -> String {
    match kind {
        "Issue" => l.text("Issue", "Issue").to_string(),
        "Clash" => l.text("Clash", "Clash").to_string(),
        "Remark" => l.text("Opmerking", "Remark").to_string(),
        "Request" => l.text("Verzoek", "Request").to_string(),
        // as other tools name a fault
        "Fault" | "Error" => l.text("Fout", "Fault").to_string(),
        "Warning" => l.text("Waarschuwing", "Warning").to_string(),
        other => other.to_string(),
    }
}

/// The issues as one HTML page: each with its picture, what it says and its comments.
fn issues_html(issues: &Issues, l: Language) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let mut out = format!(
        "<!doctype html><html lang=\"{}\"><meta charset=\"utf-8\"><title>{}</title><style>body{{font:14px system-ui,sans-serif;margin:24px;color:#222}}h1{{font-size:22px}}article{{border:1px solid #ccc;border-radius:6px;padding:12px 16px;margin:0 0 16px;page-break-inside:avoid}}h2{{font-size:16px;margin:0 0 6px}}.meta{{color:#666;font-size:12px}}img{{max-width:100%;border:1px solid #ddd;margin:8px 0}}.c{{border-left:3px solid #ddd;padding-left:8px;margin:6px 0}}</style>",
        if l == Language::Dutch { "nl" } else { "en" },
        l.text("Issues", "Issues")
    );
    out += &format!("<h1>{} ({})</h1>", l.text("Issues", "Issues"), issues.topics.len());
    for (k, t) in issues.topics.iter().enumerate() {
        out += &format!("<article><h2>{}. {}</h2>", k + 1, esc(&t.title));
        let mut meta = vec![format!("{}: {}", l.text("Status", "Status"), esc(&t.status)), format!("{}: {}", l.text("Soort", "Type"), esc(&t.kind))];
        for (label, value) in [(l.text("Prioriteit", "Priority"), &t.priority), (l.text("Toegewezen aan", "Assigned to"), &t.assigned_to), (l.text("Uiterlijk", "Due"), &t.due_date)] {
            if !value.is_empty() {
                meta.push(format!("{label}: {}", esc(value)));
            }
        }
        if !t.labels.is_empty() {
            meta.push(format!("{}: {}", l.text("Labels", "Labels"), esc(&t.labels.join(", "))));
        }
        meta.push(format!("{} {} · {}", l.text("Gemaakt door", "Made by"), esc(&t.creation_author), esc(t.creation_date.get(..10).unwrap_or(&t.creation_date))));
        out += &format!("<div class=\"meta\">{}</div>", meta.join(" · "));
        if let Some((name, bytes)) = t.viewpoints.first().and_then(|v| v.snapshot.as_ref()) {
            let mime = if name.to_ascii_lowercase().ends_with(".jpg") || name.to_ascii_lowercase().ends_with(".jpeg") { "image/jpeg" } else { "image/png" };
            out += &format!("<img alt=\"\" src=\"data:{mime};base64,{}\">", crate::remote::base64(bytes));
        }
        if !t.description.is_empty() {
            out += &format!("<p>{}</p>", esc(&t.description).replace('\n', "<br>"));
        }
        for c in &t.comments {
            out += &format!("<div class=\"c\"><div class=\"meta\">{} · {}</div>{}</div>", esc(&c.author), esc(c.date.get(..10).unwrap_or(&c.date)), esc(&c.text));
        }
        out += "</article>";
    }
    out + "</html>\n"
}

impl App {
    /// Two versions of a model compared: which, and what was added, removed and changed.
    fn collab_compare<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab;
        let p = self.theme.palette();
        let mut list = column![w::secondary(
            look,
            l.text(
                "Open de oude en de nieuwe versie van een model samen (Toevoegen) en vergelijk ze: elementen worden herkend aan hun GlobalId.",
                "Open the old and the new version of a model together (Add) and compare them: elements are known by their GlobalId.",
            )
        )]
        .spacing(6);
        let (old, new) = state.compare_pair;
        list = list.push(
            column![
                column![w::mono_label(look, l.text("Oud", "Old")), w::select(look, &state.model_names, state.model_names.get(old).cloned(), |c: Choice| go(Event::CompareOld(c)), D::Compact)].spacing(2).width(Length::FillPortion(1)),
                column![w::mono_label(look, l.text("Nieuw", "New")), w::select(look, &state.model_names, state.model_names.get(new).cloned(), |c: Choice| go(Event::CompareNew(c)), D::Compact)].spacing(2).width(Length::FillPortion(1)),
            ]
            .spacing(8),
        );
        list = list.push(w::button(look, l.text("Vergelijken", "Compare"), K::Primary, D::Compact, None, (state.model_names.len() >= 2 && old != new).then(|| go(Event::Compare))));
        let Some(c) = &state.comparison else { return list.into() };
        list = list.push(w::divider(look)).push(w::secondary(look, format!("{} ({}) → {} ({} {})", c.old, c.counts.0, c.new, c.counts.1, l.text("elementen", "elements"))));
        let mut summary = row![].spacing(4);
        summary = summary.push(site::chip(look, &format!("{} {}", c.added.len(), l.text("toegevoegd", "added")), Some(Tone::Success)));
        summary = summary.push(site::chip(look, &format!("{} {}", c.removed.len(), l.text("verwijderd", "removed")), Some(Tone::Error)));
        summary = summary.push(site::chip(look, &format!("{} {}", c.changed.len(), l.text("gewijzigd", "changed")), Some(Tone::Warning)));
        list = list.push(summary.wrap());
        let group = |list: iced::widget::Column<'a, Message>, title: String, rows: Vec<(String, String)>, kind: u8| -> iced::widget::Column<'a, Message> {
            if rows.is_empty() {
                return list;
            }
            let mut list = list.push(w::heading(look, title, 13.0));
            for (k, (name, why)) in rows.iter().enumerate().take(SHOWN) {
                let mut line = column![w::body(look, name.clone())].spacing(1);
                if !why.is_empty() {
                    line = line.push(w::secondary(look, why.clone()));
                }
                list = list.push(named(button(line).width(Length::Fill).padding([3, 6]).style(move |_, s| p.ribbon_button(false, s)).on_press(go(Event::ComparePick(kind, k))), Node::new(Role::Row, format!("{}: {name}", l.text("Element", "Element")))));
            }
            if rows.len() > SHOWN {
                list = list.push(w::secondary(look, format!("+ {}", rows.len() - SHOWN)));
            }
            list
        };
        list = group(list, format!("{} · {}", l.text("Gewijzigd", "Changed"), c.changed.len()), c.changed.iter().map(|x| (x.1.clone(), x.2.clone())).collect(), 2);
        list = group(list, format!("{} · {}", l.text("Toegevoegd", "Added"), c.added.len()), c.added.iter().map(|x| (x.1.clone(), String::new())).collect(), 0);
        list = group(list, format!("{} · {}", l.text("Verwijderd", "Removed"), c.removed.len()), c.removed.iter().map(|x| (x.1.clone(), String::new())).collect(), 1);
        list.into()
    }

    /// A remote command of the Collaborator (`collab_…`): what a person does in its panel,
    /// answered at once; checks and clash tests run before the answer.
    pub(crate) fn collab_command(&mut self, cmd: &str, a: &serde_json::Value) -> Result<serde_json::Value, String> {
        use serde_json::json;
        self.collab.refresh();
        self.collab_refresh_sides();
        self.inform_refresh();
        if cmd == "collab_share" {
            match a.get("action").and_then(|v| v.as_str()).unwrap_or("sync") {
                "set" => {
                    let folder = a.get("folder").and_then(|v| v.as_str()).map(PathBuf::from).filter(|p| p.is_dir()).ok_or("give an existing folder")?;
                    let _ = self.collab_handle(Event::ShareFolderIs(Some(folder)));
                }
                "off" => {
                    let _ = self.collab_handle(Event::Unshare);
                }
                _ => self.collab_sync(true),
            }
            return Ok(serde_json::json!({ "folder": self.collab.share_folder.as_ref().map(|p| p.display().to_string()), "issues": self.collab.issues.topics.len(), "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }));
        }
        if cmd == "collab_cloud" {
            return self.cloud_command(a);
        }
        if matches!(cmd, "collab_colours" | "collab_sets" | "collab_takeoff") {
            return self.inform_command(cmd, a);
        }
        let text = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let action = text("action").unwrap_or_default();
        let topic_json = |t: &Topic, full: bool| {
            let mut v = json!({
                "guid": t.guid, "title": t.title, "type": t.kind, "status": t.status, "priority": t.priority,
                "assigned_to": t.assigned_to, "due_date": t.due_date, "labels": t.labels, "author": t.creation_author,
                "created": t.creation_date, "comments": t.comments.len(), "viewpoints": t.viewpoints.len(),
                "snapshot": t.viewpoints.first().is_some_and(|v| v.snapshot.is_some()), "elements": t.elements().len(),
            });
            if full {
                v["description"] = json!(t.description);
                v["comments"] = json!(t.comments.iter().map(|c| json!({ "author": c.author, "date": c.date, "text": c.text })).collect::<Vec<_>>());
                v["element_guids"] = json!(t.elements());
            }
            v
        };
        let guid_of = |app: &App| -> Result<String, String> {
            let by_place = a.get("index").and_then(|v| v.as_u64()).and_then(|k| app.collab.issues.topics.get(k as usize)).map(|t| t.guid.clone());
            let guid = text("guid").or(by_place).or_else(|| app.collab.topic.clone()).ok_or("give the issue's guid or its index in the list")?;
            app.collab.issues.topics.iter().any(|t| t.guid == guid).then_some(guid.clone()).ok_or(format!("no issue {guid}"))
        };
        let answer = match cmd {
            "collab_state" => {
                let rules = self.collab.rules();
                Ok(json!({
                    "user": self.collab.author,
                    "tab": format!("{:?}", self.collab.tab),
                    "rule_set": rules.map(|r| r.name.clone()),
                    "rule_sets": self.collab.rule_sets.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
                    "issues": self.collab.issues.topics.len(), "open_issues": self.collab.open_issues(), "report": self.collab.report_job.is_some(),
                    "issues_file": self.collab.issues_path.as_ref().map(|p| p.display().to_string()), "issues_changed": self.collab.issues_changed,
                    "shown_issue": self.collab.topic,
                    "checked_models": self.collab.checked.len(), "clashes": self.collab.clashes.len(), "picked_clash": self.collab.clash,
                    "isolate": self.collab.isolate, "team": self.collab.team.iter().map(|c| c.1.clone()).collect::<Vec<_>>(),
                    "models": self.models.iter().map(|e| e.model.path.file_name().map(|n| n.to_string_lossy().into_owned())).collect::<Vec<_>>(),
                    "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()),
                }))
            }
            "collab_issue" => match action.as_str() {
                "list" | "" => {
                    let status = text("status").map(|s| s.to_lowercase());
                    Ok(json!({ "issues": self.collab.issues.topics.iter().filter(|t| status.as_deref().is_none_or(|s| t.status.to_lowercase() == s)).map(|t| topic_json(t, false)).collect::<Vec<_>>() }))
                }
                "get" => {
                    let guid = guid_of(self)?;
                    Ok(topic_json(self.collab.issues.topics.iter().find(|t| t.guid == guid).ok_or("gone")?, true))
                }
                "new" => {
                    let _ = self.collab_handle(Event::NewIssue);
                    let guid = self.collab.topic.clone().ok_or("no issue made")?;
                    self.collab_edit_topic(&guid, a);
                    Ok(json!({ "guid": guid, "note": "the picture of the view is taken by the window; from here use screenshot to see it" }))
                }
                "update" => {
                    let guid = guid_of(self)?;
                    self.collab_edit_topic(&guid, a);
                    Ok(topic_json(self.collab.issues.topics.iter().find(|t| t.guid == guid).ok_or("gone")?, false))
                }
                "comment" => {
                    let guid = guid_of(self)?;
                    self.collab.topic = Some(guid.clone());
                    let _ = self.collab_handle(Event::Comment(text("text").ok_or("give the comment's text")?));
                    let _ = self.collab_handle(Event::AddComment);
                    Ok(json!({ "guid": guid, "comments": self.collab.issues.topics.iter().find(|t| t.guid == guid).map_or(0, |t| t.comments.len()) }))
                }
                "show" => {
                    let guid = guid_of(self)?;
                    let k = a.get("viewpoint").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    self.collab.topic = Some(guid.clone());
                    self.collab.tab = Tab::Issues;
                    let _ = self.collab_handle(Event::Viewpoint(guid.clone(), k));
                    Ok(json!({ "guid": guid, "selected": self.sel.len(), "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }))
                }
                "remove" => {
                    let guid = guid_of(self)?;
                    self.collab.topic = Some(guid.clone());
                    let _ = self.collab_handle(Event::RemoveTopic);
                    Ok(json!({ "removed": guid, "issues": self.collab.issues.topics.len() }))
                }
                other => Err(format!("collab_issue: unknown action {other} (list, get, new, update, comment, show, remove)")),
            },
            "collab_bcf" => {
                let path = text("path").map(PathBuf::from).ok_or("give the .bcfzip path")?;
                match action.as_str() {
                    "open" => {
                        let _ = self.collab_handle(Event::Opened(File::Bcf, Some(path)));
                        match &self.collab.notice {
                            Some((Tone::Error, e)) => Err(e.clone()),
                            _ => Ok(json!({ "issues": self.collab.issues.topics.len(), "version": self.collab.issues.version })),
                        }
                    }
                    "save" => {
                        if let Some(version) = text("version") {
                            self.collab.bcf3 = version.starts_with('3');
                        }
                        let _ = self.collab_handle(Event::SaveIssuesTo(Some(path.clone())));
                        match &self.collab.notice {
                            Some((Tone::Error, e)) => Err(e.clone()),
                            _ => Ok(json!({ "saved": path.display().to_string(), "issues": self.collab.issues.topics.len() })),
                        }
                    }
                    other => Err(format!("collab_bcf: unknown action {other} (open, save)")),
                }
            }
            "collab_rules" => match action.as_str() {
                "list" | "" => Ok(json!({
                    "current": self.collab.rule_set,
                    "rule_sets": self.collab.rule_sets.iter().map(|r| json!({
                        "name": r.name, "description": r.description, "model_checks": r.model_checks, "ils": r.ils,
                        "ids": r.ids.iter().map(|p| crate::collab::rules::ids_name(p)).collect::<Vec<_>>(),
                        "clash_tests": r.clashes.iter().map(|c| json!({ "name": c.name, "on": c.on, "kind": format!("{:?}", c.kind), "tolerance_mm": c.tolerance_mm, "clearance_mm": c.clearance_mm, "between_models": c.between_models })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })),
                "select" => {
                    let k = match a.get("rule_set") {
                        Some(serde_json::Value::Number(n)) => n.as_u64().map(|n| n as usize),
                        Some(serde_json::Value::String(s)) => self.collab.rule_sets.iter().position(|r| r.name.to_lowercase().contains(&s.to_lowercase())),
                        _ => None,
                    }
                    .ok_or("give rule_set: its index or name")?;
                    let _ = self.collab_handle(Event::RuleSet(k));
                    Ok(json!({ "current": self.collab.rule_set, "name": self.collab.rules().map(|r| r.name.clone()) }))
                }
                "open" => {
                    let _ = self.collab_handle(Event::Opened(File::Rules, Some(text("path").map(PathBuf::from).ok_or("give the path")?)));
                    Ok(json!({ "current": self.collab.rule_set, "name": self.collab.rules().map(|r| r.name.clone()), "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }))
                }
                "save" => {
                    let _ = self.collab_handle(Event::SaveRulesTo(Some(text("path").map(PathBuf::from).ok_or("give the path")?)));
                    Ok(json!({ "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()) }))
                }
                "load_ids" => {
                    let _ = self.collab_handle(Event::Opened(File::Ids, Some(text("path").map(PathBuf::from).ok_or("give the .ids path")?)));
                    Ok(json!({ "ids": self.collab.rules().map(|r| r.ids.iter().map(|p| crate::collab::rules::ids_name(p)).collect::<Vec<_>>()) }))
                }
                "set" => {
                    if let Some(on) = a.get("model_checks").and_then(|v| v.as_bool()) {
                        let _ = self.collab_handle(Event::ModelChecks(on));
                    }
                    if let Some(on) = a.get("ils").and_then(|v| v.as_bool()) {
                        let _ = self.collab_handle(Event::Ils(on));
                    }
                    if let Some(on) = a.get("rooms").and_then(|v| v.as_bool()) {
                        let _ = self.collab_handle(Event::Rooms(on));
                    }
                    if let Some(name) = text("user") {
                        let _ = self.collab_handle(Event::User(name));
                    }
                    Ok(json!({ "user": self.collab.author, "model_checks": self.collab.rules().map(|r| r.model_checks), "ils": self.collab.rules().map(|r| r.ils) }))
                }
                other => Err(format!("collab_rules: unknown action {other} (list, select, open, save, load_ids, set)")),
            },
            "collab_check" => {
                self.collab_now(None, None, Some("check"))?;
                Ok(json!({ "models": self.collab.checked.iter().map(|c| match &c.report {
                    Ok(r) => json!({
                        "model": c.name, "status": status_key(r.status),
                        "rules": r.specifications.iter().map(|s| json!({ "code": s.identifier, "name": s.name, "status": status_key(s.status), "warning": s.severity == Severity::Warning, "passed": s.passed_count, "applicable": s.applicable_count })).collect::<Vec<_>>(),
                    }),
                    Err(e) => json!({ "model": c.name, "error": e }),
                }).collect::<Vec<_>>() }))
            }
            "collab_clashes" => match action.as_str() {
                "run" | "" => {
                    let started = Instant::now();
                    if a.get("selection").and_then(|v| v.as_bool()).unwrap_or(false) {
                        let _ = self.collab_handle(Event::ClashSelection);
                        // the clash test runs on a thread in the window; here it is waited for
                        return Ok(json!({ "started": true, "note": "the selection's clash test runs in the background; ask again with action list" }));
                    }
                    self.collab_now(None, None, Some("clash"))?;
                    let mut by_rule = std::collections::BTreeMap::<&str, usize>::new();
                    for c in &self.collab.clashes {
                        *by_rule.entry(c.rule.as_str()).or_default() += 1;
                    }
                    // at the most kept, the tests after it did not run
                    Ok(json!({ "clashes": self.collab.clashes.len(), "groups": self.collab.clashes.last().map_or(0, |c| c.group + 1), "compared": self.collab.compared, "new": self.collab.clashes.iter().filter(|c| c.fresh).count(), "solved": self.collab.resolved.len(), "runs": self.collab.rules().map_or(0, |r| r.clash_runs.len()), "by_rule": by_rule, "capped": self.collab.clashes.len() >= MOST_CLASHES, "seconds": started.elapsed().as_secs_f32() }))
                }
                "list" => {
                    let offset = a.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let limit = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(500) as usize;
                    Ok(json!({ "total": self.collab.clashes.len(), "clashes": self.collab.clashes.iter().enumerate().skip(offset).take(limit).map(|(k, c)| json!({
                        "index": k, "rule": c.rule, "a": c.a, "b": c.b, "a_name": c.a_name, "b_name": c.b_name,
                        "kind": format!("{:?}", c.kind), "distance_mm": (c.distance * 1000.0).round(), "decision": c.decision.key(),
                        "group": c.group, "double": c.copy, "new": c.fresh, "reach": c.reach.key(),
                    })).collect::<Vec<_>>() }))
                }
                "group" => {
                    let by = match text("by").as_deref().unwrap_or("element") {
                        "off" | "none" => Grouping::Off,
                        "element" => Grouping::Element,
                        "place" => Grouping::Place,
                        other => return Err(format!("group by off, element or place, not {other}")),
                    };
                    let _ = self.collab_handle(Event::GroupBy(GroupChoice(by, String::new())));
                    Ok(json!({ "groups": self.collab.clashes.last().map_or(0, |c| c.group + 1), "clashes": self.collab.clashes.len() }))
                }
                "open_group" => {
                    let g = a.get("group").and_then(|v| v.as_u64()).ok_or("give the group")? as usize;
                    let _ = self.collab_handle(Event::PickGroup(g));
                    Ok(json!({ "open": self.collab.open_group, "selected": self.sel.len() }))
                }
                "pick" => {
                    let k = a.get("index").and_then(|v| v.as_u64()).ok_or("give the clash's index")? as usize;
                    let _ = self.collab_handle(Event::PickClash(k));
                    Ok(json!({ "picked": self.collab.clash, "selected": self.sel.len() }))
                }
                "decide" => {
                    let k = a.get("index").and_then(|v| v.as_u64()).ok_or("give the clash's index")? as usize;
                    let d = text("decision").as_deref().and_then(Decision::parse).ok_or("decision: open, accepted or rejected")?;
                    let _ = self.collab_handle(Event::Decide(k, d));
                    Ok(json!({ "index": k, "decision": self.collab.clashes.get(k).map(|c| c.decision.key()) }))
                }
                "issue" => {
                    let k = a.get("index").and_then(|v| v.as_u64()).ok_or("give the clash's index")? as usize;
                    let _ = self.collab_handle(Event::IssueFromClash(k));
                    Ok(json!({ "guid": self.collab.topic, "issues": self.collab.issues.topics.len() }))
                }
                "issues" => {
                    let _ = self.collab_handle(Event::IssuesFromClashes);
                    Ok(json!({ "issues": self.collab.issues.topics.len() }))
                }
                other => Err(format!("collab_clashes: unknown action {other} (run, list, group, open_group, pick, decide, issue, issues)")),
            },
            "collab_compare" => {
                let pick = |k: &str| a.get(k).and_then(|v| v.as_u64()).map(|n| n as usize);
                if let (Some(old), Some(new)) = (pick("old"), pick("new")) {
                    self.collab.compare_pair = (old, new);
                }
                let _ = self.collab_handle(Event::Compare);
                match &self.collab.comparison {
                    Some(c) if self.collab.tab == Tab::Compare => Ok(json!({
                        "old": c.old, "new": c.new, "old_elements": c.counts.0, "new_elements": c.counts.1, "added": c.added.len(), "removed": c.removed.len(), "changed": c.changed.len(),
                        "changed_first": c.changed.iter().take(20).map(|x| json!({ "element": x.0, "name": x.1, "what": x.2 })).collect::<Vec<_>>(),
                    })),
                    _ => Err(self.collab.notice.as_ref().map_or("nothing to compare".to_string(), |(_, m)| m.clone())),
                }
            }
            "collab_export" => {
                let path = text("path").map(PathBuf::from).ok_or("give the path")?;
                let kind = match text("kind").as_deref().unwrap_or("issues") {
                    "html" | "report" => Report::Html,
                    "csv" => Report::Csv,
                    "json" => Report::Json,
                    "clashes" => Report::ClashesCsv,
                    "pdf" => Report::Pdf,
                    "xlsx" | "excel" => Report::Xlsx,
                    _ => Report::IssuesHtml,
                };
                let _ = self.collab_handle(Event::ExportTo(kind, Some(path.clone())));
                match &self.collab.notice {
                    Some((Tone::Error, e)) => Err(e.clone()),
                    _ => Ok(json!({ "saved": path.display().to_string() })),
                }
            }
            other => Err(format!("unknown command {other}")),
        };
        self.collab.refresh();
        answer
    }

    /// Sets the fields of an issue that a remote command names.
    fn collab_edit_topic(&mut self, guid: &str, a: &serde_json::Value) {
        self.collab.topic = Some(guid.to_string());
        let text = |k: &str| a.get(k).and_then(|v| v.as_str()).map(str::to_string);
        if let Some(t) = text("title") {
            let _ = self.collab_handle(Event::Title(t));
        }
        if let Some(t) = text("description") {
            let _ = self.collab_handle(Event::Description(t));
        }
        if let Some(s) = text("status").and_then(|s| STATUSES.iter().find(|x| x.eq_ignore_ascii_case(&s)).copied()) {
            let _ = self.collab_handle(Event::Status(s));
        }
        if let Some(s) = text("priority").and_then(|s| PRIORITIES.iter().find(|x| x.eq_ignore_ascii_case(&s)).copied()) {
            let _ = self.collab_handle(Event::Priority(s));
        }
        if let Some(s) = text("type").and_then(|s| KINDS.iter().find(|x| x.eq_ignore_ascii_case(&s)).copied()) {
            let _ = self.collab_handle(Event::KindOf(s));
        }
        if let Some(t) = text("assigned_to") {
            let _ = self.collab_handle(Event::Assigned(t));
        }
        if let Some(t) = text("due_date") {
            let _ = self.collab_handle(Event::Due(t));
        }
        if let Some(list) = a.get("labels") {
            let joined = match list {
                serde_json::Value::Array(l) => l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(","),
                other => other.as_str().unwrap_or_default().to_string(),
            };
            let _ = self.collab_handle(Event::Labels(joined));
        }
    }
}

/// A status or a priority in a list to choose from: as BCF names it, and as the interface says it.
#[derive(Debug, Clone, PartialEq)]
struct Label(&'static str, String);

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// A rule set in the list to choose from.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice(usize, String);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn status_text(status: ValidationStatus, l: Language) -> &'static str {
    match status {
        ValidationStatus::Passed => l.text("Voldoet", "Passed"),
        ValidationStatus::Failed => l.text("Voldoet niet", "Failed"),
        ValidationStatus::NotCheckable => l.text("Niet te controleren", "Not checkable"),
    }
}

/// How a specification came out; a failed one that only warns reads as a warning.
fn spec_text(spec: &SpecificationResult, l: Language) -> &'static str {
    if spec.status == ValidationStatus::Failed && spec.severity == Severity::Warning {
        return l.text("Waarschuwing", "Warning");
    }
    status_text(spec.status, l)
}

fn spec_tone(status: ValidationStatus, severity: Severity) -> Option<Tone> {
    Some(match (status, severity) {
        (ValidationStatus::Passed, _) => Tone::Success,
        (ValidationStatus::Failed, Severity::Warning) => Tone::Warning,
        (ValidationStatus::Failed, _) => Tone::Error,
        (ValidationStatus::NotCheckable, _) => Tone::Info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;

    /// Two boxes as elements 1 and 2 of one layer, the second sunk into the first.
    fn two_blocks() -> Model {
        let mut layer = Layer { name: "Walls".into(), color: [0.8; 3], positions: Vec::new(), normals: Vec::new(), colors: None, indices: Vec::new(), elements: Vec::new(), props: Vec::new(), hidden: false, points: false };
        for (id, lo, hi) in [(1u64, [0.0f32, 0.0, 0.0], [4.0f32, 0.2, 3.0]), (2, [1.0, -1.0, 1.0], [1.3, 1.0, 1.3])] {
            let first = layer.indices.len() as u32;
            let base = (layer.positions.len() / 3) as u32;
            for i in 0..8 {
                layer.positions.extend([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]);
                layer.normals.extend([0.0, 0.0, 1.0]);
            }
            for quad in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
                layer.indices.extend([quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]].map(|i: u32| base + i));
            }
            layer.elements.push((id, first, 36));
            let class = if id == 1 { "IfcWall" } else { "IfcPipeSegment" };
            layer.props.push((id, format!("{{\"IfcType\":\"{class}\",\"Name\":\"E{id}\",\"GlobalId\":\"guid-{id}\"}}")));
        }
        Model::new(PathBuf::from("house.ifc"), "IFC", vec![layer])
    }

    #[test]
    fn two_versions_tell_what_was_added_removed_and_changed() {
        let old = two_blocks();
        // the new version: the pipe renamed and moved, the wall as it was, and a third element
        let mut new = two_blocks();
        new.path = PathBuf::from("house-2.ifc");
        let renamed = new.props[&2].replace("\"E2\"", "\"E2 moved\"");
        new.props.insert(2, renamed);
        for p in new.layers[0].positions.chunks_exact_mut(3).skip(8) {
            p[0] += 0.5;
        }
        new.props.insert(3, "{\"IfcType\":\"IfcSlab\",\"Name\":\"E3\",\"GlobalId\":\"guid-3\"}".into());
        new.props.insert(1, "{\"IfcType\":\"IfcWall\",\"Name\":\"E1\",\"GlobalId\":\"guid-1\",\"ExpressId\":\"99\"}".into());
        let both = Model::combine(&[Arc::new(old), Arc::new(new)]);
        let c = compare_models(&both, 0, 1);
        assert_eq!((c.added.len(), c.removed.len(), c.counts), (1, 0, (2, 3)), "{c:?}");
        assert_eq!(c.added[0].1, "E3 (IfcSlab)");
        // the pipe changed in name and place; the wall's new express id is no change
        assert_eq!(c.changed.len(), 1, "{c:?}");
        assert!(c.changed[0].2.contains("geometry") && c.changed[0].2.contains("Name"), "{c:?}");
    }

    fn row(rule: &str, a: u64, b: u64, at: [f32; 3]) -> ClashRow {
        ClashRow { rule: rule.into(), a, b, a_name: format!("E{a}"), b_name: format!("E{b}"), kind: Kind::Crossing, at, distance: 0.01, decision: Decision::Open, group: 0, copy: false, key: String::new(), fresh: false, reach: Reach::Partly }
    }

    #[test]
    fn clashes_group_by_the_element_they_share_or_by_where_they_are() {
        // a duct (9) through three beams, a pipe (7) through one of them, and another test
        let found = vec![row("S×I", 1, 9, [0.0; 3]), row("S×I", 7, 2, [40.0, 0.0, 0.0]), row("other", 1, 9, [0.0; 3]), row("S×I", 2, 9, [1.0, 0.0, 0.0]), row("S×I", 3, 9, [2.0, 0.0, 0.0])];
        let mut rows = found.clone();
        group_clashes(&mut rows, Grouping::Element);
        let seen: Vec<(&str, u64, u64, usize)> = rows.iter().map(|r| (r.rule.as_str(), r.a, r.b, r.group)).collect();
        // the duct's three clashes together, then the pipe's (its beam clashes twice, the pipe once,
        // but the duct took that beam's other clash), each test apart, in their order
        assert_eq!(seen, [("S×I", 1, 9, 0), ("S×I", 2, 9, 0), ("S×I", 3, 9, 0), ("S×I", 7, 2, 1), ("other", 1, 9, 2)]);
        let title = group_title(&rows[..3].iter().collect::<Vec<_>>());
        assert_eq!(title, "E9");
        // by place: the three near each other, the one far away apart
        let mut rows = found.clone();
        group_clashes(&mut rows, Grouping::Place);
        assert_eq!(rows.iter().map(|r| r.group).collect::<Vec<_>>(), [0, 0, 0, 1, 2]);
        // not grouped: each on its own, the tests still together
        let mut rows = found;
        group_clashes(&mut rows, Grouping::Off);
        assert_eq!(rows.iter().map(|r| (r.a, r.group)).collect::<Vec<_>>(), [(1, 0), (7, 1), (2, 2), (3, 3), (1, 4)]);
    }

    #[test]
    fn a_group_decides_and_makes_one_issue_and_a_pair_two_tests_find_is_one_clash() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::Tab(Tab::Clashes));
        // two tests that find the same pair: one clash, with the first
        let model = app.model.clone().unwrap();
        let models = vec![("house.ifc".to_string(), 0..1)];
        let all = ClashRule { name: "all".into(), ..ClashRule::default() };
        let walls = ClashRule { name: "walls".into(), a: Selector::classes(&["IfcWall"]), ..ClashRule::default() };
        let found = run_clashes(&model, &models, &[all, walls], &AtomicBool::new(false));
        assert_eq!(found.iter().map(|c| c.rule.as_str()).collect::<Vec<_>>(), ["all"]);
        assert!(!found[0].copy);
        app.collab.clashes = vec![row("t", 1, 2, [0.0; 3]), row("t", 1, 3, [0.5, 0.0, 0.0]), row("t", 4, 5, [9.0, 0.0, 0.0])];
        let _ = app.collab_update(Event::GroupBy(GroupChoice(Grouping::Element, String::new())));
        let _ = app.collab_update(Event::DecideGroup(0, Decision::Accepted));
        assert_eq!(app.collab.clashes.iter().map(|c| c.decision).collect::<Vec<_>>(), [Decision::Accepted, Decision::Accepted, Decision::Open]);
        let _ = app.collab_update(Event::DecideGroup(0, Decision::Open));
        let before = app.collab.issues.topics.len();
        let _ = app.collab_update(Event::IssuesFromClashes);
        // one issue for the group of two, one for the clash on its own
        assert_eq!(app.collab.issues.topics.len(), before + 2);
        let group = &app.collab.issues.topics[before];
        assert!(group.title.contains("(2 clashes)") && group.description.lines().count() == 2, "{} / {}", group.title, group.description);
    }

    #[test]
    fn the_model_colours_by_a_property_keeps_sets_and_makes_a_takeoff() {
        use super::super::inform::{Event as I, CLASS};
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::Tab(Tab::Colours));
        let class = app.collab.inform.keys.iter().find(|k| k.key == CLASS).cloned().unwrap();
        let _ = app.collab_update(Event::Inform(I::ColourBy(class)));
        let legend: Vec<(String, usize)> = app.collab.inform.legend.iter().map(|r| (r.value.clone(), r.ids.len())).collect();
        assert_eq!(legend.len(), 2, "{legend:?}");
        assert_eq!(app.collab.inform.painted.len(), 2);
        assert!(app.unit_looks.iter().skip(1).any(|l| l[0] > 0.0), "the units take the colours");
        // a value of the legend as a set, and the selection as another
        let _ = app.collab_update(Event::Inform(I::LegendSet(0)));
        let _ = app.collab_update(Event::Inform(I::LegendPick(1)));
        let _ = app.collab_update(Event::Inform(I::SetName("Pick".into())));
        let _ = app.collab_update(Event::Inform(I::SaveSet));
        let sets = app.collab.rules().unwrap().selection_sets.clone();
        assert_eq!(sets.len(), 2);
        assert_eq!(app.set_elements(&sets[0]), app.collab.inform.legend[0].ids);
        assert_eq!((sets[1].name.as_str(), app.set_elements(&sets[1])), ("Pick", app.collab.inform.legend[1].ids.clone()));
        // off: as it was
        let _ = app.collab_update(Event::Inform(I::ColourBy(app.collab.inform.keys[0].clone())));
        assert!(app.collab.inform.painted.is_empty());
        // a takeoff of all, by class: a row per class and a total of the two
        let _ = app.collab_update(Event::Inform(I::TakeoffGroup(0, app.collab.inform.keys.iter().find(|k| k.key == CLASS).cloned().unwrap())));
        let takeoff = app.collab.inform.draft.clone();
        let table = app.takeoff_schedule(&takeoff).unwrap();
        assert_eq!(table.rows.len(), 3);
        let total = table.rows.last().unwrap();
        assert!(total.total && total.cells[1].as_deref() == Some("2"), "{:?}", total.cells);
        // the wall: 4 × 0.2 × 3 m, its side 12 m², its volume 2.4 m³
        let wall = table.rows.iter().find(|r| r.cells[0].as_deref() == Some("IfcWall")).unwrap();
        assert_eq!(wall.cells[2..].iter().map(|c| c.clone().unwrap()).collect::<Vec<_>>(), ["4.00", "12.00", "2.400"]);
        let _ = app.collab_update(Event::Inform(I::MakeTakeoff));
        assert!(app.schedules.iter().any(|s| s.computed && s.columns[0] == "Class"));
    }

    #[test]
    fn the_report_holds_the_clashes_as_pdf_and_as_a_workbook() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let model = app.model.clone().unwrap();
        let models = vec![("house.ifc".to_string(), 0..1)];
        app.collab.clashes = run_clashes(&model, &models, &[ClashRule { name: "all".into(), ..ClashRule::default() }], &AtomicBool::new(false));
        group_clashes(&mut app.collab.clashes, Grouping::Element);
        use super::super::reports::Scope;
        use crate::export::report::{Block, Jpeg};
        // the group pictured: a picture of it, and its clash in a table under it
        assert_eq!(app.report_pictured(), [0]);
        let picture = Jpeg { bytes: vec![0xFF, 0xD8, 0xFF, 0xD9], width: 64, height: 48 };
        let pictures = HashMap::from([(0, picture.clone())]);
        let blocks = app.report_blocks(Scope::All, Language::English, &pictures, &HashMap::new());
        assert!(blocks.iter().any(|b| matches!(b, Block::Picture { .. })));
        assert!(blocks.iter().any(|b| matches!(b, Block::Table { rows, .. } if rows.iter().any(|r| r.cells.iter().any(|c| c == "E1 (IfcWall)")))));
        let cover = app.report_cover(Scope::All, Language::English, Some(picture));
        assert!(cover.title == "Check and clash report" && cover.facts.iter().any(|(k, v)| k == "Clashes" && v.starts_with("1 in 1")), "{cover:?}");
        let pdf = crate::export::report::report_pdf(&cover, &app.report_title(Scope::All, Language::English), &app.report_date(), &blocks);
        let text: String = pdf.iter().map(|&b| b as char).collect();
        assert!(text.starts_with("%PDF-1.4") && text.contains("/Im2 ") && text.contains("/DCTDecode"));
        let sheets = app.report_sheets(Language::English);
        assert_eq!(sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Rules", "Findings", "Clash groups", "Clashes", "Issues"]);
        assert_eq!(sheets[3].rows.len(), 1);
        let book = crate::export::xlsx::workbook(&sheets.iter().collect::<Vec<_>>()).unwrap();
        assert!(book.starts_with(b"PK"));
    }

    #[test]
    fn a_second_run_keeps_the_decisions_and_tells_what_is_new_and_what_is_solved() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::Tab(Tab::Clashes));
        // the first run: one clash, accepted
        let mut first = vec![row("t", 1, 2, [0.0; 3])];
        app.collab_take_clashes(&mut first);
        assert!(!app.collab.compared && !first[0].fresh && first[0].key == "guid-1|guid-2", "{:?}", first[0].key);
        app.collab.clashes = first;
        let _ = app.collab_update(Event::Decide(0, Decision::Accepted));
        // the second: that one again (the other way round) and one more
        app.collab_remember();
        let mut second = vec![row("t", 2, 1, [0.0; 3]), row("t", 1, 7, [1.0, 0.0, 0.0])];
        app.collab_take_clashes(&mut second);
        assert_eq!(second.iter().map(|c| (c.decision, c.fresh)).collect::<Vec<_>>(), [(Decision::Accepted, false), (Decision::Open, true)]);
        assert!(app.collab.compared && app.collab.resolved.is_empty());
        app.collab.clashes = second;
        // the third finds only the new one: the first is solved
        app.collab_remember();
        let mut third = vec![row("t", 1, 7, [1.0, 0.0, 0.0])];
        app.collab_take_clashes(&mut third);
        assert_eq!((third[0].fresh, app.collab.resolved.len(), app.collab.resolved[0].a.as_str()), (false, 1, "E2"));
        let runs = app.collab.rules().unwrap().clash_runs.clone();
        assert_eq!(runs.iter().map(|r| (r.total, r.new, r.resolved, r.open)).collect::<Vec<_>>(), [(1, 0, 0, 1), (2, 1, 0, 1), (1, 0, 1, 1)]);
    }

    #[test]
    fn a_clash_scrapes_goes_partly_in_or_goes_through() {
        let b = |lo: [f32; 3], hi: [f32; 3]| (Vec3::from_array(lo), Vec3::from_array(hi));
        let wall = b([0.0, 0.0, 0.0], [5.0, 0.2, 3.0]);
        // a pipe through the wall, one ending in it, one that only scrapes it
        assert_eq!(Reach::of(Kind::Crossing, 0.05, b([1.0, -1.0, 1.0], [1.1, 1.0, 1.1]), wall), Reach::Through);
        assert_eq!(Reach::of(Kind::Crossing, 0.05, b([1.0, -1.0, 1.0], [1.1, 0.1, 1.1]), wall), Reach::Partly);
        assert_eq!(Reach::of(Kind::Crossing, 0.004, b([1.0, -1.0, 1.0], [1.1, 0.004, 1.1]), wall), Reach::Graze);
        assert_eq!((Reach::of(Kind::Near, 0.03, wall, wall), Reach::of(Kind::Inside, 0.1, wall, wall)), (Reach::Near, Reach::Through));
        assert!(Reach::Through > Reach::Partly && Reach::Partly > Reach::Graze);
        // the keys step through the groups and decide on the one open
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        app.collab.clashes = vec![row("t", 1, 2, [0.0; 3]), row("t", 1, 3, [0.5, 0.0, 0.0]), row("t", 4, 5, [9.0, 0.0, 0.0])];
        let _ = app.collab_update(Event::GroupBy(GroupChoice(Grouping::Element, String::new())));
        let _ = app.collab_update(Event::Step(1));
        assert_eq!(app.collab.open_group, Some(0));
        let _ = app.collab_update(Event::DecidePicked(Decision::Accepted));
        assert_eq!(app.collab.clashes.iter().map(|c| c.decision).collect::<Vec<_>>(), [Decision::Accepted, Decision::Accepted, Decision::Open]);
        let _ = app.collab_update(Event::Step(1));
        assert_eq!(app.collab.open_group, Some(1));
        let _ = app.collab_update(Event::Step(-1));
        assert_eq!(app.collab.open_group, Some(0));
    }

    #[test]
    fn a_sign_behind_a_wall_is_out_of_sight() {
        // a room of 8 × 6 m; a wall across it at x = 4; a sign at x = 7 on the far side wall
        let mut layer = Layer { name: "L".into(), color: [0.8; 3], positions: Vec::new(), normals: Vec::new(), colors: None, indices: Vec::new(), elements: Vec::new(), props: Vec::new(), hidden: false, points: false };
        let boxes: [(u64, &str, &str, [f32; 3], [f32; 3]); 3] = [(1, "IfcSpace", "0.01", [0.0, 0.0, 0.0], [8.0, 6.0, 3.0]), (2, "IfcWall", "Wand", [4.0, 0.0, 0.0], [4.1, 6.0, 3.0]), (3, "IfcBuildingElementProxy", "Vluchtwegbord", [7.9, 2.8, 2.2], [7.95, 3.2, 2.4])];
        for (id, class, name, lo, hi) in boxes {
            let first = layer.indices.len() as u32;
            let base = (layer.positions.len() / 3) as u32;
            for i in 0..8 {
                layer.positions.extend([if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]);
                layer.normals.extend([0.0, 0.0, 1.0]);
            }
            for q in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
                layer.indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]].map(|i: u32| base + i));
            }
            layer.elements.push((id, first, layer.indices.len() as u32 - first));
            layer.props.push((id, format!("{{\"IfcType\":\"{class}\",\"Name\":\"{name}\",\"GlobalId\":\"g{id}\"}}")));
        }
        let model = Model::new(PathBuf::from("room.ifc"), "IFC", vec![layer]);
        let models = vec![("room.ifc".to_string(), 0..1)];
        let rule = ClashRule { name: "signs".into(), kind: ClashKind::Sight, sight_names: "*exit*, *vlucht*".into(), sight_blocked_pct: 40.0, ..ClashRule::default() };
        let found = run_clashes(&model, &models, std::slice::from_ref(&rule), &AtomicBool::new(false));
        // from the half of the room before the wall the sign is not seen
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!((found[0].a, found[0].b, found[0].kind), (3, 2, Kind::Hidden));
        assert!((found[0].distance - 0.5).abs() < 0.01, "{}", found[0].distance);
        // half may be without sight of it: then it passes; and without the wall too
        let lenient = ClashRule { sight_blocked_pct: 60.0, ..rule.clone() };
        assert!(run_clashes(&model, &models, &[lenient], &AtomicBool::new(false)).is_empty());
    }

    #[test]
    fn the_matrix_is_edited_in_the_panel() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::RuleSet(3));
        let count = |app: &App| app.collab.rules().unwrap().disciplines.len();
        let n = count(&app);
        let _ = app.collab_update(Event::AddDiscipline);
        assert_eq!(count(&app), n + 1);
        let _ = app.collab_update(Event::DisciplineName(n, "Gevel".into()));
        let _ = app.collab_update(Event::MatrixCell(n, 0));
        let first = app.collab.rules().unwrap().disciplines[0].name.clone();
        assert!(app.collab.rules().unwrap().in_matrix("Gevel", &first));
        // renamed, its pairs go with it; removed, they go
        let _ = app.collab_update(Event::DisciplineName(n, "Façade".into()));
        assert!(app.collab.rules().unwrap().in_matrix("Façade", &first));
        let _ = app.collab_update(Event::MatrixAmount("12,5".into()));
        assert_eq!(app.collab.rules().unwrap().matrix_tolerance_mm, 12.5);
        let _ = app.collab_update(Event::RemoveDiscipline(n));
        assert!(!app.collab.rules().unwrap().matrix.iter().any(|p| p.contains(&"Façade".to_string())));
    }

    #[test]
    fn a_field_reads_from_flat_properties() {
        let props = r#"{"IfcType":"IfcWall","Name":"W \"1\"","GlobalId":"0LV8Pid0X3IA3jJLVDPidY"}"#;
        assert_eq!((field(props, "GlobalId"), field(props, "IfcType"), field(props, "Name"), field(props, "Tag")), (Some("0LV8Pid0X3IA3jJLVDPidY"), Some("IfcWall"), Some(r#"W \"1\""#), None));
    }

    #[test]
    fn a_rule_finds_the_pipe_through_the_wall_by_class() {
        let model = two_blocks();
        let models = vec![("house.ifc".to_string(), 0..1)];
        let rule = |a: &[&str], b: &[&str]| ClashRule { name: "test".into(), a: Selector::classes(a), b: Selector::classes(b), ..ClashRule::default() };
        let found = run_clashes(&model, &models, &[rule(&["IfcWall*"], &["IfcPipe*"])], &AtomicBool::new(false));
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].a, found[0].b, found[0].kind, found[0].a_name.as_str()), (1, 2, Kind::Crossing, "E1 (IfcWall)"));
        // all against all: the pair once; walls against walls: nothing
        assert_eq!(run_clashes(&model, &models, &[rule(&[], &[])], &AtomicBool::new(false)).len(), 1);
        assert!(run_clashes(&model, &models, &[rule(&["IfcWall*"], &["IfcWall*"])], &AtomicBool::new(false)).is_empty());
        // only between models: in one model the test is as it was; with the wall and the pipe in
        // one of two models, they are left alone
        let between = ClashRule { between_models: true, ..rule(&[], &[]) };
        assert_eq!(run_clashes(&model, &models, std::slice::from_ref(&between), &AtomicBool::new(false)).len(), 1);
        let two = vec![("house.ifc".to_string(), 0..model.layers.len()), ("pipes.ifc".to_string(), model.layers.len()..model.layers.len() + 1)];
        assert!(run_clashes(&model, &two, &[between], &AtomicBool::new(false)).is_empty());
        assert_eq!(run_clashes(&model, &two, &[rule(&[], &[])], &AtomicBool::new(false)).len(), 1);
    }

    #[test]
    fn the_selection_clashes_against_the_rest_and_a_side_is_chosen_from_the_model() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::Tab(Tab::Clashes));
        // the sides: everything, the two groups, the layer and the two classes
        let labels: Vec<String> = app.collab.sides.iter().map(|s| s.to_string()).collect();
        assert_eq!(labels, ["Everything", "Structure", "Services", "Layer: Walls", "Class: IfcPipeSegment", "Class: IfcWall"]);
        let pipes = app.collab.sides[4].clone();
        let _ = app.collab_update(Event::ClashSide(0, true, pipes));
        let rule = app.collab.rules().unwrap().clashes[0].clone();
        assert_eq!((rule.name.as_str(), rule.a.classes.clone(), rule.within()), ("IfcPipeSegment × everything", vec!["IfcPipeSegment".to_string()], false));
        // the pipe selected: it clashes with the wall, not with itself
        let model = app.model.clone().unwrap();
        let models = vec![("house.ifc".to_string(), 0..1)];
        let only = ClashRule { name: "selection".into(), a: Selector { only: vec![2], ..Selector::default() }, b: Selector { except: vec![2], ..Selector::default() }, ..ClashRule::default() };
        let found = run_clashes(&model, &models, &[only], &AtomicBool::new(false));
        assert_eq!(found.iter().map(|c| (c.a, c.b)).collect::<Vec<_>>(), [(2, 1)]);
        // nothing selected: the panel says so and runs nothing
        let _ = app.collab_update(Event::ClashSelection);
        assert!(!app.collab.running && app.collab.notice.is_some());
    }

    #[test]
    fn what_is_measured_and_remarked_goes_with_an_issue_and_comes_back() {
        use crate::measure::{geometry::Distance, Measurement};
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        app.measure.results = vec![
            Measurement::Distance(Distance { p1: Vec3::new(0.0, 0.0, 0.0), p2: Vec3::new(3.0, 4.0, 0.0), extensions: Vec::new() }),
            Measurement::Note { at: Vec3::new(1.0, 1.0, 2.0), text: "Te laag".into() },
            Measurement::Mark { kind: crate::measure::MarkKind::Cloud, points: vec![Vec3::ZERO, Vec3::X, Vec3::Y] },
        ];
        let view = app.collab_viewpoint_now();
        assert_eq!((view.lines.len(), view.notes.len()), (1, 1));
        assert_eq!(view.notes[0].1, "Te laag");
        let mut topic = Topic::new("Maat", "an");
        topic.viewpoints.push(view);
        app.collab.issues.topics.push(topic.clone());
        // measured away, and the issue shown again: both stand where they stood
        app.measure.results.clear();
        let _ = app.collab_update(Event::Topic(Some(topic.guid.clone())));
        let back = &app.measure.results;
        assert_eq!(back.len(), 3);
        assert!(matches!(&back[2], Measurement::Mark { kind: crate::measure::MarkKind::Cloud, points } if points.len() == 3), "{back:?}");
        assert!(matches!(&back[0], Measurement::Distance(d) if d.p1.distance(Vec3::ZERO) < 1e-4 && d.p2.distance(Vec3::new(3.0, 4.0, 0.0)) < 1e-4), "{back:?}");
        assert!(matches!(&back[1], Measurement::Note { at, text } if at.distance(Vec3::new(1.0, 1.0, 2.0)) < 1e-4 && text == "Te laag"), "{back:?}");
        // an issue without any leaves what is measured alone
        let plain = Topic::new("Zonder", "an");
        app.collab.issues.topics.push(plain.clone());
        let _ = app.collab_update(Event::Topic(Some(plain.guid.clone())));
        assert_eq!(app.measure.results.len(), 3);
    }

    #[test]
    fn an_issue_of_the_view_goes_back_to_it() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        // look from the south, the pipe selected and the wall hidden
        app.camera.yaw = 1.0;
        app.camera.pitch = 0.3;
        app.camera.distance = 9.0;
        app.collab_select([2]);
        let view = app.collab_viewpoint_now();
        assert_eq!(view.selection, ["guid-2"]);
        let (eye, yaw, pitch) = (app.camera.eye(), app.camera.yaw, app.camera.pitch);
        let mut topic = Topic::new("Pipe", "an");
        topic.viewpoints.push(view);
        app.collab.issues.topics.push(topic.clone());
        // elsewhere, nothing selected
        app.camera.yaw = -2.0;
        app.camera.distance = 30.0;
        app.collab_select([]);
        let _ = app.collab_update(Event::Topic(Some(topic.guid.clone())));
        let to = app.anim.expect("the camera moves").1;
        assert!((to.yaw - yaw).abs() < 1e-3 && (to.pitch - pitch).abs() < 1e-3, "{to:?}");
        assert!(to.eye().distance(eye) < 1e-2, "{:?} {eye:?}", to.eye());
        assert_eq!(app.sel.items(), [Pickable::Element(2)]);
    }

    #[test]
    fn clashes_become_issues_that_select_both_elements() {
        let mut app = App::default();
        app.set_model(Arc::new(two_blocks()));
        let _ = app.collab_update(Event::Tab(Tab::Clashes));
        let models = vec![("house.ifc".to_string(), 0..1)];
        let rules = app.collab.rules().unwrap().clashes.clone();
        let rows = run_clashes(app.model.as_ref().unwrap(), &models, &rules, &AtomicBool::new(false));
        let run = app.collab.run;
        let _ = app.collab_update(Event::Clashed(run, rows));
        assert_eq!(app.collab.clashes.len(), 1);
        let _ = app.collab_update(Event::PickClash(0));
        assert_eq!(app.sel.len(), 2);
        let _ = app.collab_update(Event::IssuesFromClashes);
        let topic = &app.collab.issues.topics[0];
        assert_eq!((topic.kind.as_str(), topic.viewpoints[0].selection.len()), ("Clash", 2));
        // written and read again: the same issue
        let again = Issues::read(&app.collab.issues.write().unwrap()).unwrap();
        assert_eq!(again.topics[0].viewpoints, topic.viewpoints);
    }
}

#[cfg(test)]
mod timing {
    use super::*;

    /// Where the time goes when models are added one after the other (`O3V_MODELS_DIR`: a folder
    /// of IFC files; skipped without it): `cargo test --lib timing -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a folder of models"]
    fn adding_models_one_after_the_other() {
        let Some(dir) = std::env::var_os("O3V_MODELS_DIR").map(PathBuf::from) else { return };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(|e| Some(e.ok()?.path())).filter(|p| p.to_string_lossy().ends_with("-IFC4.ifc")).collect();
        files.sort();
        files.truncate(std::env::var("O3V_MODELS").ok().and_then(|n| n.parse().ok()).unwrap_or(10));
        let mut app = App::default();
        let mut total = std::time::Duration::ZERO;
        for file in &files {
            let t = Instant::now();
            let model = Arc::new(crate::loaders::load(file).unwrap());
            let read = t.elapsed();
            let mut sources: Vec<Arc<Model>> = app.models.iter().map(|e| e.model.clone()).collect();
            if sources.is_empty() {
                sources.extend(app.model.clone());
            }
            sources.push(model.clone());
            let t = Instant::now();
            let combined = Model::combine(&sources);
            let combine = t.elapsed();
            let t = Instant::now();
            let _ = crate::explode::ExplodeIndex::new(&combined);
            let index = t.elapsed();
            let t = Instant::now();
            app.add_model(model);
            let add = t.elapsed();
            total += read + add;
            eprintln!("{:>5.2} read {:>5.2} combine {:>5.2} explode index {:>5.2} add_model (all)  {} triangles  {}", read.as_secs_f32(), combine.as_secs_f32(), index.as_secs_f32(), add.as_secs_f32(), combined.triangle_count(), file.file_name().unwrap().to_string_lossy());
        }
        // the same files added at once: read one after the other, the scene built once
        let mut at_once = App::default();
        let t = Instant::now();
        let sources: Vec<crate::loaders::Source> = files.iter().cloned().map(crate::loaders::Source::File).collect();
        let mut message = Some(Message::AddSeveral(sources[1..].to_vec()));
        at_once.set_model(Arc::new(crate::loaders::load(&files[0]).unwrap()));
        let _ = at_once.update(message.take().unwrap());
        while at_once.adding_model {
            let next = at_once.loading_stack.0.last().cloned().unwrap();
            let crate::loaders::Source::File(path) = next else { break };
            let _ = at_once.update(Message::Loaded(crate::loaders::load(&path).map(Arc::new)));
        }
        eprintln!("one after the other: {:.1} s; at once: {:.1} s, {} models, {} triangles", total.as_secs_f32(), t.elapsed().as_secs_f32(), at_once.models.len(), at_once.model.as_ref().map_or(0, |m| m.triangle_count()));
        assert_eq!(at_once.models.len(), files.len());
        assert_eq!(at_once.model.as_ref().map(|m| m.triangle_count()), app.model.as_ref().map(|m| m.triangle_count()));
    }
}
