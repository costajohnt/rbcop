use crate::prelude::*;

pub const NAME: &str = "RSpec/ScatteredLet";
pub static COP: ScatteredLet = ScatteredLet;
pub struct ScatteredLet;

impl Cop for ScatteredLet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example_group_with_body(&l, node) {
            return;
        }
        let body = helpers::rspec::block_body(&node.as_call_node().unwrap());
        // A non-`begin` body's child nodes are the parts of its single statement: never a `let`.
        if !helpers::rspec::is_begin_body(&body) {
            return helpers::rspec::ask_let_of_parts(&l, body);
        }
        let stmts = helpers::rspec::body_children(body);
        let lets: Vec<usize> = (0..stmts.len()).filter(|&i| helpers::rspec::let_(&l, &stmts[i])).collect();
        let Some(&first) = lets.first() else { return };
        let mut reference = first;
        let anc = cx.ancestors;
        let program = &anc[0];
        for (idx, &i) in lets.iter().enumerate() {
            if i == first + idx {
                reference = i;
                continue;
            }
            let (s, e) = helpers::rspec::node_range(&stmts[i]);
            let Some(mv) = helpers::rspec::MoveNode::new(cx.src, program, &stmts[i], &stmts[reference], true) else {
                break;
            };
            cx.add_fix(s, e, "Group all let/let! blocks in the example group together.", |c| mv.apply(c));
        }
    }
}
