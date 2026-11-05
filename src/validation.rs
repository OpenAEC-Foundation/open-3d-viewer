//! Native validation workspace. IFC admission and evaluation never depend on meshes.
use crate::app::{App, Message};
use bim_bcf::{BcfArchive, NewTopic};
use bim_validator_core::{ValidationReport, ValidationStatus};
use iced::widget::{column, container, row, scrollable};
use iced::{Element, Length, Task};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D};
use openaec_ui::design::Look;
use openaec_ui::Language;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[derive(Default)]
pub struct State {
    pub ifc: Option<PathBuf>,
    pub ids: Option<PathBuf>,
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
    Bcf,
    ImportedBcf,
}
#[derive(Debug, Clone)]
pub enum Event {
    Pick(Input),
    Picked(Input, Option<PathBuf>),
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
pub fn validate_paths(
    ifc: &Path,
    ids: &Path,
    cancel: &AtomicBool,
) -> Result<ValidationReport, String> {
    let bytes = read_bounded(ifc, 2 * 1024 * 1024 * 1024)?;
    let xml = String::from_utf8(read_bounded(ids, 8 * 1024 * 1024)?).map_err(|e| e.to_string())?;
    bim_validator_core::validate_with_cancel(&bytes, &xml, cancel)
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
        Event::Run => {
            if app.validation.running {
                return Task::none();
            }
            let (Some(ifc), Some(ids)) = (app.validation.ifc.clone(), app.validation.ids.clone())
            else {
                return Task::none();
            };
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
                    std::panic::catch_unwind(|| validate_paths(&ifc, &ids, &cancel))
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
                    Output::Bcf | Output::ImportedBcf => "bcfzip",
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
                    Output::Bcf => report
                        .as_ref()
                        .ok_or("No validation report".to_string())
                        .and_then(|r| report_bcf(r, &source.to_string_lossy()))
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
    let mut content = column![
        w::heading(look, "BIM / IDS", 14.0),
        row![
            btn(
                l.text("IFC kiezen", "Choose IFC"),
                Event::Pick(Input::Ifc),
                !state.running
            ),
            btn(
                l.text("IDS kiezen", "Choose IDS"),
                Event::Pick(Input::Ids),
                !state.running
            )
        ]
        .spacing(4),
        w::secondary(look, filename(&state.ifc)),
        w::secondary(look, filename(&state.ids)),
        row![
            btn(
                l.text("Valideren", "Validate"),
                Event::Run,
                state.ifc.is_some() && state.ids.is_some() && !state.running
            ),
            btn(l.text("Annuleren", "Cancel"), Event::Cancel, state.running)
        ]
        .spacing(4),
        row![
            btn("JSON", Event::Export(Output::Json), state.report.is_some()),
            btn("CSV", Event::Export(Output::Csv), state.report.is_some()),
            btn("BCF", Event::Export(Output::Bcf), state.report.is_some())
        ]
        .spacing(4),
        btn(
            l.text("BCF openen", "Open BCF"),
            Event::Pick(Input::Bcf),
            true
        ),
    ]
    .spacing(7)
    .padding(8);
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
        for spec in &report.specifications {
            content = content.push(w::body(
                look,
                format!(
                    "{} — {} ({}/{})",
                    spec.name,
                    status(spec.status, l),
                    spec.passed_count,
                    spec.applicable_count
                ),
            ));
            for reason in &spec.reasons {
                content = content.push(w::secondary(look, reason.clone()));
            }
            for finding in spec.findings.iter().take(100) {
                content = content
                    .push(w::button(
                        look,
                        format!(
                            "#{} {} · {}",
                            finding.express_id,
                            finding.entity_type,
                            status(finding.status, l)
                        ),
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
pub fn report_csv(report: &ValidationReport) -> String {
    fn cell(value: &str) -> String {
        let mut value = value.to_string();
        if value.trim_start().starts_with(['=', '+', '-', '@']) {
            value.insert(0, '\'');
        }
        format!("\"{}\"", value.replace('"', "\"\""))
    }
    let mut out = "specification,status,express_id,global_id,reason\r\n".to_string();
    for spec in &report.specifications {
        if spec.findings.is_empty() || !spec.reasons.is_empty() {
            out.push_str(&format!(
                "{},{},,,{}\r\n",
                cell(&spec.name),
                cell(status(spec.status, Language::English)),
                cell(&spec.reasons.join("; "))
            ));
        }
        for f in &spec.findings {
            out.push_str(&format!(
                "{},{},{},{},{}\r\n",
                cell(&spec.name),
                cell(status(f.status, Language::English)),
                f.express_id,
                cell(f.global_id.as_deref().unwrap_or("")),
                cell(&f.reason)
            ));
        }
    }
    out
}
pub fn report_bcf(report: &ValidationReport, source: &str) -> Result<BcfArchive, String> {
    let date = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| e.to_string())?;
    let mut topics = Vec::new();
    for (i, spec) in report.specifications.iter().enumerate() {
        if spec.status != ValidationStatus::Passed && !spec.reasons.is_empty() {
            topics.push(NewTopic {
                id: uuid::Uuid::new_v5(
                    &uuid::Uuid::NAMESPACE_URL,
                    format!("{source}|{i}|specification").as_bytes(),
                )
                .to_string(),
                title: spec.name.clone(),
                status: "Open".into(),
                description: format!(
                    "{}: {}",
                    status(spec.status, Language::English),
                    spec.reasons.join("; ")
                ),
                component_guids: Vec::new(),
                creation_date: date.clone(),
                creation_author: "Open BIM Validator".into(),
            });
        }
        for (j, f) in spec.findings.iter().enumerate() {
            if f.status == ValidationStatus::Passed {
                continue;
            }
            topics.push(NewTopic {
                id: uuid::Uuid::new_v5(
                    &uuid::Uuid::NAMESPACE_URL,
                    format!("{source}|{i}|{j}|{}", f.express_id).as_bytes(),
                )
                .to_string(),
                title: format!("{} · #{}", spec.name, f.express_id),
                status: "Open".into(),
                description: f.reason.clone(),
                component_guids: f.global_id.iter().cloned().collect(),
                creation_date: date.clone(),
                creation_author: "Open BIM Validator".into(),
            });
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
            }],
        };
        let csv = report_csv(&report);
        assert!(csv.contains("\"'=1+1\""));
        assert!(csv.contains("\"a,\"\"b\"\"\""));
        let archive = report_bcf(&report, "source.ifc").unwrap();
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
                }],
            }],
        };
        let csv = report_csv(&report);
        assert!(csv.contains("Expected at least two entities"));
        assert!(csv.contains("Incorrect name"));
    }
}
