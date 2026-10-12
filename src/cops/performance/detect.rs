use crate::prelude::*;

pub const NAME: &str = "Performance/Detect";
pub static COP: Detect = Detect;
pub struct Detect;

const CANDIDATES: &[&str] = &["select", "find_all", "filter"];

impl Cop for Detect {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // The outer node is a plain `send` (`&.first` never matches).
        if is_csend(&call) {
            return;
        }
        let second = method_name(&call);
        let args = send_args(&call);
        let index = match second {
            "first" | "last" if args.is_empty() => None,
            "[]" => match args.as_slice() {
                [a] if a.as_integer_node().is_some() => match int_literal_value(a) {
                    Some(v @ (0 | -1)) => Some(v),
                    _ => return,
                },
                _ => return,
            },
            _ => return,
        };
        // A block attached to `first` belongs to the outer send; parser sees `(block (send ...))`.
        let Some(recv) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if !CANDIDATES.contains(&method_name(&recv)) {
            return;
        }
        // `[]` takes a `send` receiver only; first/last any `call`.
        if index.is_some() && is_csend(&recv) {
            return;
        }
        let recv_args = call_args(&recv);
        match recv.block() {
            Some(b) if b.as_block_node().is_some() => {
                let block = b.as_block_node().unwrap();
                // `(block (call _ :select) ...)`: no arguments, not a numblock/itblock, and a body.
                let numbered = block.parameters().is_some_and(|p| {
                    p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()
                });
                if numbered || !recv_args.is_empty() || block.body().is_none() {
                    return;
                }
            }
            // `select(&:x).first`: the block pass must be the only argument.
            Some(_) if recv_args.is_empty() => {}
            _ => return,
        }
        // lazy?(caller)
        if recv.receiver().and_then(|r| r.as_call_node()).is_some_and(|c| {
            method_name(&c) == "lazy" && c.receiver().is_some() && !c.block().is_some_and(|b| b.as_block_node().is_some())
        }) {
            return;
        }
        // config.for_cop('Style/CollectionMethods')['PreferredMethods']['detect'] || 'detect'
        let collection = cx.config.for_cop("Style/CollectionMethods");
        let Some(Value::Map(preferred)) = collection.get("PreferredMethods") else { return };
        let prefer = match preferred.get("detect") {
            None | Some(Value::Null | Value::Bool(false)) => "detect".to_string(),
            Some(v) => v.as_string().unwrap_or_default(),
        };
        let first = method_name(&recv);
        let reverse = second == "last" || index == Some(-1);
        let msg = match index {
            Some(i) => format!("Use `{}{prefer}` instead of `{first}[{i}]`.", if reverse { "reverse." } else { "" }),
            None => format!("Use `{}{prefer}` instead of `{first}.{second}`.", if reverse { "reverse." } else { "" }),
        };
        let recv_sel = recv.message_loc().unwrap();
        let sel_end = call.message_loc().map_or(call.location().end_offset(), |l| l.end_offset());
        let recv_end = recv.location().end_offset();
        let replacement = if reverse { format!("reverse.{prefer}") } else { prefer.clone() };
        cx.add_fix(recv_sel.start_offset(), sel_end, msg, |c| {
            c.remove(recv_end, sel_end);
            c.replace(recv_sel.start_offset(), recv_sel.end_offset(), &replacement);
        });
    }
}
