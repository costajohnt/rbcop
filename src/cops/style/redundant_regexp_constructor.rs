use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantRegexpConstructor";
pub static COP: RedundantRegexpConstructor = RedundantRegexpConstructor;
pub struct RedundantRegexpConstructor;

impl Cop for RedundantRegexpConstructor {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :Regexp) {:new :compile} $(regexp _* (regopt _*)))
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "new" | "compile") || is_csend(&call) {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "Regexp")) {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        if !matches!(kind::of(arg), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE) {
            return;
        }
        let (start, end) = send_range(&call);
        let replacement = node_text(arg);
        cx.add_fix(start, end, format!("Remove the redundant `Regexp.{method}`."), |c| c.replace(start, end, replacement));
    }
}
