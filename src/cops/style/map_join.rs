use crate::prelude::*;
use ruby_prism::BlockNode;

pub const NAME: &str = "Style/MapJoin";
pub static COP: MapJoin = MapJoin;
pub struct MapJoin;

/// The block body is `x.to_s` on the block's only argument (`|x|`, `_1`, `it`).
fn block_to_s(block: &BlockNode) -> bool {
    let Some(stmt) = block.body().and_then(|b| b.as_statements_node()).and_then(|s| {
        let v: Vec<Node> = s.body().iter().collect();
        (v.len() == 1).then(|| v.into_iter().next().unwrap())
    }) else {
        return false;
    };
    let Some(to_s) = stmt.as_call_node() else { return false };
    if method_name(&to_s) != "to_s" || is_csend(&to_s) || to_s.arguments().is_some() || to_s.block().is_some() {
        return false;
    }
    let Some(recv) = to_s.receiver() else { return false };
    let Some(params) = block.parameters() else { return false };
    if let Some(n) = params.as_numbered_parameters_node() {
        return n.maximum() == 1 && recv.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1");
    }
    if params.as_it_parameters_node().is_some() {
        return recv.as_it_local_variable_read_node().is_some();
    }
    let Some(bp) = params.as_block_parameters_node() else { return false };
    let args = helpers::block_args::children(&bp);
    let [only] = args.as_slice() else { return false };
    let (Some(arg), Some(lvar)) = (only.as_required_parameter_node(), recv.as_local_variable_read_node()) else {
        return false;
    };
    arg.name().as_slice() == lvar.name().as_slice()
}

impl Cop for MapJoin {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let join = node.as_call_node().unwrap();
        if method_name(&join) != "join" {
            return;
        }
        let Some(map) = join.receiver().and_then(|r| r.as_call_node()) else { return };
        let name = method_name(&map);
        if !matches!(name, "map" | "collect") {
            return;
        }
        let ok = match map.block() {
            Some(b) if b.as_block_node().is_some() => {
                map.arguments().is_none() && block_to_s(&b.as_block_node().unwrap())
            }
            Some(b) => {
                map.arguments().is_none()
                    && b.as_block_argument_node()
                        .and_then(|p| p.expression())
                        .and_then(|e| e.as_symbol_node())
                        .is_some_and(|s| s.unescaped() == b"to_s")
            }
            None => false,
        };
        if !ok {
            return;
        }
        let Some(sel) = map.message_loc() else { return };
        let map_end = map.location().end_offset();
        let range = match map.receiver() {
            Some(recv) => {
                let Some(dot) = map.call_operator_loc() else { return };
                let rend = recv.location().end_offset();
                let start = if cx.src.line_of(rend) < cx.src.line_of(dot.start_offset()) { rend } else { dot.start_offset() };
                (start, map_end)
            }
            None => {
                let Some(dot) = join.call_operator_loc() else { return };
                (map.location().start_offset(), dot.end_offset())
            }
        };
        let msg = format!("Remove redundant `{name}(&:to_s)` before `join`.");
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(range.0, range.1, ""));
    }
}
