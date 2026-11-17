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
//! Docs writes only when the person does something, and never opens a file to list it (see
//! `docs/superpowers/specs/2026-11-17-docs-design.md`).

use super::{App, Message, FONT_BOLD};
use crate::selection::Pickable;
use crate::theme::Palette;
use bim_docs::{Document, ElementLink, File, Kind, Listing, Register, Service, State, Status as Standing, Sync};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable, text, text_input, Column};
use iced::{Center, Element, Fill, Padding, Task};
use openaec_ui::Language;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

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
}

/// What Docs has to say after something the person did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// A revision needs a code before it is recorded.
    CodeFirst,
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
        }
    }
}

/// A project folder as it was read: what is in it, what is said about it, what syncs it.
#[derive(Debug, Clone)]
pub struct Read {
    pub folder: PathBuf,
    pub result: Result<(Listing, Register, Sync), String>,
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
}

/// The folder of the tree that lists the documents whose file is gone: no file has this name.
const MISSING: &str = "\0";
/// The most files a folder shows; the rest is said as a number.
const ROWS: usize = 500;

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
    Read { folder, result }
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
            rx.await.unwrap_or_else(|_| Read { folder: asked, result: Err("the reading stopped".into()) })
        },
        |read| Message::Docs(DocsMessage::Read(Box::new(read))),
    )
}

