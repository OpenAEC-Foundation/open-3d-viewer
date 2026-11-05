//! Headless IDS, model-check and BIM basis ILS validation using the same engine as the desktop
//! workspace.
use crate::validation::BcfGrouping;
use std::{ffi::OsString, path::PathBuf, sync::atomic::AtomicBool};

const VALUE_FLAGS: &[&str] = &[
    "--validate",
    "--ids",
    "--report",
    "--bcf",
    "--html",
    "--storey-pattern",
    "--bcf-group",
    "--lang",
];
const SWITCHES: &[&str] = &["--checks", "--ils"];
const HELP: &str = "Open BIM Validator
--validate MODEL.ifc [--ids RULES.ids] [--checks] [--ils] [--storey-pattern XSD-PATTERN]
           [--report RESULT.json|RESULT.csv] [--html REPORT.html] [--lang en|nl]
           [--bcf ISSUES.bcfzip] [--bcf-group finding|rule|rule-storey]
  --ids             IDS 1.0 requirements
  --checks          built-in model checks (buildingSMART rule codes PJS/SPS/OJT/PSE/GRP, OBV-*)
  --ils             BIM basis ILS (NL) profile: storey names/elevations, NL-SfB, material,
                    LoadBearing, IsExternal, FireRating, proxies, file name
  --storey-pattern  ILS storey name pattern (XSD regex), default -?[0-9]{2}[a-zA-Z]? \\S.*
  --html            self-contained HTML report (--lang selects English or Dutch labels)
  --bcf-group       one BCF topic per finding (default), per rule, or per rule and storey
At least one of --ids, --checks or --ils is required. Without --report, JSON goes to stdout.
Exit codes: 0 passed, 1 failed, 2 not checkable, 3 input/output error. Warnings never fail.
Without --validate, starts the desktop viewer.";

#[derive(Debug, PartialEq)]
struct Options {
    ifc: PathBuf,
    ids: Option<PathBuf>,
    report: Option<PathBuf>,
    bcf: Option<PathBuf>,
    html: Option<PathBuf>,
    checks: bool,
    ils: bool,
    storey_pattern: Option<String>,
    bcf_group: BcfGrouping,
    dutch: bool,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut values: std::collections::BTreeMap<&str, OsString> = Default::default();
    let (mut checks, mut ils) = (false, false);
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let name = flag.to_str().unwrap_or_default();
        if let Some(switch) = SWITCHES.iter().find(|s| **s == name) {
            let target = if *switch == "--checks" {
                &mut checks
            } else {
                &mut ils
            };
            if *target {
                return Err(format!("Repeated argument: {name}"));
            }
            *target = true;
            continue;
        }
        let Some(key) = VALUE_FLAGS.iter().find(|f| **f == name) else {
            return Err(format!("Unknown argument: {}", flag.to_string_lossy()));
        };
        let value = iter
            .next()
            .ok_or_else(|| format!("Missing value for {name}"))?;
        if value.to_string_lossy().starts_with("--") {
            return Err("Missing argument value".into());
        }
        if values.insert(key, value.clone()).is_some() {
            return Err(format!("Repeated argument: {name}"));
        }
    }
    let mut path = |flag: &str| values.remove(flag).map(PathBuf::from);
    let ifc = path("--validate").ok_or("--validate requires an IFC path")?;
    let (ids, report, bcf, html) = (
        path("--ids"),
        path("--report"),
        path("--bcf"),
        path("--html"),
    );
    let text = |flag: &str| values.get(flag).map(|v| v.to_string_lossy().into_owned());
    if ids.is_none() && !checks && !ils {
        return Err("Select rules with --ids, --checks and/or --ils".into());
    }
    let storey_pattern = text("--storey-pattern");
    if storey_pattern.is_some() && !ils {
        return Err("--storey-pattern requires --ils".into());
    }
    let bcf_group = match text("--bcf-group") {
        None => BcfGrouping::Finding,
        Some(_) if bcf.is_none() => return Err("--bcf-group requires --bcf".into()),
        Some(v) => {
            BcfGrouping::parse(&v).ok_or("--bcf-group must be finding, rule or rule-storey")?
        }
    };
    let dutch = match text("--lang").as_deref() {
        None | Some("en") => false,
        Some("nl") => true,
        Some(_) => return Err("--lang must be en or nl".into()),
    };
    Ok(Options {
        ifc,
        ids,
        report,
        bcf,
        html,
        checks,
        ils,
        storey_pattern,
        bcf_group,
        dutch,
    })
}

