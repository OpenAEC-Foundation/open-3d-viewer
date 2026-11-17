//! What is said about the documents of a project: the register.
//!
//! Each document something was said about has one small IFCX file in [`crate::REGISTER_FOLDER`],
//! named by the document's id. Two people working on different documents never write the same
//! file. When two do change the same document at once, the sync tool leaves a second file beside
//! the first; both hold the same id, so they are read as one ([`merge`]) and the next write of
//! that document tidies the second away.
//!
//! An entry is IFCX in the form the suite writes everywhere. The document is a node at the path of
//! its id with [`DOCUMENT`]. Every element it is about that has an IFCX node path gets a node at
//! that path with `openaec::document::<id>`: layered over the model, the element carries its
//! documents.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::folder::File;

/// The attribute of a document's own node.
pub const DOCUMENT: &str = "openaec::document";

/// Where a document stands, after the states of a container in ISO 19650.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Being worked on by its author.
    #[default]
    Work,
    /// Shared with the team to work with.
    Shared,
    /// Published: approved to build or decide from.
    Published,
    /// Kept for the record, no longer in use.
    Archived,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::Work, Status::Shared, Status::Published, Status::Archived];
}

/// A revision of a document that someone recorded.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Revision {
    /// Its code, as the team names revisions ("P01", "C02", "B").
    pub code: String,
    /// When it was recorded (`YYYY-MM-DDTHH:MM:SSZ`).
    pub date: String,
    pub by: String,
    pub note: String,
    /// The file's size and time when it was recorded: how a later change is told.
    pub size: u64,
    pub modified: u64,
}

/// An element of a model that a document is about.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ElementLink {
    /// The element's lasting key: its GlobalId, else its UniqueId, else its id in the file.
    pub key: String,
    /// Its IFCX node path, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    pub name: String,
    /// The model file it was in, by its name.
    pub model: String,
}

/// What is said about one document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Document {
    /// Its lasting id (a UUID): the path of its node and the name of its register file.
    pub id: String,
    /// The file, as its path in the project folder with `/` between the names.
    pub file: String,
    pub title: String,
    pub status: Status,
    /// The code of the revision that stands.
    pub revision: String,
    /// The revisions recorded, oldest first.
    pub revisions: Vec<Revision>,
    pub elements: Vec<ElementLink>,
    /// When this entry was last changed, and by whom.
    pub changed: String,
    pub by: String,
}

/// How a document's file stands against what was recorded of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The file is as its last recorded revision.
    Recorded,
    /// The file changed since its last recorded revision.
    Changed,
    /// No revision was recorded yet.
    Unrecorded,
    /// The file is not in the project folder (any more).
    Missing,
}

impl Document {
    pub fn new(id: &str, file: &str) -> Self {
        Document { id: id.into(), file: file.into(), ..Document::default() }
    }

    /// The document's name in a list: its title, else its file's name.
    pub fn name(&self) -> &str {
        if self.title.is_empty() {
            self.file.rsplit('/').next().unwrap_or(&self.file)
        } else {
            &self.title
        }
    }

    /// Notes that `by` changed the entry at `now`.
    pub fn touch(&mut self, by: &str, now: &str) {
        self.changed = now.into();
        self.by = by.into();
    }

    /// Records the file as it is now as revision `code`, which then stands.
    pub fn record(&mut self, code: &str, note: &str, file: &File, by: &str, now: &str) {
        self.revisions.push(Revision { code: code.trim().into(), date: now.into(), by: by.into(), note: note.trim().into(), size: file.size, modified: file.modified });
        self.revision = code.trim().into();
        self.touch(by, now);
    }

    /// Links the document to an element; `false` when it was linked already.
    pub fn link(&mut self, link: ElementLink) -> bool {
        if self.elements.iter().any(|e| e.key == link.key) {
            return false;
        }
        self.elements.push(link);
        true
    }

