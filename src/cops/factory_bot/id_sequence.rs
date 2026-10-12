use crate::prelude::*;

pub const NAME: &str = "FactoryBot/IdSequence";
pub static COP: IdSequence = IdSequence;
pub struct IdSequence;

impl Cop for IdSequence {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "sequence" {
            return;
        }
        if call.receiver().is_some_and(|r| !helpers::factory_bot::factory_bot_const(&r)) {
            return;
        }
        let Some(first) = send_args(&call).into_iter().next() else { return };
        if !helpers::rspec::is_sym(cx, &first) || helpers::rails::sym_value(&first).as_deref() != Some("id") {
            return;
        }
        let (s, e) = send_range(&call);
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, s, e, true);
        cx.add_fix(s, e, "Do not create a sequence for an id attribute", |c| c.remove(rs, re));
    }
}
