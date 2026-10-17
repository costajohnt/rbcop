use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedExample";
pub static COP: RepeatedExample = RepeatedExample;
pub struct RepeatedExample;

impl Cop for RepeatedExample {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::example_group(&l, node) {
            return;
        }
        let src = cx.src;
        let examples = helpers::rspec::find_all_in_scope(&l, node, helpers::rspec::example);
        // build_example_signature: [metadata, implementation] (+ the arguments of `its`)
        let items: Vec<(String, (usize, usize))> = examples
            .iter()
            .map(|ex| {
                let call = ex.as_call_node().unwrap();
                let (_, meta) = helpers::rspec::example_parts(&call);
                let body = helpers::rspec::parser_body(helpers::rspec::block_body(&call));
                let mut key = format!(
                    "{}|{}",
                    meta.map_or("nil".to_string(), |m| helpers::rspec::skeys(src, &m)),
                    helpers::rspec::skey(src, body.as_ref())
                );
                if method_name(&call) == "its" {
                    key.push('|');
                    key.push_str(&helpers::rspec::skeys(src, &send_args(&call)));
                }
                (key, helpers::rspec::node_range(ex))
            })
            .collect();
        for group in helpers::rspec::repeated_groups(items) {
            for &(s, e) in &group {
                let mut others: Vec<usize> =
                    group.iter().filter(|&&r| r != (s, e)).map(|&(os, _)| src.line_of(os)).collect();
                others.sort_unstable();
                others.dedup();
                let lines = others.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ");
                cx.add(s, e, format!("Don't repeat examples within an example group. Repeated on line(s) {lines}."));
            }
        }
    }
}
