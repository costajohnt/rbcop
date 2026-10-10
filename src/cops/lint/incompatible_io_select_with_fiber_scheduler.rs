use crate::prelude::*;

pub const NAME: &str = "Lint/IncompatibleIoSelectWithFiberScheduler";
pub static COP: IncompatibleIoSelectWithFiberScheduler = IncompatibleIoSelectWithFiberScheduler;
pub struct IncompatibleIoSelectWithFiberScheduler;

/// `node.children.empty?` for the parser node of an expression.
fn childless(node: &Node) -> bool {
    match node {
        Node::NilNode { .. }
        | Node::TrueNode { .. }
        | Node::FalseNode { .. }
        | Node::SelfNode { .. }
        | Node::RedoNode { .. }
        | Node::RetryNode { .. }
        | Node::ForwardingSuperNode { .. } => true,
        Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().is_empty(),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().is_empty(),
        Node::ParenthesesNode { .. } => node.as_parentheses_node().unwrap().body().is_none(),
        _ => false,
    }
}

fn first_value<'pr>(n: Option<&Node<'pr>>) -> Option<Node<'pr>> {
    n.and_then(|n| n.as_array_node()).and_then(|a| a.elements().iter().next())
}

/// `single_io_array?`: an array literal with one non-splat value.
fn single_io_array(node: Option<&Node>) -> bool {
    node.and_then(|n| n.as_array_node()).is_some_and(|a| {
        let values: Vec<Node> = a.elements().iter().collect();
        values.len() == 1 && values[0].as_splat_node().is_none()
    })
}

fn scheduler_compatible(io1: Option<&Node>, io2: Option<&Node>) -> bool {
    if !single_io_array(io1) {
        return false;
    }
    match io2 {
        Some(n) => match n.as_array_node() {
            Some(a) => a.elements().is_empty(),
            None => n.as_nil_node().is_some(),
        },
        None => true,
    }
}

impl Cop for IncompatibleIoSelectWithFiberScheduler {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "select" {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "IO")) {
            return;
        }
        let args = send_args(&call);
        let (read, write, excepts, timeout) = (args.first(), args.get(1), args.get(2), args.get(3));
        if excepts.is_some_and(|e| !childless(e)) {
            return;
        }
        if !scheduler_compatible(read, write) && !scheduler_compatible(write, read) {
            return;
        }
        let timeout_argument = timeout.map(|t| format!("({})", node_text(t))).unwrap_or_default();
        let preferred = match first_value(read) {
            Some(v) => format!("{}.wait_readable{timeout_argument}", node_text(&v)),
            None => {
                let Some(v) = first_value(write) else { return };
                format!("{}.wait_writable{timeout_argument}", node_text(&v))
            }
        };
        let (start, end) = send_range(&call);
        let msg = format!("Use `{preferred}` instead of `{}`.", &cx.src.text[start..end]);
        let assignment_parent = helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(is_assignment);
        cx.add_fix(start, end, msg, |c| {
            if !assignment_parent {
                c.replace(start, end, &preferred);
            }
        });
    }
}
