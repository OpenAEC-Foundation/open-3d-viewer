use super::*;
fn model(records: &str) -> Vec<u8> {
    format!("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('IFC4'));\nENDSEC;\nDATA;\n{records}\nENDSEC;\nEND-ISO-10303-21;").into_bytes()
}
fn ids(app: &str, req: &str, min: u32, max: &str) -> String {
    format!(
        r#"<ids xmlns="http://standards.buildingsmart.org/IDS"><info><title>Regression</title></info><specifications><specification name="Check" ifcVersion="IFC4"><applicability minOccurs="{min}" maxOccurs="{max}">{app}</applicability><requirements>{req}</requirements></specification></specifications></ids>"#
    )
}
const WALL: &str = "<entity><name><simpleValue>IFCWALL</simpleValue></name></entity>";
const NAME:&str="<attribute><name><simpleValue>Name</simpleValue></name><value><simpleValue>Wall</simpleValue></value></attribute>";
#[test]
fn representationless_entity_and_global_id() {
    let report = validate(
        &model("#1=IFCWALL('guid',$,'Wrong',$,$,$,$,$,.NOTDEFINED.);"),
        &ids(WALL, NAME, 1, "unbounded"),
    )
    .unwrap();
    assert_eq!(report.status, ValidationStatus::Failed);
    assert_eq!(
        report.specifications[0].findings[0].global_id.as_deref(),
        Some("guid")
    );
    assert_eq!(report.specifications[0].findings[0].express_id, 1);
}
#[test]
fn schema_inheritance_and_attribute_pass() {
    let report = validate(
        &model("#1=IFCWALLSTANDARDCASE('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);"),
        &ids(WALL, NAME, 1, "unbounded"),
    )
    .unwrap();
    assert_eq!(report.status, ValidationStatus::Passed);
    assert_eq!(report.specifications[0].applicable_count, 1);
}
#[test]
fn required_optional_and_prohibited_applicability() {
    let m = model("#1=IFCSITE('guid',$,$,$,$,$,$,$,$,$,$,$,$,$);");
    assert_eq!(
        validate(&m, &ids(WALL, NAME, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
    assert_eq!(
        validate(&m, &ids(WALL, NAME, 0, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Passed
    );
    let wall = model("#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);");
    assert_eq!(
        validate(&wall, &ids(WALL, NAME, 0, "0")).unwrap().status,
        ValidationStatus::Failed
    );
}
#[test]
fn optional_present_wrong_value_fails() {
    let req = NAME.replace("<attribute>", "<attribute cardinality=\"optional\">");
    let wrong = model("#1=IFCWALL('guid',$,'Wrong',$,$,$,$,$,.NOTDEFINED.);");
    let absent = model("#1=IFCWALL('guid',$,$,$,$,$,$,$,.NOTDEFINED.);");
    assert_eq!(
        validate(&wrong, &ids(WALL, &req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
    assert_eq!(
        validate(&absent, &ids(WALL, &req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Passed
    );
}
#[test]
fn inherited_property_and_occurrence_override() {
    let p="<property dataType=\"IFCLABEL\"><propertySet><simpleValue>Custom</simpleValue></propertySet><baseName><simpleValue>Code</simpleValue></baseName><value><simpleValue>A</simpleValue></value></property>";
    let base="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCWALLTYPE('type',$,$,$,$,(#3),$,$,$,.NOTDEFINED.);\n#3=IFCPROPERTYSET('set',$,'Custom',$,(#4));\n#4=IFCPROPERTYSINGLEVALUE('Code',$,IFCLABEL('A'),$);\n#5=IFCRELDEFINESBYTYPE('rel',$,$,$,(#1),#2);";
    assert_eq!(
        validate(&model(base), &ids(WALL, p, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Passed
    );
    let override_records=format!("{base}\n#6=IFCPROPERTYSET('set2',$,'Custom',$,(#7));\n#7=IFCPROPERTYSINGLEVALUE('Code',$,IFCLABEL('B'),$);\n#8=IFCRELDEFINESBYPROPERTIES('rel2',$,$,$,(#1),#6);");
    assert_eq!(
        validate(&model(&override_records), &ids(WALL, p, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
}
#[test]
fn wrong_schema_and_unsupported_never_pass() {
    let m = model("#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);");
    assert_eq!(
        validate(
            &m,
            &ids(WALL, NAME, 1, "unbounded")
                .replace("ifcVersion=\"IFC4\"", "ifcVersion=\"IFC2X3\"")
        )
        .unwrap()
        .status,
        ValidationStatus::NotCheckable
    );
    let pattern=NAME.replace("<simpleValue>Wall</simpleValue>","<xs:restriction xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" base=\"xs:string\"><xs:pattern value=\"[\"/></xs:restriction>");
    assert_eq!(
        validate(&m, &ids(WALL, &pattern, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::NotCheckable
    );
}
#[test]
fn malformed_and_duplicate_entities_rejected() {
    assert!(validate(
        &model("#1=IFCWALL('oops);"),
        &ids(WALL, NAME, 1, "unbounded")
    )
    .is_err());
    assert!(validate(
        &model("#1=IFCWALL();\n#1=IFCWALL();"),
        &ids(WALL, NAME, 1, "unbounded")
    )
    .is_err());
    let deep = format!("#1=IFCWALL({}{});", "(".repeat(65), ")".repeat(65));
    assert!(validate(&model(&deep), &ids(WALL, NAME, 1, "unbounded")).is_err());
}
#[test]
fn numeric_restriction_and_material_and_partof() {
    let a = crate::values::matches(
        &openbim_ids::Restriction {
            base: "decimal".into(),
            min_inclusive: Some("2.5".into()),
            ..Default::default()
        }
        .into(),
        "3",
    )
    .unwrap();
    assert!(a);
    let m=model("#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCMATERIAL('Concrete',$,$);\n#3=IFCRELASSOCIATESMATERIAL('mat',$,$,$,(#1),#2);\n#4=IFCBUILDINGSTOREY('floor',$,'Floor',$,$,$,$,$,.ELEMENT.,0.);\n#5=IFCRELCONTAINEDINSPATIALSTRUCTURE('rel',$,$,$,(#1),#4);");
    let req="<partOf relation=\"IFCRELCONTAINEDINSPATIALSTRUCTURE\"><entity><name><simpleValue>IFCBUILDINGSTOREY</simpleValue></name></entity></partOf><material><value><simpleValue>Concrete</simpleValue></value></material>";
    assert_eq!(
        validate(&m, &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Passed
    );
}

#[test]
fn xsd_patterns_use_xml_character_subtraction_and_alternatives() {
    use openbim_ids::{Restriction, Value};
    let r: Value = Restriction {
        base: "string".into(),
        patterns: vec!["[a-z-[aeiou]]+".into(), "[0-9]+".into()],
        ..Default::default()
    }
    .into();
    assert!(crate::values::matches(&r, "rhythm").unwrap());
    assert!(crate::values::matches(&r, "123").unwrap());
    assert!(!crate::values::matches(&r, "aeiou").unwrap());
    assert!(!crate::values::matches(&r, "X123X").unwrap());
}
#[test]
fn si_length_property_is_normalized_and_missing_unit_is_not_checkable() {
    let records="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCPROPERTYSET('set',$,'Custom',$,(#3));\n#3=IFCPROPERTYSINGLEVALUE('Length',$,IFCLENGTHMEASURE(1000.),#5);\n#4=IFCRELDEFINESBYPROPERTIES('rel',$,$,$,(#1),#2);\n#5=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);";
    let req="<property dataType=\"IFCLENGTHMEASURE\"><propertySet><simpleValue>Custom</simpleValue></propertySet><baseName><simpleValue>Length</simpleValue></baseName><value><simpleValue>1</simpleValue></value></property>";
    assert_eq!(
        validate(&model(records), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Passed
    );
    assert_eq!(
        validate(
            &model(&records.replace("(1000.),#5)", "(1000.),$)")),
            &ids(WALL, req, 1, "unbounded")
        )
        .unwrap()
        .status,
        ValidationStatus::NotCheckable
    );
}
#[test]
fn unknown_logical_and_wrong_optional_classification_never_pass() {
    use crate::values::value_matches;
    use ifc_lite_core::AttributeValue as A;
    assert!(!value_matches(&None, &A::Enum("U".into())).unwrap());
    assert!(!value_matches(
        &None,
        &A::Typed {
            name: "IFCLOGICAL".into(),
            value: Box::new(A::Enum("U".into()))
        }
    )
    .unwrap());
    let records="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCCLASSIFICATION($,$,$,'Other',$,$,$);\n#3=IFCRELASSOCIATESCLASSIFICATION('rel',$,$,$,(#1),#2);";
    let req="<classification cardinality=\"optional\"><system><simpleValue>Expected</simpleValue></system></classification>";
    assert_eq!(
        validate(&model(records), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
}
#[test]
fn every_matching_property_set_requires_the_named_property() {
    let records="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCPROPERTYSET('set',$,'CustomA',$,(#3));\n#3=IFCPROPERTYSINGLEVALUE('Code',$,IFCLABEL('A'),$);\n#4=IFCRELDEFINESBYPROPERTIES('rel',$,$,$,(#1),#2);\n#5=IFCPROPERTYSET('set2',$,'CustomB',$,(#6));\n#6=IFCPROPERTYSINGLEVALUE('Other',$,IFCLABEL('A'),$);\n#7=IFCRELDEFINESBYPROPERTIES('rel2',$,$,$,(#1),#5);";
    let req="<property dataType=\"IFCLABEL\"><propertySet><xs:restriction xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" base=\"xs:string\"><xs:pattern value=\"Custom.*\"/></xs:restriction></propertySet><baseName><simpleValue>Code</simpleValue></baseName></property>";
    assert_eq!(
        validate(&model(records), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
}
#[test]
fn cancellation_and_dtd_rejection() {
    let cancel = std::sync::atomic::AtomicBool::new(true);
    assert!(validate_with_cancel(
        &model("#1=IFCWALL();"),
        &ids(WALL, NAME, 1, "unbounded"),
        &cancel
    )
    .unwrap_err()
    .contains("cancelled"));
    assert!(validate(&model("#1=IFCWALL();"), "<!DOCTYPE ids []><ids/>")
        .unwrap_err()
        .contains("DTD"));
}

#[test]
fn unsupported_semantics_are_checked_without_applicable_entities() {
    let m = model("#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);");
    let app = "<entity><name><simpleValue>IFCDOOR</simpleValue></name></entity>";
    let ordered="<attribute><name><simpleValue>Name</simpleValue></name><value><xs:restriction xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" base=\"xs:string\"><xs:minInclusive value=\"A\"/></xs:restriction></value></attribute>";
    let combined="<partOf relation=\"IFCRELVOIDSELEMENT IFCRELFILLSELEMENT\"><entity><name><simpleValue>IFCWALL</simpleValue></name></entity></partOf>";
    for req in [ordered, combined] {
        assert_eq!(
            validate(&m, &ids(app, req, 0, "unbounded")).unwrap().status,
            ValidationStatus::NotCheckable
        );
    }
}

#[test]
fn duplicate_property_and_cyclic_partof_are_not_checkable() {
    let base="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCPROPERTYSET('set',$,'Custom',$,(#3,#4));\n#3=IFCPROPERTYSINGLEVALUE('Code',$,IFCLABEL('bad'),$);\n#4=IFCPROPERTYSINGLEVALUE('Code',$,IFCLABEL('good'),$);\n#5=IFCRELDEFINESBYPROPERTIES('rel',$,$,$,(#1),#2);";
    let req="<property><propertySet><simpleValue>Custom</simpleValue></propertySet><baseName><simpleValue>Code</simpleValue></baseName><value><simpleValue>good</simpleValue></value></property>";
    assert_eq!(
        validate(&model(base), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::NotCheckable
    );
    let cyclic="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCRELAGGREGATES('rel',$,$,$,#1,(#1));";
    let req="<partOf relation=\"IFCRELAGGREGATES\"><entity><name><simpleValue>IFCWALL</simpleValue></name></entity></partOf>";
    assert_eq!(
        validate(&model(cyclic), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::NotCheckable
    );
}
#[test]
fn lower_only_bounded_property_is_present() {
    let records="#1=IFCWALL('guid',$,'Wall',$,$,$,$,$,.NOTDEFINED.);\n#2=IFCPROPERTYSET('set',$,'Custom',$,(#3));\n#3=IFCPROPERTYBOUNDEDVALUE('Code',$,$,IFCREAL(5.),$,$);\n#4=IFCRELDEFINESBYPROPERTIES('rel',$,$,$,(#1),#2);";
    let req="<property cardinality=\"optional\"><propertySet><simpleValue>Custom</simpleValue></propertySet><baseName><simpleValue>Code</simpleValue></baseName><value><simpleValue>6</simpleValue></value></property>";
    assert_eq!(
        validate(&model(records), &ids(WALL, req, 1, "unbounded"))
            .unwrap()
            .status,
        ValidationStatus::Failed
    );
}

#[test]
fn integer_values_are_compared_exactly() {
    use ifc_lite_core::AttributeValue as A;
    assert!(!crate::values::value_matches(&Some("1000001".into()), &A::Integer(1000000)).unwrap());
    assert!(crate::values::value_matches(
        &Some("9007199254740993".into()),
        &A::Integer(9007199254740993)
    )
    .unwrap());
    assert!(!crate::values::value_matches(
        &Some("9007199254740992".into()),
        &A::Integer(9007199254740993)
    )
    .unwrap());
}

#[test]
fn a_project_with_several_units_of_its_own_is_checked_and_one_with_two_length_units_is_not() {
    let units = |more: &str| {
        model(&format!(
            "#1=IFCPROJECT('2O2Fr$t4X7Zf8NOew3FLOH',$,'P',$,$,$,$,$,#2);\n#2=IFCUNITASSIGNMENT((#3,#4,#5{more}));\n#3=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#4=IFCDERIVEDUNIT((#6),.USERDEFINED.,'a');\n#5=IFCDERIVEDUNIT((#6),.USERDEFINED.,'b');\n#6=IFCDERIVEDUNITELEMENT(#3,1);\n#7=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);"
        ))
    };
    let project = ids("<entity><name><simpleValue>IFCPROJECT</simpleValue></name></entity>", "", 1, "unbounded");
    assert_eq!(validate(&units(""), &project).unwrap().status, ValidationStatus::Passed);
    assert_eq!(validate(&units(",#7"), &project).unwrap_err(), "Duplicate project unit types");
}

/// How long each specification of an IDS takes on a file: `O3V_IFC` and `O3V_IDS` name them.
#[test]
#[ignore]
fn timing_of_each_specification() {
    let (Ok(ifc), Ok(ids)) = (std::env::var("O3V_IFC"), std::env::var("O3V_IDS")) else { return };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let bytes = std::fs::read(&ifc).unwrap();
    let started = std::time::Instant::now();
    let graph = crate::graph::Graph::parse(&bytes, &cancel).unwrap();
    println!("parse {:.2} s, {} nodes", started.elapsed().as_secs_f32(), graph.nodes.len());
    let ids = crate::parse_ids(&std::fs::read_to_string(&ids).unwrap()).unwrap();
    for spec in &ids.specifications {
        let mut one = ids.clone();
        one.specifications = vec![spec.clone()];
        let started = std::time::Instant::now();
        let report = crate::evaluator::validate(&graph, &one, &cancel).unwrap();
        let r = &report.specifications[0];
        println!("{:7.2} s  {} ({} applicable, {} findings)", started.elapsed().as_secs_f32(), spec.name, r.applicable_count, r.findings.len());
    }
}
