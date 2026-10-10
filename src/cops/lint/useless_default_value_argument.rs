use crate::prelude::*;

pub const NAME: &str = "Lint/UselessDefaultValueArgument";
pub static COP: UselessDefaultValueArgument = UselessDefaultValueArgument;
pub struct UselessDefaultValueArgument;

/// The receiver of a `fetch`/`new` send that triggers `on_send`.
fn trigger<'pr>(call: &ruby_prism::CallNode<'pr>) -> Option<Node<'pr>> {
    if !matches!(method_name(call), "fetch" | "new") {
        return None;
    }
    call.receiver()
}

impl Cop for UselessDefaultValueArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `default_value_argument_and_block(node.parent)`, matched from the block's call.
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let args = call_args(&call);
        let [prev, default] = args.as_slice() else { return };
        let matched = match method_name(&call) {
            "fetch" => call.receiver().is_some(),
            "new" => {
                !is_csend(&call)
                    && call.receiver().is_some_and(|r| match kind::of(&r) {
                        kind::CONSTANT_READ_NODE => r.as_constant_read_node().unwrap().name().as_slice() == b"Array",
                        kind::CONSTANT_PATH_NODE => r.as_constant_path_node().unwrap().name().is_some_and(|n| n.as_slice() == b"Array"),
                        _ => false,
                    })
            }
            _ => false,
        };
        if !matched {
            return;
        }
        // Both the block's own send and a lone send in its body have the block as parser parent.
        let body = block.body().and_then(|b| b.as_statements_node()).and_then(|s| {
            let stmts: Vec<_> = s.body().iter().collect();
            if stmts.len() == 1 { stmts.into_iter().next() } else { None }
        });
        // A body send with a literal block of its own is that block's child instead.
        let body_call =
            body.and_then(|b| b.as_call_node()).filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()));
        let receivers: Vec<Node> = std::iter::once(call).chain(body_call).filter_map(|c| trigger(&c)).collect();
        if receivers.is_empty() || default.as_keyword_hash_node().is_some() {
            return;
        }
        let (prev_end, start, end) =
            (prev.location().end_offset(), default.location().start_offset(), default.location().end_offset());
        for r in &receivers {
            if helpers::allowed_receivers::allowed_receiver(cx, r) {
                continue;
            }
            cx.add_fix(start, end, "Block supersedes default value argument.", |c| c.remove(prev_end, end));
        }
    }
}
