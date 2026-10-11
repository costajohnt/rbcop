use crate::helpers::frozen_string_literal as fsl;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantFreeze";
pub static COP: RedundantFreeze = RedundantFreeze;
pub struct RedundantFreeze;

/// The single statement inside parentheses (parser `(begin x)`).
fn paren_child<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let body = node.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => {
            let mut it = s.body().iter();
            let first = it.next()?;
            it.next().is_none().then_some(first)
        }
        None => Some(body),
    }
}

/// `strip_parenthesis`: a `begin`'s first child (even with more statements).
fn strip_parenthesis<'pr>(node: Node<'pr>) -> Node<'pr> {
    if let Some(p) = node.as_parentheses_node()
        && let Some(body) = p.body()
    {
        return match body.as_statements_node() {
            Some(s) => s.body().iter().next().unwrap_or(node),
            None => body,
        };
    }
    node
}

/// FrozenStringLiteral#uninterpolated_string? (dstr without begin/ivar/cvar/gvar descendants).
fn uninterpolated(parts: ruby_prism::NodeList) -> bool {
    parts.iter().all(|p| match &p {
        Node::EmbeddedStatementsNode { .. } => false,
        Node::EmbeddedVariableNode { .. } => {
            let v = p.as_embedded_variable_node().unwrap().variable();
            !matches!(
                v,
                Node::InstanceVariableReadNode { .. } | Node::ClassVariableReadNode { .. } | Node::GlobalVariableReadNode { .. }
            )
        }
        Node::InterpolatedStringNode { .. } => uninterpolated(p.as_interpolated_string_node().unwrap().parts()),
        _ => true,
    })
}

fn immutable_literal(cx: &Cx, node: Node) -> bool {
    let node = strip_parenthesis(node);
    let immutable = matches!(
        node,
        Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::RationalNode { .. }
            | Node::SourceLineNode { .. }
    ) || crate::helpers::literal_node::folded_numeric(&node).is_some();
    if immutable {
        return true;
    }
    let ruby3 = cx.config.target_ruby >= 3.0;
    if ruby3 {
        let string = match &node {
            Node::StringNode { .. } | Node::SourceFileNode { .. } => true,
            Node::InterpolatedStringNode { .. } => uninterpolated(node.as_interpolated_string_node().unwrap().parts()),
            _ => false,
        };
        if string && fsl::enabled(cx) {
            return true;
        }
        matches!(
            node,
            Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. } | Node::RangeNode { .. }
        )
    } else {
        matches!(node, Node::StringNode { .. } | Node::InterpolatedStringNode { .. } | Node::SourceFileNode { .. })
            && fsl::enabled(cx)
    }
}

// Narrower than select_by::plain_send: a `&blk` pass is allowed, a literal block is not.
fn plain_send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_argument_node().is_some()))
}

fn is_number(node: &Node) -> bool {
    matches!(crate::helpers::literal_node::folded_kind(node), kind::INTEGER_NODE | kind::FLOAT_NODE)
}

fn operation_produces_immutable_object(node: &Node) -> bool {
    // (send _ {:count :length :size} ...) and its any_block form
    if let Some(c) = node.as_call_node()
        && !is_csend(&c)
        && matches!(method_name(&c), "count" | "length" | "size")
    {
        return true;
    }
    let Some(send) = paren_child(node).as_ref().and_then(plain_send) else { return false };
    let args = send_args(&send);
    let [arg] = args.as_slice() else { return false };
    let recv = send.receiver();
    match method_name(&send) {
        "==" | "===" | "!=" | "<=" | ">=" | "<" | ">" => true,
        op @ ("+" | "-" | "*" | "**" | "/" | "%" | "<<") => {
            recv.as_ref().is_some_and(is_number)
                || (op != "<<"
                    && is_number(arg)
                    && !recv.as_ref().is_some_and(|r| {
                        r.as_array_node().is_some() || (r.as_string_node().is_some() && !is_parser_dstr(r))
                    }))
        }
        _ => false,
    }
}

impl Cop for RedundantFreeze {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "freeze" || is_csend(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if !(operation_produces_immutable_object(&recv) || immutable_literal(cx, recv)) {
            return;
        }
        let (start, end) = send_range(&call);
        let dot = call.call_operator_loc().map(|l| (l.start_offset(), l.end_offset()));
        let sel = call.message_loc().map(|l| (l.start_offset(), l.end_offset()));
        cx.add_fix(start, end, "Do not freeze immutable objects, as freezing them has no effect.", |c| {
            if let Some((s, e)) = dot {
                c.remove(s, e);
            }
            if let Some((s, e)) = sel {
                c.remove(s, e);
            }
        });
    }
}
