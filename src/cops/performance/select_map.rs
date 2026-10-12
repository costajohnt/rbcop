use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;

pub const NAME: &str = "Performance/SelectMap";
pub static COP: SelectMap = SelectMap;
pub struct SelectMap;

impl Cop for SelectMap {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if name != "select" && name != "filter" {
            return;
        }
        if !call_args(&call).is_empty() {
            return;
        }
        let anc = cx.ancestors;
        let n = anc.len();
        // map_method_candidate: a parent call, or the call above a `block` parent.
        let candidate = match call.block().and_then(|b| b.as_block_node()) {
            // Its own block is the send's parent: only a plain `block` leads further.
            Some(b) if !plain(&b) => return,
            Some(_) => parser_parent_index(anc, n).filter(|&i| anc[i].as_call_node().is_some()),
            None => match parser_parent_index(anc, n) {
                Some(i) if anc[i].as_block_node().is_some() => {
                    if !plain(&anc[i].as_block_node().unwrap()) {
                        return;
                    }
                    parser_parent_index(anc, i).filter(|&j| anc[j].as_call_node().is_some())
                }
                Some(i) if anc[i].as_call_node().is_some() => Some(i),
                _ => None,
            },
        };
        let Some(m) = candidate else { return };
        let map = anc[m].as_call_node().unwrap();
        if method_name(&map) != "map" {
            return;
        }
        // map_method: the map's parent when that is a `block` (its own, or one it is the body of).
        let end = match map.block().and_then(|b| b.as_block_node()) {
            Some(b) if plain(&b) => anc[m].location().end_offset(),
            Some(_) => send_range(&map).1,
            None => match parser_parent_index(anc, m) {
                Some(p) if anc[p].as_block_node().is_some_and(|b| plain(&b)) => anc[p].location().end_offset(),
                _ => send_range(&map).1,
            },
        };
        let start = call.message_loc().unwrap().start_offset();
        cx.add(start, end, format!("Use `filter_map` instead of `{name}.map`."));
    }
}

/// A `block`, not a numblock or itblock.
fn plain(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}
