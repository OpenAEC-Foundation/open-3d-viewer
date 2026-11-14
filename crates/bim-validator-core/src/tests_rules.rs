//! Built-in model checks, BIM basis ILS and HTML report on hand-written STEP snippets.
use super::*;
use std::sync::atomic::AtomicBool;

/// Replace `(G,` in each record with a valid, unique GlobalId derived from its express ID.
fn model_in(schema: &str, records: &str) -> Vec<u8> {
    let records: Vec<String> = records
        .lines()
        .map(|line| {
            let line = line.trim();
            let id: u32 = line
                .strip_prefix('#')
                .and_then(|s| s.split('=').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            line.replacen("(G,", &format!("('{id:022}',"), 1)
        })
        .collect();
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n{}\nENDSEC;\nEND-ISO-10303-21;",
        records.join("\n")
    )
    .into_bytes()
}
fn model(records: &str) -> Vec<u8> {
    model_in("IFC4", records)
}
const UNITS: &str = "#1=IFCPROJECT(G,$,'Proj',$,$,'Projectnaam',$,$,#2);
#2=IFCUNITASSIGNMENT((#3,#4,#5,#6));
#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#4=IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.);
#5=IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.);
#6=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);";
const SPATIAL: &str = "#10=IFCSITE(G,$,'Site',$,$,$,$,$,.ELEMENT.,$,$,$,$,$);
#11=IFCBUILDING(G,$,'Building',$,$,$,$,$,.ELEMENT.,$,$,$);
#12=IFCBUILDINGSTOREY(G,$,'00 begane grond',$,$,$,$,$,.ELEMENT.,0.);
#13=IFCBUILDINGSTOREY(G,$,'01 eerste verdieping',$,$,$,$,$,.ELEMENT.,3000.);
#20=IFCRELAGGREGATES(G,$,$,$,#1,(#10));
#21=IFCRELAGGREGATES(G,$,$,$,#10,(#11));
#22=IFCRELAGGREGATES(G,$,$,$,#11,(#13,#12));";
const WALL: &str = "#30=IFCWALL(G,$,'Wall',$,$,$,$,$,.NOTDEFINED.);
#31=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#30),#12);
#32=IFCWALLTYPE(G,$,'WT',$,$,(#40),$,$,$,.SOLIDWALL.);
#33=IFCRELDEFINESBYTYPE(G,$,$,$,(#30),#32);
#34=IFCMATERIAL('Beton',$,$);
#35=IFCRELASSOCIATESMATERIAL(G,$,$,$,(#30),#34);
#36=IFCCLASSIFICATION($,$,$,'NL-SfB',$,$,$);
#37=IFCCLASSIFICATIONREFERENCE($,'21.12','Buitenwanden',#36,$,$);
#38=IFCRELASSOCIATESCLASSIFICATION(G,$,$,$,(#30),#37);
#40=IFCPROPERTYSET(G,$,'Pset_WallCommon',$,(#41,#42,#43));
#41=IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$);
#42=IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$);
#43=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('60'),$);";
fn clean() -> String {
    format!("{UNITS}\n{SPATIAL}\n{WALL}")
}
fn run(bytes: &[u8], checks: bool, ils: Option<IlsOptions>) -> ValidationReport {
    let options = ValidationOptions {
        ids_xml: None,
        model_checks: checks,
        ils,
        rooms: None,
    };
    validate_model(bytes, &options, &AtomicBool::new(false)).unwrap()
}
fn spec<'a>(report: &'a ValidationReport, code: &str) -> &'a SpecificationResult {
    report
        .specifications
        .iter()
        .find(|s| s.identifier.as_deref() == Some(code))
        .unwrap_or_else(|| panic!("missing rule {code}"))
}
fn failed_ids(report: &ValidationReport, code: &str) -> Vec<u32> {
    spec(report, code)
        .findings
        .iter()
        .map(|f| f.express_id)
        .collect()
}

