//! Port of parser's `Parser::Source::TreeRewriter` and `TreeRewriter::Action`
//! (parser 3.3.12.0, lib/parser/source/tree_rewriter{,/action}.rb).
//!
//! Positions are byte offsets into the source (parser uses character offsets; the two orders agree).
//! Actions are immutable and shared (`Rc`) like the Ruby objects, so a failed combine leaves the
//! rewriter untouched, exactly as a raised `ClobberingError` does in Ruby.
//! Not ported: `import!`/`contract`/`moved` (only used for multi-buffer ruby extractors),
//! `transaction`, and the `:warn` policy (diagnostics are never shown by RuboCop's Corrector).

use std::rc::Rc;
use std::sync::Arc;

/// `Parser::ClobberingError`: a policy set to `:raise` was triggered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clobbering;

impl std::fmt::Display for Clobbering {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("Parser::Source::TreeRewriter detected clobbering")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Accept,
    Raise,
}

/// The three configurable policies. `crossing_insertions` is not configurable and always raises.
#[derive(Clone, Copy, Debug)]
pub struct Policies {
    pub crossing_deletions: Policy,
    pub different_replacements: Policy,
    pub swallowed_insertions: Policy,
}

impl Default for Policies {
    /// TreeRewriter.new's defaults: accept everything.
    fn default() -> Self {
        Policies {
            crossing_deletions: Policy::Accept,
            different_replacements: Policy::Accept,
            swallowed_insertions: Policy::Accept,
        }
    }
}

#[derive(Clone, Copy)]
enum Event {
    CrossingDeletions,
    CrossingInsertions,
    DifferentReplacements,
    SwallowedInsertions,
}

impl Policies {
    /// TreeRewriter#enforce_policy, called once the event's conflict values exist.
    fn enforce(&self, event: Event) -> Result<(), Clobbering> {
        let p = match event {
            Event::CrossingDeletions => self.crossing_deletions,
            Event::CrossingInsertions => Policy::Raise,
            Event::DifferentReplacements => self.different_replacements,
            Event::SwallowedInsertions => self.swallowed_insertions,
        };
        if p == Policy::Raise { Err(Clobbering) } else { Ok(()) }
    }
}

/// A half-open range; the root action spans one position beyond each end of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    begin: isize,
    end: isize,
}

impl Span {
    fn join(self, o: Span) -> Span {
        Span { begin: self.begin.min(o.begin), end: self.end.max(o.end) }
    }
    fn is_empty(self) -> bool {
        self.begin == self.end
    }
}

#[derive(Clone, Debug)]
struct Action {
    range: Span,
    insert_before: String,
    replacement: Option<String>,
    insert_after: String,
    children: Vec<Rc<Action>>,
}

struct Family {
    parent: Option<Rc<Action>>,
    left: Vec<Rc<Action>>,
    right: Vec<Rc<Action>>,
    fusible: Option<Vec<Rc<Action>>>,
    child: Option<Vec<Rc<Action>>>,
}

impl Action {
    fn leaf(range: Span, insert_before: String, replacement: Option<String>, insert_after: String) -> Action {
        Action { range, insert_before, replacement, insert_after, children: Vec::new() }
    }

    fn is_empty(&self) -> bool {
        self.insert_before.is_empty()
            && self.insert_after.is_empty()
            && self.children.is_empty()
            && self.replacement.as_ref().is_none_or(|r| r.is_empty() && self.range.is_empty())
    }

    fn is_insertion(&self) -> bool {
        !self.insert_before.is_empty()
            || !self.insert_after.is_empty()
            || self.replacement.as_ref().is_some_and(|r| !r.is_empty())
    }

    fn combine(self: &Rc<Self>, action: &Rc<Action>, p: &Policies) -> Result<Rc<Action>, Clobbering> {
        if action.is_empty() {
            return Ok(self.clone());
        }
        self.do_combine(action, p)
    }