    /// How `file` (the document's file as the folder has it, if it is there) stands against the
    /// last recorded revision.
    pub fn state(&self, file: Option<&File>) -> State {
        match (file, self.revisions.last()) {
            (None, _) => State::Missing,
            (Some(_), None) => State::Unrecorded,
            (Some(file), Some(last)) if file.size == last.size && file.modified == last.modified => State::Recorded,
            (Some(_), Some(_)) => State::Changed,
        }
    }

    /// The entry as an IFCX file, with `author` and `timestamp` in its header.
    pub fn to_ifcx(&self, author: &str, timestamp: &str) -> Value {
        let mut data = vec![json!({"path": self.id, "attributes": {"bsi::ifc::prop::Name": self.name(), DOCUMENT: self}})];
        // an element in several documents gets one attribute per document, so the files compose
        let carried = json!({"name": self.name(), "file": self.file, "status": self.status, "revision": self.revision});
        for element in self.elements.iter().filter(|e| !e.path.is_empty()) {
            let mut attributes = BTreeMap::new();
            attributes.insert(format!("{DOCUMENT}::{}", self.id), carried.clone());
            data.push(json!({"path": element.path, "attributes": attributes}));
        }
        json!({
            "header": {"id": format!("{}.ifcx", self.id), "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": author, "timestamp": timestamp},
            "imports": [],
            "schemas": {},
            "data": data,
        })
    }

    /// The document of an IFCX register file; an error says why it is none.
    pub fn from_ifcx(bytes: &[u8]) -> Result<Document, String> {
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let file = ifcx_core::parser::from_slice(bytes).map_err(|e| format!("IFCX: {e}"))?;
        let document = file.data.iter().find_map(|node| node.get_attr::<Document>(DOCUMENT).map(|d| (node, d)));
        match document {
            // an entry written by hand may leave the id out: its node's path is the id
            Some((node, mut document)) => {
                if document.id.is_empty() {
                    document.id = node.path.clone();
                }
                Ok(document)
            }
            None => Err("IFCX: no document in this file".into()),
        }
    }
}

/// Two entries of the same document as one: the one changed last stands for what is said once
/// (file, title, status); the revisions and links of both are kept, and the revision recorded
/// last is the one that stands.
pub fn merge(a: Document, b: Document) -> Document {
    // dates are ISO texts in one time zone: the later one is the greater text
    let (mut stands, other) = if b.changed > a.changed { (b, a) } else { (a, b) };
    for revision in other.revisions {
        if !stands.revisions.iter().any(|r| r.code == revision.code && r.date == revision.date) {
            stands.revisions.push(revision);
        }
    }
    stands.revisions.sort_by(|x, y| x.date.cmp(&y.date));
    if let Some(last) = stands.revisions.last() {
        stands.revision = last.code.clone();
    }
    for element in other.elements {
        stands.link(element);
    }
    stands
}

/// Writes `bytes` beside `path` and then in its place, so nobody reads half a file.
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension("part");
    std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

/// The register of a project folder as it was read.
#[derive(Debug, Clone, Default)]
pub struct Register {
    /// The documents, sorted by their file's path.
    pub documents: Vec<Document>,
    /// The register files that held each document id: more than one after a conflict.
    files: BTreeMap<String, Vec<PathBuf>>,
    /// The files that could not be read (half written, or not a register file): left alone.
    pub unread: usize,
}

impl Register {
    /// Reads the register of the project folder at `project`. A project without a register has an
    /// empty one.
    pub fn read(project: &Path) -> Register {
        let mut register = Register::default();
        let Ok(entries) = std::fs::read_dir(project.join(crate::REGISTER_FOLDER)) else { return register };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifcx"))).collect();
        paths.sort();
        let mut documents: BTreeMap<String, Document> = BTreeMap::new();
        for path in paths {
            // half written by the sync tool, or not an entry: left alone, read again next time
            let Some(document) = std::fs::read(&path).ok().and_then(|bytes| Document::from_ifcx(&bytes).ok()).filter(|d| !d.id.is_empty()) else {
                register.unread += 1;
                continue;
            };
            register.files.entry(document.id.clone()).or_default().push(path);
            let merged = match documents.remove(&document.id) {
                Some(other) => merge(other, document),
                None => document,
            };
            documents.insert(merged.id.clone(), merged);
        }
        register.documents = documents.into_values().collect();
        register.documents.sort_by(|a, b| a.file.cmp(&b.file));
        register
    }

