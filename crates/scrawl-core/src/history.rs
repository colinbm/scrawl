//! Snapshot-based undo/redo.
//!
//! The editor mutates the document freely; whenever an interaction finishes it calls
//! [`History::commit`], which records an undo step if anything changed since the last commit.
//! This coalesces a whole drag (or a run of keystrokes in a field) into one step without every
//! edit site needing to know about undo.

use crate::Document;

pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// The document as of the last commit.
    baseline: Document,
    limit: usize,
}

impl History {
    pub fn new(doc: &Document) -> Self {
        Self { undo: vec![], redo: vec![], baseline: doc.clone(), limit: 200 }
    }

    /// Records an undo step if `doc` differs from the last commit. Returns whether it did.
    pub fn commit(&mut self, doc: &Document) -> bool {
        if *doc == self.baseline {
            return false;
        }
        let prev = std::mem::replace(&mut self.baseline, doc.clone());
        self.undo.push(prev);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }

    /// Whether `doc` has changes not yet committed.
    pub fn is_dirty(&self, doc: &Document) -> bool {
        *doc != self.baseline
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        self.commit(doc);
        let Some(prev) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.baseline, prev.clone()));
        *doc = prev;
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        self.commit(doc);
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.baseline, next.clone()));
        *doc = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Layer};

    #[test]
    fn coalesces_and_round_trips() {
        let mut doc = Document::default();
        let mut h = History::new(&doc);
        assert!(!h.commit(&doc));

        doc.pages[0].layers.push(Layer::rectangle(Frame::new(0.0, 0.0, 10.0, 10.0)));
        // Several edits before a commit become one step.
        doc.pages[0].layers[0].frame.x = 5.0;
        doc.pages[0].layers[0].frame.x = 7.0;
        assert!(h.commit(&doc));
        let edited = doc.clone();

        assert!(h.undo(&mut doc));
        assert!(doc.pages[0].layers.is_empty());
        assert!(!h.undo(&mut doc));
        assert!(h.redo(&mut doc));
        assert_eq!(doc, edited);

        // A new edit clears redo.
        h.undo(&mut doc);
        doc.pages[0].name = "Renamed".into();
        h.commit(&doc);
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_commits_pending_changes_first() {
        let mut doc = Document::default();
        let mut h = History::new(&doc);
        doc.pages[0].name = "Pending".into();
        assert!(h.undo(&mut doc));
        assert_eq!(doc.pages[0].name, "Page 1");
        assert!(h.redo(&mut doc));
        assert_eq!(doc.pages[0].name, "Pending");
    }
}
