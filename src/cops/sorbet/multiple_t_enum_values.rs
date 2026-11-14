use crate::helpers::sorbet_t_enum::{innermost_class, is_t_enum};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/MultipleTEnumValues";
pub static COP: MultipleTEnumValues = MultipleTEnumValues;
pub struct MultipleTEnumValues;

const MSG: &str = "`T::Enum` should have at least two values.";

fn is_casgn(node: &Node) -> bool {
    matches!(kind::of(node), kind::CONSTANT_WRITE_NODE | kind::CONSTANT_PATH_WRITE_NODE)
}

/// The casgn children of the block's parser `begin` body, else of the block itself
/// (send, args and a single-statement body).
fn casgn_count(body: &Node) -> usize {
    let stmts: Vec<Node> = match body.as_statements_node() {
        Some(s) => s.body().iter().collect(),
        None => return 0,
    };
    match stmts.as_slice() {
        [one] => match one.as_parentheses_node().and_then(|p| p.body()) {
            // `(a; b)` is a parser begin too.
            Some(inner) => inner.as_statements_node().map_or(0, |s| s.body().iter().filter(is_casgn).count()),
            None => usize::from(is_casgn(one)),
        },
        many => many.iter().filter(|n| is_casgn(n)).count(),
    }
}

impl Cop for MultipleTEnumValues {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(class) = node.as_class_node() {
            if is_t_enum(&class) && class.body().is_none() {
                let (s, e) = helpers::node_types::range(node);
                cx.add(s, e, MSG);
            }
            return;
        }
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let plain = block.parameters().is_none_or(|p| p.as_block_parameters_node().is_some());
        if !plain || call.receiver().is_some() || call.name().as_slice() != b"enums" || call.arguments().is_some() {
            return;
        }
        let Some(scope) = innermost_class(cx.ancestors).filter(is_t_enum) else { return };
        if block.body().is_none_or(|b| casgn_count(&b) < 2) {
            let (s, e) = helpers::node_types::range(&scope.as_node());
            cx.add(s, e, MSG);
        }
    }
}
