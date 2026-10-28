//! Saving and closing documents (issue #10, findings UI-016 to UI-018).
//!
//! Three promises, kept apart:
//! - **Opslaan** writes the main file. A document only shows as saved when the
//!   revision on screen is the revision that was written: an edit made while a
//!   save runs keeps the document unsaved.
//! - **Automatisch opslaan** is the same write, started by the app. It needs a
//!   file name, so a new document without one gets a recovery copy instead.
//! - **Herstelkopie** goes to a separate folder ([`recovery_dir`]) and never
//!   overwrites the main file or makes the document count as saved. After a
//!   crash the app offers it as a copy next to the main file.
//!
//! Each document writes one save at a time, so writes reach the disk in the
//! order they were asked. Closing goes through a [`ClosePlan`]: one choice per
//! changed document, and nothing closes until every choice is carried out.
//! The app does the actual writing; this module only keeps the bookkeeping.
use super::widgets::{self, ButtonKind, Density};
use super::Look;
use iced::Element;
use std::path::PathBuf;

/// Which promise a write keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// The person saved: the main file.
    Save,
    /// The app saved the main file on its own.
    Autosave,
    /// A recovery copy in [`recovery_dir`]; the main file stays as it was.
    Recovery,
}

/// One write the app must carry out. Serialize the document as it is when the
/// ticket is handed out, and report back with [`DocumentState::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveTicket {
    pub revision: u64,
    pub kind: SaveKind,
}

/// The answer to [`DocumentState::request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Write now with this ticket.
    Start(SaveTicket),
    /// A write of this document is running; the newer revision follows when it
    /// finishes ([`DocumentState::finish`] returns its ticket).
    Queued,
    /// Nothing to write: the revision on screen is already written.
    NotNeeded,
    /// The document has no file yet: ask for one (Opslaan als) and call
    /// [`DocumentState::set_path`], or make a recovery copy for an autosave.
    NeedsPath,
}

/// What the status bar and the tab show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Saved,
    Unsaved,
    Saving,
    Failed(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::Saved => "Opgeslagen".into(),
            Status::Unsaved => "Niet opgeslagen wijzigingen".into(),
            Status::Saving => "Opslaan…".into(),
            Status::Failed(reason) => format!("Opslaan mislukt: {reason}"),
        }
    }
}

/// The save bookkeeping of one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentState {
    revision: u64,
    /// The revision the main file holds.
    written: u64,
    recovered: Option<u64>,
    has_path: bool,
    writing: Option<SaveTicket>,
    recovering: bool,
    queued: bool,
    error: Option<String>,
}

impl DocumentState {
    /// A new document without a file name. Unchanged, so closing it asks nothing.
    pub fn new_file() -> Self {
        Self::default()
    }

    /// A document opened from its file.
    pub fn opened() -> Self {
        Self { has_path: true, ..Self::default() }
    }

    /// Record one change. Call it for every change the person can see, undo
    /// and redo included.
    pub fn edit(&mut self) {
        self.revision += 1;
    }

    /// The document got a file name (Opslaan als).
    pub fn set_path(&mut self) {
        self.has_path = true;
    }

    pub fn has_path(&self) -> bool {
        self.has_path
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the revision on screen differs from the main file.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.written
    }

    pub fn is_saving(&self) -> bool {
        self.writing.is_some()
    }

    /// The write of the main file that is running, if any.
    pub fn writing(&self) -> Option<SaveTicket> {
        self.writing
    }

    pub fn status(&self) -> Status {
        match (&self.writing, &self.error) {
            (Some(_), _) => Status::Saving,
            (None, Some(reason)) => Status::Failed(reason.clone()),
            (None, None) if self.is_dirty() => Status::Unsaved,
            (None, None) => Status::Saved,
        }
    }

    /// Whether a recovery copy of the current revision is still missing.
    pub fn needs_recovery_copy(&self) -> bool {
        self.is_dirty() && !self.recovering && self.recovered != Some(self.revision)
    }

    /// Ask for a write.
    pub fn request(&mut self, kind: SaveKind) -> Request {
        if kind == SaveKind::Recovery {
            if !self.needs_recovery_copy() {
                return Request::NotNeeded;
            }
            self.recovering = true;
            return Request::Start(SaveTicket { revision: self.revision, kind });
        }
        if !self.has_path {
            return Request::NeedsPath;
        }
        if self.writing.is_some() {
            // One write at a time; the newer revision follows. An autosave
            // never queues: the next interval tries again.
            if kind == SaveKind::Save && self.revision != self.writing.map_or(0, |t| t.revision) {
                self.queued = true;
                return Request::Queued;
            }
            return Request::NotNeeded;
        }
        if !self.is_dirty() && kind == SaveKind::Autosave {
            return Request::NotNeeded;
        }
        let ticket = SaveTicket { revision: self.revision, kind };
        self.writing = Some(ticket);
        Request::Start(ticket)
    }

