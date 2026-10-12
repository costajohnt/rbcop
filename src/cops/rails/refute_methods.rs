use crate::prelude::*;

pub const NAME: &str = "Rails/RefuteMethods";
pub static COP: RefuteMethods = RefuteMethods;
pub struct RefuteMethods;

const CORRECTIONS: &[(&str, &str)] = &[
    ("refute", "assert_not"),
    ("refute_empty", "assert_not_empty"),
    ("refute_equal", "assert_not_equal"),
    ("refute_in_delta", "assert_not_in_delta"),
    ("refute_in_epsilon", "assert_not_in_epsilon"),
    ("refute_includes", "assert_not_includes"),
    ("refute_instance_of", "assert_not_instance_of"),
    ("refute_kind_of", "assert_not_kind_of"),
    ("refute_nil", "assert_not_nil"),
    ("refute_operator", "assert_not_operator"),
    ("refute_predicate", "assert_not_predicate"),
    ("refute_respond_to", "assert_not_respond_to"),
    ("refute_same", "assert_not_same"),
    ("refute_match", "assert_no_match"),
];

impl Cop for RefuteMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() {
            return;
        }
        let name = method_name(&call);
        let assert_not = cx.cfg.str("EnforcedStyle") != Some("refute");
        let good = if assert_not {
            CORRECTIONS.iter().find(|(b, _)| *b == name).map(|(_, g)| *g)
        } else {
            CORRECTIONS.iter().find(|(_, b)| *b == name).map(|(g, _)| *g)
        };
        let Some(good) = good else { return };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `{good}` over `{name}`."), |c| c.replace(s, e, good));
    }
}
