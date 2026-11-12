use bim_bcf::{BcfArchive, NewTopic};
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
const VERSION: &[u8] = br#"<Version VersionId="2.1"/>"#;
const MARKUP: &[u8] = br#"<?xml version="1.0"?><Markup><Topic Guid="00000000-0000-4000-8000-000000000001" TopicStatus="Open"><Title>A &amp; B</Title><Description>Check &lt;wall&gt;</Description></Topic><Comment Guid="00000000-0000-4000-8000-000000000002"><Date>2026-11-05T00:00:00Z</Date><Author>tester</Author><Comment>Keep me</Comment></Comment><Viewpoints Guid="00000000-0000-4000-8000-000000000003"><Viewpoint>view.bcfv</Viewpoint><Snapshot>image.png</Snapshot></Viewpoints></Markup>"#;
const VIEW: &[u8] = br#"<VisualizationInfo Guid="00000000-0000-4000-8000-000000000003"><Components><Selection><Component IfcGuid="0ABCDEFGHIJKLMNOPQRSTU"/></Selection></Components><PerspectiveCamera><CameraViewPoint><X>1</X><Y>2</Y><Z>3</Z></CameraViewPoint><CameraDirection><X>0</X><Y>0</Y><Z>-1</Z></CameraDirection><CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector><FieldOfView>60</FieldOfView></PerspectiveCamera></VisualizationInfo>"#;
fn fixture() -> Vec<u8> {
    archive(&[
        ("bcf.version", VERSION),
        ("00000000-0000-4000-8000-000000000001/markup.bcf", MARKUP),
        ("00000000-0000-4000-8000-000000000001/view.bcfv", VIEW),
        (
            "00000000-0000-4000-8000-000000000001/image.png",
            b"image bytes",
        ),
        ("project.bcfp", b"<ProjectExtension/>"),
    ])
}
fn file(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut result).unwrap();
    result
}
#[test]
fn preserves_markup_comments_cameras_and_assets() {
    let input = fixture();
    let mut bcf = BcfArchive::read(&input).unwrap();
    assert_eq!(bcf.topics()[0].title, "A & B");
    assert_eq!(bcf.topics()[0].component_guids, ["0ABCDEFGHIJKLMNOPQRSTU"]);
    let camera = bcf.topics()[0].camera.clone().unwrap();
    assert_eq!(camera.position, [1., 2., 3.]);
    assert_eq!(camera.field_of_view, Some(60.));
    assert_eq!(
        file(
            &bcf.write().unwrap(),
            "00000000-0000-4000-8000-000000000001/markup.bcf"
        ),
        MARKUP
    );
    bcf.set_status(
        "00000000-0000-4000-8000-000000000001",
        "Done & \"reviewed\"",
    )
    .unwrap();
    let output = bcf.write().unwrap();
    assert_eq!(
        BcfArchive::read(&output).unwrap().topics()[0].status,
        "Done & \"reviewed\""
    );
    let expected = String::from_utf8(MARKUP.to_vec()).unwrap().replace(
        "TopicStatus=\"Open\"",
        "TopicStatus=\"Done &amp; &quot;reviewed&quot;\"",
    );
    assert_eq!(
        file(&output, "00000000-0000-4000-8000-000000000001/markup.bcf"),
        expected.as_bytes()
    );
    for name in [
        "00000000-0000-4000-8000-000000000001/view.bcfv",
        "00000000-0000-4000-8000-000000000001/image.png",
        "project.bcfp",
    ] {
        assert_eq!(file(&input, name), file(&output, name));
    }
}

#[test]
fn preserves_status_quote_styles_and_handles_absent_status() {
    for attr in [
        "",
        " TopicStatus='Open'",
        &format!(" TopicStatus{}=   'Open'", " ".repeat(300)),
    ] {
        let markup =
            format!("<Markup><Topic Guid='00000000-0000-4000-8000-000000000001'{attr}><Title>Title</Title></Topic></Markup>");
        let mut bcf = BcfArchive::read(&archive(&[
            ("bcf.version", VERSION),
            (
                "00000000-0000-4000-8000-000000000001/markup.bcf",
                markup.as_bytes(),
            ),
        ]))
        .unwrap();
        bcf.set_status("00000000-0000-4000-8000-000000000001", "It's done\nnow")
            .unwrap();
        assert_eq!(
            BcfArchive::read(&bcf.write().unwrap()).unwrap().topics()[0].status,
            "It's done\nnow"
        );
        assert!(bcf
            .set_status("00000000-0000-4000-8000-000000000001", "\0")
            .is_err());
        assert!(bcf.set_status("missing", "Open").is_err());
    }
}

