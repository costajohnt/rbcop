use crate::cops::style_map_to_set::map_then;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/MapToHash";
pub static COP: MapToHash = MapToHash;
pub struct MapToHash;

impl Cop for MapToHash {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(map) = map_then(&call, "to_h") else { return };
        let (Some(dot), Some(sel), Some(msel)) = (call.call_operator_loc(), call.message_loc(), map.message_loc()) else {
            return;
        };
        let dot_src = text(&dot);
        let msg = format!("Pass a block to `to_h` instead of calling `{}{dot_src}to_h`.", text(&msel));
        let src = cx.src.text;
        let opts = Surrounding { side: Side::Left, ..Default::default() };
        let (rs, re) = range_with_surrounding_space(src, dot.start_offset(), sel.end_offset(), opts);
        let (ms, me) = (msel.start_offset(), msel.end_offset());
        let map_dot = map.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()));
        // `(args $(mlhs (arg _)+))` on a plain block (not numblock/itblock).
        let mlhs = map
            .block()
            .and_then(|b| b.as_block_node())
            .and_then(|b| b.parameters())
            .and_then(|p| p.as_block_parameters_node())
            .and_then(|bp| {
                let args = helpers::block_args::children(&bp);
                let [only] = args.as_slice() else { return None };
                let inner = helpers::block_args::mlhs_children(only);
                let plain = !inner.is_empty() && inner.iter().all(|l| l.as_required_parameter_node().is_some());
                plain.then(|| (only.location().start_offset(), only.location().end_offset()))
            });
        cx.add_fix(ms, me, msg, |c| {
            c.remove(rs, re);
            if let Some((ds, de)) = map_dot {
                c.replace(ds, de, dot_src);
            }
            c.replace(ms, me, "to_h");
            if let Some((s, e)) = mlhs {
                c.replace(s, e, &src[s + 1..e - 1]);
            }
        });
    }
}