#[test]
fn clean_model_passes_every_model_check_and_ils_rule() {
    let report = run(&model(&clean()), true, Some(IlsOptions::default()));
    assert_eq!(report.status, ValidationStatus::Passed);
    for s in &report.specifications {
        let expected = if s.identifier.as_deref() == Some("ILS-FILE") {
            ValidationStatus::NotCheckable
        } else {
            ValidationStatus::Passed
        };
        assert_eq!(s.status, expected, "{:?}: {:?}", s.identifier, s.findings);
    }
    assert_eq!(spec(&report, "SPS007").applicable_count, 5);
    assert!(spec(&report, "OBV-UNIT01").reasons[0].contains("MILLIMETRE"));
    assert!(report
        .specifications
        .iter()
        .all(|s| s.source != RuleSource::Ids));
}

#[test]
fn global_id_format_and_uniqueness() {
    let records = clean()
        .replace("#30=IFCWALL(G,", "#30=IFCWALL('4bad',")
        .replace(
            "#34=IFCMATERIAL",
            "#39=IFCWALL('0000000000000000000012',$,$,$,$,$,$,$,.NOTDEFINED.);\n#34=IFCMATERIAL",
        );
    let report = run(&model(&records), true, None);
    assert_eq!(failed_ids(&report, "PJS003"), vec![30]);
    // #12 carries the generated id 0000000000000000000012 as well.
    assert_eq!(failed_ids(&report, "IfcRoot.UR1"), vec![12, 39]);
    assert!(spec(&report, "IfcRoot.UR1").findings[0]
        .reason
        .contains("#39"));
    assert_eq!(report.status, ValidationStatus::Failed);
}

#[test]
fn project_presence_and_spatial_breakdown() {
    let records = clean().replace(
        "#22=IFCRELAGGREGATES(G,$,$,$,#11,(#13,#12));",
        "#22=IFCRELAGGREGATES(G,$,$,$,#11,(#13));\n#23=IFCRELAGGREGATES(G,$,$,$,#10,(#12));\n#9=IFCPROJECT(G,$,'Second',$,$,$,$,$,$);",
    );
    let report = run(&model(&records), true, None);
    assert_eq!(failed_ids(&report, "SPS002"), vec![12]);
    assert!(spec(&report, "SPS002").findings[0]
        .reason
        .contains("IFCSITE"));
    assert_eq!(failed_ids(&report, "PJS101"), vec![1, 9]);
    // Units are not resolvable with two projects: not checkable dominates the error result.
    assert_eq!(
        spec(&report, "OBV-UNIT01").status,
        ValidationStatus::NotCheckable
    );
    assert_eq!(report.status, ValidationStatus::NotCheckable);

    let report = run(&model(UNITS), true, None);
    assert_eq!(spec(&report, "SPS001").status, ValidationStatus::Failed);
    assert_eq!(spec(&report, "SPS001").severity, Severity::Warning);
    let report = run(
        &model_in("IFC2X3", "#1=IFCPROJECT(G,$,'P',$,$,$,$,$,$);"),
        true,
        None,
    );
    assert_eq!(spec(&report, "SPS001").severity, Severity::Error);
    assert_eq!(spec(&report, "PJS101").status, ValidationStatus::Passed);
}

#[test]
fn spatial_containment_rules() {
    let extra = "#50=IFCWALL(G,$,'Loose',$,$,$,$,$,.NOTDEFINED.);
#51=IFCELEMENTASSEMBLY(G,$,'Assembly',$,$,$,$,$,.NOTDEFINED.,.NOTDEFINED.);
#52=IFCBEAM(G,$,'Part',$,$,$,$,$,.NOTDEFINED.);
#53=IFCRELAGGREGATES(G,$,$,$,#51,(#52));
#54=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#51,#52,#55),#13);
#55=IFCSPACE(G,$,'Room',$,$,$,$,$,.ELEMENT.,.NOTDEFINED.,$);
#56=IFCOPENINGELEMENT(G,$,$,$,$,$,$,$,.OPENING.);
#57=IFCRELVOIDSELEMENT(G,$,$,$,#30,#56);";
    let report = run(&model(&format!("{}\n{extra}", clean())), true, None);
    assert_eq!(failed_ids(&report, "SPS007"), vec![50, 52, 55]);
    let s = spec(&report, "SPS007");
    assert!(s.findings[0].reason.contains("no storey"));
    assert_eq!(s.findings[0].storey, None);
    assert_eq!(
        s.findings[1].storey.as_deref(),
        Some("01 eerste verdieping")
    );
    assert!(s.findings[1].reason.contains("Aggregated"));
    // The space is also missing from the spatial breakdown.
    assert_eq!(failed_ids(&report, "SPS002"), vec![55]);
    assert_eq!(report.model.as_ref().unwrap().without_storey, 1);
}

