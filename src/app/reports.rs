//! The Collaborator's report of a project: how its models came out of the checks of the rule
//! set, which clashes its tests found (each group with a picture of it) and the issues (BCF)
//! with their pictures, as a PDF in the house style to read and hand over, and as an Excel
//! workbook to sort and filter.
//!
//! The pictures are the 3D view's own: for each group of clashes the view goes to it (its
//! elements selected and framed), one frame is drawn and taken, and the view comes back as it
//! was when the report is written. A group that was accepted as a whole gets no picture.

use super::collab::{group_title, ClashRow, Decision};
use super::App;
use crate::collab::clash::Kind;
use crate::export::report::{Block, Cover, Jpeg, Mark, Row};
use crate::model::{Schedule, ScheduleRow};
use bim_validator_core::{Severity, ValidationStatus as S};
use openaec_ui::Language;
use std::collections::HashMap;

/// The most findings of a rule and groups of clashes the PDF lists; the workbook has all.
const IN_PDF: usize = 60;
/// The most groups of clashes the PDF shows with a picture.
pub const PICTURED: usize = 40;

/// What a report takes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The checks, the clashes and the issues.
    All,
    /// The issues alone.
    Issues,
}

fn mark(status: S, severity: Severity) -> Mark {
    match (status, severity) {
        (S::Passed, _) => Mark::Pass,
        (S::Failed, Severity::Error) => Mark::Fail,
        (S::Failed, Severity::Warning) => Mark::Warning,
        (S::NotCheckable, _) => Mark::Neutral,
    }
}

fn outcome(status: S, severity: Severity, l: Language) -> &'static str {
    match (status, severity) {
        (S::Passed, _) => l.text("Geslaagd", "Passed"),
        (S::Failed, Severity::Error) => l.text("Afgekeurd", "Failed"),
        (S::Failed, Severity::Warning) => l.text("Waarschuwing", "Warning"),
        (S::NotCheckable, _) => l.text("Niet controleerbaar", "Not checkable"),
    }
}

fn kind(c: &ClashRow, l: Language) -> String {
    let mm = (c.distance * 1000.0).round();
    match c.kind {
        _ if c.copy => l.text("dubbel element", "double element").to_string(),
        Kind::Crossing => match c.reach {
            super::collab::Reach::Graze => format!("{} · {mm} mm", l.text("schampt", "grazes")),
            super::collab::Reach::Through => format!("{} · {mm} mm", l.text("erdoorheen", "through")),
            _ => format!("{mm} mm"),
        },
        Kind::Inside => l.text("ligt erin", "lies inside").to_string(),
        Kind::Near => format!("{mm} mm {}", l.text("ertussen", "apart")),
        Kind::Hidden => format!("{:.0}% {}", c.distance * 100.0, l.text("niet te zien", "out of sight")),
    }
}

fn decision(d: Decision, l: Language) -> &'static str {
    match d {
        Decision::Open => l.text("open", "open"),
        Decision::Accepted => l.text("akkoord", "accepted"),
        Decision::Rejected => l.text("afgewezen", "rejected"),
    }
}

/// A picture as JPEG, at most `widest` pixels wide.
#[cfg(not(target_arch = "wasm32"))]
pub fn jpeg(width: u32, height: u32, rgb: Vec<u8>, widest: u32) -> Option<Jpeg> {
    let image = image::RgbImage::from_raw(width, height, rgb)?;
    let image = if width > widest {
        let h = (u64::from(height) * u64::from(widest) / u64::from(width.max(1))).max(1) as u32;
        image::imageops::resize(&image, widest, h, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 82).encode_image(&image).ok()?;
    Some(Jpeg { bytes, width: image.width(), height: image.height() })
}

#[cfg(target_arch = "wasm32")]
pub fn jpeg(_: u32, _: u32, _: Vec<u8>, _: u32) -> Option<Jpeg> {
    None
}

/// An issue's picture (PNG or JPEG) as JPEG.
fn snapshot_jpeg(name: &str, bytes: &[u8]) -> Option<Jpeg> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let size = image::load_from_memory(bytes).ok()?;
            return Some(Jpeg { bytes: bytes.to_vec(), width: size.width(), height: size.height() });
        }
        #[cfg(target_arch = "wasm32")]
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let rgb = image::load_from_memory(bytes).ok()?.to_rgb8();
        let (w, h) = rgb.dimensions();
        jpeg(w, h, rgb.into_raw(), 1400)
    }
    #[cfg(target_arch = "wasm32")]
    None
}

