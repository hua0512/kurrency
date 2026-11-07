//! Undo and redo (DESIGN §2) over whole snapshots of the list and the
//! amount: amount commits, anchor changes, adds, removes and reorders.
//!
//! Plain Rust without GPUI.

use std::collections::VecDeque;

use rust_decimal::Decimal;

/// How many steps back undo reaches.
pub(crate) const LIMIT: usize = 50;

/// What undo restores: the currencies (anchor first) and the amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub currencies: Vec<String>,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Default)]
pub struct History {
    undo: VecDeque<Snapshot>,
    redo: Vec<Snapshot>,
}

impl History {
    /// Records the state from before a change; a new change clears redo.
    pub fn record(&mut self, before: Snapshot) {
        if self.undo.back() == Some(&before) {
            return;
        }
        if self.undo.len() == LIMIT {
            self.undo.pop_front();
        }
        self.undo.push_back(before);
        self.redo.clear();
    }

    /// The state to go back to from `current`, which redo can return to.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let previous = self.undo.pop_back()?;
        self.redo.push(current);
        Some(previous)
    }

    /// The state undone last, from `current`, which undo can return to.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push_back(current);
        if self.undo.len() > LIMIT {
            self.undo.pop_front();
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(codes: &[&str], amount: i64) -> Snapshot {
        Snapshot {
            currencies: codes.iter().map(ToString::to_string).collect(),
            amount: Some(Decimal::from(amount)),
        }
    }

    #[test]
    fn undo_then_redo() {
        let mut h = History::default();
        assert_eq!(h.undo(s(&["USD"], 1)), None);
        h.record(s(&["USD"], 1));
        h.record(s(&["USD"], 2));
        // Now at 3.
        assert_eq!(h.undo(s(&["USD"], 3)), Some(s(&["USD"], 2)));
        assert_eq!(h.undo(s(&["USD"], 2)), Some(s(&["USD"], 1)));
        assert_eq!(h.undo(s(&["USD"], 1)), None);
        assert_eq!(h.redo(s(&["USD"], 1)), Some(s(&["USD"], 2)));
        assert_eq!(h.redo(s(&["USD"], 2)), Some(s(&["USD"], 3)));
        assert_eq!(h.redo(s(&["USD"], 3)), None);
        assert_eq!(h.undo(s(&["USD"], 3)), Some(s(&["USD"], 2)));
    }

    #[test]
    fn a_new_change_clears_redo() {
        let mut h = History::default();
        h.record(s(&["USD", "EUR"], 1));
        assert_eq!(h.undo(s(&["USD"], 1)), Some(s(&["USD", "EUR"], 1)));
        h.record(s(&["USD", "EUR"], 1));
        assert_eq!(h.redo(s(&["EUR", "USD"], 1)), None);
    }

    #[test]
    fn bounded_to_the_limit() {
        let mut h = History::default();
        for i in 0..(LIMIT as i64 + 20) {
            h.record(s(&["USD"], i));
        }
        let mut current = s(&["USD"], 1000);
        let mut steps = 0;
        while let Some(previous) = h.undo(current.clone()) {
            current = previous;
            steps += 1;
        }
        assert_eq!(steps, LIMIT);
        // The oldest kept state.
        assert_eq!(current, s(&["USD"], 20));
    }

    #[test]
    fn repeated_states_record_once() {
        let mut h = History::default();
        h.record(s(&["USD"], 1));
        h.record(s(&["USD"], 1));
        assert_eq!(h.undo(s(&["USD"], 2)), Some(s(&["USD"], 1)));
        assert_eq!(h.undo(s(&["USD"], 1)), None);
    }
}
