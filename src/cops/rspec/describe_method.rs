use crate::prelude::*;

pub const NAME: &str = "RSpec/DescribeMethod";
pub static COP: DescribeMethod = DescribeMethod;
pub struct DescribeMethod;

impl Cop for DescribeMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        for group in helpers::rspec::top_level_groups(&l, node) {
            // (block (send #rspec? :describe _first_argument ${str dstr} ...) ...)
            let Some(call) = helpers::rspec::plain_block_call(&group) else { continue };
            if method_name(&call) != "describe" || !helpers::rspec::rspec_recv(&call) {
                continue;
            }
            let args = send_args(&call);
            let Some(second) = args.get(1) else { continue };
            if !helpers::rspec::is_str(second) && !helpers::rspec::is_dstr(second) {
                continue;
            }
            if helpers::rspec::str_or_dstr_head(second).is_some_and(|v| v.starts_with(['.', '#'])) {
                continue;
            }
            let (s, e) = helpers::rspec::node_range(second);
            cx.add(s, e, "The second argument to describe should be the method being tested. '#instance' or '.class'.");
        }
    }
}
