use crate::prelude::*;

pub const NAME: &str = "Rails/WhereExists";
pub static COP: WhereExists = WhereExists;
pub struct WhereExists;

impl Cop for WhereExists {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "exists?" {
            return;
        }
        let exists_style = cx.cfg.str("EnforcedStyle") != Some("where");
        let sel = call.message_loc().unwrap();
        let (args, range): (Vec<Node>, (usize, usize)) = if exists_style {
            // (call (call _ :where $...) :exists?)
            if !send_args(&call).is_empty() {
                return;
            }
            let Some(w) = call.receiver().and_then(|r| r.as_call_node()) else { return };
            if method_name(&w) != "where" || w.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            let Some(wsel) = w.message_loc() else { return };
            (send_args(&w), (wsel.start_offset(), sel.end_offset()))
        } else {
            // (call _ :exists? $!splat_type?)
            let args = send_args(&call);
            if args.len() != 1 || args[0].as_splat_node().is_some() {
                return;
            }
            (args, (sel.start_offset(), send_range(&call).1))
        };
        if args.is_empty()
            || !(args.len() > 1 || helpers::rails::hash_elements(&args[0]).is_some() || args[0].as_array_node().is_some())
        {
            return;
        }
        let srcs: Vec<&str> = args.iter().map(|a| node_text(a)).collect();
        let good = if exists_style {
            if srcs.len() > 1 { format!("exists?([{}])", srcs.join(", ")) } else { format!("exists?({})", srcs[0]) }
        } else {
            let dot = call.call_operator_loc().map_or(".", |d| text(&d));
            format!("where({}){dot}exists?", srcs.join(", "))
        };
        let (s, e) = range;
        let msg = format!("Prefer `{good}` over `{}`.", cx.src.slice(s, e));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &good));
    }
}
