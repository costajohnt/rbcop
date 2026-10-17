use crate::prelude::*;

pub const NAME: &str = "RSpec/DescribeSymbol";
pub static COP: DescribeSymbol = DescribeSymbol;
pub struct DescribeSymbol;

impl Cop for DescribeSymbol {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send #rspec? :describe $sym ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "describe" || !helpers::rspec::rspec_recv(&call) {
            return;
        }
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        if helpers::rspec::is_sym(cx, &arg) {
            let loc = arg.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Avoid describing symbols.");
        }
    }
}
