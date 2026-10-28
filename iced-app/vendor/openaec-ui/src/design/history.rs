//! Undo and redo (issue #34, finding UI-023).
//!
//! The rules every app follows:
//! - **One step per task.** What the person sees as one action is one step:
//!   pasting a range, a bulk action, a drag ([`History::begin`] and
//!   [`History::end`]), and typing in one field ([`History::record_merged`]).
//! - **Per document.** Each document has its own `History`; Ongedaan maken works
//!   on the active document. Views of the same data (a budget and its quantity
//!   list) share the document's history.
//! - **Text fields.** iced 0.14 text fields have no undo of their own. Typing in
//!   one field is one step, so Ctrl+Z in a field first takes back that field's
//!   edit as a whole, and the next Ctrl+Z the step before it.
//! - **Named steps.** Every step has a name ("Prijs wijzigen"); the menu and the
//!   tooltip say what Ongedaan maken and Opnieuw will do ([`History::undo_label`]).
//! - **Not undoable.** Actions outside the document, such as exporting, printing
//!   or sending, are not recorded. Their dialog says so ([`IRREVERSIBLE`]).
//! - **Not a crash recovery.** Recovery copies are in `design::document`.
//!
//! The history keeps whole snapshots of the document state `S`: before each step
//! the app passes the state as it was, and undo hands that state back.

/// The sentence for a dialog of an action that cannot be undone.
pub const IRREVERSIBLE: &str = "Dit kan niet ongedaan worden gemaakt.";

/// How many steps a history keeps by default.
pub const DEFAULT_LIMIT: usize = 200;

#[derive(Debug, Clone)]
struct Entry<S> {
    label: String,
    /// The state on the other side of the step: before it for undo, after it for redo.
    state: S,
}

/// The undo and redo history of one document.
#[derive(Debug, Clone)]
pub struct History<S> {
    done: Vec<Entry<S>>,
    undone: Vec<Entry<S>>,
    /// A task in progress (a drag, a bulk action) and how deep `begin` is nested.
    task: Option<(Entry<S>, usize)>,
    /// The field the last step came from, while typing in it continues.
    merging: Option<String>,
    limit: usize,
}

impl<S> Default for History<S> {
    fn default() -> Self {
        Self { done: Vec::new(), undone: Vec::new(), task: None, merging: None, limit: DEFAULT_LIMIT }
    }
}

impl<S> History<S> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limit(limit: usize) -> Self {
        Self { limit: limit.max(1), ..Self::default() }
    }

    /// Record one step. `before` is the state just before the change.
    /// Inside a task the task's own step covers it.
    pub fn record(&mut self, label: impl Into<String>, before: S) {
        if self.task.is_some() {
            return;
        }
        self.merging = None;
        self.push(Entry { label: label.into(), state: before });
    }

    /// Record a step that continues the previous one while `key` stays the same,
    /// such as typing in one field: only the first change of the run is kept.
    /// [`History::seal`] or any other step ends the run.
    pub fn record_merged(&mut self, label: impl Into<String>, key: impl Into<String>, before: S) {
        if self.task.is_some() {
            return;
        }
        let key = key.into();
        if self.merging.as_ref() == Some(&key) && !self.done.is_empty() {
            return;
        }
        self.push(Entry { label: label.into(), state: before });
        self.merging = Some(key);
    }

    /// End a run of [`History::record_merged`], for example when the field loses
    /// the focus or another row is selected.
    pub fn seal(&mut self) {
        self.merging = None;
    }

    /// Start a task that becomes one step, such as a drag. Nested calls belong to
    /// the outer task.
    pub fn begin(&mut self, label: impl Into<String>, before: S) {
        match &mut self.task {
            Some((_, depth)) => *depth += 1,
            None => self.task = Some((Entry { label: label.into(), state: before }, 1)),
        }
        self.merging = None;
    }

    /// End the task as one step. `changed: false` (a drag back to where it
    /// started) records nothing.
    pub fn end(&mut self, changed: bool) {
        let Some((entry, depth)) = self.task.take() else { return };
        if depth > 1 {
            self.task = Some((entry, depth - 1));
            return;
        }
        if changed {
            self.push(entry);
        }
    }

    /// Stop the task without a step and get the state from before it, to put back
    /// (Escape during a drag).
    pub fn cancel(&mut self) -> Option<S> {
        self.task.take().map(|(entry, _)| entry.state)
    }

    pub fn in_task(&self) -> bool {
        self.task.is_some()
    }

    fn push(&mut self, entry: Entry<S>) {
        self.done.push(entry);
        if self.done.len() > self.limit {
            self.done.remove(0);
        }
        self.undone.clear();
    }

    /// Take back the last step: returns the state to show, given the current one.
    /// Nothing happens during a task.
    pub fn undo(&mut self, current: S) -> Option<S> {
        if self.task.is_some() {
            return None;
        }
        self.merging = None;
        let entry = self.done.pop()?;
        self.undone.push(Entry { label: entry.label, state: current });
        Some(entry.state)
    }

    /// Do the last undone step again.
    pub fn redo(&mut self, current: S) -> Option<S> {
        if self.task.is_some() {
            return None;
        }
        self.merging = None;
        let entry = self.undone.pop()?;
        self.done.push(Entry { label: entry.label, state: current });
        Some(entry.state)
    }

    /// The name of the step Ongedaan maken takes back.
    pub fn undo_label(&self) -> Option<&str> {
        self.done.last().map(|e| e.label.as_str()).filter(|_| self.task.is_none())
    }

    /// The name of the step Opnieuw does again.
    pub fn redo_label(&self) -> Option<&str> {
        self.undone.last().map(|e| e.label.as_str()).filter(|_| self.task.is_none())
    }

    pub fn can_undo(&self) -> bool {
        self.undo_label().is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.redo_label().is_some()
    }

    /// Forget everything, for example when another file is opened.
    pub fn clear(&mut self) {
        *self = Self::with_limit(self.limit);
    }
}

