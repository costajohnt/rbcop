use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/CombinableDefined";
pub static COP: CombinableDefined = CombinableDefined;
pub struct CombinableDefined;

type R = (usize, usize);

fn range(n: &Node) -> R {
    (n.location().start_offset(), n.location().end_offset())
}

/// A term: (its range, whether it is `defined?`, the defined subject (const or call) range and
/// its namespace/receiver range, whether it is the last child of its `and` parent).
struct Term {
    range: R,
    defined: bool,
    subject: Option<(R, Option<R>)>,
    last: bool,
}

/// `node.each_descendant.select { |d| d.parent.and_type? && !d.and_type? }` in pre-order.
#[derive(Default)]
struct Terms(Vec<Term>);

impl Terms {
    fn push(&mut self, node: &Node, sibling: &Node, is_right: bool, src: &crate::source::Source) {
        if node.as_and_node().is_some() {
            return;
        }
        // `term == term.parent.children.last` is structural.
        let last = is_right || helpers::node_equality::same(src, range(node), range(sibling));
        let mut subject = None;
        let defined = node.as_defined_node().is_some();
        if let Some(d) = node.as_defined_node() {
            let v = d.value();
            if let Some(c) = v.as_constant_path_node() {
                subject = Some((range(&v), c.parent().map(|p| range(&p))));
            } else if v.as_constant_read_node().is_some() {
                subject = Some((range(&v), None));
            } else if let Some(c) = v.as_call_node()
                && !c.block().is_some_and(|b| b.as_block_node().is_some())
            {
                subject = Some((range(&v), c.receiver().map(|r| range(&r))));
            }
        }
        self.0.push(Term { range: range(node), defined, subject, last });
    }
}

struct Walker<'a, 's> {
    terms: Terms,
    src: &'a crate::source::Source<'s>,
}

impl<'pr> Visit<'pr> for Walker<'_, '_> {
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        let (l, r) = (node.left(), node.right());
        self.terms.push(&l, &r, false, self.src);
        self.visit(&l);
        self.terms.push(&r, &r, true, self.src);
        self.visit(&r);
    }
}

impl Cop for CombinableDefined {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let mut w = Walker { terms: Terms::default(), src };
        w.visit(node);
        let terms = w.terms.0;
        if !terms.iter().all(|t| t.defined) {
            return;
        }
        let calls: Vec<&Term> = terms.iter().filter(|t| t.subject.is_some()).collect();
        let namespaces: Vec<R> = calls.iter().filter_map(|t| t.subject.unwrap().1).collect();
        let same = |a: R, b: R| helpers::node_equality::same(src, a, b);
        let Some(term) = calls.iter().find(|t| namespaces.iter().any(|n| same(*n, t.subject.unwrap().0))) else {
            return;
        };
        let text = src.text;
        let bytes = text.as_bytes();
        let right = Surrounding { side: Side::Right, newlines: false, ..Default::default() };
        let (ts, te) = term.range;
        let removal = if term.last {
            // pos -= 1 until source[pos, 3].start_with?('&&', 'and')
            let mut pos = ts;
            while !(bytes[pos..].starts_with(b"&&") || bytes[pos..].starts_with(b"and")) {
                pos -= 1;
            }
            range_with_surrounding_space(text, pos - 1, te, right)
        } else {
            // pos += 1 until source[..pos].end_with?('&&', 'and')
            let mut pos = te;
            while !(bytes[..=pos].ends_with(b"&&") || bytes[..=pos].ends_with(b"and")) {
                pos += 1;
            }
            range_with_surrounding_space(text, ts, pos + 1, right)
        };
        let (s, e) = range(node);
        cx.add_fix(s, e, "Combine nested `defined?` calls.", |c| c.remove(removal.0, removal.1));
    }
}
