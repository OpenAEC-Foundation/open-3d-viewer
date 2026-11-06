//! `sam-json` v1: the vendor-neutral exchange format of the structural analysis model (the
//! `.samjson` file, the C-ABI payload and the test fixtures). The prose specification is
//! `crates/bim-structural/docs/sam-json-v1.md`; the JSON Schema is
//! `crates/bim-structural/schema/sam-json-v1.schema.json`.

mod convert;
mod validate;
pub mod wire;

pub use convert::FORMAT;
pub use validate::{check, parse, validate, validate_block, Issue, Report, Rule, Severity};

use crate::model::{SamDocument, Shape};

/// Why a document could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum ReadError {
    /// The document breaks one or more rules.
    Invalid(Report),
    /// An inline result block could not be decoded.
    Block(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Invalid(r) => {
                let n = r.errors().count();
                write!(f, "invalid sam-json ({n} error{})", if n == 1 { "" } else { "s" })?;
                for i in r.errors().take(5) {
                    write!(f, "\n  {i}")?;
                }
                Ok(())
            }
            ReadError::Block(e) => write!(f, "result block: {e}"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads and validates a `sam-json` document. Named supports and releases are expanded through
/// the preset tables (with a diagnostic), plain ids of sections, materials, cases,
/// combinations, actions and result sets are shifted into `sam_id` form, section properties are
/// computed from the shapes, and inline result blocks are decoded. Warnings do not fail.
pub fn read(text: &str) -> Result<SamDocument, ReadError> {
    let doc = parse(text).map_err(ReadError::Invalid)?;
    let report = validate(&doc);
    if !report.is_ok() {
        return Err(ReadError::Invalid(report));
    }
    convert::document_in(&doc).map_err(ReadError::Block)
}

/// Writes a document as pretty-printed `sam-json`. `read(&write(&d)) == d` for every document
/// that `read` produced.
pub fn write(doc: &SamDocument) -> String {
    serde_json::to_string_pretty(&to_wire(doc)).expect("sam-json serialises")
}

/// The wire form of a document.
pub fn to_wire(doc: &SamDocument) -> wire::Document {
    convert::document_out(doc)
}

/// The document form of a validated wire document.
pub fn from_wire(doc: &wire::Document) -> Result<SamDocument, ReadError> {
    convert::document_in(doc).map_err(ReadError::Block)
}

pub(crate) fn convert_shape(s: &wire::Shape) -> Shape {
    convert::shape_in(s)
}

/// The JSON Schema of `sam-json` v1, generated from the serde types.
#[cfg(any(test, feature = "schema"))]
pub fn json_schema() -> serde_json::Value {
    let mut schema = schemars::schema_for!(wire::Document);
    schema.insert(
        "$id".into(),
        "https://github.com/OpenAEC-Foundation/open-3d-viewer/blob/main/crates/bim-structural/schema/sam-json-v1.schema.json".into(),
    );
    schema.insert("title".into(), "sam-json v1".into());
    sorted(serde_json::to_value(&schema).expect("schema serialises"))
}

/// Object keys in sorted order, whether or not serde_json preserves insertion order.
#[cfg(any(test, feature = "schema"))]
fn sorted(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/sam-json-v1.schema.json");

    /// The committed schema must equal the generated one. Run with `UPDATE_SCHEMA=1` to rewrite it.
    #[test]
    fn schema_is_not_stale() {
        let generated = serde_json::to_string_pretty(&json_schema()).unwrap() + "\n";
        if std::env::var_os("UPDATE_SCHEMA").is_some() {
            std::fs::write(SCHEMA_PATH, &generated).unwrap();
        }
        // Compared as JSON values, so line endings and key order do not matter.
        let text = std::fs::read_to_string(SCHEMA_PATH).unwrap_or_default();
        let committed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        assert!(
            committed == json_schema(),
            "{SCHEMA_PATH} is stale; regenerate with `UPDATE_SCHEMA=1 cargo test -p bim-structural schema_is_not_stale`"
        );
    }
}
