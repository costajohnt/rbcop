use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedIncludeExample";
pub static COP: RepeatedIncludeExample = RepeatedIncludeExample;
pub struct RepeatedIncludeExample;

/// (signature, (send range, shared examples name source))
type Item = (String, ((usize, usize), String));


impl Cop for RepeatedIncludeExample {
    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = helpers::rspec::begin_statements(cx, node) else { return };
        let l = helpers::rspec::lang(cx);
        // include_examples?: (send nil? #Includes.examples ...)
        let includes: Vec<ruby_prism::CallNode> = stmts
            .iter()
            .filter_map(helpers::rspec::send_call)
            .filter(|c| c.receiver().is_none() && l.inc_examples.iter().any(|i| i == method_name(c)))
            .collect();
        if includes.len() < 2 {
            return;
        }
        let src = cx.src;
        let items: Vec<Item> = includes
            .iter()
            .filter(|c| send_args(c).iter().all(|a| helpers::rspec::recursive_literal_or_const(a)))
            .map(|c| {
                let args = send_args(c);
                // shared_examples_name: the first argument (RuboCop raises without one)
                let name = args.first().map(|a| node_text(a).to_string()).unwrap_or_default();
                (helpers::rspec::skeys(src, &args), (send_range(c), name))
            })
            .collect();
        for group in helpers::rspec::repeated_groups(items) {
            let lines: Vec<usize> = group.iter().map(|((s, _), _)| src.line_of(*s)).collect();
            for ((s, e), name) in &group {
                let me = src.line_of(*s);
                let others: Vec<usize> = lines.iter().copied().filter(|&l| l != me).collect();
                let msg = format!("Repeated include of shared_examples {name} on line(s) {}", helpers::rspec::lines_list(&others));
                cx.add(*s, *e, msg);
            }
        }
    }
}
