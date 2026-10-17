use crate::prelude::*;

pub const NAME: &str = "Performance/MapCompact";
pub static COP: MapCompact = MapCompact;
pub struct MapCompact;

impl Cop for MapCompact {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let compact = node.as_call_node().unwrap();
        if method_name(&compact) != "compact" || !send_args(&compact).is_empty() {
            return;
        }
        let Some(map) = compact.receiver().and_then(|r| r.as_call_node()) else { return };
        if !matches!(method_name(&map), "map" | "collect") || !call_args(&map).is_empty() {
            return;
        }
        let ok = match map.block() {
            // (call _ {:map :collect} (block_pass (sym _)))
            Some(b) if b.as_block_argument_node().is_some() => b
                .as_block_argument_node()
                .unwrap()
                .expression()
                .is_some_and(|e| e.as_symbol_node().is_some() && !is_parser_dsym(&e, cx.config.target_ruby)),
            // (block $(call _ {:map :collect}) (args ...) _): a plain block.
            Some(b) => !b.as_block_node().unwrap().parameters().is_some_and(|p| {
                p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()
            }),
            None => false,
        };
        if !ok {
            return;
        }
        let src = cx.src;
        let map_sel = map.message_loc().unwrap();
        let sel = compact.message_loc().unwrap();
        let dot = compact.call_operator_loc();
        // remove_compact_method: the compact line goes whole when a dotted call continues the
        // chain on a later line; otherwise the dot and the selector.
        let (cs, ce) = send_range(&compact);
        let multiline = src.line_of(cs) != src.line_of(ce);
        // node.parent: its own block when it has one (which has no selector).
        let own_block = compact.block().is_some();
        let chained = helpers::parser_parent::parser_parent(cx.ancestors)
            .and_then(|p| p.as_call_node())
            .filter(|c| !own_block && c.message_loc().is_some() && c.call_operator_loc().is_some_and(|d| d.as_slice() == b"."));
        let whole_line = multiline
            && chained.as_ref().is_some_and(|c| {
                src.line_of(sel.start_offset()) != src.line_of(map_sel.start_offset())
                    && src.line_of(sel.start_offset()) != src.line_of(send_range(c).1)
            });
        let whole = helpers::whole_lines::range_by_whole_lines(src.text, sel.start_offset(), sel.end_offset(), true);
        cx.add_fix(map_sel.start_offset(), sel.end_offset(), "Use `filter_map` instead.", |c| {
            c.replace(map_sel.start_offset(), map_sel.end_offset(), "filter_map");
            if whole_line {
                c.remove(whole.0, whole.1);
            } else {
                if let Some(d) = dot {
                    c.remove(d.start_offset(), d.end_offset());
                }
                c.remove(sel.start_offset(), sel.end_offset());
            }
        });
    }
}