#[test]
fn predefined_type_rules() {
    let records = clean()
        .replace(
            "#30=IFCWALL(G,$,'Wall',$,$,$,$,$,.NOTDEFINED.);",
            "#30=IFCWALL(G,$,'Wall',$,$,$,$,$,.PARTITIONING.);",
        )
        + "\n#60=IFCSLAB(G,$,'Slab',$,$,$,$,$,.USERDEFINED.);\n#61=IFCSLAB(G,$,'Slab',$,'Kanaalplaat',$,$,$,.USERDEFINED.);\n#62=IFCSLABTYPE(G,$,'ST',$,$,$,$,$,$,.USERDEFINED.);\n#63=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#60,#61),#12);";
    let report = run(&model(&records), true, None);
    assert_eq!(failed_ids(&report, "OJT001"), vec![30, 60, 62]);
    let s = spec(&report, "OJT001");
    assert!(s.findings[0].reason.contains("overrides IFCWALLTYPE #32"));
    assert!(s.findings[1].reason.contains("ObjectType"));
    assert!(s.findings[2].reason.contains("ElementType"));
}

#[test]
fn units_and_conversion_based_units() {
    let foot = |name: &str, factor: &str| {
        clean().replace(
            "#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);",
            &format!("#3=IFCCONVERSIONBASEDUNIT(#7,.LENGTHUNIT.,'{name}',#8);\n#7=IFCDIMENSIONALEXPONENTS(1,0,0,0,0,0,0);\n#8=IFCMEASUREWITHUNIT(IFCLENGTHMEASURE({factor}),#9);\n#9=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);"),
        )
    };
    let report = run(&model(&foot("FOOT", "304.8")), true, None);
    assert_eq!(spec(&report, "PJS001").status, ValidationStatus::Passed);
    assert_eq!(spec(&report, "PJS001").applicable_count, 1);
    assert_eq!(
        report.model.as_ref().unwrap().length_unit.as_deref(),
        Some("FOOT")
    );
    let report = run(&model(&foot("feet", "300.")), true, None);
    let reasons: Vec<_> = spec(&report, "PJS001")
        .findings
        .iter()
        .map(|f| f.reason.as_str())
        .collect();
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert!(reasons[0].contains("not a recognised"));
    let report = run(&model(&foot("foot", "300.")), true, None);
    assert!(spec(&report, "PJS001").findings[0]
        .reason
        .contains("expected 0.3048"));

    let missing = clean().replace(
        "#2=IFCUNITASSIGNMENT((#3,#4,#5,#6));",
        "#2=IFCUNITASSIGNMENT((#4));",
    );
    let report = run(&model(&missing), true, None);
    assert_eq!(spec(&report, "OBV-UNIT01").status, ValidationStatus::Failed);
    assert_eq!(spec(&report, "OBV-UNIT02").findings.len(), 2);
    assert_eq!(spec(&report, "OBV-UNIT02").severity, Severity::Warning);
}

#[test]
fn property_set_prefix_casing() {
    let records = clean()
        + "\n#70=IFCPROPERTYSET(G,$,'PSet_Custom',$,(#73));\n#71=IFCPROPERTYSET(G,$,'Bedrijf_Data',$,(#73));\n#72=IFCELEMENTQUANTITY(G,$,'QTO_Wall',$,$,(#74));\n#73=IFCPROPERTYSINGLEVALUE('A',$,IFCLABEL('x'),$);\n#74=IFCQUANTITYLENGTH('Length',$,$,1.,$);\n#75=IFCELEMENTQUANTITY(G,$,'Qto_WallBaseQuantities',$,$,(#74));";
    let report = run(&model(&records), true, None);
    assert_eq!(failed_ids(&report, "PSE002"), vec![70, 72]);
    assert_eq!(spec(&report, "PSE002").applicable_count, 5);
    // Warnings never fail the report.
    assert_eq!(report.status, ValidationStatus::Passed);
}

