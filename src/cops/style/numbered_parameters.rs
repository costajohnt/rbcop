use crate::prelude::*;

pub const NAME: &str = "Style/NumberedParameters";
pub static COP: NumberedParameters = NumberedParameters;
pub struct NumberedParameters;

impl Cop for NumberedParameters {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // parser `numblock`: the block (with its call, for a call) whose params are `_1`..`_9`.
        let (params, open, close) = if let Some(l) = node.as_lambda_node() {
            (l.parameters(), l.opening_loc(), l.closing_loc())
        } else {
            let Some(b) = node.as_call_node().unwrap().block().and_then(|b| b.as_block_node()) else { return };
            (b.parameters(), b.opening_loc(), b.closing_loc())
        };
        if !params.is_some_and(|p| p.as_numbered_parameters_node().is_some()) {
            return;
        }
        let loc = node.location();
        if cx.cfg.str("EnforcedStyle") == Some("disallow") {
            cx.add(loc.start_offset(), loc.end_offset(), "Avoid using numbered parameters.");
        } else if cx.src.line_of(open.start_offset()) != cx.src.line_of(close.start_offset()) {
            cx.add(loc.start_offset(), loc.end_offset(), "Avoid using numbered parameters for multi-line blocks.");
        }
    }
}
