use crate::prelude::*;

pub const NAME: &str = "Rails/CompactBlank";
pub static COP: CompactBlank = CompactBlank;
pub struct CompactBlank;

impl Cop for CompactBlank {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        6.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        let predicate = match name {
            "reject" | "delete_if" => "blank?",
            "select" | "filter" | "keep_if" => "present?",
            _ => return,
        };
        if cx.config.target_ruby < 2.6 && name == "filter" {
            return;
        }
        let args = send_args(&call);
        // (send _ M (block_pass (sym :predicate)))
        let block_pass = matches!(args.as_slice(), [b] if b.as_block_argument_node().and_then(|b| b.expression())
            .is_some_and(|e| helpers::rails::sym_value(&e).as_deref() == Some(predicate)));
        let end = send_range(&call).1;
        let bad = block_pass || {
            // (block (send _ M) $(args ...) (send $(lvar _) :predicate))
            helpers::rails::block_call(node).is_some_and(|_| {
                let b = call.block().unwrap().as_block_node().unwrap();
                if !args.is_empty() {
                    return false;
                }
                let stmts = helpers::rails::body_statements(b.body());
                let [body] = stmts.as_slice() else { return false };
                let Some(pc) = helpers::rails::send_node(body) else { return false };
                let Some(lv) = pc.receiver().filter(|r| r.as_local_variable_read_node().is_some()) else { return false };
                if method_name(&pc) != predicate || !send_args(&pc).is_empty() {
                    return false;
                }
                let params: Vec<String> = b
                    .parameters()
                    .and_then(|p| p.as_block_parameters_node())
                    .and_then(|bp| bp.parameters())
                    .map(|ps| parameter_nodes(&ps).iter().map(|n| node_text(n).to_string()).collect())
                    .unwrap_or_default();
                let v = node_text(&lv);
                (params.len() == 1 && params[0] == v) || (params.len() == 2 && params[1] == v)
            })
        };
        if !bad {
            return;
        }
        let preferred = if matches!(name, "delete_if" | "keep_if") { "compact_blank!" } else { "compact_blank" };
        let s = call.message_loc().unwrap().start_offset();
        // With a block, the offense reaches the block's end (`node.parent.send_node == node`).
        let e = if helpers::rails::block_call(node).is_some() { node.location().end_offset() } else { end };
        cx.add_fix(s, e, format!("Use `{preferred}` instead."), |c| c.replace(s, e, preferred));
    }
}
