use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/MapToSet";
pub static COP: MapToSet = MapToSet;
pub struct MapToSet;

/// `{ (call (any_block $(call _ {:map :collect}) ...) <name>) (call $(call _ {:map :collect} (block_pass sym)) <name>) }`
/// on a `<name>` call without arguments or block: the map call.
pub fn map_then<'pr>(call: &CallNode<'pr>, name: &str) -> Option<CallNode<'pr>> {
    if method_name(call) != name || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let map = call.receiver()?.as_call_node()?;
    if !matches!(method_name(&map), "map" | "collect") {
        return None;
    }
    let block = map.block()?;
    if block.as_block_node().is_some() {
        return Some(map);
    }
    let pass = block.as_block_argument_node()?;
    (map.arguments().is_none() && pass.expression().is_some_and(|e| e.as_symbol_node().is_some())).then_some(map)
}

impl Cop for MapToSet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(map) = map_then(&call, "to_set") else { return };
        let (Some(dot), Some(sel), Some(msel)) = (call.call_operator_loc(), call.message_loc(), map.message_loc()) else {
            return;
        };
        let msg = format!("Pass a block to `to_set` instead of calling `{}.to_set`.", text(&msel));
        let src = cx.src.text;
        let opts = Surrounding { side: Side::Left, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(src, dot.start_offset(), sel.end_offset(), opts);
        let (ms, me) = (msel.start_offset(), msel.end_offset());
        cx.add_fix(ms, me, msg, |c| {
            c.remove(rs, re);
            c.replace(ms, me, "to_set");
        });
    }
}
