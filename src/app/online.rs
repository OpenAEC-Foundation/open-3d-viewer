//! Online libraries in the Library product: library files that are not compiled in, kept in a
//! place with an index (`bim_library::remote`), listed from the index and fetched when asked.
//!
//! The place the suite starts with is the Open BIM Library on GitHub. The page shows the
//! libraries of its index as tiles once the index is fetched; the right pane shows the picked
//! one (what it holds, how many types at each level of development, where it comes from) with
//! a button that fetches it and adds to the project's library what the project lacks.
//!
//! Nothing is fetched until it is asked for, and only from the place set.

use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::lod::Lod;
use bim_library::remote::{Entry, Index};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Column, Space};
use iced::{Center, Element, Fill, Task};
use openaec_ui::Language;

/// Where the suite's own online libraries are kept: the address of the index's folder.
pub const PLACE: &str = "https://raw.githubusercontent.com/octaafpc/open-bim-library/main/";

#[derive(Debug, Clone)]
pub enum OnlineMessage {
    /// Fetch the index of the place.
    Refresh,
    /// The index came back, or why it did not.
    Index(Result<String, String>),
    /// Show this library of the index.
    Pick(String),
    /// Fetch this library and add it to the project's.
    Take(String),
    /// A library came back, or why it did not.
    Library(String, Result<String, String>),
}

/// The place, its index once fetched, which library is picked, whether something is being
/// fetched, and what was last said: how many things were added, or why not.
#[derive(Debug, Clone)]
pub struct OnlineView {
    pub place: String,
    pub index: Option<Index>,
    pub picked: Option<String>,
    pub busy: bool,
    pub added: Option<(String, usize)>,
    pub failed: Option<String>,
}

impl Default for OnlineView {
    fn default() -> Self {
        Self { place: PLACE.into(), index: None, picked: None, busy: false, added: None, failed: None }
    }
}

/// The address of `file` of the place: the place's folder and the file's path in it.
pub fn address(place: &str, file: &str) -> String {
    format!("{}/{file}", place.trim_end_matches('/'))
}

/// Fetches the text at `address` (https only), at most 32 MB.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(address: &str) -> Result<String, String> {
    use std::io::Read;
    if !address.starts_with("https://") {
        return Err("only https addresses are fetched".into());
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    let answer = agent.get(address).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "the place has no such file".to_string(),
        other => other.to_string(),
    })?;
    let mut text = String::new();
    answer.into_reader().take(32 * 1024 * 1024).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

#[cfg(target_arch = "wasm32")]
fn fetch(_: &str) -> Result<String, String> {
    Err("not in the browser".into())
}

/// Fetches `address` on a worker thread and says `done` of what came back.
fn fetch_task(address: String, done: impl FnOnce(Result<String, String>) -> OnlineMessage + Send + 'static) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(&address));
            });
            rx.await.unwrap_or_else(|_| Err("nothing came back".into()))
        },
        move |result| Message::Online(done(result)),
    )
}

/// What a library of the index holds, in a line: "materials 37 · build-ups 79 · objects 146".
pub fn holds(entry: &Entry, l: Language) -> String {
    let c = &entry.counts;
    [
        (l.text("materialen", "materials"), c.materials),
        (l.text("opbouwen", "build-ups"), c.buildups),
        (l.text("staaftypen", "linear types"), c.linear_types),
        (l.text("systemen", "systems"), c.system_types),
        (l.text("objecten", "objects"), c.object_types),
        (l.text("tekeningsets", "drawing sets"), c.drawing_sets),
        (l.text("weergavesjablonen", "view templates"), c.view_templates),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(what, count)| format!("{what} {count}"))
    .collect::<Vec<_>>()
    .join(" · ")
}

/// The levels of development of a library of the index, in a line: "LOD 200: 17 · LOD 300: 129".
pub fn levels(entry: &Entry) -> String {
    Lod::ALL.iter().zip(entry.lod).filter(|(_, count)| *count > 0).map(|(lod, count)| format!("{}: {count}", lod.name())).collect::<Vec<_>>().join(" · ")
}