#[test]
fn rejects_nonfinite_camera_and_duplicate_comment_ids() {
    let broken = String::from_utf8(VIEW.to_vec())
        .unwrap()
        .replace("<X>1</X>", "<X>NaN</X>");
    assert!(BcfArchive::read(&archive(&[
        ("bcf.version", VERSION),
        ("00000000-0000-4000-8000-000000000001/markup.bcf", MARKUP),
        (
            "00000000-0000-4000-8000-000000000001/view.bcfv",
            broken.as_bytes()
        ),
        ("00000000-0000-4000-8000-000000000001/image.png", b"image")
    ]))
    .is_err());
    let duplicate = br#"<Markup><Topic Guid="00000000-0000-4000-8000-000000000001"><Title>A</Title></Topic><Comment Guid="00000000-0000-4000-8000-000000000002"/><Comment Guid="00000000-0000-4000-8000-000000000002"/></Markup>"#;
    assert!(BcfArchive::read(&archive(&[
        ("bcf.version", VERSION),
        ("a/markup.bcf", duplicate)
    ]))
    .is_err());
}

#[test]
fn rejects_archive_size_and_entry_limits() {
    assert!(BcfArchive::read(&vec![0; 50 * 1024 * 1024 + 1]).is_err());
    let oversized = archive(&[("oversized.bin", &vec![0; 100 * 1024 * 1024 + 1])]);
    assert!(BcfArchive::read(&oversized)
        .unwrap_err()
        .contains("expanded"));
    let names: Vec<String> = (0..2001).map(|i| format!("entry-{i}")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|s| (s.as_str(), b"".as_slice())).collect();
    assert!(BcfArchive::read(&archive(&entries))
        .unwrap_err()
        .contains("entries"));
}

#[test]
fn rejects_degenerate_camera_vectors_and_reads_orthographic_camera() {
    for value in ["<X>0</X><Y>0</Y><Z>0</Z>", "<X>0</X><Y>1</Y><Z>0</Z>"] {
        let view = String::from_utf8(VIEW.to_vec())
            .unwrap()
            .replace("<X>0</X><Y>0</Y><Z>-1</Z>", value);
        let data = archive(&[
            ("bcf.version", VERSION),
            ("a/markup.bcf", MARKUP),
            ("a/view.bcfv", view.as_bytes()),
            ("a/image.png", b"image"),
        ]);
        assert!(BcfArchive::read(&data).is_err());
    }
    let view = String::from_utf8(VIEW.to_vec())
        .unwrap()
        .replace("PerspectiveCamera", "OrthogonalCamera")
        .replace("FieldOfView", "ViewToWorldScale");
    let data = archive(&[
        ("bcf.version", VERSION),
        ("a/markup.bcf", MARKUP),
        ("a/view.bcfv", view.as_bytes()),
        ("a/image.png", b"image"),
    ]);
    let topic = BcfArchive::read(&data).unwrap().topics().remove(0);
    assert_eq!(topic.camera.unwrap().view_to_world_scale, Some(60.));
    assert_eq!(topic.viewpoint_count, 1);
}
#[test]
fn creates_validation_topics_with_escaped_content() {
    let topic = NewTopic {
        id: "00000000-0000-4000-8000-000000000004".into(),
        title: "Wall <check> & test".into(),
        status: "Open".into(),
        description: "Missing \"value\"".into(),
        component_guids: vec!["0ABCDEFGHIJKLMNOPQRSTU".into()],
        creation_date: "2026-11-05T00:00:00Z".into(),
        creation_author: "validator".into(),
    };
    let output = BcfArchive::from_topics(&[topic]).unwrap().write().unwrap();
    let bcf = BcfArchive::read(&output).unwrap();
    assert_eq!(bcf.topics()[0].title, "Wall <check> & test");
    assert_eq!(bcf.topics()[0].component_guids, ["0ABCDEFGHIJKLMNOPQRSTU"]);
}
#[test]
fn rejects_invalid_archives() {
    assert!(BcfArchive::read(b"not zip").is_err());
    for (name, content) in [
        ("../bad", b"x".as_slice()),
        ("C:/bad", b"x"),
        ("bad\\path", b"x"),
        ("bad.xml", b"<!DOCTYPE x><x/>"),
        ("bad.xml", b"<broken>"),
    ] {
        assert!(
            BcfArchive::read(&archive(&[
                ("bcf.version", VERSION),
                ("00000000-0000-4000-8000-000000000001/markup.bcf", MARKUP),
                (name, content)
            ]))
            .is_err(),
            "{name}"
        );
    }
    assert!(BcfArchive::read(&archive(&[
        ("bcf.version", VERSION),
        ("a/markup.bcf", MARKUP),
        ("b/markup.bcf", MARKUP)
    ]))
    .is_err());
    assert!(BcfArchive::read(&archive(&[(
        "bcf.version",
        br#"<Version VersionId="3.0"/>"#
    )]))
    .is_err());
    assert!(BcfArchive::read(&archive(&[
        ("bcf.version", VERSION),
        ("00000000-0000-4000-8000-000000000001/markup.bcf", MARKUP)
    ]))
    .is_err());
}
