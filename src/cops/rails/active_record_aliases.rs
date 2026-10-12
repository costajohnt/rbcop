use crate::prelude::*;

pub const NAME: &str = "Rails/ActiveRecordAliases";
pub static COP: ActiveRecordAliases = ActiveRecordAliases;
pub struct ActiveRecordAliases;

impl Cop for ActiveRecordAliases {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let prefer = match name {
            "update_attributes" => "update",
            "update_attributes!" => "update!",
            _ => return,
        };
        if send_args(&call).is_empty() {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let before = cx.out.len();
        cx.add_fix(s, e, format!("Use `{prefer}` instead of `{name}`."), |c| c.replace(s, e, prefer));
        // add_offense(..., severity: :warning)
        if cx.out.len() > before {
            cx.out.last_mut().unwrap().severity = crate::cop::Severity::Warning;
        }
    }
}
