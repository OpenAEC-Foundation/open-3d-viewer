//! `sam-check model.samjson [blocks.samr...]`: checks a `sam-json` v1 document and its sidecar
//! result blocks against every conformance rule. Exit code 0 when there are no errors, 1 when
//! there are, 2 on a usage or I/O error.

use std::process::ExitCode;

use bim_structural::exchange;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("usage: sam-check model.samjson [blocks.samr...]");
        eprintln!("Checks a sam-json v1 document (schema, ids, references, member z, cardinal points,");
        eprintln!("DOFs, frames, sections, loads, inline and sidecar result blocks).");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&args[0]) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("sam-check: {}: {e}", args[0]);
            return ExitCode::from(2);
        }
    };
    let mut blocks = Vec::new();
    for path in &args[1..] {
        match std::fs::read(path) {
            Ok(b) => blocks.push((path.clone(), b)),
            Err(e) => {
                eprintln!("sam-check: {path}: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let report = exchange::check(&text, &blocks);
    for issue in &report.issues {
        println!("{issue}");
    }
    let errors = report.errors().count();
    let warnings = report.warnings().count();
    println!("{}: {errors} error(s), {warnings} warning(s){}", args[0], if blocks.is_empty() { String::new() } else { format!(", {} block file(s)", blocks.len()) });
    if errors == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
