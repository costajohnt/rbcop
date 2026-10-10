use crate::helpers::allowed::{allowed_method, matches_allowed_pattern};
use crate::helpers::code_length::{Measured, check, global_const};
use crate::prelude::*;

pub const NAME: &str = "Metrics/BlockLength";
pub static COP: BlockLength = BlockLength;
pub struct BlockLength;

/// rubocop-ast `class_constructor?` on a block: `Class/Module/Struct.new` or `Data.define`.
fn class_constructor(call: &ruby_prism::CallNode) -> bool {
    let Some(recv) = call.receiver() else { return false };
    match call.name().as_slice() {
        b"new" => global_const(&recv, &["Class", "Module", "Struct"]),
        b"define" => global_const(&recv, &["Data"]),
        _ => false,
    }
}

impl Cop for BlockLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (method, receiver, body) = match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
                if class_constructor(&call) {
                    return;
                }
                let recv = call.receiver().map(|r| node_text(&r).split_whitespace().collect::<String>());
                (String::from_utf8_lossy(call.name().as_slice()).into_owned(), recv, block.body())
            }
            kind::LAMBDA_NODE => ("lambda".to_string(), None, node.as_lambda_node().unwrap().body()),
            kind::SUPER_NODE => {
                let Some(block) = node.as_super_node().unwrap().block().and_then(|b| b.as_block_node()) else {
                    return;
                };
                ("super".to_string(), None, block.body())
            }
            _ => {
                let Some(block) = node.as_forwarding_super_node().unwrap().block() else { return };
                ("super".to_string(), None, block.body())
            }
        };
        if allowed_method(cx.cfg, &method) || matches_allowed_pattern(cx.cfg, &method) {
            return;
        }
        // method_receiver_excluded?
        let excluded = cx.cfg.strings("AllowedMethods").iter().any(|config| {
            let mut parts = config.split('.');
            let first = parts.next().unwrap_or("");
            match parts.next() {
                Some(m) => m == method && receiver.as_deref() == Some(first),
                None => first == method,
            }
        });
        if excluded {
            return;
        }
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        check(cx, Measured::Body { root: node, body }, range, range, |len, max| {
            format!("Block has too many lines. [{len}/{max}]")
        });
    }
}
