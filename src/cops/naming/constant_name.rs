use crate::prelude::*;

pub const NAME: &str = "Naming/ConstantName";
pub static COP: ConstantName = ConstantName;
pub struct ConstantName;

static SNAKE_CASE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^[\p{Nd}\p{Uppercase}_]+$").unwrap());

impl Cop for ConstantName {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CONSTANT_TARGET_NODE,
            kind::CONSTANT_PATH_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (name location, the value RuboCop sees: casgn expression, or the or_asgn's value)
        let (name_loc, value) = match node {
            Node::ConstantWriteNode { .. } => {
                let n = node.as_constant_write_node().unwrap();
                (n.name_loc(), Some(n.value()))
            }
            Node::ConstantPathWriteNode { .. } => {
                let n = node.as_constant_path_write_node().unwrap();
                (n.target().name_loc(), Some(n.value()))
            }
            Node::ConstantOrWriteNode { .. } => {
                let n = node.as_constant_or_write_node().unwrap();
                (n.name_loc(), Some(n.value()))
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let n = node.as_constant_path_or_write_node().unwrap();
                (n.target().name_loc(), Some(n.value()))
            }
            Node::ConstantOperatorWriteNode { .. } => (node.as_constant_operator_write_node().unwrap().name_loc(), None),
            Node::ConstantAndWriteNode { .. } => (node.as_constant_and_write_node().unwrap().name_loc(), None),
            Node::ConstantPathOperatorWriteNode { .. } => {
                (node.as_constant_path_operator_write_node().unwrap().target().name_loc(), None)
            }
            Node::ConstantPathAndWriteNode { .. } => {
                (node.as_constant_path_and_write_node().unwrap().target().name_loc(), None)
            }
            Node::ConstantTargetNode { .. } => (node.location(), None),
            Node::ConstantPathTargetNode { .. } => (node.as_constant_path_target_node().unwrap().name_loc(), None),
            _ => return,
        };
        if value.as_ref().is_some_and(allowed_assignment) || SNAKE_CASE.is_match(text(&name_loc)) {
            return;
        }
        cx.add(name_loc.start_offset(), name_loc.end_offset(), "Use SCREAMING_SNAKE_CASE for constants.");
    }
}

fn allowed_assignment(value: &Node) -> bool {
    match value {
        // parser `block` (not numblock/itblock), `const`, `casgn`
        Node::LambdaNode { .. } => plain_block_params(value.as_lambda_node().unwrap().parameters()),
        Node::ConstantReadNode { .. }
        | Node::ConstantPathNode { .. }
        | Node::ConstantWriteNode { .. }
        | Node::ConstantPathWriteNode { .. } => true,
        Node::CallNode { .. } => {
            let call = value.as_call_node().unwrap();
            if let Some(b) = call.block().and_then(|b| b.as_block_node()) {
                return plain_block_params(b.parameters());
            }
            // allowed_method_call_on_rhs?: send (not csend) without a literal receiver
            !is_csend(&call) && call.receiver().is_none_or(|r| !literal_receiver(&r))
        }
        Node::IfNode { .. } | Node::UnlessNode { .. } => if_branches(value).iter().any(|b| {
            b.as_ref().is_some_and(|b| matches!(b, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }))
        }),
        _ => false,
    }
}

fn plain_block_params(params: Option<Node>) -> bool {
    !matches!(params, Some(Node::NumberedParametersNode { .. } | Node::ItParametersNode { .. }))
}

fn literal_receiver(recv: &Node) -> bool {
    if helpers::literal_node::literal(recv) {
        return true;
    }
    // (begin literal?)
    recv.as_parentheses_node()
        .and_then(|p| p.body())
        .and_then(|b| b.as_statements_node())
        .is_some_and(|s| s.body().len() == 1 && s.body().iter().all(|n| helpers::literal_node::literal(&n)))
}

/// A statements body as the parser node (single statement) or None for begin/empty.
fn single<'pr>(stmts: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    let s = stmts?;
    let body = s.body();
    if body.len() == 1 { body.iter().next() } else { None }
}

/// IfNode#branches (single-statement bodies only matter: a `begin` is never `const`).
fn if_branches<'pr>(node: &Node<'pr>) -> Vec<Option<Node<'pr>>> {
    match node {
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let mut out = vec![single(n.statements())];
            match n.subsequent() {
                Some(Node::IfNode { .. }) => out.extend(if_branches(&n.subsequent().unwrap())),
                Some(e) => out.push(single(e.as_else_node().unwrap().statements())),
                None => {}
            }
            out
        }
        _ => {
            let n = node.as_unless_node().unwrap();
            let mut out = vec![single(n.statements())];
            if let Some(e) = n.else_clause() {
                out.push(single(e.statements()));
            }
            out
        }
    }
}
