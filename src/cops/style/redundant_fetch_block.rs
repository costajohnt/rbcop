use crate::helpers::frozen_string_literal as fsl;
use crate::helpers::literal_node::basic_literal;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantFetchBlock";
pub static COP: RedundantFetchBlock = RedundantFetchBlock;
pub struct RedundantFetchBlock;

impl Cop for RedundantFetchBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "fetch" {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let args = call_args(&call);
        let [key] = args.as_slice() else { return };
        // (args): no parameters, `||` allowed; numbered/it parameters make a numblock/itblock.
        match block.parameters() {
            None => {}
            Some(p) => {
                let Some(bp) = p.as_block_parameters_node() else { return };
                if bp.parameters().is_some() || bp.locals().iter().next().is_some() {
                    return;
                }
            }
        }
        let body = match block.body() {
            None => None,
            Some(b) => {
                let Some(s) = b.as_statements_node() else { return };
                let stmts: Vec<Node> = s.body().iter().collect();
                let [one] = stmts.as_slice() else { return };
                let one = one.location();
                let n = s.body().iter().next().unwrap();
                if !(basic_literal(&n) || n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some()) {
                    return;
                }
                Some((n, one.start_offset(), one.end_offset()))
            }
        };
        if let Some((n, _, _)) = &body {
            let is_const = n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some();
            let is_str = matches!(n, Node::StringNode { .. } | Node::SourceFileNode { .. });
            if (is_const && cx.cfg.bool("SafeForConstants") != Some(true)) || (is_str && !fsl::enabled(cx)) {
                return;
            }
        }
        // rails_cache?: (send (const _ :Rails) :cache)
        if let Some(r) = call.receiver().and_then(|r| r.as_call_node())
            && method_name(&r) == "cache"
            && !is_csend(&r)
            && r.arguments().is_none()
            && r.block().is_none()
            && r.receiver().is_some_and(|c| {
                c.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Rails")
                    || c.as_constant_path_node().is_some_and(|c| c.name().is_some_and(|n| n.as_slice() == b"Rails"))
            })
        {
            return;
        }
        let src = cx.src;
        let key = src.slice(key.location().start_offset(), key.location().end_offset());
        let body = body.map(|(_, s, e)| src.slice(s, e));
        let good = format!("fetch({key}, {})", body.unwrap_or("nil"));
        let bad = match body {
            Some(b) => format!("fetch({key}) {{ {b} }}"),
            None => format!("fetch({key}) {{}}"),
        };
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        let msg = format!("Use `{good}` instead of `{bad}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &good));
    }
}
