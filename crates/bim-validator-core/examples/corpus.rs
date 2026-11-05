//! Run paired pass-/fail- IFC+IDS cases from an externally supplied test directory.
use bim_validator_core::{validate, ValidationStatus};
use std::{env, fs, path::Path};
fn walk(path: &Path, results: &mut Vec<serde_json::Value>) {
    let mut paths: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(&p, results);
            continue;
        }
        if p.extension().is_none_or(|e| e != "ifc") {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy();
        let expected = if name.starts_with("pass-") {
            ValidationStatus::Passed
        } else if name.starts_with("fail-") {
            ValidationStatus::Failed
        } else {
            continue;
        };
        let ids = p.with_extension("ids");
        if !ids.exists() {
            continue;
        }
        let result = validate(&fs::read(&p).unwrap(), &fs::read_to_string(ids).unwrap());
        match result {Ok(report)=>results.push(serde_json::json!({"case":name,"expected":expected,"actual":report.status,"matched":report.status==expected,"report":report})),Err(error)=>results.push(serde_json::json!({"case":name,"expected":expected,"actual":"error","matched":false,"error":error}))}
    }
}
fn main() {
    let root = env::args().nth(1).expect("Pass the corpus test directory");
    let mut results = vec![];
    walk(Path::new(&root), &mut results);
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
