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
fn user_file() -> Option<PathBuf> {
    Some(crate::remote_link::settings_dir()?.join("collaborator-user.txt"))
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

    fn rules_mut(&mut self) -> Option<&mut RuleSet> {
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
        let amounts: Vec<f32> = self.rules().map(|r| r.clashes.iter().map(|c| if c.kind == ClashKind::Hard { c.tolerance_mm } else { c.clearance_mm }).collect()).unwrap_or_default();
        // what is typed stays as typed while it says the same amount
        let same = |typed: &String, amount: f32| typed.trim().replace(',', ".").parse::<f32>().ok() == Some(amount);
        if amounts.len() != self.amounts.len() || !self.amounts.iter().zip(&amounts).all(|(t, a)| same(t, *a) || t.trim().is_empty() || t.ends_with(['.', ','])) {
            self.amounts = amounts.iter().map(|a| a.to_string()).collect();
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
    IssueFromClash(usize),
    IssuesFromClashes,
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
fn field<'a>(props: &'a str, key: &str) -> Option<&'a str> {
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
/// clashes found, each pair once.
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
    let mut rows: Vec<ClashRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for rule in rules.iter().filter(|r| r.on) {
        let side = |s: &Selector| -> Vec<usize> {
            let takes = |item: &Item| s.takes_element(item.id, &models[item.model.min(models.len().saturating_sub(1))].0, &model.layers[item.layer].name, &item.class);
            (0..items.len()).filter(|&k| models.is_empty() || takes(&items[k])).collect()
        };
        let (a, b) = (side(&rule.a), side(&rule.b));
        let solids = |side: &[usize]| side.iter().map(|&k| items[k].solid.clone()).collect::<Vec<_>>();
        let (sa, sb) = (solids(&a), if rule.within() { Vec::new() } else { solids(&b) });
        // a test between models leaves the pairs of one model out, when there is more than one
        let apart = rule.between_models && models.len() > 1;
        let skip = |i: usize, j: usize| apart && items[a[i]].model == items[b[j]].model;
        let found = clash::clashes_but(&sa, if rule.within() { &sa } else { &sb }, rule.within(), rule.test(), &skip, cancel);
        for c in found {
            let (ia, ib) = (&items[a[c.a]], &items[b[c.b]]);
            if ia.id == ib.id || !seen.insert((rule.name.clone(), ia.id.min(ib.id), ia.id.max(ib.id))) {
                continue;
            }
            rows.push(ClashRow { rule: rule.name.clone(), a: ia.id, b: ib.id, a_name: element_name(model, ia.id), b_name: element_name(model, ib.id), kind: c.kind, at: c.at.to_array(), distance: c.distance, decision: Decision::Open });
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
        let options = ValidationOptions { ids_xml, model_checks: first && rules.model_checks, ils: ils.clone().filter(|_| first) };
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

    fn collab_guid_of(&self, id: u64) -> Option<String> {
        self.model.as_ref()?.props.get(&id).and_then(|p| field(p, "GlobalId")).map(str::to_string)
    }

    /// The model's place in the world: BCF's coordinates are the model's own, in metres.
    fn collab_to_display(&self, world: [f64; 3]) -> Vec3 {
        let world = DVec3::from_array(world);
        self.model.as_ref().and_then(|m| m.origins.first().copied()).map_or(world, |o| o.display(world)).as_vec3()
    }

    fn collab_to_world(&self, display: Vec3) -> [f64; 3] {
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
        let (Some(model), Some(clash)) = (self.model.as_ref(), self.collab.clash.and_then(|k| self.collab.clashes.get(k))) else { return Vec::new() };
        let matrix = self.model_matrix();
        let world = |p: Vec3| matrix.transform_point3(p).to_array();
        let mut lines = Vec::new();
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
        view
    }

    /// A viewpoint that looks at a place from where the camera looks now, with elements selected.
    fn collab_viewpoint_at(&self, at: Vec3, reach: f32, ids: &[u64]) -> Viewpoint {
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

    fn collab_note(&mut self, tone: Tone, message: impl Into<String>) {
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
        };
        let reach = self.model.as_ref().and_then(|m| Some(m.element_bounds(row.a)?.radius().max(m.element_bounds(row.b)?.radius()))).unwrap_or(2.0);
        topic.viewpoints.push(self.collab_viewpoint_at(Vec3::from_array(row.at), reach.min(6.0), &[row.a, row.b]));
        topic
    }

    pub(super) fn collab_update(&mut self, event: Event) -> Task<Message> {
        self.collab.refresh();
        if let Event::ClashAmount(k, typed) = &event {
            if let Some(kept) = self.collab.amounts.get_mut(*k) {
                *kept = typed.clone();
            }
        }
        let task = self.collab_handle(event);
        self.collab.refresh();
        self.collab_refresh_sides();
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
            Event::SaveIssuesTo(Some(path)) => match self.collab.issues.write().and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string())) {
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
            Event::Checked(run, checked) => {
                if run == self.collab.run {
                    self.collab.running = false;
                    self.collab.checked = checked;
                }
            }
            Event::Spec(open) => self.collab.open_spec = open,
            Event::Finding(checked, express_id) => {
                let Some(model) = self.model.clone() else { return Task::none() };
                let which = self.collab.checked.get(checked).map_or(0, |c| c.model) as u32;
                match model.element_with_key(crate::model::ElementKey { model: which, source: u64::from(express_id) }) {
                    Some(id) if !model.element_ranges(id).is_empty() => self.collab_go_to([id]),
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
                };
                return ask_file(move || rfd::AsyncFileDialog::new().add_filter(extension, &[extension]).set_file_name(format!("{name}.{extension}")), true, move |path| Message::Collab(Event::ExportTo(kind, path)));
            }
            Event::ExportTo(kind, Some(path)) => {
                let reports = || self.collab.checked.iter().filter_map(|c| Some((c.name.as_str(), c.report.as_ref().ok()?)));
                let body = match kind {
                    Report::Csv => report_csv(&self.collab.checked),
                    Report::Json => serde_json::to_string_pretty(&reports().map(|(name, r)| serde_json::json!({"model": name, "report": **r})).collect::<Vec<_>>()).unwrap_or_default(),
                    Report::Html => {
                        let language = if l == Language::Dutch { bim_validator_core::ReportLanguage::Dutch } else { bim_validator_core::ReportLanguage::English };
                        // one model: its report; more: the reports one after the other
                        reports().map(|(name, r)| bim_validator_core::report_html(r, name, language)).collect::<Vec<_>>().join("\n")
                    }
                    Report::IssuesHtml => issues_html(&self.collab.issues, l),
                    Report::ClashesCsv => {
                        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
                        let mut out = "rule,kind,distance_mm,decision,a,b,a_global_id,b_global_id,x,y,z\r\n".to_string();
                        for c in &self.collab.clashes {
                            let kind = match c.kind {
                                Kind::Crossing => "crossing",
                                Kind::Inside => "inside",
                                Kind::Near => "clearance",
                            };
                            let at = self.collab_to_world(Vec3::from_array(c.at));
                            let guid = |id| self.collab_guid_of(id).unwrap_or_default();
                            out.push_str(&format!("{},{kind},{:.0},{},{},{},{},{},{:.3},{:.3},{:.3}\r\n", quote(&c.rule), c.distance * 1000.0, c.decision.key(), quote(&c.a_name), quote(&c.b_name), guid(c.a), guid(c.b), at[0], at[1], at[2]));
                        }
                        out
                    }
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
                if !rules.clashes.iter().any(|r| r.on) {
                    self.collab_note(Tone::Warning, l.text("De regelset heeft geen clashtest die aan staat.", "The rule set has no clash test switched on."));
                    return Task::none();
                }
                let models: Vec<(String, std::ops::Range<usize>)> = self.models.iter().map(|e| (e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.layers.clone())).collect();
                self.collab.cancel = Arc::new(AtomicBool::new(false));
                self.collab.run += 1;
                self.collab.running = true;
                self.collab.clashes.clear();
                self.collab.clash = None;
                self.collab.notice = None;
                let (run, cancel) = (self.collab.run, self.collab.cancel.clone());
                return job(move || Event::Clashed(run, std::panic::catch_unwind(|| run_clashes(&model, &models, &rules.clashes, &cancel)).unwrap_or_default()));
            }
            Event::Clashed(run, rows) => {
                if run == self.collab.run {
                    self.collab.running = false;
                    if rows.len() >= MOST_CLASHES {
                        self.collab_note(Tone::Warning, format!("{} {MOST_CLASHES} {}", l.text("Alleen de eerste", "Only the first"), l.text("clashes zijn bewaard: maak de test strenger.", "clashes are kept: make the test narrower.")));
                    }
                    self.collab.clashes = rows;
                    self.refresh_overlay();
                }
            }
            Event::PickClash(k) => {
                let Some(row) = self.collab.clashes.get(k).cloned() else { return Task::none() };
                self.collab.clash = Some(k);
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
                let topics: Vec<Topic> = self.collab.clashes.iter().take(1000).map(|row| self.collab_clash_topic(row, l)).collect();
                let made = topics.len();
                self.collab.issues.topics.extend(topics);
                self.collab.issues_changed |= made > 0;
                self.collab.tab = Tab::Issues;
                self.collab_note(Tone::Success, format!("{made} {}", l.text("issues gemaakt", "issues made")));
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
                self.collab.checked = check_files(files.map(|(model, e)| (model, e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.model.path.clone())).collect(), &rules, &cancel)
                    .into_iter()
                    .collect();
                let failed = self.collab.checked.first().and_then(|c| c.report.as_ref().ok()).and_then(|r| r.specifications.iter().position(|s| s.status == ValidationStatus::Failed));
                self.collab.open_spec = failed.map(|s| (0, s));
                self.collab.tab = Tab::Checks;
            }
            Some(what @ ("clash" | "clash-isolate")) => {
                self.collab.isolate = what == "clash-isolate";
                let (model, rules) = (self.model.clone().ok_or("no model")?, self.collab.rules().cloned().ok_or("no rule set")?);
                let models: Vec<(String, std::ops::Range<usize>)> = self.models.iter().map(|e| (e.model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), e.layers.clone())).collect();
                let started = Instant::now();
                self.collab.clashes = run_clashes(&model, &models, &rules.clashes, &AtomicBool::new(false));
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
        ]
        .spacing(2);
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
            Tab::Compare => self.collab_compare(look, l),
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
            w::button(look, l.text("Rapport", "Report"), K::Quiet, D::Compact, None, (!state.issues.topics.is_empty()).then(|| go(Event::ExportIssues))),
        ]
        .spacing(6)
        .wrap()]
        .spacing(6);
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
        if let Some(rules) = state.rules() {
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
                            kind(l.text("Vrije ruimte", "Clearance"), !hard, ClashKind::Clearance),
                            Space::new().width(Length::Fill),
                            container(w::text_field(look, "mm", amount, Some(move |t| go(Event::ClashAmount(k, t))), D::Compact, None)).width(64),
                            w::secondary(look, "mm"),
                        ]
                        .spacing(4)
                        .align_y(Alignment::Center),
                        named(
                            w::checkbox(look, l.text("Alleen tussen modellen", "Only between models"), rule.between_models, Some(move |on| go(Event::ClashBetween(k, on)))),
                            Node::new(Role::CheckBox, format!("{} {}", rule.name, l.text("Alleen tussen modellen", "Only between models"))).checked(rule.between_models),
                        ),
                    ]
                    .spacing(3),
                );
            }
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
                w::button(look, "CSV", K::Quiet, D::Compact, None, Some(go(Event::Export(Report::ClashesCsv)))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        );
        let accepted = state.clashes.iter().filter(|c| c.decision == Decision::Accepted).count();
        if accepted > 0 {
            list = list.push(w::checkbox(look, l.text("Geaccepteerde tonen", "Show accepted"), state.show_accepted, Some(|on| go(Event::ShowAccepted(on)))));
        }
        let mut rule: Option<&str> = None;
        for (k, clash) in state.clashes.iter().enumerate().take(SHOWN) {
            // the clashes of one test under its name
            if rule != Some(clash.rule.as_str()) {
                rule = Some(clash.rule.as_str());
                let of_rule = state.clashes.iter().filter(|c| c.rule == clash.rule).count();
                list = list.push(w::heading(look, format!("{} · {of_rule}", clash.rule), 13.0));
            }
            let picked = state.clash == Some(k);
            let mm = (clash.distance * 1000.0).round();
            let (what, tone) = match clash.kind {
                Kind::Crossing => (format!("{mm} mm"), Tone::Error),
                Kind::Inside => (l.text("ligt erin", "lies inside").to_string(), Tone::Error),
                Kind::Near => (format!("{mm} mm {}", l.text("ertussen", "apart")), Tone::Warning),
            };
            if !state.show_accepted && clash.decision == Decision::Accepted && !picked {
                continue;
            }
            let (what, tone) = match clash.decision {
                Decision::Accepted => (l.text("akkoord", "accepted").to_string(), Tone::Success),
                Decision::Rejected => (format!("{what} · {}", l.text("afgewezen", "rejected")), tone),
                Decision::Open => (what, tone),
            };
            let line = row![column![w::body(look, clash.a_name.clone()), w::secondary(look, format!("× {}", clash.b_name))].spacing(1).width(Length::Fill), site::chip(look, &what, Some(tone))].spacing(6).align_y(Alignment::Center);
            list = list.push(named(button(line).width(Length::Fill).padding([4, 6]).style(move |_, st| p.ribbon_button(picked, st)).on_press(go(Event::PickClash(k))), Node::new(Role::Row, format!("{} {}: {} × {}", l.text("Clash", "Clash"), k + 1, clash.a_name, clash.b_name)).selected(picked).value(clash.decision.key())));
            if picked {
                let decide = |label: &'static str, d: Decision| {
                    let active = clash.decision == d;
                    named(button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(go(Event::Decide(k, if active { Decision::Open } else { d }))), Node::new(Role::ToggleButton, label).selected(active))
                };
                list = list.push(
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
        }
        if state.clashes.len() > SHOWN {
            list = list.push(w::secondary(look, format!("+ {} {}", state.clashes.len() - SHOWN, l.text("meer, zie het CSV-bestand", "more, see the CSV file"))));
        }
        list.into()
    }
}

/// A side of a clash test in a list to choose from: what it is called and what it takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    label: String,
    selector: Selector,
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
                    "issues": self.collab.issues.topics.len(), "open_issues": self.collab.open_issues(),
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
                    Ok(json!({ "clashes": self.collab.clashes.len(), "by_rule": by_rule, "capped": self.collab.clashes.len() >= MOST_CLASHES, "seconds": started.elapsed().as_secs_f32() }))
                }
                "list" => {
                    let offset = a.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let limit = a.get("limit").and_then(|v| v.as_u64()).unwrap_or(50).min(500) as usize;
                    Ok(json!({ "total": self.collab.clashes.len(), "clashes": self.collab.clashes.iter().enumerate().skip(offset).take(limit).map(|(k, c)| json!({
                        "index": k, "rule": c.rule, "a": c.a, "b": c.b, "a_name": c.a_name, "b_name": c.b_name,
                        "kind": format!("{:?}", c.kind), "distance_mm": (c.distance * 1000.0).round(), "decision": c.decision.key(),
                    })).collect::<Vec<_>>() }))
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
                other => Err(format!("collab_clashes: unknown action {other} (run, list, pick, decide, issue, issues)")),
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
