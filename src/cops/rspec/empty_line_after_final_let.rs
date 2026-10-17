use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyLineAfterFinalLet";
pub static COP: EmptyLineAfterFinalLet = EmptyLineAfterFinalLet;
pub struct EmptyLineAfterFinalLet;

impl Cop for EmptyLineAfterFinalLet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send #rspec? {#SharedGroups.all #ExampleGroups.all #Includes.all} ...) args $_)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        if !helpers::rspec::rspec_recv(&call) || !(l.shared_group(n) || l.example_group(n) || l.include(n)) {
            return;
        }
        // Only a `begin` body has the lets as its children with itself as their parent.
        let body = helpers::rspec::block_body(&call);
        if !helpers::rspec::is_begin_body(&body) {
            return;
        }
        let stmts = helpers::rspec::body_children(body);
        let Some(i) = stmts.iter().rposition(|s| helpers::rspec::let_(&l, s)) else { return };
        if i + 1 == stmts.len() {
            return;
        }
        let final_let = &stmts[i];
        let Some((s, e)) = helpers::rspec::missing_separating_line(cx.src, final_let) else { return };
        let name = method_name(&final_let.as_call_node().unwrap()).to_string();
        cx.add_fix(s, e, format!("Add an empty line after the last `{name}`."), |c| c.insert_after(e, e, "\n"));
    }
}
