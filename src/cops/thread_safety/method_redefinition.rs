use crate::helpers::node_types::literal_block;
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/MethodRedefinition";
pub static COP: MethodRedefinition = MethodRedefinition;
pub struct MethodRedefinition;

impl Cop for MethodRedefinition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "remove_method" || literal_block(node).is_some() {
            return;
        }
        // `node.right_sibling`: the next statement of a parser `begin` (several statements).
        // ponytail: other parents (`remove_method(:a) and def a; end`) are not followed.
        let Some(stmts) = cx.parent().and_then(|p| p.as_statements_node()) else { return };
        let body: Vec<Node> = stmts.body().iter().collect();
        let start = node.location().start_offset();
        let Some(i) = body.iter().position(|n| n.location().start_offset() == start) else { return };
        if body.len() < 2 {
            return;
        }
        let Some(def) = body.get(i + 1).and_then(|n| n.as_def_node()).filter(|d| d.receiver().is_none()) else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let value = match arg {
            Node::SymbolNode { .. } if !is_parser_dsym(arg, cx.config.target_ruby) => arg.as_symbol_node().unwrap().unescaped().to_vec(),
            Node::StringNode { .. } if !is_parser_dstr(arg) => arg.as_string_node().unwrap().unescaped().to_vec(),
            _ => return,
        };
        if def.name().as_slice() != value.as_slice() {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Do not use `remove_method` followed by method definition.");
    }
}
