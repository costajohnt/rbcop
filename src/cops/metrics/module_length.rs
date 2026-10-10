use crate::helpers::code_length::{Measured, check, global_const};
use crate::prelude::*;

pub const NAME: &str = "Metrics/ModuleLength";
pub static COP: ModuleLength = ModuleLength;
pub struct ModuleLength;

fn msg(len: usize, max: i64) -> String {
    format!("Module has too many lines. [{len}/{max}]")
}

impl Cop for ModuleLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::MODULE_NODE, kind::CONSTANT_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        let Some(casgn) = node.as_constant_write_node() else {
            return check(cx, Measured::Classlike(node), range, range, msg);
        };
        // (casgn nil? _ (any_block (send (const {nil? cbase} :Module) :new) ...))
        let value = casgn.value();
        let Some(call) = value.as_call_node() else { return };
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if call.name().as_slice() != b"new"
            || call.arguments().is_some()
            || !call.receiver().is_some_and(|r| global_const(&r, &["Module"]))
        {
            return;
        }
        let name = casgn.name_loc();
        check(
            cx,
            Measured::Body { root: node, body: block.body() },
            range,
            (name.start_offset(), name.end_offset()),
            msg,
        );
    }
}
