//! Native validation workspace. IFC admission and evaluation never depend on meshes.
use crate::app::{App, Message};
use bim_bcf::{BcfArchive, NewTopic};
use bim_validator_core::{
    IlsOptions, RuleSource, Severity, SpecificationResult, ValidationOptions, ValidationReport,
    ValidationStatus,
};
use iced::widget::{column, container, row, scrollable};
use iced::{Element, Length, Task};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D};
use openaec_ui::design::Look;
use openaec_ui::Language;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
/// How exported BCF topics are formed from findings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BcfGrouping {
    /// One topic per failed finding.
    #[default]
    Finding,
    /// One topic per rule or IDS specification with every affected GlobalId as a component.
    Rule,
    /// One topic per rule and building storey.
    RuleStorey,
}
impl BcfGrouping {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "finding" => Some(Self::Finding),
            "rule" => Some(Self::Rule),
            "rule-storey" => Some(Self::RuleStorey),
            _ => None,
        }
    }
    fn next(self) -> Self {
        match self {
            Self::Finding => Self::Rule,
            Self::Rule => Self::RuleStorey,
            Self::RuleStorey => Self::Finding,
        }
    }
    fn label(self, l: Language) -> &'static str {
        match self {
            Self::Finding => l.text("BCF: per bevinding", "BCF: per finding"),
            Self::Rule => l.text("BCF: per regel", "BCF: per rule"),
            Self::RuleStorey => l.text("BCF: per regel en bouwlaag", "BCF: per rule and storey"),
        }
    }
}
#[derive(Default)]
pub struct State {
    pub ifc: Option<PathBuf>,
    pub ids: Option<PathBuf>,
    /// Built-in model checks ("Modelcontroles").
    pub model_checks: bool,
    /// BIM basis ILS profile.
    pub ils: bool,
    /// Storey name pattern for the ILS profile; empty means the default.
    pub storey_pattern: String,
    pattern_error: Option<String>,
    pub bcf_grouping: BcfGrouping,
    pub report: Option<Arc<ValidationReport>>,
    pub bcf: Option<BcfArchive>,
    pub notice: String,
    pub running: bool,
    generation: u64,
    import_generation: u64,
    cancel: Arc<AtomicBool>,
}
impl State {
    pub fn set_model(&mut self, path: Option<PathBuf>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.generation += 1;
        self.running = false;
        self.ifc = path;
        self.report = None;
        self.notice.clear();
    }
    fn accept(&mut self, generation: u64, report: Result<ValidationReport, String>) {
        if generation != self.generation {
            return;
        }
        self.running = false;
        match report {
            Ok(report) => {
                self.report = Some(Arc::new(report));
                self.notice.clear();
            }
            Err(error) => self.notice = error,
        }
    }
    /// Changed rule selection invalidates a running or finished validation.
    fn reset(&mut self) {
        let input = self.ifc.clone();
        self.set_model(input);
    }
    fn runnable(&self) -> bool {
        self.ifc.is_some()
            && (self.ids.is_some() || self.model_checks || self.ils)
            && self.pattern_error.is_none()
            && !self.running
    }
    fn ils_options(&self) -> Option<IlsOptions> {
        self.ils.then(|| IlsOptions {
            storey_name_pattern: if self.storey_pattern.trim().is_empty() {
                bim_validator_core::DEFAULT_STOREY_PATTERN.into()
            } else {
                self.storey_pattern.clone()
            },
            ..Default::default()
        })
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Input {
    Ifc,
    Ids,
    Bcf,
}
#[derive(Debug, Clone, Copy)]
pub enum Output {
    Json,
    Csv,
    Html(Language),
    Bcf(BcfGrouping),
    ImportedBcf,
}
#[derive(Debug, Clone)]
pub enum Event {
    Pick(Input),
    Picked(Input, Option<PathBuf>),
    ClearIds,
    ModelChecks(bool),
    Ils(bool),
    StoreyPattern(String),
    BcfGrouping,
    Run,
    Cancel,
    Completed(u64, Result<ValidationReport, String>),
    Imported(u64, Result<BcfArchive, String>),
    Export(Output),
    Notice(Result<String, String>),
    Select {
        express_id: u32,
        global_id: Option<String>,
    },
    BcfSelect(String),
    BcfStatus(String, String),
}
fn job(work: impl FnOnce() -> Event + Send + 'static) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(work());
            });
            rx.await
                .unwrap_or_else(|_| Event::Notice(Err("Background task stopped".into())))
        },
        Message::Bim,
    )
}
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err("File exceeds input limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("File exceeds input limit".into());
    }
    Ok(bytes)
}
/// Validate a model file against an optional IDS file and the selected built-in rule sets.
/// The ILS file-name rule uses the IFC file name.
pub fn validate_paths(
    ifc: &Path,
    ids: Option<&Path>,
    model_checks: bool,
    ils: Option<IlsOptions>,
    cancel: &AtomicBool,
) -> Result<ValidationReport, String> {
    let ids_xml = ids
        .map(|ids| {
            String::from_utf8(read_bounded(ids, 8 * 1024 * 1024)?).map_err(|e| e.to_string())
        })
        .transpose()?;
    let ils = ils.map(|mut o| {
        if o.file_name.is_none() {
            o.file_name = ifc.file_name().map(|s| s.to_string_lossy().into_owned());
        }
        o
    });
    let options = ValidationOptions {
        ids_xml,
        model_checks,
        ils,
    };
    let bytes = read_bounded(ifc, 2 * 1024 * 1024 * 1024)?;
    bim_validator_core::validate_model(&bytes, &options, cancel)
}
pub fn update(app: &mut App, event: Event) -> Task<Message> {
    match event {
        Event::Pick(kind) => {
            return job(move || {
                let (label, ext) = match kind {
                    Input::Ifc => ("IFC", vec!["ifc"]),
                    Input::Ids => ("IDS", vec!["ids"]),
                    Input::Bcf => ("BCF", vec!["bcfzip", "bcf"]),
                };
                Event::Picked(
                    kind,
                    rfd::FileDialog::new().add_filter(label, &ext).pick_file(),
                )
            })
        }
        Event::Picked(Input::Ifc, Some(path)) => return app.update(Message::Open(path)),
        Event::Picked(Input::Ids, Some(path)) => {
            let input = app.validation.ifc.clone();
            app.validation.set_model(input);
            app.validation.ids = Some(path);
        }
        Event::Picked(Input::Bcf, Some(path)) => {
            app.validation.import_generation += 1;
            let generation = app.validation.import_generation;
            return job(move || {
                Event::Imported(
                    generation,
                    read_bounded(&path, 50 * 1024 * 1024).and_then(|b| BcfArchive::read(&b)),
                )
            });
        }
        Event::Picked(_, None) => {}
        Event::ClearIds => {
            app.validation.reset();
            app.validation.ids = None;
        }
        Event::ModelChecks(on) => {
            app.validation.reset();
            app.validation.model_checks = on;
        }
        Event::Ils(on) => {
            app.validation.reset();
            app.validation.ils = on;
        }
        Event::StoreyPattern(pattern) => {
            app.validation.reset();
            app.validation.pattern_error = if pattern.trim().is_empty() {
                None
            } else {
                bim_validator_core::check_pattern(&pattern).err()
            };
            app.validation.storey_pattern = pattern;
        }
        Event::BcfGrouping => {
            app.validation.bcf_grouping = app.validation.bcf_grouping.next();
        }
        Event::Run => {
            if !app.validation.runnable() {
                return Task::none();
            }
            let Some(ifc) = app.validation.ifc.clone() else {
                return Task::none();
            };
            let ids = app.validation.ids.clone();
            let model_checks = app.validation.model_checks;
            let ils = app.validation.ils_options();
            app.validation.generation += 1;
            let generation = app.validation.generation;
            app.validation.cancel = Arc::new(AtomicBool::new(false));
            let cancel = app.validation.cancel.clone();
            app.validation.running = true;
            app.validation.report = None;
            app.validation.notice.clear();
            return job(move || {
                Event::Completed(
                    generation,
                    std::panic::catch_unwind(|| {
                        validate_paths(&ifc, ids.as_deref(), model_checks, ils, &cancel)
                    })
                    .unwrap_or_else(|_| Err("Validation worker stopped unexpectedly".into())),
                )
            });
        }
        Event::Cancel => {
            let source = app.validation.ifc.clone();
            app.validation.set_model(source);
        }
        Event::Completed(generation, result) => app.validation.accept(generation, result),
        Event::Imported(generation, result) => {
            if generation == app.validation.import_generation {
                match result {
                    Ok(bcf) => {
                        app.validation.bcf = Some(bcf);
                        app.validation.notice.clear();
                    }
                    Err(e) => app.validation.notice = e,
                }
            }
        }
        Event::Notice(result) => app.validation.notice = result.unwrap_or_else(|e| e),
        Event::Export(kind) => {
            let report = app.validation.report.clone();
            let archive = app.validation.bcf.clone();
            let source = app.validation.ifc.clone().unwrap_or_default();
            return job(move || {
                let ext = match kind {
                    Output::Json => "json",
                    Output::Csv => "csv",
                    Output::Html(_) => "html",
                    Output::Bcf(_) | Output::ImportedBcf => "bcfzip",
                };
                let Some(path) = rfd::FileDialog::new()
                    .add_filter(ext, &[ext])
                    .set_file_name(format!("validation.{ext}"))
                    .save_file()
                else {
                    return Event::Notice(Ok(String::new()));
                };
                let bytes = match kind {
                    Output::Json => report
                        .as_ref()
                        .ok_or("No validation report".to_string())
                        .and_then(|r| {
                            serde_json::to_vec_pretty(r.as_ref()).map_err(|e| e.to_string())
                        }),
                    Output::Csv => report
                        .as_ref()
                        .ok_or("No validation report".to_string())
                        .map(|r| report_csv(r).into_bytes()),
                    Output::Html(language) => report
                        .as_ref()
                        .ok_or("No validation report".to_string())
                        .map(|r| report_html(r, &source, language).into_bytes()),
                    Output::Bcf(grouping) => report
                        .as_ref()
                        .ok_or("No validation report".to_string())
                        .and_then(|r| report_bcf(r, &source.to_string_lossy(), grouping))
                        .and_then(|a| a.write()),
                    Output::ImportedBcf => archive
                        .ok_or("No imported BCF archive".to_string())
                        .and_then(|a| a.write()),
                };
                Event::Notice(
                    bytes
                        .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                        .map(|()| format!("Saved {}", path.display())),
                )
            });
        }
        Event::Select {
            express_id,
            global_id,
        } => select_finding(app, express_id, global_id.as_deref()),
        Event::BcfStatus(id, status) => {
            if let Some(archive) = &mut app.validation.bcf {
                if let Err(e) = archive.set_status(&id, &status) {
                    app.validation.notice = e;
                }
            }
        }
        Event::BcfSelect(id) => {
            if let Some(topic) = app
                .validation
                .bcf
                .as_ref()
                .and_then(|b| b.topics().into_iter().find(|t| t.id == id))
            {
                if let Some(guid) = topic.component_guids.first() {
                    select_finding(app, 0, Some(guid));
                }
                // Source cameras are preserved in export; selection frames available geometry.
                if app.validation.notice.is_empty() {
                    app.validation.notice = "BCF camera, visibility and clipping data are preserved; this view frames the selected component.".into();
                }
            }
        }
    }
    Task::none()
}
fn select_finding(app: &mut App, express_id: u32, guid: Option<&str>) {
    let Some(model) = &app.model else {
        return;
    };
    let source = app.validation.ifc.as_ref();
    let layer_range = app
        .models
        .iter()
        .find(|entry| Some(&entry.model.path) == source)
        .map(|entry| entry.layers.clone());
    if layer_range.is_none() {
        app.validation.notice = "The validated source model is not loaded in the viewer.".into();
        return;
    }
    let mut hit = None;
    for (layer_index, layer) in model.layers.iter().enumerate() {
        if layer_range
            .as_ref()
            .is_some_and(|range| !range.contains(&layer_index))
        {
            continue;
        }
        for &(id, _, _) in &layer.elements {
            let matches = if let Some(guid) = guid {
                model
                    .props
                    .get(&id)
                    .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                    .and_then(|p| {
                        p.get("GlobalId")
                            .and_then(|v| v.as_str())
                            .map(|s| s == guid)
                    })
                    .unwrap_or(false)
            } else {
                model
                    .props
                    .get(&id)
                    .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                    .and_then(|p| {
                        p.get("ExpressId")
                            .and_then(|v| v.as_str())
                            .and_then(|v| v.parse::<u32>().ok())
                    })
                    .is_some_and(|original| original == express_id)
            };
            if matches {
                hit = Some((layer_index, id));
                break;
            }
        }
        if hit.is_some() {
            break;
        }
    }
    if let Some((layer, id)) = hit {
        app.selected = Some((layer, Some(id)));
        app.selection = model.element_ranges(id);
        app.set_layer_visible(layer, true);
        if let Some(bounds) = app.selection_bounds() {
            app.anim = None;
            app.camera.fit(&bounds, app.aspect());
        }
        app.validation.notice.clear();
    } else {
        app.validation.notice =
            "This entity has no matching visible geometry in the loaded source model.".into();
    }
}
fn status(status: ValidationStatus, l: Language) -> &'static str {
    match status {
        ValidationStatus::Passed => l.text("Geslaagd", "Passed"),
        ValidationStatus::Failed => l.text("Afgekeurd", "Failed"),
        ValidationStatus::NotCheckable => l.text("Niet controleerbaar", "Not checkable"),
    }
}
/// Outcome of a specification; a failed warning-severity rule reads as a warning.
fn spec_status(spec: &SpecificationResult, l: Language) -> &'static str {
    match (spec.status, spec.severity) {
        (ValidationStatus::Failed, Severity::Warning) => l.text("Waarschuwing", "Warning"),
        (s, _) => status(s, l),
    }
}
fn severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}
/// Rule code and name of built-in rules; IDS specifications keep their own name.
fn spec_title(spec: &SpecificationResult) -> String {
    match (&spec.identifier, spec.source) {
        (Some(code), RuleSource::ModelChecks | RuleSource::Ils) => {
            format!("{code} {}", spec.name)
        }
        _ => spec.name.clone(),
    }
}
pub fn panel(app: &App, look: Look, l: Language) -> Element<'_, Message> {
    let state = &app.validation;
    let btn = |label: &'static str, event: Event, enabled: bool| {
        w::button(
            look,
            label,
            K::Secondary,
            D::Compact,
            None,
            enabled.then_some(Message::Bim(event)),
        )
    };
    let filename = |path: &Option<PathBuf>| {
        path.as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "—".into())
    };
    let idle = !state.running;
    let mut content = column![
        w::heading(look, "BIM / IDS", 14.0),
        row![
            btn(
                l.text("IFC kiezen", "Choose IFC"),
                Event::Pick(Input::Ifc),
                idle
            ),
            btn(
                l.text("IDS kiezen", "Choose IDS"),
                Event::Pick(Input::Ids),
                idle
            ),
            btn(
                l.text("IDS wissen", "Clear IDS"),
                Event::ClearIds,
                idle && state.ids.is_some()
            )
        ]
        .spacing(4),
        w::secondary(look, filename(&state.ifc)),
        w::secondary(look, filename(&state.ids)),
        w::checkbox(
            look,
            l.text(
                "Modelcontroles (buildingSMART-regels)",
                "Model checks (buildingSMART rules)"
            ),
            state.model_checks,
            idle.then_some(|on| Message::Bim(Event::ModelChecks(on))),
        ),
        w::checkbox(
            look,
            "BIM basis ILS",
            state.ils,
            idle.then_some(|on| Message::Bim(Event::Ils(on))),
        ),
    ]
    .spacing(7)
    .padding(8);
    if state.ils {
        content = content
            .push(w::secondary(
                look,
                l.text(
                    "Bouwlaagnaam (XSD-patroon, leeg = standaard)",
                    "Storey name (XSD pattern, empty = default)",
                ),
            ))
            .push(w::text_field(
                look,
                bim_validator_core::DEFAULT_STOREY_PATTERN,
                &state.storey_pattern,
                idle.then_some(|s| Message::Bim(Event::StoreyPattern(s))),
                D::Compact,
                state.pattern_error.as_deref(),
            ));
    }
    content = content
        .push(
            row![
                btn(
                    l.text("Valideren", "Validate"),
                    Event::Run,
                    state.runnable()
                ),
                btn(l.text("Annuleren", "Cancel"), Event::Cancel, state.running)
            ]
            .spacing(4),
        )
        .push(
            row![
                btn("JSON", Event::Export(Output::Json), state.report.is_some()),
                btn("CSV", Event::Export(Output::Csv), state.report.is_some()),
                btn(
                    "HTML",
                    Event::Export(Output::Html(l)),
                    state.report.is_some()
                ),
                btn(
                    "BCF",
                    Event::Export(Output::Bcf(state.bcf_grouping)),
                    state.report.is_some()
                )
            ]
            .spacing(4),
        )
        .push(btn(state.bcf_grouping.label(l), Event::BcfGrouping, true))
        .push(btn(
            l.text("BCF openen", "Open BCF"),
            Event::Pick(Input::Bcf),
            true,
        ));
    if state.running {
        content = content.push(w::body(
            look,
            l.text("Validatie loopt…", "Validation running…"),
        ));
    }
    if !state.notice.is_empty() {
        content = content.push(w::secondary(look, state.notice.clone()));
    }
    if let Some(report) = &state.report {
        content = content.push(w::heading(
            look,
            format!("{} · {}", report.schema, status(report.status, l)),
            13.0,
        ));
        if let Some(m) = &report.model {
            let pct = |v: usize| {
                if m.element_count == 0 {
                    "—".to_string()
                } else {
                    format!("{:.0}%", 100.0 * v as f64 / m.element_count as f64)
                }
            };
            content = content.push(w::secondary(
                look,
                format!(
                    "{} {} · proxy {} · {} {} · {} {} · {} {} · {} {} · {}",
                    m.element_count,
                    l.text("elementen", "elements"),
                    pct(m.proxy_count),
                    l.text("zonder bouwlaag", "without storey"),
                    pct(m.without_storey),
                    l.text("zonder materiaal", "without material"),
                    pct(m.without_material),
                    l.text("zonder NL-SfB", "without NL-SfB"),
                    pct(m.without_nlsfb),
                    l.text("zonder type", "without type"),
                    pct(m.without_type),
                    m.length_unit.as_deref().unwrap_or("—"),
                ),
            ));
        }
        for spec in &report.specifications {
            content = content.push(w::body(
                look,
                format!(
                    "{} — {} ({}/{})",
                    spec_title(spec),
                    spec_status(spec, l),
                    spec.passed_count,
                    spec.applicable_count
                ),
            ));
            for reason in &spec.reasons {
                content = content.push(w::secondary(look, reason.clone()));
            }
            for finding in spec.findings.iter().take(100) {
                let mut label = format!("#{} {}", finding.express_id, finding.entity_type);
                if let Some(name) = &finding.name {
                    label.push_str(&format!(" '{name}'"));
                }
                if let Some(storey) = &finding.storey {
                    label.push_str(&format!(" · {storey}"));
                }
                let outcome = if finding.status == ValidationStatus::NotCheckable {
                    status(finding.status, l)
                } else {
                    spec_status(spec, l)
                };
                label.push_str(&format!(" · {outcome}"));
                content = content
                    .push(w::button(
                        look,
                        label,
                        K::Quiet,
                        D::Compact,
                        None,
                        Some(Message::Bim(Event::Select {
                            express_id: finding.express_id,
                            global_id: finding.global_id.clone(),
                        })),
                    ))
                    .push(w::secondary(look, finding.reason.clone()));
            }
            if spec.findings.len() > 100 {
                content = content.push(w::secondary(
                    look,
                    l.text(
                        "Eerste 100 bevindingen; export bevat alle resultaten.",
                        "First 100 findings; export includes all results.",
                    ),
                ));
            }
        }
    }
    if let Some(archive) = &state.bcf {
        content = content.push(w::heading(look, "BCF", 13.0)).push(btn(
            l.text("BCF opslaan", "Save BCF"),
            Event::Export(Output::ImportedBcf),
            true,
        ));
        for topic in archive.topics().into_iter().take(100) {
            content = content
                .push(w::button(
                    look,
                    format!("{} · {}", topic.status, topic.title),
                    K::Quiet,
                    D::Compact,
                    None,
                    Some(Message::Bim(Event::BcfSelect(topic.id.clone()))),
                ))
                .push(
                    row![
                        btn(
                            "Open",
                            Event::BcfStatus(topic.id.clone(), "Open".into()),
                            true
                        ),
                        btn("Closed", Event::BcfStatus(topic.id, "Closed".into()), true)
                    ]
                    .spacing(4),
                );
        }
    }
    container(scrollable(content)).height(Length::Fill).into()
}
/// One row per specification-level reason and per finding. Columns after `reason` were added
/// with the built-in rule sets; the first five keep their meaning.
pub fn report_csv(report: &ValidationReport) -> String {
    fn cell(value: &str) -> String {
        let mut value = value.to_string();
        if value.trim_start().starts_with(['=', '+', '-', '@']) {
            value.insert(0, '\'');
        }
        format!("\"{}\"", value.replace('"', "\"\""))
    }
    let mut out = "specification,status,express_id,global_id,reason,code,severity,entity_type,name,storey\r\n".to_string();
    for spec in &report.specifications {
        let code = cell(spec.identifier.as_deref().unwrap_or(""));
        let sev = severity(spec.severity);
        if spec.findings.is_empty() || !spec.reasons.is_empty() {
            out.push_str(&format!(
                "{},{},,,{},{code},{sev},,,\r\n",
                cell(&spec.name),
                cell(status(spec.status, Language::English)),
                cell(&spec.reasons.join("; "))
            ));
        }
        for f in &spec.findings {
            out.push_str(&format!(
                "{},{},{},{},{},{code},{sev},{},{},{}\r\n",
                cell(&spec.name),
                cell(status(f.status, Language::English)),
                f.express_id,
                cell(f.global_id.as_deref().unwrap_or("")),
                cell(&f.reason),
                cell(&f.entity_type),
                cell(f.name.as_deref().unwrap_or("")),
                cell(f.storey.as_deref().unwrap_or(""))
            ));
        }
    }
    out
}
/// Self-contained HTML report titled after the source model file.
pub fn report_html(report: &ValidationReport, source: &Path, l: Language) -> String {
    let title = source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let language = match l {
        Language::Dutch => bim_validator_core::ReportLanguage::Dutch,
        Language::English => bim_validator_core::ReportLanguage::English,
    };
    bim_validator_core::report_html(report, &title, language)
}
/// BCF components must be valid IFC GlobalIds; malformed ones stay in the description only.
fn components<'a>(guids: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    guids
        .filter(|g| {
            g.len() == 22
                && g.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
        })
        .filter(|g| seen.insert(*g))
        .map(String::from)
        .collect()
}
/// Topics per finding, per rule, or per rule and storey; IDs are stable UUIDv5 values of the
/// source path and rule position.
pub fn report_bcf(
    report: &ValidationReport,
    source: &str,
    grouping: BcfGrouping,
) -> Result<BcfArchive, String> {
    let date = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| e.to_string())?;
    let topic = |key: String, title: String, description: String, guids: Vec<String>| NewTopic {
        id: uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("{source}|{key}").as_bytes(),
        )
        .to_string(),
        title,
        status: "Open".into(),
        description,
        component_guids: guids,
        creation_date: date.clone(),
        creation_author: "Open BIM Validator".into(),
    };
    let mut topics = Vec::new();
    for (i, spec) in report.specifications.iter().enumerate() {
        let failing: Vec<_> = spec
            .findings
            .iter()
            .filter(|f| f.status != ValidationStatus::Passed)
            .collect();
        let rule_level = spec.status != ValidationStatus::Passed && !spec.reasons.is_empty();
        if rule_level && (grouping == BcfGrouping::Finding || failing.is_empty()) {
            topics.push(topic(
                format!("{i}|specification"),
                spec_title(spec),
                format!(
                    "{}: {}",
                    spec_status(spec, Language::English),
                    spec.reasons.join("; ")
                ),
                Vec::new(),
            ));
        }
        if grouping == BcfGrouping::Finding {
            for (j, f) in spec.findings.iter().enumerate() {
                if f.status == ValidationStatus::Passed {
                    continue;
                }
                topics.push(topic(
                    format!("{i}|{j}|{}", f.express_id),
                    format!("{} · #{}", spec_title(spec), f.express_id),
                    f.reason.clone(),
                    components(f.global_id.iter().map(String::as_str)),
                ));
            }
            continue;
        }
        let mut groups: BTreeMap<Option<&str>, Vec<_>> = BTreeMap::new();
        for f in failing {
            let key = match grouping {
                BcfGrouping::RuleStorey => f.storey.as_deref(),
                _ => None,
            };
            groups.entry(key).or_default().push(f);
        }
        for (storey, findings) in groups {
            let mut title = format!("{} ({})", spec_title(spec), findings.len());
            if grouping == BcfGrouping::RuleStorey {
                title.push_str(&format!(" · {}", storey.unwrap_or("no storey")));
            }
            let mut description = format!(
                "{} ({}): {} findings",
                spec_status(spec, Language::English),
                severity(spec.severity),
                findings.len()
            );
            for reason in &spec.reasons {
                description.push_str(&format!("\n{reason}"));
            }
            for f in findings.iter().take(50) {
                description.push_str(&format!(
                    "\n#{} {}{}: {}",
                    f.express_id,
                    f.entity_type,
                    f.name
                        .as_deref()
                        .map(|n| format!(" '{n}'"))
                        .unwrap_or_default(),
                    f.reason
                ));
            }
            if findings.len() > 50 {
                description.push_str(&format!("\n… {} more", findings.len() - 50));
            }
            topics.push(topic(
                format!("{i}|rule|{}", storey.unwrap_or("")),
                title,
                description,
                components(findings.iter().filter_map(|f| f.global_id.as_deref())),
            ));
        }
    }
    BcfArchive::from_topics(&topics)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn model(path: &str) -> Arc<crate::model::Model> {
        let layer = crate::model::Layer {
            name: "test".into(),
            positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0.],
            indices: vec![0, 1, 2],
            elements: vec![(42, 0, 3)],
            props: vec![(42, r#"{"ExpressId":"42"}"#.into())],
            ..Default::default()
        };
        Arc::new(crate::model::Model::new(path.into(), "IFC", vec![layer]))
    }
    #[test]
    fn selection_never_uses_an_unrelated_loaded_source() {
        let mut app = App::default();
        let _ = app.update(Message::Loaded(Ok(model("old.ifc"))));
        app.validation.set_model(Some("missing.ifc".into()));
        select_finding(&mut app, 42, None);
        assert!(app.selected.is_none());
        assert!(app.validation.notice.contains("not loaded"));
    }
    #[test]
    fn original_express_id_selects_inside_federated_source_layers() {
        let mut app = App::default();
        let _ = app.update(Message::Loaded(Ok(model("first.ifc"))));
        app.add_model(model("second.ifc"));
        app.validation.set_model(Some("second.ifc".into()));
        select_finding(&mut app, 42, None);
        assert_eq!(app.selected.map(|s| s.0), Some(1));
    }
    #[test]
    fn changed_model_discards_late_validation_result() {
        let mut state = State::default();
        state.set_model(Some("first.ifc".into()));
        let generation = state.generation;
        state.set_model(Some("second.ifc".into()));
        state.accept(generation, Err("old failure".into()));
        assert!(state.notice.is_empty());
        assert!(state.report.is_none());
    }
    #[test]
    fn csv_quotes_and_defuses_spreadsheet_formulas() {
        let report = ValidationReport {
            schema: "IFC4".into(),
            ids_title: "test".into(),
            status: ValidationStatus::Failed,
            specifications: vec![bim_validator_core::SpecificationResult {
                name: "=1+1".into(),
                identifier: None,
                status: ValidationStatus::Failed,
                applicable_count: 0,
                passed_count: 0,
                failed_count: 0,
                findings: vec![],
                reasons: vec!["a,\"b\"".into()],
                source: RuleSource::Ids,
                severity: Severity::Error,
            }],
            model: None,
        };
        let csv = report_csv(&report);
        assert!(csv.contains("\"'=1+1\""));
        assert!(csv.contains("\"a,\"\"b\"\"\""));
        let archive = report_bcf(&report, "source.ifc", BcfGrouping::Finding).unwrap();
        assert_eq!(archive.topics().len(), 1);
        assert!(archive.topics()[0].component_guids.is_empty());
        assert!(archive.topics()[0].description.contains("a,\"b\""));
    }
    #[test]
    fn csv_retains_specification_reasons_alongside_entity_findings() {
        let report = ValidationReport {
            schema: "IFC4".into(),
            ids_title: "test".into(),
            status: ValidationStatus::Failed,
            specifications: vec![bim_validator_core::SpecificationResult {
                name: "Count and value".into(),
                identifier: None,
                status: ValidationStatus::Failed,
                applicable_count: 1,
                passed_count: 0,
                failed_count: 1,
                reasons: vec!["Expected at least two entities".into()],
                findings: vec![bim_validator_core::Finding {
                    express_id: 42,
                    global_id: None,
                    entity_type: "IFCWALL".into(),
                    facet: "attribute".into(),
                    status: ValidationStatus::Failed,
                    reason: "Incorrect name".into(),
                    name: None,
                    storey: None,
                }],
                source: RuleSource::Ids,
                severity: Severity::Error,
            }],
            model: None,
        };
        let csv = report_csv(&report);
        assert!(csv.contains("Expected at least two entities"));
        assert!(csv.contains("Incorrect name"));
    }
    fn finding(id: u32, guid: &str, storey: &str) -> bim_validator_core::Finding {
        bim_validator_core::Finding {
            express_id: id,
            global_id: Some(guid.into()),
            entity_type: "IFCWALL".into(),
            facet: "containment".into(),
            status: ValidationStatus::Failed,
            reason: "Not contained".into(),
            name: Some(format!("Wall {id}")),
            storey: Some(storey.into()),
        }
    }
    fn rule_report() -> ValidationReport {
        ValidationReport {
            schema: "IFC4".into(),
            ids_title: String::new(),
            status: ValidationStatus::Passed,
            specifications: vec![bim_validator_core::SpecificationResult {
                name: "Proxy elements".into(),
                identifier: Some("OBV-PROXY".into()),
                status: ValidationStatus::Failed,
                applicable_count: 3,
                passed_count: 0,
                failed_count: 3,
                reasons: vec!["3 of 3 elements are proxies".into()],
                findings: vec![
                    finding(1, "0000000000000000000001", "00 begane grond"),
                    finding(2, "0000000000000000000002", "01 verdieping"),
                    // Malformed GlobalIds stay out of BCF components.
                    finding(3, "bad", "00 begane grond"),
                ],
                source: RuleSource::ModelChecks,
                severity: Severity::Warning,
            }],
            model: None,
        }
    }
    #[test]
    fn bcf_topics_per_finding_rule_or_rule_and_storey() {
        let report = rule_report();
        let per_finding = report_bcf(&report, "m.ifc", BcfGrouping::Finding).unwrap();
        // One rule-level topic for the reasons plus one per finding.
        assert_eq!(per_finding.topics().len(), 4);
        let per_rule = report_bcf(&report, "m.ifc", BcfGrouping::Rule)
            .unwrap()
            .topics();
        assert_eq!(per_rule.len(), 1);
        assert_eq!(per_rule[0].title, "OBV-PROXY Proxy elements (3)");
        assert_eq!(
            per_rule[0].component_guids,
            vec!["0000000000000000000001", "0000000000000000000002"]
        );
        assert!(per_rule[0].description.contains("#3 IFCWALL 'Wall 3'"));
        let per_storey = report_bcf(&report, "m.ifc", BcfGrouping::RuleStorey)
            .unwrap()
            .topics();
        let mut titles: Vec<_> = per_storey.iter().map(|t| t.title.clone()).collect();
        titles.sort();
        assert_eq!(
            titles,
            vec![
                "OBV-PROXY Proxy elements (1) · 01 verdieping",
                "OBV-PROXY Proxy elements (2) · 00 begane grond"
            ]
        );
        let again = report_bcf(&report, "m.ifc", BcfGrouping::RuleStorey)
            .unwrap()
            .topics();
        let ids = |t: &[bim_bcf::TopicSummary]| {
            let mut v: Vec<_> = t.iter().map(|t| t.id.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(ids(&per_storey), ids(&again));
    }
    #[test]
    fn csv_and_html_carry_rule_code_severity_and_storey() {
        let report = rule_report();
        let csv = report_csv(&report);
        assert!(csv.starts_with(
            "specification,status,express_id,global_id,reason,code,severity,entity_type,name,storey\r\n"
        ));
        assert!(csv.contains("\"OBV-PROXY\",warning,\"IFCWALL\",\"Wall 1\",\"00 begane grond\""));
        let html = report_html(&report, Path::new("dir/m.ifc"), Language::Dutch);
        assert!(html.contains("<title>m.ifc</title>"));
        assert!(html.contains("Waarschuwing"));
    }
}
