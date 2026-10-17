use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedExampleGroupBody";
pub static COP: RepeatedExampleGroupBody = RepeatedExampleGroupBody;
pub struct RepeatedExampleGroupBody;

impl Cop for RepeatedExampleGroupBody {
    fn kinds(&self) -> &'static [usize] {
        &[kind::STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = helpers::rspec::begin_statements(cx, node) else { return };
        let l = helpers::rspec::lang(cx);
        let groups: Vec<&Node> = stmts.iter().filter(|s| helpers::rspec::example_group_with_body(&l, s)).collect();
        // several_example_groups?
        if groups.len() < 2 {
            return;
        }
        let src = cx.src;
        let items: Vec<(String, (usize, usize, String))> = groups
            .into_iter()
            .filter(|g| !helpers::rspec::skip_or_pending_inside_block(g))
            .map(|g| {
                let call = g.as_call_node().unwrap();
                // signature_keys: [metadata, body, const_arg]
                let args = send_args(&call);
                let meta = if args.is_empty() { "nil".to_string() } else { helpers::rspec::skeys(src, &args[1..]) };
                let body = helpers::rspec::parser_body(helpers::rspec::block_body(&call));
                let konst = args
                    .first()
                    .filter(|a| matches!(kind::of(a), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE));
                let key = format!(
                    "{meta}|[{}]|{}",
                    helpers::rspec::skey(src, body.as_ref()),
                    helpers::rspec::skey(src, konst)
                );
                (key, (helpers::rspec::node_range(g).0, helpers::rspec::node_range(g).1, method_name(&call).to_string()))
            })
            .collect();
        for group in helpers::rspec::repeated_groups(items) {
            let lines: Vec<usize> = group.iter().map(|&(s, _, _)| src.line_of(s)).collect();
            for (s, e, name) in &group {
                let me = src.line_of(*s);
                let others: Vec<usize> = lines.iter().copied().filter(|&l| l != me).collect();
                let loc = helpers::rspec::lines_list(&others);
                cx.add(*s, *e, format!("Repeated {name} block body on line(s) {loc}"));
            }
        }
    }
}
