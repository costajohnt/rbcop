use crate::prelude::*;

pub const NAME: &str = "Style/RedundantInterpolationUnfreeze";
pub static COP: RedundantInterpolationUnfreeze = RedundantInterpolationUnfreeze;
pub struct RedundantInterpolationUnfreeze;

const MSG: &str = "Don't unfreeze interpolated strings as they are already unfrozen.";

impl Cop for RedundantInterpolationUnfreeze {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_dstr, matched from the parent send: `dstr.+@`, `dstr.dup`, `String.new(dstr)`.
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let name = call.name().as_slice();
        let args = send_args(&call);
        let (dstr, start, end) = match (name, call.receiver()) {
            (b"+@" | b"dup", Some(recv)) if args.is_empty() => {
                let Some(sel) = call.message_loc() else { return };
                (recv, sel.start_offset(), sel.end_offset())
            }
            (b"new", Some(recv))
                if args.len() == 1
                    && recv.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"String") =>
            {
                let sel = call.message_loc().unwrap();
                (args.into_iter().next().unwrap(), call.location().start_offset(), sel.end_offset())
            }
            _ => return,
        };
        if !helpers::frozen_string_literal::interpolated_dstr(&dstr) {
            return;
        }
        let (send_start, send_end) = send_range(&call);
        let (ds, de) = (dstr.location().start_offset(), dstr.location().end_offset());
        // comment_outside_string?
        if cx.src.comments.iter().any(|c| send_start <= c.start && c.end <= send_end && !(ds <= c.start && c.end <= de)) {
            return;
        }
        // Only the parts around the string go, so an unfrozen string interpolated into it can be
        // corrected in the same pass.
        cx.add_fix(start, end, MSG, |c| {
            c.remove(send_start, ds);
            c.remove(de, send_end);
        });
    }
}
