//! Docs: the documents of the project (`bim-docs`).
//!
//! The project is a folder that a sync tool or a network drive keeps the same for everyone:
//! Dropbox, SharePoint through OneDrive, Synology Drive, or any shared folder. Docs is a product
//! on the switch at the top of the window: its page fills the workspace with the files of the
//! picked folder, with the status and revision that were recorded of each and what the sync tool
//! is doing to it; the folders are the left pane.
//!
//! Nothing stands over the list. Choosing the folder, opening a file, its title and status,
//! recording a revision and linking it to the selected elements are sections of the right pane.
//!
//! It works as a file manager does: a path to click above the list, the folders of the folder
//! that shows above its files, an icon per kind of file, columns to sort by, a search through the
//! whole project folder, and the keys of a file manager (up and down, Enter, Backspace). A second
//! click on a file opens it (a model in the suite, anything else in its own program), and the open
//! model is saved into the project folder.
//!
//! And as a document management system: recording a revision keeps a copy of the file, so an
//! earlier revision opens beside the present one or is put back in its place; a document is
//! checked out while someone works on it; and a file is shared through the service that keeps the
//! folder (its page on GitHub, SharePoint or Dropbox, a link, a mail). Where the folder is a Git
//! checkout, Git keeps the versions instead of copies.
//!
//! Docs writes only when the person does something, and never opens a file to list it (see
//! `docs/superpowers/specs/2026-11-17-docs-design.md`).

use super::{App, Message, FONT_BOLD};
use crate::selection::Pickable;
use crate::theme::Palette;
use bim_docs::git::{Commit, Standing as GitStanding};
use bim_docs::{Document, ElementLink, File, Kind, Listing, Register, Service, State, Status as Standing, Sync};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable, svg, text, text_input, Column};
use iced::{Center, Element, Fill, Padding, Task};
use openaec_ui::Language;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use web_time::{Duration, Instant};

/// The Docs tab's state.
#[derive(Debug, Clone)]
pub struct DocsTab {
    /// Its tab is open, and shown (it fills the workspace).
    pub open: bool,
    pub shown: bool,
    /// Docs is the product shown: the folders are the left pane, not part of the page.
    pub in_product: bool,
    /// The project folder, and whether it is being read.
    pub folder: Option<PathBuf>,
    pub reading: bool,
    /// The folder as it was read, what is said about its documents, and the tool that syncs it.
    pub listing: Arc<Listing>,
    pub register: Register,
    pub sync: Option<Sync>,
    /// How many files each folder holds, with the folders under it.
    counts: HashMap<String, usize>,
    /// The register of the folder was read, by Docs itself or for a model that opened.
    known: bool,
    /// The folder whose files show (`""` for the project folder itself), the folders of the tree
    /// that are open, and the picked file.
    pub at: String,
    pub opened: HashSet<String>,
    pub pick: Option<String>,
    /// Being typed: the picked document's title, and the code and note of a revision to record.
    pub title: String,
    pub code: String,
    pub note: String,
    /// What went wrong last, said in the right pane.
    pub notice: Option<Notice>,
    /// The file clicked last and when: a second click soon after opens it.
    clicked: Option<(String, Instant)>,
    /// What Git says of the picked file, when the folder is a checkout: the file it was asked
    /// of, its versions (newest first) and how it stands against the last one.
    pub versions: Option<Versions>,
    /// The branch the checkout is on, for the file's address on the website.
    pub branch: Option<String>,
    /// What is typed to search the whole project folder; empty shows the folder.
    pub query: String,
    /// The column the files are sorted by, and whether the order is turned round.
    pub sort: (By, bool),
    /// The version that goes back in the file's place at the next press of its button.
    pub confirm: Option<Version>,
    /// A copy is being kept or put back.
    pub busy: bool,
}

/// A column of the list of files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Name,
    Kind,
    Status,
    Revision,
    Changed,
    Size,
}

/// An earlier version of the picked file: a recorded revision (by its code and date), or a
/// version Git keeps (by its hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    Recorded { code: String, date: String },
    Git(String),
}

/// A revision to record, with the copy that was kept of its file (or why none was).
#[derive(Debug, Clone)]
pub struct Kept {
    pub path: String,
    pub code: String,
    pub note: String,
    pub file: File,
    /// The copy's path in the project folder; empty where Git keeps the versions.
    pub copy: Result<String, String>,
}

/// What Git says of a file of the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versions {
    /// The file, as its path in the project folder.
    pub path: String,
    pub result: Result<(Vec<Commit>, GitStanding), String>,
}

/// What Docs has to say after something the person did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A revision needs a code before it is recorded.
    CodeFirst,
    /// The revision was recorded, but no copy of its file could be kept: why.
    NotKept(String),
    /// An earlier version was put back; the file as it was before is kept here.
    PutBack(PathBuf),
    /// An address or path is on the clipboard.
    Copied,
    /// Reading or writing went wrong: what the computer said.
    Failed(String),
}

impl Default for DocsTab {
    fn default() -> Self {
        // the folder chosen before, if it is still there
        let folder = folder_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|s| PathBuf::from(s.trim())).filter(|p| p.is_dir());
        DocsTab { folder, ..DocsTab::empty() }
    }
}

impl DocsTab {
    /// Docs with no project folder and nothing read.
    fn empty() -> Self {
        DocsTab {
            open: false,
            shown: false,
            in_product: false,
            folder: None,
            reading: false,
            listing: Arc::default(),
            register: Register::default(),
            sync: None,
            counts: HashMap::new(),
            known: false,
            at: String::new(),
            opened: HashSet::new(),
            pick: None,
            title: String::new(),
            code: String::new(),
            note: String::new(),
            notice: None,
            clicked: None,
            versions: None,
            branch: None,
            query: String::new(),
            sort: (By::Name, false),
            confirm: None,
            busy: false,
        }
    }
}

/// A project folder as it was read: what is in it, what is said about it, what syncs it.
#[derive(Debug, Clone)]
pub struct Read {
    pub folder: PathBuf,
    pub result: Result<(Listing, Register, Sync), String>,
    /// The branch, when the folder is in a Git checkout.
    pub branch: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DocsMessage {
    /// Open the Docs tab, or show it again.
    Open,
    Close,
    /// Choose the project folder; the folder chosen (none when the dialog was closed).
    Choose,
    Chosen(Option<PathBuf>),
    /// Let go of the project folder.
    Leave,
    /// Read the project folder again; what was read.
    Refresh,
    Read(Box<Read>),
    /// Show a folder's files (and open it in the tree, or close it when it shows already).
    Folder(String),
    /// Pick a file.
    Pick(String),
    /// The picked document's title as it is typed, and keeping it.
    Title(String),
    SaveTitle,
    Standing(Standing),
    /// The code and note of the revision to record, and recording it.
    Code(String),
    Note(String),
    Record,
    /// Open the picked model in the suite (`add`: beside the open model).
    OpenModel { add: bool },
    /// Open the picked file in the program this computer opens it with; show it in its folder.
    OpenOutside,
    Reveal,
    /// Link the picked document to the selected elements; undo the link of the element with this
    /// key; select the linked element with this key in the model.
    Link,
    Unlink(String),
    Go(String),
    /// Open Docs at the document of this file (its path in the project folder).
    Show(String),
    /// The register of this project folder as it was read for a model, before Docs was opened.
    Known(Box<(PathBuf, Register)>),
    /// What Git says of a file, as it was read.
    Versions(Box<Versions>),
    /// Open the picked file as it was in the version with this hash; that version as a file of
    /// its own, ready to open.
    OpenVersion(String),
    VersionReady(Result<PathBuf, String>),
    /// Save the open model into the project folder.
    Save,
    /// Search the whole project folder for this text.
    Query(String),
    /// Sort the files by this column (again: the other way round).
    Sort(By),
    /// The keys of a file manager: the folder above, the file above or below, open the picked one.
    Up,
    Step(i32),
    Enter,
    /// A revision was recorded, with the copy kept of its file.
    Kept(Box<Kept>),
    /// Open a recorded revision's copy, by its code and date.
    OpenRecorded { code: String, date: String },
    /// Put an earlier version back in the file's place (the first press asks to press again);
    /// the file as it was before, kept aside.
    PutBack(Version),
    PutBackDone(Result<PathBuf, String>),
    /// Check the picked document out, or in again.
    CheckOut,
    CheckIn,
    /// Put this text (a path or an address) on the clipboard.
    Copy(String),
    /// Open this address: a page on the website, or a new mail.
    Visit(String),
}

/// The folder of the tree that lists the documents whose file is gone: no file has this name.
const MISSING: &str = "\0";
/// The most files a folder shows; the rest is said as a number.
const ROWS: usize = 500;
/// A second click on a file within this time opens it.
const SECOND_CLICK: Duration = Duration::from_millis(450);
/// The most versions of a file that are asked of Git.
const VERSIONS: usize = 30;
/// A file up to this size is copied at once; a larger one on a thread of its own.
const AT_ONCE: u64 = 16 << 20;

/// What Git says of the file at `path` of the checkout at `root`.
fn read_versions(root: &std::path::Path, path: &str) -> Result<(Vec<Commit>, GitStanding), String> {
    Ok((bim_docs::git::history(root, path, VERSIONS)?, bim_docs::git::standing_of(root, path)?))
}

/// Asks Git on a thread of its own: a large history can take its time.
#[cfg(not(target_arch = "wasm32"))]
fn versions_task(root: PathBuf, inside: String, path: String) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let asked = path.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Versions { result: read_versions(&root, &inside), path });
            });
            rx.await.unwrap_or_else(|_| Versions { path: asked, result: Err("the reading stopped".into()) })
        },
        |versions| Message::Docs(DocsMessage::Versions(Box::new(versions))),
    )
}

#[cfg(target_arch = "wasm32")]
fn versions_task(_root: PathBuf, _inside: String, _path: String) -> Task<Message> {
    Task::none()
}

/// Writes the file at `inside` of the checkout at `root` as it was in version `hash` to a file
/// of its own, outside the project folder: `<name> @ <hash>.<extension>` in the temporary folder.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(super) fn version_file(root: &std::path::Path, inside: &str, hash: &str) -> Result<PathBuf, String> {
    let bytes = bim_docs::git::content(root, inside, hash)?;
    let name = inside.rsplit('/').next().unwrap_or(inside);
    let short = hash.get(..7).unwrap_or(hash);
    let named = match name.rsplit_once('.') {
        Some((stem, extension)) => format!("{stem} @ {short}.{extension}"),
        None => format!("{name} @ {short}"),
    };
    let folder = std::env::temp_dir().join("open-bim-suite").join("versions");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(named);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn version_task(root: PathBuf, inside: String, hash: String) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(version_file(&root, &inside, &hash));
            });
            rx.await.unwrap_or_else(|_| Err("the reading stopped".into()))
        },
        |ready| Message::Docs(DocsMessage::VersionReady(ready)),
    )
}

#[cfg(target_arch = "wasm32")]
fn version_task(_root: PathBuf, _inside: String, _hash: String) -> Task<Message> {
    Task::none()
}

/// Does `job` on a thread of its own and hands its result to `done`: copying a large file takes
/// its time.
#[cfg(not(target_arch = "wasm32"))]
fn work<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) -> Message + Send + 'static) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(job());
            });
            rx.await.ok()
        },
        move |result| match result {
            Some(result) => done(result),
            None => Message::Docs(DocsMessage::PutBackDone(Err("the work stopped".into()))),
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn work<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) -> Message + Send + 'static) -> Task<Message> {
    Task::done(done(job()))
}

/// Opens `address` (a web page, or `mailto:` for a new mail) in the program this computer has for
/// it.
#[cfg(not(target_arch = "wasm32"))]
fn visit(address: &str) -> Result<(), String> {
    use std::process::Command;
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", address]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(address);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(address);
        command
    };
    command.spawn().map(|_| ()).map_err(|e| format!("{address}: {e}"))
}

#[cfg(target_arch = "wasm32")]
fn visit(_address: &str) -> Result<(), String> {
    Ok(())
}

/// Keeps the file at `path` aside before an earlier version takes its place: a copy named after
/// it and the time, in the temporary folder. Gives where.
fn keep_aside(path: &std::path::Path) -> Result<PathBuf, String> {
    let folder = std::env::temp_dir().join("open-bim-suite").join("versions");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let stem = path.file_stem().map_or_else(|| "file".into(), |s| s.to_string_lossy().into_owned());
    let time = crate::export::ifcx::timestamp().replace([':', '-'], "");
    let named = match path.extension() {
        Some(extension) => format!("{stem} @ {time}.{}", extension.to_string_lossy()),
        None => format!("{stem} @ {time}"),
    };
    let aside = folder.join(named);
    std::fs::copy(path, &aside).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(aside)
}

/// Writes `bytes` beside `path` and then in its place, so nobody opens half a file.
fn write_over(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension("part");
    std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path)).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("{}: {e}", path.display())
    })
}

/// Where Docs keeps the project folder that was chosen: beside the preferences.
fn folder_file() -> Option<PathBuf> {
    // a test starts as a first run: it does not take over what the person set on this machine
    if cfg!(test) {
        return None;
    }
    Some(crate::remote_link::settings_dir()?.join("docs-folder.txt"))
}