impl App {
    pub(super) fn online_update(&mut self, message: OnlineMessage) -> Task<Message> {
        match message {
            OnlineMessage::Refresh => {
                self.online.busy = true;
                self.online.failed = None;
                return fetch_task(address(&self.online.place, "index.json"), OnlineMessage::Index);
            }
            OnlineMessage::Index(result) => {
                self.online.busy = false;
                match result.and_then(|text| Index::parse(&text).map_err(|e| e.to_string())) {
                    Ok(index) => {
                        self.online.picked = self.online.picked.take().filter(|key| index.entry(key).is_some());
                        self.online.index = Some(index);
                    }
                    Err(why) => self.online.failed = Some(why),
                }
            }
            OnlineMessage::Pick(key) => self.online.picked = Some(key),
            OnlineMessage::Take(key) => {
                if let Some(file) = self.online.index.as_ref().and_then(|index| index.entry(&key)).map(|entry| entry.file.clone()) {
                    self.online.busy = true;
                    self.online.failed = None;
                    return fetch_task(address(&self.online.place, &file), move |result| OnlineMessage::Library(key, result));
                }
            }
            OnlineMessage::Library(key, result) => {
                self.online.busy = false;
                match result.and_then(|text| bim_library::remote::library(&text).map_err(|e| e.to_string())) {
                    Ok(library) => {
                        let added = self.library.merge(&library);
                        self.online.added = Some((key, added));
                        if self.modeller.shown.is_some() {
                            self.show_modelled();
                        }
                    }
                    Err(why) => self.online.failed = Some(why),
                }
            }
        }
        Task::none()
    }

