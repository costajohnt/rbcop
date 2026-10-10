use crate::prelude::*;

pub const NAME: &str = "Lint/AmbiguousOperatorPrecedence";
pub static COP: AmbiguousOperatorPrecedence = AmbiguousOperatorPrecedence;
pub struct AmbiguousOperatorPrecedence;

const MSG: &str = "Wrap expressions with varying precedence with parentheses to avoid ambiguity.";
const PRECEDENCE: &[&[&str]] = &[&["**"], &["*", "/", "%"], &["+", "-"], &["<<", ">>"], &["&"], &["|", "^"], &["&&"], &["||"]];

fn precedence_of(name: &str) -> Option<usize> {
    PRECEDENCE.iter().position(|ops| ops.contains(&name))
}

/// Operator name of a send (method name) or and/or node (`&&`, `and`, ...).
fn operator_name<'a>(node: &Node<'a>) -> Option<&'a str> {
    if let Some(c) = node.as_call_node() {
        return (!is_csend(&c)).then(|| method_name(&c));
    }
    if let Some(a) = node.as_and_node() {
        return Some(text(&a.operator_loc()));
    }
    node.as_or_node().map(|o| text(&o.operator_loc()))
}

/// `operator?`: a send of one of the operators, or any and/or node.
fn precedence(node: &Node) -> Option<usize> {
    precedence_of(operator_name(node)?)
}

impl Cop for AmbiguousOperatorPrecedence {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors) else { return };
        let (start, end) = if let Some(call) = node.as_call_node() {
            if is_csend(&call) || call.opening_loc().is_some() {
                return;
            }
            let name = method_name(&call);
            let Some(own) = precedence_of(name) else { return };
            // operator?(parent) && greater_precedence?(node, parent)
            let parent_is_operator = parent.as_call_node().is_some_and(|p| !is_csend(&p) && precedence_of(method_name(&p)).is_some())
                || helpers::parser_parent::is_operator_keyword(parent);
            if !parent_is_operator || precedence(parent).is_none_or(|p| p <= own) {
                return;
            }
            send_range(&call)
        } else {
            if parent.as_or_node().is_none() {
                return;
            }
            let l = node.location();
            (l.start_offset(), l.end_offset())
        };
        cx.add_fix(start, end, MSG, |c| c.wrap(start, end, "(", ")"));
    }
}
