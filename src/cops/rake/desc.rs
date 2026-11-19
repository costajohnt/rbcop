use crate::helpers::node_types::plain_block_params;
use crate::helpers::rake::{is_receiverless, task_name};
use crate::prelude::*;

pub const NAME: &str = "Rake/Desc";
pub static COP: Desc = Desc;
pub struct Desc;

impl Cop for Desc {
    // `extend AutoCorrector`, but no corrector: the offense is not [Correctable].
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["task"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !is_receiverless(&call, "task") || task_with_desc(cx.ancestors, &call) {
            return;
        }
        if task_name(&call, cx.src.target_ruby).as_deref() == Some("default") {
            return;
        }
        // (send nil? :task (hash (pair _ $_))) with an array value: prerequisites only.
        if let [arg] = send_args(&call).as_slice() {
            let elements: Vec<Node> = match (arg.as_hash_node(), arg.as_keyword_hash_node()) {
                (Some(h), _) => h.elements().iter().collect(),
                (_, Some(h)) => h.elements().iter().collect(),
                _ => Vec::new(),
            };
            if let [pair] = elements.as_slice()
                && pair.as_assoc_node().is_some_and(|p| p.value().as_array_node().is_some())
            {
                return;
            }
        }
        let (s, e) = send_range(&call);
        cx.add_with(s, e, "Describe the task with `desc` method.", false);
    }
}

/// `task_with_desc?` in Prism terms. The task (with its block, when a plain one wraps it) has a parser
/// parent of type begin/block/kwbegin, else it counts as described. In a begin of two or more
/// statements the statement before it is checked; Ruby's `children[-1]` makes the first statement
/// look at the last one. As the only statement of a block (args before it), a kwbegin or parentheses
/// (itself before it), or the program (no parent) it is never described.
fn task_with_desc(ancestors: &[Node], call: &ruby_prism::CallNode) -> bool {
    // `task(:x) { _1 }`: the send's parent is a numblock/itblock.
    if call.block().and_then(|b| b.as_block_node()).is_some_and(|b| !plain_block_params(b.parameters())) {
        return true;
    }
    let n = ancestors.len();
    let Some(stmts) = ancestors.last().and_then(|p| p.as_statements_node()) else {
        return true;
    };
    let body: Vec<Node> = stmts.body().iter().collect();
    if body.len() >= 2 {
        let start = call.location().start_offset();
        let Some(i) = body.iter().position(|b| b.location().start_offset() == start) else { return true };
        let prev = &body[(i + body.len() - 1) % body.len()];
        return prev
            .as_call_node()
            .is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()) && method_name(&c) == "desc");
    }
    match n.checked_sub(2).map(|i| &ancestors[i]) {
        None | Some(Node::ProgramNode { .. }) | Some(Node::ParenthesesNode { .. }) | Some(Node::EmbeddedStatementsNode { .. }) => false,
        Some(p @ Node::BlockNode { .. }) => !plain_block_params(p.as_block_node().unwrap().parameters()),
        Some(p @ Node::LambdaNode { .. }) => !plain_block_params(p.as_lambda_node().unwrap().parameters()),
        Some(p @ Node::BeginNode { .. }) => {
            let b = p.as_begin_node().unwrap();
            b.rescue_clause().is_some() || b.ensure_clause().is_some()
        }
        Some(_) => true,
    }
}