impl App {
    pub(crate) fn report_date(&self) -> String {
        let date = crate::export::ifcx::timestamp();
        date.get(..10).unwrap_or(&date).to_string()
    }

    /// The names of the open models.
    fn report_models(&self) -> Vec<String> {
        self.models.iter().filter_map(|e| e.model.path.file_name().map(|n| n.to_string_lossy().into_owned())).collect()
    }

    /// The clashes by group, each group's rows in order.
    pub(crate) fn report_groups(&self) -> Vec<Vec<&ClashRow>> {
        let mut groups: Vec<Vec<&ClashRow>> = Vec::new();
        for c in &self.collab.clashes {
            match groups.last_mut() {
                Some(last) if last[0].group == c.group => last.push(c),
                _ => groups.push(vec![c]),
            }
        }
        groups
    }

    /// The groups the PDF shows with a picture: the first that are not accepted as a whole.
    pub(crate) fn report_pictured(&self) -> Vec<usize> {
        self.report_groups().iter().filter(|g| !g.iter().all(|c| c.decision == Decision::Accepted)).take(PICTURED).map(|g| g[0].group).collect()
    }

    /// The title of a report.
    pub(crate) fn report_title(&self, scope: Scope, l: Language) -> String {
        match scope {
            Scope::All => l.text("Controle- en clashrapport", "Check and clash report"),
            Scope::Issues => l.text("Issuerapport", "Issue report"),
        }
        .to_string()
    }

    /// The cover: the project, what was checked and by whom, and the picture of the model.
    pub(crate) fn report_cover(&self, scope: Scope, l: Language, picture: Option<Jpeg>) -> Cover {
        let project = self.collab.checked.iter().filter_map(|c| c.report.as_ref().ok()?.model.as_ref()?.project_name.clone()).find(|n| !n.trim().is_empty());
        let models = self.report_models();
        let subtitle = project.clone().unwrap_or_else(|| models.join(", "));
        let mut facts = vec![(l.text("Modellen", "Models").to_string(), models.join(", "))];
        if scope == Scope::All {
            facts.push((l.text("Regelset", "Rule set").to_string(), self.collab.rules().map(|r| r.name.clone()).unwrap_or_default()));
            let groups = self.report_groups().len();
            facts.push((l.text("Clashes", "Clashes").to_string(), format!("{} {} {groups} {}", self.collab.clashes.len(), l.text("in", "in"), l.text("groepen", "groups"))));
        }
        facts.push((l.text("Issues", "Issues").to_string(), format!("{} · {} {}", self.collab.issues.topics.len(), self.collab.open_issues(), l.text("open", "open"))));
        facts.push((l.text("Opgesteld door", "Made by").to_string(), self.collab.author.clone()));
        facts.push((l.text("Datum", "Date").to_string(), self.report_date()));
        Cover { title: self.report_title(scope, l), subtitle, facts, picture }
    }

    /// The report as blocks for the PDF, with the pictures taken of the groups of clashes.
    pub(crate) fn report_blocks(&self, scope: Scope, l: Language, pictures: &HashMap<usize, Jpeg>, issue_pictures: &HashMap<usize, Jpeg>) -> Vec<Block> {
        let mut blocks = Vec::new();
        if scope == Scope::All {
            self.report_summary(l, &mut blocks);
            self.report_checks(l, &mut blocks);
            self.report_clashes(l, pictures, &mut blocks);
        }
        // the scan against the model, when they were compared
        if let Some(s) = self.collab.cloud.summary.as_ref().filter(|_| scope == Scope::All) {
            let near = (s.within + s.off).max(1);
            blocks.push(Block::PageBreak);
            blocks.push(Block::Heading(l.text("Puntenwolk tegen model", "Point cloud against model").to_string()));
            blocks.push(Block::Counts(vec![
                (format!("{:.0}%", 100.0 * s.within as f32 / near as f32), l.text("binnen tolerantie", "within tolerance").to_string(), if s.off * 20 <= near { Mark::Pass } else { Mark::Warning }),
                (s.off.to_string(), l.text("punten erbuiten", "points off").to_string(), if s.off == 0 { Mark::Pass } else { Mark::Fail }),
                (format!("{:.0} mm", s.worst * 1000.0), l.text("grootste", "largest").to_string(), Mark::Neutral),
                (s.far.to_string(), l.text("zonder model", "without model").to_string(), Mark::Neutral),
            ]));
            blocks.push(Block::Note(format!("{} {} · {} {} mm · {} {:.0} mm", s.points, l.text("punten", "points"), l.text("Tolerantie", "Tolerance"), self.collab.cloud.tolerance.trim(), l.text("gemiddeld", "mean"), s.mean * 1000.0)));
            if !s.hot_spots.is_empty() {
                blocks.push(Block::Table {
                    widths: vec![0.6, 1.2, 1.2, 3.0],
                    headings: vec!["#".into(), l.text("Punten", "Points").into(), l.text("Grootste", "Largest").into(), l.text("Plaats (x, y, z)", "Place (x, y, z)").into()],
                    rows: s
                        .hot_spots
                        .iter()
                        .enumerate()
                        .map(|(k, h)| {
                            let at = self.collab_to_world(h.at);
                            Row { cells: vec![(k + 1).to_string(), h.points.to_string(), format!("{:.0} mm", h.worst * 1000.0), format!("{:.2}, {:.2}, {:.2}", at[0], at[1], at[2])], mark: Some(Mark::Fail) }
                        })
                        .collect(),
                });
            }
        }
        self.report_issues(l, issue_pictures, &mut blocks);
        blocks
    }

