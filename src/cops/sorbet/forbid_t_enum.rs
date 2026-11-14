use crate::helpers::sorbet_signature::is_t_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTEnum";
pub static COP: ForbidTEnum = ForbidTEnum;
pub struct ForbidTEnum;

impl Cop for ForbidTEnum {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if node.as_class_node().unwrap().superclass().is_some_and(|s| is_t_const(&s, "Enum")) {
            let (s, e) = helpers::node_types::range(node);
            cx.add(s, e, "Using `T::Enum` is deprecated in this codebase.");
        }
    }
}
