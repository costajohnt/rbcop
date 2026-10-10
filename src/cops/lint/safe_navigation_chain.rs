use crate::helpers::nil_methods::nil_method;
use crate::helpers::node_equality::same;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/SafeNavigationChain";
pub static COP: SafeNavigationChain = SafeNavigationChain;
pub struct SafeNavigationChain;

const MSG: &str = "Do not chain ordinary method call after safe navigation operator.";
const COMPARISON: [&str; 7] = ["==", "===", "!=", "<=", ">=", ">", "<"];

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// `bad_method?` receiver capture: a csend (with or without a block), or `(csend)` in parentheses.
fn safe_nav(recv: &Node) -> Option<(usize, usize)> {
    if let Some(c) = recv.as_call_node() {
        return is_csend(&c).then(|| range(recv));
    }
    let body = recv.as_parentheses_node()?.body()?;
    let stmts: Vec<Node> = body.as_statements_node()?.body().iter().collect();
    let [inner] = stmts.as_slice() else { return None };
    let c = inner.as_call_node()?;
    (is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none())).then(|| range(recv))
}

/// Which branch of an if/unless the node sits in, parser-wise: Some(true) for `if_branch`,
/// Some(false) for `else_branch`.
fn branch(ifnode: &Node, below: &Node) -> Option<bool> {
    let b = range(below);
    if let Some(i) = ifnode.as_if_node() {
        if i.statements().is_some_and(|s| range(&s.as_node()) == b) {
            return Some(true);
        }
        return i.subsequent().is_some_and(|s| range(&s) == b).then_some(false);
    }
    let u = ifnode.as_unless_node()?;
    // rubocop-ast's if_branch of an `unless` is its body.
    if u.statements().is_some_and(|s| range(&s.as_node()) == b) {
        return Some(true);
    }
    u.else_clause().is_some_and(|s| range(&s.as_node()) == b).then_some(false)
}

fn condition(ifnode: &Node) -> Option<(usize, usize)> {
    match (ifnode.as_if_node(), ifnode.as_unless_node()) {
        (Some(i), _) => Some(range(&i.predicate())),
        (_, Some(u)) => Some(range(&u.predicate())),
        _ => None,
    }
}

impl Cop for SafeNavigationChain {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::CALL_TARGET_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
            kind::INDEX_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node() else { return op_assign(cx, node) };
        if is_csend(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let Some(nav) = safe_nav(&recv) else { return };
        let name = method_name(&call);
        if nil_method(cx.cfg, name) || matches!(name, "+@" | "-@") {
            return;
        }
        let src = cx.src;
        let own = send_range(&call);
        // Parser parent: the block when the call has one, else the nearest non-wrapper ancestor.
        let has_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        let idx = if has_block {
            None
        } else {
            cx.ancestors.iter().rposition(|n| frame(n) != Frame::Transparent)
        };
        let parent = idx.map(|i| &cx.ancestors[i]);
        let below = idx.and_then(|i| cx.ancestors.get(i + 1));
        // require_safe_navigation?
        if let Some(and) = parent.and_then(|p| p.as_and_node()) {
            let (lhs, rhs) = (and.left(), and.right());
            let recv_range = |n: &Node| n.as_call_node().and_then(|c| c.receiver()).map(|r| range(&r));
            if same(src, range(&rhs), own) {
                let same_recv = match (recv_range(&lhs), recv_range(&rhs)) {
                    (Some(a), Some(b)) => same(src, a, b),
                    (None, None) => true,
                    _ => false,
                };
                if same_recv {
                    return;
                }
            }
        }
        // ternary_safe_navigation? / ternary_else_branch?
        let mut else_branch = false;
        if let (Some(p), Some(b)) = (parent, below)
            && let Some(cond) = condition(p)
            && same(src, cond, nav)
        {
            match branch(p, b) {
                Some(true) => return,
                Some(false) => else_branch = true,
                None => {}
            }
        }
        let dot = call.call_operator_loc().map(|d| d.start_offset());
        let args: Vec<&str> = send_args(&call).iter().map(|a| node_text(a)).collect();
        // require_parentheses?
        let in_collection =
            parent.is_some_and(|p| p.as_array_node().is_some() || p.as_assoc_node().is_some()) && dot.is_none();
        let wrap = in_collection
            || (COMPARISON.contains(&name)
                && parent.is_some_and(|p| {
                    p.as_and_node().is_some_and(|a| a.operator_loc().as_slice() == b"&&")
                        || p.as_or_node().is_some_and(|o| o.operator_loc().as_slice() == b"||")
                        || p.as_call_node().is_some_and(|c: CallNode| COMPARISON.contains(&method_name(&c)))
                }));
        report(cx, nav, dot, name, &args, own, wrap, else_branch);
    }
}

