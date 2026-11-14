use crate::prelude::*;

pub const NAME: &str = "Sorbet/ImplicitConversionMethod";
pub static COP: ImplicitConversionMethod = ImplicitConversionMethod;
pub struct ImplicitConversionMethod;

const MSG: &str = "Avoid implicit conversion methods, as Sorbet does not support them. Explicity convert to the desired type instead.";

fn implicit(name: &[u8]) -> bool {
    matches!(name, b"to_ary" | b"to_int" | b"to_hash" | b"to_str")
}

/// A `sym` node whose value is one of IMPLICIT_CONVERSION_METHODS.
fn implicit_sym(node: &Node) -> bool {
    node.as_symbol_node().is_some_and(|s| implicit(s.unescaped()))
}

impl Cop for ImplicitConversionMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::ALIAS_METHOD_NODE, kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let target = match node {
            Node::AliasMethodNode { .. } => Some(node.as_alias_method_node().unwrap().new_name()).filter(implicit_sym),
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let empty = def.parameters().is_none_or(|p| parameter_nodes(&p).is_empty());
                (implicit(def.name().as_slice()) && empty).then(|| node.as_def_node().unwrap().as_node())
            }
            _ => {
                // `node.first_argument.value`: RuboCop crashes on a first argument without `value`
                // (or none); a string's value never matches. Neither is an offense.
                let call = node.as_call_node().unwrap();
                if method_name(&call) != "alias_method" || is_csend(&call) {
                    return;
                }
                send_args(&call).into_iter().next().filter(implicit_sym)
            }
        };
        if let Some(t) = target {
            let (s, e) = helpers::node_types::range(&t);
            cx.add(s, e, MSG);
        }
    }
}
