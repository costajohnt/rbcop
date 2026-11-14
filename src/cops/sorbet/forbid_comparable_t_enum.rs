use crate::helpers::sorbet_t_enum::in_t_enum_class;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidComparableTEnum";
pub static COP: ForbidComparableTEnum = ForbidComparableTEnum;
pub struct ForbidComparableTEnum;

impl Cop for ForbidComparableTEnum {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !matches!(call.name().as_slice(), b"include" | b"prepend") {
            return;
        }
        let comparable = |a: &Node| a.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Comparable");
        if matches!(send_args(&call).as_slice(), [a] if comparable(a)) && in_t_enum_class(cx.ancestors) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Do not use `T::Enum` as a comparable object because of significant performance overhead.");
        }
    }
}