    fn report_summary(&self, l: Language, blocks: &mut Vec<Block>) {
        let specs = || self.collab.checked.iter().filter_map(|c| c.report.as_ref().ok()).flat_map(|r| &r.specifications);
        let count = |m: Mark| specs().filter(|s| mark(s.status, s.severity) == m).count().to_string();
        let open = self.collab.clashes.iter().filter(|c| c.decision != Decision::Accepted).count();
        blocks.push(Block::Heading(l.text("Samenvatting", "Summary").to_string()));
        if let Some((score, letter)) = super::collab::quality_score(specs()) {
            blocks.push(Block::Text(format!("{}: {score} / 100 ({letter})", l.text("Kwaliteit van de modellen", "Quality of the models"))));
        }
        if self.collab.compared {
            let new = self.collab.clashes.iter().filter(|c| c.fresh).count();
            blocks.push(Block::Text(format!("{}: {new} {} · {} {} · {} {}", l.text("Sinds de vorige run", "Since the run before"), l.text("nieuw", "new"), self.collab.clashes.len() - new, l.text("gebleven", "stayed"), self.collab.resolved.len(), l.text("opgelost", "solved"))));
        }
        blocks.push(Block::Counts(vec![
            (count(Mark::Pass), l.text("regels geslaagd", "rules passed").to_string(), Mark::Pass),
            (count(Mark::Warning), l.text("waarschuwingen", "warnings").to_string(), Mark::Warning),
            (count(Mark::Fail), l.text("afgekeurd", "failed").to_string(), Mark::Fail),
            (open.to_string(), l.text("open clashes", "open clashes").to_string(), if open == 0 { Mark::Pass } else { Mark::Fail }),
        ]));
    }

