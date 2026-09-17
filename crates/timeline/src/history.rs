/// How many versions back undo reaches.
const DEPTH: usize = 200;

/// Undo by keeping whole versions rather than inverses of the edits: a
/// document is behind an `Arc`, so a version costs one clip list and going
/// back is a pointer swap. Nothing here reads what it holds.
pub struct History<T> {
    past: Vec<T>,
    future: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        History {
            past: Vec::new(),
            future: Vec::new(),
        }
    }
}

impl<T> History<T> {
    /// The version as it was *before* the edit about to be published.
    pub fn record(&mut self, previous: T) {
        self.past.push(previous);

        if self.past.len() > DEPTH {
            self.past.remove(0);
        }

        // A fresh edit is a fresh branch: what was undone is no longer ahead.
        self.future.clear();
    }

    /// Fold the next edit into the version already recorded, for a gesture
    /// that arrives as a run — a held arrow key. False means record instead.
    pub fn amend(&mut self) -> bool {
        if self.past.is_empty() {
            return false;
        }

        // Still a fresh branch: the document is changing either way.
        self.future.clear();
        true
    }

    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.past.pop()?;
        self.future.push(current);

        Some(previous)
    }

    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.future.pop()?;
        self.past.push(current);

        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::Timeline;

    type Versions = History<Arc<Timeline>>;

    /// Versions are told apart by identity, which is what undo trades in.
    fn version() -> Arc<Timeline> {
        Arc::new(Timeline::default())
    }

    #[test]
    fn walks_back_and_forward_through_versions() {
        let (a, b, c) = (version(), version(), version());
        let mut history = Versions::default();

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
    fn an_amended_run_is_one_step_back() {
        let (a, b, c) = (version(), version(), version());
        let mut history = Versions::default();

        // One keydown, then two repeats of it: the repeats amend.
        history.record(a.clone());
        assert!(history.amend());
        assert!(history.amend());

        assert!(Arc::ptr_eq(&history.undo(c).unwrap(), &a));
        assert!(history.undo(b).is_none());
    }

    #[test]
    fn nothing_recorded_yet_cannot_be_amended() {
        assert!(!Versions::default().amend());
    }

    #[test]
    fn a_fresh_edit_drops_what_was_undone() {
        let (a, b, c) = (version(), version(), version());
        let mut history = Versions::default();

        history.record(a);
        history.undo(b.clone()).unwrap();
        history.record(b);

        assert!(history.redo(c).is_none());
    }

    #[test]
    fn only_reaches_back_so_far() {
        let mut history = Versions::default();
        for _ in 0..DEPTH + 50 {
            history.record(version());
        }

        for _ in 0..DEPTH {
            assert!(history.undo(version()).is_some());
        }
        assert!(history.undo(version()).is_none());
    }
}
