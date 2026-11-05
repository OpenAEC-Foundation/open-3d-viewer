//! Native IDS evaluation over the complete IFC STEP graph.
//! Unsupported semantics produce `not_checkable`, never a successful check.
mod evaluator;
mod graph;
mod values;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Passed,
    Failed,
    NotCheckable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub schema: String,
    pub ids_title: String,
    pub status: ValidationStatus,
    pub specifications: Vec<SpecificationResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecificationResult {
    pub name: String,
    pub identifier: Option<String>,
    pub status: ValidationStatus,
    pub applicable_count: usize,
    pub passed_count: usize,
    pub failed_count: usize,
    pub findings: Vec<Finding>,
    pub reasons: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub express_id: u32,
    pub global_id: Option<String>,
    pub entity_type: String,
    pub facet: String,
    pub status: ValidationStatus,
    pub reason: String,
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
    if ids_xml.contains("<!DOCTYPE") || ids_xml.contains("<!ENTITY") {
        return Err("IDS DTDs and entity declarations are not allowed".into());
    }
    if ids_xml.len() > 8 * 1024 * 1024 {
        return Err("IDS exceeds the 8 MiB input limit".into());
    }
    let ids = openbim_ids::from_str(ids_xml).map_err(|e| format!("Invalid IDS: {e}"))?;
    let graph = graph::Graph::parse(ifc_bytes, cancel)?;
    evaluator::validate(&graph, &ids, cancel)
}

#[cfg(test)]
mod tests;
