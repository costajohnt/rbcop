use crate::prelude::*;

pub const NAME: &str = "RSpecRails/NegationBeValid";
pub static COP: NegationBeValid = NegationBeValid;
pub struct NegationBeValid;

impl Cop for NegationBeValid {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "be_valid" | "be_invalid") {
            return;
        }
        let (runner_ok, matcher, runner, replacement): (fn(&str) -> bool, &str, &str, &str) = match cx.cfg.str("EnforcedStyle") {
            // be_invalid?: (send ... :to (send nil? :be_invalid ...))
            Some("not_to") => (|r| r == "to", "be_invalid", "not_to", "be_valid"),
            // not_to?: (send ... {:not_to :to_not} (send nil? :be_valid ...))
            Some("be_invalid") => (|r| r == "not_to" || r == "to_not", "be_valid", "to", "be_invalid"),
            _ => return,
        };
        if name != matcher || call.receiver().is_some() || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        // node.parent: the runner send, with the matcher as its only argument
        let anc = cx.ancestors;
        let Some(parent) = helpers::parser_parent::parser_parent(anc).and_then(|p| p.as_call_node()) else { return };
        if is_csend(&parent) || !runner_ok(method_name(&parent)) {
            return;
        }
        let [arg] = &send_args(&parent)[..] else { return };
        if helpers::rspec::node_range(arg) != helpers::rspec::node_range(node) {
            return;
        }
        let psel = parent.message_loc().unwrap();
        let sel = call.message_loc().unwrap();
        let (ps, pe, ms, me) = (psel.start_offset(), psel.end_offset(), sel.start_offset(), sel.end_offset());
        cx.add_fix(ps, me, format!("Use `expect(...).{runner} {replacement}`."), |c| {
            c.replace(ps, pe, runner);
            c.replace(ms, me, replacement);
        });
    }
}
