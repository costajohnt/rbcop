use crate::helpers::if_node::parser_end;
use crate::prelude::*;
use ruby_prism::{CallNode, StatementsNode};

pub const NAME: &str = "Style/ComparableClamp";
pub static COP: ComparableClamp = ComparableClamp;
pub struct ComparableClamp;

type R = (usize, usize);

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

fn range(n: &Node) -> R {
    (n.location().start_offset(), n.location().end_offset())
}

fn single(stmts: Option<StatementsNode>) -> Option<Node> {
    let mut it = stmts?.body().iter();
    let n = it.next()?;
    it.next().is_none().then_some(n)
}

/// `(send L op R)` with op `<` or `>`: (L, is_lt, R).
fn comparison(node: &Node) -> Option<(R, bool, R)> {
    let call = node.as_call_node().filter(|c| !is_csend(c))?;
    let lt = match method_name(&call) {
        "<" => true,
        ">" => false,
        _ => return None,
    };
    let args = send_args(&call);
    let [arg] = args.as_slice() else { return None };
    Some((range(&call.receiver()?), lt, range(arg)))
}

fn plain_call<'pr>(node: &Node<'pr>, name: &str) -> Option<CallNode<'pr>> {
    node.as_call_node()
        .filter(|c| !is_csend(c) && method_name(c) == name && send_args(c).is_empty())
}

/// `(array _ _)`
fn pair_array(node: &Node) -> bool {
    node.as_array_node().is_some_and(|a| a.elements().iter().count() == 2)
}

impl ComparableClamp {
    fn on_send(&self, cx: &mut Cx, call: &CallNode) {
        let outer = match method_name(call) {
            "min" => "max",
            "max" => "min",
            _ => return,
        };
        if is_csend(call) || !send_args(call).is_empty() {
            return;
        }
        let Some(arr) = call.receiver().and_then(|r| r.as_array_node()) else { return };
        let elems: Vec<Node> = arr.elements().iter().collect();
        if elems.len() != 2 {
            return;
        }
        let inner = |n: &Node| plain_call(n, outer).and_then(|c| c.receiver()).is_some_and(|r| pair_array(&r));
        if inner(&elems[0]) || inner(&elems[1]) {
            let (s, e) = send_range(call);
            cx.add_with(s, e, "Use `Comparable#clamp` instead.", false);
        }
    }
}

impl Cop for ComparableClamp {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(call) = node.as_call_node() {
            return self.on_send(cx, &call);
        }
        let i = node.as_if_node().unwrap();
        // Only `if`/`elsif` chains: other shapes either do not match or crash in `node.branches`.
        if i.if_keyword_loc().is_none() || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(elsif) = i.subsequent().and_then(|s| s.as_if_node()) else { return };
        let Some(else_body) = elsif.subsequent().and_then(|s| s.as_else_node()).and_then(|e| single(e.statements()))
        else {
            return;
        };
        let (Some(if_body), Some(elsif_body)) = (single(i.statements()), single(elsif.statements())) else { return };
        let (Some(c1), Some(c2)) = (comparison(&i.predicate()), comparison(&elsif.predicate())) else { return };
        let src = cx.src;
        let same = |a: R, b: R| helpers::node_equality::same(src, a, b);
        let x = range(&else_body);
        let min_side = |(l, lt, r): (R, bool, R), b: R| if lt { same(l, x) && same(r, b) } else { same(l, b) && same(r, x) };
        let max_side = |(l, lt, r): (R, bool, R), b: R| if lt { same(l, b) && same(r, x) } else { same(l, x) && same(r, b) };
        let (b1, b2) = (range(&if_body), range(&elsif_body));
        if !((min_side(c1, b1) && max_side(c2, b2)) || (max_side(c1, b1) && min_side(c2, b2))) {
            return;
        }
        let t = |r: R| src.slice(r.0, r.1);
        let else_src = t(x);
        let (l, lt, r) = c1;
        let (min, max) = if (t(l) == else_src && lt) || (t(r) == else_src && !lt) { (b1, b2) } else { (b2, b1) };
        let paren = matches!(
            kind::of(&else_body),
            kind::AND_NODE | kind::OR_NODE | kind::IF_NODE | kind::UNLESS_NODE | kind::RANGE_NODE
        ) || is_assignment(&else_body)
            || else_body
                .as_call_node()
                .is_some_and(|c| !is_csend(&c) && OPERATOR_METHODS.contains(&method_name(&c)));
        let value = if paren { format!("({else_src})") } else { else_src.to_string() };
        let prefer = format!("{value}.clamp({}, {})", t(min), t(max));
        let start = node.location().start_offset();
        let end = parser_end(node);
        let is_elsif = i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"elsif");
        let width = helpers::alignment::configured_indentation_width(cx).max(0) as usize;
        let indentation = " ".repeat(src.column(start) + width);
        cx.add_fix(start, end, format!("Use `{prefer}` instead of `if/elsif/else`."), |c| {
            if is_elsif {
                c.insert_before(start, end, "else\n");
                c.replace(start, end, &format!("{indentation}{prefer}"));
            } else {
                c.replace(start, end, &prefer);
            }
        });
    }
}