/// The browser has no folders of the computer.
#[cfg(target_arch = "wasm32")]
fn read_task(folder: PathBuf) -> Task<Message> {
    Task::done(Message::Docs(DocsMessage::Read(Box::new(Read { folder, result: Err("Docs works in the desktop app".into()) }))))
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
            DocsMessage::Read(read) => self.docs_take(*read),
            DocsMessage::Folder(folder) => {
                // a click on the folder that shows closes it in the tree; another one opens it
                if self.docs.at == folder && self.docs.opened.contains(&folder) {
                    self.docs.opened.remove(&folder);
                } else {
                    self.docs.opened.insert(folder.clone());
                }
                self.docs.at = folder;
                self.docs.pick = None;
            }
            DocsMessage::Pick(path) => {
                self.docs.title = self.docs.register.of(&path).map(|d| d.title.clone()).unwrap_or_default();
                self.docs.code.clear();
                self.docs.note.clear();
                self.docs.notice = None;
                self.docs.pick = Some(path);
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
                let (note, by, now) = (self.docs.note.clone(), who(), crate::export::ifcx::timestamp());
                self.docs_change(|d| {
                    d.record(&code, &note, &file, &by, &now);
                    true
                });
                if self.docs.notice.is_none() {
                    self.docs.code.clear();
                    self.docs.note.clear();
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
        }
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
        let _ = self.docs_update(DocsMessage::Pick(path.to_string()));
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
                docs.notice = None;
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
        let (Some(folder), Some(path)) = (self.docs.folder.clone(), self.docs.pick.clone()) else { return };
        let mut document = self.docs.register.of(&path).cloned().unwrap_or_else(|| Document::new(&crate::document::uuid(&format!("openaec-document/{}", path.to_lowercase())), &path));
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

    /// The Docs page: the project folder's tree and the files of the folder that shows. Nothing
    /// stands over them but a line that says which folder this is.
    pub(super) fn docs_page(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let Some(folder) = &docs.folder else {
            // no project folder yet: what Docs is, and the way to choose one
            let ask = column![
                text("Docs").size(20).font(FONT_BOLD).color(p.text),
                text(l.text(
                    "De documenten van het project: modellen, tekeningen, PDF's en rapporten, met hun status en versies. Kies de projectmap die Dropbox, SharePoint, Synology Drive of een netwerkschijf voor iedereen gelijk houdt.",
                    "The documents of the project: models, drawings, PDFs and reports, with their status and revisions. Choose the project folder that Dropbox, SharePoint, Synology Drive or a network drive keeps the same for everyone.",
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
        let mut about = vec![sync_text(docs.sync.as_ref(), l)];
        about.push(format!("{}{} {}", docs.listing.files.len(), if docs.listing.more { "+" } else { "" }, l.text("bestanden", "files")));
        if docs.reading {
            about.push(l.text("wordt gelezen…", "reading…").to_string());
        }
        about.retain(|part| !part.is_empty());
        let header = row![text(name).size(14).font(FONT_BOLD).color(p.text), text(about.join(" · ")).size(11).color(p.text_muted)].spacing(10).align_y(Center);
        let files = scrollable(self.docs_files(p, l)).width(Fill).height(Fill);
        // in the Docs product the folders are the left pane; elsewhere they stand beside the files
        let body: Element<'_, Message> = if docs.in_product {
            files.into()
        } else {
            row![scrollable(self.docs_tree(p, l)).width(230).height(Fill), rule::vertical(1), files].height(Fill).into()
        };
        container(column![container(header).padding([8, 12]), rule::horizontal(1), body])
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
        scrollable(self.docs_tree(p, l)).height(Fill).into()
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

    /// The folders of the project as a tree; the one whose files show is marked.
    fn docs_tree(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let line = |path: &str, label: String, depth: usize, count: usize, mark: &'static str| -> Element<'_, Message> {
            let shown = docs.at == path;
            button(
                row![
                    text(mark).size(11).color(p.text_secondary).width(12),
                    text(label).size(12).color(p.text).width(Fill).wrapping(Wrapping::None),
                    text(count.to_string()).size(11).color(p.text_muted),
                ]
                .spacing(4)
                .align_y(Center),
            )
            .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 6.0 + 14.0 * depth as f32 })
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
        body.into()
    }

    /// The files of the folder that shows: name, status, revision, changed and size. Under a name
    /// stand the document's title and what the sync tool or a later change says of the file.
    fn docs_files(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let docs = &self.docs;
        let cell = |value: String, width: f32, muted: bool| text(value).size(12).color(if muted { p.text_muted } else { p.text }).width(width).wrapping(Wrapping::None);
        let (status_w, revision_w, changed_w, size_w) = (112.0, 60.0, 118.0, 66.0);
        let heading = |label: &'static str, width: f32| text(label).size(11).font(FONT_BOLD).color(p.group_label).width(width);
        let head = row![
            text(l.text("Naam", "Name")).size(11).font(FONT_BOLD).color(p.group_label).width(Fill),
            heading(l.text("Status", "Status"), status_w),
            heading(l.text("Versie", "Revision"), revision_w),
            heading(l.text("Gewijzigd", "Changed"), changed_w),
            heading(l.text("Grootte", "Size"), size_w),
        ]
        .spacing(8)
        .padding([4, 12]);
        let mut body = Column::new().spacing(1).push(head).push(rule::horizontal(1));
        // (path, the file if it is there, what is said about it)
        let rows: Vec<(&str, Option<&File>, Option<&Document>)> = if docs.at == MISSING {
            self.docs_missing().map(|d| (d.file.as_str(), None, Some(d))).collect()
        } else {
            docs.listing.files_in(&docs.at).map(|f| (f.path.as_str(), Some(f), docs.register.of(&f.path))).collect()
        };
        if rows.is_empty() {
            let empty = if docs.reading { l.text("De map wordt gelezen…", "Reading the folder…") } else { l.text("Geen bestanden in deze map", "No files in this folder") };
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
            if file.is_some_and(|f| f.conflict) {
                marks.push(l.text("conflictkopie", "conflict copy").to_string());
            }
            if file.is_some_and(|f| f.online_only) {
                marks.push(l.text("alleen online", "only online").to_string());
            }
            let title = document.map(|d| d.title.clone()).filter(|t| !t.is_empty());
            // the name has the width; its title and marks stand under it
            let mut label = Column::new().push(text(name).size(12).color(p.text).wrapping(Wrapping::None));
            if title.is_some() || !marks.is_empty() {
                label = label.push(
                    row![]
                        .push(title.map(|t| text(t).size(11).color(p.text_muted).wrapping(Wrapping::None)))
                        .push((!marks.is_empty()).then(|| text(marks.join(" · ")).size(11).color(p.accent).wrapping(Wrapping::None)))
                        .spacing(8),
                );
            }
            let line = row![
                container(label).width(Fill).clip(true),
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

    /// The right pane while Docs shows: the project folder, then the picked document with its
    /// title, status, revisions and the elements it is about.
    pub(super) fn docs_properties(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let docs = &self.docs;
        if !docs.shown {
            return None;
        }
        let section = |title: &str| -> Element<'static, Message> { container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label)).padding(Padding { top: 8.0, right: 0.0, bottom: 2.0, left: 0.0 }).into() };
        let note = |value: String| text(value).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let action = |label: &'static str, message: DocsMessage| button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::Docs(message));
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
            }
        }
        match &docs.notice {
            Some(Notice::CodeFirst) => content = content.push(text(l.text("Geef de versie eerst een code", "Give the revision a code first")).size(12).color(p.accent)),
            Some(Notice::Failed(error)) => content = content.push(text(error.clone()).size(12).color(p.accent).wrapping(Wrapping::WordOrGlyph)),
            None => {}
        }
        let Some(path) = docs.pick.as_deref() else { return Some(container(content).padding([2, 8]).into()) };
        let (file, document) = (docs.listing.file(path), docs.register.of(path));
        let kind = Kind::of(path);

        // the document: what it is, and the ways to open it
        content = content.push(section(l.text("Document", "Document"))).push(text(path.rsplit('/').next().unwrap_or(path).to_string()).size(12).font(FONT_BOLD).color(p.text).wrapping(Wrapping::WordOrGlyph));
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

        // its revisions: the one that stands, recording the file as it is now, and those before
        content = content.push(section(l.text("Versie", "Revision")));
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
            content = content
                .push(field(l.text("Code, bijvoorbeeld P01", "Code, for example P01"), &docs.code, DocsMessage::Code, DocsMessage::Record))
                .push(field(l.text("Opmerking bij deze versie", "Note on this revision"), &docs.note, DocsMessage::Note, DocsMessage::Record))
                .push(action(l.text("Versie vastleggen", "Record revision"), DocsMessage::Record));
        }
        for revision in document.into_iter().flat_map(|d| d.revisions.iter().rev()) {
            let day = revision.date.get(..10).unwrap_or(&revision.date);
            content = content.push(text(format!("{} · {day} · {}", revision.code, revision.by)).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph));
            if !revision.note.is_empty() {
                content = content.push(note(revision.note.clone()));
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
}