    fn report_checks(&self, l: Language, blocks: &mut Vec<Block>) {
        if self.collab.checked.is_empty() {
            blocks.push(Block::Note(l.text("De modellen zijn niet gecontroleerd.", "The models were not checked.").to_string()));
        }
        for checked in &self.collab.checked {
            blocks.push(Block::Heading(format!("{}: {}", l.text("Controle", "Check"), checked.name)));
            let report = match &checked.report {
                Ok(r) => r,
                Err(e) => {
                    blocks.push(Block::Text(format!("{}: {e}", l.text("De controle liep niet", "The check did not run"))));
                    continue;
                }
            };
            if let Some(m) = &report.model {
                blocks.push(Block::Note(format!(
                    "{} {} · {} {} · {} {}",
                    m.element_count,
                    l.text("elementen", "elements"),
                    m.proxy_count,
                    l.text("proxy's", "proxies"),
                    m.without_storey,
                    l.text("zonder bouwlaag", "without storey")
                )));
            }
            let rows = report
                .specifications
                .iter()
                .map(|s| Row {
                    cells: vec![outcome(s.status, s.severity, l).into(), s.identifier.clone().unwrap_or_default(), s.name.clone(), format!("{} / {}", s.passed_count, s.applicable_count)],
                    mark: Some(mark(s.status, s.severity)),
                })
                .collect();
            blocks.push(Block::Table {
                widths: vec![1.3, 1.4, 4.0, 1.0],
                headings: vec![l.text("Uitkomst", "Outcome").into(), l.text("Code", "Code").into(), l.text("Regel", "Rule").into(), l.text("Voldoet", "Pass").into()],
                rows,
            });
            for s in report.specifications.iter().filter(|s| s.status == S::Failed) {
                blocks.push(Block::Subheading(format!("{} · {}", s.identifier.clone().unwrap_or_default(), s.name)));
                if !s.reasons.is_empty() {
                    blocks.push(Block::Note(s.reasons.join(" ")));
                }
                let failed: Vec<_> = s.findings.iter().filter(|f| f.status == S::Failed).collect();
                let rows = failed
                    .iter()
                    .take(IN_PDF)
                    .map(|f| Row {
                        cells: vec![f.name.clone().unwrap_or_else(|| format!("#{}", f.express_id)), f.entity_type.clone(), f.storey.clone().unwrap_or_default(), f.reason.clone()],
                        mark: Some(mark(s.status, s.severity)),
                    })
                    .collect();
                blocks.push(Block::Table {
                    widths: vec![2.4, 1.4, 1.3, 3.6],
                    headings: vec![l.text("Element", "Element").into(), l.text("Klasse", "Class").into(), l.text("Bouwlaag", "Storey").into(), l.text("Waarom", "Why").into()],
                    rows,
                });
                if failed.len() > IN_PDF {
                    blocks.push(Block::Note(format!("+ {} {}", failed.len() - IN_PDF, l.text("meer in het Excel-bestand", "more in the Excel file"))));
                }
            }
        }
    }

    fn report_clashes(&self, l: Language, pictures: &HashMap<usize, Jpeg>, blocks: &mut Vec<Block>) {
        blocks.push(Block::PageBreak);
        blocks.push(Block::Heading(l.text("Clashes", "Clashes").to_string()));
        if self.collab.clashes.is_empty() {
            blocks.push(Block::Note(l.text("Geen clashes gevonden, of nog niet gezocht.", "No clashes found, or not looked for yet.").to_string()));
            return;
        }
        let mut per_test: Vec<(String, usize)> = Vec::new();
        for c in &self.collab.clashes {
            match per_test.iter_mut().find(|(r, _)| *r == c.rule) {
                Some(t) => t.1 += 1,
                None => per_test.push((c.rule.clone(), 1)),
            }
        }
        let groups = self.report_groups();
        blocks.push(Block::Text(format!(
            "{} {} {} {}: {}",
            self.collab.clashes.len(),
            l.text("clashes in", "clashes in"),
            groups.len(),
            l.text("groepen", "groups"),
            per_test.iter().map(|(r, n)| format!("{r} {n}")).collect::<Vec<_>>().join(" · ")
        )));
        let group_row = |g: &Vec<&ClashRow>| {
            let open = g.iter().filter(|c| c.decision == Decision::Open).count();
            let accepted = g.iter().all(|c| c.decision == Decision::Accepted);
            let deepest = g.iter().map(|c| c.distance).fold(0.0f32, f32::max);
            let title = if g.len() == 1 { format!("{} × {}", g[0].a_name, g[0].b_name) } else { group_title(g) };
            Row {
                cells: vec![(g[0].group + 1).to_string(), g[0].rule.clone(), title, g.len().to_string(), format!("{:.0} mm", deepest * 1000.0), if accepted { decision(Decision::Accepted, l).to_string() } else { format!("{open} {}", decision(Decision::Open, l)) }],
                mark: Some(if accepted { Mark::Pass } else if open == 0 { Mark::Warning } else { Mark::Fail }),
            }
        };
        blocks.push(Block::Table {
            widths: vec![0.6, 1.8, 3.8, 0.8, 1.0, 1.1],
            headings: vec!["#".into(), l.text("Test", "Test").into(), l.text("Groep", "Group").into(), l.text("Aantal", "Count").into(), l.text("Diepst", "Deepest").into(), l.text("Besluit", "Decision").into()],
            rows: groups.iter().take(IN_PDF * 3).map(group_row).collect(),
        });
        if groups.len() > IN_PDF * 3 {
            blocks.push(Block::Note(format!("+ {} {}", groups.len() - IN_PDF * 3, l.text("groepen meer in het Excel-bestand", "groups more in the Excel file"))));
        }
        // the groups with their pictures, each with its clashes
        let pictured: Vec<&Vec<&ClashRow>> = groups.iter().filter(|g| pictures.contains_key(&g[0].group)).collect();
        if pictured.is_empty() {
            return;
        }
        blocks.push(Block::PageBreak);
        blocks.push(Block::Heading(l.text("Clashgroepen in beeld", "Clash groups in view").to_string()));
        for g in pictured {
            let title = if g.len() == 1 { format!("{} × {}", g[0].a_name, g[0].b_name) } else { group_title(g) };
            blocks.push(Block::Subheading(format!("{} {} · {title}", l.text("Groep", "Group"), g[0].group + 1)));
            blocks.push(Block::Note(format!("{} · {} {}", g[0].rule, g.len(), l.text("clashes", "clashes"))));
            if let Some(picture) = pictures.get(&g[0].group) {
                blocks.push(Block::Picture { picture: picture.clone(), caption: String::new(), max_height: 250.0 });
            }
            blocks.push(Block::Table {
                widths: vec![3.0, 3.0, 1.2, 1.0],
                headings: vec!["A".into(), "B".into(), l.text("Hoe", "How").into(), l.text("Besluit", "Decision").into()],
                rows: g
                    .iter()
                    .take(12)
                    .map(|c| Row { cells: vec![c.a_name.clone(), c.b_name.clone(), kind(c, l), decision(c.decision, l).into()], mark: Some(if c.decision == Decision::Accepted { Mark::Pass } else { Mark::Fail }) })
                    .collect(),
            });
            if g.len() > 12 {
                blocks.push(Block::Note(format!("+ {} {}", g.len() - 12, l.text("meer in het Excel-bestand", "more in the Excel file"))));
            }
        }
    }

