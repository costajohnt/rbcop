use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedExampleGroupDescription";
pub static COP: RepeatedExampleGroupDescription = RepeatedExampleGroupDescription;
pub struct RepeatedExampleGroupDescription;

impl Cop for RepeatedExampleGroupDescription {
    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = helpers::rspec::begin_statements(cx, node) else { return };
        let l = helpers::rspec::lang(cx);
        let groups: Vec<&Node> = stmts.iter().filter(|s| helpers::rspec::example_group(&l, s)).collect();
        if groups.len() < 2 {
            return;
        }
        let src = cx.src;
        let items: Vec<(String, (usize, usize, String))> = groups
            .into_iter()
            .filter(|g| !helpers::rspec::skip_or_pending_inside_block(g))
            // empty_description?: (block (send _ _) ...)
            .filter(|g| helpers::rspec::plain_block_call(g).is_none_or(|c| !send_args(&c).is_empty()))
            .map(|g| {
                let call = g.as_call_node().unwrap();
                // doc_string_and_metadata: (block (send _ _ $_ $...) ...), nil otherwise
                let key = match helpers::rspec::plain_block_call(g) {
                    Some(c) => {
                        let args = send_args(&c);
                        format!("{}|{}", helpers::rspec::skey(src, args.first()), helpers::rspec::skeys(src, &args[1..]))
                    }
                    None => "nil".to_string(),
                };
                let (s, e) = helpers::rspec::node_range(g);
                (key, (s, e, method_name(&call).to_string()))
            })
            .collect();
        for group in helpers::rspec::repeated_groups(items) {
            let lines: Vec<usize> = group.iter().map(|&(s, _, _)| src.line_of(s)).collect();
            for (s, e, name) in &group {
                let me = src.line_of(*s);
                let others: Vec<usize> = lines.iter().copied().filter(|&l| l != me).collect();
                cx.add(*s, *e, format!("Repeated {name} block description on line(s) {}", helpers::rspec::lines_list(&others)));
            }
        }
    }
}