    /// The libraries of the place's index as tiles, and a tile that fetches the index.
    pub(super) fn online_tiles(&self, p: Palette, l: Language) -> Vec<Element<'_, Message>> {
        let lang = super::library::lang(l);
        let mut tiles: Vec<Element<'_, Message>> = self
            .online
            .index
            .iter()
            .flat_map(|index| &index.libraries)
            .map(|entry| {
                let look = container(text(entry.counts.total().to_string()).size(26).color(p.text)).width(Fill).align_x(Center);
                let on = self.online.picked.as_deref() == Some(entry.key.as_str());
                super::libraries::tile(p, look.into(), entry.names.get(lang).to_string(), levels(entry), on, Some(Message::Online(OnlineMessage::Pick(entry.key.clone()))))
            })
            .collect();
        let fetch = if self.online.index.is_some() { l.text("Index verversen", "Refresh the index") } else { l.text("Index ophalen", "Fetch the index") };
        tiles.push(super::libraries::tile_more(p, fetch, Message::Online(OnlineMessage::Refresh)));
        tiles
    }

    /// The right pane for the online libraries: the place, what was last said, and the picked
    /// library with the button that adds it to the project.
    pub fn online_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let view = &self.online;
        let line = |content: String| text(content).size(11).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph);
        let mut pane: Column<'_, Message> = column![
            text(view.index.as_ref().map_or(l.text("Online bibliotheken", "Online libraries").to_string(), |index| index.name.clone())).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            line(view.place.clone()),
        ]
        .spacing(4);
        if view.index.is_none() && !view.busy {
            pane = pane.push(line(l.text("Er wordt niets opgehaald tot je erom vraagt. Klik \"Index ophalen\" om te zien welke bibliotheken er zijn.", "Nothing is fetched until you ask. Click \"Fetch the index\" to see which libraries there are.").to_string()));
        }
        if view.busy {
            pane = pane.push(line(l.text("Bezig met ophalen...", "Fetching...").to_string()));
        }
        if let Some(why) = &view.failed {
            pane = pane.push(text(format!("{} ({why})", l.text("Niet opgehaald.", "Not fetched."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        }
        if let Some((key, added)) = &view.added {
            let name = view.index.as_ref().and_then(|index| index.entry(key)).map_or(key.as_str(), |entry| entry.names.get(lang));
            pane = pane.push(line(format!("{name}: {} {added}", l.text("toegevoegd aan het project:", "added to the project:"))));
        }
        if let Some(entry) = view.picked.as_deref().and_then(|key| view.index.as_ref()?.entry(key)) {
            let take = button(text(l.text("Aan het project toevoegen", "Add to the project")).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s));
            let take = if view.busy { take } else { take.on_press(Message::Online(OnlineMessage::Take(entry.key.clone()))) };
            pane = pane
                .push(text(entry.names.get(lang).to_string()).size(13).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word))
                .push(line(entry.about.clone()))
                .push(line(holds(entry, l)))
                .push(line(levels(entry)))
                .push(line(entry.source.clone()))
                .push(row![take, Space::new().width(Fill)]);
        }
        container(pane).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::object::{ObjectGroup, ObjectType};
    use bim_library::{Library, Names};

    /// A place with one library of two objects: its index and its file, as they would be fetched.
    fn place() -> (String, String) {
        let mut library = Library { codes: bim_library::nlsfb::starter(), ..Library::default() };
        for (id, name) in [("nl.luminaire.led-panel-595-595", "LED panel 595 × 595"), ("nl.luminaire.recessed-spot-85", "recessed spot Ø85")] {
            let mut object = ObjectType::new(id, ObjectGroup::Luminaire, Names::same(name), [0.595, 0.595, 0.04]);
            object.lod = Lod::L300;
            library.add_object_type(object).unwrap();
        }
        let entry = bim_library::remote::entry(&library, "nl-objects-electrical", "ifcx/nl-objects-electrical.ifcx", Names::new("objecten elektra", "electrical objects", "x", "y"), "Luminaires.", "test");
        let index = Index { name: "Test library".into(), libraries: vec![entry] };
        (serde_json::to_string(&index).unwrap(), serde_json::to_string(&bim_library::ifcx::file(&library, "x.ifcx", "test", "2026-11-19T00:00:00Z")).unwrap())
    }

    #[test]
    fn a_library_of_the_index_is_added_to_the_project() {
        let (index, file) = place();
        let mut app = App::default();
        assert_eq!(app.online_tiles(app.theme.palette(), Language::English).len(), 1, "before the index: the tile that fetches it");
        // the index came back: its library shows, with what it holds
        let _ = app.online_update(OnlineMessage::Index(Ok(index)));
        let entry = app.online.index.as_ref().unwrap().entry("nl-objects-electrical").unwrap().clone();
        assert_eq!((holds(&entry, Language::English), levels(&entry)), ("objects 2".to_string(), "LOD 300: 2".to_string()));
        assert_eq!(app.online_tiles(app.theme.palette(), Language::English).len(), 2);
        let _ = app.online_update(OnlineMessage::Pick("nl-objects-electrical".into()));
        // its file came back: what the project lacks is added, once
        let before = app.library.object_types.len();
        let _ = app.online_update(OnlineMessage::Library("nl-objects-electrical".into(), Ok(file.clone())));
        assert_eq!((app.library.object_types.len(), app.online.added.clone()), (before + 2, Some(("nl-objects-electrical".to_string(), 2))));
        let _ = app.online_update(OnlineMessage::Library("nl-objects-electrical".into(), Ok(file)));
        assert_eq!((app.library.object_types.len(), app.online.added.clone().map(|a| a.1)), (before + 2, Some(0)));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 700.0), app.online_pane(app.theme.palette(), Language::English));
        assert!(pane.find("Test library").is_ok() && pane.find("electrical objects").is_ok() && pane.find("LOD 300: 2").is_ok() && pane.find("Add to the project").is_ok());
    }

    #[test]
    fn what_does_not_come_back_or_is_no_library_is_said() {
        let mut app = App::default();
        let _ = app.online_update(OnlineMessage::Index(Err("no network".into())));
        assert_eq!((app.online.index.is_none(), app.online.failed.as_deref(), app.online.busy), (true, Some("no network"), false));
        let _ = app.online_update(OnlineMessage::Index(Ok("{\"libraries\": [{\"key\": \"a\", \"file\": \"../a.ifcx\", \"names\": {\"nl\": \"a\", \"en\": \"a\", \"de\": \"a\", \"fr\": \"a\"}, \"counts\": {}}]}".into())));
        assert!(app.online.index.is_none() && app.online.failed.is_some(), "an index that names a file outside its place is refused");
        let objects = app.library.object_types.len();
        let _ = app.online_update(OnlineMessage::Library("a".into(), Ok("not a library".into())));
        assert_eq!((app.library.object_types.len(), app.online.failed.is_some()), (objects, true));
        // a library the index does not have is not fetched
        let _ = app.online_update(OnlineMessage::Take("none".into()));
        assert!(!app.online.busy);
        assert_eq!(address("https://example.org/lib/", "ifcx/a.ifcx"), "https://example.org/lib/ifcx/a.ifcx");
        assert_eq!(fetch("http://example.org/index.json"), Err("only https addresses are fetched".into()));
    }
}
