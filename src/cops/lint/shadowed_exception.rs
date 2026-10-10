use crate::helpers::resbody;
use crate::helpers::ruby_exceptions::EXCEPTIONS;
use crate::prelude::*;
use std::cmp::Ordering;
use std::collections::HashMap;

pub const NAME: &str = "Lint/ShadowedException";
pub static COP: ShadowedException = ShadowedException;
pub struct ShadowedException;

/// A resolved class: its ancestor chain (self first) and whether its direct superclass is
/// SystemCallError.
#[derive(Clone, Copy)]
struct Class {
    chain: &'static str,
    sce: bool,
}

impl Class {
    fn name(self) -> &'static str {
        self.chain.split(' ').next().unwrap()
    }
    fn is_exception(self) -> bool {
        self.name() == "Exception"
    }
    /// `Module#<=>`
    fn cmp(self, other: Class) -> Option<Ordering> {
        if self.name() == other.name() {
            Some(Ordering::Equal)
        } else if self.chain.split(' ').any(|c| c == other.name()) {
            Some(Ordering::Less)
        } else if other.chain.split(' ').any(|c| c == self.name()) {
            Some(Ordering::Greater)
        } else {
            None
        }
    }
}

static TABLE: LazyLock<HashMap<&'static str, Class>> =
    LazyLock::new(|| EXCEPTIONS.iter().map(|&(p, chain, _, sce)| (p, Class { chain, sce })).collect());

/// `Kernel.const_get(source)` restricted to exception classes.
fn resolve(source: &str) -> Option<Class> {
    let mut s = source.strip_prefix("::").unwrap_or(source);
    while let Some(rest) = s.strip_prefix("Object::") {
        s = rest;
    }
    TABLE.get(s).copied()
}

type Group = Vec<Option<Class>>;

fn includes_exception(g: &Group) -> bool {
    g.iter().any(|c| c.is_some_and(|c| c.is_exception()))
}

/// `compare_exceptions` truthiness.
fn compare(a: Option<Class>, b: Option<Class>) -> bool {
    match (a, b) {
        // Errno classes are siblings: never comparable unless identical, then same Errno.
        (Some(a), Some(b)) if a.sce && b.sce => false,
        (Some(a), Some(b)) => a.cmp(b).is_some(),
        _ => false,
    }
}

fn multiple_levels(g: &Group) -> bool {
    if g.len() > 1 && includes_exception(g) {
        return true;
    }
    (0..g.len()).any(|i| (i + 1..g.len()).any(|j| compare(g[i], g[j])))
}

/// `Array#<=>` with nil elements (`nil <=> nil` is 0, nil against a class is nil).
fn array_cmp(x: &Group, y: &Group) -> Option<Ordering> {
    for (a, b) in x.iter().zip(y) {
        let r = match (a, b) {
            (None, None) => Some(Ordering::Equal),
            (Some(a), Some(b)) => a.cmp(*b),
            _ => None,
        };
        if r != Some(Ordering::Equal) {
            return r;
        }
    }
    Some(x.len().cmp(&y.len()))
}

fn pair_sorted(x: &Group, y: &Group) -> bool {
    if includes_exception(x) {
        false
    } else if includes_exception(y) || x.iter().all(Option::is_none) || y.iter().all(Option::is_none) {
        true
    } else {
        array_cmp(x, y).unwrap_or(Ordering::Equal) != Ordering::Greater
    }
}

impl Cop for ShadowedException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::BEGIN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut rescues = Vec::new();
        let mut groups: Vec<Group> = Vec::new();
        let mut clause = node.as_begin_node().unwrap().rescue_clause();
        while let Some(r) = clause {
            let exceptions: Vec<Node> = r.exceptions().iter().collect();
            groups.push(if exceptions.is_empty() {
                vec![resolve("StandardError")]
            } else {
                exceptions.iter().map(|e| resolve(node_text(e))).collect()
            });
            rescues.push(resbody::range(cx.src, &r));
            clause = r.subsequent();
        }
        let shadowing = groups
            .iter()
            .position(multiple_levels)
            .or_else(|| groups.windows(2).position(|w| !pair_sorted(&w[0], &w[1])));
        if let Some(i) = shadowing {
            let (s, e) = rescues[i];
            cx.add(s, e, "Do not shadow rescued Exceptions.");
        }
    }
}
