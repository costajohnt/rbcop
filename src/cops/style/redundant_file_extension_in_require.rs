use crate::prelude::*;

pub const NAME: &str = "Style/RedundantFileExtensionInRequire";
pub static COP: RedundantFileExtensionInRequire = RedundantFileExtensionInRequire;
pub struct RedundantFileExtensionInRequire;

impl Cop for RedundantFileExtensionInRequire {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !matches!(method_name(&call), "require" | "require_relative") {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let Some(s) = arg.as_string_node().filter(|_| !is_parser_dstr(arg)) else { return };
        if !s.unescaped().ends_with(b".rb") {
            return;
        }
        let loc = arg.location();
        let end = loc.end_offset();
        if end < loc.start_offset() + 4 || cx.src.text[loc.start_offset()..end - 4].ends_with('\\') {
            return;
        }
        let (a, b) = (end - 4, end - 1);
        cx.add_fix(a, b, "Redundant `.rb` file extension detected.", |c| c.remove(a, b));
    }
}
