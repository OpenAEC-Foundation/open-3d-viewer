//! Undo and redo: the states before each change, and the states undone since.

/// Snapshots to go back to, without a limit.
#[derive(Debug, Clone)]
pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> History<T> {
    pub fn new() -> Self {
        Self { undo: Vec::new(), redo: Vec::new() }
    }

    /// The state before a change; clears redo.
    pub fn record(&mut self, before: T) {
        self.undo.push(before);
        self.redo.clear();
    }

    /// The state to restore, given the current one (which goes to redo).
    pub fn undo(&mut self, current: T) -> Option<T> {
        let state = self.undo.pop()?;
        self.redo.push(current);
        Some(state)
    }

    /// The state undone last, given the current one (which goes back to undo).
    pub fn redo(&mut self, current: T) -> Option<T> {
        let state = self.redo.pop()?;
        self.undo.push(current);
        Some(state)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_then_redo_round_trips() {
        let mut h = History::new();
        h.record(1);
        assert_eq!(h.undo(2), Some(1));
        assert_eq!(h.redo(1), Some(2));
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn record_clears_redo() {
        let mut h = History::new();
        h.record(1);
        assert_eq!(h.undo(2), Some(1));
        h.record(1);
        assert!(!h.can_redo());
        assert_eq!(h.redo(3), None);
    }
}
