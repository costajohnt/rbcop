use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedIncludeExample";
pub static COP: RepeatedIncludeExample = RepeatedIncludeExample;
pub struct RepeatedIncludeExample;

/// (signature, (send range, shared examples name source))
type Item = (String, ((usize, usize), String));

/// RuboCop::RSpec::Node#recursive_literal_or_const?
fn recursive_literal_or_const(node: &Node) -> bool {
    let all = |items: &mut dyn Iterator<Item = Node>| {
        for n in items {
            if !recursive_literal_or_const(&n) {
                return false;
            }
        }
        true
    };
    match node {
        Node::ArrayNode { .. } => all(&mut node.as_array_node().unwrap().elements().iter()),
        Node::HashNode { .. } => all(&mut node.as_hash_node().unwrap().elements().iter()),
        Node::KeywordHashNode { .. } => all(&mut node.as_keyword_hash_node().unwrap().elements().iter()),
        Node::AssocNode { .. } => {
            let a = node.as_assoc_node().unwrap();
            recursive_literal_or_const(&a.key()) && recursive_literal_or_const(&a.value())
        }
        Node::RangeNode { .. } => {
            let r = node.as_range_node().unwrap();
            r.left().is_none_or(|n| recursive_literal_or_const(&n)) && r.right().is_none_or(|n| recursive_literal_or_const(&n))
        }
        Node::ParenthesesNode { .. } => {
            helpers::rspec::begin_children(node).is_some_and(|cs| cs.iter().all(|c| recursive_literal_or_const(c)))
        }
        // dstr, xstr, dsym, regexp: composite, children recursively (`#{...}` is a `begin`)
        Node::InterpolatedStringNode { .. } => {
            all(&mut node.as_interpolated_string_node().unwrap().parts().iter())
        }
        Node::InterpolatedSymbolNode { .. } => {
            all(&mut node.as_interpolated_symbol_node().unwrap().parts().iter())
        }
        Node::InterpolatedRegularExpressionNode { .. } => {
            all(&mut node.as_interpolated_regular_expression_node().unwrap().parts().iter())
        }
        Node::InterpolatedXStringNode { .. } => {
            all(&mut node.as_interpolated_x_string_node().unwrap().parts().iter())
        }
        Node::EmbeddedStatementsNode { .. } => node
            .as_embedded_statements_node()
            .unwrap()
            .statements()
            .is_none_or(|s| s.body().iter().all(|n| recursive_literal_or_const(&n))),
        Node::EmbeddedVariableNode { .. } => false,
        Node::StringNode { .. } | Node::XStringNode { .. } => true,
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => true,
        _ => helpers::literal_node::literal(node),
    }
}

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
            .filter(|c| send_args(c).iter().all(|a| recursive_literal_or_const(a)))
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