    fn report_issues(&self, l: Language, pictures: &HashMap<usize, Jpeg>, blocks: &mut Vec<Block>) {
        let topics = &self.collab.issues.topics;
        if topics.is_empty() {
            return;
        }
        blocks.push(Block::PageBreak);
        blocks.push(Block::Heading(format!("{} ({})", l.text("Issues", "Issues"), topics.len())));
        for (k, t) in topics.iter().enumerate() {
            blocks.push(Block::Subheading(format!("{}. {}", k + 1, t.title)));
            let mut meta = vec![format!("{}: {}", l.text("Status", "Status"), t.status), format!("{}: {}", l.text("Soort", "Type"), t.kind)];
            for (label, value) in [(l.text("Prioriteit", "Priority"), &t.priority), (l.text("Toegewezen aan", "Assigned to"), &t.assigned_to), (l.text("Uiterlijk", "Due"), &t.due_date)] {
                if !value.is_empty() {
                    meta.push(format!("{label}: {value}"));
                }
            }
            if !t.labels.is_empty() {
                meta.push(format!("{}: {}", l.text("Labels", "Labels"), t.labels.join(", ")));
            }
            meta.push(format!("{} {} · {}", l.text("Gemaakt door", "Made by"), t.creation_author, t.creation_date.get(..10).unwrap_or(&t.creation_date)));
            blocks.push(Block::Note(meta.join(" · ")));
            let own = t.viewpoints.first().and_then(|v| v.snapshot.as_ref()).and_then(|(name, bytes)| snapshot_jpeg(name, bytes));
            if let Some(picture) = own.or_else(|| pictures.get(&k).cloned()) {
                blocks.push(Block::Picture { picture, caption: String::new(), max_height: 240.0 });
            }
            if !t.description.is_empty() {
                blocks.push(Block::Text(t.description.clone()));
            }
            for c in &t.comments {
                blocks.push(Block::Note(format!("{} · {}: {}", c.author, c.date.get(..10).unwrap_or(&c.date), c.text)));
            }
        }
    }

