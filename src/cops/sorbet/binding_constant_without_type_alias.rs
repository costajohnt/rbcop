use crate::helpers::parser_parent::is_top_const;
use crate::helpers::sorbet_rbs_assertion::no_literal_block;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/BindingConstantWithoutTypeAlias";
pub static COP: BindingConstantWithoutTypeAlias = BindingConstantWithoutTypeAlias;
pub struct BindingConstantWithoutTypeAlias;

const MSG: &str =
    "It looks like you're trying to bind a type to a constant. To do this, you must alias the type using `T.type_alias`.";
const WITHOUT_BLOCK_MSG: &str = "It looks like you're using the old `T.type_alias` syntax. `T.type_alias` now expects a block.Run Sorbet with the options \"--autocorrect --error-white-list=5043\" to automatically upgrade to the new syntax.";

/// A parser `send` (not csend, no literal block) on `(const {nil? cbase} :T)`.
fn t_send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node()
        .filter(|c| !is_csend(c) && no_literal_block(c) && c.receiver().is_some_and(|r| is_top_const(&r, "T")))
}

/// rubocop-ast `receiver` of a send/csend, or of a block's call (`(any_block (call $_ ...) ...)`).
fn receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    node.as_call_node()?.receiver()
}

/// A parser `send` node: a call without a literal block, not `&.`.
fn is_send(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| !is_csend(&c) && no_literal_block(&c))
}

impl Cop for BindingConstantWithoutTypeAlias {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let expression = match node {
            Node::ConstantWriteNode { .. } => node.as_constant_write_node().unwrap().value(),
            _ => node.as_constant_path_write_node().unwrap().value(),
        };
        let (s, e) = helpers::node_types::range(&expression);
        // (send (const {nil? cbase} :T) :type_alias $_)
        if let Some(call) = t_send(&expression).filter(|c| method_name(c) == "type_alias")
            && let [ty] = send_args(&call).as_slice()
        {
            let replacement = format!("T.type_alias {{ {} }}", node_text(ty));
            return cx.add_fix(s, e, WITHOUT_BLOCK_MSG, |c| c.replace(s, e, &replacement));
        }
        // (block (send (const {nil? cbase} :T) :type_alias) ...)
        if let Some(call) = expression.as_call_node()
            && method_name(&call) == "type_alias"
            && !is_csend(&call)
            && send_args(&call).is_empty()
            && call.receiver().is_some_and(|r| is_top_const(&r, "T"))
            && call.block().and_then(|b| b.as_block_node()).is_some_and(|b| helpers::node_types::plain_block_params(b.parameters()))
        {
            return;
        }
        // send_leaf: `node = node.receiver while node&.receiver&.send_type?`
        let mut leaf = expression;
        while let Some(r) = receiver(&leaf).filter(is_send) {
            leaf = r;
        }
        let requires = t_send(&leaf).is_some_and(|c| {
            matches!(method_name(&c), "all" | "any" | "class_of" | "nilable" | "noreturn" | "proc" | "self_type" | "untyped")
        });
        if requires {
            let replacement = format!("T.type_alias {{ {} }}", cx.src.slice(s, e));
            cx.add_fix(s, e, MSG, |c| c.replace(s, e, &replacement));
        }
    }
}
