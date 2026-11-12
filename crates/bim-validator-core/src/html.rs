//! Self-contained single-file HTML report: inline CSS, no scripts, fonts or external assets.
//! Output is a pure function of the report, so identical reports render identical bytes.
use crate::{RuleSource, Severity, SpecificationResult, ValidationReport, ValidationStatus as S};
use std::fmt::Write;

/// Finding rows per rule; the JSON and CSV exports always contain every finding.
const MAX_ROWS: usize = 5000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReportLanguage {
    #[default]
    English,
    Dutch,
}
impl ReportLanguage {
    fn t(self, en: &'static str, nl: &'static str) -> &'static str {
        match self {
            Self::English => en,
            Self::Dutch => nl,
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}
fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        "—".into()
    } else {
        format!("{:.1}%", 100.0 * part as f64 / whole as f64)
    }
}
/// (css class, label) for a specification's outcome, taking severity into account.
fn outcome(spec: &SpecificationResult, l: ReportLanguage) -> (&'static str, &'static str) {
    match (spec.status, spec.severity) {
        (S::Passed, _) => ("ok", l.t("Passed", "Geslaagd")),
        (S::Failed, Severity::Error) => ("err", l.t("Failed", "Afgekeurd")),
        (S::Failed, Severity::Warning) => ("warn", l.t("Warning", "Waarschuwing")),
        (S::NotCheckable, _) => ("nc", l.t("Not checkable", "Niet controleerbaar")),
    }
}
fn source(s: RuleSource, l: ReportLanguage) -> &'static str {
    match s {
        RuleSource::Ids => "IDS",
        RuleSource::ModelChecks => l.t("Model checks", "Modelcontroles"),
        RuleSource::Ils => "BIM basis ILS",
    }
}