#[test]
fn cyclic_decomposition_and_groups() {
    let records = clean()
        + "\n#80=IFCWALL(G,$,'A',$,$,$,$,$,.NOTDEFINED.);\n#81=IFCWALL(G,$,'B',$,$,$,$,$,.NOTDEFINED.);\n#82=IFCRELAGGREGATES(G,$,$,$,#80,(#81));\n#83=IFCRELAGGREGATES(G,$,$,$,#81,(#80));\n#84=IFCGROUP(G,$,'G1',$,$);\n#85=IFCGROUP(G,$,'G2',$,$);\n#86=IFCRELASSIGNSTOGROUP(G,$,$,$,(#85),$,#84);\n#87=IFCRELASSIGNSTOGROUP(G,$,$,$,(#84,#30),$,#85);";
    let report = run(&model(&records), true, None);
    assert_eq!(failed_ids(&report, "OBV-CYCLE"), vec![80, 81]);
    assert_eq!(failed_ids(&report, "GRP001"), vec![84, 85]);
    assert_eq!(spec(&report, "GRP001").applicable_count, 3);
}

#[test]
fn proxy_material_and_type_warnings() {
    let records = clean()
        + "\n#90=IFCBUILDINGELEMENTPROXY(G,$,'Blok',$,$,$,$,$,.NOTDEFINED.);\n#91=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#90),#13);";
    let report = run(&model(&records), true, None);
    assert_eq!(report.status, ValidationStatus::Passed);
    for code in ["OBV-PROXY", "OBV-MATERIAL", "OBV-TYPE"] {
        assert_eq!(failed_ids(&report, code), vec![90], "{code}");
        assert_eq!(spec(&report, code).severity, Severity::Warning);
    }
    assert!(spec(&report, "OBV-PROXY").reasons[0].contains("1 of 2 elements (50.0%)"));
    let summary = report.model.unwrap();
    assert_eq!(
        (
            summary.element_count,
            summary.proxy_count,
            summary.without_material,
            summary.without_nlsfb,
            summary.without_type,
            summary.without_storey
        ),
        (2, 1, 1, 1, 1, 0)
    );
    let storeys: Vec<_> = summary
        .storeys
        .iter()
        .map(|s| (s.name.as_deref().unwrap(), s.element_count))
        .collect();
    assert_eq!(
        storeys,
        vec![("00 begane grond", 1), ("01 eerste verdieping", 1)]
    );
    assert_eq!(summary.length_unit.as_deref(), Some("MILLIMETRE"));
    assert_eq!(summary.length_unit_scale, Some(0.001));
    assert_eq!(summary.project_name.as_deref(), Some("Projectnaam"));
}

#[test]
fn material_on_type_or_parts_counts() {
    let records = clean().replace("#35=IFCRELASSOCIATESMATERIAL(G,$,$,$,(#30),#34);", "#35=IFCRELASSOCIATESMATERIAL(G,$,$,$,(#32),#34);")
        + "\n#92=IFCWALL(G,$,'Layered',$,$,$,$,$,.NOTDEFINED.);\n#93=IFCBUILDINGELEMENTPART(G,$,'Layer',$,$,$,$,$,.NOTDEFINED.);\n#94=IFCRELAGGREGATES(G,$,$,$,#92,(#93));\n#95=IFCRELASSOCIATESMATERIAL(G,$,$,$,(#93),#34);\n#96=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#92),#12);";
    let report = run(&model(&records), true, None);
    assert_eq!(
        spec(&report, "OBV-MATERIAL").status,
        ValidationStatus::Passed
    );
    // The part is not an independently checked element.
    assert_eq!(spec(&report, "OBV-MATERIAL").applicable_count, 2);
}

