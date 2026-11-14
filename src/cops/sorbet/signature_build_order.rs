use crate::helpers::sorbet_signature::{as_send, block_body, signature};
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Sorbet/SignatureBuildOrder";
pub static COP: SignatureBuildOrder = SignatureBuildOrder;
pub struct SignatureBuildOrder;

impl Cop for SignatureBuildOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node).is_none() {
            return;
        }
        let Some(body) = block_body(&node.as_call_node().unwrap()) else { return };
        // call_chain: the sends from the root of the chain to the body.
        let mut chain = Vec::new();
        let (bs, be) = helpers::node_types::range(&body);
        let mut cur = Some(body);
        while let Some(call) = cur.as_ref().and_then(|n| as_send(n)) {
            cur = call.receiver();
            chain.push(call);
        }
        chain.reverse();
        let indexes: HashMap<String, usize> = cx.cfg.strings("Order").into_iter().enumerate().map(|(i, n)| (n, i)).collect();
        let actual: Vec<(usize, Option<usize>)> =
            chain.iter().enumerate().map(|(i, c)| (i, indexes.get(method_name(c)).copied())).collect();
        let (mut expected, unknown): (Vec<_>, Vec<_>) = actual.iter().copied().partition(|(_, e)| e.is_some());
        // ponytail: Ruby's sort_by! is not stable; equal keys (a repeated builder) keep source order here.
        expected.sort_by_key(|(_, e)| *e);
        for entry in unknown {
            expected.insert(entry.0, entry);
        }
        let names = |v: &[(usize, Option<usize>)]| v.iter().map(|(i, _)| method_name(&chain[*i])).collect::<Vec<_>>();
        let expected_names = names(&expected);
        if expected_names == names(&actual) {
            return;
        }
        let src = cx.src;
        let source = expected
            .iter()
            .map(|(i, _)| {
                let c = &chain[*i];
                let args = send_args(c);
                if args.is_empty() {
                    method_name(c).to_string()
                } else {
                    let args: Vec<&str> = args.iter().map(|a| {
                        let (s, e) = helpers::node_types::range(a);
                        src.slice(s, e)
                    }).collect();
                    format!("{}({})", method_name(c), args.join(", "))
                }
            })
            .collect::<Vec<_>>()
            .join(".");
        let (s, e) = (bs, be);
        let msg = format!("Sig builders must be invoked in the following order: {}.", expected_names.join(", "));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &source));
    }
}
