use crate::prelude::*;
use std::cell::Cell;

pub const NAME: &str = "Style/LambdaCall";
pub static COP: LambdaCall = LambdaCall;
pub struct LambdaCall;

impl Cop for LambdaCall {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "call" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let (s, e) = send_range(&call);
        if cx.src.comments.iter().any(|c| s <= c.start && c.end <= e) {
            return;
        }
        let implicit = call.message_loc().is_none();
        let explicit_style = cx.cfg.str("EnforcedStyle") != Some("braces");
        if explicit_style != implicit {
            return cx.correct_style_detected();
        }
        let dot = call.call_operator_loc().map_or("", |d| text(&d));
        let args: Vec<&str> = send_args(&call).iter().map(node_text).collect();
        let method = if explicit_style {
            if args.is_empty() { "call".to_string() } else { format!("call({})", args.join(", ")) }
        } else {
            format!("({})", args.join(", "))
        };
        let prefer = format!("{}{dot}{method}", node_text(&recv));
        let msg = format!("Prefer the use of `{prefer}` over `{}`.", &cx.src.text[s..e]);
        let ignored = cx.state::<Vec<(usize, usize)>>().iter().any(|&(a, b)| a <= s && e <= b);
        let ran = Cell::new(false);
        cx.add_fix(s, e, msg, |c| {
            if !ignored {
                c.replace(s, e, &prefer);
                ran.set(true);
            }
        });
        if ran.get() {
            cx.opposite_style_detected();
            cx.state::<Vec<(usize, usize)>>().push((s, e));
        }
    }
}
