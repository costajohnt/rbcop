use crate::prelude::*;

pub const NAME: &str = "Rails/Output";
pub static COP: Output = Output;
pub struct Output;

impl Cop for Output {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        let output = call.receiver().is_none() && matches!(name, "ap" | "p" | "pp" | "pretty_print" | "print" | "puts");
        let io = matches!(name, "binwrite" | "syswrite" | "write" | "write_nonblock")
            && call.receiver().is_some_and(|r| {
                r.as_global_variable_read_node().is_some_and(|g| matches!(g.name().as_slice(), b"$stdout" | b"$stderr"))
                    || helpers::parser_parent::is_top_const(&r, "STDOUT")
                    || helpers::parser_parent::is_top_const(&r, "STDERR")
            });
        if !output && !io {
            return;
        }
        // node.parent&.call_type? || node.block_node
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let anc = cx.ancestors;
        if helpers::rails::parent_index(anc).is_some_and(|i| anc[i].as_call_node().is_some()) {
            return;
        }
        let args = send_args(&call);
        if args.iter().any(|a| helpers::rails::hash_elements(a).is_some() || a.as_block_argument_node().is_some()) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = if call.receiver().is_some() {
            (node.location().start_offset(), sel.end_offset())
        } else {
            (sel.start_offset(), sel.end_offset())
        };
        cx.add_fix(s, e, "Do not write to stdout. Use Rails's logger if you want to log.", |c| {
            c.replace(s, e, "Rails.logger.debug")
        });
    }
}
