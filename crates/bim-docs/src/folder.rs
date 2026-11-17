//! What is in a project folder: its folders and files, without opening any of them.
//!
//! A file's size, time and attributes come from the folder's own listing, so a file the sync tool
//! keeps only online is not downloaded by looking at it.

use std::collections::HashSet;
use std::path::Path;

/// The most files one reading takes; a project folder with more says so ([`Listing::more`]).
pub const LIMIT: usize = 20_000;

/// A file of the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Its path in the project folder, with `/` between the names.
    pub path: String,
    pub size: u64,
    /// When it was last changed, in seconds since 1970 (0 when the folder does not say).
    pub modified: u64,
    /// The sync tool keeps it only online: opening it downloads it first.
    pub online_only: bool,
    /// A copy the sync tool made because two people changed the file at the same time.
    pub conflict: bool,
}

impl File {
    /// Its name, without the folders.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The folder it is in (`""` for the project folder itself).
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(folder, _)| folder)
    }

    pub fn kind(&self) -> Kind {
        Kind::of(&self.path)
    }
}

/// What kind of document a file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A model: IFC, IFCX, and the formats of modelling programs.
    Model,
    /// A point cloud.
    Scan,
    /// A 2D drawing (DWG, DXF).
    Drawing,
    Pdf,
    Image,
    /// Texts, spreadsheets and slides.
    Office,
    /// Plain text and data.
    Text,
    Other,
}

impl Kind {
    pub fn of(path: &str) -> Kind {
        let name = path.rsplit('/').next().unwrap_or(path);
        let extension = name.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_ascii_lowercase());
        match extension.as_str() {
            "ifc" | "ifcx" | "ifczip" | "ifcxml" | "rvt" | "rfa" | "skp" | "3dm" | "pln" | "pla" | "db1" | "ndw" | "dgn" | "stp" | "step" | "stpz" | "fbx" | "obj"
            | "glb" | "gltf" | "stl" | "dae" | "blend" | "vxz" | "vxm" | "btl" | "smc" | "3mf" | "usd" | "usda" | "usdc" | "usdz" | "fcstd" | "e2k" => Kind::Model,
            "las" | "laz" | "e57" | "pts" | "ptx" | "pcd" | "rcp" | "rcs" => Kind::Scan,
            "dwg" | "dxf" => Kind::Drawing,
            "pdf" => Kind::Pdf,
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "gif" | "svg" | "webp" => Kind::Image,
            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" => Kind::Office,
            "txt" | "md" | "csv" | "json" | "xml" | "bcf" | "bcfzip" | "ids" => Kind::Text,
            _ => Kind::Other,
        }
    }
}

/// A project folder as it was read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// Every folder under the project folder, as a path with `/`, sorted.
    pub folders: Vec<String>,
    /// Every file, sorted by path.
    pub files: Vec<File>,
    /// The folder holds more than [`LIMIT`] files: the rest was not read.
    pub more: bool,
}

impl Listing {
    /// The files directly in `folder` (`""` for the project folder itself).
    pub fn files_in<'a>(&'a self, folder: &'a str) -> impl Iterator<Item = &'a File> {
        self.files.iter().filter(move |f| f.folder() == folder)
    }

    /// The folders directly in `folder`.
    pub fn folders_in<'a>(&'a self, folder: &'a str) -> impl Iterator<Item = &'a str> {
        self.folders.iter().map(String::as_str).filter(move |f| f.rsplit_once('/').map_or("", |(parent, _)| parent) == folder)
    }

    /// How many files are in `folder` and the folders under it.
    pub fn count_under(&self, folder: &str) -> usize {
        if folder.is_empty() {
            return self.files.len();
        }
        self.files.iter().filter(|f| f.folder() == folder || f.folder().strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))).count()
    }

    pub fn file(&self, path: &str) -> Option<&File> {
        self.files.iter().find(|f| f.path == path)
    }
}

/// Whether a file or folder of this name is left out of the list: the sync tools' own files,
/// temporary and lock files, and the suite's own folder.
pub fn left_out(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // hidden files and folders, the marks and caches of the sync tools among them
    name.starts_with('.')
        || name == crate::SUITE_FOLDER
        // the lock file a text or spreadsheet program lays beside an open file
        || name.starts_with("~$")
        // what Windows, a Mac and a network storage keep for themselves
        || matches!(lower.as_str(), "desktop.ini" | "thumbs.db" | "system volume information" | "$recycle.bin" | "icon\r" | "@eadir" | "@recycle" | "@tmp" | "#recycle" | "#snapshot")
        || [".tmp", ".part", ".crdownload", ".partial"].iter().any(|end| lower.ends_with(end))
}

