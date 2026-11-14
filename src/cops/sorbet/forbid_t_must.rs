use crate::helpers::sorbet_rbs_assertion::{assertion_statement, autocorrectable, is_t_call, send_parent, t_send};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTMust";
pub static COP: ForbidTMust = ForbidTMust;
pub struct ForbidTMust;

impl Cop for ForbidTMust {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :must _)
        let call = node.as_call_node().unwrap();
        if !is_t_call(&call, "must", Some(1)) {
            return;
        }
        let arg = &send_args(&call)[0];
        let (s, e) = send_range(&call);
        let fix = arg.as_splat_node().is_none()
            && !t_send(arg, "must").is_some_and(|c| is_t_call(&c, "must", Some(1)))
            && autocorrectable(cx, (s, e), assertion_statement(send_parent(&call, cx.ancestors), (s, e), true));
        let replacement = format!("{} #: as !nil", node_text(arg));
        cx.add_fix(s, e, "Do not use `T.must`.", |c| {
            if fix {
                c.replace(s, e, &replacement);
            }
        });
    }
}
