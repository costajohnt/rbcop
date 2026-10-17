use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Style/MultipleComparison";
pub static COP: MultipleComparison = MultipleComparison;
pub struct MultipleComparison;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// parser `call`: send/csend (a call with a literal block is a `block`; a folded `- 1` an `int`).
fn is_call(n: &Node) -> bool {
    n.as_call_node().is_some_and(|c| !c.block().is_some_and(|b| b.as_block_node().is_some()))
        && helpers::literal_node::folded_numeric(n).is_none()
}

/// parser `lvar`, `it` in an `it` block included.
fn is_lvar(n: &Node) -> bool {
    helpers::node_types::lvar_name(n).is_some()
}

/// `(send $recv :== $arg)`: the receiver and the single argument.
fn eq_parts<'pr>(n: &Node<'pr>) -> Option<(Node<'pr>, Node<'pr>)> {
    let c = n.as_call_node()?;
    if method_name(&c) != "==" || is_csend(&c) || !is_call(n) || c.block().is_some() {
        return None;
    }
    let mut args = call_args(&c).into_iter();
    let arg = args.next()?;
    if args.next().is_some() {
        return None;
    }
    Some((c.receiver()?, arg))
}

struct Comparison<'pr> {
    var: Node<'pr>,
    obj: Node<'pr>,
}

/// simple_comparison: `(send ${lvar call} :== $_)` or `(send $_ :== ${lvar call})`.
fn simple_comparison<'pr>(n: &Node<'pr>, allow_method: bool) -> Option<Comparison<'pr>> {
    let (recv, arg) = eq_parts(n)?;
    let (var, obj) = if is_lvar(&recv) || is_call(&recv) {
        (recv, arg)
    } else if is_lvar(&arg) || is_call(&arg) {
        (arg, recv)
    } else {
        return None;
    };
    if is_call(&var) && !allow_method {
        return None;
    }
    Some(Comparison { var, obj })
}

fn nested_comparison(n: &Node, allow_method: bool) -> bool {
    let Some(or) = n.as_or_node() else { return false };
    [or.left(), or.right()].iter().all(|p| simple_comparison(p, allow_method).is_some() || nested_comparison(p, allow_method))
}

struct Search<'s, 'pr> {
    src: &'s Source<'s>,
    allow_method: bool,
    /// distinct (structurally) variables
    variables: Vec<Node<'pr>>,
    /// (value range, its comparison's range)
    values: Vec<((usize, usize), (usize, usize))>,
    skipped: Vec<(usize, usize)>,
}

impl<'pr> Search<'_, 'pr> {
    fn find(&mut self, n: Node<'pr>) {
        if let Some(or) = n.as_or_node() {
            self.find(or.left());
            self.find(or.right());
            return;
        }
        if let Some((r, a)) = eq_parts(&n)
            && is_lvar(&r) && is_lvar(&a) {
                return;
            }
        let Some(Comparison { var, obj }) = simple_comparison(&n, self.allow_method) else { return };
        if self.allow_method && is_call(&obj) {
            self.skipped.push(range(&n));
            return;
        }
        if !self.variables.iter().any(|v| helpers::node_equality::same(self.src, range(v), range(&var))) {
            self.variables.push(var);
        }
        if self.variables.len() > 1 {
            return;
        }
        self.values.push((range(&obj), range(&n)));
    }
}

impl Cop for MultipleComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // root_of_or_node: the parser parent is not an `or`
        if cx.parent().is_some_and(|p| p.as_or_node().is_some()) {
            return;
        }
        let allow_method = cx.cfg.bool("AllowMethodComparison").unwrap_or(true);
        if !nested_comparison(node, allow_method) {
            return;
        }
        let or = node.as_or_node().unwrap();
        let mut s = Search { src: cx.src, allow_method, variables: Vec::new(), values: Vec::new(), skipped: Vec::new() };
        s.find(or.left());
        s.find(or.right());
        let Some(var) = s.variables.first() else { return };
        let threshold = cx.cfg.int("ComparisonsThreshold").unwrap_or(2);
        if (s.values.len() as i64) < threshold {
            return;
        }
        let (start, end) = (s.values[0].1 .0, s.values.last().unwrap().1 .1);
        if s.skipped.iter().any(|&(a, b)| start <= a && b <= end) {
            return;
        }
        let elements: Vec<&str> = s.values.iter().map(|&((a, b), _)| &cx.src.text[a..b]).collect();
        let argument = match helpers::node_types::lvar_name(var) {
            Some(l) => std::str::from_utf8(l).unwrap_or(""),
            None => node_text(var),
        };
        let repl = format!("[{}].include?({argument})", elements.join(", "));
        cx.add_fix(
            start,
            end,
            "Avoid comparing a variable with multiple items in a conditional, use `Array#include?` instead.",
            |c| c.replace(start, end, &repl),
        );
    }
}