/// The offense and its corrector, shared by plain sends and the sends inside op-assigns and
/// multiple-assignment targets.
#[allow(clippy::too_many_arguments)]
fn report(
    cx: &mut Cx,
    nav: (usize, usize),
    dot: Option<usize>,
    name: &str,
    args: &[&str],
    own: (usize, usize),
    wrap: bool,
    skip_fix: bool,
) {
    let start = dot.unwrap_or(nav.1);
    let end = own.1;
    let source = if matches!(name, "[]" | "[]=") {
        format!("{name}({})", args.join(", "))
    } else {
        cx.src.slice(start, end).to_string()
    };
    let replacement = if source.starts_with('.') { format!("&{source}") } else { format!("&.{source}") };
    cx.add_fix(start, end, MSG, |c| {
        if skip_fix {
            return;
        }
        c.replace(start, end, &replacement);
        if wrap {
            c.wrap(own.0, own.1, "(", ")");
        }
    });
}

fn utf8(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn index_args<'pr>(args: Option<ruby_prism::ArgumentsNode<'pr>>, block: Option<Node<'pr>>) -> Vec<&'pr str> {
    let mut out: Vec<&str> = args.map(|a| a.arguments().iter().map(|x| node_text(&x)).collect()).unwrap_or_default();
    out.extend(block.map(|b| node_text(&b)));
    out
}

/// Parser `send` inside `op_asgn`/`or_asgn`/`and_asgn`/`mlhs` (Prism Call*WriteNode,
/// Index*WriteNode, Call/IndexTargetNode). Its parser parent is the assignment, so no
/// and/if/collection rule applies.
fn op_assign(cx: &mut Cx, node: &Node) {
    let msg_end = |m: Option<ruby_prism::Location>| m.map_or(node.location().end_offset(), |m| m.end_offset());
    let (recv, dot, name, args, end) = match kind::of(node) {
        kind::CALL_OPERATOR_WRITE_NODE => {
            let n = node.as_call_operator_write_node().unwrap();
            (n.receiver(), n.call_operator_loc(), utf8(n.read_name().as_slice()), vec![], msg_end(n.message_loc()))
        }
        kind::CALL_OR_WRITE_NODE => {
            let n = node.as_call_or_write_node().unwrap();
            (n.receiver(), n.call_operator_loc(), utf8(n.read_name().as_slice()), vec![], msg_end(n.message_loc()))
        }
        kind::CALL_AND_WRITE_NODE => {
            let n = node.as_call_and_write_node().unwrap();
            (n.receiver(), n.call_operator_loc(), utf8(n.read_name().as_slice()), vec![], msg_end(n.message_loc()))
        }
        kind::CALL_TARGET_NODE => {
            let n = node.as_call_target_node().unwrap();
            let end = n.message_loc().end_offset();
            (Some(n.receiver()), Some(n.call_operator_loc()), utf8(n.name().as_slice()), vec![], end)
        }
        kind::INDEX_OPERATOR_WRITE_NODE => {
            let n = node.as_index_operator_write_node().unwrap();
            let args = index_args(n.arguments(), n.block().map(|b| b.as_node()));
            (n.receiver(), n.call_operator_loc(), "[]".into(), args, n.closing_loc().end_offset())
        }
        kind::INDEX_OR_WRITE_NODE => {
            let n = node.as_index_or_write_node().unwrap();
            let args = index_args(n.arguments(), n.block().map(|b| b.as_node()));
            (n.receiver(), n.call_operator_loc(), "[]".into(), args, n.closing_loc().end_offset())
        }
        kind::INDEX_AND_WRITE_NODE => {
            let n = node.as_index_and_write_node().unwrap();
            let args = index_args(n.arguments(), n.block().map(|b| b.as_node()));
            (n.receiver(), n.call_operator_loc(), "[]".into(), args, n.closing_loc().end_offset())
        }
        kind::INDEX_TARGET_NODE => {
            let n = node.as_index_target_node().unwrap();
            let args = index_args(n.arguments(), n.block().map(|b| b.as_node()));
            (Some(n.receiver()), None, "[]=".into(), args, n.closing_loc().end_offset())
        }
        _ => return,
    };
    let Some(recv) = recv else { return };
    let Some(nav) = safe_nav(&recv) else { return };
    if dot.as_ref().is_some_and(|d| d.as_slice() == b"&.") || nil_method(cx.cfg, &name) {
        return;
    }
    let own = (node.location().start_offset(), end);
    report(cx, nav, dot.map(|d| d.start_offset()), &name, &args, own, false, false);
}
