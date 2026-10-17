use crate::prelude::*;

pub const NAME: &str = "Performance/FlatMap";
pub static COP: FlatMap = FlatMap;
pub struct FlatMap;

/// `flatten_level, = *params.first`: None for no parameter or one without children
/// (nil/true/false/self), Some(is it 1) otherwise.
fn flatten_level(param: Option<&Node>) -> Option<bool> {
    let p = param?;
    match kind::of(p) {
        kind::NIL_NODE | kind::TRUE_NODE | kind::FALSE_NODE | kind::SELF_NODE | kind::FORWARDING_SUPER_NODE => None,
        kind::INTEGER_NODE => Some(int_literal_value(p) == Some(1)),
        kind::FLOAT_NODE => Some(helpers::literal_node::float_literal_value(p) == Some(1.0)),
        _ => Some(false),
    }
}

impl Cop for FlatMap {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let flatten = method_name(&call);
        if flatten != "flatten" && flatten != "flatten!" {
            return;
        }
        let Some(map) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let first = method_name(&map);
        if first != "map" && first != "collect" {
            return;
        }
        let map_args = send_args(&map);
        let ok = match map.block() {
            // (block (call _ ${:collect :map}) ...): a plain block, no arguments.
            Some(b) if b.as_block_node().is_some() => {
                let b = b.as_block_node().unwrap();
                map_args.is_empty()
                    && !b.parameters().is_some_and(|p| {
                        p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()
                    })
            }
            // (call _ ${:collect :map} (block_pass _))
            Some(_) => map_args.len() == 1,
            None => false,
        };
        if !ok {
            return;
        }
        let params = send_args(&call);
        let level = flatten_level(params.first());
        let mut msg = format!("Use `flat_map` instead of `{first}...{flatten}`.");
        match level {
            None if cx.cfg.bool("EnabledForFlattenWithoutParams") == Some(true) => {
                msg.push_str(
                    " Beware, `flat_map` only flattens 1 level and `flatten` can be used to flatten multiple levels.",
                );
            }
            Some(true) => {}
            _ => return,
        }
        let sel = map.message_loc().unwrap();
        let end = send_range(&call).1;
        let map_end = map.location().end_offset();
        let correct = level.is_some();
        cx.add_fix(sel.start_offset(), end, msg, |c| {
            if correct {
                c.remove(map_end, end);
                c.replace(sel.start_offset(), sel.end_offset(), "flat_map");
            }
        });
    }
}