    /// Report a finished write. Returns the next ticket when a newer revision
    /// was saved while this one was written.
    pub fn finish(&mut self, ticket: SaveTicket, result: Result<(), String>) -> Option<SaveTicket> {
        if ticket.kind == SaveKind::Recovery {
            self.recovering = false;
            if result.is_ok() {
                self.recovered = Some(ticket.revision);
            }
            return None;
        }
        if self.writing != Some(ticket) {
            return None;
        }
        self.writing = None;
        match result {
            Ok(()) => {
                // Only the written revision counts; an edit made meanwhile stays unsaved.
                self.written = self.written.max(ticket.revision);
                self.error = None;
            }
            Err(reason) => {
                self.error = Some(reason);
                self.queued = false;
                return None;
            }
        }
        if std::mem::take(&mut self.queued) && self.is_dirty() {
            let next = SaveTicket { revision: self.revision, kind: SaveKind::Save };
            self.writing = Some(next);
            return Some(next);
        }
        None
    }
}

/// The person's choice for one changed document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Save,
    Discard,
}

/// What a [`ClosePlan`] needs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<Id> {
    /// Ask about this document; `number` of `of` changed documents.
    Ask { id: Id, number: usize, of: usize },
    /// Saves are running; wait for [`ClosePlan::saved`].
    Wait,
    /// Every choice is carried out: close these documents (or the window).
    Close(Vec<Id>),
}

/// Closing one or more documents, or the whole window.
///
/// Build it with every document that closes and whether it has changes. The
/// plan asks about each changed document in turn; the app saves the ones the
/// person keeps and reports each result. A failed or cancelled save stops the
/// plan: drop it and nothing closes, so no document is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosePlan<Id> {
    closing: Vec<Id>,
    undecided: Vec<Id>,
    saving: Vec<Id>,
    changed: usize,
}

impl<Id: Copy + PartialEq> ClosePlan<Id> {
    pub fn new(documents: impl IntoIterator<Item = (Id, bool)>) -> Self {
        let (mut closing, mut undecided) = (Vec::new(), Vec::new());
        for (id, dirty) in documents {
            closing.push(id);
            if dirty {
                undecided.push(id);
            }
        }
        let changed = undecided.len();
        Self { closing, undecided, saving: Vec::new(), changed }
    }

    pub fn step(&self) -> Step<Id> {
        if let Some(&id) = self.undecided.first() {
            Step::Ask { id, number: self.changed - self.undecided.len() + 1, of: self.changed }
        } else if !self.saving.is_empty() {
            Step::Wait
        } else {
            Step::Close(self.closing.clone())
        }
    }

    /// Record the choice for the document being asked about. Returns the
    /// documents the app must save now.
    pub fn choose(&mut self, choice: Choice) -> Vec<Id> {
        let Some(id) = (!self.undecided.is_empty()).then(|| self.undecided.remove(0)) else { return Vec::new() };
        self.carry_out(id, choice)
    }

    /// The same choice for every document not asked about yet.
    pub fn choose_all(&mut self, choice: Choice) -> Vec<Id> {
        std::mem::take(&mut self.undecided).into_iter().flat_map(|id| self.carry_out(id, choice)).collect()
    }

    fn carry_out(&mut self, id: Id, choice: Choice) -> Vec<Id> {
        match choice {
            Choice::Save => {
                self.saving.push(id);
                vec![id]
            }
            Choice::Discard => Vec::new(),
        }
    }

    /// Report a save the plan asked for. `false` (a write error, or Opslaan als
    /// cancelled) means the plan has failed: drop it, nothing closes.
    #[must_use]
    pub fn saved(&mut self, id: Id, ok: bool) -> bool {
        self.saving.retain(|s| *s != id);
        ok
    }

    pub fn is_waiting_for(&self, id: Id) -> bool {
        self.saving.contains(&id)
    }
}

/// The close question for one changed document, with the same wording in
/// every app. `number` of `of` shows when more than one document has changes,
/// and then also offers to save all of them.
#[allow(clippy::too_many_arguments)]
pub fn close_dialog<'a, M: Clone + 'a>(
    look: Look,
    name: &str,
    number: usize,
    of: usize,
    close_icon: &'static str,
    on_cancel: M,
    on_discard: M,
    on_save: M,
    on_save_all: M,
) -> Element<'a, M> {
    let body = if of > 1 {
        format!("Er zijn wijzigingen in {name} die nog niet zijn opgeslagen. Document {number} van {of} met wijzigingen.")
    } else {
        format!("Er zijn wijzigingen in {name} die nog niet zijn opgeslagen.")
    };
    let mut actions = vec![widgets::button(look, "Annuleren", ButtonKind::Secondary, Density::Regular, None, Some(on_cancel.clone()))];
    if of > 1 && number < of {
        actions.push(widgets::button(look, "Alles opslaan", ButtonKind::Secondary, Density::Regular, None, Some(on_save_all)));
    }
    actions.push(widgets::button(look, "Opslaan", ButtonKind::Primary, Density::Regular, None, Some(on_save)));
    widgets::dialog(
        look,
        "Wijzigingen opslaan?",
        close_icon,
        Some(on_cancel),
        widgets::secondary(look, body).into(),
        Some(widgets::button(look, "Niet opslaan", ButtonKind::Quiet, Density::Regular, None, Some(on_discard))),
        actions,
        false,
    )
}