    fn ordered_replacements(&self, out: &mut Vec<(Span, String)>) {
        let r = self.range;
        if !self.insert_before.is_empty() {
            out.push((Span { begin: r.begin, end: r.begin }, self.insert_before.clone()));
        }
        if let Some(rep) = &self.replacement {
            out.push((r, rep.clone()));
        }
        for c in &self.children {
            c.ordered_replacements(out);
        }
        if !self.insert_after.is_empty() {
            out.push((Span { begin: r.end, end: r.end }, self.insert_after.clone()));
        }
    }

    /// Action#with: a copy with some attributes changed. A replacing action swallows its children.
    fn with(
        &self,
        p: &Policies,
        range: Option<Span>,
        children: Option<Vec<Rc<Action>>>,
        replace: Option<(String, Option<String>, String)>,
    ) -> Result<Rc<Action>, Clobbering> {
        let (insert_before, replacement, insert_after) = replace
            .unwrap_or_else(|| (self.insert_before.clone(), self.replacement.clone(), self.insert_after.clone()));
        let mut children = children.unwrap_or_else(|| self.children.clone());
        let range = range.unwrap_or(self.range);
        if replacement.is_some() {
            children = swallow(p, &children)?;
        }
        Ok(Rc::new(Action { range, insert_before, replacement, insert_after, children }))
    }

    fn do_combine(self: &Rc<Self>, action: &Rc<Action>, p: &Policies) -> Result<Rc<Action>, Clobbering> {
        if action.range == self.range { self.merge(action, p) } else { self.place_in_hierarchy(action, p) }
    }

    fn place_in_hierarchy(self: &Rc<Self>, action: &Rc<Action>, p: &Policies) -> Result<Rc<Action>, Clobbering> {
        let family = self.analyse_hierarchy(action, p)?;
        if let Some(fusible) = family.fusible {
            let mut others = family.left;
            others.extend(family.child.unwrap_or_default());
            others.extend(family.right);
            return self.fuse_deletions(action, &fusible, others, p);
        }
        let extra = if let Some(parent) = family.parent {
            parent.do_combine(action, p)?
        } else if let Some(child) = family.child {
            action.with(p, None, Some(child), None)?.combine_children(&action.children, p)?
        } else {
            action.clone()
        };
        let mut children = family.left;
        children.push(extra);
        children.extend(family.right);
        self.with(p, None, Some(children), None)
    }

    fn combine_children(self: Rc<Self>, more: &[Rc<Action>], p: &Policies) -> Result<Rc<Action>, Clobbering> {
        more.iter().try_fold(self, |parent, child| parent.place_in_hierarchy(child, p))
    }

    fn fuse_deletions(
        &self,
        action: &Rc<Action>,
        fusible: &[Rc<Action>],
        others: Vec<Rc<Action>>,
        p: &Policies,
    ) -> Result<Rc<Action>, Clobbering> {
        let without = self.with(p, None, Some(others), None)?;
        let fused_range = fusible.iter().fold(action.range, |r, f| r.join(f.range));
        let fused = action.with(p, Some(fused_range), None, None)?;
        without.do_combine(&fused, p)
    }

    /// First index at or after `from` whose child satisfies `pred` (children are sorted), else len.
    fn bsearch_child_index(&self, from: usize, pred: impl Fn(&Action) -> bool) -> usize {
        from + self.children[from..].partition_point(|c| !pred(c))
    }

