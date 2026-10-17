use crate::prelude::*;

pub const NAME: &str = "RSpec/Output";
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
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        let name = method_name(&call);
        let output = match call.receiver() {
            // (send nil? KERNEL_METHODS ...)
            None => matches!(name, "ap" | "p" | "pp" | "pretty_print" | "print" | "puts"),
            // (send {(gvar #match_gvar?) (const {nil? cbase} {:STDOUT :STDERR})} IO_METHODS ...)
            Some(r) => {
                matches!(name, "binwrite" | "syswrite" | "write" | "write_nonblock")
                    && (r.as_global_variable_read_node().is_some_and(|g| matches!(g.name().as_slice(), b"$stdout" | b"$stderr"))
                        || helpers::parser_parent::is_top_const(&r, "STDOUT")
                        || helpers::parser_parent::is_top_const(&r, "STDERR"))
            }
        };
        if !output {
            return;
        }
        // node.parent&.call_type?
        if helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| p.as_call_node().is_some()) {
            return;
        }
        if send_args(&call).iter().any(|a| {
            matches!(kind::of(a), kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::BLOCK_ARGUMENT_NODE)
        }) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Do not write to stdout in specs.", |c| c.remove(s, e));
    }
}
