//! RuboCop's `Cop::Corrector` (lib/rubocop/cop/corrector.rb): a TreeRewriter that raises on
//! different replacements and swallowed insertions, accepts crossing deletions, plus helpers.
//!
//! All positions are byte offsets into the file (`cx.src.text`); RuboCop's are character
//! offsets, so sizes passed to `remove_preceding` and friends are byte counts here.

use super::tree_rewriter::{Clobbering, Policies, Policy, TreeRewriter};
use std::sync::Arc;

/// The `corrector` a cop's `add_offense` block receives. Edits never fail at the call site: the
/// first clobbering edit (RuboCop raises `ClobberingError` out of the block) poisons the corrector,
/// and the offense that owns it is then dropped with an error, as RuboCop does.
#[derive(Clone, Debug)]
pub struct Corrector {
    rw: TreeRewriter,
    clobbered: bool,
}

impl Corrector {
    pub fn new(source: Arc<str>) -> Self {
        let policies = Policies {
            different_replacements: Policy::Raise,
            swallowed_insertions: Policy::Raise,
            crossing_deletions: Policy::Accept,
        };
        Corrector { rw: TreeRewriter::new(source, policies), clobbered: false }
    }

    /// The source text the corrector edits.
    pub fn source(&self) -> &str {
        self.rw.source()
    }

    pub fn is_empty(&self) -> bool {
        self.rw.is_empty()
    }

    /// TreeRewriter#as_replacements: `(start, end, replacement)` in application order.
    pub fn as_replacements(&self) -> Vec<(usize, usize, String)> {
        self.rw.as_replacements()
    }

    /// Whether an edit clobbered an earlier one (the block raised in RuboCop).
    pub fn clobbered(&self) -> bool {
        self.clobbered
    }

    fn apply(&mut self, edit: impl FnOnce(&mut TreeRewriter) -> Result<(), Clobbering>) {
        if !self.clobbered && edit(&mut self.rw).is_err() {
            self.clobbered = true;
        }
    }

    pub fn replace(&mut self, start: usize, end: usize, content: &str) {
        self.apply(|r| r.replace(start, end, content));
    }

    pub fn remove(&mut self, start: usize, end: usize) {
        self.apply(|r| r.remove(start, end));
    }

    pub fn wrap(&mut self, start: usize, end: usize, before: &str, after: &str) {
        self.apply(|r| r.wrap(start, end, before, after));
    }

    /// Insert before `start`; the whole range still decides how the edit nests with others.
    pub fn insert_before(&mut self, start: usize, end: usize, content: &str) {
        self.apply(|r| r.insert_before(start, end, content));
    }

    /// Insert after `end`; the whole range still decides how the edit nests with others.
    pub fn insert_after(&mut self, start: usize, end: usize, content: &str) {
        self.apply(|r| r.insert_after(start, end, content));
    }

    /// Remove `size` bytes before the range.
    pub fn remove_preceding(&mut self, start: usize, _end: usize, size: usize) {
        self.remove(start - size, start);
    }

    /// Remove the first `size` bytes of the range.
    pub fn remove_leading(&mut self, start: usize, _end: usize, size: usize) {
        self.remove(start, start + size);
    }

    /// Remove the last `size` bytes of the range.
    pub fn remove_trailing(&mut self, _start: usize, end: usize, size: usize) {
        self.remove(end - size, end);
    }

    /// Swap the source of two ranges.
    pub fn swap(&mut self, a: (usize, usize), b: (usize, usize)) {
        let src = self.rw.source().clone();
        if a.1 == b.0 {
            self.insert_before(a.0, a.1, &src[b.0..b.1]);
            self.remove(b.0, b.1);
        } else if b.1 == a.0 {
            self.insert_before(b.0, b.1, &src[a.0..a.1]);
            self.remove(a.0, a.1);
        } else {
            self.replace(a.0, a.1, &src[b.0..b.1]);
            self.replace(b.0, b.1, &src[a.0..a.1]);
        }
    }

    /// `merge!`: on clobbering `self` is unchanged.
    pub fn merge(&mut self, other: &Corrector) -> Result<(), Clobbering> {
        self.rw.merge(&other.rw)
    }

    /// The corrected source (`rewrite` / `process`).
    pub fn rewrite(&self) -> String {
        self.rw.process()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(src: &str) -> Corrector {
        Corrector::new(Arc::from(src))
    }

    #[test]
    fn helpers() {
        let mut k = c("foo(bar, baz)");
        k.swap((4, 7), (9, 12));
        assert_eq!(k.rewrite(), "foo(baz, bar)");
        let mut k = c("ab");
        k.swap((0, 1), (1, 2));
        assert_eq!(k.rewrite(), "ba");
        let mut k = c("x = 1  ");
        k.remove_trailing(0, 7, 2);
        k.remove_leading(0, 7, 1);
        k.remove_preceding(4, 5, 2);
        assert_eq!(k.rewrite(), " 1");
    }

    #[test]
    fn policies_and_poisoning() {
        // Corrector raises on different replacements and swallowed insertions ...
        let mut k = c("abcdef");
        k.replace(1, 2, "x");
        k.replace(1, 2, "y");
        assert!(k.clobbered());
        let mut k = c("abcdef");
        k.insert_before(2, 3, "!");
        k.replace(1, 4, "y");
        assert!(k.clobbered());
        // ... but accepts crossing deletions.
        let mut k = c("abcdef");
        k.remove(1, 3);
        k.remove(2, 4);
        assert!(!k.clobbered());
        assert_eq!(k.rewrite(), "aef");
    }
}
