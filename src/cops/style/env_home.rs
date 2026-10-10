use crate::prelude::*;

pub const NAME: &str = "Style/EnvHome";
pub static COP: EnvHome = EnvHome;
pub struct EnvHome;

/// Parser `(str "HOME")`.
fn is_home_str(node: &Node) -> bool {
    node.as_string_node().is_some_and(|s| s.unescaped() == b"HOME") && !is_parser_dstr(node)
}

impl Cop for EnvHome {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "[]" | "fetch") || is_csend(&call) {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "ENV")) {
            return;
        }
        let args = send_args(&call);
        if !args.first().is_some_and(is_home_str) {
            return;
        }
        if args.len() == 2 && args[1].as_nil_node().is_none() {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, "Use `Dir.home` instead.", |c| c.replace(start, end, "Dir.home"));
    }
}