    fn analyse_hierarchy(&self, action: &Action, p: &Policies) -> Result<Family, Clobbering> {
        let r = action.range;
        let mut left_index = self.bsearch_child_index(0, |c| c.range.end > r.begin);
        let start = left_index.saturating_sub(1); // see "corner case" in action.rb
        let mut right_index = self.bsearch_child_index(start, |c| c.range.begin >= r.end);
        let center = right_index as isize - left_index as isize;
        let (mut parent, mut fusible, mut child) = (None, None, None);
        match center {
            0 => {}
            -1 => {
                left_index -= 1;
                right_index += 1;
                parent = Some(self.children[left_index].clone());
            }
            _ => {
                let overlap_left = self.children[left_index].range.begin.cmp(&r.begin) as i8;
                let overlap_right = self.children[right_index - 1].range.end.cmp(&r.end) as i8;
                if center == 1 && overlap_left <= 0 && overlap_right >= 0 {
                    parent = Some(self.children[left_index].clone());
                } else {
                    let mut contained: Vec<Rc<Action>> = self.children[left_index..right_index].to_vec();
                    let first = if overlap_left < 0 { Some(contained.remove(0)) } else { None };
                    let last = if overlap_right > 0 { contained.pop() } else { None };
                    fusible = check_fusible(action, [first, last].into_iter().flatten().collect(), p)?;
                    child = Some(contained);
                }
            }
        }
        Ok(Family {
            parent,
            left: self.children[..left_index].to_vec(),
            right: self.children[right_index..].to_vec(),
            fusible,
            child,
        })
    }

    fn merge(&self, action: &Rc<Action>, p: &Policies) -> Result<Rc<Action>, Clobbering> {
        // call_enforcer_for_merge
        if let (Some(a), Some(b)) = (&self.replacement, &action.replacement)
            && a != b
        {
            p.enforce(Event::DifferentReplacements)?;
        }
        let merged = self.with(
            p,
            None,
            None,
            Some((
                format!("{}{}", action.insert_before, self.insert_before),
                action.replacement.clone().or_else(|| self.replacement.clone()),
                format!("{}{}", self.insert_after, action.insert_after),
            )),
        )?;
        merged.combine_children(&action.children, p)
    }
}

fn check_fusible(
    action: &Action,
    fusible: Vec<Rc<Action>>,
    p: &Policies,
) -> Result<Option<Vec<Rc<Action>>>, Clobbering> {
    if fusible.is_empty() {
        return Ok(None);
    }
    for child in &fusible {
        let event = if action.is_insertion() || child.is_insertion() {
            Event::CrossingInsertions
        } else {
            Event::CrossingDeletions
        };
        p.enforce(event)?;
    }
    Ok(Some(fusible))
}

fn swallow(p: &Policies, children: &[Rc<Action>]) -> Result<Vec<Rc<Action>>, Clobbering> {
    if children.iter().any(|c| c.is_insertion()) {
        p.enforce(Event::SwallowedInsertions)?;
    }
    Ok(Vec::new())
}

/// `Parser::Source::TreeRewriter` over one source text.
#[derive(Clone)]
pub struct TreeRewriter {
    source: Arc<str>,
    policies: Policies,
    root: Rc<Action>,
}

impl std::fmt::Debug for TreeRewriter {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TreeRewriter").field("replacements", &self.as_replacements()).finish()
    }
}

impl TreeRewriter {
    pub fn new(source: Arc<str>, policies: Policies) -> Self {
        let all = Span { begin: -1, end: source.len() as isize + 1 };
        TreeRewriter { source, policies, root: Rc::new(Action::leaf(all, String::new(), None, String::new())) }
    }

    pub fn source(&self) -> &Arc<str> {
        &self.source
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_empty()
    }

    /// TreeRewriter#merge!: on error `self` is unchanged.
    pub fn merge(&mut self, with: &TreeRewriter) -> Result<(), Clobbering> {
        assert!(
            Arc::ptr_eq(&self.source, &with.source) || self.source == with.source,
            "TreeRewriter are not for the same source_buffer"
        );
        // The roots never carry edits of their own (no range can reach theirs), so merging is
        // placing `with`'s top-level actions; in the common case each fits between ours.
        if with.root.insert_before.is_empty()
            && with.root.insert_after.is_empty()
            && with.root.replacement.is_none()
            && self.insert_in_gaps(&with.root.children)
        {
            return Ok(());
        }
        self.root = self.root.combine(&with.root, &self.policies)?;
        Ok(())
    }

