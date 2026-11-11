//! Native IDS evaluation over the complete IFC STEP graph, plus built-in model checks and the
//! Dutch BIM basis ILS profile. Unsupported semantics produce `not_checkable`, never a success.
mod checks;
mod evaluator;
mod graph;
mod html;
mod ils;
mod values;
pub use html::{report_html, ReportLanguage};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Passed,
    Failed,
    NotCheckable,
}
/// Normative rules are errors; industry practices are warnings and never fail the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Error,
    Warning,
}
/// Origin of a specification result: an IDS document or a built-in rule set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    #[default]
    Ids,
    ModelChecks,
    Ils,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema: String,
    pub ids_title: String,
    /// Aggregate of error-severity specifications; warnings are reported but do not fail.
    pub status: ValidationStatus,
    pub specifications: Vec<SpecificationResult>,
    #[serde(default)]
    pub model: Option<ModelSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificationResult {
    pub name: String,
    /// IDS identifier, or the rule code of a built-in check (for example `SPS007`).
    pub identifier: Option<String>,
    pub status: ValidationStatus,
    pub applicable_count: usize,
    pub passed_count: usize,
    pub failed_count: usize,
    pub findings: Vec<Finding>,
    pub reasons: Vec<String>,
    #[serde(default)]
    pub source: RuleSource,
    #[serde(default)]
    pub severity: Severity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub express_id: u32,
    pub global_id: Option<String>,
    pub entity_type: String,
    pub facet: String,
    pub status: ValidationStatus,
    pub reason: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Name of the building storey reached through containment or decomposition.
    #[serde(default)]
    pub storey: Option<String>,
}
/// Model-quality numbers over the checked elements: IfcElement occurrences except feature and
/// virtual elements and parts aggregated into another element.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelSummary {
    pub project_name: Option<String>,
    /// Declared project length unit, for example `MILLIMETRE` or `FOOT`.
    pub length_unit: Option<String>,
    /// Metres per declared length unit, when resolvable.
    pub length_unit_scale: Option<f64>,
    pub entity_count: usize,
    pub element_count: usize,
    pub proxy_count: usize,
    pub without_storey: usize,
    pub without_material: usize,
    pub without_nlsfb: usize,
    pub without_type: usize,
    /// Storeys ordered by elevation (missing elevations last), then express ID.
    pub storeys: Vec<StoreySummary>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoreySummary {
    pub express_id: u32,
    pub global_id: Option<String>,
    pub name: Option<String>,
    /// Elevation in project length units.
    pub elevation: Option<f64>,
    pub element_count: usize,
}
/// Default BIM basis ILS storey name: sortable two-digit number (optionally negative), an
/// optional letter, a space and a description, as in `00 begane grond` or `-01 kelder`.
pub const DEFAULT_STOREY_PATTERN: &str = r"-?[0-9]{2}[a-zA-Z]? \S.*";
/// Default BIM basis ILS file name: `<project>_<discipline>_<part>.ifc`.
pub const DEFAULT_FILE_NAME_PATTERN: &str = r"[^_]+_[^_]+_[^_]+\.[iI][fF][cC]";
/// Configurable parts of the BIM basis ILS profile. Patterns use XSD regular-expression syntax
/// (implicitly anchored, as in IDS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IlsOptions {
    pub storey_name_pattern: String,
    pub file_name_pattern: String,
    /// File name of the model; without it the file-name rule is not checkable.
    pub file_name: Option<String>,
}
impl Default for IlsOptions {
    fn default() -> Self {
        Self {
            storey_name_pattern: DEFAULT_STOREY_PATTERN.into(),
            file_name_pattern: DEFAULT_FILE_NAME_PATTERN.into(),
            file_name: None,
        }
    }
}
/// Rule sets to evaluate. At least one must be selected.
#[derive(Debug, Clone, Default)]
pub struct ValidationOptions {
    pub ids_xml: Option<String>,
    /// Built-in model checks modelled on buildingSMART validation-service rules.
    pub model_checks: bool,
    pub ils: Option<IlsOptions>,
}

/// Validate a complete IFC STEP model against an IDS 1.0 document.
/// Admission limits: IFC 2 GiB, IDS 8 MiB, 5 million entities, nesting depth 64.
/// Inputs exceeding limits or containing malformed STEP records return an error.
pub fn validate(ifc_bytes: &[u8], ids_xml: &str) -> Result<ValidationReport, String> {
    validate_with_cancel(
        ifc_bytes,
        ids_xml,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

/// Cooperatively cancellable validation; checks between records and entities.
pub fn validate_with_cancel(
    ifc_bytes: &[u8],
    ids_xml: &str,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ValidationReport, String> {
    let options = ValidationOptions {
        ids_xml: Some(ids_xml.into()),
        ..Default::default()
    };
    validate_model(ifc_bytes, &options, cancel)
}

/// Evaluate the selected IDS and built-in rule sets over one parsed model. Specifications are
/// ordered IDS (document order), model checks, then ILS rules, each in a fixed order.
pub fn validate_model(
    ifc_bytes: &[u8],
    options: &ValidationOptions,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ValidationReport, String> {
    if options.ids_xml.is_none() && !options.model_checks && options.ils.is_none() {
        return Err("Select an IDS, the model checks or the BIM basis ILS profile".into());
    }
    let ids = options.ids_xml.as_deref().map(parse_ids).transpose()?;
    if let Some(ils) = &options.ils {
        ils::admit(ils)?;
    }
    let graph = graph::Graph::parse(ifc_bytes, cancel)?;
    let mut report = match &ids {
        Some(ids) => evaluator::validate(&graph, ids, cancel)?,
        None => ValidationReport {
            schema: graph.label.clone(),
            ids_title: String::new(),
            status: ValidationStatus::Passed,
            specifications: vec![],
            model: None,
        },
    };
    let model = checks::Model::new(&graph, cancel)?;
    if options.model_checks {
        report.specifications.extend(checks::run(&model)?);
    }
    if let Some(ils) = &options.ils {
        report.specifications.extend(ils::run(&model, ils)?);
    }
    report.model = Some(model.summary()?);
    report.status = report
        .specifications
        .iter()
        .filter(|s| s.severity == Severity::Error)
        .fold(ValidationStatus::Passed, |s, r| combine(s, r.status));
    Ok(report)
}

/// Check an XSD regular expression as accepted by [`IlsOptions`].
pub fn check_pattern(pattern: &str) -> Result<(), String> {
    values::pattern_match(pattern, "").map(|_| ())
}

fn parse_ids(ids_xml: &str) -> Result<openbim_ids::Ids, String> {
    if ids_xml.contains("<!DOCTYPE") || ids_xml.contains("<!ENTITY") {
        return Err("IDS DTDs and entity declarations are not allowed".into());
    }
    if ids_xml.len() > 8 * 1024 * 1024 {
        return Err("IDS exceeds the 8 MiB input limit".into());
    }
    openbim_ids::from_str(ids_xml).map_err(|e| format!("Invalid IDS: {e}"))
}

pub(crate) fn combine(a: ValidationStatus, b: ValidationStatus) -> ValidationStatus {
    use ValidationStatus as S;
    if a == S::NotCheckable || b == S::NotCheckable {
        S::NotCheckable
    } else if a == S::Failed || b == S::Failed {
        S::Failed
    } else {
        S::Passed
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_rules;
