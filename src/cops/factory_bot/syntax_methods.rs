use crate::prelude::*;

pub const NAME: &str = "FactoryBot/SyntaxMethods";
pub static COP: SyntaxMethods = SyntaxMethods;
pub struct SyntaxMethods;

/// spec_group?: a plain block of an RSpec example group or shared group method (not configurable).
fn spec_group(node: &Node) -> bool {
    let Some(c) = helpers::rspec::plain_block_call(node) else { return false };
    let recv_ok = c.receiver().is_none_or(|r| helpers::rspec::is_explicit_rspec(&r));
    !is_csend(&c)
        && recv_ok
        && matches!(
            method_name(&c),
            "describe"
                | "context"
                | "feature"
                | "example_group"
                | "xdescribe"
                | "xcontext"
                | "xfeature"
                | "fdescribe"
                | "fcontext"
                | "ffeature"
                | "shared_examples"
                | "shared_examples_for"
                | "shared_context"
        )
}

impl Cop for SyntaxMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !helpers::factory_bot::METHODS.contains(&method_name(&call)) {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::factory_bot::factory_bot_const(&r)) {
            return;
        }
        // inside_example_group?: the node (a send is never a block) or an ancestor is a spec group
        if !cx.ancestors.iter().any(spec_group) {
            return;
        }
        let s = node.location().start_offset();
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, se, format!("Use `{}` from `FactoryBot::Syntax::Methods`.", method_name(&call)), |c| c.remove(s, ss));
    }
}
