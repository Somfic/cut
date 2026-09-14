use std::sync::Arc;

use crate::project::Timeline;

/// How many versions back undo reaches.
const DEPTH: usize = 200;

/// Undo, by keeping the versions themselves rather than inverses of the edits.
///
/// A document already lives behind an `Arc` and an edit clones it, so a version
/// costs one clip list — every source is shared — and going back is a pointer
/// swap. It also means undo cannot disagree with what an edit actually did.
#[derive(Default)]
pub struct History {
    past: Vec<Arc<Timeline>>,
    future: Vec<Arc<Timeline>>,
}

impl History {
    /// Record the document as it was *before* the edit about to be published.
    pub fn record(&mut self, previous: Arc<Timeline>) {
        self.past.push(previous);

        if self.past.len() > DEPTH {
            self.past.remove(0);
        }

        // A fresh edit is a fresh branch: what was undone is no longer ahead.
        self.future.clear();
    }

    pub fn undo(&mut self, current: Arc<Timeline>) -> Option<Arc<Timeline>> {
        let previous = self.past.pop()?;
        self.future.push(current);

        Some(previous)
    }

    pub fn redo(&mut self, current: Arc<Timeline>) -> Option<Arc<Timeline>> {
        let next = self.future.pop()?;
        self.past.push(current);

        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Versions are told apart by identity, which is what undo trades in.
    fn version() -> Arc<Timeline> {
        Arc::new(Timeline::default())
    }

    #[test]
    fn walks_back_and_forward_through_versions() {
        let (a, b, c) = (version(), version(), version());
        let mut history = History::default();

        history.record(a.clone());
        history.record(b.clone());

        assert!(Arc::ptr_eq(&history.undo(c.clone()).unwrap(), &b));
        assert!(Arc::ptr_eq(&history.undo(b.clone()).unwrap(), &a));
        assert!(history.undo(a.clone()).is_none());

        assert!(Arc::ptr_eq(&history.redo(a.clone()).unwrap(), &b));
        assert!(Arc::ptr_eq(&history.redo(b).unwrap(), &c));
        assert!(history.redo(c).is_none());
    }

    #[test]
    fn a_fresh_edit_drops_what_was_undone() {
        let (a, b, c) = (version(), version(), version());
        let mut history = History::default();

        history.record(a);
        history.undo(b.clone()).unwrap();
        history.record(b);

        assert!(history.redo(c).is_none());
    }

    #[test]
    fn only_reaches_back_so_far() {
        let mut history = History::default();
        for _ in 0..DEPTH + 50 {
            history.record(version());
        }

        for _ in 0..DEPTH {
            assert!(history.undo(version()).is_some());
        }
        assert!(history.undo(version()).is_none());
    }
}