/// "Ongedaan maken: Prijs wijzigen", or just "Ongedaan maken".
pub fn undo_text(history_label: Option<&str>) -> String {
    match history_label {
        Some(label) => format!("Ongedaan maken: {label}"),
        None => "Ongedaan maken".into(),
    }
}

/// "Opnieuw: Prijs wijzigen", or just "Opnieuw".
pub fn redo_text(history_label: Option<&str>) -> String {
    match history_label {
        Some(label) => format!("Opnieuw: {label}"),
        None => "Opnieuw".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_named_steps() {
        let mut h = History::new();
        let mut doc = 1;
        h.record("Post toevoegen", doc);
        doc = 2;
        h.record("Prijs wijzigen", doc);
        doc = 3;
        assert_eq!(undo_text(h.undo_label()), "Ongedaan maken: Prijs wijzigen");
        doc = h.undo(doc).unwrap();
        assert_eq!(doc, 2);
        assert_eq!(redo_text(h.redo_label()), "Opnieuw: Prijs wijzigen");
        doc = h.undo(doc).unwrap();
        assert_eq!(doc, 1);
        assert!(!h.can_undo());
        doc = h.redo(doc).unwrap();
        assert_eq!(doc, 2);
        h.record("Naam wijzigen", doc);
        assert!(!h.can_redo(), "a new step drops what was undone");
    }

    #[test]
    fn typing_in_one_field_is_one_step() {
        let mut h = History::new();
        let mut price = String::from("240");
        for typed in ["24", "245", "245,5"] {
            h.record_merged("Prijs wijzigen", "price:2", price.clone());
            price = typed.into();
        }
        h.seal();
        h.record_merged("Prijs wijzigen", "price:2", price.clone());
        price = "250".into();
        price = h.undo(price).unwrap();
        assert_eq!(price, "245,5", "a sealed run starts a new step");
        price = h.undo(price).unwrap();
        assert_eq!(price, "240", "the whole first run comes back in one step");
        assert!(!h.can_undo());
    }

    #[test]
    fn a_drag_or_bulk_action_is_one_step() {
        let mut h = History::new();
        let mut rows = vec![1, 2, 3];
        h.begin("Posten verplaatsen", rows.clone());
        for _ in 0..5 {
            h.record("ignored inside the task", rows.clone());
            rows.rotate_left(1);
        }
        h.begin("nested", rows.clone());
        h.end(true);
        assert!(h.in_task() && !h.can_undo(), "nothing to undo mid-drag");
        assert_eq!(h.undo(rows.clone()), None);
        h.end(true);
        assert_eq!(h.undo_label(), Some("Posten verplaatsen"));
        rows = h.undo(rows).unwrap();
        assert_eq!(rows, vec![1, 2, 3]);
        // A drag back to the start, or one cancelled with Escape, records nothing.
        h.begin("Posten verplaatsen", rows.clone());
        h.end(false);
        h.begin("Posten verplaatsen", rows.clone());
        assert_eq!(h.cancel(), Some(vec![1, 2, 3]));
        assert!(!h.can_undo());
    }

    #[test]
    fn the_limit_drops_the_oldest_steps() {
        let mut h = History::with_limit(2);
        for i in 0..4 {
            h.record(format!("Stap {i}"), i);
        }
        assert_eq!(h.undo(9), Some(3));
        assert_eq!(h.undo(3), Some(2));
        assert_eq!(h.undo(2), None);
    }
}
