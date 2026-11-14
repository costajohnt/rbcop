use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTAssertType";
pub static COP: ForbidTAssertType = ForbidTAssertType;
pub struct ForbidTAssertType;

impl Cop for ForbidTAssertType {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (const {nil? cbase} :T) :assert_type! ...)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "assert_type!" || !call.receiver().is_some_and(|r| is_top_const(&r, "T")) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Do not use `T.assert_type!`.");
    }
}
