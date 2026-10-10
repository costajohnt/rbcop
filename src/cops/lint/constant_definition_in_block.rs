use crate::prelude::*;

pub const NAME: &str = "Lint/ConstantDefinitionInBlock";
pub static COP: ConstantDefinitionInBlock = ConstantDefinitionInBlock;
pub struct ConstantDefinitionInBlock;

impl Cop for ConstantDefinitionInBlock {
    fn kinds(&self) -> &'static [usize] {
        // casgn without a scope (`X = 1`), class, module.
        &[kind::CONSTANT_WRITE_NODE, kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `{^any_block [^begin ^^any_block]}`: the parent is a block, or a `begin` whose parent is.
        let parents = parser_parents(cx.ancestors);
        let block = match parents.as_slice() {
            [b, ..] if is_block(&cx.ancestors[*b]) => *b,
            [p, b, ..] if is_begin(&cx.ancestors[*p]) && is_block(&cx.ancestors[*b]) => *b,
            _ => return,
        };
        let name = block_method_name(cx.ancestors, block);
        if helpers::allowed::allowed_method(cx.cfg, name) {
            return;
        }
        let loc = node.location();
        cx.add(loc.start_offset(), loc.end_offset(), "Do not define constants this way within a block.");
    }
}

fn is_block(n: &Node) -> bool {
    matches!(n, Node::BlockNode { .. } | Node::LambdaNode { .. })
}

/// Parser `begin`: a multi-statement body or parentheses.
fn is_begin(n: &Node) -> bool {
    matches!(n, Node::StatementsNode { .. } | Node::ParenthesesNode { .. })
}

/// Indexes of `ancestors` that are parser nodes, nearest first: single-statement bodies, the
/// statements of parentheses and other Prism-only wrappers are skipped.
fn parser_parents(ancestors: &[Node]) -> Vec<usize> {
    (0..ancestors.len())
        .rev()
        .filter(|&i| match &ancestors[i] {
            Node::StatementsNode { .. } => {
                let paren_body = i > 0 && ancestors[i - 1].as_parentheses_node().is_some();
                ancestors[i].as_statements_node().unwrap().body().iter().count() > 1 && !paren_body
            }
            Node::ProgramNode { .. } | Node::ElseNode { .. } | Node::ArgumentsNode { .. } => false,
            _ => true,
        })
        .collect()
}

/// `method_name` of the parser block at `ancestors[i]`.
fn block_method_name<'a>(ancestors: &'a [Node], i: usize) -> &'a str {
    if ancestors[i].as_lambda_node().is_some() {
        return "lambda";
    }
    match i.checked_sub(1).and_then(|p| ancestors[p].as_call_node()) {
        Some(c) => method_name(&c),
        None => "super",
    }
}