/// Folder for recovery copies of one app: `%LOCALAPPDATA%\OpenAEC\<app>\herstel`
/// on Windows. Local, not roaming: a copy belongs to this computer.
pub fn recovery_dir(app: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("OpenAEC"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/OpenAEC"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .map(|d| d.join("openaec"))
    };
    base.map(|b| b.join(app).join("herstel"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(state: &mut DocumentState, kind: SaveKind) -> SaveTicket {
        match state.request(kind) {
            Request::Start(ticket) => ticket,
            other => panic!("expected a write, got {other:?}"),
        }
    }

    #[test]
    fn an_edit_during_a_save_keeps_the_document_unsaved() {
        // UI-016: Calc marked the document clean after any save, even when
        // version 2 was on screen and only version 1 was written.
        let mut doc = DocumentState::opened();
        doc.edit();
        let first = start(&mut doc, SaveKind::Save);
        assert_eq!(doc.status(), Status::Saving);
        doc.edit();
        assert_eq!(doc.finish(first, Ok(())), None);
        assert!(doc.is_dirty());
        assert_eq!(doc.status(), Status::Unsaved);
        let second = start(&mut doc, SaveKind::Save);
        assert_eq!(doc.finish(second, Ok(())), None);
        assert_eq!(doc.status(), Status::Saved);
    }

    #[test]
    fn writes_follow_each_other_in_order() {
        let mut doc = DocumentState::opened();
        doc.edit();
        let first = start(&mut doc, SaveKind::Save);
        doc.edit();
        assert_eq!(doc.request(SaveKind::Save), Request::Queued);
        assert_eq!(doc.request(SaveKind::Autosave), Request::NotNeeded);
        // The queued revision starts only after the first write finished.
        let next = doc.finish(first, Ok(())).expect("the newer revision follows");
        assert_eq!(next.revision, 2);
        assert!(doc.is_saving());
        // A late report for the old ticket changes nothing.
        assert_eq!(doc.finish(first, Ok(())), None);
        assert_eq!(doc.finish(next, Ok(())), None);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn a_failed_write_keeps_the_changes_and_says_why() {
        let mut doc = DocumentState::opened();
        doc.edit();
        let ticket = start(&mut doc, SaveKind::Save);
        doc.finish(ticket, Err("De schijf is vol.".into()));
        assert!(doc.is_dirty());
        assert_eq!(doc.status().label(), "Opslaan mislukt: De schijf is vol.");
        let again = start(&mut doc, SaveKind::Save);
        doc.finish(again, Ok(()));
        assert_eq!(doc.status(), Status::Saved);
    }

    #[test]
    fn a_new_document_needs_a_name_and_gets_a_recovery_copy_meanwhile() {
        // UI-018: autosave skipped new documents without a path.
        let mut doc = DocumentState::new_file();
        assert!(!doc.is_dirty(), "an untouched new document closes without asking");
        doc.edit();
        assert_eq!(doc.request(SaveKind::Save), Request::NeedsPath);
        assert_eq!(doc.request(SaveKind::Autosave), Request::NeedsPath);
        let copy = start(&mut doc, SaveKind::Recovery);
        assert_eq!(doc.request(SaveKind::Recovery), Request::NotNeeded, "one copy at a time");
        doc.finish(copy, Ok(()));
        assert!(doc.is_dirty(), "a recovery copy is not a save");
        assert!(!doc.needs_recovery_copy());
        doc.edit();
        assert!(doc.needs_recovery_copy());
        doc.set_path();
        let save = start(&mut doc, SaveKind::Save);
        doc.finish(save, Ok(()));
        assert!(!doc.is_dirty() && !doc.needs_recovery_copy());
    }

    #[test]
    fn closing_asks_about_every_changed_document() {
        // UI-017: Calc asked about the first changed document only.
        let mut plan = ClosePlan::new([("Begroting", true), ("Rapport", false), ("Hoeveelheden", true)]);
        assert_eq!(plan.step(), Step::Ask { id: "Begroting", number: 1, of: 2 });
        assert_eq!(plan.choose(Choice::Save), vec!["Begroting"]);
        assert_eq!(plan.step(), Step::Ask { id: "Hoeveelheden", number: 2, of: 2 });
        assert!(plan.choose(Choice::Discard).is_empty());
        assert_eq!(plan.step(), Step::Wait, "nothing closes while the save runs");
        assert!(plan.saved("Begroting", true));
        assert_eq!(plan.step(), Step::Close(vec!["Begroting", "Rapport", "Hoeveelheden"]));
    }

    #[test]
    fn a_failed_save_stops_the_plan() {
        let mut plan = ClosePlan::new([(1, true), (2, true)]);
        assert_eq!(plan.choose_all(Choice::Save), vec![1, 2]);
        assert!(!plan.saved(1, false), "the app drops the plan; both documents stay open");
        let unchanged = ClosePlan::new([(1, false)]);
        assert_eq!(unchanged.step(), Step::Close(vec![1]));
    }
}
