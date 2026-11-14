use crate::helpers::sorbet_rbs_assertion::{is_t_call, rbi_type_to_rbs, rbs_assertion_edits, t_send};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTCast";
pub static COP: ForbidTCast = ForbidTCast;
pub struct ForbidTCast;

impl Cop for ForbidTCast {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (const nil? :T) :cast _ _)
        let call = node.as_call_node().unwrap();
        if !is_t_call(&call, "cast", Some(2)) {
            return;
        }
        let args = send_args(&call);
        let nested = t_send(&args[0], "cast").is_some_and(|c| is_t_call(&c, "cast", Some(2)));
        let edits = if nested {
            Vec::new()
        } else {
            rbs_assertion_edits(cx, &call, true, || {
                Some((node_text(&args[0]).to_string(), format!("as {}", rbi_type_to_rbs(node_text(&args[1]))?)))
            })
        };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Do not use `T.cast`.", |c| {
            for (a, b, t) in &edits {
                c.replace(*a, *b, t);
            }
        });
    }
}