/// Who is at this computer: the name given to the BIM Validator, else the name they logged in with.
fn who() -> String {
    let kept = crate::remote_link::settings_dir()
        .filter(|_| !cfg!(test))
        .and_then(|folder| std::fs::read_to_string(folder.join("collaborator-user.txt")).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    kept.unwrap_or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "user".into()))
}

/// Reads the project folder at `folder`: its files, its register and the tool that syncs it.
pub(super) fn read_project(folder: PathBuf) -> Read {
    let result = bim_docs::folder::scan(&folder).map(|listing| (listing, Register::read(&folder), bim_docs::service::of(&folder, &bim_docs::service::known())));
    let branch = result.as_ref().ok().map(|(_, _, sync)| sync).filter(|sync| sync.service.git()).and_then(|sync| bim_docs::git::branch(&sync.root));
    Read { folder, result, branch }
}

/// Reads the project folder on a thread of its own: a network folder can take its time.
#[cfg(not(target_arch = "wasm32"))]
fn read_task(folder: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            let asked = folder.clone();
            std::thread::spawn(move || {
                let _ = tx.send(read_project(folder));
            });
            rx.await.unwrap_or_else(|_| Read { folder: asked, result: Err("the reading stopped".into()), branch: None })
        },
        |read| Message::Docs(DocsMessage::Read(Box::new(read))),
    )
}

/// The browser has no folders of the computer.
#[cfg(target_arch = "wasm32")]
fn read_task(folder: PathBuf) -> Task<Message> {
    Task::done(Message::Docs(DocsMessage::Read(Box::new(Read { folder, result: Err("Docs works in the desktop app".into()), branch: None }))))
}

/// Reads the register of the project folder on a thread of its own, for a model that opened.
#[cfg(not(target_arch = "wasm32"))]
fn register_task(folder: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let register = Register::read(&folder);
                let _ = tx.send((folder, register));
            });
            rx.await.ok()
        },
        |read| match read {
            Some(read) => Message::Docs(DocsMessage::Known(Box::new(read))),
            None => Message::Docs(DocsMessage::Known(Box::new((PathBuf::new(), Register::default())))),
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn register_task(_folder: PathBuf) -> Task<Message> {
    Task::none()
}

#[cfg(not(target_arch = "wasm32"))]
fn choose_folder() -> Task<Message> {
    Task::perform(async { rfd::AsyncFileDialog::new().pick_folder().await.map(|f| f.path().to_path_buf()) }, |folder| Message::Docs(DocsMessage::Chosen(folder)))
}

#[cfg(target_arch = "wasm32")]
fn choose_folder() -> Task<Message> {
    Task::none()
}

/// Opens `path` in the program this computer opens it with, or (`reveal`) shows it in its folder.
/// A file that is only online is fetched by the sync tool first.
#[cfg(not(target_arch = "wasm32"))]
fn outside(path: &std::path::Path, reveal: bool) -> Result<(), String> {
    use std::process::Command;
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer");
        // the file manager takes "/select," and the path as one word, the path in quotes
        let path = path.to_string_lossy().replace('/', "\\");
        if reveal {
            command.raw_arg(format!("/select,\"{path}\""));
        } else {
            command.raw_arg(format!("\"{path}\""));
        }
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        if reveal {
            command.arg("-R");
        }
        command.arg(path);
        command
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(if reveal { path.parent().unwrap_or(path) } else { path });
        command
    };
    command.spawn().map(|_| ()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(target_arch = "wasm32")]
fn outside(_path: &std::path::Path, _reveal: bool) -> Result<(), String> {
    Ok(())
}

/// The lasting key of an element with these properties: its GlobalId, else its UniqueId, else
/// its id in the file.
fn element_key(props: &serde_json::Value, id: u64) -> String {
    ["GlobalId", "UniqueId", "Id"].iter().find_map(|key| props.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())).map_or_else(|| format!("#{id}"), str::to_string)
}

/// Whether the suite reads a file of this name as a model, by its extension.
fn suite_reads(path: &str) -> bool {
    let extension = path.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_ascii_lowercase());
    crate::loaders::supported_extensions().contains(&extension.as_str())
}

fn size_text(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1024 => format!("{} kB", b.div_ceil(1024)),
        b => format!("{b} B"),
    }
}

/// When a file was changed, in the time of this computer.
#[cfg(not(target_arch = "wasm32"))]
fn when(seconds: u64) -> String {
    if seconds == 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(seconds as i64, 0).map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn when(_seconds: u64) -> String {
    String::new()
}

fn standing_text(status: Standing, l: Language) -> &'static str {
    match status {
        Standing::Work => l.text("In bewerking", "Work in progress"),
        Standing::Shared => l.text("Gedeeld", "Shared"),
        Standing::Published => l.text("Gepubliceerd", "Published"),
        Standing::Archived => l.text("Gearchiveerd", "Archived"),
    }
}

fn kind_text(kind: Kind, l: Language) -> &'static str {
    match kind {
        Kind::Model => "Model",
        Kind::Scan => l.text("Puntenwolk", "Point cloud"),
        Kind::Drawing => l.text("Tekening", "Drawing"),
        Kind::Pdf => "PDF",
        Kind::Image => l.text("Afbeelding", "Image"),
        Kind::Office => l.text("Document", "Document"),
        Kind::Text => l.text("Tekst", "Text"),
        Kind::Other => l.text("Bestand", "File"),
    }
}

/// What keeps the folder the same for everyone, in a few words.
fn sync_text(sync: Option<&Sync>, l: Language) -> String {
    match sync.map(|s| s.service) {
        Some(service) => match service.name() {
            Some(name) => format!("{} {name}", l.text("gesynchroniseerd met", "synced with")),
            None if service == Service::Network => l.text("netwerkmap", "network folder").to_string(),
            None => l.text("lokale map, niet gedeeld", "local folder, not shared").to_string(),
        },
        None => String::new(),
    }
}