fn execute(options: Options) -> Result<i32, String> {
    use crate::validation::{report_bcf, report_csv, report_html, validate_paths};
    use bim_validator_core::{IlsOptions, ValidationStatus};
    // Validate output formats and patterns before starting an expensive model evaluation.
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
    if let Some(pattern) = &options.storey_pattern {
        bim_validator_core::check_pattern(pattern)
            .map_err(|e| format!("Invalid --storey-pattern: {e}"))?;
    }
    for output in [&options.report, &options.bcf, &options.html]
        .into_iter()
        .flatten()
    {
        if output.exists() {
            return Err(format!("Output already exists: {}", output.display()));
        }
    }
    let ils = options.ils.then(|| {
        let mut o = IlsOptions::default();
        if let Some(pattern) = &options.storey_pattern {
            o.storey_name_pattern = pattern.clone();
        }
        o
    });
    let report = validate_paths(
        &options.ifc,
        options.ids.as_deref(),
        options.checks,
        ils,
        &AtomicBool::new(false),
    )?;
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
    if let Some(path) = options.html {
        let language = if options.dutch {
            openaec_ui::Language::Dutch
        } else {
            openaec_ui::Language::English
        };
        write_new(
            &path,
            report_html(&report, &options.ifc, language).as_bytes(),
        )?;
    }
    if let Some(path) = options.bcf {
        write_new(
            &path,
            &report_bcf(&report, &options.ifc.to_string_lossy(), options.bcf_group)?.write()?,
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
    let cli = |a: &OsString| {
        a == "--help"
            || VALUE_FLAGS.iter().any(|flag| a == flag)
            || SWITCHES.iter().any(|flag| a == flag)
    };
    #[cfg(windows)]
    if args.iter().any(cli) {
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
        println!("{HELP}");
        return Some(0);
    }
    if !args.iter().any(cli) {
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
            vec!["--validate", "a", "--checks", "--checks"],
            vec!["--validate", "a", "--checks", "--storey-pattern", "x"],
            vec!["--validate", "a", "--checks", "--bcf-group", "rule"],
            vec![
                "--validate",
                "a",
                "--checks",
                "--bcf",
                "b",
                "--bcf-group",
                "x",
            ],
            vec!["--validate", "a", "--ils", "--lang", "de"],
        ] {
            assert!(parse(&args(&values)).is_err(), "{values:?}");
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
        assert!(!options.checks && !options.ils);
    }
    #[test]
    fn built_in_rule_sets_do_not_need_an_ids() {
        let options = parse(&args(&[
            "--checks",
            "--validate",
            "m.ifc",
            "--ils",
            "--storey-pattern",
            "-?[0-9]{2} .*",
            "--html",
            "r.html",
            "--lang",
            "nl",
            "--bcf",
            "i.bcfzip",
            "--bcf-group",
            "rule-storey",
        ]))
        .unwrap();
        assert!(options.checks && options.ils && options.dutch);
        assert_eq!(options.ids, None);
        assert_eq!(options.storey_pattern.as_deref(), Some("-?[0-9]{2} .*"));
        assert_eq!(options.html, Some(PathBuf::from("r.html")));
        assert_eq!(options.bcf_group, BcfGrouping::RuleStorey);
    }
}
