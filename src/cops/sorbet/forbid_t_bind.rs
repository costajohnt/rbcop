use crate::helpers::parser_parent::is_top_const;
use crate::helpers::sorbet_rbs_assertion::{assertion_statement, autocorrectable, rbi_type_to_rbs, send_parent};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTBind";
pub static COP: ForbidTBind = ForbidTBind;
pub struct ForbidTBind;

impl Cop for ForbidTBind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const {nil? cbase} :T) :bind _ _)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "bind" || is_csend(&call) || !call.receiver().is_some_and(|r| is_top_const(&r, "T")) {
            return;
        }
        let args = send_args(&call);
        if args.len() != 2 {
            return;
        }
        let (s, e) = send_range(&call);
        let replacement = (args[0].as_self_node().is_some()
            && autocorrectable(cx, (s, e), assertion_statement(send_parent(&call, cx.ancestors), (s, e), false)))
        .then(|| rbi_type_to_rbs(node_text(&args[1])))
        .flatten()
        .map(|t| format!("#: self as {t}"));
        cx.add_fix(s, e, "Do not use `T.bind`.", |c| {
            if let Some(r) = &replacement {
                c.replace(s, e, r);
            }
        });
    }
}