    /// The document of the file at `path` in the project folder.
    pub fn of(&self, path: &str) -> Option<&Document> {
        self.documents.iter().find(|d| d.file == path)
    }

    pub fn by_id(&self, id: &str) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    /// The documents an element is linked to, by its lasting key.
    pub fn about<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Document> {
        self.documents.iter().filter(move |d| d.elements.iter().any(|e| e.key == key))
    }

    /// Writes `document` to the register of `project` and keeps it here. The file is written
    /// beside the old one and then takes its place, so nobody reads half of it; other files that
    /// held the same document (conflict copies, read as one with it) are removed.
    pub fn save(&mut self, project: &Path, document: Document, author: &str, timestamp: &str) -> Result<(), String> {
        // the id names the file: nothing but what a UUID holds
        if document.id.is_empty() || !document.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(format!("a document id cannot name a file: {:?}", document.id));
        }
        let folder = project.join(crate::REGISTER_FOLDER);
        std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = folder.join(format!("{}.ifcx", document.id));
        let bytes = serde_json::to_vec_pretty(&document.to_ifcx(author, timestamp)).map_err(|e| e.to_string())?;
        put(&path, &bytes)?;
        // the copies that were read as one with it say nothing the new file does not
        for copy in self.files.remove(&document.id).unwrap_or_default().into_iter().filter(|copy| *copy != path) {
            let _ = std::fs::remove_file(copy);
        }
        self.files.insert(document.id.clone(), vec![path]);
        self.documents.retain(|d| d.id != document.id);
        self.documents.push(document);
        self.documents.sort_by(|a, b| a.file.cmp(&b.file));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bim-docs-register-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn file(path: &str, size: u64, modified: u64) -> File {
        File { path: path.into(), size, modified, online_only: false, conflict: false }
    }

    const ID: &str = "0b5c1c0e-7a52-4f6e-9a55-3a1d6a0c9f01";

    fn plan() -> Document {
        let mut d = Document::new(ID, "drawings/A-101.pdf");
        d.title = "Ground floor plan".into();
        d.status = Status::Shared;
        d.record("P01", "for coordination", &file("drawings/A-101.pdf", 2048, 1_794_000_000), "an", "2026-11-17T09:00:00Z");
        d
    }

    #[test]
    fn recording_a_revision_makes_it_stand_and_keeps_the_ones_before() {
        let mut d = plan();
        assert_eq!((d.revision.as_str(), d.revisions.len(), d.changed.as_str(), d.by.as_str()), ("P01", 1, "2026-11-17T09:00:00Z", "an"));
        d.record("P02", "", &file("drawings/A-101.pdf", 4096, 1_794_100_000), "bo", "2026-11-18T10:00:00Z");
        assert_eq!(d.revision, "P02");
        assert_eq!(d.revisions.iter().map(|r| (r.code.as_str(), r.by.as_str(), r.size)).collect::<Vec<_>>(), [("P01", "an", 2048), ("P02", "bo", 4096)]);
        assert_eq!((d.changed.as_str(), d.by.as_str()), ("2026-11-18T10:00:00Z", "bo"));
    }

    #[test]
    fn a_file_is_told_from_its_last_recorded_revision_without_reading_it() {
        let d = plan();
        assert_eq!(d.state(Some(&file("drawings/A-101.pdf", 2048, 1_794_000_000))), State::Recorded);
        assert_eq!(d.state(Some(&file("drawings/A-101.pdf", 2048, 1_794_000_500))), State::Changed);
        assert_eq!(d.state(Some(&file("drawings/A-101.pdf", 2050, 1_794_000_000))), State::Changed);
        assert_eq!(d.state(None), State::Missing);
        let fresh = Document::new(ID, "drawings/A-101.pdf");
        assert_eq!(fresh.state(Some(&file("drawings/A-101.pdf", 1, 1))), State::Unrecorded);
    }