const STYLE: &str = r#":root{--bg:#fff;--fg:#1d2329;--muted:#5d6873;--line:#dde2e7;--tile:#f4f6f8;--ok:#1e7a3c;--err:#b3261e;--warn:#9a6700;--nc:#5d6873}
@media (prefers-color-scheme:dark){:root{--bg:#16191d;--fg:#e6e9ec;--muted:#9aa4ae;--line:#2d333a;--tile:#1f2328;--ok:#5cc27e;--err:#f2867e;--warn:#e3b341;--nc:#9aa4ae}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:1200px;margin:0 auto;padding:24px 16px 48px}h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:32px 0 10px}h3{font-size:15px;margin:0}
.meta{color:var(--muted);margin:0 0 16px}.tiles{display:grid;grid-template-columns:repeat(auto-fill,minmax(150px,1fr));gap:8px}
.tile{background:var(--tile);border-radius:8px;padding:10px 12px}.tile b{display:block;font-size:20px;font-variant-numeric:tabular-nums}.tile span{color:var(--muted);font-size:12px}
.wrap{overflow-x:auto}table{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}th,td{text-align:left;padding:5px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{font-size:12px;color:var(--muted);font-weight:600}td.n{text-align:right}code{font:12px ui-monospace,Consolas,monospace;word-break:break-all}
.badge{display:inline-block;border-radius:10px;padding:0 8px;font-size:12px;font-weight:600;border:1px solid currentColor}.ok{color:var(--ok)}.err{color:var(--err)}.warn{color:var(--warn)}.nc{color:var(--nc)}
details{border:1px solid var(--line);border-radius:8px;margin:8px 0;padding:8px 12px}summary{cursor:pointer}summary h3{display:inline}ul{margin:6px 0 8px;padding-left:20px}a{color:inherit}"#;

pub fn report_html(report: &ValidationReport, title: &str, l: ReportLanguage) -> String {
    let mut h = String::new();
    let lang = l.t("en", "nl");
    let heading = if title.is_empty() {
        l.t("Validation report", "Validatierapport")
    } else {
        title
    };
    let _ = write!(
        h,
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{STYLE}</style></head><body><main>",
        esc(heading)
    );
    let overall = match report.status {
        S::Passed => ("ok", l.t("Passed", "Geslaagd")),
        S::Failed => ("err", l.t("Failed", "Afgekeurd")),
        S::NotCheckable => ("nc", l.t("Not checkable", "Niet controleerbaar")),
    };
    let _ = write!(
        h,
        "<h1>{}</h1><p class=\"meta\">{} {}{} · <span class=\"badge {}\">{}</span></p>",
        esc(heading),
        l.t("Schema", "Schema"),
        esc(&report.schema),
        if report.ids_title.is_empty() {
            String::new()
        } else {
            format!(" · IDS {}", esc(&report.ids_title))
        },
        overall.0,
        overall.1
    );

    let specs = &report.specifications;
    let count = |f: &dyn Fn(&SpecificationResult) -> bool| specs.iter().filter(|s| f(s)).count();
    let tiles = [
        (
            count(&|s| s.status == S::Failed && s.severity == Severity::Error),
            l.t("rules failed (error)", "regels afgekeurd (fout)"),
        ),
        (
            count(&|s| s.status == S::Failed && s.severity == Severity::Warning),
            l.t("rules with warnings", "regels met waarschuwing"),
        ),
        (
            count(&|s| s.status == S::Passed),
            l.t("rules passed", "regels geslaagd"),
        ),
        (
            count(&|s| s.status == S::NotCheckable),
            l.t("rules not checkable", "regels niet controleerbaar"),
        ),
        (
            specs.iter().map(|s| s.findings.len()).sum(),
            l.t("findings", "bevindingen"),
        ),
    ];
    h.push_str("<h2>");
    h.push_str(l.t("Summary", "Samenvatting"));
    h.push_str("</h2><div class=\"tiles\">");
    for (value, label) in tiles {
        let _ = write!(
            h,
            "<div class=\"tile\"><b>{value}</b><span>{label}</span></div>"
        );
    }
    h.push_str("</div>");

    if let Some(m) = &report.model {
        h.push_str("<h2>");
        h.push_str(l.t("Model quality", "Modelkwaliteit"));
        h.push_str("</h2>");
        let mut meta = vec![];
        if let Some(p) = &m.project_name {
            meta.push(format!("{} {}", l.t("Project", "Project"), esc(p)));
        }
        meta.push(format!(
            "{} {}",
            l.t("length unit", "lengte-eenheid"),
            esc(m.length_unit.as_deref().unwrap_or("—"))
        ));
        meta.push(format!(
            "{} {}",
            m.entity_count,
            l.t("STEP entities", "STEP-entiteiten")
        ));
        let _ = write!(
            h,
            "<p class=\"meta\">{}</p><div class=\"tiles\">",
            meta.join(" · ")
        );
        let e = m.element_count;
        let _ = write!(
            h,
            "<div class=\"tile\"><b>{e}</b><span>{}</span></div>",
            l.t("elements", "elementen")
        );
        for (value, label) in [
            (m.proxy_count, l.t("proxies", "proxy's")),
            (m.without_storey, l.t("without storey", "zonder bouwlaag")),
            (
                m.without_material,
                l.t("without material", "zonder materiaal"),
            ),
            (m.without_nlsfb, l.t("without NL-SfB", "zonder NL-SfB")),
            (m.without_type, l.t("without type", "zonder type")),
        ] {
            let _ = write!(
                h,
                "<div class=\"tile\"><b>{value}</b><span>{label} · {}</span></div>",
                percent(value, e)
            );
        }
        h.push_str("</div>");
        if !m.storeys.is_empty() {
            let _ = write!(
                h,
                "<h2>{}</h2><div class=\"wrap\"><table><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>GlobalId</th></tr></thead><tbody>",
                l.t("Storeys by elevation", "Bouwlagen op hoogte"),
                l.t("Name", "Naam"),
                l.t("Elevation", "Hoogte"),
                l.t("Elements", "Elementen"),
            );
            for s in &m.storeys {
                let _ = write!(
                    h,
                    "<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td><code>{}</code></td></tr>",
                    esc(s.name.as_deref().unwrap_or("—")),
                    s.elevation.map_or("—".into(), num),
                    s.element_count,
                    esc(s.global_id.as_deref().unwrap_or(""))
                );
            }
            h.push_str("</tbody></table></div>");
        }
    }

    let _ = write!(
        h,
        "<h2>{}</h2><div class=\"wrap\"><table><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        l.t("Rules", "Regels"),
        l.t("Source", "Bron"),
        l.t("Code", "Code"),
        l.t("Rule", "Regel"),
        l.t("Severity", "Ernst"),
        l.t("Result", "Resultaat"),
        l.t("Checked", "Gecontroleerd"),
        l.t("Passed", "Geslaagd"),
        l.t("Failed", "Afgekeurd"),
    );
    for (i, s) in specs.iter().enumerate() {
        let (class, label) = outcome(s, l);
        let has_detail = !s.findings.is_empty() || !s.reasons.is_empty();
        let name = if has_detail {
            format!("<a href=\"#rule-{i}\">{}</a>", esc(&s.name))
        } else {
            esc(&s.name)
        };
        let _ = write!(
            h,
            "<tr><td>{}</td><td><code>{}</code></td><td>{name}</td><td>{}</td><td class=\"{class}\">{label}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
            source(s.source, l),
            esc(s.identifier.as_deref().unwrap_or("")),
            match s.severity {
                Severity::Error => l.t("error", "fout"),
                Severity::Warning => l.t("warning", "waarschuwing"),
            },
            s.applicable_count,
            s.passed_count,
            s.failed_count
        );
    }
    h.push_str("</tbody></table></div>");

    let detailed: Vec<_> = specs
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.findings.is_empty() || !s.reasons.is_empty())
        .collect();
    if !detailed.is_empty() {
        let _ = write!(
            h,
            "<h2>{}</h2>",
            l.t("Findings per rule", "Bevindingen per regel")
        );
    }
    for (i, s) in detailed {
        let (class, label) = outcome(s, l);
        let open = if s.status == S::Passed { "" } else { " open" };
        let _ = write!(
            h,
            "<details id=\"rule-{i}\"{open}><summary><h3>{} {}</h3> <span class=\"badge {class}\">{label}</span> <span class=\"meta\">{} {}</span></summary>",
            esc(s.identifier.as_deref().unwrap_or("")),
            esc(&s.name),
            s.findings.len(),
            l.t("findings", "bevindingen")
        );
        if !s.reasons.is_empty() {
            h.push_str("<ul>");
            for r in &s.reasons {
                let _ = write!(h, "<li>{}</li>", esc(r));
            }
            h.push_str("</ul>");
        }
        if !s.findings.is_empty() {
            let _ = write!(
                h,
                "<div class=\"wrap\"><table><thead><tr><th>#</th><th>GlobalId</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
                l.t("Class", "Klasse"),
                l.t("Name", "Naam"),
                l.t("Storey", "Bouwlaag"),
                l.t("Reason", "Reden")
            );
            for f in s.findings.iter().take(MAX_ROWS) {
                let status = match f.status {
                    S::NotCheckable => format!(
                        "<span class=\"nc\">{}:</span> ",
                        l.t("Not checkable", "Niet controleerbaar")
                    ),
                    _ => String::new(),
                };
                let _ = write!(
                    h,
                    "<tr><td class=\"n\">{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{status}{}</td></tr>",
                    f.express_id,
                    esc(f.global_id.as_deref().unwrap_or("")),
                    esc(&f.entity_type),
                    esc(f.name.as_deref().unwrap_or("")),
                    esc(f.storey.as_deref().unwrap_or("—")),
                    esc(&f.reason)
                );
            }
            h.push_str("</tbody></table></div>");
            if s.findings.len() > MAX_ROWS {
                let _ = write!(
                    h,
                    "<p class=\"meta\">{} {MAX_ROWS} / {}; {}</p>",
                    l.t("Showing", "Getoond"),
                    s.findings.len(),
                    l.t(
                        "the JSON and CSV exports contain every finding.",
                        "de JSON- en CSV-export bevatten alle bevindingen."
                    )
                );
            }
        }
        h.push_str("</details>");
    }
    let _ = writeln!(
        h,
        "<p class=\"meta\">Open BIM Validator · {}</p></main></body></html>",
        l.t(
            "PJS, SPS, OJT, PSE and GRP codes refer to buildingSMART validation-service rules; IfcRoot.UR1 is an IFC schema rule; OBV- and ILS- codes are built-in checks.",
            "PJS-, SPS-, OJT-, PSE- en GRP-codes verwijzen naar regels van de buildingSMART-validatieservice; IfcRoot.UR1 is een IFC-schemaregel; OBV- en ILS-codes zijn ingebouwde controles."
        )
    );
    h
}