    /// Where `action` would land among the root's children if it overlaps none of them:
    /// analyse_hierarchy's `center == 0` (place_in_hierarchy then only splices it in).
    fn gap_index(&self, action: &Action) -> Option<usize> {
        let r = action.range;
        let left = self.root.bsearch_child_index(0, |c| c.range.end > r.begin);
        let right = self.root.bsearch_child_index(left.saturating_sub(1), |c| c.range.begin >= r.end);
        (left == right).then_some(left)
    }

    /// Splice actions that each fall in a gap between the root's children, in place: what
    /// `root.combine` would build, without copying the root's children per action (quadratic
    /// over a file's corrections). False, with nothing changed, unless every action fits.
    fn insert_in_gaps(&mut self, actions: &[Rc<Action>]) -> bool {
        if self.root.replacement.is_some() {
            return false;
        }
        let actions: Vec<&Rc<Action>> = actions.iter().filter(|a| !a.is_empty()).collect();
        // Sorted, non-overlapping actions keep fitting once the earlier ones are spliced in.
        let disjoint =
            actions.windows(2).all(|w| w[0].range.end <= w[1].range.begin && w[0].range.end < w[1].range.end);
        if !disjoint || actions.iter().any(|a| self.gap_index(a).is_none()) {
            return false;
        }
        for a in actions {
            let i = self.gap_index(a).unwrap();
            Rc::make_mut(&mut self.root).children.insert(i, a.clone());
        }
        true
    }

    pub fn replace(&mut self, start: usize, end: usize, content: &str) -> Result<(), Clobbering> {
        self.combine(start, end, String::new(), Some(content.to_string()), String::new())
    }

    pub fn wrap(&mut self, start: usize, end: usize, before: &str, after: &str) -> Result<(), Clobbering> {
        self.combine(start, end, before.to_string(), None, after.to_string())
    }

    pub fn remove(&mut self, start: usize, end: usize) -> Result<(), Clobbering> {
        self.replace(start, end, "")
    }

    pub fn insert_before(&mut self, start: usize, end: usize, content: &str) -> Result<(), Clobbering> {
        self.wrap(start, end, content, "")
    }

    pub fn insert_after(&mut self, start: usize, end: usize, content: &str) -> Result<(), Clobbering> {
        self.wrap(start, end, "", content)
    }

    /// TreeRewriter#process: the rewritten source.
    pub fn process(&self) -> String {
        let src = &*self.source;
        let mut out = String::with_capacity(src.len());
        let mut last_end = 0usize;
        for (r, rep) in self.ordered() {
            out.push_str(&src[last_end..r.begin as usize]);
            out.push_str(&rep);
            last_end = r.end as usize;
        }
        out.push_str(&src[last_end..]);
        out
    }

    /// TreeRewriter#as_replacements: `(start, end, replacement)` in application order.
    pub fn as_replacements(&self) -> Vec<(usize, usize, String)> {
        self.ordered().into_iter().map(|(r, s)| (r.begin as usize, r.end as usize, s)).collect()
    }

    fn ordered(&self) -> Vec<(Span, String)> {
        let mut out = Vec::new();
        self.root.ordered_replacements(&mut out);
        out
    }

    fn combine(
        &mut self,
        start: usize,
        end: usize,
        before: String,
        replacement: Option<String>,
        after: String,
    ) -> Result<(), Clobbering> {
        // check_range_validity: an out-of-bounds range is a bug in the caller (IndexError in Ruby).
        assert!(
            start <= end && end <= self.source.len(),
            "The range {start}...{end} is outside the bounds of the source"
        );
        let span = Span { begin: start as isize, end: end as isize };
        let action = Rc::new(Action::leaf(span, before, replacement, after));
        if self.insert_in_gaps(std::slice::from_ref(&action)) {
            return Ok(());
        }
        self.root = self.root.combine(&action, &self.policies)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "tree_rewriter_tests.rs"]
mod tests;