/// Whether `name` is the name a sync tool gives a conflict copy. `beside` says whether a file of a
/// given name lies in the same folder: one tool only adds the computer's name to the file's own.
pub fn is_conflict_copy(name: &str, beside: &dyn Fn(&str) -> bool) -> bool {
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    };
    let lower = stem.to_lowercase();
    // "plan (An's conflicted copy 2026-11-17).pdf", in the languages the tool speaks
    if ["conflicted copy", "conflicterende kopie", "in konflikt stehende kopie", "copie en conflit"].iter().any(|words| lower.contains(words)) {
        return true;
    }
    // "plan_DESKTOP-AB12CD_Nov-17-0914-2026_Conflict.pdf"
    if lower.ends_with("_conflict") {
        return true;
    }
    // "plan-DESKTOP-AB12CD.pdf" beside "plan.pdf": the computer's name after the file's own
    let Some((own, computer)) = stem.rsplit_once('-').and_then(|(before, last)| match before.rsplit_once('-') {
        // the names Windows gives a computer have a dash of their own
        Some((own, kind)) if matches!(kind, "DESKTOP" | "LAPTOP") => Some((own, last)),
        _ => None,
    }) else {
        return false;
    };
    let named = computer.len() >= 4 && computer.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    named && beside(&match extension {
        Some(extension) => format!("{own}.{extension}"),
        None => own.to_string(),
    })
}

/// Whether the attributes Windows gives a file say the sync tool keeps it only online: a
/// placeholder whose data comes when it is opened.
pub fn online_only(attributes: u32) -> bool {
    const OFFLINE: u32 = 0x1000;
    const RECALL_ON_OPEN: u32 = 0x4_0000;
    const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    attributes & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0
}

#[cfg(windows)]
fn only_online(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    online_only(metadata.file_attributes())
}

#[cfg(not(windows))]
fn only_online(_metadata: &std::fs::Metadata) -> bool {
    false
}

