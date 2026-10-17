use crate::prelude::*;

pub const NAME: &str = "Rails/ToSWithArgument";
pub static COP: ToSWithArgument = ToSWithArgument;
pub struct ToSWithArgument;

const FORMATS: &[&str] = &[
    "currency", "db", "delimited", "human", "human_size", "inspect", "iso8601", "long", "long_ordinal", "nsec",
    "number", "percentage", "phone", "rfc822", "rounded", "short", "time", "usec",
];

impl Cop for ToSWithArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        7.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "to_s" {
            return;
        }
        let args = send_args(&call);
        if !args.first().and_then(helpers::rails::sym_value).is_some_and(|v| FORMATS.contains(&v.as_str())) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Use `to_formatted_s` instead.", |c| c.replace(s, e, "to_formatted_s"));
    }
}
