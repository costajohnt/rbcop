//! rubocop-ast node-type predicates over Prism nodes.

use crate::prelude::*;

/// rubocop-ast `assignment?`: (lvasgn ivasgn cvasgn gvasgn casgn masgn op_asgn or_asgn and_asgn),
/// plus `setter_method?` (its alias on send nodes: `a.b = x`, `a[i] = x`).
pub fn is_assignment(node: &Node) -> bool {
    if let Some(c) = node.as_call_node() {
        return c.equal_loc().is_some();
    }
    ASSIGNMENTS.contains(&kind::of(node))
}

const ASSIGNMENTS: &[usize] = &[
    kind::CALL_AND_WRITE_NODE,
    kind::CALL_OPERATOR_WRITE_NODE,
    kind::CALL_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_AND_WRITE_NODE,
    kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    kind::CLASS_VARIABLE_OR_WRITE_NODE,
    kind::CLASS_VARIABLE_WRITE_NODE,
    kind::CONSTANT_AND_WRITE_NODE,
    kind::CONSTANT_OPERATOR_WRITE_NODE,
    kind::CONSTANT_OR_WRITE_NODE,
    kind::CONSTANT_PATH_AND_WRITE_NODE,
    kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
    kind::CONSTANT_PATH_OR_WRITE_NODE,
    kind::CONSTANT_PATH_WRITE_NODE,
    kind::CONSTANT_WRITE_NODE,
    kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    kind::GLOBAL_VARIABLE_WRITE_NODE,
    kind::INDEX_AND_WRITE_NODE,
    kind::INDEX_OPERATOR_WRITE_NODE,
    kind::INDEX_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    kind::INSTANCE_VARIABLE_WRITE_NODE,
    kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    kind::LOCAL_VARIABLE_WRITE_NODE,
    kind::MULTI_WRITE_NODE,
];

const OPERATOR_METHODS: &[&str] = &[
    "|", "^", "&", "<=>", "==", "===", "=~", ">", ">=", "<", "<=", "<<", ">>", "+", "-", "*", "/", "%", "**", "~",
    "+@", "-@", "!@", "~@", "[]", "[]=", "!", "!=", "!~", "`",
];

/// The name of a parser `lvar`: a local variable read, or `it` in an `it` block (Prism's
/// ItLocalVariableReadNode is the parser gem's `(lvar :it)`).
pub fn lvar_name<'pr>(node: &Node<'pr>) -> Option<&'pr [u8]> {
    match node {
        Node::LocalVariableReadNode { .. } => Some(node.as_local_variable_read_node().unwrap().name().as_slice()),
        Node::ItLocalVariableReadNode { .. } => Some(b"it"),
        _ => None,
    }
}

/// An undeclared `_1`..`_9` read below Ruby 2.7: Prism's LocalVariableReadNode is the parser
/// gem's `(send nil :_1)` there (`syntax::legacy_numbered_sends`).
pub fn is_legacy_numbered_read(cx: &Cx, node: &Node) -> bool {
    node.as_local_variable_read_node().is_some_and(|l| cx.src.numbered_send(l.location().start_offset()))
}

/// `operator_method?`
pub fn is_operator_method(name: &str) -> bool {
    OPERATOR_METHODS.contains(&name)
}

/// `binary_operation?`: an operator method whose selector does not start the expression
/// (`a + b`, `a[1]`, `a.+(b)`).
pub fn is_binary_operation(call: &ruby_prism::CallNode) -> bool {
    call.message_loc().is_some_and(|sel| {
        is_operator_method(method_name(call)) && sel.start_offset() != call.location().start_offset()
    })
}

/// `unary_operation?`: an operator method whose selector starts the expression (`-a`, `!a`, `not a`).
pub fn is_unary_operation(call: &ruby_prism::CallNode) -> bool {
    call.message_loc().is_some_and(|sel| {
        is_operator_method(method_name(call)) && sel.start_offset() == call.location().start_offset()
    })
}

/// The literal block (parser `block`/`numblock`/`itblock`) of a call, `super(...)` or zsuper: the
/// Prism nodes that own a BlockNode.
pub fn literal_block<'pr>(node: &Node<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
    let block = match node {
        Node::CallNode { .. } => node.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => node.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    };
    block.and_then(|b| b.as_block_node())
}

/// A block's or lambda's parameters make it a parser `block`: not numbered (`numblock`) or `it`
/// (`itblock`).
pub fn plain_block_params(params: Option<Node>) -> bool {
    !matches!(params.map(|p| kind::of(&p)), Some(kind::NUMBERED_PARAMETERS_NODE | kind::IT_PARAMETERS_NODE))
}

/// `lambda_or_proc?`: `-> {}`, `lambda {}`, `proc {}`, `Proc.new {}` / `Proc.new`.
pub fn lambda_or_proc(node: &Node) -> bool {
    if node.as_lambda_node().is_some() {
        return true;
    }
    let Some(call) = node.as_call_node() else { return false };
    let block = call.block().and_then(|b| b.as_block_node());
    let name = method_name(&call);
    // BlockNode#lambda? (any block, numbered/`it` included): the block's send is named `lambda`,
    // whatever its receiver or arguments.
    if block.is_some() && name == "lambda" {
        return true;
    }
    // `proc?` matches a parser `block` only: a block with numbered parameters (`numblock`) or `it`
    // (`itblock`) is neither `proc { _1 }` nor `Proc.new { it }`.
    let plain_block = block.as_ref().is_some_and(|b| plain_block_params(b.parameters()));
    // Its patterns take a bare `send` (no arguments; `&blk` is a parser argument).
    if is_csend(&call) || call.arguments().is_some() || call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
        return false;
    }
    match call.receiver() {
        None => name == "proc" && plain_block,
        Some(r) => {
            let proc_const = match &r {
                Node::ConstantReadNode { .. } => node_text(&r) == "Proc",
                Node::ConstantPathNode { .. } => {
                    let p = r.as_constant_path_node().unwrap();
                    p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Proc")
                }
                _ => false,
            };
            proc_const && name == "new" && (block.is_none() || plain_block)
        }
    }
}