/// Reads the project folder at `root`: every folder and file under it, but not what is
/// [`left_out`]. Links to other folders are not followed.
pub fn scan(root: &Path) -> Result<Listing, String> {
    let mut listing = Listing::default();
    // folders still to read, as their path in the project
    let mut todo = vec![String::new()];
    while let Some(folder) = todo.pop() {
        let at = if folder.is_empty() { root.to_path_buf() } else { root.join(&folder) };
        let entries = match std::fs::read_dir(&at) {
            Ok(entries) => entries,
            // the project folder itself must be there; a folder inside it that cannot be read is skipped
            Err(e) if folder.is_empty() => return Err(format!("{}: {e}", root.display())),
            Err(_) => continue,
        };
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if left_out(&name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = if folder.is_empty() { name } else { format!("{folder}/{name}") };
            if kind.is_dir() {
                listing.folders.push(path.clone());
                todo.push(path);
            } else if kind.is_file() {
                // from the folder's listing: the file itself is not opened
                let Ok(metadata) = entry.metadata() else { continue };
                let modified = metadata.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                files.push(File { path, size: metadata.len(), modified, online_only: only_online(&metadata), conflict: false });
            }
            // a link to another folder or file is not followed
        }
        let names: HashSet<String> = files.iter().map(|f| f.name().to_string()).collect();
        for file in &mut files {
            file.conflict = is_conflict_copy(file.name(), &|name| names.contains(name));
        }
        listing.files.append(&mut files);
        if listing.files.len() > LIMIT {
            listing.files.truncate(LIMIT);
            listing.more = true;
            break;
        }
    }
    listing.folders.sort();
    listing.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(listing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh folder for a test, with `files` in it (a path ending in `/` is an empty folder).
    fn project(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bim-docs-folder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for file in files {
            let path = root.join(file);
            if file.ends_with('/') {
                std::fs::create_dir_all(&path).unwrap();
            } else {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, file.as_bytes()).unwrap();
            }
        }
        root
    }

    #[test]
    fn a_scan_lists_folders_and_files_by_their_path_in_the_project() {
        let root = project("lists", &["model/kade12.ifc", "drawings/plans/A-101.pdf", "drawings/A-000.pdf", "notes.txt", "empty/"]);
        let listing = scan(&root).unwrap();
        assert_eq!(listing.folders, ["drawings", "drawings/plans", "empty", "model"]);
        let paths: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["drawings/A-000.pdf", "drawings/plans/A-101.pdf", "model/kade12.ifc", "notes.txt"]);
        let model = listing.file("model/kade12.ifc").unwrap();
        assert_eq!((model.name(), model.folder(), model.size, model.kind()), ("kade12.ifc", "model", 16, Kind::Model));
        assert!(model.modified > 1_700_000_000, "the time the file was written: {}", model.modified);
        assert!(!listing.more);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_gives_its_own_files_and_folders_and_counts_what_is_under_it() {
        let root = project("under", &["drawings/plans/A-101.pdf", "drawings/plans/A-102.pdf", "drawings/A-000.pdf", "notes.txt"]);
        let listing = scan(&root).unwrap();
        assert_eq!(listing.files_in("drawings").map(File::name).collect::<Vec<_>>(), ["A-000.pdf"]);
        assert_eq!(listing.files_in("").map(File::name).collect::<Vec<_>>(), ["notes.txt"]);
        assert_eq!(listing.folders_in("").collect::<Vec<_>>(), ["drawings"]);
        assert_eq!(listing.folders_in("drawings").collect::<Vec<_>>(), ["drawings/plans"]);
        assert_eq!((listing.count_under(""), listing.count_under("drawings"), listing.count_under("drawings/plans")), (4, 3, 2));
        // a folder whose name starts the same is another folder
        assert_eq!(listing.count_under("draw"), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_sync_tools_own_files_and_the_register_are_left_out() {
        let root = project(
            "leftout",
            &[".dropbox", ".dropbox.cache/x", ".SynologyWorkingDirectory/x", "_openaec/docs/a.ifcx", "desktop.ini", "Thumbs.db", "~$report.docx", "report.docx", "plan.pdf.part", "old.tmp", "@eaDir/x", "#recycle/x"],
        );
        let listing = scan(&root).unwrap();
        assert_eq!(listing.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["report.docx"]);
        assert!(listing.folders.is_empty(), "{:?}", listing.folders);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conflict_copies_are_known_by_the_names_the_tools_give_them() {
        let none = |_: &str| false;
        assert!(is_conflict_copy("plan (An's conflicted copy 2026-11-17).pdf", &none));
        assert!(is_conflict_copy("plan (Conflicterende kopie van An 2026-11-17).pdf", &none));
        assert!(is_conflict_copy("plan_DESKTOP-AB12CD_Nov-17-0914-2026_Conflict.pdf", &none));
        assert!(!is_conflict_copy("plan.pdf", &none));
        assert!(!is_conflict_copy("conflict analysis.pdf", &none));
        // the computer's name after the file's own, only beside the file itself
        let beside = |name: &str| name == "plan.pdf";
        assert!(is_conflict_copy("plan-DESKTOP-AB12CD.pdf", &beside));
        assert!(is_conflict_copy("plan-LAPTOP-9XK2.pdf", &beside));
        assert!(!is_conflict_copy("plan-DESKTOP-AB12CD.pdf", &none));
        assert!(!is_conflict_copy("plan-B.pdf", &beside));
    }

    #[test]
    fn a_scan_marks_the_conflict_copies() {
        let root = project("conflict", &["plan.pdf", "plan-DESKTOP-AB12CD.pdf", "model (Bo's conflicted copy 2026-11-17).ifc"]);
        let listing = scan(&root).unwrap();
        let marked: Vec<&str> = listing.files.iter().filter(|f| f.conflict).map(File::name).collect();
        assert_eq!(marked, ["model (Bo's conflicted copy 2026-11-17).ifc", "plan-DESKTOP-AB12CD.pdf"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_that_is_not_there_says_so() {
        let error = scan(&std::env::temp_dir().join("bim-docs-no-such-folder")).unwrap_err();
        assert!(error.contains("bim-docs-no-such-folder"), "{error}");
    }

    #[test]
    fn a_placeholder_of_the_sync_tool_is_only_online() {
        // FILE_ATTRIBUTE_ARCHIVE alone: an ordinary file
        assert!(!online_only(0x20));
        // pinned (always on this device) is not a placeholder
        assert!(!online_only(0x20 | 0x8_0000));
        // unpinned with its data to be recalled: only online
        assert!(online_only(0x20 | 0x10_0000 | 0x40_0000));
        assert!(online_only(0x1000));
        assert!(online_only(0x4_0000));
    }

    #[test]
    fn kinds_follow_the_extension_in_any_case() {
        assert_eq!(Kind::of("a/b/Model.IFCX"), Kind::Model);
        assert_eq!(Kind::of("scan.laz"), Kind::Scan);
        assert_eq!(Kind::of("A-101.DWG"), Kind::Drawing);
        assert_eq!(Kind::of("A-101.pdf"), Kind::Pdf);
        assert_eq!(Kind::of("photo.JPG"), Kind::Image);
        assert_eq!(Kind::of("report.docx"), Kind::Office);
        assert_eq!(Kind::of("issues.bcf"), Kind::Text);
        assert_eq!(Kind::of("no extension"), Kind::Other);
    }
}
