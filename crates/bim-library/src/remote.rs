//! Libraries that are not compiled in: library files kept somewhere else (a repository, a
//! server, a folder) with an index that says what each holds, so a program lists them without
//! reading them and fetches the one that is asked for.
//!
//! - A **library file** is an IFCX file of a library's nodes ([`crate::ifcx::file`]).
//! - The **index** (`index.json`) names each file with its key, its names, what it is about and
//!   how many of each kind of thing it holds, and how many types it has at each level of
//!   development ([`crate::lod`]).
//!
//! [`entry`] makes the index entry of a library; [`Index::parse`] and [`library`] read what was
//! fetched. Fetching itself is the program's: this crate has no network.

use crate::lod::Lod;
use crate::material::Names;
use crate::{Library, LibraryError};
use serde::{Deserialize, Serialize};

/// How many of each kind of thing a library holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    #[serde(default)]
    pub materials: usize,
    #[serde(default)]
    pub buildups: usize,
    #[serde(default)]
    pub linear_types: usize,
    #[serde(default)]
    pub system_types: usize,
    #[serde(default)]
    pub object_types: usize,
    #[serde(default)]
    pub connection_types: usize,
    #[serde(default)]
    pub drawing_sets: usize,
    #[serde(default)]
    pub view_templates: usize,
}

impl Counts {
    /// Everything it holds, counted together.
    pub fn total(&self) -> usize {
        self.materials + self.buildups + self.linear_types + self.system_types + self.object_types + self.connection_types + self.drawing_sets + self.view_templates
    }
}

/// A library file of an index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Stable, lower case with hyphens.
    pub key: String,
    /// The file, relative to the index.
    pub file: String,
    pub names: Names,
    /// What it is about, in English.
    #[serde(default)]
    pub about: String,
    /// Where its content comes from and under which licence.
    #[serde(default)]
    pub source: String,
    pub counts: Counts,
    /// How many of its types are at each level of development: LOD 200, 300, 350 and 400.
    #[serde(default)]
    pub lod: [usize; 4],
}

/// The index of a place that keeps library files.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    /// What the place is called.
    #[serde(default)]
    pub name: String,
    pub libraries: Vec<Entry>,
}

impl Index {
    /// The index in `text`. Refused when it is no index, or names a file twice or outside its
    /// own place (a path that climbs out, or an address).
    pub fn parse(text: &str) -> Result<Index, LibraryError> {
        let index: Index = serde_json::from_str(text).map_err(|e| LibraryError::Unreadable(format!("index: {e}")))?;
        for (k, entry) in index.libraries.iter().enumerate() {
            let inside = !entry.file.is_empty() && !entry.file.starts_with('/') && !entry.file.contains("..") && !entry.file.contains(':') && !entry.file.contains('\\');
            if entry.key.trim().is_empty() || !inside {
                return Err(LibraryError::Unreadable(format!("index: {:?} is no file of this place", entry.file)));
            }
            if index.libraries[..k].iter().any(|e| e.key == entry.key || e.file == entry.file) {
                return Err(LibraryError::Duplicate(entry.key.clone()));
            }
        }
        Ok(index)
    }

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.libraries.iter().find(|e| e.key == key)
    }
}

/// How many types of `library` are at each level of development: its build-ups, linear types,
/// system types and object types, at LOD 200, 300, 350 and 400.
pub fn levels(library: &Library) -> [usize; 4] {
    let mut count = [0; 4];
    let mut at = |lod: Lod| count[Lod::ALL.iter().position(|l| *l == lod).unwrap_or(0)] += 1;
    library.buildups.iter().for_each(|b| at(b.lod()));
    library.linear_types.iter().for_each(|t| at(t.lod()));
    library.system_types.iter().for_each(|t| at(t.lod()));
    library.object_types.iter().for_each(|o| at(o.lod));
    library.connection_types.iter().for_each(|c| at(c.lod()));
    count
}

