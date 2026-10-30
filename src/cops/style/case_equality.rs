use crate::prelude::*;

pub const NAME: &str = "Style/CaseEquality";
pub static COP: CaseEquality = CaseEquality;
pub struct CaseEquality;

fn is_const(node: &Node) -> bool {
    node.as_constant_read_node().is_some() || node.as_constant_path_node().is_some()
}

/// rubocop-ast `numeric_type?`: int, float, rational or complex literal.
fn is_numeric(node: &Node) -> bool {
    matches!(kind::of(node), kind::INTEGER_NODE | kind::FLOAT_NODE | kind::RATIONAL_NODE | kind::IMAGINARY_NODE)
}

/// (send (self) :class)
fn self_class(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == "class"
            && c.receiver().is_some_and(|r| r.as_self_node().is_some())
            && c.arguments().is_none()
            && c.block().is_none()
    })
}

/// `requires_parentheses?`
fn requires_parentheses(node: &Node) -> bool {
    if matches!(
        node,
        Node::AndNode { .. }
            | Node::OrNode { .. }
            | Node::IfNode { .. }
            | Node::UnlessNode { .. }
            | Node::RangeNode { .. }
    ) || helpers::node_types::is_assignment(node)
    {
        return true;
    }
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()) && helpers::node_types::is_operator_method(method_name(&c))
    })
}

impl Cop for CaseEquality {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "===" {
            return;
        }
        let args = send_args(&call);
        let [rhs] = args.as_slice() else { return };
        // `lhs.regexp_type?` on a nil receiver raises: no offense.
        let Some(lhs) = call.receiver() else { return };
        if is_const(&lhs) && cx.cfg.bool("AllowOnConstant") == Some(true) {
            return;
        }
        if self_class(&lhs) && cx.cfg.bool("AllowOnSelfClass") == Some(true) {
            return;
        }
        if matches!(lhs, Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }) {
            return;
        }
        if is_const(&lhs) {
            let short = helpers::const_name::const_name(&lhs).unwrap_or_default();
            let short = short.rsplit("::").next().unwrap_or("");
            if !short.chars().any(char::is_lowercase) {
                return;
            }
        }
        let paren = |n: &Node| {
            if requires_parentheses(n) { format!("({})", node_text(n)) } else { node_text(n).to_string() }
        };
        let replacement = if let Some(p) = lhs.as_parentheses_node() {
            let first = p.body().and_then(|b| match b.as_statements_node() {
                Some(s) => s.body().iter().next(),
                None => Some(b),
            });
            first.and_then(|f| f.as_range_node()).map(|range| {
                // cover_semantics?: `Range#===` is `cover?` since Ruby 2.7, which `include?`
                // matches only for numeric ranges.
                let numeric = [range.left(), range.right()].iter().flatten().all(is_numeric);
                let method = if cx.config.target_ruby >= 2.7 && !numeric { "cover?" } else { "include?" };
                format!("{}.{method}({})", node_text(&lhs), node_text(rhs))
            })
        } else if is_const(&lhs) || self_class(&lhs) {
            Some(format!("{}.is_a?({})", paren(rhs), node_text(&lhs)))
        } else {
            None
        };
        let (start, end) = send_range(&call);
        let sel = call.message_loc().unwrap();
        cx.add_fix(sel.start_offset(), sel.end_offset(), "Avoid the use of the case equality operator `===`.", |c| {
            if let Some(r) = replacement {
                c.replace(start, end, &r);
            }
        });
    }
}
