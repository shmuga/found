//! Snapshot undo. The app observes its document each frame; when the content key changes and
//! the user is not mid-gesture, the previous snapshot is committed. One drag = one undo step.

pub struct History<T: Clone> {
    past: Vec<T>,
    future: Vec<T>,
    committed: T,
    committed_key: String,
    limit: usize,
}

impl<T: Clone> History<T> {
    pub fn new(initial: T, key: String) -> Self {
        Self {
            past: vec![],
            future: vec![],
            committed: initial,
            committed_key: key,
            limit: 200,
        }
    }

    /// Returns true when a new state was committed.
    pub fn observe(&mut self, cur: &T, key: String, interacting: bool) -> bool {
        if interacting || key == self.committed_key {
            return false;
        }
        let prev = std::mem::replace(&mut self.committed, cur.clone());
        self.past.push(prev);
        if self.past.len() > self.limit {
            self.past.remove(0);
        }
        self.future.clear();
        self.committed_key = key;
        true
    }

    pub fn undo(&mut self, key_of: impl Fn(&T) -> String) -> Option<T> {
        let prev = self.past.pop()?;
        let cur = std::mem::replace(&mut self.committed, prev.clone());
        self.future.push(cur);
        self.committed_key = key_of(&prev);
        Some(prev)
    }

    pub fn redo(&mut self, key_of: impl Fn(&T) -> String) -> Option<T> {
        let next = self.future.pop()?;
        let cur = std::mem::replace(&mut self.committed, next.clone());
        self.past.push(cur);
        self.committed_key = key_of(&next);
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}
