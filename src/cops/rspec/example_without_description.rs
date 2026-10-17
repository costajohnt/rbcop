use crate::prelude::*;

pub const NAME: &str = "RSpec/ExampleWithoutDescription";
pub static COP: ExampleWithoutDescription = ExampleWithoutDescription;
pub struct ExampleWithoutDescription;

impl Cop for ExampleWithoutDescription {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example(&l, node) {
            return;
        }
        let call = node.as_call_node().unwrap();
        let args = send_args(&call);
        let multiline = !helpers::rspec::single_line(cx.src, node);
        if args.is_empty() {
            let style = cx.cfg.str("EnforcedStyle");
            let disallow = style == Some("disallow") || (style == Some("single_line_only") && multiline);
            if disallow && !(method_name(&call) == "specify" && multiline) {
                let (s, e) = send_range(&call);
                cx.add(s, e, "Add a description.");
            }
        }
        // (send nil? _ $(str $_)) with an empty string
        if let [arg] = &args[..]
            && helpers::rspec::is_str(arg)
            && arg.as_string_node().unwrap().unescaped().is_empty()
        {
            let (s, e) = helpers::rspec::node_range(arg);
            cx.add(s, e, "Omit the argument when you want to have auto-generated description.");
        }
    }
}