    #[test]
    fn an_entry_is_ifcx_and_reads_back_as_it_was() {
        let mut d = plan();
        d.link(ElementLink { key: "2O2Fr$t4X7Zf8NOew3FLOH".into(), path: "9d6c7f0a-1111-4222-8333-444455556666".into(), name: "Wall 12".into(), model: "kade12.ifcx".into() });
        d.link(ElementLink { key: "3hVbZ0ukP2nhs5mXkY1vT9".into(), path: String::new(), name: "Door 3".into(), model: "kade12.ifc".into() });
        let value = d.to_ifcx("Open BIM Suite 1.0", "2026-11-17T09:00:00Z");
        assert_eq!(value["header"], json!({"id": format!("{ID}.ifcx"), "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "Open BIM Suite 1.0", "timestamp": "2026-11-17T09:00:00Z"}));
        assert_eq!((&value["imports"], &value["schemas"]), (&json!([]), &json!({})));
        // the document's own node, at the path of its id
        let node = &value["data"][0];
        assert_eq!((node["path"].as_str(), node["attributes"]["bsi::ifc::prop::Name"].as_str()), (Some(ID), Some("Ground floor plan")));
        assert_eq!(node["attributes"][DOCUMENT]["status"], "shared");
        assert_eq!(node["attributes"][DOCUMENT]["revisions"][0]["code"], "P01");
        // the element with an IFCX path carries the document; the one without has no node
        assert_eq!(value["data"].as_array().unwrap().len(), 2);
        let element = &value["data"][1];
        assert_eq!(element["path"], "9d6c7f0a-1111-4222-8333-444455556666");
        assert_eq!(element["attributes"][format!("openaec::document::{ID}")], json!({"name": "Ground floor plan", "file": "drawings/A-101.pdf", "status": "shared", "revision": "P01"}));
        // and back, also through the suite's IFCX reader with a byte order mark before it
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(serde_json::to_vec(&value).unwrap());
        assert_eq!(Document::from_ifcx(&bytes).unwrap(), d);
    }

    #[test]
    fn a_file_that_is_no_register_entry_says_why() {
        assert!(Document::from_ifcx(b"{ not json").unwrap_err().contains("IFCX"));
        let model = json!({"header": {}, "imports": [], "schemas": {}, "data": [{"path": "w1", "attributes": {"bsi::ifc::class": {"code": "IfcWall"}}}]});
        assert!(Document::from_ifcx(&serde_json::to_vec(&model).unwrap()).unwrap_err().contains("document"));
    }

    #[test]
    fn linking_an_element_twice_keeps_one_link() {
        let mut d = plan();
        let wall = ElementLink { key: "k1".into(), path: String::new(), name: "Wall".into(), model: "m.ifc".into() };
        assert!(d.link(wall.clone()));
        assert!(!d.link(wall));
        assert_eq!(d.elements.len(), 1);
    }

    #[test]
    fn two_entries_of_one_document_merge_to_the_last_change_with_the_revisions_and_links_of_both() {
        let base = plan();
        // an publishes it; bo, at the same time on an older copy, records P02 and links a wall
        let mut an = base.clone();
        an.status = Status::Published;
        an.touch("an", "2026-11-18T12:00:00Z");
        let mut bo = base.clone();
        bo.record("P02", "walls moved", &file("drawings/A-101.pdf", 4096, 1_794_100_000), "bo", "2026-11-18T11:00:00Z");
        bo.link(ElementLink { key: "k1".into(), path: String::new(), name: "Wall".into(), model: "m.ifc".into() });
        for merged in [merge(an.clone(), bo.clone()), merge(bo, an)] {
            assert_eq!(merged.status, Status::Published, "the entry changed last stands");
            assert_eq!((merged.changed.as_str(), merged.by.as_str()), ("2026-11-18T12:00:00Z", "an"));
            assert_eq!(merged.revisions.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(), ["P01", "P02"]);
            // the revision that stands is the last one recorded, whoever changed the entry last
            assert_eq!(merged.revision, "P02");
            assert_eq!(merged.elements.len(), 1);
        }
    }

    #[test]
    fn the_register_is_one_file_per_document_and_reads_back_sorted() {
        let root = project("roundtrip");
        let mut register = Register::read(&root);
        assert!(register.documents.is_empty());
        register.save(&root, plan(), "Suite", "2026-11-17T09:00:00Z").unwrap();
        register.save(&root, Document::new("11111111-2222-4333-8444-555555555555", "A-000.pdf"), "Suite", "2026-11-17T09:00:00Z").unwrap();
        let names: Vec<String> = std::fs::read_dir(root.join(crate::REGISTER_FOLDER)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!({ let mut n = names; n.sort(); n }, ["0b5c1c0e-7a52-4f6e-9a55-3a1d6a0c9f01.ifcx", "11111111-2222-4333-8444-555555555555.ifcx"]);
        let read = Register::read(&root);
        assert_eq!(read.documents.iter().map(|d| d.file.as_str()).collect::<Vec<_>>(), ["A-000.pdf", "drawings/A-101.pdf"]);
        assert_eq!(read.of("drawings/A-101.pdf"), Some(&plan()));
        assert_eq!(read.by_id(ID).map(Document::name), Some("Ground floor plan"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_conflict_copy_of_an_entry_is_read_as_one_with_it_and_tidied_at_the_next_write() {
        let root = project("conflict");
        let mut register = Register::read(&root);
        let mut an = plan();
        an.status = Status::Published;
        an.touch("an", "2026-11-18T12:00:00Z");
        register.save(&root, an, "Suite", "2026-11-18T12:00:00Z").unwrap();
        // the sync tool leaves bo's version beside it under another name
        let mut bo = plan();
        bo.record("P02", "", &file("drawings/A-101.pdf", 4096, 1_794_100_000), "bo", "2026-11-18T11:00:00Z");
        let copy = root.join(crate::REGISTER_FOLDER).join(format!("{ID} (bo's conflicted copy 2026-11-18).ifcx"));
        std::fs::write(&copy, serde_json::to_vec(&bo.to_ifcx("Suite", "2026-11-18T11:00:00Z")).unwrap()).unwrap();
        // and something that is no entry at all
        std::fs::write(root.join(crate::REGISTER_FOLDER).join("half.ifcx"), b"{").unwrap();

        let mut read = Register::read(&root);
        assert_eq!(read.documents.len(), 1);
        assert_eq!(read.unread, 1);
        let merged = read.documents[0].clone();
        assert_eq!((merged.status, merged.revision.as_str(), merged.revisions.len()), (Status::Published, "P02", 2));
        // the next write of the document leaves one file for it; what could not be read stays
        read.save(&root, merged, "Suite", "2026-11-18T13:00:00Z").unwrap();
        assert!(!copy.exists());
        assert!(root.join(crate::REGISTER_FOLDER).join("half.ifcx").exists());
        assert_eq!(Register::read(&root).documents.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_documents_about_an_element_are_found_by_its_key() {
        let mut register = Register::default();
        let mut d = plan();
        d.link(ElementLink { key: "k1".into(), path: String::new(), name: "Wall".into(), model: "m.ifc".into() });
        register.documents.push(d);
        register.documents.push(Document::new("x", "other.pdf"));
        assert_eq!(register.about("k1").map(|d| d.file.as_str()).collect::<Vec<_>>(), ["drawings/A-101.pdf"]);
        assert_eq!(register.about("k2").count(), 0);
    }

    #[test]
    fn a_status_is_written_in_lower_case() {
        assert_eq!(serde_json::to_string(&Status::ALL).unwrap(), r#"["work","shared","published","archived"]"#);
    }
}
