use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/Yield";
pub static COP: Yield = Yield;
pub struct Yield;

/// block_call?: `(send (lvar %) :call ...)`
fn block_call<'pr>(node: &Node<'pr>, name: &[u8]) -> Option<ruby_prism::CallNode<'pr>> {
    let c = helpers::rspec::send_call(node).filter(|c| !is_csend(c) && method_name(c) == "call")?;
    c.receiver()?.as_local_variable_read_node().filter(|l| l.name().as_slice() == name)?;
    Some(c)
}

impl Cop for Yield {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        // method_on_stub?(node.send_node): (send nil? :receive ...) in the send
        struct V(bool);
        impl<'pr> Visit<'pr> for V {
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                self.0 |= n.receiver().is_none() && method_name(n) == "receive";
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V(call.receiver().is_none() && method_name(&call) == "receive");
        if let Some(r) = call.receiver() {
            v.visit(&r);
        }
        for a in send_args(&call) {
            v.visit(&a);
        }
        if !v.0 {
            return;
        }
        let block = helpers::rspec::block_of(&call).unwrap();
        // block_arg: (args (blockarg $_))
        let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if params.locals().iter().next().is_some() {
            return;
        }
        let Some(p) = params.parameters() else { return };
        if p.requireds().iter().next().is_some()
            || p.optionals().iter().next().is_some()
            || p.rest().is_some()
            || p.posts().iter().next().is_some()
            || p.keywords().iter().next().is_some()
            || p.keyword_rest().is_some()
        {
            return;
        }
        let Some(name) = p.block().and_then(|b| b.name()) else { return };
        let name = name.as_slice().to_vec();
        // calling_block?: every statement calls the block (RuboCop raises without a body)
        let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
        let stmts = helpers::rspec::begin_children(&body).unwrap_or_else(|| vec![body]);
        let calls: Option<Vec<ruby_prism::CallNode>> = stmts.iter().map(|s| block_call(s, &name)).collect();
        let Some(calls) = calls else { return };
        let replacement: String = calls
            .iter()
            .map(|c| {
                let args = send_args(c);
                if args.is_empty() {
                    ".and_yield".to_string()
                } else {
                    format!(".and_yield({})", args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", "))
                }
            })
            .collect();
        let (s, e) = (block.opening_loc().start_offset(), block.closing_loc().end_offset());
        let opts = helpers::range_help::Surrounding { side: helpers::range_help::Side::Left, ..Default::default() };
        let (rs, re) = helpers::range_help::range_with_surrounding_space(cx.src.text, s, e, opts);
        cx.add_fix(s, e, "Use `.and_yield`.", |c| c.replace(rs, re, &replacement));
    }
}
