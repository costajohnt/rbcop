use crate::helpers::block_args::{Single, single_body, single_var};
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantMinMaxBy";
pub static COP: RedundantMinMaxBy = RedundantMinMaxBy;
pub struct RedundantMinMaxBy;

impl Cop for RedundantMinMaxBy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        let replacement = match method {
            "max_by" => "max",
            "min_by" => "min",
            "minmax_by" => "minmax",
            _ => return,
        };
        if call.arguments().is_some() {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(sel) = call.message_loc() else { return };
        let Some((single, var)) = single_var(&block) else { return };
        let Some(body) = single_body(&block) else { return };
        let body_is_var = match single {
            Single::It => body.as_it_local_variable_read_node().is_some(),
            _ => body.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == var),
        };
        if !body_is_var {
            return;
        }
        let msg = match single {
            Single::Arg => {
                let var = String::from_utf8_lossy(var);
                format!("Use `{replacement}` instead of `{method} {{ |{var}| {var} }}`.")
            }
            Single::Numbered => format!("Use `{replacement}` instead of `{method} {{ _1 }}`."),
            Single::It => format!("Use `{replacement}` instead of `{method} {{ it }}`."),
        };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        cx.add_fix(start, end, msg, |c| c.replace(start, end, replacement));
    }
}
