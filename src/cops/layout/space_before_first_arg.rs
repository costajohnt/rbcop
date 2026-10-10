use crate::helpers::preceding_following_alignment::{Alignment, Data, Range};
use crate::helpers::range_help::with_surrounding_space;
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceBeforeFirstArg";
pub static COP: SpaceBeforeFirstArg = SpaceBeforeFirstArg;
pub struct SpaceBeforeFirstArg;

const MSG: &str = "Put one space between the method name and the first argument.";

impl Cop for SpaceBeforeFirstArg {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(first) = send_args(&call).into_iter().next() else { return };
        if helpers::node_types::is_operator_method(method_name(&call))
            || call.equal_loc().is_some()
            || call.opening_loc().is_some_and(|l| l.as_slice() == b"(")
        {
            return;
        }
        let Some(selector) = call.message_loc() else { return };
        let src = cx.src;
        let (arg_s, arg_e) = (first.location().start_offset(), first.location().end_offset());
        let (space_s, _) = with_surrounding_space(src, arg_s, arg_e, true, false, true);
        if src.slice(space_s, arg_s).chars().count() == 1 {
            return;
        }
        if selector.end_offset() != arg_s {
            if src.line_of(arg_s) != src.line_of(node.location().start_offset()) {
                return;
            }
            if cx.cfg.bool("AllowForAlignment").unwrap_or(true) {
                let program = &cx.ancestors[0];
                let data = cx.state::<Option<Data>>().get_or_insert_with(|| Data::new(src, program));
                let al = Alignment { src, d: data };
                if al.aligned_with_something(Range { start: arg_s, end: arg_e }) {
                    return;
                }
            }
        }
        cx.add_fix(space_s, arg_s, MSG, |c| c.replace(space_s, arg_s, " "));
    }
}
