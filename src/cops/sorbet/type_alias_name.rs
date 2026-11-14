use crate::helpers::sorbet_rbs_assertion::is_t_call;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/TypeAliasName";
pub static COP: TypeAliasName = TypeAliasName;
pub struct TypeAliasName;

impl Cop for TypeAliasName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (casgn _ /_/ (block (send (const nil? :T) :type_alias) ...))
        let (name, value) = match node {
            Node::ConstantWriteNode { .. } => {
                let w = node.as_constant_write_node().unwrap();
                (w.name().as_slice(), w.value())
            }
            _ => {
                let w = node.as_constant_path_write_node().unwrap();
                let Some(n) = w.target().name() else { return };
                (n.as_slice(), w.value())
            }
        };
        if !name.contains(&b'_') {
            return;
        }
        let Some(call) = value.as_call_node() else { return };
        let block = call.block().and_then(|b| b.as_block_node());
        if !block.is_some_and(|b| helpers::node_types::plain_block_params(b.parameters()))
            || !is_t_call(&call, "type_alias", Some(0))
        {
            return;
        }
        let (s, e) = helpers::node_types::range(node);
        cx.add(s, e, "Type alias constant name should be in CamelCase");
    }
}