/// The index entry of `library`, kept in `file`.
pub fn entry(library: &Library, key: &str, file: &str, names: Names, about: &str, source: &str) -> Entry {
    Entry {
        key: key.into(),
        file: file.into(),
        names,
        about: about.into(),
        source: source.into(),
        counts: Counts {
            materials: library.materials.len(),
            buildups: library.buildups.len(),
            linear_types: library.linear_types.len(),
            system_types: library.system_types.len(),
            object_types: library.object_types.len(),
            connection_types: library.connection_types.len(),
            drawing_sets: library.drawing_sets.len(),
            view_templates: library.view_templates.len(),
        },
        lod: levels(library),
    }
}

/// The library in the text of a library file that was fetched.
pub fn library(text: &str) -> Result<Library, LibraryError> {
    let file: serde_json::Value = serde_json::from_str(text).map_err(|e| LibraryError::Unreadable(format!("library file: {e}")))?;
    crate::ifcx::read(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_says_what_a_library_holds_without_reading_it() {
        let starter = Library::starter();
        let made = entry(&starter, "general", "ifcx/general.ifcx", Names::new("algemeen", "general", "allgemein", "général"), "The suite's starter library.", "Open BIM Suite, LGPL-3.0-or-later");
        assert_eq!((made.counts.materials, made.counts.object_types, made.counts.view_templates), (starter.materials.len(), starter.object_types.len(), 20));
        assert_eq!(made.counts.total(), starter.materials.len() + starter.buildups.len() + starter.linear_types.len() + starter.system_types.len() + starter.object_types.len() + starter.connection_types.len() + starter.drawing_sets.len() + starter.view_templates.len());
        // every type is at one level
        assert_eq!(made.lod.iter().sum::<usize>(), starter.buildups.len() + starter.linear_types.len() + starter.system_types.len() + starter.object_types.len() + starter.connection_types.len());
        assert!(made.lod[0] >= starter.object_types.len(), "the generic objects are LOD 200: {:?}", made.lod);
        let index = Index { name: "Open BIM Library".into(), libraries: vec![made.clone()] };
        let text = serde_json::to_string_pretty(&index).unwrap();
        assert_eq!(Index::parse(&text).unwrap(), index);
        assert_eq!(index.entry("general"), Some(&made));
        // and the file reads back as the library
        let file = serde_json::to_string(&crate::ifcx::file(&starter, "general.ifcx", "test", "2026-11-19T00:00:00Z")).unwrap();
        assert_eq!(library(&file).unwrap(), starter);
        assert!(library("{}").is_err() && library("no json").is_err());
    }

    #[test]
    fn an_index_names_files_of_its_own_place_each_once() {
        let of = |entries: &[(&str, &str)]| {
            let libraries: Vec<serde_json::Value> = entries.iter().map(|(key, file)| serde_json::json!({"key": key, "file": file, "names": {"nl": "x", "en": "x", "de": "x", "fr": "x"}, "counts": {}})).collect();
            Index::parse(&serde_json::json!({"libraries": libraries}).to_string())
        };
        assert_eq!(of(&[("a", "ifcx/a.ifcx"), ("b", "ifcx/b.ifcx")]).map(|i| i.libraries.len()), Ok(2));
        // what an older index lacks reads as nothing
        assert_eq!(of(&[("a", "a.ifcx")]).unwrap().libraries[0].lod, [0; 4]);
        for outside in ["../secret.ifcx", "/etc/passwd", "https://example.org/a.ifcx", "c:\\a.ifcx", ""] {
            assert!(of(&[("a", outside)]).is_err(), "{outside}");
        }
        assert!(of(&[("a", "a.ifcx"), ("a", "b.ifcx")]).is_err() && of(&[("a", "a.ifcx"), ("b", "a.ifcx")]).is_err());
        assert!(Index::parse("[]").is_err());
    }
}