#[test]
fn ils_storeys() {
    let records = clean()
        .replace("'01 eerste verdieping'", "'Eerste verdieping'")
        .replace(",.ELEMENT.,3000.);", ",.ELEMENT.,0.0004);");
    let report = run(&model(&records), false, Some(IlsOptions::default()));
    assert_eq!(failed_ids(&report, "ILS-STOREY-NAME"), vec![13]);
    let finding = &spec(&report, "ILS-STOREY-NAME").findings[0];
    assert_eq!(finding.storey.as_deref(), Some("Eerste verdieping"));
    assert_eq!(failed_ids(&report, "ILS-STOREY-ELEV"), vec![12, 13]);
    assert_eq!(report.status, ValidationStatus::Failed);
    assert!(!report
        .specifications
        .iter()
        .any(|s| s.source == RuleSource::ModelChecks));
    for name in ["-01 kelder", "01a entresol", "00 begane grond"] {
        assert!(
            crate::values::pattern_match(DEFAULT_STOREY_PATTERN, name).unwrap(),
            "{name}"
        );
    }
    for name in ["0 begane grond", "00begane grond", "BG", "00 "] {
        assert!(
            !crate::values::pattern_match(DEFAULT_STOREY_PATTERN, name).unwrap(),
            "{name}"
        );
    }
    let custom = IlsOptions {
        storey_name_pattern: ".*".into(),
        ..Default::default()
    };
    let report = run(&model(&records), false, Some(custom));
    assert_eq!(
        spec(&report, "ILS-STOREY-NAME").status,
        ValidationStatus::Passed
    );
}

#[test]
fn ils_classification_and_properties() {
    let records = clean()
        .replace("'21.12'", "'2112'")
        .replace("IFCBOOLEAN(.T.),$);\n#42", "IFCLABEL('ja'),$);\n#42")
        .replace("IFCLABEL('60')", "IFCLABEL('EI 60')")
        + "\n#100=IFCDOOR(G,$,'Deur',$,$,$,$,$,$,$,.DOOR.,$,$);\n#101=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#100),#12);\n#102=IFCPROPERTYSET(G,$,'Pset_DoorCommon',$,(#103));\n#103=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('30 min'),$);\n#104=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#100),#102);\n#105=IFCCLASSIFICATION($,$,$,'STABU',$,$,$);\n#106=IFCCLASSIFICATIONREFERENCE($,'31.12','Deuren',#105,$,$);\n#107=IFCRELASSOCIATESCLASSIFICATION(G,$,$,$,(#100),#106);";
    let report = run(&model(&records), false, Some(IlsOptions::default()));
    let nlsfb = spec(&report, "ILS-NLSFB");
    assert_eq!(failed_ids(&report, "ILS-NLSFB"), vec![30, 100]);
    assert!(nlsfb.findings[0].reason.contains("'2112'"));
    assert!(nlsfb.findings[1].reason.contains("No NL-SfB"));
    assert!(nlsfb.reasons[0].contains("STABU"));
    let load = spec(&report, "ILS-LOADBEARING");
    assert_eq!(load.applicable_count, 1);
    assert!(load.findings[0]
        .reason
        .contains("must be a boolean (found ja)"));
    let external = spec(&report, "ILS-ISEXTERNAL");
    assert_eq!(failed_ids(&report, "ILS-ISEXTERNAL"), vec![100]);
    assert!(external.findings[0]
        .reason
        .contains("Pset_DoorCommon.IsExternal is missing"));
    assert_eq!(failed_ids(&report, "ILS-FIRERATING"), vec![30]);
    assert_eq!(spec(&report, "ILS-FIRERATING").applicable_count, 2);
    assert_eq!(spec(&report, "ILS-MATERIAL").failed_count, 1);
}

#[test]
fn ils_file_name_and_occurrence_override() {
    let named = |name: &str| IlsOptions {
        file_name: Some(name.into()),
        ..Default::default()
    };
    let report = run(&model(&clean()), false, Some(named("1234_BK_00.ifc")));
    assert_eq!(spec(&report, "ILS-FILE").status, ValidationStatus::Passed);
    let report = run(&model(&clean()), false, Some(named("model.ifc")));
    assert_eq!(spec(&report, "ILS-FILE").status, ValidationStatus::Failed);
    assert_eq!(report.status, ValidationStatus::Passed);
    // An occurrence property overrides the type's value.
    let records = clean()
        + "\n#110=IFCPROPERTYSET(G,$,'Pset_WallCommon',$,(#111));\n#111=IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCLABEL('nee'),$);\n#112=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#30),#110);";
    let report = run(&model(&records), false, Some(IlsOptions::default()));
    assert_eq!(failed_ids(&report, "ILS-LOADBEARING"), vec![30]);
}

