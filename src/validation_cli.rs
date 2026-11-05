//! Headless IDS validation using the same engine as the desktop workspace.
use std::{ffi::OsString, path::PathBuf, sync::atomic::AtomicBool};

#[derive(Debug, PartialEq)]
struct Options {
    ifc: PathBuf,
    ids: PathBuf,
    report: Option<PathBuf>,
    bcf: Option<PathBuf>,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut ifc = None;
    let mut ids = None;
    let mut report = None;
    let mut bcf = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let target = match flag.to_str() {
            Some("--validate") => &mut ifc,
            Some("--ids") => &mut ids,
            Some("--report") => &mut report,
            Some("--bcf") => &mut bcf,
            _ => return Err(format!("Unknown argument: {}", flag.to_string_lossy())),
        };
        if target.is_some() {
            return Err(format!("Repeated argument: {}", flag.to_string_lossy()));
        }
        let value = iter
            .next()
            .ok_or_else(|| format!("Missing value for {}", flag.to_string_lossy()))?;
        if value.to_string_lossy().starts_with("--") {
            return Err("Missing argument value".into());
        }
        *target = Some(PathBuf::from(value));
    }
    Ok(Options {
        ifc: ifc.ok_or("--validate requires an IFC path")?,
        ids: ids.ok_or("--ids requires an IDS path")?,
        report,
        bcf,
    })
}

fn execute(options: Options) -> Result<i32, String> {
    use crate::validation::{report_bcf, report_csv, validate_paths};
    use bim_validator_core::ValidationStatus;
    // Validate output formats before starting an expensive model evaluation.
    let csv = match options
        .report
        .as_ref()
        .and_then(|p| p.extension())
        .and_then(|s| s.to_str())
    {
        None if options.report.is_none() => false,
        Some("json") => false,
        Some("csv") => true,
        _ => return Err("--report must end in .json or .csv".into()),
    };
    for output in [&options.report, &options.bcf].into_iter().flatten() {
        if output.exists() {
            return Err(format!("Output already exists: {}", output.display()));
        }
    }
    let report = validate_paths(&options.ifc, &options.ids, &AtomicBool::new(false))?;
    let json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    if let Some(path) = options.report {
        write_new(
            &path,
            &if csv {
                report_csv(&report).into_bytes()
            } else {
                json
            },
        )?;
    } else {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(&json)
            .map_err(|e| e.to_string())?;
    }
    if let Some(path) = options.bcf {
        write_new(
            &path,
            &report_bcf(&report, &options.ifc.to_string_lossy())?.write()?,
        )?;
    }
    Ok(match report.status {
        ValidationStatus::Passed => 0,
        ValidationStatus::Failed => 1,
        ValidationStatus::NotCheckable => 2,
    })
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Return None for normal desktop invocation; otherwise return a process exit code.
pub fn run() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(windows)]
    if args.iter().any(|a| {
        ["--help", "--validate", "--ids", "--report", "--bcf"]
            .iter()
            .any(|flag| a == flag)
    }) {
        // The desktop executable uses the GUI subsystem; connect CLI output to its caller.
        unsafe {
            use windows::Win32::System::Console::{
                AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
                STD_OUTPUT_HANDLE,
            };
            let output_present = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid());
            let error_present = GetStdHandle(STD_ERROR_HANDLE).is_ok_and(|h| !h.is_invalid());
            if !output_present && !error_present {
                let _ = AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
    if args.iter().any(|a| a == "--help") {
        println!("Open BIM Validator\n--validate MODEL.ifc --ids RULES.ids [--report RESULT.json|RESULT.csv] [--bcf ISSUES.bcfzip]\nExit codes: 0 passed, 1 failed, 2 not checkable, 3 input/output error.\nWithout --validate, starts the desktop viewer.");
        return Some(0);
    }
    if !args
        .iter()
        .any(|a| a == "--validate" || a == "--ids" || a == "--report" || a == "--bcf")
    {
        return None;
    }
    Some(match parse(&args).and_then(execute) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            3
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn rejects_incomplete_or_ambiguous_invocations() {
        for values in [
            vec!["--validate", "a.ifc"],
            vec!["--validate", "--ids", "a.ids"],
            vec!["--validate", "a", "--ids", "b", "--ids", "c"],
        ] {
            assert!(parse(&args(&values)).is_err());
        }
    }
    #[test]
    fn preserves_paths_with_spaces() {
        let options = parse(&args(&[
            "--validate",
            "a b.ifc",
            "--ids",
            "c d.ids",
            "--report",
            "out.csv",
        ]))
        .unwrap();
        assert_eq!(options.ifc, PathBuf::from("a b.ifc"));
        assert_eq!(options.report, Some(PathBuf::from("out.csv")));
    }
}