/// The icon of a folder, of the way up, or of a file of this kind (none: a file that is gone).
fn icon(kind: Option<Kind>, folder: bool, up: bool) -> svg::Handle {
    const PAGE: &str = r##"<path d="M3.5 1.5h6l3 3v10h-9z" fill="#FFFFFF" stroke="#8C939C"/><path d="M9.5 1.5v3h3" fill="none" stroke="#8C939C"/>"##;
    let mark = match (folder, up, kind) {
        (_, true, _) => return svg::Handle::from_memory(UP.as_bytes()),
        (true, _, _) => return svg::Handle::from_memory(FOLDER.as_bytes()),
        (_, _, None) => r##"<path d="M5.5 8.5l5 5M10.5 8.5l-5 5" stroke="#B0B6BE" stroke-width="1.2"/>"##,
        (_, _, Some(Kind::Model)) => r##"<path d="M8 6.3l3.2 1.7v3.6L8 13.3l-3.2-1.7V8z" fill="#3B82C4"/><path d="M4.8 8L8 9.7l3.2-1.7M8 9.7v3.6" fill="none" stroke="#FFFFFF" stroke-width="0.7"/>"##,
        (_, _, Some(Kind::Scan)) => r##"<g fill="#0E9AA7"><circle cx="6" cy="8" r="0.9"/><circle cx="9" cy="7.5" r="0.9"/><circle cx="7.5" cy="10" r="0.9"/><circle cx="10.5" cy="10.5" r="0.9"/><circle cx="6" cy="12" r="0.9"/><circle cx="9" cy="12.5" r="0.9"/></g>"##,
        (_, _, Some(Kind::Drawing)) => r##"<path d="M5 7h6v6H5zM8 7v3.5h3" fill="none" stroke="#2E9E6A" stroke-width="1.2"/>"##,
        (_, _, Some(Kind::Pdf)) => r##"<rect x="3.5" y="9" width="9" height="4" fill="#D9483B"/><path d="M5.5 6.5h5" stroke="#D9483B" stroke-width="1.1"/>"##,
        (_, _, Some(Kind::Image)) => r##"<path d="M4.5 13.5L7 10l2 2 1.5-1.5 2 3z" fill="#8A63D2"/><circle cx="6.5" cy="7.3" r="1.1" fill="#8A63D2"/>"##,
        (_, _, Some(Kind::Office)) => r##"<path d="M5.5 7.5h5M5.5 9.5h5M5.5 11.5h3.5" stroke="#2B6CB0" stroke-width="1.1"/>"##,
        (_, _, Some(Kind::Text)) => r##"<path d="M5.5 7.5h5M5.5 9.5h5M5.5 11.5h3.5" stroke="#8C939C" stroke-width="1.1"/>"##,
        (_, _, Some(Kind::Other)) => "",
    };
    svg::Handle::from_memory(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">{PAGE}{mark}</svg>"#).into_bytes())
}

/// A folder, in the amber of the suite.
const FOLDER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M1.5 3.5a1 1 0 0 1 1-1h3.6l1.5 1.5h5.9a1 1 0 0 1 1 1v7.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="#D9912B"/><path d="M1.5 6.2h13v6.3a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="#F2B54F"/></svg>"##;
/// The way up: a folder with an arrow.
const UP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M1.5 3.5a1 1 0 0 1 1-1h3.6l1.5 1.5h5.9a1 1 0 0 1 1 1v7.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="#C9CDD2"/><path d="M8 12V7.2M5.8 9.4L8 7.2l2.2 2.2" fill="none" stroke="#5F6670" stroke-width="1.4"/></svg>"##;

/// An icon at the size of a line of the list.
fn icon_view<'a>(handle: svg::Handle) -> Element<'a, Message> {
    svg(handle).width(16).height(16).into()
}

/// When, in a few words: the day and hour of an ISO date (`2026-11-18T10:05:00Z` → `2026-11-18 10:05`).
fn day_and_hour(date: &str) -> String {
    let date = date.replace('T', " ");
    date.get(..16).unwrap_or(&date).to_string()
}

impl App {
    pub(super) fn docs_update(&mut self, message: DocsMessage) -> Task<Message> {
        match message {
            DocsMessage::Open => {
                self.docs.open = true;
                self.show_docs();
                // the others may have changed the folder since it was read
                return self.docs_read();
            }
            DocsMessage::Close => self.close_docs(),
            DocsMessage::Choose => return choose_folder(),
            DocsMessage::Chosen(None) => {}
            DocsMessage::Chosen(Some(folder)) => {
                if let Some(file) = folder_file() {
                    let _ = file.parent().map(std::fs::create_dir_all);
                    let _ = std::fs::write(file, folder.to_string_lossy().as_bytes());
                }
                self.docs = DocsTab { open: self.docs.open, shown: self.docs.shown, in_product: self.docs.in_product, folder: Some(folder), ..DocsTab::empty() };
                return self.docs_read();
            }
            DocsMessage::Leave => {
                if let Some(file) = folder_file() {
                    let _ = std::fs::remove_file(file);
                }
                self.docs = DocsTab { open: self.docs.open, shown: self.docs.shown, in_product: self.docs.in_product, ..DocsTab::empty() };
            }
            DocsMessage::Refresh => return self.docs_read(),
            DocsMessage::Read(read) => {
                self.docs_take(*read);
                return self.docs_versions();
            }
            DocsMessage::Folder(folder) => {
                // a click on the folder that shows closes it in the tree; another one opens it
                if self.docs.at == folder && self.docs.opened.contains(&folder) {
                    self.docs.opened.remove(&folder);
                } else {
                    // the folders above it open too, as when it is reached through the path
                    let mut above = folder.as_str();
                    while !above.is_empty() && above != MISSING {
                        self.docs.opened.insert(above.to_string());
                        above = above.rsplit_once('/').map_or("", |(parent, _)| parent);
                    }
                }
                self.docs.at = folder;
                self.docs.pick = None;
                self.docs.query.clear();
            }
            DocsMessage::Pick(path) => {
                // a second click soon after the first opens the file, as in a file manager
                let again = self.docs.clicked.take().is_some_and(|(before, at)| before == path && at.elapsed() < SECOND_CLICK);
                if again && self.docs.pick.as_deref() == Some(path.as_str()) && self.docs.listing.file(&path).is_some() {
                    let open = if suite_reads(&path) { DocsMessage::OpenModel { add: false } } else { DocsMessage::OpenOutside };
                    return self.docs_update(open);
                }
                self.docs.clicked = Some((path.clone(), Instant::now()));
                self.docs_pick(path);
                return self.docs_versions();
            }
            DocsMessage::Title(title) => self.docs.title = title,
            DocsMessage::SaveTitle => {
                let title = self.docs.title.trim().to_string();
                self.docs_change(|d| {
                    let changed = d.title != title;
                    d.title = title;
                    changed
                });
            }
            DocsMessage::Standing(status) => self.docs_change(|d| {
                let changed = d.status != status;
                d.status = status;
                changed
            }),
            DocsMessage::Code(code) => self.docs.code = code,
            DocsMessage::Note(note) => self.docs.note = note,
            DocsMessage::Record => {
                let code = self.docs.code.trim().to_string();
                if code.is_empty() {
                    self.docs.notice = Some(Notice::CodeFirst);
                    return Task::none();
                }
                // the file as the folder had it when it was read: its size and time tell a later change
                let Some(file) = self.docs.pick.as_deref().and_then(|path| self.docs.listing.file(path)).cloned() else { return Task::none() };
                let Some(folder) = self.docs.folder.clone() else { return Task::none() };
                let (path, note) = (file.path.clone(), self.docs.note.trim().to_string());
                self.docs.code.clear();
                self.docs.note.clear();
                // Git keeps the versions of a checkout; elsewhere a copy of the file is kept
                if self.docs.sync.as_ref().is_some_and(|sync| sync.service.git()) {
                    return self.docs_update(DocsMessage::Kept(Box::new(Kept { path, code, note, file, copy: Ok(String::new()) })));
                }
                let document = self.docs_document(&path);
                let large = file.size > AT_ONCE;
                let kept = Kept { path, code: code.clone(), note, file, copy: Ok(String::new()) };
                let job = move || bim_docs::register::keep(&folder, &document, &code);
                if !large {
                    return self.docs_update(DocsMessage::Kept(Box::new(Kept { copy: job(), ..kept })));
                }
                self.docs.busy = true;
                return work(job, move |copy| Message::Docs(DocsMessage::Kept(Box::new(Kept { copy, ..kept }))));
            }
            DocsMessage::Kept(kept) => {
                self.docs.busy = false;
                let Kept { path, code, note, file, copy } = *kept;
                let (by, now) = (who(), crate::export::ifcx::timestamp());
                self.docs_change_at(&path, |d| {
                    d.record(&code, &note, &file, &by, &now);
                    if let (Ok(copy), Some(last)) = (&copy, d.revisions.last_mut()) {
                        last.copy = copy.clone();
                    }
                    true
                });
                if let Err(why) = copy {
                    self.docs.notice = Some(Notice::NotKept(why));
                }
            }
            DocsMessage::OpenRecorded { code, date } => {
                let (Some(folder), Some(path)) = (self.docs.folder.clone(), self.docs.pick.clone()) else { return Task::none() };
                let copy = self.docs.register.of(&path).and_then(|d| d.revisions.iter().find(|r| r.code == code && r.date == date)).map(|r| r.copy.clone()).filter(|c| !c.is_empty());
                if let Some(copy) = copy {
                    return self.docs_update(DocsMessage::VersionReady(Ok(folder.join(copy))));
                }
            }
            DocsMessage::PutBack(version) => {
                // the first press asks; the second puts the version back
                if self.docs.confirm.as_ref() != Some(&version) {
                    self.docs.confirm = Some(version);
                    return Task::none();
                }
                self.docs.confirm = None;
                let (Some(folder), Some(path)) = (self.docs.folder.clone(), self.docs.pick.clone()) else { return Task::none() };
                let target = folder.join(&path);
                let large = self.docs.listing.file(&path).is_some_and(|f| f.size > AT_ONCE);
                let job: Box<dyn FnOnce() -> Result<PathBuf, String> + Send> = match version {
                    Version::Recorded { code, date } => {
                        let Some(document) = self.docs.register.of(&path).cloned() else { return Task::none() };
                        let Some(revision) = document.revisions.iter().find(|r| r.code == code && r.date == date).cloned() else { return Task::none() };
                        Box::new(move || {
                            let aside = keep_aside(&target)?;
                            bim_docs::register::put_back(&folder, &document, &revision)?;
                            Ok(aside)
                        })
                    }
                    Version::Git(hash) => {
                        let Some((root, inside)) = self.docs_in_git() else { return Task::none() };
                        Box::new(move || {
                            let bytes = bim_docs::git::content(&root, &inside, &hash)?;
                            let aside = keep_aside(&target)?;
                            write_over(&target, &bytes)?;
                            Ok(aside)
                        })
                    }
                };
                if !large {
                    return self.docs_update(DocsMessage::PutBackDone(job()));
                }
                self.docs.busy = true;
                return work(job, |done| Message::Docs(DocsMessage::PutBackDone(done)));
            }
            DocsMessage::PutBackDone(done) => {
                self.docs.busy = false;
                match done {
                    Ok(aside) => {
                        self.docs.notice = Some(Notice::PutBack(aside));
                        // its size and time changed: the folder and Git are asked again
                        return self.docs_read();
                    }
                    Err(error) => self.docs.notice = Some(Notice::Failed(error)),
                }
            }
            DocsMessage::CheckOut => {
                let by = who();
                let now = crate::export::ifcx::timestamp();
                self.docs_change(|d| {
                    if d.out.is_some() {
                        return false;
                    }
                    d.check_out(&by, &now);
                    true
                });
            }
            DocsMessage::CheckIn => {
                let by = who();
                let now = crate::export::ifcx::timestamp();
                self.docs_change(|d| {
                    if d.out.is_none() {
                        return false;
                    }
                    d.check_in(&by, &now);
                    true
                });
            }
            DocsMessage::Copy(value) => {
                self.docs.notice = Some(Notice::Copied);
                return iced::clipboard::write(value);
            }
            DocsMessage::Visit(address) => self.docs.notice = visit(&address).err().map(Notice::Failed),
            DocsMessage::Query(query) => {
                self.docs.query = query;
            }
            DocsMessage::Sort(column) => {
                let (by, turned) = self.docs.sort;
                self.docs.sort = (column, by == column && !turned);
            }
            DocsMessage::Up => {
                if !self.docs.query.is_empty() {
                    self.docs.query.clear();
                } else if !self.docs.at.is_empty() {
                    let up = if self.docs.at == MISSING { String::new() } else { self.docs.at.rsplit_once('/').map_or(String::new(), |(parent, _)| parent.to_string()) };
                    self.docs.at = up;
                    self.docs.pick = None;
                }
            }
            DocsMessage::Step(_) if !self.docs.shown => {}
            DocsMessage::Step(by) => {
                let rows: Vec<String> = self.docs_rows().into_iter().map(|(path, ..)| path.to_string()).collect();
                if rows.is_empty() {
                    return Task::none();
                }
                let at = self.docs.pick.as_ref().and_then(|pick| rows.iter().position(|row| row == pick));
                let next = match at {
                    Some(at) => (at as i64 + by as i64).clamp(0, rows.len() as i64 - 1) as usize,
                    None if by < 0 => rows.len() - 1,
                    None => 0,
                };
                self.docs.clicked = None;
                self.docs_pick(rows[next].clone());
                return self.docs_versions();
            }
            DocsMessage::Enter => {
                if let Some(path) = self.docs.pick.clone().filter(|path| self.docs.listing.file(path).is_some()) {
                    let open = if suite_reads(&path) { DocsMessage::OpenModel { add: false } } else { DocsMessage::OpenOutside };
                    return self.docs_update(open);
                }
            }
            DocsMessage::OpenModel { add } => {
                let Some(path) = self.docs_picked_path() else { return Task::none() };
                // the model shows, not the page
                self.docs.shown = false;
                return self.open(crate::loaders::Source::File(path), add);
            }
            DocsMessage::OpenOutside | DocsMessage::Reveal => {
                if let Some(path) = self.docs_picked_path() {
                    self.docs.notice = outside(&path, matches!(message, DocsMessage::Reveal)).err().map(Notice::Failed);
                }
            }
            DocsMessage::Link => {
                let links = self.docs_selected_links();
                self.docs_change(|d| links.into_iter().fold(false, |any, link| d.link(link) || any));
            }
            DocsMessage::Unlink(key) => self.docs_change(|d| {
                let before = d.elements.len();
                d.elements.retain(|e| e.key != key);
                d.elements.len() != before
            }),
            DocsMessage::Show(path) => {
                self.docs.open = true;
                self.show_docs();
                self.docs_go_to(&path);
                // the others may have changed the folder since it was read
                return self.docs_read();
            }
            DocsMessage::Known(known) => {
                let (folder, register) = *known;
                // still this project folder, and Docs has not read it itself meanwhile
                if self.docs.folder.as_ref() == Some(&folder) && !self.docs.known {
                    self.docs.register = register;
                    self.docs.known = true;
                }
            }
            DocsMessage::Versions(versions) => {
                // still the picked file
                if self.docs.pick.as_deref() == Some(versions.path.as_str()) {
                    self.docs.versions = Some(*versions);
                }
            }
            DocsMessage::OpenVersion(hash) => {
                if let Some((root, inside)) = self.docs_in_git() {
                    return version_task(root, inside, hash);
                }
            }
            DocsMessage::VersionReady(Err(error)) => self.docs.notice = Some(Notice::Failed(error)),
            DocsMessage::VersionReady(Ok(path)) => {
                if suite_reads(&path.to_string_lossy()) {
                    // beside the open model, so the two can be compared; alone when none is open
                    self.docs.shown = false;
                    let add = self.model.is_some();
                    return self.open(crate::loaders::Source::File(path), add);
                }
                self.docs.notice = outside(&path, false).err().map(Notice::Failed);
            }
            DocsMessage::Save => {
                if let Some((path, replace)) = self.docs_save_target() {
                    return self.start_save(path, replace);
                }
            }
            DocsMessage::Go(key) => {
                let Some(model) = self.model.clone() else { return Task::none() };
                // the key is somewhere in the element's properties: only those are taken apart
                let ids: Vec<u64> = model
                    .props
                    .iter()
                    .filter(|(id, props)| key == format!("#{id}") || props.contains(key.as_str()))
                    .filter(|(id, props)| element_key(&serde_json::from_str(props).unwrap_or_default(), **id) == key)
                    .map(|(id, _)| *id)
                    .collect();
                if !ids.is_empty() {
                    self.sel.clear();
                    self.sel.extend(ids.into_iter().map(Pickable::Element));
                    self.selection_changed();
                    self.docs.shown = false;
                }
            }
        }
        Task::none()
    }

    /// Opens Docs on the project folder at `folder`, read at once, with the file at `pick` (its
    /// path in the folder) picked: for the command line, where no message runs. The folder is
    /// not kept as the person's choice.
    pub fn docs_at(&mut self, folder: PathBuf, pick: Option<&str>) {
        self.docs = DocsTab { open: true, in_product: self.docs.in_product, folder: Some(folder.clone()), ..DocsTab::empty() };
        self.show_docs();
        self.docs_take(read_project(folder));
        if let Some(path) = pick.map(|p| p.replace('\\', "/")) {
            self.docs_go_to(&path);
            self.docs_versions_now();
        }
    }

    /// The checkout the project folder is in and the picked file's path in it, when Git keeps
    /// the folder.
    fn docs_in_git(&self) -> Option<(PathBuf, String)> {
        let docs = &self.docs;
        let sync = docs.sync.as_ref().filter(|sync| sync.service.git())?;
        let (folder, pick) = (docs.folder.as_ref()?, docs.pick.as_deref()?);
        docs.listing.file(pick)?;
        let above = folder.strip_prefix(&sync.root).ok()?.to_string_lossy().replace('\\', "/");
        Some((sync.root.clone(), if above.is_empty() { pick.to_string() } else { format!("{above}/{pick}") }))
    }

    /// Asks Git for the versions of the picked file; what was known of another file goes.
    fn docs_versions(&mut self) -> Task<Message> {
        if self.docs.versions.as_ref().map(|v| v.path.as_str()) != self.docs.pick.as_deref() {
            self.docs.versions = None;
        }
        match (self.docs_in_git(), self.docs.pick.clone()) {
            (Some((root, inside)), Some(path)) => versions_task(root, inside, path),
            _ => Task::none(),
        }
    }

    /// Asks Git at once, where no message runs (the command line, a test).
    pub(super) fn docs_versions_now(&mut self) {
        self.docs.versions = match (self.docs_in_git(), self.docs.pick.clone()) {
            (Some((root, inside)), Some(path)) => Some(Versions { result: read_versions(&root, &inside), path }),
            _ => None,
        };
    }

    /// Where saving puts the open model, and whether a file there is replaced. A model that was
    /// opened from the project folder, in a format the suite writes, is saved where it is. Any
    /// other is saved as a new IFCX file in the folder that shows, and never over a file.
    pub fn docs_save_target(&self) -> Option<(PathBuf, bool)> {
        let (model, folder) = (self.model.as_ref()?, self.docs.folder.as_ref()?);
        let name = model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if model.path.starts_with(folder) && model.path.is_file() && crate::export::writer_for(&name).is_ok() {
            return Some((model.path.clone(), true));
        }
        let stem = model.path.file_stem().map_or_else(|| "model".to_string(), |s| s.to_string_lossy().into_owned());
        let at = if self.docs.at == MISSING { "" } else { self.docs.at.as_str() };
        Some((folder.join(at).join(format!("{stem}.{}", crate::export::registry()[0].extensions()[0])), false))
    }

    /// A model was saved at `path`: when that is in the project folder, Docs goes to it and
    /// reads the folder again.
    pub(super) fn docs_saved(&mut self, path: &std::path::Path) -> Task<Message> {
        let Some(inside) = self.docs.folder.as_ref().and_then(|folder| path.strip_prefix(folder).ok()) else { return Task::none() };
        let inside = inside.to_string_lossy().replace('\\', "/");
        self.docs.clicked = None;
        self.docs_go_to(&inside);
        self.docs_read()
    }

    /// Whether the register of the project folder is still to be read: a folder is chosen and
    /// nothing was read of it yet.
    pub(super) fn docs_wants_register(&self) -> bool {
        self.docs.folder.is_some() && !self.docs.known && !self.docs.reading
    }

    /// Reads the register of the project folder when a model opens and Docs was not opened yet,
    /// so the model's elements can say which documents are about them. Only the register: the
    /// folder itself is read when Docs opens.
    pub(super) fn docs_register(&self) -> Task<Message> {
        match self.docs.folder.clone().filter(|_| self.docs_wants_register()) {
            Some(folder) => register_task(folder),
            None => Task::none(),
        }
    }

    /// Goes to the document of the file at `path`: its folder shows, the folders above it are
    /// open in the tree, and it is picked.
    fn docs_go_to(&mut self, path: &str) {
        let mut folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
        self.docs.at = folder.to_string();
        while !folder.is_empty() {
            self.docs.opened.insert(folder.to_string());
            folder = folder.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
        self.docs.clicked = None;
        let _ = self.docs_update(DocsMessage::Pick(path.to_string()));
        self.docs.clicked = None;
    }

    /// The documents of the project folder that are about element `id` of the model, for the
    /// right pane; none when no document is linked to it. A document goes to Docs.
    pub(super) fn docs_of_element(&self, p: Palette, l: Language, id: u64) -> Option<Element<'_, Message>> {
        if self.docs.register.documents.is_empty() {
            return None;
        }
        let props: serde_json::Value = self.model.as_ref()?.props.get(&id).and_then(|props| serde_json::from_str(props).ok()).unwrap_or_default();
        let key = element_key(&props, id);
        let mut rows = Column::new().spacing(2);
        let mut any = false;
        for document in self.docs.register.about(&key) {
            any = true;
            let mut said = vec![standing_text(document.status, l).to_string()];
            if !document.revision.is_empty() {
                said.push(document.revision.clone());
            }
            rows = rows.push(
                button(column![
                    text(document.name().to_string()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph),
                    row![text(said.remove(0)).size(11).color(p.text_muted)].push(said.pop().map(|revision| text(revision).size(11).color(p.text_muted))).spacing(8),
                ])
                .padding([2, 6])
                .width(Fill)
                .style(move |_, s| p.row_button(s))
                .on_press(Message::Docs(DocsMessage::Show(document.file.clone()))),
            );
        }
        any.then(|| rows.into())
    }

    /// Shows the Docs tab again, as it was.
    pub(super) fn show_docs(&mut self) {
        self.active_schedule = None;
        self.active_sheet = None;
        self.active_draft = None;
        self.active_doc_sheet = None;
        self.ifc.shown = false;
        self.load_view(None);
        self.docs.shown = true;
    }

    /// Closes the Docs tab; the project folder stays chosen.
    pub(super) fn close_docs(&mut self) {
        self.docs.open = false;
        self.docs.shown = false;
    }

    /// Starts reading the project folder, unless it is being read.
    fn docs_read(&mut self) -> Task<Message> {
        let Some(folder) = self.docs.folder.clone().filter(|_| !self.docs.reading) else { return Task::none() };
        self.docs.reading = true;
        read_task(folder)
    }

    /// Takes what was read of the project folder, if that is still the folder.
    /// Picks the file at `path`: what is typed about the one picked before goes.
    fn docs_pick(&mut self, path: String) {
        self.docs.title = self.docs.register.of(&path).map(|d| d.title.clone()).unwrap_or_default();
        self.docs.code.clear();
        self.docs.note.clear();
        self.docs.notice = None;
        self.docs.confirm = None;
        self.docs.pick = Some(path);
    }

    /// The files of the list as they show: those of the folder (or the documents whose file is
    /// gone, or what the search found), in the order of the column sorted by.
    fn docs_rows(&self) -> Vec<(&str, Option<&File>, Option<&Document>)> {
        let docs = &self.docs;
        let query = docs.query.trim().to_lowercase();
        let mut rows: Vec<(&str, Option<&File>, Option<&Document>)> = if !query.is_empty() {
            // the whole project folder: by the file's name and the document's title
            docs.listing
                .files
                .iter()
                .map(|f| (f.path.as_str(), Some(f), docs.register.of(&f.path)))
                .filter(|(_, f, d)| f.is_some_and(|f| f.name().to_lowercase().contains(&query)) || d.is_some_and(|d| d.title.to_lowercase().contains(&query)))
                .collect()
        } else if docs.at == MISSING {
            self.docs_missing().map(|d| (d.file.as_str(), None, Some(d))).collect()
        } else {
            docs.listing.files_in(&docs.at).map(|f| (f.path.as_str(), Some(f), docs.register.of(&f.path))).collect()
        };
        let name = |path: &str| path.rsplit('/').next().unwrap_or(path).to_lowercase();
        let (column, turned) = docs.sort;
        rows.sort_by(|a, b| {
            let order = match column {
                By::Name => std::cmp::Ordering::Equal,
                By::Kind => kind_text(Kind::of(a.0), Language::English).cmp(kind_text(Kind::of(b.0), Language::English)),
                By::Status => a.2.map(|d| d.status as u8).cmp(&b.2.map(|d| d.status as u8)),
                By::Revision => a.2.map(|d| d.revision.as_str()).cmp(&b.2.map(|d| d.revision.as_str())),
                By::Changed => a.1.map(|f| f.modified).cmp(&b.1.map(|f| f.modified)),
                By::Size => a.1.map(|f| f.size).cmp(&b.1.map(|f| f.size)),
            };
            let order = order.then_with(|| name(a.0).cmp(&name(b.0))).then_with(|| a.0.cmp(b.0));
            if turned {
                order.reverse()
            } else {
                order
            }
        });
        rows
    }

    fn docs_take(&mut self, read: Read) {
        if self.docs.folder.as_ref() != Some(&read.folder) {
            return;
        }
        let docs = &mut self.docs;
        docs.reading = false;
        match read.result {
            Ok((listing, register, sync)) => {
                docs.counts.clear();
                for file in &listing.files {
                    let mut folder = file.folder();
                    while !folder.is_empty() {
                        *docs.counts.entry(folder.to_string()).or_default() += 1;
                        folder = folder.rsplit_once('/').map_or("", |(parent, _)| parent);
                    }
                }
                // the folder that showed, or the picked file, may be gone
                if !docs.at.is_empty() && docs.at != MISSING && !listing.folders.contains(&docs.at) {
                    docs.at.clear();
                }
                if docs.pick.as_deref().is_some_and(|path| listing.file(path).is_none() && register.of(path).is_none()) {
                    docs.pick = None;
                }
                docs.listing = Arc::new(listing);
                docs.register = register;
                docs.known = true;
                docs.sync = Some(sync);
                docs.branch = read.branch;
                // what was put back or kept aside stays said until something else is done
                if !matches!(docs.notice, Some(Notice::PutBack(_) | Notice::NotKept(_))) {
                    docs.notice = None;
                }
            }
            Err(error) => {
                docs.listing = Arc::default();
                docs.register = Register::default();
                docs.counts.clear();
                docs.sync = None;
                docs.pick = None;
                docs.notice = Some(Notice::Failed(error));
            }
        }
    }

    /// The picked file, as a path on this computer.
    fn docs_picked_path(&self) -> Option<PathBuf> {
        Some(self.docs.folder.as_ref()?.join(self.docs.pick.as_deref()?))
    }

    /// Changes what is said about the picked document, and writes it when `change` says it did.
    /// A document nothing was said about gets its entry here; its id follows its path, so two
    /// people who start on the same file make the same entry.
    fn docs_change(&mut self, change: impl FnOnce(&mut Document) -> bool) {
        let Some(path) = self.docs.pick.clone() else { return };
        self.docs_change_at(&path, change);
    }

    /// What is said about the file at `path`: its entry, or a new one. Its id follows its path,
    /// so two people who start on the same file make the same entry.
    fn docs_document(&self, path: &str) -> Document {
        self.docs.register.of(path).cloned().unwrap_or_else(|| Document::new(&crate::document::uuid(&format!("openaec-document/{}", path.to_lowercase())), path))
    }

    /// Changes what is said about the document of the file at `path`, as [`Self::docs_change`].
    fn docs_change_at(&mut self, path: &str, change: impl FnOnce(&mut Document) -> bool) {
        let Some(folder) = self.docs.folder.clone() else { return };
        let mut document = self.docs_document(path);
        if !change(&mut document) {
            return;
        }
        let now = crate::export::ifcx::timestamp();
        document.touch(&who(), &now);
        let author = format!("Open BIM Suite {}", env!("CARGO_PKG_VERSION"));
        self.docs.notice = self.docs.register.save(&folder, document, &author, &now).err().map(Notice::Failed);
    }

    /// The selected elements of the model as links: each with its lasting key, its IFCX node path
    /// when the model is IFCX, its name and the model's file.
    fn docs_selected_links(&self) -> Vec<ElementLink> {
        let Some(model) = &self.model else { return Vec::new() };
        let file = model.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let ifcx = model.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifcx"));
        let elements = self.sel.items().iter().filter_map(|item| match item {
            Pickable::Element(id) => Some(*id),
            Pickable::Layer(_) => None,
        });
        elements
            .map(|id| {
                let props: serde_json::Value = model.props.get(&id).and_then(|p| serde_json::from_str(p).ok()).unwrap_or_default();
                let field = |key: &str| props.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
                ElementLink {
                    key: element_key(&props, id),
                    // in an IFCX model the element's Id is the path of its node
                    path: field("Id").filter(|_| ifcx).unwrap_or_default(),
                    name: field("Name").unwrap_or_else(|| format!("#{id}")),
                    model: file.clone(),
                }
            })
            .collect()
    }

    /// The documents of the register whose file is not in the folder (any more).
    fn docs_missing(&self) -> impl Iterator<Item = &Document> {
        self.docs.register.documents.iter().filter(|d| self.docs.listing.file(&d.file).is_none())
    }

    /// The Docs page: a path to click with what keeps the folder, then the files of the folder
    /// that shows (or what the search found). Nothing stands over the list but that line.
    pub(super) fn docs_page(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let Some(folder) = &docs.folder else {
            // no project folder yet: what Docs is, and the way to choose one
            let ask = column![
                text("Docs").size(20).font(FONT_BOLD).color(p.text),
                text(l.text(
                    "De documenten van het project: modellen, tekeningen, PDF's en rapporten, met hun status en versies. Kies de projectmap die Dropbox, SharePoint, Synology Drive, GitHub of een netwerkschijf voor iedereen gelijk houdt, of een map op deze computer.",
                    "The documents of the project: models, drawings, PDFs and reports, with their status and revisions. Choose the project folder that Dropbox, SharePoint, Synology Drive, GitHub or a network drive keeps the same for everyone, or a folder on this computer.",
                ))
                .size(12)
                .color(p.text_secondary)
                .width(460)
                .wrapping(Wrapping::Word)
                .center(),
                button(text(l.text("Projectmap kiezen…", "Choose project folder…")).size(13).color(p.accent_text)).padding([6, 14]).style(move |_, s| p.primary_button(s)).on_press(Message::Docs(DocsMessage::Choose)),
            ]
            .spacing(12)
            .align_x(Center);
            return container(ask).center(Fill).style(move |_| p.panel()).into();
        };
        let name = folder.file_name().map_or_else(|| folder.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        // the path, a name to click per folder, as in the address line of a file manager
        let crumb = |label: String, to: String, last: bool| -> Element<'_, Message> {
            let label = text(label).size(13).color(if last { p.text } else { p.text_secondary });
            let label = if last { label.font(FONT_BOLD) } else { label };
            button(label).padding([2, 6]).style(move |_, s| p.row_button(s)).on_press(Message::Docs(DocsMessage::Folder(to))).into()
        };
        let searching = !docs.query.trim().is_empty();
        let mut path = row![icon_view(icon(None, true, false))].spacing(2).align_y(Center);
        path = path.push(crumb(name, String::new(), docs.at.is_empty() && !searching));
        if docs.at == MISSING {
            path = path.push(text("›").size(13).color(p.text_muted)).push(crumb(l.text("Vermiste bestanden", "Missing files").to_string(), MISSING.to_string(), true));
        } else if !docs.at.is_empty() {
            let parts: Vec<&str> = docs.at.split('/').collect();
            for (i, part) in parts.iter().enumerate() {
                path = path.push(text("›").size(13).color(p.text_muted)).push(crumb(part.to_string(), parts[..=i].join("/"), i + 1 == parts.len() && !searching));
            }
        }
        if searching {
            path = path.push(text("›").size(13).color(p.text_muted)).push(text(format!("{} “{}”", l.text("Zoeken naar", "Search for"), docs.query.trim())).size(13).font(FONT_BOLD).color(p.text));
        }
        let mut about = vec![sync_text(docs.sync.as_ref(), l)];
        if searching {
            about.push(format!("{} {}", self.docs_rows().len(), l.text("gevonden", "found")));
        } else {
            about.push(format!("{}{} {}", docs.listing.files.len(), if docs.listing.more { "+" } else { "" }, l.text("bestanden", "files")));
        }
        if docs.reading {
            about.push(l.text("wordt gelezen…", "reading…").to_string());
        }
        about.retain(|part| !part.is_empty());
        let header = row![path.width(Fill), text(about.join(" · ")).size(11).color(p.text_muted)].spacing(10).align_y(Center);
        let files = scrollable(self.docs_files(p, l)).width(Fill).height(Fill);
        // in the Docs product the folders are the left pane; elsewhere they stand beside the files
        let body: Element<'_, Message> = if docs.in_product {
            files.into()
        } else {
            row![container(self.docs_tree(p, l)).width(230).height(Fill), rule::vertical(1), files].height(Fill).into()
        };
        container(column![container(header).padding([6, 8]), rule::horizontal(1), body])
            .width(Fill)
            .height(Fill)
            .style(move |_| p.panel())
            .into()
    }

    /// The folders of the project folder as the left pane of the Docs product.
    pub fn docs_tree_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        if self.docs.folder.is_none() {
            return container(text(l.text("Nog geen projectmap gekozen.", "No project folder chosen yet.")).size(12).color(p.text_muted)).padding(8).into();
        }
        self.docs_tree(p, l)
    }

    /// The picked file, when it is in the project folder.
    pub fn docs_picked(&self) -> Option<&File> {
        self.docs.pick.as_deref().and_then(|path| self.docs.listing.file(path))
    }

    /// Whether the suite reads the picked file as a model.
    pub fn docs_reads_picked(&self) -> bool {
        self.docs_picked().is_some_and(|file| suite_reads(&file.path))
    }

    /// Whether the picked document can be linked now: elements of the model are selected.
    pub fn docs_can_link(&self) -> bool {
        self.docs.pick.is_some() && self.sel.items().iter().any(|item| matches!(item, Pickable::Element(_)))
    }

    /// The search through the project folder, then its folders as a tree; the one whose files
    /// show is marked.
    fn docs_tree(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let line = |path: &str, label: String, depth: usize, count: usize, mark: &'static str| -> Element<'_, Message> {
            let shown = docs.at == path && docs.query.trim().is_empty();
            button(
                row![
                    text(mark).size(11).color(p.text_secondary).width(10),
                    icon_view(icon(None, path != MISSING, false)),
                    text(label).size(12).color(p.text).width(Fill).wrapping(Wrapping::None),
                    text(count.to_string()).size(11).color(p.text_muted),
                ]
                .spacing(5)
                .align_y(Center),
            )
            .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 4.0 + 14.0 * depth as f32 })
            .width(Fill)
            .style(move |_, s| if shown { p.ribbon_button(true, s) } else { p.row_button(s) })
            .on_press(Message::Docs(DocsMessage::Folder(path.to_string())))
            .into()
        };
        let mut body = Column::new().spacing(1).padding(4);
        let project = docs.folder.as_ref().and_then(|f| f.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        body = body.push(line("", project, 0, docs.listing.files.len(), "▾"));
        // depth first, the folders of an open folder under it
        let mut todo: Vec<(&str, usize)> = docs.listing.folders_in("").map(|f| (f, 1)).collect();
        todo.reverse();
        while let Some((folder, depth)) = todo.pop() {
            let open = docs.opened.contains(folder);
            let mut inside: Vec<(&str, usize)> = docs.listing.folders_in(folder).map(|f| (f, depth + 1)).collect();
            let mark = match (inside.is_empty(), open) {
                (true, _) => "",
                (false, true) => "▾",
                (false, false) => "▸",
            };
            let label = folder.rsplit('/').next().unwrap_or(folder).to_string();
            body = body.push(line(folder, label, depth, docs.counts.get(folder).copied().unwrap_or(0), mark));
            if open {
                inside.reverse();
                todo.extend(inside);
            }
        }
        let missing = self.docs_missing().count();
        if missing > 0 {
            body = body.push(line(MISSING, l.text("Vermiste bestanden", "Missing files").to_string(), 0, missing, ""));
        }
        let search = text_input(l.text("Zoeken in projectmap", "Search project folder"), &docs.query)
            .on_input(|query| Message::Docs(DocsMessage::Query(query)))
            .size(12)
            .padding([4, 8]);
        column![container(search).padding(Padding { top: 6.0, right: 6.0, bottom: 4.0, left: 6.0 }), scrollable(body).height(Fill)].height(Fill).into()
    }

    /// The files of the folder that shows: icon, name, kind, status, revision, changed and size,
    /// the folders of this folder above them. Under a name stand the document's title and what the
    /// sync tool, a later change or a check-out says of the file; in a search, its folder.
    fn docs_files(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let cell = |value: String, width: f32, muted: bool| text(value).size(12).color(if muted { p.text_muted } else { p.text }).width(width).wrapping(Wrapping::None);
        let (kind_w, status_w, revision_w, changed_w, size_w) = (88.0, 104.0, 56.0, 118.0, 72.0);
        let searching = !docs.query.trim().is_empty();
        // a heading sorts by its column; again turns the order round
        let heading = |label: &'static str, column: By, width: Option<f32>| -> Element<'_, Message> {
            let (by, turned) = docs.sort;
            let arrow = if by == column { if turned { " ▾" } else { " ▴" } } else { "" };
            let label = button(text(format!("{label}{arrow}")).size(11).font(FONT_BOLD).color(p.group_label).wrapping(Wrapping::None)).padding([2, 0]).style(move |_, s| p.row_button(s)).on_press(Message::Docs(DocsMessage::Sort(column)));
            match width {
                Some(width) => label.width(width).into(),
                None => label.width(Fill).into(),
            }
        };
        let head = row![
            container(text("")).width(16),
            heading(l.text("Naam", "Name"), By::Name, None),
            heading(l.text("Soort", "Type"), By::Kind, Some(kind_w)),
            heading(l.text("Status", "Status"), By::Status, Some(status_w)),
            heading(l.text("Versie", "Revision"), By::Revision, Some(revision_w)),
            heading(l.text("Gewijzigd", "Changed"), By::Changed, Some(changed_w)),
            heading(l.text("Grootte", "Size"), By::Size, Some(size_w)),
        ]
        .spacing(8)
        .align_y(Center)
        .padding([2, 12]);
        let mut body = Column::new().spacing(1).push(head).push(rule::horizontal(1));
        let rows = self.docs_rows();
        // as in a file manager: the way up, then the folders of this folder, then its files
        let folder_row = |path: &str, label: String, count: Option<usize>, up: bool| -> Element<'_, Message> {
            let line = row![
                icon_view(icon(None, true, up)),
                text(label).size(12).color(p.text).width(Fill).wrapping(Wrapping::None),
                cell(if up { String::new() } else { l.text("Map", "Folder").to_string() }, kind_w, true),
                container(text("")).width(status_w + revision_w + changed_w + 16.0),
                cell(count.map_or_else(String::new, |n| format!("{n} {}", l.text("bestanden", "files"))), size_w, true),
            ]
            .spacing(8)
            .align_y(Center);
            button(line).padding([3, 12]).width(Fill).style(move |_, s| p.row_button(s)).on_press(Message::Docs(DocsMessage::Folder(path.to_string()))).into()
        };
        let mut folders = 0;
        if docs.at != MISSING && !searching {
            if !docs.at.is_empty() {
                body = body.push(folder_row(docs.at.rsplit_once('/').map_or("", |(parent, _)| parent), "..".to_string(), None, true));
                folders += 1;
            }
            for folder in docs.listing.folders_in(&docs.at) {
                let name = folder.rsplit('/').next().unwrap_or(folder);
                body = body.push(folder_row(folder, name.to_string(), Some(docs.counts.get(folder).copied().unwrap_or(0)), false));
                folders += 1;
            }
        }
        if rows.is_empty() && folders > 0 {
            return body.into();
        }
        if rows.is_empty() {
            let empty = if docs.reading {
                l.text("De map wordt gelezen…", "Reading the folder…")
            } else if searching {
                l.text("Niets gevonden in de projectmap", "Nothing found in the project folder")
            } else {
                l.text("Geen bestanden in deze map", "No files in this folder")
            };
            return body.push(container(text(empty).size(12).color(p.text_muted)).padding(12)).into();
        }
        let more = rows.len().saturating_sub(ROWS);
        for (path, file, document) in rows.into_iter().take(ROWS) {
            let picked = docs.pick.as_deref() == Some(path);
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            let mut marks: Vec<String> = Vec::new();
            match (file, document.map(|d| d.state(file))) {
                (None, _) => marks.push(l.text("bestand is weg", "file is gone").to_string()),
                (Some(_), Some(State::Changed)) => marks.push(format!("{} {}", l.text("gewijzigd sinds", "changed since"), document.map_or("", |d| d.revision.as_str()))),
                _ => {}
            }
            if let Some(out) = document.and_then(|d| d.out.as_ref()) {
                marks.push(format!("{} {}", l.text("uitgecheckt door", "checked out by"), out.by));
            }
            if file.is_some_and(|f| f.conflict) {
                marks.push(l.text("conflictkopie", "conflict copy").to_string());
            }
            if file.is_some_and(|f| f.online_only) {
                marks.push(l.text("alleen online", "only online").to_string());
            }
            // in a search the folder it is in says where it was found
            let mut under: Vec<String> = Vec::new();
            if searching {
                under.push(path.rsplit_once('/').map_or_else(|| "/".to_string(), |(folder, _)| folder.to_string()));
            }
            under.extend(document.map(|d| d.title.clone()).filter(|t| !t.is_empty()));
            // the name has the width; its title and marks stand under it
            let mut label = Column::new().push(text(name).size(12).color(p.text).wrapping(Wrapping::None));
            // the marks on a line of their own: they say what to look at, and are not cut off
            if !under.is_empty() {
                label = label.push(text(under.join(" · ")).size(11).color(p.text_muted).wrapping(Wrapping::None));
            }
            if !marks.is_empty() {
                label = label.push(text(marks.join(" · ")).size(11).color(p.accent).wrapping(Wrapping::None));
            }
            let kind = file.map(|f| f.kind());
            let line = row![
                icon_view(icon(kind, false, false)),
                container(label).width(Fill).clip(true),
                cell(kind_text(Kind::of(path), l).to_string(), kind_w, true),
                cell(document.map_or_else(String::new, |d| standing_text(d.status, l).to_string()), status_w, false),
                cell(document.map_or_else(String::new, |d| d.revision.clone()), revision_w, false),
                cell(file.map_or_else(String::new, |f| when(f.modified)), changed_w, true),
                cell(file.map_or_else(String::new, |f| size_text(f.size)), size_w, true),
            ]
            .spacing(8)
            .align_y(Center);
            body = body.push(
                button(line)
                    .padding([3, 12])
                    .width(Fill)
                    .style(move |_, s| if picked { p.ribbon_button(true, s) } else { p.row_button(s) })
                    .on_press(Message::Docs(DocsMessage::Pick(path.to_string()))),
            );
        }
        if more > 0 {
            body = body.push(container(text(format!("… {more} {}", l.text("meer", "more"))).size(12).color(p.text_muted)).padding([4, 12]));
        }
        body.into()
    }

    /// The right pane while Docs shows: the project folder, then the picked document as a
    /// document management system has it: what it is and the ways to open it, who has it checked
    /// out, its title and status, its versions (to record, open and put back), the ways to share
    /// it, and the elements it is about.
    pub(super) fn docs_properties(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let docs = &self.docs;
        if !docs.shown {
            return None;
        }
        let section = |title: &str| -> Element<'static, Message> { container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label)).padding(Padding { top: 8.0, right: 0.0, bottom: 2.0, left: 0.0 }).into() };
        let note = |value: String| text(value).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let action = |label: &str, message: DocsMessage| button(text(label.to_string()).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::Docs(message));
        let field = |hint: &'static str, value: &str, typed: fn(String) -> DocsMessage, entered: DocsMessage| {
            text_input(hint, value).on_input(move |t| Message::Docs(typed(t))).on_submit(Message::Docs(entered)).size(12).padding([3, 8])
        };
        let mut content = Column::new().spacing(5).push(section(l.text("Projectmap", "Project folder")));
        match &docs.folder {
            None => content = content.push(note(l.text("Nog geen projectmap gekozen.", "No project folder chosen yet.").to_string())),
            Some(folder) => {
                content = content.push(text(folder.to_string_lossy().into_owned()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph));
                if let Some(sync) = &docs.sync {
                    content = content.push(note(sync_text(Some(sync), l)));
                    if let Some(site) = &sync.site {
                        content = content.push(note(site.clone()));
                    }
                }
                content = content.push(
                    row![
                        action(l.text("Vernieuwen", "Refresh"), DocsMessage::Refresh),
                        action(l.text("Andere map…", "Other folder…"), DocsMessage::Choose),
                        action(l.text("Loslaten", "Let go"), DocsMessage::Leave),
                    ]
                    .spacing(4)
                    .wrap(),
                );
                if self.model.is_some() {
                    content = content.push(action(l.text("Model opslaan in projectmap", "Save model in project folder"), DocsMessage::Save));
                }
            }
        }
        match &docs.notice {
            Some(Notice::CodeFirst) => content = content.push(text(l.text("Geef de versie eerst een code", "Give the revision a code first")).size(12).color(p.accent)),
            Some(Notice::NotKept(why)) => {
                content = content.push(text(format!("{}: {why}", l.text("Versie vastgelegd, maar zonder kopie", "Revision recorded, but without a copy"))).size(12).color(p.accent).wrapping(Wrapping::WordOrGlyph));
            }
            Some(Notice::PutBack(aside)) => {
                content = content.push(text(l.text("De eerdere versie staat terug.", "The earlier version is back.")).size(12).color(p.accent)).push(note(format!(
                    "{} {}",
                    l.text("Het bestand zoals het was:", "The file as it was:"),
                    aside.display()
                )));
            }
            Some(Notice::Copied) => content = content.push(text(l.text("Staat op het klembord", "On the clipboard")).size(12).color(p.accent)),
            Some(Notice::Failed(error)) => content = content.push(text(error.clone()).size(12).color(p.accent).wrapping(Wrapping::WordOrGlyph)),
            None => {}
        }
        let Some(path) = docs.pick.as_deref() else { return Some(container(content).padding([2, 8]).into()) };
        let (file, document) = (docs.listing.file(path), docs.register.of(path));
        let kind = Kind::of(path);
        let me = who();

        // the document: what it is, and the ways to open it
        content = content.push(section(l.text("Document", "Document"))).push(
            row![icon_view(icon(file.map(|f| f.kind()), false, false)), text(path.rsplit('/').next().unwrap_or(path).to_string()).size(12).font(FONT_BOLD).color(p.text).wrapping(Wrapping::WordOrGlyph)]
                .spacing(6)
                .align_y(Center),
        );
        let mut facts = vec![kind_text(kind, l).to_string()];
        if let Some(file) = file {
            facts.push(size_text(file.size));
            facts.push(when(file.modified));
        }
        facts.retain(|fact| !fact.is_empty());
        content = content.push(note(facts.join(" · ")));
        if file.is_some_and(|f| f.online_only) {
            content = content.push(note(l.text("Staat alleen online: openen haalt het bestand eerst op.", "Only online: opening it fetches the file first.").to_string()));
        }
        if file.is_some_and(|f| f.conflict) {
            content = content.push(note(
                l.text(
                    "Een conflictkopie: twee mensen wijzigden dit bestand tegelijk. Vergelijk hem met het origineel en houd er één.",
                    "A conflict copy: two people changed this file at the same time. Compare it with the original and keep one.",
                )
                .to_string(),
            ));
        }
        if file.is_some() {
            let mut open = row![].spacing(4);
            if suite_reads(path) {
                open = open.push(action(l.text("Openen in de suite", "Open in the suite"), DocsMessage::OpenModel { add: false }));
                if self.model.is_some() {
                    open = open.push(action(l.text("Toevoegen aan model", "Add to the model"), DocsMessage::OpenModel { add: true }));
                }
            }
            open = open.push(action(l.text("Openen in programma", "Open in its program"), DocsMessage::OpenOutside)).push(action(l.text("Tonen in map", "Show in folder"), DocsMessage::Reveal));
            content = content.push(open.wrap());
        } else {
            content = content.push(note(l.text("Het bestand staat niet meer in de projectmap.", "The file is no longer in the project folder.").to_string()));
        }

        // who works on it: checked out, so the others know
        match document.and_then(|d| d.out.as_ref()) {
            Some(out) => {
                let by = if out.by == me { l.text("jou", "you").to_string() } else { out.by.clone() };
                content = content
                    .push(text(format!("{} {by} · {}", l.text("Uitgecheckt door", "Checked out by"), day_and_hour(&out.since))).size(12).color(p.accent).wrapping(Wrapping::WordOrGlyph))
                    .push(action(l.text("Inchecken", "Check in"), DocsMessage::CheckIn));
            }
            None if file.is_some() => {
                content = content.push(
                    row![action(l.text("Uitchecken", "Check out"), DocsMessage::CheckOut), note(l.text("De anderen zien dat jij eraan werkt.", "The others see you are working on it.").to_string())]
                        .spacing(6)
                        .align_y(Center),
                );
            }
            None => {}
        }

        // its title
        content = content.push(section(l.text("Titel", "Title"))).push(field(l.text("Titel van het document", "Title of the document"), &docs.title, DocsMessage::Title, DocsMessage::SaveTitle));
        if docs.title.trim() != document.map_or("", |d| d.title.as_str()) {
            content = content.push(action(l.text("Titel bewaren", "Keep title"), DocsMessage::SaveTitle));
        }

        // where it stands
        let mut standing = row![].spacing(4);
        for status in Standing::ALL {
            let active = document.is_some_and(|d| d.status == status);
            standing = standing.push(
                button(text(standing_text(status, l)).size(12).color(if active { p.accent } else { p.text_secondary }))
                    .padding([3, 8])
                    .style(move |_, s| p.ribbon_button(active, s))
                    .on_press(Message::Docs(DocsMessage::Standing(status))),
            );
        }
        content = content.push(section(l.text("Status", "Status"))).push(standing.wrap());

        // its versions: how the file stands, recording it as it is now, and those before, to open
        // beside the present one or to put back in its place
        content = content.push(section(l.text("Versies", "Versions")));
        let in_git = docs.sync.as_ref().is_some_and(|s| s.service.git());
        let state = match (file, document) {
            (None, _) => String::new(),
            (Some(_), None) => l.text("Nog geen versie vastgelegd", "No revision recorded yet").to_string(),
            (Some(_), Some(d)) => match d.state(file) {
                State::Recorded => format!("{} {}", l.text("Vastgelegd als", "Recorded as"), d.revision),
                State::Changed => format!("{} {}", l.text("Gewijzigd sinds", "Changed since"), d.revision),
                State::Unrecorded | State::Missing => l.text("Nog geen versie vastgelegd", "No revision recorded yet").to_string(),
            },
        };
        if !state.is_empty() {
            content = content.push(note(state));
        }
        if file.is_some() {
            let record = if docs.busy { l.text("Bezig…", "Working…") } else { l.text("Versie vastleggen", "Record revision") };
            content = content
                .push(field(l.text("Code, bijvoorbeeld P01", "Code, for example P01"), &docs.code, DocsMessage::Code, DocsMessage::Record))
                .push(field(l.text("Opmerking bij deze versie", "Note on this revision"), &docs.note, DocsMessage::Note, DocsMessage::Record))
                .push(action(record, DocsMessage::Record));
            if !in_git {
                content = content.push(note(l.text("Vastleggen bewaart een kopie in de projectmap, om later te openen of terug te zetten.", "Recording keeps a copy in the project folder, to open or put back later.").to_string()));
            }
        }
        // the button that puts a version back asks once more before it does
        let put_back = |version: Version| {
            let sure = docs.confirm.as_ref() == Some(&version);
            let label = if sure { l.text("Zeker terugzetten?", "Put back for sure?") } else { l.text("Terugzetten", "Put back") };
            button(text(label).size(12).color(if sure { p.accent } else { p.text })).padding([3, 8]).style(move |_, s| p.ribbon_button(sure, s)).on_press(Message::Docs(DocsMessage::PutBack(version)))
        };
        for revision in document.into_iter().flat_map(|d| d.revisions.iter().rev()) {
            let current = document.is_some_and(|d| d.revision == revision.code) && document.and_then(|d| d.revisions.last()).is_some_and(|last| last == revision);
            let mut line = row![text(format!("{} · {} · {}", revision.code, day_and_hour(&revision.date), revision.by)).size(12).font(if current { FONT_BOLD } else { iced::Font::DEFAULT }).color(p.text).width(Fill).wrapping(Wrapping::WordOrGlyph)]
                .spacing(4)
                .align_y(Center);
            if !revision.copy.is_empty() {
                line = line.push(action(l.text("Openen", "Open"), DocsMessage::OpenRecorded { code: revision.code.clone(), date: revision.date.clone() }));
                if file.is_some() {
                    line = line.push(put_back(Version::Recorded { code: revision.code.clone(), date: revision.date.clone() }));
                }
            }
            content = content.push(line);
            if !revision.note.is_empty() {
                content = content.push(note(revision.note.clone()));
            }
        }

        // the versions Git keeps of it, when the project folder is a checkout
        if let Some(versions) = docs.versions.as_ref().filter(|v| v.path == path) {
            content = content.push(section(l.text("Geschiedenis in Git", "History in Git")));
            match &versions.result {
                Err(error) => content = content.push(note(error.clone())),
                Ok((commits, standing)) => {
                    content = content.push(note(
                        match standing {
                            GitStanding::Same => l.text("Zoals de laatste versie in Git", "As the last version in Git"),
                            GitStanding::Changed => l.text("Gewijzigd sinds de laatste versie in Git", "Changed since the last version in Git"),
                            GitStanding::New => l.text("Staat nog niet in Git", "Not in Git yet"),
                        }
                        .to_string(),
                    ));
                    for (i, commit) in commits.iter().enumerate() {
                        let day = commit.date.get(..10).unwrap_or(&commit.date);
                        let mut line = row![text(format!("{} · {day} · {}", commit.short(), commit.by)).size(12).color(p.text).width(Fill).wrapping(Wrapping::WordOrGlyph), action(l.text("Openen", "Open"), DocsMessage::OpenVersion(commit.hash.clone()))]
                            .spacing(4)
                            .align_y(Center);
                        // the newest one is the file as it is, unless it changed since
                        if i > 0 || *standing == GitStanding::Changed {
                            line = line.push(put_back(Version::Git(commit.hash.clone())));
                        }
                        content = content.push(line);
                        if !commit.subject.is_empty() {
                            content = content.push(note(commit.subject.clone()));
                        }
                    }
                }
            }
        }

        // sharing it: through the service that keeps the folder, by its address, by mail
        if let (Some(file), Some(folder)) = (file, docs.folder.as_ref()) {
            content = content.push(section(l.text("Delen", "Share")));
            let local = folder.join(&file.path).to_string_lossy().into_owned();
            let web = docs.sync.as_ref().and_then(|sync| bim_docs::share::web(sync, folder, &file.path, docs.branch.as_deref()));
            let service = docs.sync.as_ref().and_then(|s| s.service.name()).unwrap_or("");
            let mut ways = row![].spacing(4);
            if let Some(web) = &web {
                ways = ways.push(action(&format!("{} {service}", l.text("Openen in", "Open in")), DocsMessage::Visit(web.file.clone()))).push(action(l.text("Link kopiëren", "Copy link"), DocsMessage::Copy(web.file.clone())));
                if let Some(history) = &web.history {
                    ways = ways.push(action(&format!("{} {service}", l.text("Versies in", "Versions in")), DocsMessage::Visit(history.clone())));
                }
            }
            ways = ways.push(action(l.text("Pad kopiëren", "Copy path"), DocsMessage::Copy(local.clone())));
            let name = file.name().to_string();
            let mail = bim_docs::share::mail(&name, web.as_ref().map_or(local.as_str(), |w| w.file.as_str()));
            ways = ways.push(action(l.text("Mailen", "Mail"), DocsMessage::Visit(mail)));
            content = content.push(ways.wrap());
            let said = match docs.sync.as_ref().map(|s| s.service) {
                Some(Service::SharePoint | Service::OneDrive) if web.is_some() => l.text(
                    "Met anderen delen en de versies die SharePoint en Teams bijhouden staan op de pagina van het bestand.",
                    "Sharing with others and the versions SharePoint and Teams keep are on the file's page.",
                ),
                Some(Service::Dropbox) => l.text("Met anderen delen en de versies die Dropbox bijhoudt staan op de pagina van het bestand.", "Sharing with others and the versions Dropbox keeps are on the file's page."),
                Some(Service::GitHub) => l.text("Wie de repository kan zien, opent de link.", "Whoever can see the repository opens the link."),
                Some(Service::Network) => l.text("Wie de netwerkmap kan bereiken, opent het pad.", "Whoever can reach the network folder opens the path."),
                Some(Service::Local) => l.text("Een map op deze computer: het pad werkt alleen hier.", "A folder on this computer: the path only works here."),
                _ => "",
            };
            if !said.is_empty() {
                content = content.push(note(said.to_string()));
            }
        }

        // the elements of the model it is about
        content = content.push(section(l.text("Elementen", "Elements")));
        let selected = self.sel.items().iter().filter(|item| matches!(item, Pickable::Element(_))).count();
        if selected > 0 {
            content = content.push(action(l.text("Koppelen aan selectie", "Link to selection"), DocsMessage::Link));
        } else if document.is_none_or(|d| d.elements.is_empty()) {
            content = content.push(note(l.text("Selecteer elementen in het model om dit document eraan te koppelen.", "Select elements in the model to link this document to them.").to_string()));
        }
        for element in document.into_iter().flat_map(|d| d.elements.iter()) {
            content = content.push(
                row![
                    button(text(element.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                        .padding([2, 6])
                        .width(Fill)
                        .style(move |_, s| p.row_button(s))
                        .on_press(Message::Docs(DocsMessage::Go(element.key.clone()))),
                    text(element.model.clone()).size(11).color(p.text_muted),
                    button(text("×").size(12).color(p.text_secondary)).padding([0, 6]).style(move |_, s| p.row_button(s)).on_press(Message::Docs(DocsMessage::Unlink(element.key.clone()))),
                ]
                .spacing(4)
                .align_y(Center),
            );
        }
        Some(container(content).padding([2, 8]).into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DocTab, Status};
    use super::*;
    use crate::model::{Layer, Model};
    use crate::selection::Pickable;
    use std::path::Path;

    /// A fresh project folder with `files` in it.
    fn project(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("o3v-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, file.as_bytes()).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn send(app: &mut App, message: DocsMessage) {
        let _ = app.update(Message::Docs(message));
    }

    /// Reads the project folder at once, as the reading thread would.
    fn read(app: &mut App) {
        let folder = app.docs.folder.clone().expect("a project folder");
        send(app, DocsMessage::Read(Box::new(read_project(folder))));
    }

    /// An app with Docs open on the project folder `root`.
    fn opened(root: &Path) -> App {
        let mut app = App::default();
        send(&mut app, DocsMessage::Open);
        send(&mut app, DocsMessage::Chosen(Some(root.to_path_buf())));
        read(&mut app);
        app
    }

    fn shows(app: &App, wording: &str) -> bool {
        let mut ui = crate::app::test_ui(app, iced::Size::new(1400.0, 900.0));
        ui.find(wording).is_ok()
    }

    /// An IFCX model of two squares: elements 10 and 20, each with a GlobalId, a name and its
    /// node path.
    fn model() -> Arc<Model> {
        let mut positions = Vec::new();
        for x in [0.0, 2.0] {
            positions.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
        }
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            props: vec![
                (10, r#"{"Name":"Wall 10","GlobalId":"g-10","Id":"path-10"}"#.into()),
                (20, r#"{"Name":"Wall 20","GlobalId":"g-20","Id":"path-20"}"#.into()),
            ],
            ..Layer::default()
        };
        Arc::new(Model::new(PathBuf::from("kade12.ifcx"), "IFCX", vec![layer]))
    }

    #[test]
    fn the_docs_tab_opens_without_a_model_and_asks_for_the_project_folder() {
        let mut app = App::default();
        send(&mut app, DocsMessage::Open);
        assert_eq!(app.doc_tabs(), [DocTab::Model, DocTab::Docs]);
        assert_eq!(app.active_doc_tab(), 1);
        assert!(shows(&app, "Choose project folder…"));
        let _ = app.update(Message::SelectTab(0));
        assert!(!app.docs.shown);
        let _ = app.update(Message::SelectTab(1));
        assert!(app.docs.shown);
        let _ = app.update(Message::CloseTab(1));
        assert_eq!(app.doc_tabs(), [DocTab::Model]);
        assert!(!app.docs.shown);
    }

    #[test]
    fn a_project_folder_shows_its_folders_and_the_files_of_the_picked_one() {
        let root = project("lists", &["model/kade12.ifc", "drawings/A-101.pdf", "notes.txt"]);
        let mut app = opened(&root);
        assert!(shows(&app, "notes.txt") && shows(&app, "drawings") && shows(&app, "model"));
        assert!(!shows(&app, "A-101.pdf"));
        send(&mut app, DocsMessage::Folder("drawings".into()));
        assert!(shows(&app, "A-101.pdf"));
        assert!(!shows(&app, "notes.txt"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recording_a_revision_writes_its_entry_and_a_later_change_of_the_file_shows() {
        let root = project("record", &["drawings/A-101.pdf"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Folder("drawings".into()));
        send(&mut app, DocsMessage::Pick("drawings/A-101.pdf".into()));
        send(&mut app, DocsMessage::Code("P01".into()));
        send(&mut app, DocsMessage::Note("for coordination".into()));
        send(&mut app, DocsMessage::Record);
        assert_eq!(std::fs::read_dir(root.join(bim_docs::REGISTER_FOLDER)).expect("the register").count(), 1);
        let document = app.docs.register.of("drawings/A-101.pdf").expect("its entry");
        assert_eq!((document.revision.as_str(), document.revisions.len(), document.revisions[0].note.as_str()), ("P01", 1, "for coordination"));
        assert!(app.docs.code.is_empty() && app.docs.note.is_empty(), "the fields are empty for the next revision");
        assert!(shows(&app, "P01"));
        assert!(!shows(&app, "changed since P01"));
        // the file changes, and the folder is read again
        std::fs::write(root.join("drawings/A-101.pdf"), b"a longer drawing than the one recorded").unwrap();
        read(&mut app);
        assert!(shows(&app, "changed since P01"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recording_without_a_code_writes_nothing_and_says_so() {
        let root = project("nocode", &["notes.txt"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Pick("notes.txt".into()));
        send(&mut app, DocsMessage::Record);
        assert!(!root.join(bim_docs::SUITE_FOLDER).exists());
        assert!(shows(&app, "Give the revision a code first"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_title_and_a_status_are_kept_in_the_register() {
        let root = project("title", &["notes.txt"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Pick("notes.txt".into()));
        send(&mut app, DocsMessage::Title("Site notes".into()));
        send(&mut app, DocsMessage::SaveTitle);
        send(&mut app, DocsMessage::Standing(Standing::Published));
        // as someone else reads it from the folder
        let register = Register::read(&root);
        assert_eq!(register.documents.len(), 1, "one entry for both changes");
        let document = register.of("notes.txt").expect("its entry");
        assert_eq!((document.title.as_str(), document.status), ("Site notes", Standing::Published));
        assert!(!document.by.is_empty() && !document.changed.is_empty());
        assert!(shows(&app, "Site notes"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_document_links_to_the_selected_elements_and_a_link_selects_its_element_again() {
        let root = project("links", &["drawings/A-101.pdf"]);
        let mut app = opened(&root);
        app.set_model(model());
        app.sel.extend([Pickable::Element(10), Pickable::Element(20)]);
        app.selection_changed();
        send(&mut app, DocsMessage::Open);
        send(&mut app, DocsMessage::Folder("drawings".into()));
        send(&mut app, DocsMessage::Pick("drawings/A-101.pdf".into()));
        send(&mut app, DocsMessage::Link);
        let document = app.docs.register.of("drawings/A-101.pdf").expect("its entry");
        let links: Vec<_> = document.elements.iter().map(|e| (e.key.as_str(), e.path.as_str(), e.name.as_str(), e.model.as_str())).collect();
        assert_eq!(links, [("g-10", "path-10", "Wall 10", "kade12.ifcx"), ("g-20", "path-20", "Wall 20", "kade12.ifcx")]);
        assert!(shows(&app, "Wall 10") && shows(&app, "Wall 20"));
        // a link goes to its element in the model
        app.sel.clear();
        app.selection_changed();
        send(&mut app, DocsMessage::Go("g-20".into()));
        assert_eq!(app.sel.items(), [Pickable::Element(20)]);
        assert!(!app.docs.shown, "the model shows");
        // and a link can be undone
        send(&mut app, DocsMessage::Open);
        send(&mut app, DocsMessage::Unlink("g-10".into()));
        assert_eq!(Register::read(&root).of("drawings/A-101.pdf").map(|d| d.elements.len()), Some(1));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_selected_element_lists_the_documents_about_it_and_one_opens_in_docs() {
        let root = project("element", &["drawings/plans/A-101.pdf", "notes.txt"]);
        let mut app = opened(&root);
        app.set_model(model());
        app.sel.set(Some(Pickable::Element(10)));
        app.selection_changed();
        send(&mut app, DocsMessage::Open);
        send(&mut app, DocsMessage::Pick("drawings/plans/A-101.pdf".into()));
        send(&mut app, DocsMessage::Title("Ground floor plan".into()));
        send(&mut app, DocsMessage::SaveTitle);
        send(&mut app, DocsMessage::Standing(Standing::Shared));
        send(&mut app, DocsMessage::Link);
        // back at the model, the wall says which document is about it
        let _ = app.update(Message::SelectTab(0));
        assert!(shows(&app, "DOCUMENTS") && shows(&app, "Ground floor plan"));
        assert!(shows(&app, "Shared"));
        // the other wall has none
        app.sel.set(Some(Pickable::Element(20)));
        app.selection_changed();
        assert!(!shows(&app, "DOCUMENTS") && !shows(&app, "Ground floor plan"));
        // the document opens in Docs: its folder shows, open in the tree, and it is picked
        send(&mut app, DocsMessage::Show("drawings/plans/A-101.pdf".into()));
        assert!(app.docs.shown);
        assert_eq!((app.docs.at.as_str(), app.docs.pick.as_deref()), ("drawings/plans", Some("drawings/plans/A-101.pdf")));
        assert!(app.docs.opened.contains("drawings") && app.docs.opened.contains("drawings/plans"));
        assert_eq!(app.docs.title, "Ground floor plan");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_register_is_read_for_a_model_before_docs_was_opened() {
        let root = project("known", &["drawings/A-101.pdf"]);
        // someone linked the plan to wall 10 before
        {
            let mut app = opened(&root);
            app.set_model(model());
            app.sel.set(Some(Pickable::Element(10)));
            app.selection_changed();
            send(&mut app, DocsMessage::Open);
            send(&mut app, DocsMessage::Folder("drawings".into()));
            send(&mut app, DocsMessage::Pick("drawings/A-101.pdf".into()));
            send(&mut app, DocsMessage::Link);
        }
        // a fresh start with that folder chosen, Docs not opened
        let mut app = App::default();
        assert!(!app.docs_wants_register(), "no folder, nothing to read");
        app.docs.folder = Some(root.clone());
        assert!(app.docs_wants_register());
        // the register of another folder is not this project's
        send(&mut app, DocsMessage::Known(Box::new((root.join("drawings"), Register::read(&root)))));
        assert!(app.docs_wants_register() && app.docs.register.documents.is_empty());
        send(&mut app, DocsMessage::Known(Box::new((root.clone(), Register::read(&root)))));
        assert!(!app.docs_wants_register());
        app.set_model(model());
        app.sel.set(Some(Pickable::Element(10)));
        app.selection_changed();
        assert!(shows(&app, "DOCUMENTS") && shows(&app, "A-101.pdf"));
        assert!(!app.docs.open, "Docs itself stays closed");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_second_click_on_a_file_opens_it_and_the_folders_stand_above_the_files() {
        let root = project("click", &["model/kade12.ifc", "model/old/first.ifc", "report.docx"]);
        let mut app = opened(&root);
        // the way up shows in a folder, not in the project folder itself
        assert!(!shows(&app, ".."));
        send(&mut app, DocsMessage::Folder("model".into()));
        assert!(shows(&app, "..") && shows(&app, "old"));
        // one click picks, a second one soon after opens the model in the suite
        send(&mut app, DocsMessage::Pick("model/kade12.ifc".into()));
        assert!(app.docs.shown && !matches!(app.status, Status::Loading { .. }));
        send(&mut app, DocsMessage::Pick("model/kade12.ifc".into()));
        assert!(matches!(app.status, Status::Loading { .. }), "{:?}", app.status);
        assert!(!app.docs.shown);
        // a click long after the first only picks
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Pick("report.docx".into()));
        app.docs.clicked = Some(("report.docx".into(), Instant::now() - Duration::from_secs(2)));
        send(&mut app, DocsMessage::Pick("report.docx".into()));
        assert!(app.docs.shown && app.docs.notice.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_open_model_is_saved_in_the_folder_that_shows_or_where_it_came_from() {
        let root = project("save", &["model/kade12.ifcx", "model/site.rvt", "report.docx"]);
        let mut app = opened(&root);
        // no model: nothing to save
        assert_eq!(app.docs_save_target(), None);
        assert!(!shows(&app, "Save model in project folder"));
        // a model from elsewhere: a new IFCX file in the folder that shows, never over a file
        app.model = Some(model());
        assert!(shows(&app, "Save model in project folder"));
        assert_eq!(app.docs_save_target(), Some((root.join("kade12.ifcx"), false)));
        send(&mut app, DocsMessage::Folder("model".into()));
        assert_eq!(app.docs_save_target(), Some((root.join("model").join("kade12.ifcx"), false)));
        // a model opened from the project folder in a format the suite writes: where it is
        let mut own = (*model()).clone();
        own.path = root.join("model").join("kade12.ifcx");
        app.model = Some(Arc::new(own));
        send(&mut app, DocsMessage::Folder(String::new()));
        assert_eq!(app.docs_save_target(), Some((root.join("model").join("kade12.ifcx"), true)));
        // one in a format the suite only reads: a new IFCX file beside what shows
        let mut read_only = (*model()).clone();
        read_only.path = root.join("model").join("site.rvt");
        app.model = Some(Arc::new(read_only));
        assert_eq!(app.docs_save_target(), Some((root.join("site.ifcx"), false)));
        // saving starts, and once saved Docs goes to the file
        send(&mut app, DocsMessage::Save);
        assert!(matches!(app.status, Status::Saving { .. }), "{:?}", app.status);
        std::fs::write(root.join("site.ifcx"), b"{}").unwrap();
        let _ = app.update(Message::Saved(Ok((root.join("site.ifcx"), 0.1))));
        read(&mut app);
        assert_eq!((app.docs.pick.as_deref(), app.docs.at.as_str()), (Some("site.ifcx"), ""));
        assert!(app.docs.listing.file("site.ifcx").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Against the `git` program itself; a computer without it has nothing to check.
    #[test]
    fn a_git_checkout_shows_the_versions_of_the_picked_file_and_gives_an_older_one() {
        let root = project("git", &["Kade 12/model/kade12.ifc"]);
        let git = |arguments: &[&str]| std::process::Command::new("git").arg("-C").arg(&root).args(arguments).output().is_ok_and(|o| o.status.success());
        if !git(&["init", "-q"]) {
            return;
        }
        let commit = |message: &str| {
            assert!(git(&["add", "-A"]));
            assert!(git(&["-c", "user.name=An", "-c", "user.email=an@firm.example", "-c", "commit.gpgsign=false", "commit", "-q", "-m", message]));
        };
        commit("The first model");
        std::fs::write(root.join("Kade 12/model/kade12.ifc"), b"second").unwrap();
        commit("Windows on the first floor");
        assert!(git(&["remote", "add", "origin", "git@github.com:firm/kade-12.git"]));

        // the project folder is a folder of the checkout, not the checkout itself
        let mut app = opened(&root.join("Kade 12"));
        assert_eq!(app.docs.sync.as_ref().map(|s| (s.service, s.site.as_deref())), Some((Service::GitHub, Some("https://github.com/firm/kade-12"))));
        assert!(shows(&app, "synced with GitHub"));
        send(&mut app, DocsMessage::Folder("model".into()));
        send(&mut app, DocsMessage::Pick("model/kade12.ifc".into()));
        app.docs_versions_now();
        let versions = app.docs.versions.clone().expect("asked of Git");
        let (commits, standing) = versions.result.expect("git answers");
        assert_eq!(commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["Windows on the first floor", "The first model"]);
        assert_eq!(standing, GitStanding::Same);
        for text in ["HISTORY IN GIT", "Windows on the first floor", "As the last version in Git"] {
            assert!(shows(&app, text), "{text}");
        }
        // the first version, as a file of its own outside the project folder
        let (checkout, inside) = app.docs_in_git().expect("in a checkout");
        assert_eq!(inside, "Kade 12/model/kade12.ifc");
        let older = version_file(&checkout, &inside, &commits[1].hash).expect("the older version");
        assert_eq!(std::fs::read(&older).unwrap(), b"Kade 12/model/kade12.ifc");
        assert!(older.file_name().unwrap().to_string_lossy().starts_with("kade12 @ ") && !older.starts_with(&root));
        // shared through its page on GitHub, at the branch the checkout is on
        assert!(app.docs.branch.is_some());
        assert!(shows(&app, "Open in GitHub") && shows(&app, "Copy link") && shows(&app, "Versions in GitHub"));
        // the first version goes back in the file's place, at the second press
        send(&mut app, DocsMessage::PutBack(Version::Git(commits[1].hash.clone())));
        assert!(shows(&app, "Put back for sure?"));
        assert_eq!(std::fs::read(root.join("Kade 12/model/kade12.ifc")).unwrap(), b"second");
        send(&mut app, DocsMessage::PutBack(Version::Git(commits[1].hash.clone())));
        assert_eq!(std::fs::read(root.join("Kade 12/model/kade12.ifc")).unwrap(), b"Kade 12/model/kade12.ifc");
        let Some(Notice::PutBack(aside)) = app.docs.notice.clone() else { panic!("{:?}", app.docs.notice) };
        assert_eq!(std::fs::read(&aside).unwrap(), b"second", "the file as it was is kept aside");
        let _ = std::fs::remove_file(aside);
        // another file picked: what Git said of the first no longer shows
        std::fs::write(root.join("Kade 12/model/new.ifc"), b"new").unwrap();
        read(&mut app);
        send(&mut app, DocsMessage::Pick("model/new.ifc".into()));
        assert!(app.docs.versions.is_none());
        app.docs_versions_now();
        assert!(shows(&app, "Not in Git yet"));
        let _ = std::fs::remove_file(older);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opening_a_model_from_docs_starts_loading_it_and_leaves_the_page() {
        let root = project("open", &["model/kade12.ifc", "report.docx"]);
        let mut app = opened(&root);
        // only what the suite reads offers to open in it
        send(&mut app, DocsMessage::Pick("report.docx".into()));
        assert!(shows(&app, "Open in its program"));
        assert!(!shows(&app, "Open in the suite"));
        send(&mut app, DocsMessage::Folder("model".into()));
        send(&mut app, DocsMessage::Pick("model/kade12.ifc".into()));
        assert!(shows(&app, "Open in the suite"));
        send(&mut app, DocsMessage::OpenModel { add: false });
        assert!(matches!(app.status, Status::Loading { .. }), "{:?}", app.status);
        assert!(!app.docs.shown);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_command_line_opens_docs_on_a_folder_with_a_file_picked() {
        let root = project("cli", &["drawings/plans/A-101.pdf", "notes.txt"]);
        let mut app = App::default();
        app.docs_at(root.clone(), Some("drawings\\plans\\A-101.pdf"));
        assert!(app.docs.shown && !app.docs.reading);
        assert_eq!((app.docs.at.as_str(), app.docs.pick.as_deref()), ("drawings/plans", Some("drawings/plans/A-101.pdf")));
        // the folders above the file are open, so it is in sight in the tree
        assert!(shows(&app, "plans") && shows(&app, "A-101.pdf"));
        assert!(shows(&app, "No revision recorded yet"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_that_cannot_be_read_says_so_and_letting_go_asks_for_a_folder_again() {
        let mut app = App::default();
        send(&mut app, DocsMessage::Open);
        let gone = std::env::temp_dir().join("o3v-docs-no-such-folder");
        send(&mut app, DocsMessage::Chosen(Some(gone)));
        read(&mut app);
        assert!(matches!(&app.docs.notice, Some(Notice::Failed(error)) if error.contains("o3v-docs-no-such-folder")), "{:?}", app.docs.notice);
        send(&mut app, DocsMessage::Leave);
        assert!(app.docs.folder.is_none() && app.docs.notice.is_none());
        assert!(shows(&app, "Choose project folder…"));
    }

    #[test]
    fn the_path_above_the_list_goes_to_a_folder_and_backspace_goes_up() {
        let root = project("path", &["drawings/plans/A-101.pdf", "drawings/D-301.dwg", "notes.txt"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Folder("drawings/plans".into()));
        // the path names each folder on the way, and the folders above it open in the tree
        assert!(shows(&app, "plans") && shows(&app, "A-101.pdf"));
        assert!(app.docs.opened.contains("drawings") && app.docs.opened.contains("drawings/plans"));
        // Backspace goes to the folder above, as in a file manager
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.docs.at, "drawings");
        assert!(shows(&app, "D-301.dwg") && !shows(&app, "A-101.pdf"));
        // a name in the path goes to that folder: here the project folder itself
        send(&mut app, DocsMessage::Folder(String::new()));
        assert!(shows(&app, "notes.txt"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_search_finds_files_in_every_folder_by_name_and_title() {
        let root = project("search", &["drawings/plans/A-101.pdf", "drawings/A-201.pdf", "model/kade12.ifc"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Folder("model".into()));
        send(&mut app, DocsMessage::Pick("model/kade12.ifc".into()));
        send(&mut app, DocsMessage::Title("Structural model".into()));
        send(&mut app, DocsMessage::SaveTitle);
        // by name, through every folder; each found file says where it is
        send(&mut app, DocsMessage::Query("a-".into()));
        assert_eq!(app.docs_rows().iter().map(|(path, ..)| *path).collect::<Vec<_>>(), ["drawings/plans/A-101.pdf", "drawings/A-201.pdf"]);
        assert!(shows(&app, "drawings/plans") && shows(&app, "local folder, not shared · 2 found"));
        // by title
        send(&mut app, DocsMessage::Query("structural".into()));
        assert_eq!(app.docs_rows().iter().map(|(path, ..)| *path).collect::<Vec<_>>(), ["model/kade12.ifc"]);
        send(&mut app, DocsMessage::Query("nothing like it".into()));
        assert!(shows(&app, "Nothing found in the project folder"));
        // Backspace outside the search field ends the search
        let _ = app.update(Message::DeletePicked);
        assert!(app.docs.query.is_empty() && app.docs.at == "model");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_column_heading_sorts_the_files_and_again_turns_the_order_round() {
        let root = project("sort", &["b-middle.txt", "a-longest name of all.txt", "c.txt"]);
        let mut app = opened(&root);
        let order = |app: &App| app.docs_rows().iter().map(|(path, ..)| path.to_string()).collect::<Vec<_>>();
        assert_eq!(order(&app), ["a-longest name of all.txt", "b-middle.txt", "c.txt"]);
        // the files hold their own names: the size follows the name's length
        send(&mut app, DocsMessage::Sort(By::Size));
        assert_eq!(order(&app), ["c.txt", "b-middle.txt", "a-longest name of all.txt"]);
        assert!(shows(&app, "Size ▴"));
        send(&mut app, DocsMessage::Sort(By::Size));
        assert_eq!(order(&app), ["a-longest name of all.txt", "b-middle.txt", "c.txt"]);
        assert!(shows(&app, "Size ▾"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_arrow_keys_step_through_the_files_and_enter_opens_one() {
        let root = project("keys", &["a.ifc", "b.txt", "c.txt"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Step(1));
        assert_eq!(app.docs.pick.as_deref(), Some("a.ifc"));
        send(&mut app, DocsMessage::Step(1));
        send(&mut app, DocsMessage::Step(1));
        send(&mut app, DocsMessage::Step(1));
        assert_eq!(app.docs.pick.as_deref(), Some("c.txt"), "the last file stays picked");
        send(&mut app, DocsMessage::Step(-2));
        assert_eq!(app.docs.pick.as_deref(), Some("a.ifc"));
        // Enter opens the picked model in the suite
        let _ = app.update(Message::MeasureFinish);
        assert!(matches!(app.status, Status::Loading { .. }), "{:?}", app.status);
        assert!(!app.docs.shown);
        // the keys do nothing to Docs while it does not show
        send(&mut app, DocsMessage::Step(1));
        assert_eq!(app.docs.pick.as_deref(), Some("a.ifc"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_recorded_revision_keeps_a_copy_that_opens_and_goes_back_after_a_second_press() {
        let root = project("copies", &["drawings/A-101.pdf"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Folder("drawings".into()));
        send(&mut app, DocsMessage::Pick("drawings/A-101.pdf".into()));
        assert!(shows(&app, "Recording keeps a copy in the project folder, to open or put back later."));
        send(&mut app, DocsMessage::Code("P01".into()));
        send(&mut app, DocsMessage::Record);
        let document = app.docs.register.of("drawings/A-101.pdf").expect("its entry").clone();
        let revision = document.revisions[0].clone();
        assert_eq!(std::fs::read(root.join(&revision.copy)).unwrap(), b"drawings/A-101.pdf", "a copy of the file as it was");
        // the copy is kept out of the list
        read(&mut app);
        assert!(app.docs.listing.files.iter().all(|f| !f.path.starts_with("_openaec")));
        // the file changes; the revision is put back at the second press, the change kept aside
        std::fs::write(root.join("drawings/A-101.pdf"), b"a later drawing").unwrap();
        read(&mut app);
        assert!(shows(&app, "Put back"));
        let version = Version::Recorded { code: revision.code.clone(), date: revision.date.clone() };
        send(&mut app, DocsMessage::PutBack(version.clone()));
        assert!(shows(&app, "Put back for sure?"));
        assert_eq!(std::fs::read(root.join("drawings/A-101.pdf")).unwrap(), b"a later drawing");
        send(&mut app, DocsMessage::PutBack(version));
        assert_eq!(std::fs::read(root.join("drawings/A-101.pdf")).unwrap(), b"drawings/A-101.pdf");
        let Some(Notice::PutBack(aside)) = app.docs.notice.clone() else { panic!("{:?}", app.docs.notice) };
        assert_eq!(std::fs::read(&aside).unwrap(), b"a later drawing");
        assert!(shows(&app, "The earlier version is back."));
        let _ = std::fs::remove_file(aside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_document_is_checked_out_and_the_list_says_by_whom() {
        let root = project("out", &["report.docx"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Pick("report.docx".into()));
        assert!(shows(&app, "Check out"));
        send(&mut app, DocsMessage::CheckOut);
        let out = app.docs.register.of("report.docx").and_then(|d| d.out.clone()).expect("checked out");
        assert_eq!(out.by, who());
        // read again, as the others would see it
        read(&mut app);
        assert!(shows(&app, &format!("checked out by {}", who())));
        assert!(shows(&app, "Check in"));
        send(&mut app, DocsMessage::CheckIn);
        assert!(app.docs.register.of("report.docx").is_some_and(|d| d.out.is_none()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_in_a_local_folder_is_shared_by_its_path_and_by_mail() {
        let root = project("share", &["report.docx"]);
        let mut app = opened(&root);
        send(&mut app, DocsMessage::Pick("report.docx".into()));
        assert!(shows(&app, "SHARE") && shows(&app, "Copy path") && shows(&app, "Mail"));
        assert!(shows(&app, "A folder on this computer: the path only works here."));
        assert!(!shows(&app, "Copy link"), "a local folder has no page on a website");
        send(&mut app, DocsMessage::Copy(root.join("report.docx").to_string_lossy().into_owned()));
        assert!(shows(&app, "On the clipboard"));
        let _ = std::fs::remove_dir_all(root);
    }
}