#[test]
fn options_are_admitted_before_parsing_and_cancellation_is_honoured() {
    let none = ValidationOptions::default();
    assert!(validate_model(&model(&clean()), &none, &AtomicBool::new(false)).is_err());
    let bad = ValidationOptions {
        ils: Some(IlsOptions {
            storey_name_pattern: "[".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(validate_model(b"not ifc", &bad, &AtomicBool::new(false))
        .unwrap_err()
        .contains("storey name pattern"));
    let checks = ValidationOptions {
        model_checks: true,
        ..Default::default()
    };
    assert!(
        validate_model(&model(&clean()), &checks, &AtomicBool::new(true))
            .unwrap_err()
            .contains("cancelled")
    );
}

#[test]
fn ids_and_built_in_rules_share_one_report() {
    let ids = r#"<ids xmlns="http://standards.buildingsmart.org/IDS"><info><title>Eisen</title></info><specifications><specification name="Walls named" ifcVersion="IFC4"><applicability minOccurs="1" maxOccurs="unbounded"><entity><name><simpleValue>IFCWALL</simpleValue></name></entity></applicability><requirements><attribute><name><simpleValue>Name</simpleValue></name><value><simpleValue>Other</simpleValue></value></attribute></requirements></specification></specifications></ids>"#;
    let options = ValidationOptions {
        ids_xml: Some(ids.into()),
        model_checks: true,
        ils: None,
        rooms: None,
    };
    let report = validate_model(&model(&clean()), &options, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.specifications[0].source, RuleSource::Ids);
    assert_eq!(
        report.specifications[0].findings[0].storey.as_deref(),
        Some("00 begane grond")
    );
    assert_eq!(
        report.specifications[0].findings[0].name.as_deref(),
        Some("Wall")
    );
    assert_eq!(
        report.specifications[1].identifier.as_deref(),
        Some("PJS101")
    );
    assert_eq!(report.ids_title, "Eisen");
    assert_eq!(report.status, ValidationStatus::Failed);
}

#[test]
fn html_report_is_self_contained_escaped_and_deterministic() {
    // The uncontained proxy produces findings that carry its name into the tables.
    let records = clean()
        + "\n#90=IFCBUILDINGELEMENTPROXY(G,$,'<script>alert(1)</script>',$,$,$,$,$,.NOTDEFINED.);";
    let report = run(&model(&records), true, Some(IlsOptions::default()));
    let html = report_html(&report, "Model & check", ReportLanguage::Dutch);
    assert_eq!(
        html,
        report_html(&report, "Model & check", ReportLanguage::Dutch)
    );
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<title>Model &amp; check</title>"));
    assert!(!html.contains("<script"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("src="));
    assert!(!html.contains("href=\"http"));
    for needle in [
        "SPS007",
        "ILS-NLSFB",
        "Modelkwaliteit",
        "zonder bouwlaag",
        "00 begane grond",
        "MILLIMETRE",
    ] {
        assert!(html.contains(needle), "{needle}");
    }
    let english = report_html(&report, "", ReportLanguage::English);
    assert!(english.contains("Model quality") && english.contains("without storey"));
}

#[test]
fn rooms_doors_and_fire_separations_are_checked() {
    // two rooms with one number on the ground floor, a slab on the first floor without rooms, a
    // narrow door without a fire rating in the 60-minute wall, and an opening in it left open
    let rooms = "#50=IFCSPACE(G,$,'0.01',$,$,$,$,'Woonkamer',.ELEMENT.,.SPACE.,$);
#51=IFCSPACE(G,$,'0.01',$,$,$,$,'Keuken',.ELEMENT.,.SPACE.,$);
#52=IFCRELAGGREGATES(G,$,$,$,#12,(#50,#51));
#60=IFCSLAB(G,$,'Vloer',$,$,$,$,$,.FLOOR.);
#61=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#60),#13);
#70=IFCDOOR(G,$,'D1',$,$,$,$,$,2315.,800.,.DOOR.,.SINGLE_SWING_LEFT.,$);
#71=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#70),#12);
#72=IFCOPENINGELEMENT(G,$,'O1',$,$,$,$,$,.OPENING.);
#73=IFCRELVOIDSELEMENT(G,$,$,$,#30,#72);
#74=IFCRELFILLSELEMENT(G,$,$,$,#72,#70);
#75=IFCOPENINGELEMENT(G,$,'O2',$,$,$,$,$,.OPENING.);
#76=IFCRELVOIDSELEMENT(G,$,$,$,#30,#75);";
    let options = ValidationOptions { rooms: Some(RoomOptions::default()), ..Default::default() };
    let report = validate_model(&model(&format!("{}\n{rooms}", clean())), &options, &AtomicBool::new(false)).unwrap();
    assert!(report.specifications.iter().all(|s| s.source == RuleSource::Rooms && s.severity == Severity::Warning));
    assert_eq!(failed_ids(&report, "RMT-ROOMS"), vec![13]);
    assert_eq!(failed_ids(&report, "RMT-NUMBER"), vec![50, 51]);
    assert_eq!(failed_ids(&report, "RMT-DOOR"), vec![70]);
    assert!(spec(&report, "RMT-DOOR").findings[0].reason.contains("800 mm"), "{:?}", spec(&report, "RMT-DOOR").findings);
    assert_eq!(failed_ids(&report, "RMT-FIREDOOR"), vec![70]);
    assert_eq!(failed_ids(&report, "RMT-FIREOPENING"), vec![75]);
    // a threshold of 35 mm and a ramp of 1 in 8 (7.1 degrees) are flagged; 1 in 14 passes
    let access = "#80=IFCPROPERTYSET(G,$,'Deur',$,(#81));
#81=IFCPROPERTYSINGLEVALUE('ThresholdHeight',$,IFCLENGTHMEASURE(35.),$);
#82=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#70),#80);
#83=IFCRAMPFLIGHT(G,$,'Steil',$,$,$,$,$,.STRAIGHT.);
#84=IFCRAMPFLIGHT(G,$,'Flauw',$,$,$,$,$,.STRAIGHT.);
#85=IFCRELCONTAINEDINSPATIALSTRUCTURE(G,$,$,$,(#83,#84),#12);
#86=IFCPROPERTYSET(G,$,'Pset_RampFlightCommon',$,(#87));
#87=IFCPROPERTYSINGLEVALUE('Slope',$,IFCPLANEANGLEMEASURE(7.125),$);
#88=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#83),#86);
#89=IFCPROPERTYSET(G,$,'Pset_RampFlightCommon',$,(#90));
#90=IFCPROPERTYSINGLEVALUE('Slope',$,IFCPLANEANGLEMEASURE(0.0713),$);
#91=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#84),#89);";
    let report = validate_model(&model(&format!("{}\n{rooms}\n{access}", clean())), &options, &AtomicBool::new(false)).unwrap();
    assert_eq!(failed_ids(&report, "RMT-THRESHOLD"), vec![70]);
    assert_eq!((failed_ids(&report, "RMT-RAMP"), spec(&report, "RMT-RAMP").applicable_count), (vec![83], 2));
    // a rated door as wide as asked passes
    let fixed = rooms.replace("800.,.DOOR.", "900.,.DOOR.").replace("#74=", "#77=IFCPROPERTYSET(G,$,'Pset_DoorCommon',$,(#78));\n#78=IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('EI 60'),$);\n#79=IFCRELDEFINESBYPROPERTIES(G,$,$,$,(#70),#77);\n#74=");
    let report = validate_model(&model(&format!("{}\n{fixed}", clean())), &options, &AtomicBool::new(false)).unwrap();
    assert!(failed_ids(&report, "RMT-DOOR").is_empty() && failed_ids(&report, "RMT-FIREDOOR").is_empty());
}
