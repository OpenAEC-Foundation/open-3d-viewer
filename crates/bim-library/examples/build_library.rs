//! Builds the library files of a place that keeps libraries (a repository): the suite's own
//! libraries as IFCX files, the object catalogues of a folder, and the index of them all.
//!
//!     cargo run -p bim-library --example build_library -- OBJECTS_DIR OUT_DIR TIMESTAMP
//!
//! `OBJECTS_DIR` holds the object catalogues, a JSON list of object types per file
//! (`architecture.json`, `ventilation.json`, ...); beside it a folder `types` may hold
//! catalogues of linear types (`pipes.json`, `structure.json`, ...), each entry a linear type or
//! a steel profile of the steel table by its designation (`{"id", "role", "designation"}`).
//! `OUT_DIR` gets `ifcx/*.ifcx` and `index.json`; `TIMESTAMP` (`YYYY-MM-DDTHH:MM:SSZ`) goes into
//! the headers, so a build of the same content at the same time gives the same files.

use bim_library::object::ObjectType;
use bim_library::remote::{self, Entry, Index};
use bim_library::{Lang, Library, LinearRole, LinearType, Names};
use std::path::Path;

const AUTHOR: &str = "Open BIM Suite";
const SUITE: &str = "Open BIM Suite (OpenAEC Foundation), LGPL-3.0-or-later";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [objects, out, timestamp] = args.as_slice() else {
        return Err("usage: build_library OBJECTS_DIR OUT_DIR TIMESTAMP".into());
    };
    let (objects, out) = (Path::new(objects), Path::new(out));
    std::fs::create_dir_all(out.join("ifcx"))?;
    let mut entries: Vec<Entry> = Vec::new();
    let mut write = |library: &Library, key: &str, names: Names, about: &str, source: &str| -> Result<(), Box<dyn std::error::Error>> {
        let file = format!("ifcx/{key}.ifcx");
        let document = bim_library::ifcx::file(library, &format!("{key}.ifcx"), AUTHOR, timestamp);
        std::fs::write(out.join(&file), serde_json::to_string(&document)? + "\n")?;
        // what was written reads back as it was
        let back = remote::library(&std::fs::read_to_string(out.join(&file))?)?;
        if back != *library {
            return Err(format!("{key}: the file does not read back as the library").into());
        }
        entries.push(remote::entry(library, key, &file, names, about, source));
        Ok(())
    };
    write(
        &Library::starter(),
        "general",
        Names::new("algemeen", "general", "allgemein", "général"),
        "The library every project starts with: materials, build-ups, linear and system types, a generic object of each group, NL-SfB codes, drawing sets and view templates.",
        SUITE,
    )?;
    write(
        &bim_library::dutch::library(),
        "nl",
        Names::new("Nederland", "the Netherlands", "Niederlande", "Pays-Bas"),
        "Dutch materials and types: the INB template, the materials, timber sizes, concrete sections, walls and floors of Project Ocondat, and usual Dutch layered build-ups.",
        "INB template and Project Ocondat (OpenAEC Foundation), LGPL-3.0; layered build-ups: Open BIM Suite",
    )?;
    // the object catalogues, a library file each, in the order of their names
    let mut catalogues: Vec<_> = std::fs::read_dir(objects)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    catalogues.sort();
    for path in catalogues {
        let trade = path.file_stem().and_then(|s| s.to_str()).ok_or("a catalogue has a name")?.to_string();
        let list: Vec<ObjectType> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        // the materials objects name are the starter's; the codes come with them
        let mut library = Library { materials: bim_library::material::starter(), codes: bim_library::nlsfb::starter(), ..Library::default() };
        for object in list {
            let id = object.id.clone();
            library.add_object_type(object).map_err(|e| format!("{trade}: {id}: {e}"))?;
        }
        // (only the materials objects are made of go into the file)
        let used: Vec<String> = library.object_types.iter().filter_map(|o| o.material.clone()).collect();
        library.materials.retain(|m| used.contains(&m.key));
        let first = library.object_types.first().map(|o| o.group.trade());
        let name = |lang: Lang| first.map_or(trade.clone(), |t| format!("{} {}", if lang == Lang::Nl { "objecten" } else { "objects" }, t.name(lang)));
        let names = Names::new(&name(Lang::Nl), &name(Lang::En), &name(Lang::En), &name(Lang::En));
        write(&library, &format!("nl-objects-{trade}"), names, "Generic objects of usual Dutch sizes: one at LOD 200 per kind and its usual sizes at LOD 300.", SUITE)?;
    }
    // the catalogues of linear types, a library file each
    let types = objects.parent().map(|p| p.join("types")).filter(|p| p.is_dir());
    let mut catalogues: Vec<_> = types.iter().flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten()).filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    catalogues.sort();
    let dutch = bim_library::dutch::library();
    for path in catalogues {
        let what = path.file_stem().and_then(|s| s.to_str()).ok_or("a catalogue has a name")?.to_string();
        let list: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        // the materials are the Dutch library's; only those the types are of go into the file
        let mut library = Library { materials: dutch.materials.clone(), codes: bim_library::nlsfb::starter(), ..Library::default() };
        for entry in list {
            let id = entry["id"].as_str().unwrap_or_default().to_string();
            let linear = match entry.get("designation").and_then(|d| d.as_str()) {
                Some(designation) => {
                    let role: LinearRole = serde_json::from_value(entry["role"].clone())?;
                    LinearType::steel(&id, role, designation).ok_or_else(|| format!("{what}: the steel table has no {designation}"))?
                }
                None => serde_json::from_value::<LinearType>(entry)?,
            };
            library.add_linear_type(linear).map_err(|e| format!("{what}: {id}: {e}"))?;
        }
        let used: Vec<String> = library.linear_types.iter().map(|t| t.material.clone()).collect();
        library.materials.retain(|m| used.contains(&m.key));
        let (nl, en) = match what.as_str() {
            "structure" => ("constructie: balken en kolommen", "structure: beams and columns"),
            "foundation" => ("fundering: palen en balken", "foundation: piles and beams"),
            "timber" => ("hout: regels, latten, kozijnhout en plinten", "timber: studs, battens, frame sections and skirtings"),
            "facade" => ("gevel: stijlen en regels", "facade: mullions"),
            "pipes" => ("leidingen", "pipes"),
            "ducts" => ("luchtkanalen", "ducts"),
            "cable-trays" => ("kabelgoten", "cable trays"),
            other => (other, other),
        };
        write(&library, &format!("nl-types-{what}"), Names::new(nl, en, en, en), "Linear types of usual Dutch sizes: steel profiles of the steel table, timber, concrete, pipes, ducts and trays.", SUITE)?;
    }
    // the starter house style, a file of its own
    let style = bim_library::house::HouseStyle::starter();
    std::fs::write(out.join("ifcx/house-style-openaec.ifcx"), serde_json::to_string(&bim_library::house::file(&style, "house-style-openaec.ifcx", AUTHOR, timestamp))? + "\n")?;
    let index = Index { name: "Open BIM Library".into(), libraries: entries };
    std::fs::write(out.join("index.json"), serde_json::to_string_pretty(&index)? + "\n")?;
    for e in &index.libraries {
        println!("{:<28} {:>5} things, LOD 200/300/350/400: {:?}", e.key, e.counts.total(), e.lod);
    }
    Ok(())
}
