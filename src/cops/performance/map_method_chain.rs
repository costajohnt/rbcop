use crate::prelude::*;

pub const NAME: &str = "Performance/MapMethodChain";
pub static COP: MapMethodChain = MapMethodChain;
pub struct MapMethodChain;

/// `(:block_pass (:sym $_))` on a call's first argument.
fn symbol_block_pass<'pr>(call: &ruby_prism::CallNode<'pr>, target: f64) -> Option<String> {
    if !call_args(call).is_empty() {
        return None;
    }
    let pass = call.block()?.as_block_argument_node()?;
    let sym = pass.expression()?;
    if is_parser_dsym(&sym, target) {
        return None;
    }
    Some(String::from_utf8_lossy(sym.as_symbol_node()?.unescaped()).into_owned())
}

fn map_or_collect(call: &ruby_prism::CallNode) -> bool {
    matches!(method_name(call), "map" | "collect")
}

impl Cop for MapMethodChain {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !map_or_collect(&call) {
            return;
        }
        let (start, end) = send_range(&call);
        // part_of_ignored_node?
        let ignored: &mut Vec<(usize, usize)> = cx.state();
        if ignored.iter().any(|&(s, e)| s <= start && end <= e) {
            return;
        }
        let target = cx.config.target_ruby;
        let Some(arg) = symbol_block_pass(&call, target) else { return };
        let mut args = vec![arg];
        // find_begin_of_chained_map_method
        let mut current = call;
        let begin = loop {
            let Some(chained) = current.receiver().and_then(|r| r.as_call_node()) else { return };
            if !map_or_collect(&chained) {
                return;
            }
            let Some(a) = symbol_block_pass(&chained, target) else { return };
            args.insert(0, a);
            match chained.receiver().and_then(|r| r.as_call_node()) {
                Some(r) if symbol_block_pass(&r, target).is_some() => current = chained,
                _ => break chained,
            }
        };
        let msg = format!(
            "Use `{0} {{ |x| x.{1} }}` instead of `{0}` method chain.",
            method_name(&begin),
            args.join(".")
        );
        cx.add(begin.message_loc().unwrap().start_offset(), end, msg);
        cx.state::<Vec<(usize, usize)>>().push((start, end));
    }
}
