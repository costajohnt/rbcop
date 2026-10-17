use crate::prelude::*;

pub const NAME: &str = "RSpec/OverwritingSetup";
pub static COP: OverwritingSetup = OverwritingSetup;
pub struct OverwritingSetup;

impl Cop for OverwritingSetup {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example_group_with_body(&l, node) {
            return;
        }
        let Some(body) = helpers::rspec::parser_body(helpers::rspec::block_body(&node.as_call_node().unwrap())) else {
            return;
        };
        // find_duplicates: the body's block children that set up a let/subject with literal arguments
        let mut seen: Vec<String> = Vec::new();
        for child in helpers::rspec::child_nodes(&body) {
            let Some(call) = helpers::rspec::plain_block_call(&child) else { continue };
            let n = method_name(&call);
            if call.receiver().is_some() || !(l.helper(n) || l.subject(n)) {
                continue;
            }
            let args = send_args(&call);
            if !args.iter().all(helpers::literal_node::basic_literal) {
                continue;
            }
            let name = match &args[..] {
                [] => "subject".to_string(),
                // first_argument_name: (send _ _ ({str sym} $_))
                [a] if helpers::rspec::is_sym(cx, a) => helpers::rails::sym_value(a).unwrap_or_default(),
                [a] if helpers::rspec::is_str(a) => String::from_utf8_lossy(a.as_string_node().unwrap().unescaped()).into_owned(),
                // `nil.to_sym` raises: the rest of the group is not checked
                _ => return,
            };
            if seen.contains(&name) {
                let (s, e) = helpers::rspec::node_range(&child);
                cx.add(s, e, format!("`{name}` is already defined."));
            } else {
                seen.push(name);
            }
        }
    }
}
