/// How many versions back undo reaches.
const DEPTH: usize = 200;

/// Undo, by keeping the versions themselves rather than inverses of the edits.
///
/// A document already lives behind an `Arc` and an edit clones it, so a version
/// costs one clip list — every source is shared — and going back is a pointer
/// swap. It also means undo cannot disagree with what an edit actually did.
///
/// What a version *is* belongs to whoever is keeping the history: the document
/// alone here, the document and where the user was looking at it in the app.
/// Nothing below reads the thing it is holding.
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
    /// Record the version as it was *before* the edit about to be published.
    pub fn record(&mut self, previous: T) {
        self.past.push(previous);

        if self.past.len() > DEPTH {
            self.past.remove(0);
        }

        // A fresh edit is a fresh branch: what was undone is no longer ahead.
        self.future.clear();
    }

    /// Fold what is about to be published into the version already recorded,
    /// rather than stacking another one.
    ///
    /// For a gesture that arrives as a run of edits — a held arrow key, where
    /// every repeat moves the clips again — so that undo takes back the whole
    /// hold rather than one keyboard repeat of it. False when there is
    /// nothing recorded to fold into, which is the caller's cue to record.
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
    use crate::project::Timeline;

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
        // The caller records instead, so a run that starts before anything
        // else has happened still has somewhere to go back to.
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
