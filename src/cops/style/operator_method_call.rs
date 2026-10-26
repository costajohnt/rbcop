use crate::prelude::*;
use helpers::parentheses_corrector;
use helpers::parser_parent::parser_parent;

pub const NAME: &str = "Style/OperatorMethodCall";
pub static COP: OperatorMethodCall = OperatorMethodCall;
pub struct OperatorMethodCall;

const OPERATORS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~", "!",
    "!=", "!~",
];

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// Parser `node.children.first` is truthy (not nil and not a missing child).
fn first_child_truthy(n: &Node) -> bool {
    match n {
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            // a call with a block is a parser block (first child: the send)
            c.receiver().is_some() || c.block().is_some_and(|b| b.as_block_node().is_some())
        }
        Node::ConstantReadNode { .. } => false,
        Node::ConstantPathNode { .. } => true,
        Node::ArrayNode { .. } => n.as_array_node().unwrap().elements().iter().next().is_some(),
        Node::HashNode { .. } => n.as_hash_node().unwrap().elements().iter().next().is_some(),
        Node::ParenthesesNode { .. } => n.as_parentheses_node().unwrap().body().is_some(),
        Node::NilNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::SelfNode { .. }
        | Node::ForwardingSuperNode { .. }
        | Node::RedoNode { .. }
        | Node::RetryNode { .. }
        | Node::SourceEncodingNode { .. }
        | Node::SourceLineNode { .. } => false,
        Node::SuperNode { .. } => n.as_super_node().unwrap().arguments().is_some(),
        Node::YieldNode { .. } => n.as_yield_node().unwrap().arguments().is_some(),
        Node::ReturnNode { .. } => n.as_return_node().unwrap().arguments().is_some(),
        Node::BreakNode { .. } => n.as_break_node().unwrap().arguments().is_some(),
        Node::NextNode { .. } => n.as_next_node().unwrap().arguments().is_some(),
        _ => true,
    }
}

/// INVALID_SYNTAX_ARG_TYPES: splat kwsplat forwarded_args forwarded_restarg forwarded_kwrestarg block_pass
fn invalid_syntax_argument(arg: &Node) -> bool {
    let first = match (arg.as_hash_node(), arg.as_keyword_hash_node()) {
        (Some(h), _) => h.elements().iter().next(),
        (_, Some(h)) => h.elements().iter().next(),
        _ => {
            return matches!(
                kind::of(arg),
                kind::SPLAT_NODE | kind::FORWARDING_ARGUMENTS_NODE | kind::BLOCK_ARGUMENT_NODE | kind::ASSOC_SPLAT_NODE
            );
        }
    };
    first.is_some_and(|f| f.as_assoc_splat_node().is_some())
}

impl Cop for OperatorMethodCall {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !OPERATORS.contains(&name) || is_csend(&call) {
            return;
        }
        let Some(dot) = call.call_operator_loc() else { return };
        let Some(sel) = call.message_loc() else { return };
        if matches!(name, "~" | "!") && text(&sel) != name {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        if matches!(kind::of(&receiver), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        let rhs = &args[0];
        let parent = parser_parent(cx.ancestors).and_then(|p| p.as_call_node());
        // method_call_with_parenthesized_arg?
        if parent.as_ref().is_some_and(|p| !is_csend(p)) && call.opening_loc().is_some() && first_child_truthy(rhs) {
            return;
        }
        if invalid_syntax_argument(rhs) {
            return;
        }
        let src = cx.src;
        let node_range = send_range(&call);
        // wrap_in_parentheses_if_chained
        let chained = parent.as_ref().is_some_and(|p| {
            send_args(p).first().is_none_or(|f| !helpers::node_equality::same(src, range(f), node_range))
        });
        let paren_fix = if chained {
            let (Some(o), Some(c)) = (call.opening_loc(), call.closing_loc()) else {
                // ParenthesesCorrector raises on a call without parentheses: no offense.
                return;
            };
            let heredoc = is_heredoc(rhs).then(|| range(rhs));
            Some(parentheses_corrector::plan(
                src,
                (o.start_offset(), o.end_offset()),
                (c.start_offset(), c.end_offset()),
                Some(rhs),
                heredoc,
                None,
            ))
        } else {
            None
        };
        let (rhs_start, sel_start, sel_end) = (rhs.location().start_offset(), sel.start_offset(), sel.end_offset());
        let space_after = sel_end == rhs_start
            || (parent.is_none() && name == "/" && src.text.get(sel_end..rhs_start) == Some("("));
        let (ds, de) = (dot.start_offset(), dot.end_offset());
        cx.add_fix(ds, de, "Redundant dot detected.", |c| {
            if let Some(f) = &paren_fix {
                parentheses_corrector::apply(c, f);
                c.insert_after(sel_start, sel_end, " ");
                c.wrap(node_range.0, node_range.1, "(", ")");
            }
            c.replace(ds, de, " ");
            if space_after {
                c.insert_after(sel_start, sel_end, " ");
            }
        });
    }
}
