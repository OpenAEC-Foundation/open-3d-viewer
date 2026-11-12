//! Run with an output path to create a minimal interoperable validation archive.
use bim_bcf::{BcfArchive, NewTopic};

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Expected output .bcfzip path")?;
    let topic = NewTopic {
        id: "00000000-0000-4000-8000-000000000001".into(),
        title: "Validation finding".into(),
        status: "Open".into(),
        description: "Review the required property.".into(),
        component_guids: vec!["0ABCDEFGHIJKLMNOPQRSTU".into()],
        creation_date: "2026-11-05T00:00:00Z".into(),
        creation_author: "Open BIM Validator".into(),
    };
    let mut without_components = topic.clone();
    without_components.id = "00000000-0000-4000-8000-000000000002".into();
    without_components.component_guids.clear();
    let bytes = BcfArchive::from_topics(&[topic, without_components])?.write()?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}
