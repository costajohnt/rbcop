use crate::helpers::code_length::{Measured, check, global_const};
use crate::prelude::*;

pub const NAME: &str = "Metrics/ClassLength";
pub static COP: ClassLength = ClassLength;
pub struct ClassLength;

fn msg(len: usize, max: i64) -> String {
    format!("Class has too many lines. [{len}/{max}]")
}

/// The value a constant assignment (or a shorthand / multiple assignment of a constant) assigns.
fn casgn_value<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    macro_rules! value {
        ($($as:ident),*) => {
            $(if let Some(n) = node.$as() {
                return Some(n.value());
            })*
        };
    }
    value!(
        as_constant_write_node,
        as_constant_path_write_node,
        as_constant_or_write_node,
        as_constant_and_write_node,
        as_constant_operator_write_node,
        as_constant_path_or_write_node,
        as_constant_path_and_write_node,
        as_constant_path_operator_write_node
    );
    let m = node.as_multi_write_node()?;
    let casgn = m
        .lefts()
        .iter()
        .chain(crate::ast::parser_rest(m.rest()))
        .chain(m.rights().iter())
        .any(|t| matches!(kind::of(&t), kind::CONSTANT_TARGET_NODE | kind::CONSTANT_PATH_TARGET_NODE));
    casgn.then(|| m.value())
}

impl Cop for ClassLength {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CLASS_NODE,
            kind::SINGLETON_CLASS_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_AND_WRITE_NODE,
            kind::CONSTANT_OPERATOR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
            kind::CONSTANT_PATH_AND_WRITE_NODE,
            kind::CONSTANT_PATH_OPERATOR_WRITE_NODE,
            kind::MULTI_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        match kind::of(node) {
            kind::CLASS_NODE => check(cx, Measured::Classlike(node), range, range, msg),
            kind::SINGLETON_CLASS_NODE => {
                if cx.ancestors.iter().any(|a| kind::of(a) == kind::CLASS_NODE) {
                    return;
                }
                let body = node.as_singleton_class_node().unwrap().body();
                check(cx, Measured::Body { root: node, body }, range, range, msg);
            }
            _ => {
                // class_definition?: (any_block (send #global_const?({:Struct :Class}) :new ...) _ $_)
                let Some(value) = casgn_value(node) else { return };
                let Some(call) = value.as_call_node() else { return };
                let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
                if call.name().as_slice() != b"new"
                    || !call.receiver().is_some_and(|r| global_const(&r, &["Struct", "Class"]))
                {
                    return;
                }
                let l = value.location();
                let range = (l.start_offset(), l.end_offset());
                check(cx, Measured::Body { root: &value, body: block.body() }, range, range, msg);
            }
        }
    }
}