    /// The report as sheets for the workbook: the rules, the findings, the clash groups, the
    /// clashes and the issues.
    pub(crate) fn report_sheets(&self, l: Language) -> Vec<Schedule> {
        let about = format!("{} · {}", self.report_models().join(", "), self.report_date());
        let sheet = |name: &str, columns: Vec<&str>, rows: Vec<Vec<String>>| Schedule {
            name: name.to_string(),
            title: format!("{name} · {about}"),
            columns: columns.into_iter().map(str::to_string).collect(),
            rows: rows.into_iter().map(|cells| ScheduleRow { cells: cells.into_iter().map(Some).collect(), images: Vec::new(), elements: Vec::new(), total: false }).collect(),
            computed: true,
        };
        let mut rules = Vec::new();
        let mut findings = Vec::new();
        for c in &self.collab.checked {
            let Ok(report) = &c.report else { continue };
            for s in &report.specifications {
                rules.push(vec![c.name.clone(), s.identifier.clone().unwrap_or_default(), s.name.clone(), outcome(s.status, s.severity, l).into(), s.passed_count.to_string(), s.failed_count.to_string(), s.applicable_count.to_string(), s.reasons.join(" ")]);
                for f in s.findings.iter().filter(|f| f.status == S::Failed) {
                    findings.push(vec![
                        c.name.clone(),
                        s.identifier.clone().unwrap_or_default(),
                        outcome(s.status, s.severity, l).into(),
                        f.name.clone().unwrap_or_default(),
                        f.entity_type.clone(),
                        f.storey.clone().unwrap_or_default(),
                        f.global_id.clone().unwrap_or_default(),
                        f.reason.clone(),
                    ]);
                }
            }
        }
        let clashes: Vec<Vec<String>> = self
            .collab
            .clashes
            .iter()
            .map(|c| {
                let guid = |id| self.collab_guid_of(id).unwrap_or_default();
                let at = self.collab_to_world(glam::Vec3::from_array(c.at));
                vec![c.rule.clone(), (c.group + 1).to_string(), kind(c, l), decision(c.decision, l).into(), c.a_name.clone(), c.b_name.clone(), guid(c.a), guid(c.b), format!("{:.3}", at[0]), format!("{:.3}", at[1]), format!("{:.3}", at[2])]
            })
            .collect();
        let groups: Vec<Vec<String>> = self
            .report_groups()
            .iter()
            .map(|g| {
                let deepest = g.iter().map(|c| c.distance).fold(0.0f32, f32::max);
                let count = |d: Decision| g.iter().filter(|c| c.decision == d).count().to_string();
                vec![g[0].rule.clone(), (g[0].group + 1).to_string(), group_title(g), g.len().to_string(), format!("{:.0}", deepest * 1000.0), count(Decision::Open), count(Decision::Accepted), count(Decision::Rejected)]
            })
            .collect();
        let issues: Vec<Vec<String>> = self
            .collab
            .issues
            .topics
            .iter()
            .enumerate()
            .map(|(k, t)| vec![(k + 1).to_string(), t.title.clone(), t.status.clone(), t.kind.clone(), t.priority.clone(), t.assigned_to.clone(), t.due_date.clone(), t.creation_author.clone(), t.creation_date.get(..10).unwrap_or(&t.creation_date).to_string(), t.description.clone(), t.comments.len().to_string(), t.guid.clone()])
            .collect();
        vec![
            sheet(l.text("Regels", "Rules"), vec![l.text("Model", "Model"), l.text("Code", "Code"), l.text("Regel", "Rule"), l.text("Uitkomst", "Outcome"), l.text("Geslaagd", "Passed"), l.text("Afgekeurd", "Failed"), l.text("Getoetst", "Checked"), l.text("Opmerking", "Note")], rules),
            sheet(l.text("Bevindingen", "Findings"), vec![l.text("Model", "Model"), l.text("Code", "Code"), l.text("Uitkomst", "Outcome"), l.text("Element", "Element"), l.text("Klasse", "Class"), l.text("Bouwlaag", "Storey"), "GlobalId", l.text("Waarom", "Why")], findings),
            sheet(l.text("Clashgroepen", "Clash groups"), vec![l.text("Test", "Test"), l.text("Groep", "Group"), l.text("Wat", "What"), l.text("Aantal", "Count"), l.text("Diepst (mm)", "Deepest (mm)"), l.text("Open", "Open"), l.text("Akkoord", "Accepted"), l.text("Afgewezen", "Rejected")], groups),
            sheet(l.text("Clashes", "Clashes"), vec![l.text("Test", "Test"), l.text("Groep", "Group"), l.text("Hoe", "How"), l.text("Besluit", "Decision"), "A", "B", "GlobalId A", "GlobalId B", "x", "y", "z"], clashes),
            sheet(
                l.text("Issues", "Issues"),
                vec!["#", l.text("Titel", "Title"), l.text("Status", "Status"), l.text("Soort", "Type"), l.text("Prioriteit", "Priority"), l.text("Toegewezen aan", "Assigned to"), l.text("Uiterlijk", "Due"), l.text("Gemaakt door", "Made by"), l.text("Datum", "Date"), l.text("Omschrijving", "Description"), l.text("Reacties", "Comments"), "Guid"],
                issues,
            ),
        ]
    }
}
